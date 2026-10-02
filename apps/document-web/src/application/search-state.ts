import Ajv from 'ajv';

export type DocumentView = 'published' | 'authoring' | 'history';
export type DocumentDetailTab = 'overview' | 'versions' | 'compare' | 'history' | 'access';
export type VersionWorkflow = 'newVersion' | 'publication';
export type ListSearch = {
  view: DocumentView;
  titleContains?: string;
  folderId?: string;
  includeDescendants: boolean;
  sort: 'created_at_desc' | 'title_asc' | 'published_at_desc';
  pageSize: number;
  cursor?: string;
  selectedDocumentId?: string;
  panel: 'open' | 'closed';
};
export type DetailSearch = {
  tab: DocumentDetailTab;
  view: 'published' | 'authoring';
  versionId?: string;
  baseRevisionId?: string;
  targetRevisionId?: string;
  returnTo?: string;
  workflow?: VersionWorkflow;
};

const ajv = new Ajv({ coerceTypes: true, useDefaults: true, removeAdditional: 'all' });
const uuid = '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$';

const validateList = ajv.compile<ListSearch>({
  type: 'object',
  additionalProperties: false,
  properties: {
    view: { type: 'string', enum: ['published', 'authoring', 'history'], default: 'published' },
    titleContains: { type: 'string', maxLength: 1024 },
    folderId: { type: 'string', pattern: uuid },
    includeDescendants: { type: 'boolean', default: false },
    sort: { type: 'string', enum: ['created_at_desc', 'title_asc', 'published_at_desc'], default: 'created_at_desc' },
    pageSize: { type: 'integer', minimum: 1, maximum: 200, default: 50 },
    cursor: { type: 'string', minLength: 1, maxLength: 4096 },
    selectedDocumentId: { type: 'string', pattern: uuid },
    panel: { type: 'string', enum: ['open', 'closed'], default: 'open' },
  },
});

const validateDetail = ajv.compile<DetailSearch>({
  type: 'object',
  additionalProperties: false,
  properties: {
    tab: { type: 'string', enum: ['overview', 'versions', 'compare', 'history', 'access'], default: 'overview' },
    view: { type: 'string', enum: ['published', 'authoring'], default: 'published' },
    versionId: { type: 'string', pattern: uuid },
    baseRevisionId: { type: 'string', pattern: uuid },
    targetRevisionId: { type: 'string', pattern: uuid },
    returnTo: { type: 'string', maxLength: 2048, pattern: '^/documents(?:\\?.*)?$' },
    workflow: { type: 'string', enum: ['newVersion', 'publication'] },
  },
});

function sourceRecord(value: unknown): Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
    ? { ...(value as Record<string, unknown>) }
    : {};
}

export function validateListSearch(value: unknown): ListSearch {
  const candidate = sourceRecord(value);
  if (!validateList(candidate)) return defaultListSearch();
  if (candidate.titleContains === '') delete candidate.titleContains;
  const hasExplicitSort = Object.prototype.hasOwnProperty.call(sourceRecord(value), 'sort');
  const result = candidate as ListSearch;
  if (!hasExplicitSort) result.sort = result.view === 'published' ? 'published_at_desc' : 'created_at_desc';
  if (result.view !== 'published' && result.sort === 'published_at_desc') result.sort = 'created_at_desc';
  return result;
}

export function validateDetailSearch(value: unknown): DetailSearch {
  const candidate = sourceRecord(value);
  if (!validateDetail(candidate)) {
    return { tab: 'overview', view: 'published' };
  }
  const result = candidate as DetailSearch;
  if (result.tab !== 'versions') delete result.workflow;
  return result;
}

export function defaultListSearch(): ListSearch {
  return {
    view: 'published',
    includeDescendants: false,
    sort: 'published_at_desc',
    pageSize: 50,
    panel: 'open',
  };
}
