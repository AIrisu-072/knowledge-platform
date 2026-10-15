import { createOperationId } from '../src/application/operation-id';
import { test } from '@playwright/test';
import { expect, type APIRequestContext, type Page } from '@playwright/test';
import { readFileSync } from 'node:fs';
import { readFile, writeFile } from 'node:fs/promises';
import { isAbsolute } from 'node:path';
import { isDeepStrictEqual } from 'node:util';
import type { WorkflowActionCommand, Completed, Held, Resumed, AgentExecutionRequest, AgentExecutionRequested, AgentExecution, AgentResult, Finding, DecisionCommand, DecisionRecorded, EvidenceCommand, EvidenceRegistered, FindingCommand, FindingRegistered, RevisionRef, Claimed, DraftSaved, HandoffSnapshot, ReturnCommand, Returned, ReturnInstruction, Submitted, TaskDetail, TaskPage, WorkSession } from '../src/api/generated-work/types.gen';

import type { MoveFolderData, RenameFolderData, CreateFolderData, DocumentRevisionPage, FileList, Folder, FolderChildren, FolderDetail, MutationResult, PublishedDocumentDetail } from '@knowledge-platform/document-api-client';

export type RuntimeContext = { sales: string; office: string; documentId: string; statePath: string };
export type PersistedState = {
  schemaVersion: 6;
  documentId: string;
  salesTaskId: string;
  officeTaskId: string;
  artifactId: string;
  snapshotId: string;
  text: string;
  save: { operationId: string; result: DraftSaved };
  submit: { operationId: string; result: Submitted };
  claim: { operationId: string; result: Claimed };
  rework: {
    text: string;
    returned: { operationId: string; command: ReturnCommand; result: Returned };
    salesClaim: { operationId: string; result: Claimed };
    save: { operationId: string; result: DraftSaved };
    submit: { operationId: string; result: Submitted };
    officeClaim: { operationId: string; result: Claimed };
  };
  evidence: EvidenceState;
  agents: AgentState;
  holdResume: { sales: HoldResumeState; office: HoldResumeState };
  completion: { operationId: string; command: WorkflowActionCommand; result: Completed };
  final: {
    salesContext: TaskPage;
    salesQueue: TaskPage;
    officeContext: TaskPage;
    officeQueue: TaskPage;
    salesTask: TaskDetail;
    officeTask: TaskDetail;
    snapshot: HandoffSnapshot;
    priorSnapshot: HandoffSnapshot;
    returnInstruction: ReturnInstruction;
  };
};

export function readRuntimeContext(): RuntimeContext {
  const path = process.env.KP_ORGANIZATION_RUNTIME_CONTEXT;
  if (!path) throw new Error('Organization runtime context file is required');
  const value: unknown = JSON.parse(readFileSync(path, 'utf8'));
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('Invalid Organization context');
  const context = value as RuntimeContext;
  for (const origin of [context.sales, context.office]) {
    if (typeof origin !== 'string') throw new Error('Both fixed-profile origins are required');
    const url = new URL(origin);
    if (url.protocol !== 'http:' || url.hostname !== '127.0.0.1' || !url.port || url.username || url.password || url.pathname !== '/' || url.search || url.hash) {
      throw new Error('Organization acceptance requires explicit loopback HTTP origins');
    }
  }
  if (new URL(context.sales).origin === new URL(context.office).origin) throw new Error('Sales and office must be separate fixed-profile processes');
  if (typeof context.documentId !== 'string' || !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(context.documentId)) throw new Error('Seeded Document UUID is required');
  if (typeof context.statePath !== 'string' || !isAbsolute(context.statePath)) throw new Error('Harness-owned absolute statePath is required');
  return { ...context, sales: new URL(context.sales).origin, office: new URL(context.office).origin };
}

export async function get<T>(request: APIRequestContext, origin: string, path: string): Promise<T> {
  const response = await request.get(`${new URL(origin).origin}${path}`);
  try {
    expect(response.status(), `GET ${path} returned an unexpected status`).toBe(200);
  } catch (error) {
    try {
      // Closed failure-only metadata: no URL, ID, response body or additional request.
      const routes: [RegExp, string][] = [
        [/^\/v1\/organization\/session$/, 'session'],
        [/^\/v1\/organization\/tasks\?view=(?:context|queue)$/, 'task-list'],
        [/^\/v1\/organization\/tasks\/[A-Za-z0-9_-]+$/, 'task'],
        [/^\/v1\/organization\/handoff-snapshots\/[A-Za-z0-9_-]+$/, 'snapshot'],
        [/^\/v1\/organization\/return-instructions\/[A-Za-z0-9_-]+$/, 'return-instruction'],
        [/^\/v1\/organization\/working-artifacts\/[A-Za-z0-9_-]+$/, 'artifact'],
        [/^\/v1\/organization\/operations\/[A-Za-z0-9_-]+$/, 'operation'],
        [/^\/v1\/organization\/(?:evidence\/[A-Za-z0-9_-]+|tasks\/[A-Za-z0-9_-]+\/evidence)$/, 'evidence'],
        [/^\/v1\/organization\/(?:findings\/[A-Za-z0-9_-]+|tasks\/[A-Za-z0-9_-]+\/findings)$/, 'finding'],
        [/^\/v1\/organization\/findings\/[A-Za-z0-9_-]+\/decisions$/, 'decision'],
        [/^\/v1\/organization\/agent-executions\/[A-Za-z0-9_-]+$/, 'agent'],
        [/^\/v1\/organization\/agent-executions\/[A-Za-z0-9_-]+\/result$/, 'agent-result'],
        [/^\/v1\/documents\/[A-Za-z0-9_-]+(?:\?view=published|\/revisions\?pageSize=100|\/versions\/[A-Za-z0-9_-]+\/files\?purpose=published)$/, 'document'],
        [/^\/v1\/folders\/root$/, 'folder-root'],
        [/^\/v1\/folders\/[A-Za-z0-9_-]+\/children\?pageSize=200$/, 'folder-children'],
      ];
      const endpoint = path.length <= 2048 && !/[\r\n]/.test(path) ? routes.find(([route]) => route.test(path))?.[1] : undefined;
      const status = response.status();
      if (endpoint && Number.isInteger(status) && status >= 100 && status <= 599 && status !== 200) {
        test.info().annotations.push({ type: 'organization-read-failure', description: `${status}:${endpoint}` });
      }
    } catch { /* Diagnostics must not replace the original status assertion. */ }
    throw error;
  }
  return await response.json() as T;
}
export async function assertSessions(request: APIRequestContext, context: RuntimeContext) {
  const sales = await get<WorkSession>(request, context.sales, '/v1/organization/session');
  const office = await get<WorkSession>(request, context.office, '/v1/organization/session');
  expect(sales.principalId).toBe('sales-01');
  expect(office.principalId).toBe('office-01');
  expect(sales.actingAssignmentId).not.toBe(office.actingAssignmentId);
  // Work files (U3) are composed with the Work-owned store; the capability is a hint only.
  for (const session of [sales, office]) expect(session.capabilities).toEqual({ nativeWorkspace: false, agent: true, search: false, fileUpload: true, return: true });
  return { sales, office };
}
export async function assertHidden(request: APIRequestContext, origin: string, path: string, code: string, privateText?: string) {
  currentAction('visibility-verify');
  const response = await request.get(`${new URL(origin).origin}${path}`);
  expect(response.status()).toBe(404);
  expect(response.headers()['cache-control']).toBe('no-store');
  const body = await response.json() as Record<string, unknown>;
  expect(Object.keys(body).sort()).toEqual(['code', 'status', 'title', 'traceId', 'type']);
  expect(body).toMatchObject({ status: 404, code });
  if (privateText) expect(JSON.stringify(body)).not.toContain(privateText);
}
export async function captureFinal(request: APIRequestContext, context: RuntimeContext, salesTaskId: string, officeTaskId: string, snapshotId: string, priorSnapshotId: string, returnInstructionId: string): Promise<PersistedState['final']> {
  return {
    salesContext: await get(request, context.sales, '/v1/organization/tasks?view=context'),
    salesQueue: await get(request, context.sales, '/v1/organization/tasks?view=queue'),
    officeContext: await get(request, context.office, '/v1/organization/tasks?view=context'),
    officeQueue: await get(request, context.office, '/v1/organization/tasks?view=queue'),
    salesTask: await get(request, context.sales, `/v1/organization/tasks/${salesTaskId}`),
    officeTask: await get(request, context.office, `/v1/organization/tasks/${officeTaskId}`),
    snapshot: await get(request, context.sales, `/v1/organization/handoff-snapshots/${snapshotId}`),
    priorSnapshot: await get(request, context.sales, `/v1/organization/handoff-snapshots/${priorSnapshotId}`),
    returnInstruction: await get(request, context.sales, `/v1/organization/return-instructions/${returnInstructionId}`),
  };
}
export async function saveState(context: RuntimeContext, state: PersistedState) {
  // This is a private synthetic restart oracle, not a Playwright attachment or export.
  // Never overwrite a prior journey receipt: each harness run owns a fresh directory.
  await writeFile(context.statePath, `${JSON.stringify(state, null, 2)}\n`, { mode: 0o600, flag: 'wx' });
}
export async function loadState(context: RuntimeContext): Promise<PersistedState> {
  const state = JSON.parse(await readFile(context.statePath, 'utf8')) as PersistedState;
  expect(state.schemaVersion).toBe(6);
  expect(state.documentId).toBe(context.documentId);
  return state;
}


export type RootFolderState = {
  schemaVersion: 5;
  documentId: string;
  request: CreateFolderData['body'];
  receipt: MutationResult;
  sales: RootFolderSnapshot;
  office: RootFolderSnapshot;
  paginationChildren: Folder[];
  selectedCreate: { request: CreateFolderData['body']; receipt: MutationResult; child: Folder };
  selectedRename?: { targetFolderId: string; request: RenameFolderData['body']; receipt: MutationResult; beforeChild: Folder; child: Folder };
  selectedMove: { targetFolderId: string; request: MoveFolderData['body']; receipt: MutationResult; beforeChild: Folder; child: Folder };
};
type RootFolderCurrentState = Omit<RootFolderState, 'selectedMove'> & Partial<Pick<RootFolderState, 'selectedMove'>>;
export type RootFolderSnapshot = { root: FolderDetail; children: FolderChildren };

