import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtemp, readFile, rm, stat, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { startProcess, command, waitForDrain } from '../harness.mjs';
import { createLogRedactor } from '../bounded-process-log.mjs';

const tailBytes = 1024 * 1024;
const harnessUrl = new URL('../harness.mjs', import.meta.url).href;

async function until(predicate, message) {
  const deadline = Date.now() + 5000;
  while (Date.now() < deadline) {
    if (await predicate()) return;
    await delay(5);
  }
  assert.fail(message);
}

async function withDirectory(action) {
  const directory = await mkdtemp(join(tmpdir(), 'poc-bounded-log-'));
  try { await action(directory); }
  finally { await rm(directory, { recursive: true, force: true }); }
}

test('bounded capture streams the complete redacted multi-MiB log while retaining only a tail', async () => {
  await withDirectory(async directory => {
    const secret = 'synthetic-password-猫-123';
    const database = `postgres://synthetic:${secret}@127.0.0.1:43210/disposable`;
    const line = 'x'.repeat(8192) + ` ${database} ${secret}\n`;
    const count = 512;
    const release = join(directory, 'release');
    const log = join(directory, 'server.log');
    const source = `
      const { existsSync } = require('node:fs');
      const { once } = require('node:events');
      (async () => {
        process.stdout.write('old-start-marker\\n');
        for (let i = 0; i < ${count}; i++) {
          if (!process.stdout.write(${JSON.stringify(line)})) await once(process.stdout, 'drain');
        }
        process.stdout.write('awaiting-release\\n');
        const timer = setInterval(() => {
          if (existsSync(${JSON.stringify(release)})) {
            clearInterval(timer);
            process.stdout.end('graceful drain complete\\n');
          }
        }, 5);
      })();`;
    const owned = startProcess(process.execPath, ['-e', source], {
      cwd: directory, env: process.env, log, secrets: [database, secret], captureMode: 'bounded-tail',
    });
    try {
      await until(() => owned.output().includes('awaiting-release'), 'child did not produce its complete initial log');
      assert.equal(owned.child.exitCode, null);
      assert.ok(Buffer.byteLength(owned.output()) <= tailBytes, 'live output must be bounded');
      assert.ok(!owned.output().includes('old-start-marker'));
      assert.ok(!owned.output().includes(secret));
      assert.ok((await stat(log)).size > 4 * tailBytes, 'full log must reach disk before child exit');
      const during = await readFile(log, 'utf8');
      assert.ok(!during.includes(secret));
      assert.ok(!during.includes(database));
      await writeFile(release, 'continue');
      await waitForDrain(owned, 5000);
      const result = await owned.done;
      assert.equal(result.code, 0);
      assert.ok(Buffer.byteLength(result.output) <= tailBytes);
      assert.equal(result.output, owned.output());
      assert.ok(result.output.endsWith('graceful drain complete\n'));
      const expected = 'old-start-marker\n' + line.replaceAll(database, '[REDACTED]').replaceAll(secret, '[REDACTED]').repeat(count)
        + 'awaiting-release\ngraceful drain complete\n';
      assert.equal(await readFile(log, 'utf8'), expected);
    } finally {
      if (owned.child.exitCode === null) owned.child.kill('SIGKILL');
      await owned.done.catch(() => {});
    }
  });
});

