import test from 'node:test';
import assert from 'node:assert/strict';
const budget=await import('../local-resource-budget.mjs').catch(()=>({}));
const rss='KP_DOCUMENT_LOAD_LOCAL_MAX_RSS_BYTES',reserve='KP_DOCUMENT_LOAD_LOCAL_MIN_AVAILABLE_MEMORY_BYTES';
test('only an explicit approved local80h4 pair changes the legacy byte budget',()=>{
 assert.equal(typeof budget.parseLocalResourceBudget,'function');
 assert.equal(budget.parseLocalResourceBudget({}),undefined);
 assert.deepEqual(budget.parseLocalResourceBudget({[rss]:'4294967296',[reserve]:'4294967296'}),{maxRssBytes:4294967296,minAvailableMemoryBytes:4294967296});
});
test('missing malformed unsafe and unapproved explicit bytes fail closed',()=>{
 assert.equal(typeof budget.parseLocalResourceBudget,'function');
 for(const value of ['', '4GiB','4', '0', '-1','4294967296.0',' 4294967296','4294967296 ', '04294967296','1e9','9007199254740992',4294967296,null,'5368709120']){
  assert.throws(()=>budget.parseLocalResourceBudget({[rss]:value,[reserve]:'4294967296'}));assert.throws(()=>budget.parseLocalResourceBudget({[rss]:'4294967296',[reserve]:value}));
 }
 assert.throws(()=>budget.parseLocalResourceBudget({[rss]:'4294967296'}));assert.throws(()=>budget.parseLocalResourceBudget({[reserve]:'4294967296'}));
});
