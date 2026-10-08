import { createHash } from 'node:crypto';
import {
  BinaryTransportBridge, createClient, createFolder, getDocument, getDocumentAccessPolicy,
  getDocumentRevision, getDocumentVersion, getFolderAccessPolicy, getRootFolder, getSession,
  listDocumentRevisions, listDocuments, listDocumentVersions, listVersionFiles,
  patchDocumentMetadata, publishVersion, setDocumentAccessPolicy, setFolderAccessPolicy,
} from '@knowledge-platform/document-api-client';
import type {
  CommandsCreateFolder, CommandsPolicyExplicit, CommandsSetAccessPolicy, ModelsFileList,
  PolicyGrantInput,
} from '@knowledge-platform/document-api-client';

export const HUMAN_GRANT: PolicyGrantInput = { subjectKind: 'group', identityProvider: 'poc', subjectId: 'poc-users', actions: ['read', 'readHistory', 'write', 'publish', 'administer'] };
export const AGENT_GRANT: PolicyGrantInput = { subjectKind: 'group', identityProvider: 'poc', subjectId: 'poc-agents', actions: ['read', 'readHistory'] };
const reason = 'Bounded Document load qualification';
const fixture = 'document-load-qualification-v1';
const MAX_FILE_BYTES = 32 * 1024 * 1024;
const MAX_SNAPSHOT_BYTES = 128 * 1024 * 1024;
const MAX_PAGES = 1_000;
const MAX_ITEMS = 100_000;
export type Timing = { operation: string; elapsedMs: number; status: number | 'network_error' | 'validation_error' };
export type Asset = { bytes: Uint8Array | Blob; filename: string; mediaType: string; title?: string };
export type ProbeOptions = { humanUrl: string; agentUrl: string; fetch?: typeof fetch; onTiming?: (timing: Timing) => void; maxFileBytes?: number; maxSnapshotBytes?: number; signal?: AbortSignal };
type Endpoint = 'human' | 'agent';

export type FailureDiagnostic = { operation: string; httpStatus: number | null; problemCode: string | null };
const problemStatuses: Readonly<Record<string, number>> = Object.freeze({ VALIDATION_FAILED: 422, AUTHENTICATION_REQUIRED: 401, FORBIDDEN: 403, DOCUMENT_NOT_FOUND: 404, DOCUMENT_VERSION_NOT_FOUND: 404, REVISION_NOT_FOUND: 404, FOLDER_NOT_FOUND: 404, REVISION_CONFLICT: 409, OPERATION_CONFLICT: 409, CURSOR_STALE: 409, STALE_VERSION: 409, STALE_COMPARISON_INPUT: 409, BUSINESS_RULE_REJECTED: 422, RESERVED_DOCUMENT: 409, FOLDER_CYCLE: 409, ROOT_PROTECTED: 409, IDENTITY_UNAVAILABLE: 503, PUBLISH_QUALITY_REJECTED: 422, UNSUPPORTED_MEDIA_TYPE: 415, DEPENDENCY_UNAVAILABLE: 503, TIMEOUT: 504, COMMIT_OUTCOME_UNKNOWN: 503, INTEGRITY_VIOLATION: 500, INTERNAL: 500 });
function diagnostic(operation: string, status: Timing['status'], error: unknown): FailureDiagnostic {
  const candidate = error && typeof error === 'object' && 'problem' in error ? error.problem : error;
  const problem = candidate && typeof candidate === 'object' ? candidate as Record<string, unknown> : {};
  const code = typeof problem.code === 'string' && Object.hasOwn(problemStatuses, problem.code) && problemStatuses[problem.code] === status && problem.status === status ? problem.code : null;
  return { operation, httpStatus: typeof status === 'number' ? status : null, problemCode: code };
}

export class ProbeSafetyError extends Error {
  constructor(message: string, readonly status?: number | 'network_error' | 'validation_error', readonly diagnostic?: FailureDiagnostic) { super(message); this.name = 'ProbeSafetyError'; }
}
function origin(value: string): string {
  let url: URL;
  try { url = new URL(value); } catch { throw new ProbeSafetyError('A loopback HTTP origin is required'); }
  if (url.protocol !== 'http:' || !['127.0.0.1', '[::1]', 'localhost'].includes(url.hostname)
    || url.username || url.password || url.pathname !== '/' || url.search || url.hash) {
    throw new ProbeSafetyError('A loopback HTTP origin without credentials, path, query or fragment is required');
  }
  return url.origin;
}
function bound(value: number | undefined, maximum: number): number {
  const result = value ?? maximum;
  if (!Number.isSafeInteger(result) || result < 1 || result > maximum) throw new ProbeSafetyError('Invalid byte limit');
  return result;
}
function canonicalGrants(grants: readonly PolicyGrantInput[]): string {
  return JSON.stringify(grants.map(g => ({ subjectKind: g.subjectKind, identityProvider: g.identityProvider, subjectId: g.subjectId, actions: [...g.actions].sort() })).sort((a, b) => JSON.stringify(a).localeCompare(JSON.stringify(b))));
}
async function payload<T>(request: Promise<{ data: T }>): Promise<T> { return (await request).data; }

