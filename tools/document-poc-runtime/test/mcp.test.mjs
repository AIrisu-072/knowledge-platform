import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {RUNTIME_STAGES} from '../harness.mjs';
import {summarize} from '../ci-summary.mjs';
test('actual MCP acceptance and stopped-server check are mandatory ordered runtime stages',()=>{assert.ok(RUNTIME_STAGES.includes('agent-acceptance'));assert.ok(RUNTIME_STAGES.indexOf('agent-acceptance')>RUNTIME_STAGES.indexOf('browser-journey'));assert.ok(RUNTIME_STAGES.indexOf('agent-acceptance')<RUNTIME_STAGES.indexOf('health-recovery'));assert.ok(RUNTIME_STAGES.indexOf('agent-outage')>RUNTIME_STAGES.indexOf('shutdown'));assert.ok(RUNTIME_STAGES.indexOf('agent-outage')<RUNTIME_STAGES.indexOf('restart'));});
test('bounded summary includes compiled MCP hashes and rejects missing provenance',()=>{const report=qualifiedReport();assert.equal(summarize(report).artifacts.mcp,'b'.repeat(64));assert.equal(summarize(report).artifacts.mcpRuntime,'c'.repeat(64));assert.equal(summarize(report).acceptanceQualified,true);delete report.artifacts.mcp;assert.equal(summarize(report).acceptanceQualified,false);});
test('MCP focused verification is an exact-head required-check predecessor',async()=>{const ci=await readFile(new URL('../../../.github/workflows/ci.yml',import.meta.url),'utf8');assert.match(ci,/  document-mcp:\n/);assert.match(ci,/      - document-mcp\n/);assert.match(ci,/DOCUMENT_MCP: \$\{\{ needs.document-mcp.result \}\}/);assert.match(ci,/"\$DOCUMENT_MCP"/);assert.match(ci,/mise run document:mcp:test/);assert.match(ci,/mise run document:poc:agent/);});
test('failed or missing Agent evidence cannot qualify and exposes only bounded diagnostics',()=>{const report={gitHead:'a'.repeat(40),gitDirty:false,status:'passed',acceptanceQualified:true,stages:RUNTIME_STAGES.map(name=>({name,status:'passed'})),artifacts:{mcp:'b'.repeat(64),mcpRuntime:'c'.repeat(64),mcpConsistency:'e'.repeat(64)},agentAcceptance:{status:'FAIL',phase:'metadata-update',failureCategory:'assertion',message:'secret',checks:[]}};const output=summarize(report);assert.equal(output.acceptanceQualified,false);assert.equal(output.agent.phase,'metadata-update');assert.ok(!JSON.stringify(output).includes('secret'));delete report.agentAcceptance;assert.equal(summarize(report).acceptanceQualified,false);});
function qualifiedReport(){const runId='12345678-1234-4234-8234-123456789abc';
const receipt={runId,sourceHead:'a'.repeat(40),ports:{human:41001,agent:41002,postgres:41003,proxy:41004},databaseIdentitySha256:'f'.repeat(64),storageIdentitySha256:'1'.repeat(64),fixtureHash:'2'.repeat(64)};
return {database:{ownership:'harness-owned'},runtimeProvenance:{initial:receipt,beforeRestart:structuredClone(receipt),afterRestart:structuredClone(receipt)},runId,gitHead:'a'.repeat(40),gitDirty:false,status:'passed',acceptanceQualified:true,stages:RUNTIME_STAGES.map(name=>({name,status:'passed'})),sourceLocks:{pnpm:'d'.repeat(64)},artifacts:{mcp:'b'.repeat(64),mcpRuntime:'c'.repeat(64),mcpConsistency:'e'.repeat(64)},agentAcceptance:{status:'PASS',phase:'complete',checks:[],runId,sourceHead:'a'.repeat(40),mainSha256:'b'.repeat(64),runtimeSha256:'c'.repeat(64),workspaceLockSha256:'d'.repeat(64)}};}
test('Agent summary independently requires matching run/head/executable/lock provenance',()=>{assert.equal(summarize(qualifiedReport()).acceptanceQualified,true);for(const key of ['runId','sourceHead','mainSha256','runtimeSha256','workspaceLockSha256']){const missing=qualifiedReport();delete missing.agentAcceptance[key];assert.equal(summarize(missing).acceptanceQualified,false,`missing ${key}`);const mismatch=qualifiedReport();mismatch.agentAcceptance[key]='private://wrong-value';assert.equal(summarize(mismatch).acceptanceQualified,false,`mismatch ${key}`);assert.ok(!JSON.stringify(summarize(mismatch)).includes('private://'));}const malformed=qualifiedReport();malformed.runId=malformed.agentAcceptance.runId='not-a-run-id';assert.equal(summarize(malformed).acceptanceQualified,false);});

test('owned runtime identity is required in addition to every existing Agent provenance check',()=>{
  const report=qualifiedReport(); assert.equal(summarize(report).acceptanceQualified,true);
  for(const name of ['initial','beforeRestart','afterRestart']) {
    const missing=structuredClone(report); delete missing.runtimeProvenance[name];
    assert.equal(summarize(missing).acceptanceQualified,false);
    const mismatch=structuredClone(report); mismatch.runtimeProvenance[name].ports.agent=41005;
    assert.equal(summarize(mismatch).acceptanceQualified,false);
  }
  const external=qualifiedReport(); external.database.ownership='caller-asserted-disposable'; delete external.runtimeProvenance;
  assert.equal(summarize(external).acceptanceQualified,true);
  assert.equal(summarize(external).runtime.ownership,'unverified-external');
  assert.equal(summarize(external).runtime.restartIdentityVerified,false);
});
