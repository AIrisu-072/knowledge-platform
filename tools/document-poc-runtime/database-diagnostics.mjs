// Fixed operation/category evidence only. Error objects and their causes stay private.
const operations = new Set(['external-validation', 'docker-run', 'cid-read', 'cid-validation', 'port-query', 'port-validation',
  'image-inspect', 'repo-digest-query', 'repo-digest-parse', 'readiness', 'sql-version-query', 'proxy-start']);
const categories = new Set(['ok', 'command-unavailable', 'command-exit', 'command-signal', 'filesystem-unavailable',
  'invalid-container-id', 'invalid-port-binding', 'invalid-json', 'external-database-rejected', 'readiness-exhausted', 'proxy-unavailable', 'operation-failed']);
const statuses = new Set(['running', 'passed', 'failed']);
const defaults = { 'external-validation': 'external-database-rejected', 'cid-read': 'filesystem-unavailable',
  'cid-validation': 'invalid-container-id', 'port-validation': 'invalid-port-binding', 'repo-digest-parse': 'invalid-json',
  readiness: 'readiness-exhausted', 'proxy-start': 'proxy-unavailable' };
const exitCode = value => Number.isInteger(value) && value >= 0 && value <= 255;

function commandFailure(error) {
  const seen = new Set();
  for (let depth = 0; error && typeof error === 'object' && depth < 4 && !seen.has(error); depth++, error = error.cause) {
    seen.add(error);
    const failure = error.commandFailure;
    if (failure && ['command-unavailable', 'command-exit', 'command-signal'].includes(failure.category)) return failure;
  }
}

export function sanitizeDatabaseDiagnostics(value) {
  const steps = (Array.isArray(value?.steps) ? value.steps.slice(0, 100) : [])
    .filter(step => step && operations.has(step.operation) && statuses.has(step.status)).slice(0, 20)
    .map(step => ({ operation: step.operation, status: step.status,
      category: categories.has(step.category) ? step.category : 'operation-failed',
      ...(typeof step.available === 'boolean' ? { available: step.available } : {}),
      ...(typeof step.commandAvailable === 'boolean' ? { commandAvailable: step.commandAvailable } : {}),
      ...(exitCode(step.exitCode) ? { exitCode: step.exitCode } : {}),
    }));
  return { ...(typeof value?.externalDatabaseSupplied === 'boolean' ? { externalDatabaseSupplied: value.externalDatabaseSupplied } : {}), steps };
}

export class DatabaseDiagnostics {
  constructor(report, externalDatabaseSupplied) {
    this.report = report;
    this.report.data.databaseDiagnostics = { externalDatabaseSupplied: Boolean(externalDatabaseSupplied), steps: [] };
  }
  snapshot() { return sanitizeDatabaseDiagnostics(this.report.data.databaseDiagnostics); }
  async step(operation, action) {
    if (!operations.has(operation)) throw Error('Unknown database diagnostic operation');
    const step = { operation, status: 'running', category: 'operation-failed' };
    this.report.data.databaseDiagnostics.steps.push(step); await this.report.save();
    try {
      const result = await action(); Object.assign(step, { status: 'passed', category: 'ok', available: true }); return result;
    } catch (error) {
      const failure = commandFailure(error);
      Object.assign(step, { status: 'failed', category: defaults[operation] ?? failure?.category ?? 'operation-failed', available: false });
      if (failure) {
        if (typeof failure.available === 'boolean') step.commandAvailable = failure.available;
        if (exitCode(failure.exitCode)) step.exitCode = failure.exitCode;
      }
      throw error; // Preserve the original failure/cause and existing fail-closed behavior.
    } finally { await this.report.save(); }
  }
}
