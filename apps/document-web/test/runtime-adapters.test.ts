import { TextDecoder, TextEncoder } from 'node:util';
import { browserRuntime } from '../src/runtime/browser-runtime';
import { createDesktopRuntime, RUNTIME_IPC_COMMAND } from '../src/runtime/desktop-runtime';
import { selectRuntime } from '../src/runtime/select-runtime';
import { RuntimeFailure, type ContextRef } from '../src/runtime/contract';

const context: ContextRef = { workspaceId: 'w_1', effectiveContextRevision: 'c1' };
const workspace = {
  workspaceId: 'w_1', name: '案件A', revision: 'r1', effectiveContextRevision: 'c1', scope: 'principal_device_local', managedBindingId: 'b_m',
  bindings: [{ bindingId: 'b_m', source: 'managed', label: '管理フォルダー', available: true }],
};

function fakeInvoke(responses: Array<unknown | { reject: unknown }>) {
  const calls: Array<{ command: string; request: unknown }> = [];
  const invoke = jest.fn(async (name: string, args: { command: string; request: unknown }) => {
    expect(name).toBe(RUNTIME_IPC_COMMAND);
    calls.push(args);
    const next = responses.shift();
    if (next && typeof next === 'object' && 'reject' in next) throw (next as { reject: unknown }).reject;
    return next;
  });
  return { invoke, calls };
}

async function failure(promise: Promise<unknown>) {
  try { await promise; } catch (error) { if (error instanceof RuntimeFailure) return { code: error.code, reason: error.reason }; throw error; }
  throw new Error('expected a RuntimeFailure');
}

test('browser runtime reports every capability unavailable and never fabricates local state', async () => {
  expect(browserRuntime.kind).toBe('browser');
  await expect(browserRuntime.capabilities()).resolves.toEqual({
    localResources: 'unavailable', nativeDirectoryPicker: 'unavailable', managedWorkspace: 'unavailable', multiWindow: false, sidecar: false,
  });
  for (const call of [
    () => browserRuntime.workspace.listWorkspaces(),
    () => browserRuntime.workspace.createLocalWorkspace('x', 'op'),
    () => browserRuntime.dialog.chooseDirectory(context),
    () => browserRuntime.resources.listEntries(context, { bindingId: 'b', locator: [] }),
    () => browserRuntime.resources.createFile(context, { bindingId: 'b', locator: [] }, 'a.txt', new Uint8Array(), 'op'),
  ]) {
    await expect(failure(call())).resolves.toEqual({ code: 'unavailable', reason: 'unsupported_platform' });
  }
});

test('desktop adapter sends only the single bounded IPC command with camelCase requests', async () => {
  const { invoke, calls } = fakeInvoke([
    { localResources: 'available', nativeDirectoryPicker: 'available', managedWorkspace: 'available', multiWindow: false, sidecar: false },
    { receipt: { operationId: 'op-1', workspaceId: 'w_1', managedBindingId: 'b_m', runtimeRevision: '1' }, workspace },
    null,
    { selectionId: 's_1' },
    { receipt: { operationId: 'op-2', bindingId: 'b_x', runtimeRevision: 'r2' }, workspace: { ...workspace, effectiveContextRevision: 'c2', bindings: [...workspace.bindings, { bindingId: 'b_x', source: 'explicit', label: '資料', available: true }] } },
  ]);
  const runtime = createDesktopRuntime(invoke);
  expect(runtime.kind).toBe('desktop');
  await expect(runtime.capabilities()).resolves.toMatchObject({ localResources: 'available', multiWindow: false });
  const created = await runtime.workspace.createLocalWorkspace('案件A', 'op-1');
  expect(created.workspace.name).toBe('案件A');
  await expect(runtime.dialog.chooseDirectory(context)).resolves.toBeNull();
  await expect(runtime.dialog.chooseDirectory(context)).resolves.toEqual({ selectionId: 's_1' });
  const attached = await runtime.resources.attachDirectory(context, { selectionId: 's_1' }, 'op-2');
  expect(attached.workspace.effectiveContextRevision).toBe('c2');
  expect(calls).toEqual([
    { command: 'capabilities', request: null },
    { command: 'workspace.create', request: { name: '案件A', operationId: 'op-1' } },
    { command: 'directory.choose', request: { context } },
    { command: 'directory.choose', request: { context } },
    { command: 'directory.attach', request: { context, selectionId: 's_1', operationId: 'op-2' } },
  ]);
});

test('bytes travel as Base64 and contract bounds are enforced before IPC', async () => {
  const { invoke, calls } = fakeInvoke([
    { bytesBase64: '5pys5paH', offset: 0, contentGeneration: 'g1', eof: true },
    { operationId: 'op-3', ref: { bindingId: 'b_m', locator: ['a.txt'] }, fileIdentity: 'id', sizeBytes: 6, sha256: 'f'.repeat(64) },
  ]);
  const runtime = createDesktopRuntime(invoke);
  const handle = { readHandleId: 'h_1', contentGeneration: 'g1', sizeBytes: 6 };
  const page = await runtime.resources.readFile(context, handle, 0, 1024);
  expect(new TextDecoder().decode(page.bytes)).toBe('本文');
  await runtime.resources.createFile(context, { bindingId: 'b_m', locator: [] }, 'a.txt', new TextEncoder().encode('本文'), 'op-3');
  expect(calls[1]).toEqual({ command: 'file.create', request: {
    context, parent: { bindingId: 'b_m', locator: [] }, name: 'a.txt', bytesBase64: '5pys5paH', operationId: 'op-3',
  } });
  await expect(failure(runtime.resources.readFile(context, handle, 0, 1024 * 1024 + 1))).resolves.toEqual({ code: 'limit', reason: 'too_large' });
  await expect(failure(runtime.resources.createFile(context, { bindingId: 'b_m', locator: [] }, 'big', new Uint8Array(8 * 1024 * 1024 + 1), 'op-4')))
    .resolves.toEqual({ code: 'limit', reason: 'too_large' });
  expect(invoke).toHaveBeenCalledTimes(2);
});

