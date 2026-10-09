import test from 'node:test';
import assert from 'node:assert/strict';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';
import {randomBytes,randomUUID} from 'node:crypto';
import {mkdir,readFile,writeFile,lstat} from 'node:fs/promises';
import {join} from 'node:path';
import {prepareOwnedDatabaseStorage,ownedPostgresArguments,verifyOwnedDatabaseMounts,ownedPrivateDirectory,verifyExt4Directory} from '../local-storage.mjs';
import {postgresReadyArgs,parsePostgresReadyStatus,waitForPostgresTcp,postgresVersionArgs} from '../../document-poc-runtime/postgres-readiness.mjs';

const exec=promisify(execFile);
test('real PG18 rejects the private ancestor and starts when fresh bind root is PGDATA',{
  skip:process.env.KP_DOCUMENT_LOAD_EXT4_CANARY!=='true',timeout:90000,
},async()=>{
  assert.equal(process.platform,'linux');assert.equal(process.arch,'x64');
  const root=process.env.KP_DOCUMENT_LOAD_EXT4_CANARY_DIR;
  ownedPrivateDirectory(root);await verifyExt4Directory(root);
  for(const scenario of ['baseline','fixed','cli-failure']){
    const fixed=scenario!=='baseline',directory=join(root,scenario);await mkdir(directory,{mode:0o700});
    const binding=await prepareOwnedDatabaseStorage(directory),runId=randomUUID(),password=randomBytes(24).toString('hex');
    const env={...process.env,POSTGRES_PASSWORD:password,PGPASSWORD:password};
    const docker=async args=>(await exec('docker',args,{env,timeout:15000,maxBuffer:65536,encoding:'utf8'})).stdout.trim();
    let args=ownedPostgresArguments(runId,join(directory,'postgres.cid'),binding).filter(arg=>arg!=='--rm');
    if(!fixed){const index=args.indexOf('PGDATA=/var/lib/postgresql');assert.ok(index>0);args.splice(index-1,2);}
    let cid;
    try{
      await docker(args);
      if(scenario==='cli-failure')throw Error('simulated-cli-failure-after-owned-creation');
      cid=(await readFile(join(directory,'postgres.cid'),'utf8')).trim();assert.match(cid,/^[a-f0-9]{64}$/);
      await verifyOwnedDatabaseMounts(cid,binding,async(_name,_command,arguments_)=>docker(arguments_));
      const ready=()=>waitForPostgresTcp(async()=>parsePostgresReadyStatus(await docker(postgresReadyArgs(cid))));
      if(!fixed){
        await assert.rejects(ready);
        const captured=await exec('docker',['logs',cid],{env,timeout:10000,maxBuffer:65536,encoding:'utf8'});
        const logs=captured.stdout+captured.stderr;
        await writeFile(join(directory,'postgres.log'),logs,{mode:0o600,flag:'wx'});
        assert.match(logs,/Permission denied/);
        assert.equal(await docker(['inspect','--format','{{.State.ExitCode}}',cid]),'1');
      }else{
        await ready();
        const version=await docker(postgresVersionArgs(cid));assert.match(version,/^18\.6(?: |$)/);
        assert.equal(await docker(['exec',cid,'sh','-c','printf "%s" "$PGDATA"']),'/var/lib/postgresql');
        assert.equal(await docker(['exec',cid,'gosu','postgres','sh','-c','test -x /var/lib/postgresql && printf traversable']),'traversable');
        const source=await lstat(join(directory,'postgres-data'));assert.equal(source.mode&0o777,0o700);
        assert.equal(source.uid,Number(await docker(['exec',cid,'id','-u','postgres'])));
        assert.equal((await lstat(directory)).uid,process.getuid());
        await verifyOwnedDatabaseMounts(cid,binding,async(_name,_command,arguments_)=>docker(arguments_));
        await writeFile(join(directory,'result.json'),JSON.stringify({status:'SUCCEEDED',databaseOnly:true,version,mode:source.mode&0o777,uid:source.uid})+'\n',{mode:0o600,flag:'wx'});
      }
    }catch(error){
      if(scenario!=='cli-failure')throw error;
      assert.equal(error.message,'simulated-cli-failure-after-owned-creation');
    }finally{
      // Docker can create the owned container/cidfile before its CLI times out.
      if(!cid){try{cid=(await readFile(join(directory,'postgres.cid'),'utf8')).trim();}catch(error){if(error.code!=='ENOENT')throw error;}}
      if(cid)assert.match(cid,/^[a-f0-9]{64}$/);
      if(cid){assert.equal(await docker(['inspect','--format','{{index .Config.Labels "kp.document-poc.run"}}',cid]),runId);await docker(['rm','--force',cid]);}
    }
    if(scenario==='cli-failure'){
      assert.ok(cid,'Recover cidfile after a CLI failure');
      await assert.rejects(docker(['inspect','--format','{{.State.Running}}',cid]));
      await writeFile(join(directory,'result.json'),JSON.stringify({status:'SUCCEEDED',simulatedCliFailure:true,ownedCleanupConfirmed:true})+'\n',{mode:0o600,flag:'wx'});
    }
  }
});
