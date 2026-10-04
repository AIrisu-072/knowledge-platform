import test from 'node:test';
import assert from 'node:assert/strict';
import {summarizeAgent} from '../agent-summary.mjs';
test('Agent diagnostic summary exposes only fixed phase/category and bounded counts',()=>{const input={status:'FAIL',phase:'metadata-update',failureCategory:'assertion',checks:['a','b'],message:'postgres://secret/private',runId:'secret',unknown:'secret'};assert.deepEqual(summarizeAgent(input),{status:'failed',phase:'metadata-update',failureCategory:'assertion',completedChecks:2});assert.ok(!JSON.stringify(summarizeAgent(input)).includes('secret'));});
test('unknown Agent diagnostic strings cannot leak or qualify',()=>{assert.deepEqual(summarizeAgent({status:'PASS',phase:'postgres://secret',failureCategory:'secret',checks:Array(1000).fill('secret')}),{status:'unavailable',phase:'unverified',failureCategory:'unverified',completedChecks:100});assert.equal(summarizeAgent(null).status,'unavailable');assert.equal(summarizeAgent({status:'PASS',phase:'complete',checks:[]}).status,'passed');});
test('comparison checkpoint diagnostics are a fixed allowlist, never response content', () => {
  for (const checkpoint of ['history-mcp', 'history-human', 'revision-mcp', 'version-mcp', 'revision-human', 'version-human', 'comparison-oracle']) {
    assert.equal(summarizeAgent({status:'FAIL',phase:'history-comparisons',failureCategory:'assertion',checkpoint}).checkpoint, checkpoint);
  }
  const result=summarizeAgent({status:'FAIL',phase:'history-comparisons',checkpoint:'postgres://secret/private'});
  assert.equal(result.checkpoint,'unverified');
  assert.ok(!JSON.stringify(result).includes('secret'));
});