/** Real generated SDK and binary transport. No mutation is retried by this adapter. */
export class DocumentProbe {
  readonly humanUrl: string;
  readonly agentUrl: string;
  private readonly fetcher: typeof fetch;
  private readonly onTiming?: ProbeOptions['onTiming'];
  private readonly maxFileBytes: number;
  private readonly maxSnapshotBytes: number;
  private readonly signal?: AbortSignal;
  private verified = false;

  constructor(options: ProbeOptions) {
    this.humanUrl = origin(options.humanUrl); this.agentUrl = origin(options.agentUrl);
    if (this.humanUrl === this.agentUrl) throw new ProbeSafetyError('Human and Agent origins must be distinct');
    this.fetcher = options.fetch ?? globalThis.fetch.bind(globalThis);
    this.onTiming = options.onTiming;
    this.signal = options.signal;
    this.maxFileBytes = bound(options.maxFileBytes, MAX_FILE_BYTES);
    this.maxSnapshotBytes = bound(options.maxSnapshotBytes, MAX_SNAPSHOT_BYTES);
  }

  private requireVerified(): void {
    if (!this.verified) throw new ProbeSafetyError('Verify the fixed PoC sessions before mutation');
  }

  private async request<T>(operation: string, endpoint: Endpoint, work: (common: ReturnType<DocumentProbe['common']>, bridge: BinaryTransportBridge) => Promise<T>): Promise<T> {
    const started = performance.now();
    let status: Timing['status'] = 'validation_error';
    const baseUrl = endpoint === 'human' ? this.humanUrl : this.agentUrl;
    const fetcher: typeof fetch = async (input, init) => {
      const request = new Request(input, init);
      if (new URL(request.url).origin !== baseUrl) throw new ProbeSafetyError('Unexpected request origin');
      const safe = new Request(request, { redirect: 'error', credentials: 'omit', signal: AbortSignal.any([request.signal, AbortSignal.timeout(120_000), ...(this.signal ? [this.signal] : [])]) });
      status = 'network_error';
      const response = await this.fetcher(safe);
      status = response.status;
      return response;
    };
    try { return await work(this.common(baseUrl, fetcher), new BinaryTransportBridge({ baseUrl, fetch: fetcher })); }
    catch (error) {
      if (error instanceof ProbeSafetyError) throw error;
      // Never surface transport causes, response bodies, credentials, or server URLs.
      throw new ProbeSafetyError(`${operation} failed (${status})`, status, diagnostic(operation, status, error));
    } finally {
      // Telemetry cannot turn a confirmed mutation into an unknown result.
      try { this.onTiming?.({ operation, elapsedMs: performance.now() - started, status }); } catch { /* diagnostic callback only */ }
    }
  }

  private common(baseUrl: string, fetcher: typeof fetch) {
    return { client: createClient(), baseUrl, fetch: fetcher, throwOnError: true as const };
  }

  async verifySessions() {
    this.verified = false;
    const human = await this.request('verifyHumanSession', 'human', c => payload(getSession(c)));
    if (human.principal?.identityProvider !== 'poc' || human.principal.principalId !== 'poc-human' || human.invocationKind !== 'human_interactive') {
      throw new ProbeSafetyError('Human endpoint must verify poc-human / human_interactive');
    }
    const agent = await this.request('verifyAgentSession', 'agent', c => payload(getSession(c)));
    if (agent.principal?.identityProvider !== 'poc' || agent.principal.principalId !== 'poc-agent' || agent.invocationKind !== 'agent') {
      throw new ProbeSafetyError('Agent endpoint must verify poc-agent / agent');
    }
    const root = await this.request('rootFolder', 'human', c => payload(getRootFolder(c)));
    const policy = await this.request('rootPolicy', 'human', c => payload(getFolderAccessPolicy({ ...c, path: { folderId: root.folderId } })));
    if (policy.bindingMode !== 'explicit' || canonicalGrants(policy.effectiveGrants) !== canonicalGrants([HUMAN_GRANT, AGENT_GRANT])) {
      throw new ProbeSafetyError('Root must have the fixed PoC human and agent bootstrap grants');
    }
    this.verified = true;
    return { human, agent, rootFolderId: root.folderId, root };
  }

  async createFolder(request: CommandsCreateFolder) {
    this.requireVerified();
    return this.request('createFolder', 'human', c => payload(createFolder({ ...c, body: request })));
  }

