import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp,mkdir,lstat,chmod,rename,symlink,rm} from 'node:fs/promises';
import {join} from 'node:path';
import {tmpdir} from 'node:os';
import {postgresArguments} from '../../document-poc-runtime/harness.mjs';

const storage = await import('../local-storage.mjs').catch(() => ({}));
const runId = '631081c1-bc45-4475-9f92-d87912342d43';
const cid = 'a'.repeat(64);
const dependencies = {platform:'linux',arch:'x64',execute:async () => ({stdout:'ext4\n'})};
async function fixture(t) {
  const root = await mkdtemp(join(tmpdir(),'owned-db-'));
  t.after(() => rm(root,{recursive:true,force:true}));
  const directory = join(root,'run'); await mkdir(directory,{mode:0o700});
  return {root,directory};
}
function mount(source,overrides={}) { return {Type:'bind',Source:source,Destination:'/var/lib/postgresql',RW:true,...overrides}; }

test('owned ext4 canary is explicit and refuses external databases or hosted opt-in',()=>{
  assert.equal(typeof storage.ownedExt4DatabaseEnabled,'function');
  assert.equal(storage.ownedExt4DatabaseEnabled({}),false);
  assert.equal(storage.ownedExt4DatabaseEnabled({KP_DOCUMENT_LOAD_HUNDRED_THOUSAND:'true'}),true);
  assert.equal(storage.ownedExt4DatabaseEnabled({KP_DOCUMENT_LOAD_SMALL:'true'}),false);
  assert.equal(storage.ownedExt4DatabaseEnabled({KP_POC_OWNED_EXT4_DATABASE:'true'}),true);
  assert.equal(storage.ownedExt4DatabaseEnabled({KP_POC_OWNED_EXT4_DATABASE:'true',KP_DOCUMENT_LOAD_SMALL:'true'}),true);
  for(const env of [{KP_POC_OWNED_EXT4_DATABASE:'false'},{KP_POC_OWNED_EXT4_DATABASE:''},
    {KP_POC_OWNED_EXT4_DATABASE:'true',GITHUB_ACTIONS:'true'},
    {KP_POC_OWNED_EXT4_DATABASE:'true',TEST_DATABASE_URL:'postgres://external'},
    {KP_POC_OWNED_EXT4_DATABASE:'true',KP_POC_DISPOSABLE_DATABASE:'true'}]) {
    assert.throws(()=>storage.ownedExt4DatabaseEnabled(env),/explicit|owned|hosted/i);
  }
});

test('creates a fresh private ext4 database directory and keeps paths out of public identity',async t => {
  assert.equal(typeof storage.prepareOwnedDatabaseStorage,'function');
  const {directory} = await fixture(t);
  const binding = await storage.prepareOwnedDatabaseStorage(directory,dependencies);
  assert.equal((await lstat(join(directory,'postgres-data'))).mode & 0o777,0o700);
  assert.deepEqual(Object.keys(binding).sort(),['databaseStorageIdentitySha256','databaseStorageMode']);
  assert.equal(binding.databaseStorageMode,'owned-ext4');
  assert.match(binding.databaseStorageIdentitySha256,/^[a-f0-9]{64}$/);
  assert.equal(JSON.stringify(binding).includes(directory),false);
  await assert.rejects(storage.prepareOwnedDatabaseStorage(directory,dependencies),/fresh|exists/i);
});

test('filesystem detection is bounded and rejects other filesystems or ambiguous output',async t => {
  assert.equal(typeof storage.prepareOwnedDatabaseStorage,'function');
  const {directory} = await fixture(t);
  for (const stdout of ['tmpfs\n','overlay\n','ext4\next4\n','']) {
    await assert.rejects(storage.prepareOwnedDatabaseStorage(directory,{...dependencies,execute:async (command,args,options) => {
      assert.equal(command,'findmnt'); assert.deepEqual(args,['--noheadings','--raw','--output','FSTYPE','--target',directory]);
      assert.equal(options.timeout,5000); assert.equal(options.killSignal,'SIGKILL'); assert.ok(options.maxBuffer<=4096);
      return {stdout};
    }}),/ext4/);
  }
  await assert.rejects(lstat(join(directory,'postgres-data')), {code:'ENOENT'});
});

