import { expect, test, type Page } from '@playwright/test';
import { tokenContrastRatio } from './token-contrast';

const documentId = '00000000-0000-4000-8000-000000000010';
const versionId = '00000000-0000-4000-8000-000000000011';
const baseVersionId = '00000000-0000-4000-8000-000000000012';
const revisionId = '00000000-0000-4000-8000-000000000013';
const newerRevisionId = '00000000-0000-4000-8000-000000000014';
const folderId = '00000000-0000-4000-8000-000000000015';
const generalFolderId = '00000000-0000-4000-8000-000000000016';
const loanFolderId = '00000000-0000-4000-8000-000000000017';
const reviewFolderId = '00000000-0000-4000-8000-000000000018';
const manualFolderId = '00000000-0000-4000-8000-000000000019';
const regulationFolderId = '00000000-0000-4000-8000-000000000020';
const noticeFolderId = '00000000-0000-4000-8000-000000000021';

const available = { status: 'available' };
const permissionDenied = { status: 'disabled', reason: 'permission' };

function versionSummary(lifecycleState: 'WORKING' | 'PUBLISHED', versionNo: number) {
  return {
    versionId: lifecycleState === 'WORKING' ? versionId : baseVersionId,
    versionNo,
    baseVersionId: null,
    lifecycleState,
    isCurrent: lifecycleState === 'PUBLISHED',
    approvedAt: lifecycleState === 'PUBLISHED' ? '2026-09-30T03:00:00Z' : null,
    scheduledPublishAt: null,
    publishedAt: lifecycleState === 'PUBLISHED' ? '2026-09-30T03:00:00Z' : null,
    withdrawnAt: null,
    updatedAt: '2026-10-01T01:00:00Z',
    fileSummary: {
      authoritativeItemCount: 1,
      totalSizeBytes: 12,
      primary: { displayName: 'source.txt', mediaType: 'text/plain', sizeBytes: 12 },
    },
  };
}

function revision(id: string, documentVersionId: string, major: number) {
  return {
    revisionId: id,
    documentVersionId,
    major,
    minor: 0,
    label: `${major}.0`,
    createdAt: major === 1 ? '2026-09-30T03:00:00Z' : '2026-10-01T03:00:00Z',
    sourceKind: major === 1 ? 'initialPublication' : 'contentPublication',
    metadataSnapshotStatus: 'complete',
  };
}

function mockDocument(view: 'published' | 'authoring') {
  const working = view === 'authoring';
  const currentVersion = versionSummary(working ? 'WORKING' : 'PUBLISHED', working ? 3 : 2);
  const common = {
    documentId,
    documentVersionId: working ? versionId : baseVersionId,
    title: '受入手順',
    folderId: view === 'published' ? reviewFolderId : null,
    folderName: view === 'published' ? '審査' : null,
    revision: 7,
    metadata: { category: 'operations', owner: '総務' },
    createdAt: '2026-09-01T00:00:00Z',
    displayVersion: currentVersion,
    displayRevision: working ? null : revision(revisionId, baseVersionId, 1),
    readState: { isRead: true, firstReadAt: '2026-09-30T03:00:00Z' },
    displayTimestamp: {
      kind: working ? 'workingUpdatedAt' : 'revisionCreatedAt',
      value: '2026-10-01T01:00:00Z',
    },
    capabilities: {
      createVersion: available,
      updateMetadata: permissionDenied,
      moveDocument: permissionDenied,
      endPublication: permissionDenied,
      manageAccess: available,
      compareVersions: available,
    },
  };
  return working
    ? { ...common, currentVersionId: null, lifecycleState: 'working' }
    : { ...common, currentVersionId: baseVersionId, unread: false, publishedAt: '2026-09-30T03:00:00Z' };
}

function mockVersion() {
  return {
    versionId,
    versionNo: 3,
    baseVersionId,
    lifecycleState: 'working',
    isCurrent: false,
    createdAt: '2026-09-30T00:00:00Z',
    approvedAt: null,
    scheduledPublishAt: null,
    publishedAt: null,
    withdrawnAt: null,
    updatedAt: '2026-10-01T01:00:00Z',
    fileSummary: {
      authoritativeItemCount: 1,
      totalSizeBytes: 12,
      primary: { displayName: 'source.txt', mediaType: 'text/plain', sizeBytes: 12 },
    },
    firstReadAt: null,
    title: '受入手順',
    metadata: {},
    capabilities: {
      edit: available,
      rebase: permissionDenied,
      publish: available,
      withdraw: permissionDenied,
      schedulePublication: available,
      cancelPublicationSchedule: permissionDenied,
      download: available,
    },
  };
}