test('bounded capture preserves UTF-8 and redacts secrets split at every byte boundary despite stderr interleaving', async () => {
  await withDirectory(async directory => {
    const secret = 'synthetic-秘密-🔑-password';
    const bytes = Buffer.from(secret);
    const log = join(directory, 'split.log');
    const owned = startProcess(process.execPath, ['-e', `
      const { setTimeout: delay } = require('node:timers/promises');
      const bytes = Buffer.from(${JSON.stringify(secret)});
      (async () => {
        for (let i = 1; i < bytes.length; i++) {
          process.stdout.write('前 ');
          process.stdout.write(bytes.subarray(0, i));
          await delay(2);
          process.stderr.write('別の出力\\n');
          await delay(2);
          process.stdout.write(bytes.subarray(i));
          process.stdout.write(' 後\\n');
          await delay(2);
        }
      })();`], { cwd: directory, env: process.env, log, secrets: [secret], captureMode: 'bounded-tail' });
    const result = await owned.done;
    assert.equal(result.code, 0);
    const saved = await readFile(log, 'utf8');
    assert.ok(!saved.includes(secret));
    assert.ok(!saved.includes('秘密'));
    assert.ok(!saved.includes('password'));
    assert.ok(!saved.includes('\uFFFD'));
    assert.equal(saved.match(/\[REDACTED\]/g)?.length, bytes.length - 1);
    assert.equal(saved.match(/別の出力/g)?.length, bytes.length - 1);
    assert.equal(saved.match(/前 /g)?.length, bytes.length - 1);
    assert.equal(saved.match(/ 後/g)?.length, bytes.length - 1);
    assert.equal(result.output, saved);
  });
});

test('streaming redaction withholds partial secrets and covers every overlapping match at every text split', () => {
  const redactor = createLogRedactor(['synthetic-password']);
  assert.equal(redactor.write('ready\nsynthetic-pass'), 'ready\n');
  assert.equal(redactor.write('word\n'), '[REDACTED]\n');
  assert.equal(redactor.end(), '');
  for (const [text, secrets, expected] of [
    ['prefix ababab suffix', ['abab'], 'prefix [REDACTED] suffix'],
    ['prefix abcdef suffix', ['abcd', 'cdef'], 'prefix [REDACTED] suffix'],
    ['prefix abcdef suffix', ['ab', 'abcdef'], 'prefix [REDACTED] suffix'],
    ['prefix 🔑秘密🔑 suffix', ['🔑秘密🔑'], 'prefix [REDACTED] suffix'],
    ['nothing ended synthe', ['synthetic-password'], 'nothing ended synthe'],
  ]) {
    for (let boundary = 0; boundary <= text.length; boundary++) {
      const matcher = createLogRedactor(secrets);
      assert.equal(matcher.write(text.slice(0, boundary)) + matcher.write(text.slice(boundary)) + matcher.end(), expected);
    }
    const matcher = createLogRedactor(secrets);
    assert.equal([...text].map(character => matcher.write(character)).join('') + matcher.end(), expected);
  }
});

test('bounded capture also redacts a secret formed at the merge of two finished pipes', async () => {
  await withDirectory(async directory => {
    const log = join(directory, 'merged.log');
    const owned = startProcess(process.execPath, ['-e', `
      process.stdout.end('synthetic-cross-');
      setTimeout(() => process.stderr.end('pipe-secret'), 40);
    `], { env: process.env, log, secrets: ['synthetic-cross-pipe-secret'], captureMode: 'bounded-tail' });
    const result = await owned.done;
    assert.equal(result.code, 0);
    assert.equal(result.output, '[REDACTED]');
    assert.equal(await readFile(log, 'utf8'), '[REDACTED]');
  });
});

test('bounded capture rejects oversized secret state before starting a child', async () => {
  await withDirectory(async directory => {
    assert.throws(() => startProcess(process.execPath, ['-e', 'process.exit(0)'], {
      cwd: directory, env: process.env, log: join(directory, 'invalid.log'),
      captureMode: 'bounded-tail', secrets: ['x'.repeat(64 * 1024 + 1)],
    }), /secret.*(?:limit|large|long|65536)/i);
    for (const secrets of [
      ['🔑'.repeat(16385)],
      Array.from({ length: 129 }, (_, index) => `synthetic-${index}`),
      Array.from({ length: 5 }, (_, index) => `${index}`.repeat(64 * 1024)),
    ]) assert.throws(() => createLogRedactor(secrets), /secret.*limit/i);
    assert.doesNotThrow(() => createLogRedactor(['x'.repeat(64 * 1024)]));
  });
});

