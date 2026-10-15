import test from 'node:test';
import assert from 'node:assert/strict';
const budget = await import('../local-budget.mjs').catch(() => ({}));
const MINUTE = 60_000, HOUR = 60*MINUTE;
const startedAt = Date.parse('2026-10-09T00:00:00.000Z');
function create(options = {}) {
  assert.equal(typeof budget.createLocalRunBudget, 'function');
  return budget.createLocalRunBudget({startedAt, now:startedAt, ...options});
}

test('local budget fixes an immutable 80-hour run and reserves the last 15 minutes', () => {
  const value = create({now:startedAt+30*MINUTE});
  assert.deepEqual(value, {runStartedAt:new Date(startedAt).toISOString(),
    hardDeadlineAt:new Date(startedAt+80*HOUR).toISOString(),
    workDeadlineAt:new Date(startedAt+80*HOUR-15*MINUTE).toISOString()});
  assert.equal(Object.isFrozen(value), true);
  assert.equal(Date.parse(value.workDeadlineAt)-(startedAt+30*MINUTE), 80*HOUR-45*MINUTE);
});
test('default local start is captured now without a hosted-run environment', () => {
  assert.equal(typeof budget.createLocalRunBudget, 'function');
  const before = Date.now(), value = budget.createLocalRunBudget(), after = Date.now();
  assert.ok(Date.parse(value.runStartedAt)>=before && Date.parse(value.runStartedAt)<=after);
  assert.equal(Date.parse(value.hardDeadlineAt)-Date.parse(value.runStartedAt), 80*HOUR);
});
for (const value of [null, '2026-10-09T00:00:00.000Z', NaN, Infinity, 1.5, Number.MAX_SAFE_INTEGER]) {
  test(`invalid local start fails closed: ${value}`, () => assert.throws(() => create({startedAt:value}), /local-budget-start-invalid/));
  test(`invalid local clock fails closed: ${value}`, () => assert.throws(() => create({now:value}), /local-budget-clock-invalid/));
}
test('a future start cannot extend the fixed run', () => assert.throws(() => create({startedAt:startedAt+1}), /local-budget-start-invalid/));
test('an exhausted preparation window cannot be reset at stage start', () => {
  for (const elapsed of [80*HOUR-15*MINUTE, 80*HOUR]) {
    assert.throws(() => create({now:startedAt+elapsed}), /local-budget-deadline-exhausted/);
  }
  assert.equal(Date.parse(create({now:startedAt+80*HOUR-15*MINUTE-1}).workDeadlineAt), startedAt+80*HOUR-15*MINUTE);
});
test('caller-supplied duration and deadlines cannot expand the fixed local budget', () => {
  const value = create({maxWallTimeMs:1000*HOUR, durationHours:1000, hardDeadlineAt:'2099-01-01T00:00:00.000Z', workDeadlineAt:'2099-01-01T00:00:00.000Z'});
  assert.equal(Date.parse(value.hardDeadlineAt), startedAt+80*HOUR);
  assert.equal(Date.parse(value.workDeadlineAt), startedAt+80*HOUR-15*MINUTE);
});
