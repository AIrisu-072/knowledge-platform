// Shared immutable validation schemas. Runtime validators are generated at build preparation.
const uuid = '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$';

const listSchema = {
  type: 'object',
  additionalProperties: false,
  properties: {
    view: { type: 'string', enum: ['published', 'authoring', 'history'], default: 'published' },
    titleContains: { type: 'string', maxLength: 1024 },
    // Retain invalid URL text so the form can explain byte/control errors and stop GET.
    documentType: { type: 'string' },
    owningDepartment: { type: 'string' },
    category: { type: 'string' },
    folderId: { type: 'string', pattern: uuid },
    includeDescendants: { type: 'boolean', default: false },
    sort: { type: 'string', enum: ['created_at_desc', 'title_asc', 'published_at_desc'], default: 'created_at_desc' },
    pageSize: { type: 'integer', minimum: 1, maximum: 200, default: 50 },
    cursor: { type: 'string', minLength: 1, maxLength: 4096 },
    selectedDocumentId: { type: 'string', pattern: uuid },
    panel: { type: 'string', enum: ['open', 'closed'], default: 'open' },
  },
};

const detailSchema = {
  type: 'object',
  additionalProperties: false,
  properties: {
    tab: { type: 'string', enum: ['overview', 'versions', 'compare', 'history', 'access'], default: 'overview' },
    view: { type: 'string', enum: ['published', 'authoring'], default: 'published' },
    versionId: { type: 'string', pattern: uuid },
    baseRevisionId: { type: 'string', pattern: uuid },
    targetRevisionId: { type: 'string', pattern: uuid },
    returnTo: { type: 'string', maxLength: 81920, pattern: '^/documents(?:\\?.*)?$' },
    workflow: { type: 'string', enum: ['newVersion', 'publication'] },
  },
};

module.exports = { listSchema, detailSchema };
