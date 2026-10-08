import {
  BinaryTransportBridge,
  getCurrentDocumentVersionReadState,
  recordDocumentVersionView,
  resetDocumentVersionReadState,
  type CurrentReadState,
  type ReadStateMutationRequest,
  type ReadStateMutationResult,
  cancelPublicationSchedule,
  createFolder,
  renameFolder,
  moveFolder,
  moveDocument,
  withdrawVersion,
  endDocumentPublication,
  compareDocumentRevisions,
  compareDocumentVersions,
  getDocument,
  getDocumentRevision,
  getDocumentAccessPolicy,
  getFolderAccessPolicy,
  setFolderAccessPolicy,
  getDocumentHistory,
  getDocumentVersion,
  getVersionEditManifest,
  rebaseWorkingVersion,
  getRootFolder,
  getSession,
  listDocumentRevisions,
  listDocuments,
  listDocumentVersions,
  listFolderChildren,
  listVersionFiles,
  publishVersion,
  patchDocumentMetadata,
  recoverDocumentCreation,
  schedulePublication,
  setDocumentAccessPolicy,
  type CommandsComparisonRequest,
  type CommandsWithdrawVersion,
  type CommandsEndPublication,
  type CommandsCreateDocument,
  type CommandsCreateFolder,
  type CommandsRenameFolder,
  type CommandsMoveFolder,
  type CommandsMoveDocument,
  type CommandsMetadataPatch,
  type CreateDocumentResult,
  type CommandsPolicyExplicit,
  type CommandsPolicyInherit,
  type CommandsRevisionComparisonRequest,
  type CommandsSetAccessPolicy,
  type CommandsVersionWrite,
  type ModelsEditManifest,
  type DocumentDetail,
  type DocumentList,
  type DocumentRevisionPage,
  type FileList,
  type FolderChildren,
  type FolderDetail,
  type History,
  type ModelsAccessPolicyRead,
  type ModelsComparisonResponse,
  type ModelsSession,
  type RevisionComparisonResponse,
  type VersionList,
  type VersionDetail,
  type View,
} from '@knowledge-platform/document-api-client';

const data = { throwOnError: true as const };
const binary = new BinaryTransportBridge();

async function payload<T>(request: Promise<{ data: T }>): Promise<T> {
  return (await request).data;
}

function apiSort(sort: string | undefined): string | undefined {
  switch (sort) {
    case 'published_at_desc': return 'publishedAtDesc';
    case 'created_at_desc': return 'createdAtDesc';
    case 'title_asc': return 'titleAsc';
    default: return sort;
  }
}

