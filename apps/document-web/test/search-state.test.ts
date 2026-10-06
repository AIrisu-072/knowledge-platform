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

test.each([
  '   ', 'Case', 'e\u0301', 'é', '123', 'true', 'null', '[1]', 'quote"\\+/?%_',
  'a'.repeat(1024), '日'.repeat(341) + 'a', '😀'.repeat(256),
  '日'.repeat(342), 'a\u0085b',
])('metadata URL state preserves exact text for display and explicit validation: %p', async value => {
  const { validateListSearch } = await import('../src/application/search-state');
  expect(validateListSearch({ view: 'authoring', titleContains: 'keep', documentType: value, owningDepartment: value, category: value }))
    .toMatchObject({ view: 'authoring', titleContains: 'keep', documentType: value, owningDepartment: value, category: value });
});

test('metadata empty fields alone are omitted while other URL state remains', async () => {
  const { validateListSearch } = await import('../src/application/search-state');
  const search = validateListSearch({ documentType: '', owningDepartment: '', category: '', titleContains: 'keep', cursor: 'cursor' });
  expect(search).toMatchObject({ titleContains: 'keep', cursor: 'cursor' });
  expect(search).not.toHaveProperty('documentType');
  expect(search).not.toHaveProperty('owningDepartment');
  expect(search).not.toHaveProperty('category');
});

test('returnTo accepts exactly 80KiB and rejects larger, external and different paths', async () => {
  const { validateDetailSearch } = await import('../src/application/search-state');
  const boundary = '/documents?x=' + 'x'.repeat(81920 - '/documents?x='.length);
  expect(validateDetailSearch({ returnTo: boundary }).returnTo).toBe(boundary);
  for (const returnTo of [boundary + 'x', 'https://example.invalid/documents', '//example.invalid/documents', '/documents/other', '/tasks']) {
    expect(validateDetailSearch({ returnTo }).returnTo).toBeUndefined();
  }
});


test.each(['documentType', 'owningDepartment', 'category'])('route rejects isolated surrogate in %s before router URI serialization', async key => {
  const { validateListSearch } = await import('../src/application/search-state');
  expect(() => validateListSearch({ [key]: '\ud800', titleContains: 'keep' })).toThrow('に不正なUnicode文字が含まれています。入力を確認してください。');
});


test.each([123, true, false, null, [1], { value: 'synthetic' }])('non-string metadata URL input fails closed instead of becoming an unspecified filter: %p', async value => {
  const { validateListSearch } = await import('../src/application/search-state');
  expect(() => validateListSearch({ documentType: value, titleContains: 'keep' })).toThrow('文書種別は文字列で指定してください。URLの条件を確認してください。');
});

test.each([
  [{ documentType: 'a\u0001b', titleContains: 'keep', pageSize: 0 }, 'documentType', '文書種別に制御文字を含めることはできません。'],
  [{ category: 'a'.repeat(1025), titleContains: 'keep', cursor: '' }, 'category', 'カテゴリは1024 UTF-8 bytes以下で入力してください。'],
])('old schema failure keeps invalid metadata for fail-closed form validation: %p', async (input, key, reason) => {
  const { validateListSearch, defaultListSearch } = await import('../src/application/search-state');
  const { metadataFilterValidation } = await import('../src/application/document-metadata-filters');
  const result = validateListSearch(input);
  expect(result).toEqual({ ...defaultListSearch(), [key as string]: input[key as keyof typeof input] });
  expect(metadataFilterValidation(result)).toBe(reason);
});

test.each([{ pageSize: 0 }, { cursor: '' }, { sort: 'invalid' }, { view: 'invalid' }])('old schema fallback retains valid exact metadata only: %p', async invalidOldCondition => {
  const { validateListSearch, defaultListSearch } = await import('../src/application/search-state');
  const metadata = { documentType: '123', owningDepartment: '   ', category: 'e\u0301' };
  expect(validateListSearch({ ...metadata, titleContains: 'keep', ...invalidOldCondition })).toEqual({ ...defaultListSearch(), ...metadata });
});

test.each([{ pageSize: 0 }, { cursor: '' }])('empty metadata stays omitted and metadata-free old fallback stays unchanged: %p', async invalidOldCondition => {
  const { validateListSearch, defaultListSearch } = await import('../src/application/search-state');
  expect(validateListSearch({ titleContains: 'keep', ...invalidOldCondition })).toEqual(defaultListSearch());
  expect(validateListSearch({ documentType: '', owningDepartment: '', category: '', titleContains: 'keep', ...invalidOldCondition })).toEqual(defaultListSearch());
});

test('unread is optional and only true remains an applied published condition', async () => {
  const { validateListSearch, defaultListSearch } = await import('../src/application/search-state');
  expect(defaultListSearch()).not.toHaveProperty('unreadOnly');
  expect(validateListSearch({})).not.toHaveProperty('unreadOnly');
  expect(validateListSearch({ unreadOnly: false })).not.toHaveProperty('unreadOnly');
  expect(validateListSearch({ unreadOnly: true })).toHaveProperty('unreadOnly', true);
});

test.each(['', 0, 1, null, 'true', 'false', ' ', [], [true], {}])('unread rejects non-booleans before AJV coercion: %p', async unreadOnly => {
  const { validateListSearch } = await import('../src/application/search-state');
  expect(() => validateListSearch({ unreadOnly })).toThrow('未読のみはtrueまたはfalseで指定してください。URLの条件を確認してください。');
});

test.each(['authoring', 'history', 'invalid', '', null])('any unread key is invalid outside raw published scope: %p', async view => {
  const { validateListSearch } = await import('../src/application/search-state');
  for (const unreadOnly of [true, false]) expect(() => validateListSearch({ view, unreadOnly })).toThrow('未読のみは公開一覧でのみ指定できます。URLの条件を確認してください。');
});

test.each([{ pageSize: 0 }, { cursor: '' }, { sort: 'invalid' }])('old-condition fallback preserves validated unread and metadata: %p', async invalid => {
  const { validateListSearch, defaultListSearch } = await import('../src/application/search-state');
  expect(validateListSearch({ unreadOnly: true, documentType: 'keep', ...invalid })).toEqual({ ...defaultListSearch(), unreadOnly: true, documentType: 'keep' });
});
