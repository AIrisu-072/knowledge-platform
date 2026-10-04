/** Actual-stdio checkpoint reader used only by the owned C3 runtime harness. */
import assert from 'node:assert/strict';
import { join } from 'node:path';
import { Client } from '@modelcontextprotocol/client';
import { StdioClientTransport } from '@modelcontextprotocol/client/stdio';
import { getDocument, listDocumentRevisions, listVersionFiles,
  type PublishedDocumentDetail, type ModelsDocumentRevisionPage, type ModelsFileList,
} from '@knowledge-platform/document-api-client';
import { sharedDetail } from './runtime-oracle';
export { sharedDetail, assertSharedState, assertRevisionTransition, assertNoopState, assertMutationReplay } from './runtime-oracle';

export type SharedState = {
  detail: ReturnType<typeof sharedDetail>;
  revisions: ModelsDocumentRevisionPage;
  files: Array<{ versionId: string; files: ModelsFileList }>;
};
type Transcript = Array<{ name: string; arguments: Record<string, unknown>; response: unknown }>;
function versions(detail: PublishedDocumentDetail, revisions: ModelsDocumentRevisionPage): string[] {
  assert.equal(revisions.nextCursor, null, 'Checkpoint revision history must be complete');
  assert.ok(detail.currentVersionId);
  return [...new Set([detail.currentVersionId, ...revisions.items.map(item => item.documentVersionId)])].sort();
}
export async function readHumanSharedState(baseUrl: string, documentId: string): Promise<SharedState> {
  const options = { baseUrl, throwOnError: true as const, get signal() { return AbortSignal.timeout(50_000); } };
  const detail = (await getDocument({ ...options, path: { documentId }, query: { view: 'published' } })).data as PublishedDocumentDetail;
  const revisions = (await listDocumentRevisions({ ...options, path: { documentId }, query: { pageSize: 200 } })).data;
  const files = [];
  for (const versionId of versions(detail, revisions)) files.push({ versionId,
    files: (await listVersionFiles({ ...options, path: { documentId, versionId }, query: { purpose: 'history' } })).data });
  return { detail: sharedDetail(detail), revisions, files };
}
export async function readMcpSharedState(baseUrl: string, documentId: string): Promise<{ state: SharedState; transcript: Transcript }> {
  const client = new Client({ name: 'document-ordered-consistency-acceptance', version: '0.0.0' });
  const transport = new StdioClientTransport({ command: process.execPath, args: [join(__dirname, 'main.cjs')],
    env: { KP_DOCUMENT_API_BASE_URL: baseUrl }, stderr: 'pipe' });
  let stderr = ''; transport.stderr!.on('data', chunk => { stderr += chunk; });
  const transcript: Transcript = [];
  async function call<T>(name: string, args: Record<string, unknown>): Promise<T> {
    const result = await client.callTool({ name, arguments: args }, { timeout: 55_000 });
    assert.notEqual(result.isError, true, `${name} failed`);
    assert.deepEqual(JSON.parse((result.content[0] as { text: string }).text), result.structuredContent);
    transcript.push({ name, arguments: args, response: result.structuredContent });
    return result.structuredContent as T;
  }
  try {
    await client.connect(transport);
    const detail = await call<PublishedDocumentDetail>('document_get', { documentId, view: 'published' });
    const revisions = await call<ModelsDocumentRevisionPage>('document_list_revisions', { documentId, pageSize: 200 });
    const files = [];
    for (const versionId of versions(detail, revisions)) files.push({ versionId,
      files: await call<ModelsFileList>('document_list_files', { documentId, versionId, purpose: 'history' }) });
    assert.equal(stderr, '');
    return { state: { detail: sharedDetail(detail), revisions, files }, transcript };
  } finally { await client.close(); }
}