test('bounded capture reports spawn failure and closes the log without a false success', async () => {
  await withDirectory(async directory => {
    const log = join(directory, 'missing.log');
    const owned = startProcess(join(directory, 'missing-executable'), [], {
      cwd: directory, env: process.env, log, captureMode: 'bounded-tail',
    });
    const result = await owned.done;
    assert.equal(owned.child.spawnFailure, true);
    assert.notEqual(result.code, 0);
    assert.equal(result.output, '');
    assert.equal(await readFile(log, 'utf8'), '');
  });
});

test('bounded capture rejects file errors and stops the owned process instead of fabricating success', async () => {
  await withDirectory(async directory => {
    const result = spawnSync(process.execPath, ['--input-type=module', '-e', `
      import assert from 'node:assert/strict';
      import { startProcess } from ${JSON.stringify(harnessUrl)};
      const owned = startProcess(process.execPath, ['-e', 'setInterval(() => console.log("running"), 5)'], {
        env: process.env, log: ${JSON.stringify(directory)}, captureMode: 'bounded-tail',
      });
      await assert.rejects(owned.done);
      assert.equal(owned.child.captureFailure, true);
      assert.ok(owned.child.exitCode !== null || owned.child.signalCode);
      console.log('capture-failed-closed');
    `], { encoding: 'utf8', timeout: 5000 });
    assert.equal(result.status, 0, result.stderr || result.error?.message);
    assert.equal(result.stdout.trim(), 'capture-failed-closed');
  });
});

test('bounded capture rejects unexpected pipe errors and closes its streams', async () => {
  await withDirectory(async directory => {
    const result = spawnSync(process.execPath, ['--input-type=module', '-e', `
      import assert from 'node:assert/strict';
      import { startProcess } from ${JSON.stringify(harnessUrl)};
      const owned = startProcess(process.execPath, ['-e', 'setInterval(() => console.log("running"), 5)'], {
        env: process.env, log: ${JSON.stringify(join(directory, 'pipe-error.log'))}, captureMode: 'bounded-tail',
      });
      owned.child.stdout.destroy(new Error('synthetic-pipe-error'));
      await assert.rejects(owned.done, /synthetic-pipe-error/);
      assert.equal(owned.child.captureFailure, true);
      assert.ok(owned.child.exitCode !== null || owned.child.signalCode);
      console.log('pipe-failed-closed');
    `], { encoding: 'utf8', timeout: 5000 });
    assert.equal(result.status, 0, result.stderr || result.error?.message);
    assert.equal(result.stdout.trim(), 'pipe-failed-closed');
  });
});

test('bounded capture rejects a pipe closed before its normal end', async () => {
  await withDirectory(async directory => {
    const owned = startProcess(process.execPath, ['-e', 'setInterval(() => console.log("running"), 5)'], {
      env: process.env, log: join(directory, 'premature-close.log'), captureMode: 'bounded-tail',
    });
    owned.child.stdout.destroy();
    await assert.rejects(owned.done, /premature close/i);
    assert.equal(owned.child.captureFailure, true);
    assert.ok(owned.child.exitCode !== null || owned.child.signalCode);
  });
});

test('bounded capture rejects a real filesystem write error', { skip: process.platform !== 'linux' }, async () => {
  const owned = startProcess(process.execPath, ['-e', 'setInterval(() => console.log("running"), 5)'], {
    env: process.env, log: '/dev/full', captureMode: 'bounded-tail',
  });
  await assert.rejects(owned.done, { code: 'ENOSPC' });
  assert.equal(owned.child.captureFailure, true);
  assert.ok(owned.child.exitCode !== null || owned.child.signalCode);
});