export async function saveRootFolderState(context: RuntimeContext, state: RootFolderState) {
  // Private synthetic restart oracle only; never attach it or modify the Work state schema.
  await writeFile(`${context.statePath}.root-folder`, `${JSON.stringify(state, null, 2)}\n`, { mode: 0o600, flag: 'wx' });
}
export async function loadRootFolderState(context: RuntimeContext): Promise<RootFolderState> {
  const state = JSON.parse(await readFile(`${context.statePath}.root-folder`, 'utf8')) as RootFolderState;
  expect(state.schemaVersion).toBe(5);
  expect(state.documentId === context.documentId).toBe(true);
  const move = state.selectedMove;
  expect(Boolean(move && typeof move === 'object' && !Array.isArray(move))).toBe(true);
  expect(Object.keys(move).sort().join(',') === 'beforeChild,child,receipt,request,targetFolderId').toBe(true);
  expect(Boolean(move.request && move.receipt && move.beforeChild && move.child && state.selectedRename)).toBe(true);
  expect(Object.keys(move.request).sort().join(',') === 'expectedFolderRevision,fromParentId,operationId,reason,toParentId').toBe(true);
  expect(Object.keys(move.receipt).sort().join(',') === 'changed,occurredAt,operationId,resourceId,resultingRevision').toBe(true);
  expect(isDeepStrictEqual(move.beforeChild, state.selectedRename!.child)).toBe(true);
  expect(move.targetFolderId === move.beforeChild.folderId && move.child.folderId === move.targetFolderId).toBe(true);
  expect(move.request.fromParentId === move.beforeChild.parentFolderId && move.request.toParentId === move.child.parentFolderId
    && move.request.fromParentId !== move.request.toParentId && move.request.expectedFolderRevision === move.beforeChild.revision).toBe(true);
  expect(typeof move.request.operationId === 'string' && Boolean(move.request.operationId)
    && typeof move.request.reason === 'string' && Boolean(move.request.reason)).toBe(true);
  expect(move.receipt.operationId === move.request.operationId && move.receipt.resourceId === move.targetFolderId
    && move.receipt.changed === true && Number.isSafeInteger(move.receipt.resultingRevision)
    && move.receipt.resultingRevision === move.beforeChild.revision + 1 && Number.isFinite(Date.parse(move.receipt.occurredAt))).toBe(true);
  expect(isDeepStrictEqual(move.child, { ...move.beforeChild, parentFolderId: move.request.toParentId, revision: move.receipt.resultingRevision })).toBe(true);
  return state;
}
export async function readRootFolderSnapshot(request: APIRequestContext, origin: string): Promise<RootFolderSnapshot> {
  const root = await get<FolderDetail>(request, origin, '/v1/folders/root');
  const children = await get<FolderChildren>(request, origin, `/v1/folders/${root.folderId}/children?pageSize=200`);
  // The bounded synthetic fixture must fit in one page; never silently omit another page.
  expect(children.nextCursor).toBeNull();
  return { root, children };
}
export function assertRootFolderCreated(snapshot: RootFolderSnapshot, command: RootFolderState['request']) {
  expect(snapshot.root.folderId === command.parentFolderId).toBe(true);
  expect(snapshot.root.parentFolderId).toBeNull();
  expect(snapshot.root.revision).toBe(command.expectedParentRevision);
  const matches = snapshot.children.items.filter(folder => folder.folderId === command.folderId);
  expect(matches.length).toBe(1);
  const child = matches[0]!;
  expect(child.parentFolderId === snapshot.root.folderId).toBe(true);
  expect(child.name === command.name).toBe(true);
  expect(child.revision).toBe(0);
}
export async function replayRootFolderCreate(request: APIRequestContext, context: RuntimeContext, state: Pick<RootFolderState, 'request' | 'receipt' | 'sales' | 'office'>) {
  currentAction('root-folder-replay');
  for (const role of ['sales', 'office'] as const) {
    expect(isDeepStrictEqual(await readRootFolderSnapshot(request, context[role]), state[role])).toBe(true);
  }
  // Deliberate backend fixed-request replay, not a GUI transport-loss recovery qualification.
  const replay = await request.post(`${context.sales}/v1/folders`, { data: state.request });
  expect(replay.status()).toBe(201);
  expect(isDeepStrictEqual(await replay.json(), state.receipt)).toBe(true);
  for (const role of ['sales', 'office'] as const) {
    const actual = await readRootFolderSnapshot(request, context[role]);
    expect(isDeepStrictEqual(actual, state[role])).toBe(true);
    assertRootFolderCreated(actual, state.request);
  }
}
export async function openRootFolderHome(page: Page, origin: string) {
  currentAction('document-navigation');
  // Opened directly, without visiting タスク first: this server offers the Work API,
  // so the primary navigation has タスク and 検索 on the document screen too.
  await page.goto(`${origin}/documents`);
  const navigation = page.getByRole('navigation', { name: 'メインナビゲーション', exact: true });
  await expect(navigation.getByRole('link', { name: 'タスク', exact: true })).toBeVisible();
  await expect(navigation.getByRole('link', { name: '検索', exact: true })).toBeVisible();
  await navigation.getByRole('link', { name: '文書', exact: true }).click();
  await expect(page.getByRole('region', { name: 'フォルダー', exact: true })).toBeVisible();
}
export async function assertRootFolderUi(page: Page, snapshot: RootFolderSnapshot, childId: string, role: 'sales' | 'office') {
  const rail = page.getByRole('region', { name: 'フォルダー', exact: true });
  const child = snapshot.children.items.find(folder => folder.folderId === childId)!;
  // Boolean/count oracles keep synthetic names out of assertion value diffs.
  for (const name of [snapshot.root.name, child.name]) {
    await expect.poll(async () => (await rail.getByRole('button').allTextContents()).filter(text => text === name).length).toBe(1);
  }
  const entry = rail.getByRole('button', { name: 'System Rootにフォルダーを作成', exact: true });
  if (role === 'office') {
    expect(snapshot.root.capabilities.createFolder).toEqual({ status: 'disabled', reason: 'permission' });
    await expect(entry).toBeDisabled();
    await expect.poll(() => entry.evaluate(button => document.getElementById(button.getAttribute('aria-describedby') ?? '')?.textContent?.trim() === '権限がありません')).toBe(true);
  } else {
    expect(snapshot.root.capabilities.createFolder).toEqual({ status: 'available' });
    await expect(entry).toBeEnabled();
  }
}

// Fixture preparation only: these are not GUI-created nonroot folders.
export async function prepareFolderPagination(request: APIRequestContext, origin: string, parentFolderId: string, expectedParentRevision: number): Promise<Folder[]> {
  currentAction('root-folder-verify');
  const children: Folder[] = [];
  for (let index = 1; index <= 201; index++) {
    const command: CreateFolderData['body'] = {
      operationId: createOperationId(), folderId: createOperationId(), parentFolderId, expectedParentRevision,
      name: `合成ページ送り-${String(index).padStart(3, '0')}`, reason: '【合成データ】フォルダー一覧の続き表示を確認する',
    };
    // Each fixed request is sent exactly once. An unknown result aborts this case.
    const response = await request.post(`${origin}/v1/folders`, { data: command });
    expect(response.status()).toBe(201);
    const receipt = await response.json() as MutationResult;
    expect(receipt.operationId === command.operationId).toBe(true);
    expect(receipt.resourceId === command.folderId).toBe(true);
    expect(receipt.changed).toBe(true);
    expect(receipt.resultingRevision).toBe(0);
    children.push({ folderId: command.folderId, parentFolderId, name: command.name, revision: receipt.resultingRevision });
  }
  return children;
}

export async function assertFolderPaginationUi(page: Page, state: Pick<RootFolderState, 'request' | 'paginationChildren'>,
  observedFirst?: import('@playwright/test').Response) {
  currentAction('root-folder-verify');
  const rail = page.getByRole('region', { name: 'フォルダー', exact: true });
  const path = `/v1/folders/${state.request.folderId}/children`;
  const firstResponse = observedFirst ?? page.waitForResponse(response => {
    const url = new URL(response.url());
    return url.pathname === path && url.searchParams.get('pageSize') === '200' && !url.searchParams.has('cursor') && response.request().method() === 'GET';
  });
  const expand = rail.getByRole('button', { name: `${state.request.name}の子フォルダーを開く`, exact: true });
  const parent = rail.getByRole('button', { name: state.request.name, exact: true }).locator('..').locator('..');
  if (!observedFirst) await expand.click();
  const response = await firstResponse;
  expect(response.status()).toBe(200);
  const first = await response.json() as FolderChildren;
  expect(first.items.length).toBe(200);
  expect(typeof first.nextCursor === 'string').toBe(true);
  const rows = parent.locator(':scope > ul > li');
  await expect(rows).toHaveCount(200);
  await expect.poll(async () => isDeepStrictEqual(await rows.locator(':scope > div > button:nth-child(2)').allTextContents(), first.items.map(folder => folder.name))).toBe(true);
  const more = rail.getByRole('button', { name: `${state.request.name}の子フォルダーをさらに表示`, exact: true });
  const nextResponse = page.waitForResponse(response => {
    const url = new URL(response.url());
    return url.pathname === path && url.searchParams.get('pageSize') === '200' && url.searchParams.get('cursor') === first.nextCursor && response.request().method() === 'GET';
  });
  await more.click();
  const next = await nextResponse;
  expect(next.status()).toBe(200);
  const last = await next.json() as FolderChildren;
  expect(last.items.length).toBe(1); expect(last.nextCursor).toBeNull();
  const byId = (a: Folder, b: Folder) => a.folderId.localeCompare(b.folderId);
  expect(isDeepStrictEqual([...first.items, ...last.items].sort(byId), [...state.paginationChildren].sort(byId))).toBe(true);
  await expect(rows).toHaveCount(201);
  await expect.poll(async () => isDeepStrictEqual(await rows.locator(':scope > div > button:nth-child(2)').allTextContents(), [...first.items, ...last.items].map(folder => folder.name))).toBe(true);
  await expect(more).toHaveCount(0);
  const tail = rail.getByRole('button', { name: last.items[0]!.name, exact: true });
  await tail.click();
  await expect(tail).toHaveAttribute('aria-current', 'location');
  await expect.poll(() => new URL(page.url()).searchParams.get('folderId') === last.items[0]!.folderId).toBe(true);
}

