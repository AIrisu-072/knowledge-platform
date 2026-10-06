import type { CommandsCreateFolder, Folder, FolderChildren, MutationResult } from '@knowledge-platform/document-api-client';
import { readSelectedFolder, rootFolderOperations, sendRootFolderOperation, type SelectedFolderContext } from '../src/application/document-root-folder';
const available = { status: 'available' as const };
const context: SelectedFolderContext = { kind: 'selected', folderId: 'selected', sourceParentId: 'source', pageLimit: 3, name: '選択行' };
const row: Folder = { folderId: 'selected', parentFolderId: 'source', revision: 24, name: '現在名' };
const page = (items: Folder[], nextCursor: string | null = null): FolderChildren => ({ items, nextCursor, capabilities: { createFolder: available, createDocument: available, renameFolder: available, moveFolder: available, manageAccess: available } });
const request: CommandsCreateFolder = { operationId: 'operation', folderId: 'new', parentFolderId: 'selected', expectedParentRevision: 24, name: '子', reason: '理由' };
const receipt: MutationResult = { operationId: 'operation', resourceId: 'new', changed: true, resultingRevision: 0, occurredAt: '2026-10-05T12:00:00Z' };

test('3ページのfresh chainは空ページも順次読み最後のrow/capabilityを使用する', async () => {
  const calls: [string, string | undefined][] = [];
  const read = async (id: string, cursor?: string) => {
    calls.push([id, cursor]);
    return id === 'selected' ? page([{ ...row, folderId: 'child', revision: 0 }])
      : cursor === '新3+/=?' ? page([{ ...row, revision: 31 }])
        : cursor === '新2+/=?' ? page([], '新3+/=?') : page([row], '新2+/=?');
  };
  expect(await readSelectedFolder(context, read)).toMatchObject({ ...row, revision: 31, capabilities: page([]).capabilities });
  expect(calls).toEqual([['source', undefined], ['source', '新2+/=?'], ['source', '新3+/=?'], ['selected', undefined]]);
});

test('terminalで読取を止め、schema上省略可能なrow parentは実queryの親に結ぶ', async () => {
  const { parentFolderId: _parent, ...withoutParent } = row; const calls: string[] = [];
  const current = await readSelectedFolder(context, async id => { calls.push(id); return page(id === 'source' ? [withoutParent] : []); });
  expect(current.parentFolderId).toBe('source'); expect(calls).toEqual(['source', 'selected']);
});

test.each([0, -1, 1.5, Number.MAX_SAFE_INTEGER + 1])('不正な既取得ページ上限%pなら一度もAPIを読まない', async pageLimit => {
  let calls = 0; await expect(readSelectedFolder({ ...context, pageLimit }, async () => { calls++; return page([row]); })).rejects.toThrow(); expect(calls).toBe(0);
});

test.each(['', undefined, 12, 'repeat'] as const)('壊れた/循環cursor %pをfollowしない', async nextCursor => {
  const calls: [string, string | undefined][] = [];
  await expect(readSelectedFolder(context, async (id, cursor) => { calls.push([id, cursor]); return { ...page([row]), nextCursor } as FolderChildren; })).rejects.toThrow();
  expect(calls.every(([id]) => id === 'source')).toBe(true);
  expect(calls.length).toBe(nextCursor === 'repeat' ? 2 : 1);
});

test('対象が上限の先へ移動した場合は全走査も対象capability readもしない', async () => {
  const calls: [string, string | undefined][] = [];
  await expect(readSelectedFolder({ ...context, pageLimit: 1 }, async (id, cursor) => { calls.push([id, cursor]); return page([], 'beyond'); })).rejects.toThrow();
  expect(calls).toEqual([['source', undefined]]);
});

test('後続read失敗を先頭で発見したrowにfallbackしない', async () => {
  const calls: string[] = [];
  await expect(readSelectedFolder(context, async (id, cursor) => { calls.push(id); if (cursor) throw Error('read error'); return page([row], 'next'); })).rejects.toThrow('read error');
  expect(calls).toEqual(['source', 'source']);
});

test('shared storeはowner内で同一、別ownerと独立で、固定context/payloadをfreezeしてunknown再送に維持する', async () => {
  const owner = {}; const store = rootFolderOperations(owner); expect(rootFolderOperations(owner)).toBe(store); expect(rootFolderOperations({})).not.toBe(store);
  const mutable = { ...context }; const submitted: CommandsCreateFolder[] = [];
  await sendRootFolderOperation({ store, request, context: mutable, send: async body => { submitted.push(body); throw Error('lost'); }, invalidate: async () => {} });
  const saved = store.get()!; expect(saved.status).toBe('unknown'); expect(Object.isFrozen(saved.request)).toBe(true); expect(Object.isFrozen(saved.context)).toBe(true);
  mutable.sourceParentId = 'different';
  await sendRootFolderOperation({ store, request: { ...request, parentFolderId: 'root' }, context: { kind: 'root', name: 'System Root' }, send: async body => { submitted.push(body); return receipt; }, invalidate: async () => { throw Error('refresh'); } });
  expect(store.get()).toMatchObject({ status: 'succeeded', context }); expect(submitted[1]).toBe(submitted[0]);
  expect(store.clearSettled(saved)).toBe(false); expect(store.clearSettled(store.get()!)).toBe(true);
});

test.each([
  { ...receipt, resourceId: 'wrong' }, { ...receipt, operationId: 'wrong' },
  { ...receipt, changed: false }, { ...receipt, resultingRevision: 24 },
  { ...receipt, occurredAt: '2026-02-30T12:00:00Z' },
])('selected requestの不正receipt %pはunknownと元contextを保持する', async result => {
  const store = rootFolderOperations({});
  await sendRootFolderOperation({ store, request, context, send: async () => result, invalidate: async () => { throw Error('must not invalidate'); } });
  expect(store.get()).toMatchObject({ status: 'unknown', request, context });
});

test('開始時のpage上限を固定し、fresh read待ち中の取得済み範囲増加を追わない', async () => {
  const mutable = { ...context, pageLimit: 1 }; const calls: [string, string | undefined][] = [];
  let resolve!: (page: FolderChildren) => void; const first = new Promise<FolderChildren>(yes => { resolve = yes; });
  const pending = readSelectedFolder(mutable, async (id, cursor) => { calls.push([id, cursor]); return id === 'source' && cursor === undefined ? first : id === 'source' ? page([row], 'next') : page([]); });
  mutable.pageLimit = 3; resolve(page([row], 'next'));
  expect(await pending).toMatchObject(row); expect(calls).toEqual([['source', undefined], ['selected', undefined]]);
});
