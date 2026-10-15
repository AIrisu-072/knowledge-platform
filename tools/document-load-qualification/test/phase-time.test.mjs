import test from 'node:test';
import assert from 'node:assert/strict';
const module = await import('../phase-time.mjs').catch(() => ({}));
const { createPhaseTimer, validatePhaseTime, projectElapsedTime } = module;
const phases = (overrides = {}) => ({schemaVersion:1,documentCount:2,totalElapsedMs:1000,segments:[
  {name:'restart-process-replacement',kind:'fixed',startMs:0,endMs:800},
  {name:'positive-mutations',kind:'serial',startMs:800,endMs:900},
  {name:'whole-list',kind:'scaled',startMs:900,endMs:950},
  {name:'tail',kind:'unclassified',startMs:950,endMs:1000},
],...overrides});
const report = () => ({documentCount:2,metrics:{totalElapsedMs:1000},phaseTime:phases()});
test('phase model API exists', () => {for(const fn of [createPhaseTimer,validatePhaseTime,projectElapsedTime])assert.equal(typeof fn,'function');});
test('fixed duration gets one safety allocation while all other work scales',()=>{
  assert.equal(projectElapsedTime(report(),1000,2).totalElapsedMs,201600);
  const legacy=report();delete legacy.phaseTime;assert.equal(projectElapsedTime(legacy,1000,2).totalElapsedMs,1000000);
});
test('phase evidence refuses malformed, unassigned, overlapping and unbounded intervals',()=>{
  const bad=[null,{},phases({documentCount:3}),phases({totalElapsedMs:999}),phases({segments:[]}),phases({segments:Array(65).fill({name:'x',kind:'scaled',startMs:0,endMs:0})})];
  for(const [field,value] of [['startMs',1],['endMs',NaN],['endMs',-1],['endMs',Infinity],['kind','unknown'],['name','unknown-fixed']]){
    const p=phases();p.segments[0][field]=value;bad.push(p);
  }
  for(const start of [799,801]){const p=phases();p.segments[1].startMs=start;bad.push(p);}
  const duplicate=phases();duplicate.segments[1].name=duplicate.segments[0].name;bad.push(duplicate);
  for(const p of bad){assert.equal(validatePhaseTime(p,1000,2).valid,false);assert.throws(()=>projectElapsedTime({...report(),phaseTime:p},1000,2));}
});
test('recorder measures continuous monotonic intervals including unclassified gaps',()=>{
  let now=10;const timer=createPhaseTimer({now:()=>now,startedAt:10,documentCount:2});
  now=20;timer.transition('restart-process-replacement','fixed');now=40;timer.transition('positive-mutations','serial');now=45;
  const p=timer.finish(35);assert.equal(validatePhaseTime(p,35,2).valid,true);assert.deepEqual(p.segments.map(s=>[s.kind,s.startMs,s.endMs]),[['unclassified',0,10],['fixed',10,30],['serial',30,35]]);
  assert.throws(()=>timer.finish(35));assert.throws(()=>timer.transition('tail','scaled'));
});
test('recorder refuses backwards clock and too many transitions',()=>{
  let now=10;const timer=createPhaseTimer({now:()=>now,startedAt:10,documentCount:2});now=9;assert.throws(()=>timer.transition('x','scaled'));
  const bounded=createPhaseTimer({now:()=>10,startedAt:10,documentCount:2});for(let i=0;i<63;i++)bounded.transition(`s${i}`,'scaled');assert.throws(()=>bounded.transition('overflow','scaled'));
});
test('projection refuses invalid count, factor, unsafe values and does not shrink lower target',()=>{
  for(const count of [0,NaN,'1000',1.5])assert.throws(()=>projectElapsedTime(report(),count,2));
  for(const factor of [0,NaN,Infinity,'2'])assert.throws(()=>projectElapsedTime(report(),1000,factor));
  assert.equal(projectElapsedTime(report(),1,2).totalElapsedMs,2000);
  const huge=report();huge.metrics.totalElapsedMs=Number.MAX_SAFE_INTEGER;delete huge.phaseTime;assert.throws(()=>projectElapsedTime(huge,1000,2));
});

test('recorder does not coerce invalid monotonic clock samples',()=>{
  for(const sample of [null,'10',NaN,Infinity,undefined]){
    const timer=createPhaseTimer({now:()=>sample,startedAt:0,documentCount:2});
    assert.throws(()=>timer.transition('work','scaled'));
  }
});