// The existing renamed child moves once; all 201 siblings and historical receipts stay intact.
export async function moveSelectedFolderFromUi(page: Page, request: APIRequestContext, context: RuntimeContext,
  state: RootFolderCurrentState, guiMovePosts: () => number): Promise<RootFolderState['selectedMove']> {
  currentAction('root-folder-input');
  const beforeChild = state.selectedRename!.child;
  const parent = state.paginationChildren.find(folder => folder.folderId === beforeChild.parentFolderId)!;
  const destination = state.paginationChildren.find(folder => folder.folderId !== parent.folderId)!;
  expect(Boolean(parent && destination)).toBe(true);
  const rail = page.getByRole('region', { name: 'フォルダー', exact: true });
  await rail.getByRole('button', { name: `${parent.name}の子フォルダーを開く`, exact: true }).click();
  await rail.getByRole('button', { name: beforeChild.name, exact: true }).click();
  await expect.poll(() => new URL(page.url()).searchParams.get('folderId') === beforeChild.folderId).toBe(true);
  const entry = rail.getByRole('button', { name: '選択したフォルダーを移動', exact: true });
  await expect(entry).toBeEnabled();
  const isChildren = (response: import('@playwright/test').Response, folderId: string) => {
    const url = new URL(response.url());
    return url.origin === context.sales && url.pathname === `/v1/folders/${folderId}/children`
      && url.searchParams.get('pageSize') === '200' && response.request().method() === 'GET';
  };
  const openingSource = page.waitForResponse(response => isChildren(response, parent.folderId));
  const openingCapability = page.waitForResponse(response => isChildren(response, beforeChild.folderId));
  await entry.click();
  const opening = await openingSource; expect(opening.status()).toBe(200);
  const openingRows = await opening.json() as FolderChildren;
  expect(openingRows.nextCursor).toBeNull(); expect(isDeepStrictEqual(openingRows.items, [beforeChild])).toBe(true);
  const capability = await openingCapability; expect(capability.status()).toBe(200);
  expect((await capability.json() as FolderChildren).capabilities.moveFolder.status).toBe('available');
  const dialog = page.getByRole('dialog', { name: '選択したフォルダーを移動', exact: true });
  const candidates = dialog.getByRole('group', { name: '移動先フォルダー', exact: true });
  await expect(candidates.locator('legend')).toHaveText('移動先を選択');
  await candidates.getByRole('button', { name: `${state.request.name}の子フォルダーを開く`, exact: true }).click();
  // Destination selection always performs its own fresh read, even when the candidate tree is cached.
  const destinationRows = page.waitForResponse(response => isChildren(response, state.request.folderId) && !new URL(response.url()).searchParams.has('cursor'));
  const destinationCapability = page.waitForResponse(response => isChildren(response, destination.folderId));
  await candidates.getByRole('button', { name: destination.name, exact: true }).click();
  const destinationRead = await destinationRows; expect(destinationRead.status()).toBe(200);
  expect(isDeepStrictEqual((await destinationRead.json() as FolderChildren).items.find(row => row.folderId === destination.folderId), destination)).toBe(true);
  const destinationHint = await destinationCapability; expect(destinationHint.status()).toBe(200);
  expect((await destinationHint.json() as FolderChildren).items.length).toBe(0);
  const reason = '【合成データ】改名済みの子を既存の別親へ移動して現在読取を確認する';
  await dialog.getByLabel('移動理由', { exact: true }).fill(reason);
  for (const value of [`対象フォルダーID：${beforeChild.folderId}`, `現在の親ID：${parent.folderId}`,
    `現在の対象名：${beforeChild.name}（revision 1）`, `移動先フォルダーID：${destination.folderId}`, `移動先名：${destination.name}`]) {
    await expect.poll(async () => (await dialog.textContent())!.includes(value)).toBe(true);
  }
  await expect(dialog).toContainText('継承アクセス設定の変化により、配下や自分の閲覧・編集権限が変わる可能性があります。');
  await expect(dialog.getByLabel('移動理由', { exact: true })).toHaveValue(reason);
  const confirmation = dialog.getByRole('checkbox', { name: '継承アクセス設定への影響を確認しました', exact: true });
  await expect(confirmation).not.toBeChecked();
  await expect(dialog.getByRole('button', { name: '移動する', exact: true })).toBeDisabled();
  expect(guiMovePosts()).toBe(0);
  await confirmation.check(); await expect(confirmation).toBeChecked();
  expect(guiMovePosts()).toBe(0);

  const path = `/v1/folders/${beforeChild.folderId}:move`;
  const sourcePromise = page.waitForResponse(response => isChildren(response, parent.folderId));
  const capabilityPromise = page.waitForResponse(response => isChildren(response, beforeChild.folderId));
  const destinationPromise = page.waitForResponse(response => isChildren(response, state.request.folderId) && !new URL(response.url()).searchParams.has('cursor'));
  const destinationChildrenPromise = page.waitForResponse(response => isChildren(response, destination.folderId));
  const responsePromise = page.waitForResponse(response => new URL(response.url()).origin === context.sales
    && new URL(response.url()).pathname === path && response.request().method() === 'POST');
  currentAction('root-folder-create');
  await dialog.getByRole('button', { name: '移動する', exact: true }).click();
  const source = await sourcePromise; expect(source.status()).toBe(200);
  const sourceRows = await source.json() as FolderChildren;
  expect(sourceRows.nextCursor).toBeNull(); expect(isDeepStrictEqual(sourceRows.items, [beforeChild])).toBe(true);
  const currentCapability = await capabilityPromise; expect(currentCapability.status()).toBe(200);
  expect((await currentCapability.json() as FolderChildren).capabilities.moveFolder.status).toBe('available');
  const currentDestination = await destinationPromise; expect(currentDestination.status()).toBe(200);
  const currentDestinationRows = await currentDestination.json() as FolderChildren;
  expect(isDeepStrictEqual(currentDestinationRows.items.find(row => row.folderId === destination.folderId), destination)).toBe(true);
  const priorCursor = currentDestinationRows.nextCursor;
  expect(typeof priorCursor === 'string').toBe(true);
  const destinationChildren = await destinationChildrenPromise; expect(destinationChildren.status()).toBe(200);
  const beforeDestinationChildren = await destinationChildren.json() as FolderChildren;
  expect(beforeDestinationChildren.nextCursor).toBeNull(); expect(beforeDestinationChildren.items.length).toBe(0);
  const response = await responsePromise; expect(response.status()).toBe(200);
  const command = response.request().postDataJSON() as MoveFolderData['body'];
  expect(/^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(command.operationId)).toBe(true);
  expect(isDeepStrictEqual(command, { operationId: command.operationId, fromParentId: parent.folderId, toParentId: destination.folderId,
    expectedFolderRevision: beforeChild.revision, reason })).toBe(true);
  const receipt = await response.json() as MutationResult;
  expect(Object.keys(receipt).sort().join(',') === 'changed,occurredAt,operationId,resourceId,resultingRevision').toBe(true);
  expect(receipt.operationId === command.operationId && receipt.resourceId === beforeChild.folderId).toBe(true);
  expect(receipt.changed).toBe(true); expect(receipt.resultingRevision).toBe(2); expect(Number.isFinite(Date.parse(receipt.occurredAt))).toBe(true);
  await expect(dialog.getByText('フォルダーを移動しました。', { exact: true })).toBeVisible();
  await expect(dialog.getByText('表示を更新中です。', { exact: true })).toHaveCount(0);
  await expect(dialog.getByText('表示を更新できませんでした。移動結果は確定しています。読取を再試行してください。', { exact: true })).toHaveCount(0);
  await dialog.getByRole('button', { name: '確認して閉じる', exact: true }).click();
  await expect(dialog).not.toBeVisible();
  await expect.poll(() => new URL(page.url()).searchParams.get('folderId') === beforeChild.folderId).toBe(true);
  // Reset can remount a closed tree. Open cached rows normally, then explicitly restart
  // after the successful POST, so submit's earlier destination GET cannot satisfy this read.
  const rootExpand = rail.getByRole('button', { name: `${state.request.name}の子フォルダーを開く`, exact: true });
  if (await rootExpand.count()) await rootExpand.click();
  const restart = rail.getByRole('button', { name: `${state.request.name}の子フォルダーを最初から読み直す`, exact: true });
  await expect(restart).toBeEnabled();
  const refreshedFirst = page.waitForResponse(response => isChildren(response, state.request.folderId)
    && !new URL(response.url()).searchParams.has('cursor'));
  await restart.click();
  const first = await refreshedFirst;
  expect(first.status()).toBe(200);
  expect((await first.json() as FolderChildren).nextCursor !== priorCursor).toBe(true);
  await assertFolderPaginationUi(page, state, first);
  const oldChildren = await get<FolderChildren>(request, context.sales, `/v1/folders/${parent.folderId}/children?pageSize=200`);
  expect(oldChildren.nextCursor).toBeNull(); expect(oldChildren.items.length).toBe(0);
  // Opening P can reuse its fresh empty page for 15 seconds. The existing move
  // form always reads the selected P itself freshly, without submitting a move.
  await expect(entry).toBeEnabled();
  const oldGuiResponse = page.waitForResponse(response => isChildren(response, parent.folderId));
  await entry.click();
  const oldGui = await oldGuiResponse; expect(oldGui.status()).toBe(200);
  const oldGuiChildren = await oldGui.json() as FolderChildren;
  expect(oldGuiChildren.nextCursor).toBeNull(); expect(oldGuiChildren.items.length).toBe(0);
  await dialog.getByRole('button', { name: 'キャンセル', exact: true }).click();
  expect(guiMovePosts()).toBe(1);
  await rail.getByRole('button', { name: `${parent.name}の子フォルダーを開く`, exact: true }).click();
  const oldRow = rail.getByRole('button', { name: parent.name, exact: true }).locator('..').locator('..');
  await expect(oldRow.getByRole('status')).toHaveCount(0);
  await expect(oldRow.getByRole('alert')).toHaveCount(0);
  await expect(oldRow.locator(':scope > ul > li')).toHaveCount(0);
  await rail.getByRole('button', { name: `${parent.name}の子フォルダーを閉じる`, exact: true }).click();
  await rail.getByRole('button', { name: destination.name, exact: true }).click();
  const movedResponse = page.waitForResponse(response => isChildren(response, destination.folderId));
  await rail.getByRole('button', { name: `${destination.name}の子フォルダーを開く`, exact: true }).click();
  const moved = await movedResponse; expect(moved.status()).toBe(200);
  const movedChildren = await moved.json() as FolderChildren;
  expect(movedChildren.nextCursor).toBeNull(); expect(movedChildren.items.length).toBe(1);
  const child = movedChildren.items[0]!;
  expect(isDeepStrictEqual(child, { ...beforeChild, parentFolderId: destination.folderId, revision: 2 })).toBe(true);
  const destinationRow = rail.getByRole('button', { name: destination.name, exact: true }).locator('..').locator('..');
  await expect(destinationRow.locator(':scope > ul > li')).toHaveCount(1);
  await destinationRow.getByRole('button', { name: child.name, exact: true }).click();
  await expect(destinationRow.getByRole('button', { name: child.name, exact: true })).toHaveAttribute('aria-current', 'location');
  await expect.poll(() => new URL(page.url()).searchParams.get('folderId') === child.folderId).toBe(true);
  const selectedMove = { targetFolderId: child.folderId, request: command, receipt, beforeChild, child };
  for (const role of ['sales', 'office'] as const) await readSelectedFolderParents(request, context[role], { ...state, selectedMove });

  currentAction('document-navigation');
  const navigation = page.getByRole('navigation', { name: 'メインナビゲーション', exact: true });
  await navigation.getByRole('link', { name: 'タスク', exact: true }).click();
  await expect(navigation.getByRole('link', { name: 'タスク', exact: true })).toHaveAttribute('aria-current', 'page');
  const work = await loadState(context);
  await page.getByRole('complementary', { name: 'タスク一覧' }).getByRole('button', { name: new RegExp(work.final.salesTask.title) }).click();
  await expect(page.getByRole('region', { name: '提出済みスナップショット', exact: true })).toContainText(work.rework.text);
  await navigation.getByRole('link', { name: '文書', exact: true }).click();
  await expect(navigation.getByRole('link', { name: '文書', exact: true })).toHaveAttribute('aria-current', 'page');
  // The ordinary navigation remounts a closed tree; cached reads need no unconditional response wait.
  await rail.getByRole('button', { name: `${state.request.name}の子フォルダーを開く`, exact: true }).click();
  await rail.getByRole('button', { name: destination.name, exact: true }).click();
  await rail.getByRole('button', { name: `${destination.name}の子フォルダーを開く`, exact: true }).click();
  await rail.getByRole('button', { name: child.name, exact: true }).click();
  await expect.poll(() => new URL(page.url()).searchParams.get('folderId') === child.folderId).toBe(true);
  await expect(entry).toBeEnabled(); await entry.click();
  await expect.poll(async () => (await dialog.textContent())!.includes(`現在の親ID：${destination.folderId}`)
    && (await dialog.textContent())!.includes(`現在の対象名：${child.name}（revision 2）`)).toBe(true);
  await expect(dialog.getByRole('checkbox', { name: '継承アクセス設定への影響を確認しました', exact: true })).not.toBeChecked();
  await dialog.getByRole('button', { name: 'キャンセル', exact: true }).click();
  expect(guiMovePosts()).toBe(1);
  return selectedMove;
}

