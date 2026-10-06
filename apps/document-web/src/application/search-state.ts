import { validateList, validateDetail } from './search-validators.generated.js';
import { unreadFilterRouteError } from './document-unread-filter';
import { metadataFilterFields, metadataFilterRouteError } from './document-metadata-filters';

export type DocumentView = 'published' | 'authoring' | 'history';
export type DocumentDetailTab = 'overview' | 'versions' | 'compare' | 'history' | 'access';
export type VersionWorkflow = 'newVersion' | 'publication';
export type ListSearch = {
  view: DocumentView;
  titleContains?: string;
  unreadOnly?: boolean;
  documentType?: string;
  owningDepartment?: string;
  category?: string;
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
  // The default router serializer replaces lone surrogates. Stop before it changes exact-match text.
  const routeError = metadataFilterRouteError(candidate) ?? unreadFilterRouteError(candidate);
  if (routeError) throw new Error(routeError);
  if (!validateList(candidate)) {
    const fallback = defaultListSearch();
    // Old-condition fallback must not discard exact metadata or enable a broader GET.
    for (const { key } of metadataFilterFields) {
      const metadata = candidate[key];
      if (typeof metadata === 'string' && metadata !== '') fallback[key] = metadata;
    }
    if (candidate.unreadOnly === true) fallback.unreadOnly = true;
    return fallback;
  }
  for (const key of ['titleContains', 'documentType', 'owningDepartment', 'category'] as const) {
    if (candidate[key] === '') delete candidate[key];
  }
  if (candidate.unreadOnly !== true) delete candidate.unreadOnly;
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