export const documentApi = {
  getCurrentDocumentVersionReadState(documentId: string, versionId: string, options?: { signal?: AbortSignal }): Promise<CurrentReadState> {
    return payload(getCurrentDocumentVersionReadState({ ...data, path: { documentId, versionId }, ...(options?.signal ? { signal: options.signal } : {}) }));
  },
  recordDocumentVersionView(documentId: string, versionId: string, body: ReadStateMutationRequest): Promise<ReadStateMutationResult> {
    return payload(recordDocumentVersionView({ ...data, path: { documentId, versionId }, body }));
  },
  resetDocumentVersionReadState(documentId: string, versionId: string, body: ReadStateMutationRequest): Promise<ReadStateMutationResult> {
    return payload(resetDocumentVersionReadState({ ...data, path: { documentId, versionId }, body }));
  },
  getFolderAccessPolicy(folderId: string): Promise<ModelsAccessPolicyRead> {
    return payload(getFolderAccessPolicy({ ...data, path: { folderId } }));
  },
  setFolderAccessPolicy(folderId: string, body: CommandsPolicyExplicit | CommandsPolicyInherit) {
    // Preserve the normative wire discriminators despite the generated synthetic union.
    return payload(setFolderAccessPolicy({ ...data, path: { folderId }, body: body as unknown as CommandsSetAccessPolicy }));
  },
  moveDocument(documentId: string, body: CommandsMoveDocument) {
    return payload(moveDocument({ ...data, path: { documentId }, body }));
  },
  moveFolder(folderId: string, body: CommandsMoveFolder) {
    return payload(moveFolder({ ...data, path: { folderId }, body }));
  },
  renameFolder(folderId: string, body: CommandsRenameFolder) {
    return payload(renameFolder({ ...data, path: { folderId }, body }));
  },
  createFolder(body: CommandsCreateFolder) {
    return payload(createFolder({ ...data, body }));
  },
  patchDocumentMetadata(documentId: string, body: CommandsMetadataPatch) {
    return payload(patchDocumentMetadata({ ...data, path: { documentId }, body }));
  },
  createDocument(request: CommandsCreateDocument, file: File): Promise<CreateDocumentResult> {
    return binary.createDocument({ request, file, originalFilename: file.name, mediaType: file.type || 'application/octet-stream' });
  },
  recoverDocumentCreation(ids: CreateDocumentResult): Promise<CreateDocumentResult> {
    return payload(recoverDocumentCreation({ ...data, path: { documentId: ids.documentId },
      query: { documentVersionId: ids.documentVersionId, fileId: ids.fileId } }));
  },
  getSession(): Promise<ModelsSession> {
    return payload(getSession(data));
  },
  getRootFolder(): Promise<FolderDetail> {
    return payload(getRootFolder(data));
  },
  listFolderChildren(folderId: string, cursor?: string): Promise<FolderChildren> {
    return payload(listFolderChildren({
      ...data,
      path: { folderId },
      query: { pageSize: 200, ...(cursor === undefined ? {} : { cursor }) },
    }));
  },
  listDocuments(query: Parameters<typeof listDocuments>[0]['query']): Promise<DocumentList> {
    return payload(listDocuments({ ...data, query: { ...query, sort: apiSort(query.sort) } }));
  },
  getDocument(documentId: string, view: Exclude<View, 'history'> = 'published'): Promise<DocumentDetail> {
    return payload(getDocument({ ...data, path: { documentId }, query: { view } }));
  },
  listDocumentVersions(documentId: string, purpose: View, cursor?: string): Promise<VersionList> {
    return payload(listDocumentVersions({
      ...data,
      path: { documentId },
      query: { purpose, pageSize: 100, ...(cursor === undefined ? {} : { cursor }) },
    }));
  },
  getDocumentVersion(documentId: string, versionId: string, purpose: View): Promise<VersionDetail> {
    return payload(getDocumentVersion({ ...data, path: { documentId, versionId }, query: { purpose } }));
  },
  getDocumentRevision(documentId: string, revisionId: string, signal?: AbortSignal) {
    return payload(getDocumentRevision({ ...data, path: { documentId, revisionId }, signal }));
  },
  listDocumentRevisions(documentId: string, cursor?: string): Promise<DocumentRevisionPage> {
    return payload(listDocumentRevisions({
      ...data,
      path: { documentId },
      query: { pageSize: 100, ...(cursor === undefined ? {} : { cursor }) },
    }));
  },
  getDocumentHistory(documentId: string, cursor?: string): Promise<History> {
    return payload(getDocumentHistory({
      ...data,
      path: { documentId },
      query: { pageSize: 100, ...(cursor === undefined ? {} : { cursor }) },
    }));
  },
  listVersionFiles(documentId: string, versionId: string, purpose: View): Promise<FileList> {
    return payload(listVersionFiles({ ...data, path: { documentId, versionId }, query: { purpose } }));
  },
  getDocumentAccessPolicy(documentId: string): Promise<ModelsAccessPolicyRead> {
    return payload(getDocumentAccessPolicy({ ...data, path: { documentId } }));
  },
  compareDocumentVersions(documentId: string, body: CommandsComparisonRequest): Promise<ModelsComparisonResponse> {
    return payload(compareDocumentVersions({ ...data, path: { documentId }, body }));
  },
  compareDocumentRevisions(documentId: string, body: CommandsRevisionComparisonRequest): Promise<RevisionComparisonResponse> {
    return payload(compareDocumentRevisions({ ...data, path: { documentId }, body }));
  },
  publishVersion(documentId: string, versionId: string, body: { operationId: string; expectedRevision: number }) {
    return payload(publishVersion({ ...data, path: { documentId, versionId }, body }));
  },
  schedulePublication(documentId: string, versionId: string, body: { operationId: string; expectedRevision: number; scheduledPublishAt: string }) {
    return payload(schedulePublication({ ...data, path: { documentId, versionId }, body }));
  },
  cancelPublicationSchedule(documentId: string, versionId: string, body: { operationId: string; publishOperationId: string; expectedRevision: number }) {
    return payload(cancelPublicationSchedule({ ...data, path: { documentId, versionId }, body }));
  },
  withdrawVersion(documentId: string, versionId: string, body: CommandsWithdrawVersion) {
    return payload(withdrawVersion({ ...data, path: { documentId, versionId }, body }));
  },
  endDocumentPublication(documentId: string, body: CommandsEndPublication) {
    return payload(endDocumentPublication({ ...data, path: { documentId }, body }));
  },
  setDocumentAccessPolicy(documentId: string, body: CommandsPolicyExplicit | CommandsPolicyInherit) {
    // The generated union currently adds a synthetic discriminator that conflicts with the normative wire values.
    return payload(setDocumentAccessPolicy({ ...data, path: { documentId }, body: body as unknown as CommandsSetAccessPolicy }));
  },
  getVersionEditManifest(documentId: string, versionId: string, purpose: 'published' | 'authoring'): Promise<ModelsEditManifest> {
    return payload(getVersionEditManifest({ ...data, path: { documentId, versionId }, query: { purpose } }));
  },
  prepareVersionUpload(request: CommandsVersionWrite, files: ReadonlyMap<string, Blob | File>) {
    return binary.prepareVersionUpload({ request, files });
  },
  createVersion(documentId: string, request: CommandsVersionWrite, files: ReadonlyMap<string, Blob | File>, prepared?: ReturnType<BinaryTransportBridge['prepareVersionUpload']>) {
    return binary.createVersion(documentId, { request, files }, prepared);
  },
  updateWorkingVersion(documentId: string, versionId: string, request: CommandsVersionWrite, files: ReadonlyMap<string, Blob | File>, prepared?: ReturnType<BinaryTransportBridge['prepareVersionUpload']>) {
    return binary.updateWorkingVersion(documentId, versionId, { request, files }, prepared);
  },
  rebaseWorkingVersion(documentId: string, versionId: string, body: { operationId: string; expectedRevision: number }) {
    return payload(rebaseWorkingVersion({ ...data, path: { documentId, versionId }, body }));
  },
  downloadVersionFile(input: {
    documentId: string;
    versionId: string;
    contentItemId: string;
    representationId: string;
    purpose: View;
  }, options?: { signal?: AbortSignal }) {
    return binary.downloadVersionFileBlob(input, options);
  },
};

export function isProblemRecord(error: unknown): error is {
  type: string;
  title: string;
  status: number;
  code: string;
  traceId: string;
  retryable: boolean;
  detail?: string;
  exactRetry?: boolean;
  errors?: Array<{ pointer: string; code: string; detail?: string }>;
} {
  if (typeof error !== 'object' || error === null) return false;
  const candidate = error as Record<string, unknown>;
  return typeof candidate.type === 'string'
    && typeof candidate.title === 'string'
    && typeof candidate.status === 'number'
    && typeof candidate.code === 'string'
    && typeof candidate.traceId === 'string'
    && typeof candidate.retryable === 'boolean';
}