  async setFolderPolicy(folderId: string, request: CommandsPolicyExplicit) {
    this.requireVerified();
    if (request.mode !== 'explicit' || ![canonicalGrants([HUMAN_GRANT, AGENT_GRANT]), canonicalGrants([HUMAN_GRANT])].includes(canonicalGrants(request.grants))) {
      throw new ProbeSafetyError('Qualification folders require exact fixed grants');
    }
    // Same narrow generated-discriminator workaround as the qualified PoC seed.
    return this.request('setFolderPolicy', 'human', c => payload(setFolderAccessPolicy({ ...c, path: { folderId }, body: request as unknown as CommandsSetAccessPolicy })));
  }

  private blob(asset: Asset): Blob {
    const bytes = asset.bytes instanceof Blob ? asset.bytes : new Blob([Uint8Array.from(asset.bytes)]);
    if (bytes.size === 0 || bytes.size > this.maxFileBytes) throw new ProbeSafetyError('Upload exceeds the nonempty file byte limit');
    if (!asset.filename.trim() || !asset.mediaType.trim()) throw new ProbeSafetyError('Upload filename and media type are required');
    return bytes;
  }

  async create(input: Asset & { folderId: string; title: string }) {
    this.requireVerified();
    const file = this.blob(input);
    return this.request('create', 'human', (_c, bridge) => bridge.createDocument({
      request: { folderId: input.folderId, title: input.title, documentMetadata: { syntheticFixture: fixture }, versionMetadata: { syntheticFixture: fixture } },
      file, originalFilename: input.filename, mediaType: input.mediaType,
    }));
  }

  async detail(documentId: string) {
    return this.request('detail', 'human', c => payload(getDocument({ ...c, path: { documentId }, query: { view: 'authoring' } })));
  }

  private async pages<T>(read: (cursor?: string) => Promise<{ items: T[]; nextCursor: string | null }>, key: (item: T) => string): Promise<T[]> {
    const items: T[] = [], cursors = new Set<string>(), ids = new Set<string>();
    let cursor: string | undefined;
    for (let pageIndex = 0; pageIndex < MAX_PAGES; pageIndex++) {
      const page = await read(cursor);
      if (!Array.isArray(page.items) || !(page.nextCursor === null || typeof page.nextCursor === 'string' && page.nextCursor.length > 0)) throw new ProbeSafetyError('Invalid pagination response');
      for (const item of page.items) {
        const id = key(item);
        if (!id || ids.has(id)) throw new ProbeSafetyError('Duplicate or missing item identity in pagination');
        ids.add(id); items.push(item);
        if (items.length > MAX_ITEMS) throw new ProbeSafetyError('Pagination item limit exceeded');
      }
      if (page.nextCursor === null) return items;
      if (cursors.has(page.nextCursor)) throw new ProbeSafetyError('Pagination cursor loop detected');
      cursors.add(page.nextCursor); cursor = page.nextCursor;
    }
    throw new ProbeSafetyError('Pagination page limit exceeded');
  }

  async list(folderId: string, endpoint: Endpoint = 'human'): Promise<string[]> {
    if (!['human', 'agent'].includes(endpoint)) throw new ProbeSafetyError('Unknown endpoint');
    const items = await this.pages<{ documentId: string }>(cursor => this.request('list', endpoint, c => payload(listDocuments({ ...c,
      query: { view: endpoint === 'human' ? 'authoring' : 'published', folderId, includeDescendants: false, pageSize: 100, ...(cursor ? { cursor } : {}) },
    }))), item => item.documentId);
    return items.map(item => item.documentId);
  }

  async publish(documentId: string, versionId: string, expectedRevision: number, operationId: string) {
    this.requireVerified();
    return this.request('publish', 'human', c => payload(publishVersion({ ...c, path: { documentId, versionId }, body: { operationId, expectedRevision } })));
  }

  async updateMetadata(documentId: string, expectedDocumentRevision: number, operationId: string) {
    this.requireVerified();
    return this.request('updateMetadata', 'human', c => payload(patchDocumentMetadata({ ...c, path: { documentId },
      body: { operationId, expectedDocumentRevision, set: { loadQualificationOperation: operationId }, unset: [], reason },
    })));
  }

  async createNextVersion(documentId: string, expectedRevision: number, operationId: string, targetVersionId: string, fileId: string, asset: Asset & { title: string }) {
    this.requireVerified();
    const file = this.blob(asset);
    return this.request('createNextVersion', 'human', (_c, bridge) => bridge.createVersion(documentId, {
      request: { operationId, targetVersionId, expectedRevision, title: asset.title,
        items: [{ logicalPath: 'primary', ordinal: 0, fileId, partId: 'primary', mediaType: asset.mediaType, originalFilename: asset.filename }] },
      files: new Map([['primary', file]]),
    }));
  }

