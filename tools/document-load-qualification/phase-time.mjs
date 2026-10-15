// Fixed work is an explicit reviewed allowlist, never an arbitrary caller label.
const FIXED_PHASES = new Set(['restart-process-replacement']);
const MAX_SEGMENTS = 64;
const number = value => typeof value === 'number' && Number.isFinite(value) && value >= 0 && value <= Number.MAX_SAFE_INTEGER;
const count = value => Number.isSafeInteger(value) && value > 0;
const record = value => value !== null && typeof value === 'object' && !Array.isArray(value);
const name = value => typeof value === 'string' && /^[a-z][a-z0-9-]{0,79}$/.test(value);
const kinds = new Set(['fixed', 'serial', 'scaled', 'unclassified']);

/** Exact contiguous coverage prevents missing/overlapping time from reducing admission. */
export function validatePhaseTime(value, totalElapsedMs, documentCount) {
  const errors = [];
  if (!record(value)) return {valid:false,errors:['phaseTime must be an object']};
  if (value.schemaVersion !== 1) errors.push('phaseTime schemaVersion must be 1');
  if (!count(documentCount) || value.documentCount !== documentCount) errors.push('phaseTime documentCount differs');
  if (!number(totalElapsedMs) || totalElapsedMs <= 0 || value.totalElapsedMs !== totalElapsedMs) errors.push('phaseTime total elapsed differs');
  if (!Array.isArray(value.segments) || value.segments.length === 0 || value.segments.length > MAX_SEGMENTS) errors.push('phaseTime requires 1..64 segments');
  else {
    let cursor = 0;
    const names = new Set();
    for (const segment of value.segments) {
      if (!record(segment)) { errors.push('phaseTime segment must be an object'); continue; }
      if (!name(segment.name) || names.has(segment.name)) errors.push('phaseTime segment name is invalid or duplicated');
      names.add(segment.name);
      if (!kinds.has(segment.kind) || (segment.kind === 'fixed' && !FIXED_PHASES.has(segment.name))) errors.push('phaseTime kind or fixed classification is invalid');
      if (!number(segment.startMs) || !number(segment.endMs) || segment.startMs !== cursor || segment.endMs < segment.startMs) errors.push('phaseTime has invalid, overlapping or unassigned time');
      cursor = segment.endMs;
    }
    if (cursor !== totalElapsedMs) errors.push('phaseTime has unassigned final time');
  }
  return {valid:errors.length === 0,errors};
}

/** Bounded monotonic segmentation; awaited probes and gaps remain in elapsed time. */
export function createPhaseTimer({now = () => performance.now(), startedAt, documentCount}) {
  if (typeof now !== 'function' || !number(startedAt) || !count(documentCount)) throw new TypeError('invalid phase timer origin/count');
  const segments = [];
  let current = {name:'initial-unclassified',kind:'unclassified',startMs:0};
  let finished = false;
  function close(endMs) {
    if (!number(endMs) || endMs < current.startMs) throw new Error('phase clock moved backwards or is invalid');
    segments.push({...current,endMs});
  }
  return {
    transition(nextName, kind) {
      if (finished) throw new Error('phase timer already finished');
      if (!name(nextName) || !kinds.has(kind) || (kind === 'fixed' && !FIXED_PHASES.has(nextName))) throw new Error('invalid phase name/kind');
      if (nextName === current.name || segments.some(s => s.name === nextName)) throw new Error('duplicate phase name');
      if (segments.length >= MAX_SEGMENTS - 1) throw new Error('phase segment bound exceeded');
      const sampledAt = now();
      if (!number(sampledAt)) throw new Error('phase clock sample is invalid');
      const endMs = sampledAt - startedAt;
      close(endMs);
      current = {name:nextName,kind,startMs:endMs};
    },
    finish(totalElapsedMs) {
      if (finished) throw new Error('phase timer already finished');
      close(totalElapsedMs);
      finished = true;
      const value = {schemaVersion:1,documentCount,totalElapsedMs,segments};
      const validation = validatePhaseTime(value,totalElapsedMs,documentCount);
      if (!validation.valid) throw new Error(validation.errors.join('; '));
      return value;
    },
  };
}

/** Legacy absence stays conservative; malformed present evidence never falls back. */
export function projectElapsedTime(report, nextCount, safetyFactor) {
  if (!record(report) || !count(report.documentCount) || !count(nextCount)
    || !number(safetyFactor) || safetyFactor < 1 || !number(report.metrics?.totalElapsedMs) || report.metrics.totalElapsedMs <= 0) throw new TypeError('invalid elapsed projection inputs');
  const total = report.metrics.totalElapsedMs;
  let fixedElapsedMs = 0;
  const measuredPhases = Object.hasOwn(report,'phaseTime');
  if (measuredPhases) {
    const validation = validatePhaseTime(report.phaseTime,total,report.documentCount);
    if (!validation.valid) throw new Error(validation.errors.join('; '));
    fixedElapsedMs = report.phaseTime.segments.filter(s => s.kind === 'fixed').reduce((sum,s) => sum + (s.endMs - s.startMs),0);
  }
  const countRatio = Math.max(1,nextCount / report.documentCount);
  const totalElapsedMs = Math.ceil(safetyFactor * (fixedElapsedMs + (total - fixedElapsedMs) * countRatio));
  if (!Number.isSafeInteger(totalElapsedMs)) throw new Error('projected totalElapsedMs exceeds safe numeric range');
  return {totalElapsedMs,fixedElapsedMs,scaledElapsedMs:total-fixedElapsedMs,countRatio,basis:measuredPhases?'phase-time: fixed once; all remaining elapsed count-scaled':'legacy: all elapsed count-scaled'};
}
