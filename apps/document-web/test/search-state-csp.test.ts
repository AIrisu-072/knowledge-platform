
test('route validation initializes and operates without runtime string code generation', () => {
  jest.isolateModules(() => {
    const original = globalThis.Function;
    globalThis.Function = function () { throw new EvalError('String code generation disabled by CSP'); } as unknown as FunctionConstructor;
    try {
      const state = require('../src/application/search-state') as typeof import('../src/application/search-state');
      expect(state.validateListSearch({ pageSize: '25', includeDescendants: 'true', extra: 'remove' })).toEqual({
        view: 'published', pageSize: 25, includeDescendants: true, sort: 'published_at_desc', panel: 'open',
      });
      expect(state.validateDetailSearch({ tab: 'versions', workflow: 'publication' })).toEqual({ tab: 'versions', workflow: 'publication', view: 'published' });
    } finally { globalThis.Function = original; }
  });
});

test('standalone validators preserve Ajv coercion, defaults, pruning and boundary outcomes', () => {
  const Ajv = require('ajv');
  const { listSchema, detailSchema } = require('../src/application/search-schemas.cjs');
  const generated = require('../src/application/search-validators.generated.js');
  const ajv = new Ajv({ coerceTypes: true, useDefaults: true, removeAdditional: 'all' });
  const samples = [
    {}, { ignored: 'remove' }, { view: 'history', pageSize: '25', includeDescendants: 'true' },
    { pageSize: 0 }, { pageSize: 200 }, { pageSize: 201 }, { pageSize: 'bad' },
    { folderId: '00000000-0000-0000-0000-000000000000' }, { folderId: 'invalid' },
    { titleContains: '日'.repeat(1024) }, { titleContains: '日'.repeat(1025) },
    { cursor: '' }, { cursor: 'x'.repeat(4096) }, { cursor: 'x'.repeat(4097) },
    { panel: 'closed' }, { panel: 'invalid' }, { tab: 'versions', workflow: 'publication' },
    { returnTo: '/documents?view=published' }, { returnTo: 'https://example.invalid/' },
    { tab: 'unknown', view: 'history' }, null, [], 'invalid',
  ];
  for (const [schema, standalone] of [[listSchema, generated.validateList], [detailSchema, generated.validateDetail]]) {
    const reference = ajv.compile(schema);
    for (const sample of samples) {
      const actual = JSON.parse(JSON.stringify(sample)), expected = JSON.parse(JSON.stringify(sample));
      expect(standalone(actual)).toBe(reference(expected));
      expect(actual).toEqual(expected);
      expect(standalone.errors).toEqual(reference.errors);
    }
  }
});
