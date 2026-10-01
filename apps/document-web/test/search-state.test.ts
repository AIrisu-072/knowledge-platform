test('list search has stable defaults and drops unsupported URL state', async () => {
  const { validateListSearch } = await import('../src/application/search-state');

  expect(validateListSearch({})).toMatchObject({ view: 'published', sort: 'published_at_desc', panel: 'open' });
  expect(validateListSearch({ view: 'authoring' })).toMatchObject({ view: 'authoring', sort: 'created_at_desc' });
  expect(validateListSearch({ view: 'history', titleContains: ' policy ', pageSize: '25', ignored: 'x' })).toMatchObject({
    view: 'history',
    titleContains: ' policy ',
    pageSize: 25,
    sort: 'created_at_desc',
  });
  expect(validateListSearch({ view: 'other', panel: 'unexpected' })).toMatchObject({
    view: 'published',
    panel: 'open',
    pageSize: 50,
  });
});

test('detail search only accepts local document-workspace return paths', async () => {
  const { validateDetailSearch } = await import('../src/application/search-state');

  expect(validateDetailSearch({ tab: 'versions', returnTo: '/documents?view=authoring' })).toMatchObject({
    tab: 'versions',
    returnTo: '/documents?view=authoring',
  });
  expect(validateDetailSearch({ tab: 'other', returnTo: 'https://example.invalid' })).toMatchObject({
    tab: 'overview',
  });
  expect(validateDetailSearch({ returnTo: '//example.invalid' }).returnTo).toBeUndefined();
});

test('detail search preserves supported version workflows and drops workflow state from other tabs', async () => {
  const { validateDetailSearch } = await import('../src/application/search-state');

  expect(validateDetailSearch({ tab: 'versions', workflow: 'newVersion', view: 'authoring' })).toMatchObject({
    tab: 'versions',
    workflow: 'newVersion',
    view: 'authoring',
  });
  const overviewSearch = validateDetailSearch({ tab: 'overview', workflow: 'publication', view: 'published' });
  expect(overviewSearch).toMatchObject({
    tab: 'overview',
    view: 'published',
  });
  expect(overviewSearch).not.toHaveProperty('workflow');
});
