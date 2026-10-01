// CI reports are untrusted: never copy error messages, endpoint URLs or artifact values.
const phases = new Set(['gui-oracle', 'stdio-initialize', 'discovery', 'root-navigation', 'list-visibility', 'shared-state', 'history-comparisons', 'files', 'denied-ids', 'metadata-update', 'acl-revocation', 'stdio-cleanup', 'complete']);
export function summarizeAgent(value) {
  const input = value && typeof value === 'object' && !Array.isArray(value) ? value : {};
  const phase = phases.has(input.phase) ? input.phase : 'unverified';
  const status = input.status === 'PASS' && phase === 'complete' ? 'passed'
    : input.status === 'FAIL' && phase !== 'unverified' ? 'failed' : 'unavailable';
  return { status, phase, failureCategory: status === 'passed' ? 'none'
    : ['assertion', 'execution-failure'].includes(input.failureCategory) ? input.failureCategory : 'unverified',
    completedChecks: Array.isArray(input.checks) ? Math.min(input.checks.length, 100) : 0 };
}