// Extend the existing tail selection; no extra fixture, runner, fault or capture is introduced.
export async function createSelectedFolderFromUi(page: Page, request: APIRequestContext, context: RuntimeContext,
  state: Omit<RootFolderState, 'selectedCreate' | 'selectedMove'>): Promise<RootFolderState['selectedCreate']> {
  currentAction('root-folder-input');
  const parentId = new URL(page.url()).searchParams.get('folderId');
  const parent = state.paginationChildren.find(folder => folder.folderId === parentId)!;
  expect(Boolean(parent)).toBe(true);
  const rail = page.getByRole('region', { name: 'フォルダー', exact: true });
  const entry = rail.getByRole('button', { name: '選択したフォルダーに子フォルダーを作成', exact: true });
  await expect(entry).toBeEnabled(); await entry.click();
  const dialog = page.getByRole('dialog', { name: '選択したフォルダーに子フォルダーを作成', exact: true });
  expect((await dialog.textContent())!.includes(parent.folderId)).toBe(true);
  expect((await dialog.textContent())!.includes(parent.name)).toBe(true);
  const name = '合成選択親の子フォルダー'; const reason = '【合成データ】201件目の選択親への子作成を確認する';
  await dialog.getByLabel('フォルダー名', { exact: true }).fill(name);
  await dialog.getByLabel('作成理由', { exact: true }).fill(reason);
  const sourcePath = `/v1/folders/${state.request.folderId}/children`;
  const sourceResponse = (response: import('@playwright/test').Response) => {
    const url = new URL(response.url());
    return url.origin === context.sales && url.pathname === sourcePath && response.request().method() === 'GET';
  };
  const firstPromise = page.waitForResponse(response => sourceResponse(response) && !new URL(response.url()).searchParams.has('cursor'));
  const lastPromise = page.waitForResponse(response => sourceResponse(response) && new URL(response.url()).searchParams.has('cursor'));
  const capabilityPromise = page.waitForResponse(response => new URL(response.url()).origin === context.sales && new URL(response.url()).pathname === `/v1/folders/${parent.folderId}/children` && response.request().method() === 'GET');
  const responsePromise = page.waitForResponse(response => new URL(response.url()).origin === context.sales && new URL(response.url()).pathname === '/v1/folders' && response.request().method() === 'POST');
  currentAction('root-folder-create');
  await dialog.getByRole('button', { name: '作成する', exact: true }).click();
  const firstResponse = await firstPromise; expect(firstResponse.status()).toBe(200);
  const first = await firstResponse.json() as FolderChildren; expect(first.items.length).toBe(200);
  const lastResponse = await lastPromise; expect(lastResponse.status()).toBe(200);
  expect(new URL(lastResponse.url()).searchParams.get('cursor') === first.nextCursor).toBe(true);
  const last = await lastResponse.json() as FolderChildren; expect(last.items.length).toBe(1); expect(last.nextCursor).toBeNull();
  const currentParent = last.items[0]!;
  expect(isDeepStrictEqual(currentParent, parent)).toBe(true);
  const capabilityResponse = await capabilityPromise; expect(capabilityResponse.status()).toBe(200);
  expect((await capabilityResponse.json() as FolderChildren).capabilities.createFolder.status).toBe('available');
  const response = await responsePromise; expect(response.status()).toBe(201);
  const command = response.request().postDataJSON() as CreateFolderData['body'];
  const receipt = await response.json() as MutationResult;
  const uuidV7 = /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
  expect(uuidV7.test(command.operationId) && uuidV7.test(command.folderId) && command.operationId !== command.folderId).toBe(true);
  expect(isDeepStrictEqual(command, { operationId: command.operationId, folderId: command.folderId, parentFolderId: parent.folderId,
    expectedParentRevision: currentParent.revision, name, reason })).toBe(true);
  expect(receipt.operationId === command.operationId && receipt.resourceId === command.folderId).toBe(true);
  expect(receipt.changed).toBe(true); expect(receipt.resultingRevision).toBe(0);
  expect(Number.isFinite(Date.parse(receipt.occurredAt))).toBe(true);
  await expect(dialog.getByRole('status')).toHaveText('フォルダーを作成しました。');
  await dialog.getByRole('button', { name: '確認して閉じる', exact: true }).click();
  const selectedCreate = { request: command, receipt, child: { folderId: command.folderId, parentFolderId: parent.folderId, name, revision: 0 } };
  await assertSelectedFolderUi(page, request, context, { ...state, selectedCreate });
  return selectedCreate;
}

export async function assertSelectedFolderUi(page: Page, request: APIRequestContext, context: RuntimeContext, state: RootFolderCurrentState) {
  currentAction('root-folder-verify');
  const { request: command } = state.selectedCreate;
  const child = state.selectedMove?.child ?? state.selectedRename?.child ?? state.selectedCreate.child;
  const parent = state.paginationChildren.find(folder => folder.folderId === child.parentFolderId)!;
  const rail = page.getByRole('region', { name: 'フォルダー', exact: true });
  if (state.selectedMove) await rail.getByRole('button', { name: parent.name, exact: true }).click();
  await expect.poll(() => new URL(page.url()).searchParams.get('folderId') === parent.folderId).toBe(true);
  for (const role of ['sales', 'office'] as const) {
    const children = await get<FolderChildren>(request, context[role], `/v1/folders/${parent.folderId}/children?pageSize=200`);
    expect(children.nextCursor).toBeNull(); expect(isDeepStrictEqual(children.items, [child])).toBe(true);
    expect(isDeepStrictEqual(children.capabilities.createFolder, role === 'sales' ? { status: 'available' } : { status: 'disabled', reason: 'permission' })).toBe(true);
    if (state.selectedMove) {
      const old = await get<FolderChildren>(request, context[role], `/v1/folders/${command.parentFolderId}/children?pageSize=200`);
      expect(old.nextCursor).toBeNull(); expect(old.items.length).toBe(0);
    }
  }
  await rail.getByRole('button', { name: `${parent.name}の子フォルダーを開く`, exact: true }).click();
  await expect.poll(async () => (await rail.getByRole('button').allTextContents()).filter(text => text === child.name).length).toBe(1);
  if (state.selectedMove) {
    const parentRow = rail.getByRole('button', { name: parent.name, exact: true }).locator('..').locator('..');
    await parentRow.getByRole('button', { name: child.name, exact: true }).click();
    await expect.poll(() => new URL(page.url()).searchParams.get('folderId') === child.folderId).toBe(true);
    await rail.getByRole('button', { name: parent.name, exact: true }).click();
  }
  await rail.getByRole('button', { name: `${parent.name}の子フォルダーを閉じる`, exact: true }).click();
}

export async function readSelectedFolderParents(request: APIRequestContext, origin: string, state: RootFolderCurrentState) {
  const child = state.selectedMove?.child ?? state.selectedRename?.child ?? state.selectedCreate.child;
  const original = await get<FolderChildren>(request, origin, `/v1/folders/${state.selectedCreate.request.parentFolderId}/children?pageSize=200`);
  const current = child.parentFolderId === state.selectedCreate.request.parentFolderId ? original
    : await get<FolderChildren>(request, origin, `/v1/folders/${child.parentFolderId}/children?pageSize=200`);
  expect(original.nextCursor).toBeNull(); expect(current.nextCursor).toBeNull();
  expect(isDeepStrictEqual(original.items, state.selectedMove ? [] : [child])).toBe(true);
  expect(isDeepStrictEqual(current.items, [child])).toBe(true);
  return { original, current };
}

export async function replaySelectedFolderCreate(request: APIRequestContext, context: RuntimeContext, state: RootFolderCurrentState) {
  currentAction('root-folder-replay');
  const command = state.selectedCreate.request;
  const before = await Promise.all((['sales', 'office'] as const).map(role => readSelectedFolderParents(request, context[role], state)));
  const replay = await request.post(`${context.sales}/v1/folders`, { data: command });
  expect(replay.status()).toBe(201); expect(isDeepStrictEqual(await replay.json(), state.selectedCreate.receipt)).toBe(true);
  // Idempotent replay still requires current authorization; the readable office profile cannot create.
  const denied = await request.post(`${context.office}/v1/folders`, { data: command });
  expect(denied.status()).toBe(403); expect((await denied.json()).code).toBe('FORBIDDEN');
  for (const [index, role] of (['sales', 'office'] as const).entries()) {
    const children = await readSelectedFolderParents(request, context[role], state);
    expect(isDeepStrictEqual(children, before[index])).toBe(true);
    expect(isDeepStrictEqual(await readRootFolderSnapshot(request, context[role]), state[role])).toBe(true);
  }
}