function mockComparison() {
  const evidence = {
    documentId,
    versionId: baseVersionId,
    contentItemId: 'content-id',
    representationId: 'representation-id',
    fileId: 'file-id',
    rawSha256: 'sha256',
    inspectionProfile: 'dsi-v0',
    locator: { kind: 'textSpan', line: 14, byteStart: 0, byteEnd: 20 },
    granularity: 'exact',
    parserProvenance: 'adapter',
  };
  return {
    projection: 'display',
    baseRevision: { revisionId, documentVersionId: baseVersionId, major: 1, minor: 0, createdAt: '2026-09-30T03:00:00Z' },
    targetRevision: { revisionId: newerRevisionId, documentVersionId: versionId, major: 2, minor: 0, createdAt: '2026-10-01T03:00:00Z' },
    contentComparisonStatus: 'differentAuthoritativeVersions',
    verdict: 'unknown',
    coverage: 'partial',
    resultDigest: 'digest',
    changes: [],
    rows: [],
    unverifiedRegions: [{ reason: 'unsupportedSemanticConstruct', base: evidence, target: { ...evidence, versionId }, navigationHint: '14行目の原本を確認' }],
    ancillaryChanges: [],
    metadataComparisonStatus: 'unavailableLegacy',
    metadataChanges: [],
    baseMetadataSnapshotDigest: null,
    targetMetadataSnapshotDigest: null,
    auditEventId: 'audit-1',
    displayItems: [],
    pageSize: 50,
    nextCursor: null,
  };
}

