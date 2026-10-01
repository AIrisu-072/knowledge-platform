import { validateList, validateDetail } from './search-validators.generated.js';

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