// Rename only the GUI-created child. Root and the 201 pagination rows remain unchanged.
export async function renameSelectedFolderFromUi(page: Page, request: APIRequestContext, context: RuntimeContext,
  state: RootFolderCurrentState): Promise<NonNullable<RootFolderState['selectedRename']>> {
  currentAction('root-folder-input');
  const beforeChild = state.selectedCreate.child;
  const parent = state.paginationChildren.find(folder => folder.folderId === beforeChild.parentFolderId)!;
  const rail = page.getByRole('region', { name: 'フォルダー', exact: true });
  await rail.getByRole('button', { name: `${parent.name}の子フォルダーを開く`, exact: true }).click();
  await rail.getByRole('button', { name: beforeChild.name, exact: true }).click();
  await expect.poll(() => new URL(page.url()).searchParams.get('folderId') === beforeChild.folderId).toBe(true);
  const entry = rail.getByRole('button', { name: '選択したフォルダー名を変更', exact: true });
  await expect(entry).toBeEnabled(); await entry.click();
  const dialog = page.getByRole('dialog', { name: '選択したフォルダー名を変更', exact: true });
  await expect(dialog.getByLabel('変更先のフォルダー名', { exact: true })).toHaveValue(beforeChild.name);
  const name = '合成改名済み選択親の子フォルダー'; const reason = '【合成データ】GUI作成した子だけの改名を確認する';
  await dialog.getByLabel('変更先のフォルダー名', { exact: true }).fill(name);
  await dialog.getByLabel('変更理由', { exact: true }).fill(reason);
  const sourcePromise = page.waitForResponse(response => new URL(response.url()).origin === context.sales
    && new URL(response.url()).pathname === `/v1/folders/${parent.folderId}/children` && response.request().method() === 'GET');
  const capabilityPromise = page.waitForResponse(response => new URL(response.url()).origin === context.sales
    && new URL(response.url()).pathname === `/v1/folders/${beforeChild.folderId}/children` && response.request().method() === 'GET');
  const responsePromise = page.waitForResponse(response => new URL(response.url()).origin === context.sales
    && new URL(response.url()).pathname === `/v1/folders/${beforeChild.folderId}` && response.request().method() === 'PATCH');
  currentAction('root-folder-create');
  await dialog.getByRole('button', { name: '変更を保存する', exact: true }).click();
  const source = await sourcePromise; expect(source.status()).toBe(200);
  const rows = await source.json() as FolderChildren; expect(rows.nextCursor).toBeNull(); expect(isDeepStrictEqual(rows.items, [beforeChild])).toBe(true);
  const capability = await capabilityPromise; expect(capability.status()).toBe(200);
  expect((await capability.json() as FolderChildren).capabilities.renameFolder.status).toBe('available');
  const response = await responsePromise; expect(response.status()).toBe(200);
  const command = response.request().postDataJSON() as RenameFolderData['body'];
  const receipt = await response.json() as MutationResult;
  expect(/^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i.test(command.operationId)).toBe(true);
  expect(isDeepStrictEqual(command, { operationId: command.operationId, expectedFolderRevision: beforeChild.revision, name, reason })).toBe(true);
  expect(receipt.operationId === command.operationId && receipt.resourceId === beforeChild.folderId).toBe(true);
  expect(receipt.changed).toBe(true); expect(receipt.resultingRevision).toBe(1); expect(Number.isFinite(Date.parse(receipt.occurredAt))).toBe(true);
  await expect(dialog.getByText('フォルダー名を変更しました。', { exact: true })).toBeVisible();
  await dialog.getByRole('button', { name: '確認して閉じる', exact: true }).click();
  const child = { ...beforeChild, name, revision: 1 };
  await expect.poll(() => new URL(page.url()).searchParams.get('folderId') === child.folderId).toBe(true);
  await expect(rail.getByRole('button', { name, exact: true })).toHaveAttribute('aria-current', 'location');
  // Explicitly return to the original tail so the established pagination/UI helper assumptions remain intact.
  await rail.getByRole('button', { name: parent.name, exact: true }).click();
  await rail.getByRole('button', { name: `${parent.name}の子フォルダーを閉じる`, exact: true }).click();
  const selectedRename = { targetFolderId: child.folderId, request: command, receipt, beforeChild, child };
  await assertSelectedFolderUi(page, request, context, { ...state, selectedRename });
  return selectedRename;
}

export async function replaySelectedFolderRename(request: APIRequestContext, context: RuntimeContext, state: RootFolderCurrentState) {
  currentAction('root-folder-replay');
  const rename = state.selectedRename!;
  expect(Boolean(rename)).toBe(true);
  expect(rename.targetFolderId === state.selectedCreate.child.folderId).toBe(true);
  expect(isDeepStrictEqual(rename.beforeChild, state.selectedCreate.child)).toBe(true);
  const before = await Promise.all((['sales', 'office'] as const).map(role => readSelectedFolderParents(request, context[role], state)));
  for (const role of ['sales', 'office'] as const) {
    const capability = await get<FolderChildren>(request, context[role], `/v1/folders/${rename.targetFolderId}/children?pageSize=200`);
    expect(isDeepStrictEqual(capability.capabilities.renameFolder, role === 'sales' ? { status: 'available' } : { status: 'disabled', reason: 'permission' })).toBe(true);
  }
  // Backend fixed-request replay qualification only; no GUI fault or new mutation is introduced.
  const replay = await request.patch(`${context.sales}/v1/folders/${rename.targetFolderId}`, { data: rename.request });
  expect(replay.status()).toBe(200); expect(isDeepStrictEqual(await replay.json(), rename.receipt)).toBe(true);
  const denied = await request.patch(`${context.office}/v1/folders/${rename.targetFolderId}`, { data: rename.request });
  expect(denied.status()).toBe(403); expect((await denied.json()).code).toBe('FORBIDDEN');
  for (const [index, role] of (['sales', 'office'] as const).entries()) {
    const current = await readSelectedFolderParents(request, context[role], state);
    expect(isDeepStrictEqual(current, before[index])).toBe(true);
    expect(isDeepStrictEqual(await readRootFolderSnapshot(request, context[role]), state[role])).toBe(true);
  }
}

export async function replaySelectedFolderMove(request: APIRequestContext, context: RuntimeContext, state: RootFolderState) {
  currentAction('root-folder-replay');
  const move = state.selectedMove;
  expect(isDeepStrictEqual(move.beforeChild, state.selectedRename!.child)).toBe(true);
  const before = await Promise.all((['sales', 'office'] as const).map(role => readSelectedFolderParents(request, context[role], state)));
  for (const role of ['sales', 'office'] as const) {
    const capability = await get<FolderChildren>(request, context[role], `/v1/folders/${move.targetFolderId}/children?pageSize=200`);
    expect(isDeepStrictEqual(capability.capabilities.moveFolder, role === 'sales' ? { status: 'available' } : { status: 'disabled', reason: 'permission' })).toBe(true);
  }
  const replay = await request.post(`${context.sales}/v1/folders/${move.targetFolderId}:move`, { data: move.request });
  expect(replay.status()).toBe(200); expect(isDeepStrictEqual(await replay.json(), move.receipt)).toBe(true);
  const denied = await request.post(`${context.office}/v1/folders/${move.targetFolderId}:move`, { data: move.request });
  expect(denied.status()).toBe(403); expect((await denied.json()).code).toBe('FORBIDDEN');
  for (const [index, role] of (['sales', 'office'] as const).entries()) {
    expect(isDeepStrictEqual(await readSelectedFolderParents(request, context[role], state), before[index])).toBe(true);
    expect(isDeepStrictEqual(await readRootFolderSnapshot(request, context[role]), state[role])).toBe(true);
  }
}