async function installApi(page: Page, publishDelayMs = 0) {
  const state = {
    requests: [] as string[],
    publishCalls: 0,
    scheduleCalls: 0,
    scheduleBody: undefined as Record<string, unknown> | undefined,
  };
  await page.route('**/v1/**', async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    const method = request.method();
    state.requests.push(`${method} ${url.pathname}${url.search}`);
    let body: unknown;

    if (method === 'GET' && url.pathname === '/v1/folders/root') {
      body = { folderId, name: '全文書', revision: 1, parentFolderId: null, capabilities: {} };
    } else if (method === 'GET' && url.pathname === `/v1/folders/${folderId}/children`) {
      body = { items: [
        { folderId: generalFolderId, name: '総務部', revision: 1, parentFolderId: folderId, capabilities: {} },
        { folderId: loanFolderId, name: '融資部', revision: 2, parentFolderId: folderId, capabilities: {} },
      ], nextCursor: null, capabilities: {} };
    } else if (method === 'GET' && url.pathname === `/v1/folders/${generalFolderId}/children`) {
      body = { items: [
        { folderId: regulationFolderId, name: '規程', revision: 1, parentFolderId: generalFolderId, capabilities: {} },
        { folderId: noticeFolderId, name: '通達', revision: 1, parentFolderId: generalFolderId, capabilities: {} },
      ], nextCursor: null, capabilities: {} };
    } else if (method === 'GET' && url.pathname === `/v1/folders/${loanFolderId}/children`) {
      body = { items: [
        { folderId: reviewFolderId, name: '審査', revision: 3, parentFolderId: loanFolderId, capabilities: {} },
        { folderId: manualFolderId, name: 'マニュアル', revision: 1, parentFolderId: loanFolderId, capabilities: {} },
      ], nextCursor: null, capabilities: {} };
    } else if (method === 'GET' && url.pathname === '/v1/documents') {
      const view = url.searchParams.get('view') === 'authoring' ? 'authoring' : 'published';
      const working = view === 'authoring';
      body = {
        view,
        items: [{
          ...mockDocument(view),
          displayVersion: versionSummary(working ? 'WORKING' : 'PUBLISHED', working ? 3 : 2),
          displayRevision: working ? null : revision(revisionId, baseVersionId, 1),
        }],
        nextCursor: null,
      };
    } else if (method === 'GET' && url.pathname === `/v1/documents/${documentId}`) {
      body = mockDocument(url.searchParams.get('view') === 'authoring' ? 'authoring' : 'published');
    } else if (method === 'GET' && url.pathname === `/v1/documents/${documentId}/versions`) {
      body = { items: [mockVersion(), { ...mockVersion(), versionId: baseVersionId, versionNo: 2, lifecycleState: 'published', isCurrent: true }], nextCursor: null };
    } else if (method === 'GET' && url.pathname === `/v1/documents/${documentId}/versions/${versionId}`) {
      body = mockVersion();
    } else if (method === 'GET' && url.pathname === `/v1/documents/${documentId}/versions/${baseVersionId}`) {
      body = { ...mockVersion(), versionId: baseVersionId, versionNo: 2, lifecycleState: 'PUBLISHED', isCurrent: true };
    } else if (method === 'GET' && url.pathname === `/v1/documents/${documentId}/revisions`) {
      body = {
        items: [revision(newerRevisionId, versionId, 2), revision(revisionId, baseVersionId, 1)],
        nextCursor: null,
      };
    } else if (method === 'GET' && [versionId, baseVersionId].some((id) => url.pathname === `/v1/documents/${documentId}/versions/${id}/files`)) {
      body = { items: [{ contentItemId: 'content-id', representationId: 'representation-id', logicalPath: 'source.txt', ordinal: 0, role: 'primary', displayName: 'source.txt', mediaType: 'text/plain', sizeBytes: 12 }] };
    } else if (method === 'GET' && url.pathname === `/v1/documents/${documentId}/history`) {
      body = { items: [], nextCursor: null };
    } else if (method === 'GET' && url.pathname === `/v1/documents/${documentId}/access-policy`) {
      body = {
        target: { kind: 'document', id: documentId },
        bindingMode: 'explicit',
        policyId: 'document-policy',
        policyRevision: 3,
        effectivePolicyId: 'document-policy',
        effectiveSource: { kind: 'document', id: documentId },
        effectiveGrants: [{
          subjectKind: 'role', identityProvider: 'directory', subjectId: 'loan-reviewers', actions: ['read', 'readHistory', 'write'],
          presentation: { ref: { provider: 'directory', kind: 'role', subjectId: 'loan-reviewers' }, displayName: '融資審査担当', secondaryText: 'グループ', resolution: 'resolved' },
        }, {
          subjectKind: 'role', identityProvider: 'directory', subjectId: 'audit-reviewers', actions: ['read', 'readHistory'],
          presentation: { ref: { provider: 'directory', kind: 'role', subjectId: 'audit-reviewers' }, displayName: '監査担当', secondaryText: 'グループ', resolution: 'resolved' },
        }],
      };
    } else if (method === 'POST' && url.pathname === `/v1/documents/${documentId}/revision-comparisons`) {
      body = mockComparison();
    } else if (method === 'POST' && url.pathname.endsWith(':schedule-publication')) {
      state.scheduleCalls += 1;
      state.scheduleBody = request.postDataJSON() as Record<string, unknown>;
      await new Promise((resolve) => setTimeout(resolve, publishDelayMs));
      body = {
        scheduleOperationId: '00000000-0000-4000-8000-000000000099',
        documentId,
        documentVersionId: versionId,
        resultingDocumentRevision: 8,
        scheduledPublishAt: state.scheduleBody?.scheduledPublishAt,
      };
    } else if (method === 'POST' && url.pathname.endsWith(':publish')) {
      state.publishCalls += 1;
      await new Promise((resolve) => setTimeout(resolve, publishDelayMs));
      body = {
        publishOperationId: '00000000-0000-4000-8000-000000000099',
        documentId,
        documentVersionId: versionId,
        resultingDocumentRevision: 8,
        publishedAt: '2026-10-01T02:00:00Z',
      };
    } else {
      await route.fulfill({ status: 404, json: { code: 'NOT_FOUND', title: 'Not found' } });
      return;
    }

    await route.fulfill({ status: 200, json: body });
  });
  return state;
}

test('detail route code is loaded after the document list is usable', async ({ page }) => {
  const scriptPaths = new Set<string>();
  page.on('request', (request) => {
    if (request.resourceType() === 'script') scriptPaths.add(new URL(request.url()).pathname);
  });
  await installApi(page);
  await page.goto('/documents?view=published');
  await expect(page.getByRole('table', { name: '文書一覧' })).toBeVisible();
  const listScripts = new Set(scriptPaths);
  await page.getByRole('button', { name: '詳細を開く' }).click();
  await expect(page.getByRole('heading', { name: '受入手順', level: 1 })).toBeVisible();
  expect([...scriptPaths].some((path) => !listScripts.has(path))).toBe(true);
});

