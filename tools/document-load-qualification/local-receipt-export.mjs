import { constants } from 'node:fs';
import { lstat, mkdir, open, realpath } from 'node:fs/promises';
import { isAbsolute, join, resolve } from 'node:path';
import { types } from 'node:util';
import { MAX_LOCAL_SCALE_RECEIPT_BYTES, projectLocalScaleReceipt } from './local-receipt.mjs';

const invalid = () => { throw Error('Local scale receipt export failed'); };
const requireValue = condition => { if (!condition) invalid(); };
function object(value) {
  requireValue(value !== null && typeof value === 'object' && !types.isProxy(value) && !Array.isArray(value)
    && [Object.prototype, null].includes(Object.getPrototypeOf(value)));
}
function read(value, key) {
  object(value);
  const descriptor = Object.getOwnPropertyDescriptor(value, key);
  requireValue(descriptor && Object.hasOwn(descriptor, 'value') && descriptor.enumerable);
  return descriptor.value;
}
function exact(value, keys) {
  object(value);
  const actual = Reflect.ownKeys(value);
  requireValue(actual.length === keys.length && actual.every(key => keys.includes(key)));
  for (const key of keys) read(value, key);
}
function pair(value) {
  requireValue(Array.isArray(value) && !types.isProxy(value) && Object.getPrototypeOf(value) === Array.prototype && value.length === 2);
  const keys = Reflect.ownKeys(value);
  requireValue(keys.length === 3 && keys.every(key => ['0', '1', 'length'].includes(key)));
  return [0, 1].map(index => {
    const descriptor = Object.getOwnPropertyDescriptor(value, String(index));
    requireValue(descriptor && Object.hasOwn(descriptor, 'value') && descriptor.enumerable);
    return descriptor.value;
  });
}
async function privateDirectory(path) {
  const info = await lstat(path);
  requireValue(info.isDirectory() && !info.isSymbolicLink() && typeof process.getuid === 'function'
    && info.uid === process.getuid() && (info.mode & 0o777) === 0o700 && await realpath(path) === path);
}

/**
 * Local-only output inside the caller's owned private runtime directory. Call
 * only after report.finish() confirms acceptanceQualified, and pass actual
 * successful final-shutdown records plus cleanupOwnedRuntime's result. No
 * GitHub upload, private report serialization or admission conversion occurs.
 */
export async function writeLocalScaleReceipt(directory, chain, options) {
  try {
    exact(options, ['sourceHead', 'acceptanceQualified', 'finalShutdown', 'cleanup']);
    const sourceHead = read(options, 'sourceHead'), cleanup = read(options, 'cleanup');
    requireValue(typeof sourceHead === 'string' && /^[a-f0-9]{40}$/.test(sourceHead)
      && read(options, 'acceptanceQualified') === true && read(chain, 'status') === 'SUCCEEDED');
    exact(cleanup, ['failed', 'cleanup']);
    requireValue(read(cleanup, 'failed') === false);
    const records = [...pair(read(options, 'finalShutdown')), ...pair(read(cleanup, 'cleanup'))];
    const envelope = projectLocalScaleReceipt({
      small: read(chain, 'small'), thousand: read(chain, 'thousand'), tenThousand: read(chain, 'tenThousand'),
      hundredThousand: read(chain, 'hundredThousand'), cleanup: { ownedCleanupSucceeded: true, records },
    });
    requireValue(envelope.receipt.fingerprint.code === sourceHead);
    const bytes = Buffer.from(JSON.stringify(envelope) + '\n');
    requireValue(bytes.length <= MAX_LOCAL_SCALE_RECEIPT_BYTES);
    requireValue(typeof directory === 'string' && isAbsolute(directory));
    const root = resolve(directory);
    await privateDirectory(root);
    const destination = join(root, 'local-scale-export');
    // Exclusive creation refuses stale runs, existing files and symlink targets.
    await mkdir(destination, { mode: 0o700 });
    await privateDirectory(destination);
    const file = await open(join(destination, 'qualification.json'), constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL | constants.O_NOFOLLOW, 0o600);
    try {
      const info = await file.stat();
      requireValue(info.isFile() && info.nlink === 1 && info.uid === process.getuid() && (info.mode & 0o777) === 0o600);
      await file.writeFile(bytes);
      await file.sync();
    } finally { await file.close(); }
    return { sha256: envelope.sha256, byteLength: bytes.length };
  } catch { invalid(); }
}