export type EvidenceReceipt = { operationId: string; command: EvidenceCommand; result: EvidenceRegistered };
export type FindingReceipt = { operationId: string; command: FindingCommand; result: FindingRegistered };
export type DecisionReceipt = { operationId: string; command: DecisionCommand; result: DecisionRecorded };
export type EvidenceState = {
  selected: EvidenceReceipt;
  unselected: EvidenceReceipt;
  finding: FindingReceipt;
  privateFinding: FindingReceipt;
  decisions: DecisionReceipt[];
  privateDecision: DecisionReceipt;
  officeDecision: DecisionReceipt;
  officeReworkDecision: DecisionReceipt;
  rework: { evidence: EvidenceReceipt; finding: FindingReceipt; decision: DecisionReceipt };
};
export function revisionRef(record: RevisionRef): RevisionRef {
  return { id: record.id, revision: record.revision };
}
export async function publishedEvidenceSource(request: APIRequestContext, origin: string, documentId: string) {
  currentAction('source-read');
  const document = await get<PublishedDocumentDetail>(request, origin, `/v1/documents/${documentId}?view=published`);
  currentAction('source-read');
  const revisions = await get<DocumentRevisionPage>(request, origin, `/v1/documents/${documentId}/revisions?pageSize=100`);
  expect(document.displayRevision).not.toBeNull();
  const revision = document.displayRevision!;
  expect(revisions.items).toContainEqual(revision);
  expect(revision.documentVersionId).toBe(document.currentVersionId);
  currentAction('source-read');
  const files = await get<FileList>(request, origin, `/v1/documents/${documentId}/versions/${document.currentVersionId}/files?purpose=published`);
  const authoritative = files.items.filter((file) => file.role === 'AUTHORITATIVE');
  expect(authoritative.length).toBeGreaterThan(0);
  const file = authoritative[0]!;
  return {
    sourceRef: { providerId: 'document' as const, resourceId: documentId, revisionId: revision.revisionId, versionId: document.currentVersionId },
    authoritativeLocator: { kind: 'contentItem' as const, contentItemId: file.contentItemId, representationId: file.representationId },
  };
}
export async function registerEvidence(page: Page, taskId: string, source: Awaited<ReturnType<typeof publishedEvidenceSource>>, relevantLocation: string): Promise<EvidenceReceipt> {
  currentAction('source-document-select');
  await page.getByLabel('根拠にする入力文書', { exact: true }).selectOption(source.sourceRef.resourceId);
  currentAction('source-file-select');
  await page.getByLabel('原本ファイル', { exact: true }).selectOption(`${source.authoritativeLocator.contentItemId}:${source.authoritativeLocator.representationId}`);
  currentAction('evidence-input');
  await page.getByLabel('該当箇所（人間の記載・未検証）', { exact: true }).fill(relevantLocation);
  const responsePromise = page.waitForResponse((response) => new URL(response.url()).pathname === `/v1/organization/tasks/${taskId}/evidence` && response.request().method() === 'POST');
  currentAction('evidence-submit');
  await page.getByRole('button', { name: '根拠を登録', exact: true }).click();
  const response = await responsePromise;
  expect(response.status()).toBe(200);
  const result = await response.json() as EvidenceRegistered;
  const command = response.request().postDataJSON() as EvidenceCommand;
  expect(command).toMatchObject({ ...source, relevantLocation, expectedAttemptId: result.task.attemptId });
  expect(command.expectedRevision).toBe(result.task.revision - 1);
  expect(result.kind).toBe('evidence_registered');
  expect(result.evidence).toMatchObject({ ...source, relevantLocation, taskId, attemptId: result.task.attemptId, revision: 1, contextId: result.task.contextId, createdBy: 'sales-01', actingAssignmentId: command.actingAssignmentId, origin: 'human', coverage: 'unknown', fragmentOmissionReason: 'not_retained', relevantLocationVerified: false, policyDisposition: 'reference_only', visibility: 'work_item_private' });
  expect(result.evidence).not.toHaveProperty('fragment');
  expect(result.task.state).toBe('active');
  await expect(page.getByLabel(`候補の根拠 ${result.evidence.id}`, { exact: true })).toBeVisible();
  const saved = page.getByRole('region', { name: `根拠 ${result.evidence.id}`, exact: true });
  for (const value of [source.sourceRef.resourceId, source.sourceRef.revisionId, source.sourceRef.versionId, source.authoritativeLocator.contentItemId, source.authoritativeLocator.representationId, relevantLocation, 'unknown', 'not_retained']) await expect(saved).toContainText(value);
  return { operationId: command.operationId, command, result };
}
export async function registerFinding(page: Page, taskId: string, evidence: EvidenceRegistered['evidence'], claim: string): Promise<FindingReceipt> {
  currentAction('finding-input');
  await page.getByLabel('候補の主張', { exact: true }).fill(claim);
  const selected = page.getByRole('checkbox', { name: /^候補の根拠 / });
  for (const checkbox of await selected.all()) await checkbox.uncheck();
  await expect(page.getByRole('button', { name: '候補を登録', exact: true })).toBeDisabled();
  await page.getByLabel(`候補の根拠 ${evidence.id}`, { exact: true }).check();
  const responsePromise = page.waitForResponse((response) => new URL(response.url()).pathname === `/v1/organization/tasks/${taskId}/findings` && response.request().method() === 'POST');
  currentAction('finding-submit');
  await page.getByRole('button', { name: '候補を登録', exact: true }).click();
  const response = await responsePromise;
  expect(response.status()).toBe(200);
  const result = await response.json() as FindingRegistered;
  const command = response.request().postDataJSON() as FindingCommand;
  expect(command).toMatchObject({ claim, evidenceRevisionRefs: [revisionRef(evidence)], expectedAttemptId: result.task.attemptId });
  expect(command.expectedRevision).toBe(result.task.revision - 1);
  expect(result).toMatchObject({ kind: 'finding_registered', finding: { taskId, attemptId: result.task.attemptId, revision: 1, contextId: result.task.contextId, author: 'sales-01', actingAssignmentId: command.actingAssignmentId, claim, evidenceRevisionRefs: [revisionRef(evidence)], visibility: 'work_item_private' } });
  await expect(page.getByRole('region', { name: `候補 ${result.finding.id}`, exact: true })).toContainText(claim);
  return { operationId: command.operationId, command, result };
}
export async function recordDecision(page: Page, taskId: string, finding: FindingRegistered['finding'], evidence: EvidenceRegistered['evidence'], decision: DecisionCommand['decision'], reason: string, adoptedClaim?: string, cancelFirst = false): Promise<DecisionReceipt> {
  const region = page.getByRole('region', { name: `候補 ${finding.id}`, exact: true });
  currentAction('decision-select');
  await region.getByLabel(`候補の判断 ${finding.id}`, { exact: true }).selectOption(decision);
  if (decision === 'modified') {
    currentAction('decision-input');
    await region.getByLabel('採用文', { exact: true }).fill('');
    await expect(region.getByRole('button', { name: '判断内容を確認', exact: true })).toBeDisabled();
    currentAction('decision-input');
    await region.getByLabel('採用文', { exact: true }).fill(adoptedClaim!);
  }
  currentAction('decision-input');
  await region.getByLabel('判断理由', { exact: true }).fill(reason);
  const dialog = page.getByRole('dialog', { name: '人間判断の確認', exact: true });
  if (cancelFirst) {
    let posts = 0;
    const count = (outgoing: import('@playwright/test').Request) => { if (new URL(outgoing.url()).pathname === `/v1/organization/findings/${finding.id}/decisions` && outgoing.method() === 'POST') posts += 1; };
    page.on('request', count);
    currentAction('decision-preview');
    await region.getByRole('button', { name: '判断内容を確認', exact: true }).click();
    await expect(dialog).toContainText(finding.claim);
    await expect(dialog.getByRole('button', { name: 'キャンセル', exact: true })).toBeFocused();
    await dialog.getByRole('button', { name: 'キャンセル', exact: true }).click();
    await expect(dialog).not.toBeVisible();
    await expect(region.getByLabel('判断理由', { exact: true })).toHaveValue(reason);
    if (adoptedClaim) await expect(region.getByLabel('採用文', { exact: true })).toHaveValue(adoptedClaim);
    expect(posts).toBe(0);
    page.off('request', count);
  }
  currentAction('decision-preview');
  await region.getByRole('button', { name: '判断内容を確認', exact: true }).click();
  const responsePromise = page.waitForResponse((response) => new URL(response.url()).pathname === `/v1/organization/findings/${finding.id}/decisions` && response.request().method() === 'POST');
  currentAction('decision-confirm');
  await dialog.getByRole('button', { name: '判断を確定', exact: true }).click();
  const response = await responsePromise;
  expect(response.status()).toBe(200);
  const result = await response.json() as DecisionRecorded;
  const command = response.request().postDataJSON() as DecisionCommand;
  expect(command).toMatchObject({ taskId, expectedAttemptId: result.task.attemptId, findingRevision: finding.revision, decision, reason, evidenceRevisionRefs: [revisionRef(evidence)], ...(adoptedClaim ? { adoptedClaim } : {}) });
  expect(command.expectedRevision).toBe(result.task.revision - 1);
  if (decision !== 'modified') expect(command).not.toHaveProperty('adoptedClaim');
  expect(result).toMatchObject({ kind: 'decision_recorded', decision: { taskId, attemptId: result.task.attemptId, findingId: finding.id, findingRevision: finding.revision, revision: 1, decision, reason, evidenceRevisionRefs: [revisionRef(evidence)], ...(adoptedClaim ? { adoptedClaim } : {}) } });
  expect(result.task.state).toBe('active');
  await expect(dialog).not.toBeVisible();
  await expect(region).toContainText(reason);
  return { operationId: command.operationId, command, result };
}

export async function assertEvidenceState(request: APIRequestContext, context: RuntimeContext, salesTaskId: string, officeTaskId: string, state: EvidenceState, agents: AgentState) {
  const sharedEvidence = state.selected.result.evidence;
  const sharedFinding = state.finding.result.finding;
  const privateEvidence = state.rework.evidence.result.evidence;
  const privateFinding = state.rework.finding.result.finding;
  const sharedDecisions = state.decisions.map((receipt) => receipt.result.decision);
  expect(await get(request, context.sales, `/v1/organization/tasks/${salesTaskId}/evidence`)).toEqual({ items: [sharedEvidence, privateEvidence], nextCursor: null });
  expect(await get(request, context.sales, `/v1/organization/tasks/${salesTaskId}/findings`)).toEqual({ items: [sharedFinding, privateFinding, agents.sales.finding], nextCursor: null });
  expect(await get(request, context.office, `/v1/organization/tasks/${officeTaskId}/evidence`)).toEqual({ items: [sharedEvidence], nextCursor: null });
  expect(await get(request, context.office, `/v1/organization/tasks/${officeTaskId}/findings`)).toEqual({ items: [sharedFinding, agents.sales.finding, agents.office.finding], nextCursor: null });
  for (const origin of [context.sales, context.office]) {
    expect(await get(request, origin, `/v1/organization/evidence/${sharedEvidence.id}`)).toEqual(sharedEvidence);
    expect(await get(request, origin, `/v1/organization/findings/${sharedFinding.id}`)).toEqual(sharedFinding);
    await assertHidden(request, origin, `/v1/organization/evidence/${state.unselected.result.evidence.id}`, 'EVIDENCE_NOT_FOUND', state.unselected.result.evidence.relevantLocation);
    await assertHidden(request, origin, `/v1/organization/findings/${state.privateFinding.result.finding.id}`, 'FINDING_NOT_FOUND', state.privateFinding.result.finding.claim);
    await assertHidden(request, origin, `/v1/organization/findings/${state.privateFinding.result.finding.id}/decisions`, 'FINDING_NOT_FOUND');
  }
  expect(await get(request, context.sales, `/v1/organization/findings/${sharedFinding.id}/decisions`)).toEqual({ items: sharedDecisions, nextCursor: null });
  expect(await get(request, context.office, `/v1/organization/findings/${sharedFinding.id}/decisions`)).toEqual({ items: [...sharedDecisions, state.officeReworkDecision.result.decision], nextCursor: null });
  expect(await get(request, context.sales, `/v1/organization/evidence/${privateEvidence.id}`)).toEqual(privateEvidence);
  expect(await get(request, context.sales, `/v1/organization/findings/${privateFinding.id}`)).toEqual(privateFinding);
  expect(await get(request, context.sales, `/v1/organization/findings/${privateFinding.id}/decisions`)).toEqual({ items: [state.rework.decision.result.decision], nextCursor: null });
  await assertHidden(request, context.office, `/v1/organization/evidence/${privateEvidence.id}`, 'EVIDENCE_NOT_FOUND', privateEvidence.relevantLocation);
  await assertHidden(request, context.office, `/v1/organization/findings/${privateFinding.id}`, 'FINDING_NOT_FOUND', privateFinding.claim);
  await assertHidden(request, context.office, `/v1/organization/findings/${privateFinding.id}/decisions`, 'FINDING_NOT_FOUND');
  for (const receipt of [state.selected, state.finding, ...state.decisions, state.rework.evidence, state.rework.finding, state.rework.decision]) {
    expect(await get(request, context.sales, `/v1/organization/operations/${receipt.operationId}`)).toEqual(receipt.result);
    await assertHidden(request, context.office, `/v1/organization/operations/${receipt.operationId}`, 'WORK_ITEM_NOT_FOUND');
  }
  await assertHidden(request, context.sales, `/v1/organization/operations/${state.unselected.operationId}`, 'EVIDENCE_NOT_FOUND');
  for (const receipt of [state.privateFinding, state.privateDecision]) await assertHidden(request, context.sales, `/v1/organization/operations/${receipt.operationId}`, 'FINDING_NOT_FOUND');
  await assertHidden(request, context.office, `/v1/organization/operations/${state.officeDecision.operationId}`, 'FINDING_NOT_FOUND');
  expect(await get(request, context.office, `/v1/organization/operations/${state.officeReworkDecision.operationId}`)).toEqual(state.officeReworkDecision.result);
  await assertHidden(request, context.sales, `/v1/organization/operations/${state.officeReworkDecision.operationId}`, 'WORK_ITEM_NOT_FOUND');
}

// The last action entered, not proof that it completed or that the test is waiting there.
type OrganizationAction =
  | 'journey-setup' | 'office-navigation' | 'sales-navigation' | 'document-navigation' | 'task-navigation'
  | 'draft-save' | 'source-read' | 'evidence-module' | 'source-document-select' | 'source-file-select'
  | 'evidence-input' | 'evidence-submit' | 'finding-input' | 'finding-submit' | 'decision-select'
  | 'decision-input' | 'decision-preview' | 'decision-confirm' | 'visibility-verify' | 'submit-preview'
  | 'submit-selection' | 'submit-confirm' | 'office-claim' | 'return-preview' | 'return-confirm'
  | 'sales-reclaim' | 'resubmit' | 'office-reclaim' | 'final-verify' | 'persistence-verify'
  | 'agent-module' | 'agent-input' | 'agent-request' | 'agent-result' | 'agent-replay'
  | 'complete-preview' | 'complete-confirm' | 'complete-replay'
  | 'hold-preview' | 'hold-confirm' | 'hold-replay' | 'resume-preview' | 'resume-confirm' | 'resume-replay'
  | 'root-folder-read' | 'root-folder-preview' | 'root-folder-cancel' | 'root-folder-input'
  | 'root-folder-create' | 'root-folder-verify' | 'root-folder-replay' | 'root-folder-office' | 'root-folder-persistence';