test('keyboard activation keeps list URL context and restores focus after returning', async ({ page }) => {
  const api = await installApi(page);
  await page.goto('/documents?view=authoring&titleContains=manual&sort=title_asc&pageSize=25');

  const navigation = page.getByRole('navigation', { name: 'メインナビゲーション' });
  await expect(navigation.getByRole('link', { name: '文書' })).toBeVisible();
  await expect(navigation.getByRole('link', { name: '編集作業' })).toBeVisible();
  await expect(page.getByRole('region', { name: 'フォルダー' })).toBeVisible();
  await expect(page.getByRole('columnheader', { name: '状態' })).toBeVisible();
  await expect(page.getByRole('columnheader', { name: 'フォルダー' })).toBeVisible();
  await expect(page.getByRole('columnheader', { name: '版' })).toBeVisible();
  const navigationTiming = await page.evaluate(() => {
    const entry = performance.getEntriesByType('navigation')[0] as PerformanceNavigationTiming;
    return { domInteractiveMs: entry.domInteractive, loadEventEndMs: entry.loadEventEnd };
  });
  const tUsableMs = await page.evaluate(() => performance.now());
  const row = page.getByRole('button', { name: /受入手順/ });
  await expect(row).toBeVisible();
  const inputStartedAt = await page.evaluate(() => performance.now());
  await row.focus();
  await page.keyboard.press('Enter');
  await expect(page.getByRole('complementary', { name: '選択中の文書' })).toContainText('受入手順');
  const tInputMs = await page.evaluate((startedAt) => performance.now() - startedAt, inputStartedAt);
  await page.getByRole('button', { name: '詳細を開く' }).press('Enter');
  await expect(page.getByRole('heading', { name: '受入手順', level: 1 })).toBeVisible();

  await page.getByRole('button', { name: /一覧へ戻る/ }).press('Enter');
  await expect(page).toHaveURL(/\/documents\?.*view=authoring/);
  await expect(page.getByRole('button', { name: /受入手順/ })).toBeFocused();
  expect(api.requests.some((request) => request.includes('titleContains=manual'))).toBe(true);
  const motionSpatial = await page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue('--motion-spatial').trim());
  await test.info().attach('gui-performance.json', {
    body: Buffer.from(JSON.stringify({ tUsableMs, tInputMs, motionSpatial, ...navigationTiming }, null, 2)),
    contentType: 'application/json',
  });
});

test('publication workspace requires confirmation and reports success only after the API response', async ({ page }) => {
  const api = await installApi(page, 300);
  await page.goto(`/documents/${documentId}?view=authoring&tab=versions`);
  const publish = page.getByRole('button', { name: '公開する', exact: true });
  await expect(publish).toBeVisible();
  await publish.click();

  await expect(page).toHaveURL(/workflow=publication/);
  await expect(page.getByRole('heading', { name: '公開・予約公開', level: 1 })).toBeVisible();
  await expect(page.getByRole('tablist', { name: '文書の詳細' })).toHaveCount(0);
  await expect(page.getByRole('complementary', { name: '原本と版' })).toHaveCount(0);
  await expect(page.getByText('現在', { exact: true })).toBeVisible();
  await expect(page.getByText('公開対象', { exact: true })).toBeVisible();
  const acknowledge = page.getByRole('checkbox', { name: '公開対象の版とファイルを確認しました。' });
  await expect(acknowledge).not.toBeChecked();
  await expect(publish).toBeDisabled();
  await acknowledge.check();
  await publish.click();

  const dialog = page.getByRole('dialog', { name: '公開を確認' });
  await expect(dialog).toBeVisible();
  expect(await dialog.evaluate((element) => element.contains(document.activeElement))).toBe(true);
  const cancel = dialog.getByRole('button', { name: 'キャンセル' });
  await cancel.focus();
  await page.keyboard.press('Tab');
  await expect(dialog.getByRole('button', { name: '確定する' })).toBeFocused();

  const publishRequest = page.waitForRequest((request) => request.method() === 'POST' && request.url().endsWith(':publish'));
  await page.keyboard.press('Enter');
  await publishRequest;
  await expect(dialog.getByRole('button', { name: '処理中…' })).toBeVisible();
  await expect(page.getByText(/公開しました/)).toHaveCount(0);
  await expect(page.getByRole('status')).toContainText('公開しました');
  await expect(publish).toBeFocused();
  expect(api.publishCalls).toBe(1);
});