  async denyAgent(documentId: string, expectedPolicyRevision: number, operationId: string) {
    this.requireVerified();
    const body: CommandsPolicyExplicit = { operationId, expectedPolicyRevision, mode: 'explicit', grants: [HUMAN_GRANT], reason };
    return this.request('denyAgent', 'human', c => payload(setDocumentAccessPolicy({ ...c, path: { documentId }, body: body as unknown as CommandsSetAccessPolicy })));
  }

  async agentReadStatus(documentId: string) {
    return this.request('agentRead', 'agent', async c => {
      const result = await getDocument({ ...c, throwOnError: false, path: { documentId }, query: { view: 'published' } });
      if (!result.response) throw new ProbeSafetyError('Agent read failed before receiving a response', 'network_error');
      const status = result.response.status;
      if (status !== 200 && status !== 403) throw new ProbeSafetyError(`Unexpected Agent read status ${status}`, status);
      if (status === 200 && result.data?.documentId !== documentId) throw new ProbeSafetyError('Agent read returned a different document');
      return { status, allowed: status === 200 };
    });
  }

  async snapshot(documentId: string, { purpose = 'history' }: { purpose?: 'history' | 'authoring' } = {}) {
    if (purpose !== 'history' && purpose !== 'authoring') throw new ProbeSafetyError('Invalid snapshot purpose');
    const detail = await this.detail(documentId);
    const path = { documentId };
    const policy = await this.request('documentPolicy', 'human', c => payload(getDocumentAccessPolicy({ ...c, path })));
    const revisionSummaries = await this.pages(cursor => this.request('revisions', 'human', c => payload(listDocumentRevisions({ ...c, path, query: { pageSize: 100, ...(cursor ? { cursor } : {}) } }))), item => item.revisionId);
    const revisions = [];
    for (const revision of revisionSummaries) {
      revisions.push(await this.request('revisionDetail', 'human', c => payload(getDocumentRevision({ ...c, path: { documentId, revisionId: revision.revisionId } }))));
    }
    const summaries = await this.pages(cursor => this.request('versions', 'human', c => payload(listDocumentVersions({ ...c, path, query: { purpose, pageSize: 100, ...(cursor ? { cursor } : {}) } }))), item => item.versionId);
    const versions = [];
    let totalBytes = 0;
    for (const summary of summaries) {
      const versionId = summary.versionId;
      const versionDetail = await this.request('versionDetail', 'human', c => payload(getDocumentVersion({ ...c, path: { documentId, versionId }, query: { purpose } })));
      const listing = await this.request('versionFiles', 'human', c => payload(listVersionFiles({ ...c, path: { documentId, versionId }, query: { purpose } })));
      const files: (ModelsFileList['items'][number] & { sha256: string; downloadedBytes: number })[] = [];
      const identities = new Set<string>();
      for (const file of listing.items) {
        const key = `${file.contentItemId}\0${file.representationId}`;
        if (identities.has(key)) throw new ProbeSafetyError('Duplicate file identity in snapshot');
        identities.add(key);
        if (!Number.isSafeInteger(file.sizeBytes) || file.sizeBytes < 0 || file.sizeBytes > this.maxFileBytes || totalBytes + file.sizeBytes > this.maxSnapshotBytes) throw new ProbeSafetyError('Snapshot file byte limit exceeded');
        const fingerprint = await this.request('downloadHash', 'human', async (_c, bridge) => {
          const stream = await bridge.downloadVersionFileStream({ documentId, versionId, contentItemId: file.contentItemId, representationId: file.representationId, purpose });
          const reader = stream.getReader(), hash = createHash('sha256');
          let downloadedBytes = 0;
          try {
            while (true) {
              const part = await reader.read();
              if (part.done) break;
              downloadedBytes += part.value.byteLength;
              if (downloadedBytes > this.maxFileBytes || totalBytes + downloadedBytes > this.maxSnapshotBytes || downloadedBytes > file.sizeBytes) throw new ProbeSafetyError('Downloaded snapshot file byte limit exceeded');
              hash.update(part.value);
            }
            if (downloadedBytes !== file.sizeBytes) throw new ProbeSafetyError('Downloaded bytes differ from file metadata size');
            return { sha256: hash.digest('hex'), downloadedBytes };
          } catch (error) { await reader.cancel().catch(() => undefined); throw error; }
          finally { reader.releaseLock(); }
        });
        totalBytes += fingerprint.downloadedBytes;
        files.push({ ...file, ...fingerprint });
      }
      versions.push({ summary, detail: versionDetail, files });
    }
    return { detail, policy, revisions, versions };
  }
}