export function currentAction(action: OrganizationAction): void {
  const annotations = test.info().annotations;
  for (let index = annotations.length - 1; index >= 0; index--) {
    if (annotations[index]?.type === 'organization-stage') annotations.splice(index, 1);
  }
  annotations.push({ type: 'organization-stage', description: action });
}


export type AgentReceipt = { operationId: string; command: AgentExecutionRequest; result: AgentExecutionRequested; execution: AgentExecution; output: AgentResult; finding: Finding };
export type AgentState = { sales: AgentReceipt & { decision: DecisionReceipt }; office: AgentReceipt & { decision: DecisionReceipt } };

export async function requestSyntheticFinding(page: Page, request: APIRequestContext, origin: string, taskId: string, evidence: EvidenceRegistered['evidence'], principal: 'sales-01' | 'office-01'): Promise<AgentReceipt> {
  currentAction('agent-module');
  await page.getByRole('button', { name: 'Agent', exact: true }).click();
  const module = page.getByRole('region', { name: '合成Agent', exact: true });
  for (const label of ['固定規則の模擬処理', '原本本文を分析しません', '実LLM・MCP通信は使用しません']) await expect(module).toContainText(label);
  currentAction('agent-input');
  const purpose = '【合成データ】選択した参照から固定規則の候補を作成し、人間が別途判断する。';
  await module.getByLabel('Agentへの依頼目的', { exact: true }).fill(purpose);
  await expect(module.getByRole('button', { name: '合成Agentに依頼', exact: true })).toBeDisabled();
  await module.getByLabel(`Agentの根拠 ${evidence.id}`, { exact: true }).check();
  const accepted = page.waitForResponse((response) => new URL(response.url()).pathname === `/v1/organization/tasks/${taskId}/agent-executions` && response.request().method() === 'POST');
  currentAction('agent-request');
  await module.getByRole('button', { name: '合成Agentに依頼', exact: true }).click();
  const response = await accepted;
  expect(response.status()).toBe(202);
  const command = response.request().postDataJSON() as AgentExecutionRequest;
  const result = await response.json() as AgentExecutionRequested;
  expect(Object.keys(command).sort()).toEqual(['actingAssignmentId', 'evidenceRevisionRefs', 'expectedAttemptId', 'expectedRevision', 'operationId', 'purpose']);
  expect(command).toMatchObject({ expectedAttemptId: result.task.attemptId, expectedRevision: result.task.revision - 1, purpose, evidenceRevisionRefs: [revisionRef(evidence)] });
  expect(result).toMatchObject({ kind: 'agent_execution_requested', task: { id: taskId, state: 'active', canRequestAgent: false }, execution: { workItemId: taskId, attemptId: result.task.attemptId, contextId: result.task.contextId, requestedBy: principal, requesterResponsibility: command.actingAssignmentId, executedBy: 'organization-synthetic/agent-01', executorInvocationKind: 'agent', status: 'queued', result: null, purpose, evidenceRevisionRefs: [revisionRef(evidence)], providerPrincipalBindings: [{ providerId: 'document', principalId: 'poc/poc-agent', invocationKind: 'agent' }] } });
  currentAction('agent-result');
  const executionRegion = page.getByRole('region', { name: `Agent実行 ${result.execution.id}`, exact: true });
  try {
    await expect(executionRegion).toContainText('実行状態：成功');
  } catch (error) {
    // Failure-only observation of this same authorized execution. Never log its body, identifiers or purpose.
    try {
      const observed = await request.get(`${origin}/v1/organization/agent-executions/${result.execution.id}`, { timeout: 2000, maxRetries: 0, maxRedirects: 0 });
      if (observed.status() === 200) {
        const value: unknown = await observed.json();
        if (value && typeof value === 'object' && !Array.isArray(value)) {
          const { status, failureCode } = value as Record<string, unknown>;
          if (typeof status === 'string' && ['queued', 'running', 'succeeded', 'failed', 'cancelled', 'outcome_unknown'].includes(status) && (failureCode === null || typeof failureCode === 'string' && ['provider_denied', 'context_stale', 'invalid_output', 'dependency_unavailable', 'interrupted', 'commit_outcome_unknown'].includes(failureCode))) {
            test.info().annotations.push({ type: 'organization-agent-status', description: status }, { type: 'organization-agent-failure-code', description: failureCode ?? 'none' });
          }
        }
      }
    } catch { /* Preserve the original UI failure even when the bounded diagnostic read is unavailable. */ }
    throw error;
  }
  const openCandidate = executionRegion.getByRole('button', { name: '候補を根拠モジュールで確認', exact: true });
  await expect(openCandidate).toBeEnabled();
  const execution = await get<AgentExecution>(request, origin, `/v1/organization/agent-executions/${result.execution.id}`);
  const output = await get<AgentResult>(request, origin, `/v1/organization/agent-executions/${result.execution.id}/result`);
  expect(execution).toMatchObject({ ...result.execution, status: 'succeeded', taskRevision: result.task.revision + 1, startedAt: execution.startedAt, effectiveContextRevision: execution.effectiveContextRevision, endedAt: execution.endedAt, result: output });
  expect(execution.endedAt).not.toBeNull();
  expect(output).toMatchObject({ simulated: true, bodyAnalyzed: false, liveLlm: false, mcpWireExecuted: false, evidenceRevisionRefs: [revisionRef(evidence)] });
  expect(output.findingRevisionRefs).toHaveLength(1);
  expect(output.uncertainty.length).toBeGreaterThan(0);
  const finding = await get<Finding>(request, origin, `/v1/organization/findings/${output.findingRevisionRefs[0]!.id}`);
  expect(finding).toMatchObject({ ...output.findingRevisionRefs[0], taskId, attemptId: result.task.attemptId, contextId: result.task.contextId, author: 'organization-synthetic/agent-01', originExecutionId: execution.id, evidenceRevisionRefs: [revisionRef(evidence)], uncertainty: output.uncertainty, visibility: 'work_item_private' });
  expect(await get(request, origin, `/v1/organization/findings/${finding.id}/decisions`)).toEqual({ items: [], nextCursor: null });
  const beforeReplay = await get<TaskDetail>(request, origin, `/v1/organization/tasks/${taskId}`);
  expect(beforeReplay).toMatchObject({ state: 'active', revision: execution.taskRevision, agentExecutionIds: [execution.id] });
  if (principal === 'office-01') expect(beforeReplay).toMatchObject({ canEdit: false, workingArtifacts: [] });
  currentAction('agent-replay');
  const replay = await request.post(`${origin}/v1/organization/tasks/${taskId}/agent-executions`, { data: command });
  expect(replay.status()).toBe(202);
  expect(await replay.json()).toEqual(result);
  expect(await get(request, origin, `/v1/organization/operations/${command.operationId}`)).toEqual(result);
  expect(await get(request, origin, `/v1/organization/agent-executions/${execution.id}`)).toEqual(execution);
  expect(await get(request, origin, `/v1/organization/tasks/${taskId}`)).toEqual(beforeReplay);
  currentAction('agent-result');
  await openCandidate.click();
  const candidate = page.getByRole('region', { name: `候補 ${finding.id}`, exact: true });
  await expect(candidate).toContainText(finding.claim);
  await expect(candidate).toContainText('organization-synthetic/agent-01');
  await expect(candidate).toContainText(`生成元の実行 ${execution.id}`);
  return { operationId: command.operationId, command, result, execution, output, finding };
}

export async function assertAgentState(request: APIRequestContext, context: RuntimeContext, agents: AgentState) {
  for (const [role, receipt] of Object.entries(agents) as ['sales' | 'office', AgentState['sales']][]) {
    const origin = context[role], other = context[role === 'sales' ? 'office' : 'sales'];
    expect(await get(request, origin, `/v1/organization/agent-executions/${receipt.execution.id}`)).toEqual(receipt.execution);
    expect(await get(request, origin, `/v1/organization/agent-executions/${receipt.execution.id}/result`)).toEqual(receipt.output);
    expect(await get(request, origin, `/v1/organization/operations/${receipt.operationId}`)).toEqual(receipt.result);
    expect(await get(request, origin, `/v1/organization/operations/${receipt.decision.operationId}`)).toEqual(receipt.decision.result);
    expect(await get(request, origin, `/v1/organization/findings/${receipt.finding.id}`)).toEqual(receipt.finding);
    expect(await get(request, origin, `/v1/organization/findings/${receipt.finding.id}/decisions`)).toEqual({ items: [receipt.decision.result.decision], nextCursor: null });
    await assertHidden(request, other, `/v1/organization/agent-executions/${receipt.execution.id}`, 'WORK_ITEM_NOT_FOUND', receipt.command.purpose);
    await assertHidden(request, other, `/v1/organization/agent-executions/${receipt.execution.id}/result`, 'WORK_ITEM_NOT_FOUND', receipt.command.purpose);
    await assertHidden(request, other, `/v1/organization/operations/${receipt.operationId}`, 'WORK_ITEM_NOT_FOUND', receipt.command.purpose);
    await assertHidden(request, other, `/v1/organization/operations/${receipt.decision.operationId}`, 'WORK_ITEM_NOT_FOUND');
  }
  expect(await get(request, context.office, `/v1/organization/findings/${agents.sales.finding.id}`)).toEqual(agents.sales.finding);
  expect(await get(request, context.office, `/v1/organization/findings/${agents.sales.finding.id}/decisions`)).toEqual({ items: [agents.sales.decision.result.decision], nextCursor: null });
  await assertHidden(request, context.sales, `/v1/organization/findings/${agents.office.finding.id}`, 'FINDING_NOT_FOUND', agents.office.finding.claim);
  await assertHidden(request, context.sales, `/v1/organization/findings/${agents.office.finding.id}/decisions`, 'FINDING_NOT_FOUND');
}


export async function assertCompletionState(request: APIRequestContext, context: RuntimeContext, completion: PersistedState['completion']) {
  const taskId = completion.result.task.id;
  const before = await get<TaskDetail>(request, context.office, `/v1/organization/tasks/${taskId}`);
  expect(before).toMatchObject({ ...completion.result.task, state: 'completed', canClaim: false, canEdit: false, canSubmit: false, canReturn: false, canComplete: false, completionActionId: null, canRegisterEvidence: false, canRegisterFinding: false, canRecordDecision: false, canRequestAgent: false });
  expect(before.history.filter((entry) => entry.kind === 'completed')).toHaveLength(1);
  expect(await get(request, context.office, `/v1/organization/operations/${completion.operationId}`)).toEqual(completion.result);
  await assertHidden(request, context.sales, `/v1/organization/operations/${completion.operationId}`, 'WORK_ITEM_NOT_FOUND');
  currentAction('complete-replay');
  const replay = await request.post(`${context.office}/v1/organization/tasks/${taskId}/actions`, { data: completion.command });
  expect(replay.status()).toBe(200);
  expect(await replay.json()).toEqual(completion.result);
  const denied = await request.post(`${context.office}/v1/organization/tasks/${taskId}/actions`, { data: { ...completion.command, operationId: createOperationId(), expectedRevision: before.revision } });
  expect(denied.status()).toBe(409);
  expect(await denied.json()).toMatchObject({ code: 'HANDOFF_NOT_READY' });
  expect(await get(request, context.office, `/v1/organization/tasks/${taskId}`)).toEqual(before);
}

