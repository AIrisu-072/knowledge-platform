import type {
  CommandsCreateDocument,
  CommandsVersionWrite,
  CreateDocumentMultipart,
  CreateDocumentResponses,
  CreateDocumentVersionResponses,
  DownloadVersionFileData,
  Problem,
  VersionMultipart,
  VersionMutationResult,
} from './generated/types.gen';

export type CreateDocumentUpload = Omit<CreateDocumentMultipart, 'file'> & {
  file: Blob | File;
  originalFilename: string;
  mediaType: string;
};

export type VersionUpload = Omit<VersionMultipart, 'files'> & {
  files: ReadonlyMap<string, Blob | File>;
};

export type DownloadVersionFileInput = DownloadVersionFileData['path']
  & DownloadVersionFileData['query'];

export type BinaryTransportBridgeOptions = {
  baseUrl?: string;
  fetch?: typeof fetch;
};

type HttpMethod = 'GET' | 'POST' | 'PUT';

export class BinaryTransportError extends Error {
  readonly status?: number;

  constructor(message: string, options?: { status?: number; cause?: unknown }) {
    super(message, options?.cause === undefined ? undefined : { cause: options.cause });
    this.name = 'BinaryTransportError';
    this.status = options?.status;
  }
}

export class DocumentApiProblemError extends Error {
  readonly problem: Problem;
  readonly status: number;

  constructor(status: number, problem: Problem) {
    super(problem.detail ?? problem.title);
    this.name = 'DocumentApiProblemError';
    this.status = status;
    this.problem = problem;
  }
}

export class BinaryTransportBridge {
  private readonly baseUrl: string;
  private readonly fetcher: typeof fetch;

  constructor(options: BinaryTransportBridgeOptions = {}) {
    this.baseUrl = options.baseUrl ?? '';
    this.fetcher = options.fetch ?? globalThis.fetch.bind(globalThis);
  }

  async createDocument(
    input: CreateDocumentUpload,
  ): Promise<CreateDocumentResponses[201]> {
    if (!input.originalFilename.trim() || !input.mediaType.trim()) {
      throw new BinaryTransportError('originalFilename and mediaType are required');
    }

    const body = new FormData();
    body.append('request', new Blob([JSON.stringify(input.request)], {
      type: 'application/json',
    }));
    body.append('file', new Blob([input.file], { type: input.mediaType }), input.originalFilename);

    return this.requestJson<CreateDocumentResponses[201]>(
      'v1/documents',
      'POST',
      body,
    );
  }

  async createVersion(
    documentId: string,
    input: VersionUpload,
  ): Promise<CreateDocumentVersionResponses[201]> {
    const multipart = buildVersionMultipart(input.request, input.files);
    return this.requestJson<CreateDocumentVersionResponses[201]>(
      `v1/documents/${encodeURIComponent(documentId)}/versions`,
      'POST',
      multipart.body,
      { 'Content-Type': multipart.contentType },
    );
  }

  async updateWorkingVersion(
    documentId: string,
    versionId: string,
    input: VersionUpload,
  ): Promise<VersionMutationResult> {
    const multipart = buildVersionMultipart(input.request, input.files);
    return this.requestJson<VersionMutationResult>(
      `v1/documents/${encodeURIComponent(documentId)}/versions/${encodeURIComponent(versionId)}`,
      'PUT',
      multipart.body,
      { 'Content-Type': multipart.contentType },
    );
  }

  async downloadVersionFileBlob(input: DownloadVersionFileInput): Promise<Blob> {
    const response = await this.request(downloadPath(input), 'GET');
    return response.blob();
  }

  async downloadVersionFileStream(
    input: DownloadVersionFileInput,
  ): Promise<ReadableStream<Uint8Array>> {
    const response = await this.request(downloadPath(input), 'GET');
    if (!response.body) {
      throw new BinaryTransportError('The successful file response had no body', {
        status: response.status,
      });
    }
    return response.body;
  }

  private async requestJson<T>(
    path: string,
    method: HttpMethod,
    body: BodyInit,
    headers?: Record<string, string>,
  ): Promise<T> {
    const response = await this.request(path, method, body, headers);
    try {
      return await response.json() as T;
    } catch (cause) {
      throw new BinaryTransportError('The successful API response was not valid JSON', {
        status: response.status,
        cause,
      });
    }
  }

