import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const read = name => readFileSync(new URL(name, import.meta.url), 'utf8');

test('two concrete archetype entries preserve exactly three primary navigation items', () => {
  for (const [file, archetype] of [['sales.html', 'context'], ['office.html', 'queue']]) {
    const source = read(file);
    assert.match(source, new RegExp(`data-archetype="${archetype}"`));
    assert.deepEqual([...source.matchAll(/data-primary="([^"]+)"/g)].map(m => m[1]), ['tasks', 'documents', 'search']);
    assert.match(source, /data-primary="tasks"[^>]*aria-current="page"/);
    for (const id of ['task-collection', 'work-surface', 'context-surface', 'action-surface']) assert.ok(source.includes(`id="${id}"`));
  }
});

test('all ten requested scenarios retain the two-archetype model', () => {
  const data = JSON.parse(read('scenarios.json'));
  assert.deepEqual(data.archetypes, ['context', 'queue']);
  assert.deepEqual(data.scenarios.map(s => s.id), ['normal', 'newly_assigned', 'returned', 'working_draft', 'handed_off', 'due_soon', 'blocked', 'agent_active', 'evidence_review', 'document_compare']);
  for (const state of data.scenarios) {
    assert.ok(state.label && state.work && state.attention);
    assert.ok(['evidence', 'agent', 'document', 'search', 'return', 'history', 'resources'].includes(state.module));
  }
});

test('prototype is offline source design, not hidden backend or native security proof', () => {
  const js = read('prototype.js');
  for (const forbidden of [/\bfetch\s*\(/, /XMLHttpRequest/, /WebSocket/, /localStorage/, /sessionStorage/, /eval\s*\(/, /window\.__TAURI/]) assert.doesNotMatch(js, forbidden);
  assert.match(js, /設計デモ/);
  for (const name of ['EvidenceRecord', 'Finding', 'HumanDecision', 'AgentExecution', 'InputResourceRef', 'work_item_private', 'handoff']) assert.ok(js.includes(name));
  assert.match(js, /showModal\(\)/);
  assert.match(js, /returnFocus/);
});

test('source keeps explicit focus, motion and bounded layout rules', () => {
  const css = read('prototype.css');
  assert.match(css, /prefers-reduced-motion/);
  assert.match(css, /:focus-visible/);
  assert.match(css, /minmax\(0,\s*1fr\)/);
  assert.match(css, /--motion-spatial:\s*180ms/);
});