test('refuses non-Linux-x64, relative or unsafe paths, nonprivate directories, and symlink ancestors',async t => {
  assert.equal(typeof storage.prepareOwnedDatabaseStorage,'function');
  const {root,directory} = await fixture(t);
  for (const input of ['', '.', directory+',bad', directory+'\nbad']) await assert.rejects(storage.prepareOwnedDatabaseStorage(input,dependencies));
  for (const override of [{platform:'darwin'},{arch:'arm64'}]) await assert.rejects(storage.prepareOwnedDatabaseStorage(directory,{...dependencies,...override}),/Linux x64/);
  const alias = join(root,'alias'); await symlink(directory,alias); await assert.rejects(storage.prepareOwnedDatabaseStorage(alias,dependencies),/real|symlink/);
  const nested=join(directory,'nested'); await mkdir(nested,{mode:0o700}); await assert.rejects(storage.prepareOwnedDatabaseStorage(join(alias,'nested'),dependencies),/real|symlink/);
  await chmod(directory,0o755); await assert.rejects(storage.prepareOwnedDatabaseStorage(directory,dependencies),/private/);
});

test('postgres wrapper preserves hosted tmpfs defaults and accepts only a verified owned binding',async t => {
  assert.equal(typeof storage.ownedPostgresArguments,'function');
  const {directory} = await fixture(t),cidfile=join(directory,'postgres.cid');
  assert.deepEqual(storage.ownedPostgresArguments(runId,cidfile),postgresArguments(runId,cidfile));
  assert.throws(()=>storage.ownedPostgresArguments(runId,cidfile,{databaseStorageMode:'owned-ext4'}),/verified/);
  const binding=await storage.prepareOwnedDatabaseStorage(directory,dependencies);
  const args=storage.ownedPostgresArguments(runId,cidfile,binding);
  const expected=postgresArguments(runId,cidfile),position=expected.indexOf('--tmpfs');
  expected.splice(position,2,'--mount',`type=bind,src=${join(directory,'postgres-data')},dst=/var/lib/postgresql`);
  expected.splice(expected.length-1,0,'--env','PGDATA=/var/lib/postgresql');
  assert.deepEqual(args,expected);
  assert.throws(()=>storage.ownedPostgresArguments(runId,join(directory,'..','foreign.cid'),binding),/cidfile/);
});

test('actual Docker mount must be the exact single read-write bind and inspection is bounded',async t => {
  assert.equal(typeof storage.verifyOwnedDatabaseMounts,'function');
  const {directory}=await fixture(t),source=join(directory,'postgres-data');
  const binding=await storage.prepareOwnedDatabaseStorage(directory,dependencies);
  const verify=mounts=>storage.verifyOwnedDatabaseMounts(cid,binding,async (name,command,args,env,timeoutMs)=>{
    assert.equal(command,'docker');assert.deepEqual(args,['inspect','--format','{{json .Mounts}}',cid]);assert.equal(timeoutMs,10000);
    return JSON.stringify(mounts);
  });
  assert.deepEqual(await verify([mount(source)]),binding);
  for (const mounts of [[],[mount(source,{Type:'volume'})],[mount('/foreign')],[mount(source,{RW:false})],[mount(source,{Destination:'/var/lib/postgresql/data'})],[mount(source),mount('/other',{Destination:'/tmp'})]]) await assert.rejects(verify(mounts),/mount/i);
  await assert.rejects(storage.verifyOwnedDatabaseMounts('bad',binding,()=>{throw Error('must not run');}),/container/);
});

test('directory replacement after binding fails both argument generation and actual-mount verification',async t => {
  assert.equal(typeof storage.prepareOwnedDatabaseStorage,'function');
  const {directory}=await fixture(t),source=join(directory,'postgres-data');
  const binding=await storage.prepareOwnedDatabaseStorage(directory,dependencies);
  await rename(source,join(directory,'retained-original')); await mkdir(source,{mode:0o700});
  assert.throws(()=>storage.ownedPostgresArguments(runId,join(directory,'postgres.cid'),binding),/identity/);
  await assert.rejects(storage.verifyOwnedDatabaseMounts(cid,binding,async()=>JSON.stringify([mount(source)])),/identity/);
});
