import {execFile} from 'node:child_process';
import {createHash} from 'node:crypto';
import {lstatSync,realpathSync} from 'node:fs';
import {mkdir} from 'node:fs/promises';
import {isAbsolute,join,resolve} from 'node:path';
import {promisify} from 'node:util';
import {postgresArguments} from '../document-poc-runtime/harness.mjs';

const executeFile = promisify(execFile);
const bindings = new WeakMap();
const destination = '/var/lib/postgresql';

/** Only an explicit local canary changes ordinary/small storage; hosted stays tmpfs. */
export function ownedExt4DatabaseEnabled(env) {
  if (env.KP_POC_OWNED_EXT4_DATABASE === undefined) return env.KP_DOCUMENT_LOAD_HUNDRED_THOUSAND === 'true';
  if (env.KP_POC_OWNED_EXT4_DATABASE !== 'true') throw Error('Explicit owned ext4 database opt-in must be true');
  if (env.GITHUB_ACTIONS === 'true') throw Error('Owned ext4 canary cannot change hosted storage');
  if (env.TEST_DATABASE_URL !== undefined || env.KP_POC_DISPOSABLE_DATABASE !== undefined) throw Error('Owned ext4 canary requires its fresh owned database');
  return true;
}

export function assertLocalLinux({platform=process.platform,arch=process.arch}={}) {
  if (platform !== 'linux' || arch !== 'x64') throw Error('Owned local qualification requires Linux x64');
}

/** Refuse aliases as well as a symlink at the leaf. Never fix caller permissions. */
export function ownedPrivateDirectory(path,{owner=true}={}) {
  if (typeof path !== 'string' || !isAbsolute(path) || resolve(path) !== path || /[,\x00-\x1f\x7f]/.test(path)) throw Error('A canonical safe absolute directory is required');
  const stat = lstatSync(path,{bigint:true});
  if (!stat.isDirectory() || stat.isSymbolicLink() || realpathSync(path) !== path) throw Error('Owned directory must be real without symlink ancestors');
  if ((stat.mode & 0o777n) !== 0o700n || (owner && stat.uid !== BigInt(process.getuid()))) throw Error('Owned directory must be private and owned by this user');
  return stat;
}

export async function verifyExt4Directory(path,{execute=executeFile}={}) {
  const {stdout} = await execute('findmnt',['--noheadings','--raw','--output','FSTYPE','--target',path],
    {timeout:5000,killSignal:'SIGKILL',maxBuffer:4096,encoding:'utf8'});
  if (typeof stdout !== 'string' || !/^ext4\n?$/.test(stdout)) throw Error('Owned evidence and database storage require verified ext4');
}

function sameIdentity(actual,expected) {
  if (actual.dev !== expected.dev || actual.ino !== expected.ino) throw Error('Owned database directory identity changed');
}

function verifiedBinding(binding) {
  const record = bindings.get(binding);
  if (!record) throw Error('A verified owned database storage binding is required');
  sameIdentity(ownedPrivateDirectory(record.directory),record.directoryStat);
  // The official PostgreSQL entrypoint chowns this new directory. Its private
  // parent stays user-owned; neither the inode nor private mode may change.
  sameIdentity(ownedPrivateDirectory(record.source,{owner:false}),record.sourceStat);
  return record;
}

/** Creates exactly one new private database directory. Evidence is never deleted. */
export async function prepareOwnedDatabaseStorage(directory,dependencies={}) {
  assertLocalLinux(dependencies);
  const directoryStat = ownedPrivateDirectory(directory);
  await verifyExt4Directory(directory,dependencies);
  sameIdentity(ownedPrivateDirectory(directory),directoryStat);
  const source = join(directory,'postgres-data');
  await mkdir(source,{mode:0o700});
  const sourceStat = ownedPrivateDirectory(source);
  await verifyExt4Directory(source,dependencies);
  sameIdentity(ownedPrivateDirectory(directory),directoryStat);
  sameIdentity(ownedPrivateDirectory(source),sourceStat);
  const identity = JSON.stringify({version:1,directory,source,filesystem:'ext4',
    directoryDevice:String(directoryStat.dev),directoryInode:String(directoryStat.ino),
    sourceDevice:String(sourceStat.dev),sourceInode:String(sourceStat.ino)});
  const binding = Object.freeze({databaseStorageMode:'owned-ext4',
    databaseStorageIdentitySha256:createHash('sha256').update(identity).digest('hex')});
  bindings.set(binding,{directory,source,directoryStat,sourceStat,dependencies});
  return binding;
}

/** Undefined preserves the existing hosted tmpfs arguments byte for byte. */
export function ownedPostgresArguments(runId,cidfile,binding) {
  const args = postgresArguments(runId,cidfile);
  if (binding === undefined) return args;
  const record = verifiedBinding(binding);
  if (cidfile !== join(record.directory,'postgres.cid')) throw Error('Owned database cidfile must belong to the same run directory');
  const index = args.indexOf('--tmpfs');
  if (index < 0 || args[index+1] !== `${destination}:rw` || args.indexOf('--tmpfs',index+1) !== -1) throw Error('Default PostgreSQL mount contract changed');
  args.splice(index,2,'--mount',`type=bind,src=${record.source},dst=${destination}`);
  // PG18's default nested PGDATA leaves this private mount root user-owned.
  // Make the fresh disposable mount the data directory so the official
  // entrypoint chowns that exact directory before dropping to postgres.
  args.splice(args.length-1,0,'--env',`PGDATA=${destination}`);
  return args;
}

/** The caller supplies its existing bounded command runner and owned container ID. */
export async function verifyOwnedDatabaseMounts(cid,binding,run) {
  if (typeof cid !== 'string' || !/^[a-f0-9]{64}$/.test(cid)) throw Error('Owned container ID is invalid');
  if (typeof run !== 'function') throw Error('A bounded Docker inspection runner is required');
  const record = verifiedBinding(binding);
  await verifyExt4Directory(record.source,record.dependencies);
  const raw = await run('postgres-owned-mounts','docker',['inspect','--format','{{json .Mounts}}',cid],process.env,10000);
  let mounts;
  try { mounts = JSON.parse(raw); } catch { throw Error('Owned database mount inspection is invalid'); }
  if (!Array.isArray(mounts) || mounts.length !== 1 || mounts[0]?.Type !== 'bind'
    || mounts[0].Source !== record.source || mounts[0].Destination !== destination || mounts[0].RW !== true) {
    throw Error('Owned database mount does not match the verified read-write bind');
  }
  verifiedBinding(binding);
  return {...binding};
}
