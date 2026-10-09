import test from 'node:test';import assert from 'node:assert/strict';
const{createLoadRestarter}=await import('../restart.mjs').catch(()=>({}));
test('two qualification restarts replace owned processes using unique log generations',async()=>{
 let pair={human:{id:3},agent:{id:2}};const events=[];
 const restart=createLoadRestarter({current:()=>pair,stop:async p=>events.push(['stop',p.id]),start:async(profile,generation)=>{events.push(['start',profile,generation]);return{id:generation};},replace:next=>{pair=next;}});
 await restart();await restart();assert.deepEqual(events,[['stop',3],['stop',2],['start','poc-human',4],['start','poc-agent',3],['stop',4],['stop',3],['start','poc-human',5],['start','poc-agent',4]]);
 await assert.rejects(restart(),/restart limit/);assert.equal(events.length,8);
});
test('failed stop cannot start new server processes or replace their handles',async()=>{
 let started=0,replaced=0;const restart=createLoadRestarter({current:()=>({human:{},agent:{}}),stop:async()=>{throw Error('stop failed');},start:async()=>{started++;},replace:()=>{replaced++;}});
 await assert.rejects(restart(),/stop failed/);assert.equal(started,0);assert.equal(replaced,0);
});