export type HoldResumeState = {
  hold: { operationId: string; command: WorkflowActionCommand; result: Held };
  resume: { operationId: string; command: WorkflowActionCommand; result: Resumed };
};

export async function holdAndResume(page: Page, request: APIRequestContext, origin: string, otherOrigin: string, taskId: string, session: WorkSession, unsaved: { label: string; text: string }): Promise<HoldResumeState> {
  const before = await get<TaskDetail>(request, origin, `/v1/organization/tasks/${taskId}`);
  expect(before).toMatchObject({ state: 'active', canHold: true, canResume: false, resumeActionId: null });
  expect(before.holdActionId).not.toBeNull();
  const readSaved = async () => ({
    evidence: await get(request, origin, `/v1/organization/tasks/${taskId}/evidence`),
    findings: await get(request, origin, `/v1/organization/tasks/${taskId}/findings`),
    snapshot: before.handoffSnapshotId ? await get(request, origin, `/v1/organization/handoff-snapshots/${before.handoffSnapshotId}`) : null,
    instruction: before.returnInstructionId ? await get(request, origin, `/v1/organization/return-instructions/${before.returnInstructionId}`) : null,
    executions: await Promise.all(before.agentExecutionIds.map((id) => get(request, origin, `/v1/organization/agent-executions/${id}`))),
  });
  const saved = await readSaved();
  await page.getByLabel(unsaved.label, { exact: true }).fill(unsaved.text);
  currentAction('hold-preview');
  await page.getByRole('button', { name: '保留内容を確認', exact: true }).click();
  const confirmation = page.getByRole('dialog', { name: '保留の確認', exact: true });
  await expect(confirmation).toContainText(before.attemptId);
  await expect(confirmation).toContainText(session.actingAssignmentId!);
  await expect(confirmation).toContainText('未保存の入力は保存せず');
  await expect(confirmation.getByRole('button', { name: 'キャンセル', exact: true })).toBeFocused();
  await confirmation.getByRole('button', { name: 'キャンセル', exact: true }).click();
  expect(await get(request, origin, `/v1/organization/tasks/${taskId}`)).toEqual(before);
  await expect(page.getByLabel(unsaved.label, { exact: true })).toHaveValue(unsaved.text);
  await page.getByRole('button', { name: '保留内容を確認', exact: true }).click();
  const heldResponse = page.waitForResponse((response) => new URL(response.url()).pathname === `/v1/organization/tasks/${taskId}/actions` && response.request().method() === 'POST');
  currentAction('hold-confirm');
  await page.getByRole('button', { name: '保留を確定', exact: true }).click();
  const holdResponse = await heldResponse;
  expect(holdResponse.status()).toBe(200);
  const held = await holdResponse.json() as Held;
  const holdCommand = holdResponse.request().postDataJSON() as WorkflowActionCommand;
  expect(holdCommand).toEqual({ operationId: expect.any(String), expectedRevision: before.revision, actingAssignmentId: session.actingAssignmentId, expectedAttemptId: before.attemptId, action: 'hold', definitionActionId: before.holdActionId });
  expect(held).toMatchObject({ kind: 'held', task: { id: taskId, attemptId: before.attemptId, attemptNumber: before.attemptNumber, revision: before.revision + 1, state: 'held', canClaim: false, canEdit: false, canSubmit: false, canReturn: false, canComplete: false, canHold: false, holdActionId: null, canResume: true, canRegisterEvidence: false, canRegisterFinding: false, canRecordDecision: false, canRequestAgent: false } });
  expect(held.task.resumeActionId).not.toBeNull();
  await expect(page.getByText('タスクを保留しました', { exact: true })).toBeVisible();
  await expect(page.getByText(/未保存の入力はこのタブ内だけに保持しています/)).toBeVisible();
  for (const name of ['作業中の文案', '差戻理由']) await expect(page.getByLabel(name, { exact: true })).toHaveCount(0);
  for (const name of ['文案を保存', '提出内容を確認', '完了内容を確認', '差戻内容を確認', '保留内容を確認']) await expect(page.getByRole('button', { name, exact: true })).toHaveCount(0);
  await expect(page.getByText(unsaved.text, { exact: true })).toHaveCount(0);
  if (before.workingArtifacts.length) {
    const readonly = page.getByRole('region', { name: '保存済みの作業文案', exact: true });
    for (const artifact of before.workingArtifacts) await expect(readonly).toContainText(artifact.value?.text ?? artifact.file!.fileName);
    await expect(readonly).not.toContainText(unsaved.text);
  }
  const heldDetail = await get<TaskDetail>(request, origin, `/v1/organization/tasks/${taskId}`);
  expect(heldDetail.history.at(-1)).toEqual({ kind: 'held', occurredAt: expect.any(String) });
  expect(heldDetail).toEqual({ ...before, ...held.task, history: [...before.history, heldDetail.history.at(-1)] });
  expect(await readSaved()).toEqual(saved);
  await assertHidden(request, otherOrigin, `/v1/organization/tasks/${taskId}`, 'WORK_ITEM_NOT_FOUND', unsaved.text);
  await assertHidden(request, otherOrigin, `/v1/organization/operations/${holdCommand.operationId}`, 'WORK_ITEM_NOT_FOUND', unsaved.text);
  await page.getByRole('button', { name: 'Agent', exact: true }).click();
  await expect(page.getByLabel('Agentへの依頼目的', { exact: true })).toBeDisabled();
  await expect(page.getByRole('button', { name: '実行を取消', exact: true })).toHaveCount(0);
  await page.getByRole('button', { name: '履歴', exact: true }).click();
  await expect(page.getByText('タスクを保留', { exact: true })).toHaveCount(heldDetail.history.filter((entry) => entry.kind === 'held').length);
  currentAction('hold-replay');
  expect(await get(request, origin, `/v1/organization/operations/${holdCommand.operationId}`)).toEqual(held);
  const holdReplay = await request.post(`${origin}/v1/organization/tasks/${taskId}/actions`, { data: holdCommand });
  expect(holdReplay.status()).toBe(200);
  expect(await holdReplay.json()).toEqual(held);
  expect(await get(request, origin, `/v1/organization/tasks/${taskId}`)).toEqual(heldDetail);

  currentAction('resume-preview');
  await page.getByRole('button', { name: '再開内容を確認', exact: true }).click();
  const resumeDialog = page.getByRole('dialog', { name: '再開の確認', exact: true });
  await expect(resumeDialog).toContainText(before.attemptId);
  await expect(resumeDialog).toContainText('Agentは自動再実行しません');
  await resumeDialog.getByRole('button', { name: 'キャンセル', exact: true }).click();
  expect(await get(request, origin, `/v1/organization/tasks/${taskId}`)).toEqual(heldDetail);
  await page.getByRole('button', { name: '再開内容を確認', exact: true }).click();
  const resumedResponse = page.waitForResponse((response) => new URL(response.url()).pathname === `/v1/organization/tasks/${taskId}/actions` && response.request().method() === 'POST');
  currentAction('resume-confirm');
  await page.getByRole('button', { name: '再開を確定', exact: true }).click();
  const resumeResponse = await resumedResponse;
  expect(resumeResponse.status()).toBe(200);
  const resumed = await resumeResponse.json() as Resumed;
  const resumeCommand = resumeResponse.request().postDataJSON() as WorkflowActionCommand;
  expect(resumeCommand).toEqual({ operationId: expect.any(String), expectedRevision: held.task.revision, actingAssignmentId: session.actingAssignmentId, expectedAttemptId: before.attemptId, action: 'resume', definitionActionId: held.task.resumeActionId });
  const { inputResources, workingArtifacts, history, agentExecutionIds, ...beforeSummary } = before;
  expect(resumed.task).toEqual({ ...beforeSummary, revision: before.revision + 2 });
  expect(resumed.kind).toBe('resumed');
  await expect(page.getByText('タスクを再開しました', { exact: true })).toBeVisible();
  await expect(page.getByLabel(unsaved.label, { exact: true })).toHaveValue(unsaved.text);
  const resumedDetail = await get<TaskDetail>(request, origin, `/v1/organization/tasks/${taskId}`);
  expect(resumedDetail.history.at(-1)).toEqual({ kind: 'resumed', occurredAt: expect.any(String) });
  expect(resumedDetail).toEqual({ ...before, ...resumed.task, history: [...heldDetail.history, resumedDetail.history.at(-1)] });
  expect(await readSaved()).toEqual(saved);
  await expect(page.getByText('タスクを再開', { exact: true })).toHaveCount(resumedDetail.history.filter((entry) => entry.kind === 'resumed').length);
  currentAction('resume-replay');
  expect(await get(request, origin, `/v1/organization/operations/${resumeCommand.operationId}`)).toEqual(resumed);
  const resumeReplay = await request.post(`${origin}/v1/organization/tasks/${taskId}/actions`, { data: resumeCommand });
  expect(resumeReplay.status()).toBe(200);
  expect(await resumeReplay.json()).toEqual(resumed);
  expect(await get(request, origin, `/v1/organization/tasks/${taskId}`)).toEqual(resumedDetail);
  return { hold: { operationId: holdCommand.operationId, command: holdCommand, result: held }, resume: { operationId: resumeCommand.operationId, command: resumeCommand, result: resumed } };
}

export async function assertHoldResumeState(request: APIRequestContext, context: RuntimeContext, transitions: PersistedState['holdResume']) {
  for (const role of ['sales', 'office'] as const) {
    const origin = context[role], other = context[role === 'sales' ? 'office' : 'sales'];
    const before = await get<TaskDetail>(request, origin, `/v1/organization/tasks/${transitions[role].hold.result.task.id}`);
    for (const action of ['hold', 'resume'] as const) {
      const receipt = transitions[role][action];
      expect(receipt.result.task.attemptId).toBe(before.attemptId);
      expect(await get(request, origin, `/v1/organization/operations/${receipt.operationId}`)).toEqual(receipt.result);
      await assertHidden(request, other, `/v1/organization/operations/${receipt.operationId}`, 'WORK_ITEM_NOT_FOUND');
      currentAction(action === 'hold' ? 'hold-replay' : 'resume-replay');
      const replay = await request.post(`${origin}/v1/organization/tasks/${before.id}/actions`, { data: receipt.command });
      expect(replay.status()).toBe(200);
      expect(await replay.json()).toEqual(receipt.result);
    }
    expect(await get(request, origin, `/v1/organization/tasks/${before.id}`)).toEqual(before);
    expect(before.history.filter((entry) => entry.kind === 'held')).toHaveLength(2);
    expect(before.history.filter((entry) => entry.kind === 'resumed')).toHaveLength(2);
  }
}