  private async request(
    path: string,
    method: HttpMethod,
    body?: BodyInit,
    headers?: Record<string, string>,
  ): Promise<Response> {
    let response: Response;
    try {
      response = await this.fetcher(this.url(path), {
        method,
        credentials: 'same-origin',
        ...(body === undefined ? {} : { body }),
        ...(headers === undefined ? {} : { headers }),
      });
    } catch (cause) {
      throw new BinaryTransportError('The document API request failed before receiving a response', {
        cause,
      });
    }

    if (!response.ok) {
      throw await normalizeProblem(response);
    }
    return response;
  }

  private url(path: string): URL {
    const configuredBase = this.baseUrl || globalThis.location?.origin;
    if (!configuredBase) {
      throw new BinaryTransportError('A baseUrl is required outside a browser origin');
    }
    const base = configuredBase.endsWith('/') ? configuredBase : `${configuredBase}/`;
    try {
      return new URL(path, base);
    } catch (cause) {
      throw new BinaryTransportError('A valid baseUrl is required for document API requests', {
        cause,
      });
    }
  }
}

type BuiltMultipart = { body: Blob; contentType: string };

function buildVersionMultipart(request: CommandsVersionWrite, files: ReadonlyMap<string, Blob | File>): BuiltMultipart {
  const parts = request.items.flatMap((item) => [
    { partId: item.partId, fileId: item.fileId },
    ...(item.renditions ?? []).map((rendition) => ({
      partId: rendition.partId,
      fileId: rendition.fileId,
    })),
  ]);
  const expectedIds = new Set<string>();
  for (const part of parts) {
    if (!isSafePartId(part.partId) || expectedIds.has(part.partId)) {
      throw new BinaryTransportError('The version manifest has an invalid or duplicate partId');
    }
    expectedIds.add(part.partId);
  }

  if (expectedIds.size !== files.size || [...files.keys()].some((id) => !expectedIds.has(id))) {
    throw new BinaryTransportError('Version manifest partIds must match the binary file map exactly');
  }

  const boundary = `knowledge-platform-${newBoundaryToken()}`;
  const body: BlobPart[] = [];
  body.push(
    `--${boundary}\r\n`,
    'Content-Disposition: form-data; name="request"\r\n',
    'Content-Type: application/json\r\n\r\n',
    JSON.stringify(request),
    '\r\n',
  );

  for (const { partId } of parts) {
    const file = files.get(partId);
    if (!file) {
      throw new BinaryTransportError(`Binary file for manifest part ${partId} is missing`);
    }
    body.push(
      `--${boundary}\r\n`,
      'Content-Disposition: form-data; name="files"; filename="binary"\r\n',
      'Content-Type: application/octet-stream\r\n',
      `X-Part-Id: ${partId}\r\n\r\n`,
      file,
      '\r\n',
    );
  }
  body.push(`--${boundary}--\r\n`);

  return {
    body: new Blob(body, { type: `multipart/form-data; boundary=${boundary}` }),
    contentType: `multipart/form-data; boundary=${boundary}`,
  };
}

function isSafePartId(partId: string): boolean {
  return partId.length > 0
    && partId.length <= 255
    && partId.trim() === partId
    && /^[\x21-\x7e]+$/.test(partId);
}

function newBoundaryToken(): string {
  const cryptoApi = globalThis.crypto;
  if (cryptoApi?.randomUUID) {
    return cryptoApi.randomUUID().replaceAll('-', '');
  }
  if (cryptoApi?.getRandomValues) {
    const random = cryptoApi.getRandomValues(new Uint8Array(16));
    return [...random].map((value) => value.toString(16).padStart(2, '0')).join('');
  }
  throw new BinaryTransportError('A secure random source is required for multipart boundaries');
}

function downloadPath(input: DownloadVersionFileInput): string {
  const query = new URLSearchParams({ purpose: input.purpose });
  return `v1/documents/${encodeURIComponent(input.documentId)}`
    + `/versions/${encodeURIComponent(input.versionId)}`
    + `/files/${encodeURIComponent(input.contentItemId)}/${encodeURIComponent(input.representationId)}`
    + `?${query.toString()}`;
}

async function normalizeProblem(response: Response): Promise<Error> {
  const body: unknown = await response.json().catch(() => undefined);
  if (isProblem(body)) {
    return new DocumentApiProblemError(response.status, body);
  }
  return new BinaryTransportError(
    `Document API request failed with HTTP ${response.status}`,
    { status: response.status },
  );
}

function isProblem(value: unknown): value is Problem {
  if (typeof value !== 'object' || value === null) return false;
  const problem = value as Partial<Problem>;
  return typeof problem.type === 'string'
    && typeof problem.title === 'string'
    && typeof problem.status === 'number'
    && typeof problem.code === 'string'
    && typeof problem.traceId === 'string'
    && typeof problem.retryable === 'boolean';
}
