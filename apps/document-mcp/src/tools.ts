import { type McpServer } from '@modelcontextprotocol/server';
import { z } from 'zod';
import Ajv from 'ajv';
import addFormats from 'ajv-formats';
import { getRootFolder, listFolderChildren, listDocuments, getDocument, listDocumentRevisions, getDocumentHistory, compareDocumentVersions, compareDocumentRevisions, listVersionFiles, type Client } from '@knowledge-platform/document-api-client';
import { requestSignal, ORDINARY_DEADLINE_MS, COMPARISON_DEADLINE_MS } from './api';
import { toolValue, toToolError } from './errors';
const validator = new Ajv();
addFormats(validator);
const uuid = z.string().refine(validator.compile({ type: 'string', format: 'uuid' })).meta({ format: 'uuid' });
const timestamp = z.string().refine(validator.compile({ type: 'string', format: 'date-time' })).meta({ format: 'date-time' });
const page = { pageSize: z.number().int().min(1).max(200).optional(), cursor: z.string().min(1).optional() };
const documentId = { documentId: uuid };
const comparison = { projection: z.enum(['diff', 'comparisonTable', 'display']), pageSize: z.number().int().min(1).max(100).optional(), cursor: z.string().optional() };
const incomplete = 'Unknown, Partial, None and truncation are incomplete evidence, not unchanged. Values are returned unaltered.';
type ApiResult = { data?: unknown; error?: unknown; response?: Response };
export function registerDocumentTools(server: McpServer, client: Client): void {
  function register<S extends z.ZodRawShape>(name: string, description: string, shape: S, call: (args: z.output<z.ZodObject<S>>, signal: AbortSignal) => Promise<ApiResult>, comparisonCall = false): void {
    server.registerTool(name, { description, inputSchema: z.strictObject(shape), annotations: { readOnlyHint: true, destructiveHint: false, openWorldHint: false } }, async (args, ctx) => {
      const signal = requestSignal(comparisonCall ? COMPARISON_DEADLINE_MS : ORDINARY_DEADLINE_MS, ctx.mcpReq.signal);
      try {
        const result = await call(args, signal);
        if (result.error !== undefined || !result.response?.ok) return toToolError(result.error, result.response?.status);
        if (!result.data || typeof result.data !== 'object' || Array.isArray(result.data)) return toToolError(undefined);
        return toolValue(result.data as Record<string, unknown>);
      } catch {
        return toToolError({ code: signal.aborted ? 'REQUEST_ABORTED' : 'UPSTREAM_UNAVAILABLE' });
      }
    });
  }
  register('document_get_root', 'Read the authorized root folder metadata.', {}, (_, signal) => getRootFolder({ client, signal }));
  register('document_list_folder', 'Read one bounded page of authorized child folders; return opaque cursor unchanged.', { folderId: uuid, ...page }, ({ folderId, ...query }, signal) => listFolderChildren({ client, signal, path: { folderId }, query }));
  register('document_list', 'Read one bounded document page in the requested authorization view. OCC revision, content Version and human Major.Minor Revision are distinct.', { view: z.enum(['published', 'authoring', 'history']), titleContains: z.string().optional(), folderId: uuid.optional(), includeDescendants: z.boolean().optional(), documentType: z.string().optional(), owningDepartment: z.string().optional(), category: z.string().optional(), createdFrom: timestamp.optional(), createdBefore: timestamp.optional(), sort: z.string().optional(), unreadOnly: z.boolean().optional(), ...page }, (query, signal) => listDocuments({ client, signal, query }));
  register('document_get', 'Read authorized document detail in published or authoring view; OCC revision is not human Major.Minor Revision.', { ...documentId, view: z.enum(['published', 'authoring']) }, ({ documentId, ...query }, signal) => getDocument({ client, signal, path: { documentId }, query }));
  register('document_list_revisions', 'Read one page of issued human Major.Minor Revisions, distinct from OCC revisions and content Versions. Legacy unavailable metadata remains unavailable.', { ...documentId, ...page }, ({ documentId, ...query }, signal) => listDocumentRevisions({ client, signal, path: { documentId }, query }));
  register('document_get_history', 'Read one authorized page of document history; no auto-pagination.', { ...documentId, ...page }, ({ documentId, ...query }, signal) => getDocumentHistory({ client, signal, path: { documentId }, query }));
  register('document_compare_versions', `Compare the required base/target content Version IDs using document-diff-v0 and the requested projection. ${incomplete} Server-owned comparison may write cache/audit.`, { ...documentId, baseVersionId: uuid, targetVersionId: uuid, profile: z.literal('document-diff-v0'), ...comparison }, ({ documentId, ...body }, signal) => compareDocumentVersions({ client, signal, path: { documentId }, body }), true);
  register('document_compare_revisions', `Compare the required base/target issued Revision IDs (human Major.Minor, not OCC) with the requested projection; unavailable_legacy remains unavailable. ${incomplete} Server-owned comparison may write cache/audit.`, { ...documentId, baseRevisionId: uuid, targetRevisionId: uuid, ...comparison }, ({ documentId, ...body }, signal) => compareDocumentRevisions({ client, signal, path: { documentId }, body }), true);
  register('document_list_files', 'Read file metadata for a content Version with explicit authorization purpose. Does not download bytes or full text.', { ...documentId, versionId: uuid, purpose: z.enum(['published', 'authoring', 'history']) }, ({ documentId, versionId, ...query }, signal) => listVersionFiles({ client, signal, path: { documentId, versionId }, query }));
}
