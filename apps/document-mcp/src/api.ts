import { createClient, createConfig, getSession, type Client } from '@knowledge-platform/document-api-client';
import type { McpConfig } from './config';
// API ordinary/diff budgets are 30s/45s; clients must outlive those structured errors.
export const ORDINARY_DEADLINE_MS = 35_000;
export const COMPARISON_DEADLINE_MS = 50_000;
export function createDocumentClient(config: McpConfig): Client {
  return createClient(createConfig({ baseUrl: config.apiBaseUrl, redirect: 'error', credentials: 'omit', parseAs: 'json' }));
}
export function requestSignal(milliseconds: number, cancellation?: AbortSignal): AbortSignal {
  const timeout = AbortSignal.timeout(milliseconds);
  return cancellation ? AbortSignal.any([timeout, cancellation]) : timeout;
}
export async function verifyAgentSession(client: Client, cancellation?: AbortSignal): Promise<void> {
  const { data } = await getSession({ client, signal: requestSignal(ORDINARY_DEADLINE_MS, cancellation) });
  if (!data || data.principal.identityProvider !== 'poc' || data.principal.principalId !== 'poc-agent' || data.invocationKind !== 'agent') {
    throw new Error('Document API did not verify the fixed PoC Agent session');
  }
}