test('typed rejections keep only known safe codes and reasons', async () => {
  const { invoke } = fakeInvoke([
    { reject: { code: 'stale_context' } },
    { reject: { code: 'denied', reason: 'symbolic_link' } },
    { reject: { code: 'denied', reason: '/home/user/secret path' } },
    { reject: 'raw OS error: /etc/passwd' },
    { reject: 'IPC transport closed' },
  ]);
  const runtime = createDesktopRuntime(invoke);
  const ref = { bindingId: 'b', locator: [] };
  await expect(failure(runtime.resources.listEntries(context, ref))).resolves.toEqual({ code: 'stale_context', reason: undefined });
  await expect(failure(runtime.resources.listEntries(context, ref))).resolves.toEqual({ code: 'denied', reason: 'symbolic_link' });
  await expect(failure(runtime.resources.listEntries(context, ref))).resolves.toEqual({ code: 'denied', reason: undefined });
  // An unknown failure of a read is unavailable; of a mutation it is outcome_unknown.
  await expect(failure(runtime.resources.listEntries(context, ref))).resolves.toEqual({ code: 'unavailable', reason: undefined });
  await expect(failure(runtime.resources.createFile(context, ref, 'a', new Uint8Array([1]), 'op'))).resolves.toEqual({ code: 'outcome_unknown', reason: undefined });
});

test('malformed success values are never trusted', async () => {
  const { invoke } = fakeInvoke([
    { entries: [{ locator: ['../x'], name: 'x', kind: 'symlink', fileIdentity: 'i' }], nextCursor: null, omittedCount: 0 },
    { receipt: { operationId: 'op' }, workspace: { ...workspace, path: '/home/user' } },
    { bytesBase64: '%%%', offset: 0, contentGeneration: 'g', eof: true },
  ]);
  const runtime = createDesktopRuntime(invoke);
  await expect(failure(runtime.resources.listEntries(context, { bindingId: 'b', locator: [] }))).resolves.toMatchObject({ code: 'unavailable' });
  await expect(failure(runtime.workspace.createLocalWorkspace('x', 'op'))).resolves.toMatchObject({ code: 'outcome_unknown' });
  await expect(failure(runtime.resources.readFile(context, { readHandleId: 'h', contentGeneration: 'g', sizeBytes: 1 }, 0, 1)))
    .resolves.toMatchObject({ code: 'unavailable' });
});

test('runtime selection uses the desktop bridge only when the shell injected it', () => {
  expect(selectRuntime({}).kind).toBe('browser');
  expect(selectRuntime({ __TAURI__: { core: {} } }).kind).toBe('browser');
  expect(selectRuntime({ __TAURI__: { core: { invoke: jest.fn() } } }).kind).toBe('desktop');
});

test('replies must match the request they answer', async () => {
  const handle = { readHandleId: 'h_1', contentGeneration: 'g1', sizeBytes: 3 };
  const parent = { bindingId: 'b_m', locator: ['sub'] };
  const { invoke } = fakeInvoke([
    { bytesBase64: 'YWJj', offset: 1, contentGeneration: 'g1', eof: true },
    { bytesBase64: 'YWJj', offset: 0, contentGeneration: 'other', eof: true },
    { bytesBase64: 'YWI=', offset: 0, contentGeneration: 'g1', eof: true },
    { operationId: 'someone-else', ref: { ...parent, locator: ['sub', 'a.txt'] }, fileIdentity: 'i', sizeBytes: 1, sha256: '0'.repeat(64) },
    { operationId: 'op-1', ref: { ...parent, locator: ['other.txt'] }, fileIdentity: 'i', sizeBytes: 1, sha256: '0'.repeat(64) },
    { entries: [{ locator: ['elsewhere', 'x'], name: 'x', kind: 'file', fileIdentity: 'i' }], nextCursor: null, omittedCount: 0 },
    { reject: 'IPC transport closed' },
  ]);
  const runtime = createDesktopRuntime(invoke);
  await expect(failure(runtime.resources.readFile(context, handle, 0, 3))).resolves.toMatchObject({ code: 'unavailable' });
  await expect(failure(runtime.resources.readFile(context, handle, 0, 3))).resolves.toMatchObject({ code: 'unavailable' });
  await expect(failure(runtime.resources.readFile(context, handle, 0, 3))).resolves.toMatchObject({ code: 'unavailable' });
  await expect(failure(runtime.resources.createFile(context, parent, 'a.txt', new Uint8Array([1]), 'op-1'))).resolves.toMatchObject({ code: 'outcome_unknown' });
  await expect(failure(runtime.resources.createFile(context, parent, 'a.txt', new Uint8Array([1]), 'op-1'))).resolves.toMatchObject({ code: 'outcome_unknown' });
  await expect(failure(runtime.resources.listEntries(context, parent))).resolves.toMatchObject({ code: 'unavailable' });
  // Recovery completes an interrupted creation, so a lost reply is uncertain.
  await expect(failure(runtime.workspace.recoverWorkspace('op-1'))).resolves.toMatchObject({ code: 'outcome_unknown' });
});
