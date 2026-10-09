import { spawn } from 'node:child_process';
import { createWriteStream } from 'node:fs';
import { StringDecoder } from 'node:string_decoder';
import { finished } from 'node:stream/promises';

const TAIL_BYTES = 1024 * 1024;
const MAX_SECRET_BYTES = 64 * 1024;
const MAX_TOTAL_SECRET_BYTES = 256 * 1024;
const MAX_SECRETS = 128;

function secretPatterns(secrets) {
  if (!Array.isArray(secrets)) throw new TypeError('Log secrets must be an array');
  const values = [...new Set(secrets.filter(Boolean))];
  let totalBytes = 0;
  if (values.length > MAX_SECRETS) throw new RangeError('Log secret count exceeds its bounded limit');
  return values.map(value => {
    if (typeof value !== 'string') throw new TypeError('Log secrets must be strings');
    const bytes = Buffer.byteLength(value);
    totalBytes += bytes;
    if (bytes > MAX_SECRET_BYTES || totalBytes > MAX_TOTAL_SECRET_BYTES) {
      throw new RangeError('Log secret size exceeds its bounded limit');
    }
    // KMP prefix tables let long, repeated prefixes span chunks without rescanning
    // or retaining the preceding log. The table is shared by the three passes.
    const prefix = new Uint32Array(value.length);
    for (let index = 1, matched = 0; index < value.length; index++) {
      while (matched && value[index] !== value[matched]) matched = prefix[matched - 1];
      if (value[index] === value[matched]) matched++;
      prefix[index] = matched;
    }
    return { value, prefix };
  });
}

function redactor(patterns) {
  const states = patterns.map(() => 0);
  let pending = '', protectedPrefix = new Uint8Array(0), inRedaction = false;
  function consume(text, ending = false) {
    const priorLength = pending.length;
    const input = pending + text;
    const coverage = new Int32Array(input.length + 1);
    for (let patternIndex = 0; patternIndex < patterns.length; patternIndex++) {
      const { value, prefix } = patterns[patternIndex];
      let matched = states[patternIndex];
      for (let index = 0; index < text.length; index++) {
        while (matched && text[index] !== value[matched]) matched = prefix[matched - 1];
        if (text[index] === value[matched]) matched++;
        if (matched === value.length) {
          const end = priorLength + index + 1;
          coverage[end - value.length]++;
          coverage[end]--;
          matched = prefix[matched - 1];
        }
      }
      states[patternIndex] = matched;
    }
    // Only a suffix that could still become a secret remains raw in memory.
    // Coverage retained on this suffix also handles overlapping secret matches.
    const boundary = ending ? input.length : input.length - Math.max(0, ...states);
    const nextProtected = new Uint8Array(input.length - boundary);
    const parts = [];
    let active = 0, plainStart = 0;
    for (let index = 0; index < input.length; index++) {
      active += coverage[index];
      const hidden = active > 0 || protectedPrefix[index] === 1;
      if (index >= boundary) { nextProtected[index - boundary] = Number(hidden); continue; }
      if (hidden) {
        if (plainStart < index) parts.push(input.slice(plainStart, index));
        if (!inRedaction) parts.push('[REDACTED]');
        inRedaction = true;
        plainStart = index + 1;
      } else {
        inRedaction = false;
      }
    }
    if (plainStart < boundary) parts.push(input.slice(plainStart, boundary));
    pending = input.slice(boundary);
    protectedPrefix = nextProtected;
    return parts.join('');
  }
  return { write: text => consume(text), end: () => consume('', true) };
}

// Export the actual streaming matcher so chunk-boundary tests do not depend on
// operating-system pipe coalescing. It accepts decoded text, never raw UTF-8.
export function createLogRedactor(secrets) { return redactor(secretPatterns(secrets)); }

export function startBoundedProcess(command, args, { cwd, env, log, secrets = [] }) {
  const patterns = secretPatterns(secrets); // Validate before opening files or spawning.
  const stream = createWriteStream(log, { flags: 'a', mode: 0o600, highWaterMark: 64 * 1024 });
  const child = spawn(command, args, { cwd, env, stdio: ['ignore', 'pipe', 'pipe'] });
  let tail = Buffer.alloc(0), failure;
  const mergedRedactor = redactor(patterns);
  const closed = new Promise(resolve => child.once('close', (code, signal) => resolve({ code, signal })));
  child.on('error', error => { child.spawnFailure = true; child.spawnError = error; });

  function fail(error) {
    if (failure) return;
    failure = error;
    child.captureFailure = true;
    child.captureError = error;
    // A server without a trustworthy evidence sink cannot continue this run.
    // Stop only this owned child, and wait for its close before rejecting done.
    child.stdout.destroy();
    child.stderr.destroy();
    stream.destroy();
    if (child.exitCode === null && !child.signalCode) child.kill('SIGKILL');
  }
  stream.on('error', fail);
  const streamDone = finished(stream);
  streamDone.catch(fail);

  async function persist(text) {
    if (failure) throw failure;
    if (!text) return;
    const bytes = Buffer.from(text);
    // Each of the two pipe readers waits for its disk callback before reading
    // again. There is no growing promise queue or unbounded pending write list.
    await new Promise((resolve, reject) => {
      stream.write(bytes, error => error ? reject(error) : resolve());
    });
    const joined = Buffer.concat([tail, bytes]);
    let begin = Math.max(0, joined.length - TAIL_BYTES);
    while (begin < joined.length && (joined[begin] & 0xc0) === 0x80) begin++;
    tail = Buffer.from(joined.subarray(begin));
  }
  async function consume(pipe) {
    const decoder = new StringDecoder('utf8');
    const pipeRedactor = redactor(patterns);
    try {
      for await (const bytes of pipe) {
        await persist(mergedRedactor.write(pipeRedactor.write(decoder.write(bytes))));
      }
      await persist(mergedRedactor.write(pipeRedactor.write(decoder.end()) + pipeRedactor.end()));
    } catch (error) { fail(error); throw error; }
  }

  const readers = [consume(child.stdout), consume(child.stderr)];
  const done = (async () => {
    const [, , result] = await Promise.allSettled([...readers, closed]);
    try {
      if (failure) throw failure;
      await persist(mergedRedactor.end());
      stream.end();
      await streamDone;
      return { ...result.value, output: tail.toString('utf8') };
    } catch (error) {
      fail(error);
      await streamDone.catch(() => {});
      throw failure;
    }
  })();
  // Readiness can observe a failed process before the caller starts awaiting done.
  done.catch(() => {});
  return { child, done, log, output: () => tail.toString('utf8') };
}
