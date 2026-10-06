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


const createdFields = ['createdFrom', 'createdBefore'] as const;
test.each(['published', 'authoring', 'history'])('created range preserves both raw instants in %s', async view => {
  const { validateListSearch } = await import('../src/application/search-state');
  const range = { createdFrom: '2026-10-01T00:00:00.123456+09:00', createdBefore: '2026-10-02t00:00:00z' };
  expect(validateListSearch({ view, ...range, documentType: 'keep' })).toMatchObject({ view, ...range, documentType: 'keep' });
});

test.each(createdFields)('created range omits only empty %s and keeps the opposite endpoint', async key => {
  const { validateListSearch } = await import('../src/application/search-state');
  const other = key === 'createdFrom' ? 'createdBefore' : 'createdFrom';
  const search = validateListSearch({ [key]: '', [other]: ' ', unreadOnly: true });
  expect(search).not.toHaveProperty(key);
  expect(search).toHaveProperty(other, ' ');
});

test.each(createdFields)('created %s rejects type, control, invalid Unicode and >128 UTF8 bytes before AJV', async key => {
  const { validateListSearch } = await import('../src/application/search-state');
  const label = key === 'createdFrom' ? '作成日時の開始（含む）' : '作成日時の終了（含まない）';
  for (const value of [null, 123, true, [], {}]) expect(() => validateListSearch({ [key]: value, pageSize: 0 })).toThrow(`${label}は文字列で指定してください。URLの条件を確認してください。`);
  for (const value of ['a\u0001b', 'a\u0085b']) expect(() => validateListSearch({ [key]: value })).toThrow(`${label}に制御文字を含めることはできません。`);
  for (const value of ['\ud800', '\udfff']) expect(() => validateListSearch({ [key]: value })).toThrow(`${label}に不正なUnicode文字が含まれています。入力を確認してください。`);
  for (const value of ['a'.repeat(129), '日'.repeat(43), '😀'.repeat(33)]) expect(() => validateListSearch({ [key]: value })).toThrow(`${label}は128 UTF-8 bytes以下で入力してください。`);
  for (const value of ['a'.repeat(128), '日'.repeat(42)+'ab', '😀'.repeat(32), '2026-02-30T00:00:00Z', '2016-12-31T23:59:60Z']) expect(validateListSearch({ [key]: value })).toHaveProperty(key, value);
});

test.each([{ pageSize: 0 }, { cursor: '' }, { sort: 'invalid' }, { view: 'invalid' }])('old schema fallback preserves created raw range alongside other groups: %p', async invalid => {
  const { validateListSearch, defaultListSearch } = await import('../src/application/search-state');
  const range = { createdFrom: 'not-an-instant', createdBefore: '2026-10-02T00:00:00.123Z' };
  expect(validateListSearch({ ...range, documentType: 'keep', ...invalid })).toEqual({ ...defaultListSearch(), ...range, documentType: 'keep' });
});


test('created minute display and draft resolution preserve raw pairs without authenticating dates', async () => {
  const { createdRangeLocalValue, initialCreatedRangeDraft, currentCreatedRangeDraft, resolveCreatedRangeDraft } = await import('../src/application/document-created-range');
  const range = { createdFrom: '2026-10-01T00:00:00.000Z', createdBefore: '2016-12-31T23:59:60Z' };
  expect(createdRangeLocalValue(range.createdFrom)).toBe('2026-10-01T09:00');
  expect(createdRangeLocalValue(range.createdBefore)).toBeNull();
  const kept = initialCreatedRangeDraft(range);
  expect(resolveCreatedRangeDraft(kept)).toEqual({ range, error: null });
  expect(resolveCreatedRangeDraft({ ...kept, createdFrom: { intent: 'clear' } })).toEqual({ range: { createdFrom: undefined, createdBefore: range.createdBefore }, error: null });
  expect(resolveCreatedRangeDraft({ ...kept, createdBefore: { intent: 'set', local: '2026-10-02T09:00' } })).toEqual({ range: { createdFrom: range.createdFrom, createdBefore: '2026-10-02T00:00:00.000Z' }, error: null });
  for (const local of ['', '2026-02-30T09:00', '2026-10-02T09:00:01', 'bad']) expect(resolveCreatedRangeDraft({ ...kept, createdBefore: { intent: 'set', local } })).toEqual({ error: '作成日時の終了（含まない）をJSTのカレンダーと時刻で指定してください。' });
  const unsent = { ...kept, createdFrom: { intent: 'set' as const, local: '2026-10-09T09:00' } };
  expect(currentCreatedRangeDraft(range, unsent)).toBe(unsent);
  const changed = { ...range, createdBefore: '2026-10-03T00:00:00.000Z' };
  // Simulate the render before effects: stale draft is already unusable for the new pair.
  expect(resolveCreatedRangeDraft(currentCreatedRangeDraft(changed, unsent))).toEqual({ range: changed, error: null });
  for (const raw of ['2026-10-01T00:00:00Z', '2026-10-01T00:00:00.001Z', '2026-10-01T09:00:00.000+09:00', '2026-02-30T00:00:00.000Z', 'invalid', '+275760-09-13T00:00:00.000Z']) expect(createdRangeLocalValue(raw)).toBeNull();
});


test('known maximum list conditions fit the unchanged returnTo bound with real router serialization', async () => {
  const { defaultParseSearch, defaultStringifySearch } = await import('@tanstack/react-router');
  const { validateListSearch, validateDetailSearch } = await import('../src/application/search-state');
  const { documentListUrlError } = await import('../src/application/document-created-range');
  const uuid = '00000000-0000-4000-8000-000000000015';
  for (const expanded of ['"'.repeat(1024), '\\'.repeat(1024), '😀'.repeat(256)]) {
    const createdText = expanded.startsWith('😀') ? '😀'.repeat(32) : expanded.slice(0, 128);
    const conditions = { view: 'published', titleContains: '😀'.repeat(1024), cursor: '😀'.repeat(4096),
      documentType: expanded, owningDepartment: expanded, category: expanded,
      createdFrom: createdText, createdBefore: createdText, unreadOnly: true,
      folderId: uuid, includeDescendants: true, sort: 'published_at_desc', pageSize: 200, selectedDocumentId: uuid, panel: 'closed' };
    const validated = validateListSearch(conditions);
    const serialized = defaultStringifySearch(validated);
    const returnTo = '/documents' + serialized;
    expect(returnTo.length).toBeLessThanOrEqual(81760);
    expect(documentListUrlError(returnTo)).toBeNull();
    expect(defaultParseSearch(serialized)).toEqual(validated);
    expect(validateDetailSearch({ returnTo })).toHaveProperty('returnTo', returnTo);
  }
  const boundary = '/documents?x=' + 'x'.repeat(81920 - '/documents?x='.length);
  expect(documentListUrlError(boundary)).toBeNull();
  expect(documentListUrlError(boundary+'x')).toBe('一覧のURLが81920文字を超えています。URLの条件を短くして再度お試しください。');
});