test('scheduled publication converts the selected JST time to UTC', async ({ page }) => {
  const api = await installApi(page, 250);
  await page.goto(`/documents/${documentId}?view=authoring&tab=versions`);
  await page.getByRole('button', { name: '予約公開する', exact: true }).click();

  await expect(page).toHaveURL(/workflow=publication/);
  await expect(page.getByRole('radio', { name: /日時を指定/ })).toBeChecked();
  const scheduledAt = page.locator('input[type="datetime-local"]');
  await expect(scheduledAt).toBeVisible();
  await scheduledAt.fill('2026-10-02T09:30');
  await expect(page.getByRole('button', { name: '公開を予約する', exact: true })).toBeDisabled();
  await page.getByRole('checkbox', { name: '公開対象の版とファイルを確認しました。' }).check();
  const schedule = page.getByRole('button', { name: '公開を予約する', exact: true });
  await expect(schedule).toBeEnabled();
  await schedule.click();

  const dialog = page.getByRole('dialog', { name: '予約公開を確認' });
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText('2026/10/02 9:30');
  const request = page.waitForRequest((candidate) => candidate.method() === 'POST' && candidate.url().endsWith(':schedule-publication'));
  await dialog.getByRole('button', { name: '確定する' }).click();
  await request;
  await expect(dialog.getByRole('button', { name: '処理中…' })).toBeVisible();
  await expect(page.getByRole('status')).toContainText('公開を予約しました');
  expect(api.scheduleCalls).toBe(1);
  expect(api.scheduleBody?.scheduledPublishAt).toBe('2026-10-02T00:30:00.000Z');
});

test('reduced motion, key landmarks, token contrast, and 1280/1440 layouts meet the visual contract', async ({ page }) => {
  await installApi(page);
  await page.emulateMedia({ reducedMotion: 'reduce' });
  for (const width of [1280, 1440]) {
    await page.setViewportSize({ width, height: 900 });
    await page.goto('/documents?view=authoring');
    await expect(page.getByRole('main', { name: '文書ワークスペース' })).toBeVisible();
    await expect(page.getByRole('table', { name: '文書一覧' })).toBeVisible();
    await expect(page.getByRole('heading', { name: '編集作業', level: 1 })).toBeVisible();

    const audit = await page.evaluate(() => {
      const root = getComputedStyle(document.documentElement);
      const readColor = (token: string) => root.getPropertyValue(token).trim();
      const pairs = [
        ['--color-text', '--color-surface'],
        ['--color-text-muted', '--color-surface'],
        ['--color-text-faint', '--color-surface'],
        ['--color-accent', '--color-surface'],
        ['--color-success', '--color-accent-soft'],
        ['--color-warning', '--color-warning-soft'],
        ['--color-danger', '--color-danger-soft'],
      ] as const;
      const contrastColors = pairs.map(([foreground, background]) => [readColor(foreground), readColor(background)] as const);
      const unnamedButtons = Array.from(document.querySelectorAll('button')).filter((button) =>
        !button.getAttribute('aria-label') && !button.textContent?.trim(),
      ).length;
      return {
        width: document.documentElement.clientWidth,
        scrollWidth: document.documentElement.scrollWidth,
        language: document.documentElement.lang,
        headingCount: document.querySelectorAll('h1').length,
        unnamedButtons,
        contrastColors,
        spatialMotion: readColor('--motion-spatial'),
      };
    });
    expect(audit.scrollWidth).toBeLessThanOrEqual(audit.width);
    expect(audit.language).toBe('ja');
    expect(audit.headingCount).toBe(1);
    expect(audit.unnamedButtons).toBe(0);
    const contrastRatios = audit.contrastColors.map(([foreground, background]) => tokenContrastRatio(foreground, background));
    expect(contrastRatios.every((ratio) => ratio >= 4.5)).toBe(true);
    expect(audit.spatialMotion).toBe('0ms');
  }
});

