// Failure-only, closed backend projection. Never print raw process output/errors.
const PREFIX = 'KP_FINDING_DIAGNOSTIC ';
const MAX_BYTES = 64 * 1024, MAX_LINE_BYTES = 512, MAX_LINES = 128, MAX_EVENTS = 4, MAX_PROCESSES = 6;
const enums = {
  operation: new Set(['finding', 'finding_list']),
  phase: new Set(['load', 'policy', 'acknowledgements', 'agent', 'evidence', 'authority', 'freshness']),
  dependency: new Set(['work', 'agent', 'document', 'none']),
  sql_class: new Set(['none', 'connection', 'acquire_timeout', 'pool_closed', 'database_connection', 'transaction_rollback', 'data_exception', 'constraint', 'resource', 'other_database', 'decode', 'other']),
  failure: new Set(['dependency_unavailable', 'commit_unknown', 'artifact_unavailable', 'forbidden', 'not_found', 'conflict', 'validation', 'integrity']),
};
const keys = [...Object.keys(enums), 'elapsed_ms'];
function project(value) {
  if (!value || typeof value !== 'object' || Array.isArray(value) || Object.keys(value).length !== keys.length || Object.keys(value).some(key => !keys.includes(key))) return null;
  for (const [key, allowed] of Object.entries(enums)) if (!allowed.has(value[key])) return null;
  if (!Number.isInteger(value.elapsed_ms) || value.elapsed_ms < 0 || value.elapsed_ms > 4294967295) return null;
  return Object.fromEntries(keys.map(key => [key, value[key]]));
}
const unavailable = () => ({ correlation: 'none', events: [] });
const overflow = () => ({ correlation: 'overflow', events: [] });
const result = events => ({ correlation: events.length === 0 ? 'none' : events.length === 1 ? 'single' : 'ambiguous', events });
const unavailableFailures = new Set(['dependency_unavailable', 'commit_unknown', 'artifact_unavailable']);
export function findingFailureDiagnostics(raw) {
  if (typeof raw !== 'string') return unavailable();
  // Reject oversized phase output instead of attributing a truncated tail.
  if (raw.length > MAX_BYTES || Buffer.byteLength(raw) > MAX_BYTES) return overflow();
  const complete = raw.lastIndexOf('\n'); if (complete < 0) return unavailable();
  const lines = raw.slice(0, complete).split('\n'); if (lines.length > MAX_LINES) return overflow();
  const events = [];
  for (const line of lines) {
    if (!line.startsWith(PREFIX) || Buffer.byteLength(line) > MAX_LINE_BYTES) continue;
    try {
      const json = line.slice(PREFIX.length);
      // Valid enum values contain no object syntax; reject duplicate/extra JSON keys.
      if (json.match(/"[^"]*"\s*:/gu)?.length !== keys.length) continue;
      const event = project(JSON.parse(json)); if (event && unavailableFailures.has(event.failure)) events.push(event);
    }
    catch { /* Malformed or unrelated output is not diagnostic evidence. */ }
    if (events.length > MAX_EVENTS) return overflow();
  }
  return result(events);
}
function output(owned) {
  try { const raw = owned.output(); return typeof raw === 'string' ? raw : ''; }
  catch { return ''; }
}
export function beginFindingDiagnosticWindow(processes) {
  // Use only owned processes live at phase start, not obsolete restart generations.
  const live = processes.filter(owned => owned.child.exitCode === null && !owned.child.signalCode);
  if (live.length > MAX_PROCESSES) return overflow;
  const windows = live.map(owned => ({ owned, offset: output(owned).length }));
  return () => {
    const events = [];
    for (const { owned, offset } of windows) {
      const observation = findingFailureDiagnostics(output(owned).slice(offset));
      if (observation.correlation === 'overflow') return overflow();
      events.push(...observation.events); if (events.length > MAX_EVENTS) return overflow();
    }
    return result(events);
  };
}