test('bounded capture rejects synchronous write failures and closes the real file', async () => {
  await withDirectory(async directory => {
    const result = spawnSync(process.execPath, ['--input-type=module', '-e', `
      import assert from 'node:assert/strict';
      import fs from 'node:fs';
      import { syncBuiltinESMExports } from 'node:module';
      const original = fs.createWriteStream;
      let stream;
      fs.createWriteStream = (...args) => {
        stream = original(...args);
        stream.write = () => { throw new Error('synthetic-write-throw'); };
        return stream;
      };
      syncBuiltinESMExports();
      const { startProcess } = await import(${JSON.stringify(harnessUrl)});
      const owned = startProcess(process.execPath, ['-e', 'setInterval(() => console.log("running"), 5)'], {
        env: process.env, log: ${JSON.stringify(join(directory, 'write-error.log'))}, captureMode: 'bounded-tail',
      });
      await assert.rejects(owned.done, /synthetic-write-throw/);
      assert.equal(owned.child.captureFailure, true);
      assert.equal(stream.closed, true);
      assert.ok(owned.child.exitCode !== null || owned.child.signalCode);
      console.log('write-failed-closed');
    `], { encoding: 'utf8', timeout: 5000 });
    assert.equal(result.status, 0, result.stderr || result.error?.message);
    assert.equal(result.stdout.trim(), 'write-failed-closed');
  });
});

test('slow real disk writes bound pending memory across both busy pipes', async () => {
  await withDirectory(async directory => {
    const log = join(directory, 'backpressure.log');
    const result = spawnSync(process.execPath, ['--input-type=module', '-e', `
      import assert from 'node:assert/strict';
      import fs from 'node:fs';
      import { syncBuiltinESMExports } from 'node:module';
      const original = fs.createWriteStream;
      let queued = 0;
      fs.createWriteStream = (...args) => {
        const stream = original(...args);
        const write = stream._write;
        stream._write = function (bytes, encoding, callback) {
          setTimeout(() => write.call(this, bytes, encoding, callback), 3);
        };
        const enqueue = stream.write;
        stream.write = function (...args) {
          const result = enqueue.apply(this, args);
          queued = Math.max(queued, this.writableLength);
          return result;
        };
        return stream;
      };
      syncBuiltinESMExports();
      const { startProcess } = await import(${JSON.stringify(harnessUrl)});
      const owned = startProcess(process.execPath, ['-e', ${JSON.stringify(`
        const { once } = require('node:events');
        async function emit(pipe, character) {
          for (let i = 0; i < 256; i++) {
            if (!pipe.write(character.repeat(8192))) await once(pipe, 'drain');
          }
        }
        Promise.all([emit(process.stdout, 'x'), emit(process.stderr, 'y')]);
      `)}], { env: process.env, log: ${JSON.stringify(log)}, captureMode: 'bounded-tail' });
      const result = await owned.done;
      assert.equal(result.code, 0);
      assert.ok(queued > 0 && queued <= 256 * 1024, 'pending writes exceeded bounded pipe reads: ' + queued);
      assert.ok(Buffer.byteLength(result.output) <= 1024 * 1024);
      console.log('bounded-backpressure');
    `], { encoding: 'utf8', timeout: 10000 });
    assert.equal(result.status, 0, result.stderr || result.error?.message);
    assert.equal(result.stdout.trim(), 'bounded-backpressure');
    const output = await readFile(log, 'utf8');
    assert.equal(output.length, 4 * tailBytes);
    assert.equal(output.replaceAll('x', '').length, 2 * tailBytes);
    assert.equal(output.replaceAll('y', '').length, 2 * tailBytes);
  });
});

test('default command capture still returns the complete redacted output', async () => {
  await withDirectory(async directory => {
    const log = join(directory, 'command.log');
    const output = await command(process.execPath, ['-e', 'process.stdout.write("z".repeat(2 * 1024 * 1024) + "synthetic-secret")'], {
      cwd: directory, env: process.env, log, secrets: ['synthetic-secret'],
    });
    assert.equal(output, 'z'.repeat(2 * tailBytes) + '[REDACTED]');
    assert.equal(await readFile(log, 'utf8'), output);
  });
});