test('Mock 1 through Mock 7 preserve the approved screens and core states', async ({ page }) => {
  await installApi(page);
  await page.setViewportSize({ width: 1440, height: 900 });
  const snapshot = async (name: string) => {
    if (process.platform === 'darwin') {
      await expect(page).toHaveScreenshot(name, { animations: 'disabled', caret: 'hide' });
    }
  };
  await page.goto(`/documents?view=published&folderId=${reviewFolderId}`);
  await page.getByRole('button', { name: '総務部の子フォルダーを開く' }).click();
  await page.getByRole('button', { name: '融資部の子フォルダーを開く' }).click();
  const reviewFolder = page.getByRole('button', { name: '審査', exact: true });
  await expect(reviewFolder).toHaveAttribute('aria-current', 'location');
  await page.getByRole('button', { name: /受入手順/ }).click();
  await expect(page.getByRole('table', { name: '文書一覧' })).toBeVisible();
  await expect(page.getByRole('complementary', { name: '選択中の文書' }).getByRole('button', { name: 'ファイルを取得' })).toBeVisible();
  await snapshot('mock-1-document-list.png');

  await page.goto(`/documents/${documentId}?view=published&tab=overview`);
  await expect(page.getByRole('navigation', { name: 'メインナビゲーション' }).getByRole('link', { name: '文書' })).toBeVisible();
  await expect(page.getByRole('complementary', { name: '原本と版' })).toBeVisible();
  await expect(page.getByRole('heading', { name: '基本情報' })).toBeVisible();
  await expect(page.getByRole('heading', { name: '基本情報' }).locator('xpath=..')).toHaveCSS('border-top-width', '0px');
  await expect(page.getByRole('complementary', { name: '原本と版' }).getByRole('button', { name: '現行ファイルを取得' })).toBeVisible();
  await snapshot('mock-2-document-detail.png');

  await page.goto(`/documents/${documentId}?view=authoring&tab=versions`);
  await expect(page.getByRole('heading', { name: 'コンテンツ版' })).toBeVisible();
  await expect(page.getByRole('button', { name: /WORKING · 版 3/ })).toBeVisible();
  await expect(page.getByRole('heading', { name: '正式改訂' })).toBeVisible();
  await snapshot('mock-3-revisions-versions.png');

  await page.getByRole('button', { name: '新しい版を作成', exact: true }).first().click();
  await expect(page.getByRole('heading', { name: '新しい版を作成', level: 1 })).toBeVisible();
  await expect(page.getByRole('complementary', { name: '原本と版' })).toHaveCount(0);
  await page.getByLabel('原本ファイル').setInputFiles({ name: 'new-policy.txt', mimeType: 'text/plain', buffer: Buffer.from('new policy') });
  await expect(page.getByText('new-policy.txt')).toBeVisible();
  await expect(page.getByRole('button', { name: '新しい版を作成', exact: true })).toBeEnabled();
  await snapshot('mock-4-new-version-selected.png');

  await page.getByRole('button', { name: '← 版の一覧へ戻る' }).click();
  await page.getByRole('button', { name: '公開する', exact: true }).click();
  await expect(page.getByRole('heading', { name: '公開・予約公開', level: 1 })).toBeVisible();
  await expect(page.getByRole('radio', { name: /今すぐ公開/ })).toBeChecked();
  await expect(page.getByLabel(/JST \/ UTC\+09:00/)).toHaveCount(0);
  await snapshot('mock-5-publication-ready.png');

  await page.goto(`/documents/${documentId}?view=authoring&tab=compare`);
  await expect(page.getByRole('heading', { name: '新旧比較', level: 1 })).toBeVisible();
  await expect(page.getByRole('complementary', { name: '原本と版' })).toHaveCount(0);
  await expect(page.getByText('不明')).toBeVisible();
  await expect(page.getByText('一部のみ')).toBeVisible();
  await expect(page.getByRole('heading', { name: /未比較範囲/ })).toBeVisible();
  await expect(page.getByRole('button', { name: '基準原本を確認' })).toBeVisible();
  await snapshot('mock-6-partial-comparison.png');

  await page.goto(`/documents/${documentId}?view=authoring&tab=access`);
  await expect(page.getByRole('heading', { name: 'アクセス設定' })).toBeVisible();
  await expect(page.getByText('現在有効なアクセス権')).toBeVisible();
  await expect(page.getByRole('radio', { name: 'この文書だけに個別設定' })).toBeChecked();
  await expect(page.getByRole('table')).toBeVisible();
  await expect(page.getByText('融資審査担当').first()).toBeVisible();
  await expect(page.getByText('document-policy')).toHaveCount(0);
  await snapshot('mock-7-access-policy.png');
});
