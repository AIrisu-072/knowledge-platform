import type { CallToolResult } from '@modelcontextprotocol/server';
export function toolValue(value: Record<string, unknown>, isError = false): CallToolResult {
  return { content: [{ type: 'text', text: JSON.stringify(value) }], structuredContent: value, ...(isError ? { isError: true } : {}) };
}
export function toToolError(problem: unknown, status?: number): CallToolResult {
  const p = problem && typeof problem === 'object' ? problem as Record<string, unknown> : {};
  // Never reflect arbitrary HTML, stack traces, problem detail/title or network diagnostics.
  return toolValue({
    code: typeof p.code === 'string' && /^[A-Z][A-Z0-9_]{0,127}$/.test(p.code) ? p.code : 'UPSTREAM_UNAVAILABLE',
    ...(typeof status === 'number' ? { status } : {}),
    ...(typeof p.traceId === 'string' ? { traceId: p.traceId } : {}),
    ...(typeof p.retryable === 'boolean' ? { retryable: p.retryable } : {}),
  }, true);
}
