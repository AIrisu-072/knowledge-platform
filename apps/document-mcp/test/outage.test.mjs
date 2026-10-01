import test from 'node:test';
import assert from 'node:assert/strict';
import {createServer} from 'node:http';
import {mkdtemp,writeFile,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {spawn} from 'node:child_process';
async function fixture(t,live,status=200,principal="poc-agent"){const s=createServer((req,res)=>{res.writeHead(status,{'content-type':'application/json'});res.end(JSON.stringify({principal:{identityProvider:'poc',principalId:principal},invocationKind:'agent'}));});await new Promise(r=>s.listen(0,'127.0.0.1',r));const agent=`http://127.0.0.1:${s.address().port}`;if(!live)await new Promise(r=>s.close(r));t.after(()=>{s.closeAllConnections();s.close();});const dir=await mkdtemp(join(tmpdir(),'mcp-outage-'));t.after(()=>rm(dir,{recursive:true,force:true}));const context=join(dir,'runtime-context.json');await writeFile(context,JSON.stringify({runId:'synthetic-boundary-test',agent}));return context;}
async function run(context){const child=spawn(process.execPath,[new URL('./outage.mjs',import.meta.url).pathname],{env:{...process.env,KP_POC_RUNTIME_CONTEXT:context},stdio:['ignore','pipe','pipe']});let out='',err='';child.stdout.on('data',x=>out+=x);child.stderr.on('data',x=>err+=x);const code=await new Promise(r=>child.on('exit',r));return{code,out,err};}
test('outage assertion passes only for stopped upstream',async t=>{const r=await run(await fixture(t,false));assert.equal(r.code,0,r.err);assert.match(r.out,/PASS/);});
test('outage assertion cannot pass for a live Agent server',async t=>{const r=await run(await fixture(t,true));assert.notEqual(r.code,0);assert.doesNotMatch(r.out,/PASS/);});

test('outage assertion rejects a live API returning503',async t=>{const r=await run(await fixture(t,true,503));assert.notEqual(r.code,0);assert.doesNotMatch(r.out,/PASS/);});
test('outage assertion rejects a live API returning the wrong session',async t=>{const r=await run(await fixture(t,true,200,'poc-human'));assert.notEqual(r.code,0);assert.doesNotMatch(r.out,/PASS/);});
