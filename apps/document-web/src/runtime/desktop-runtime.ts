import {
  MAX_CREATE_BYTES, MAX_READ_RANGE, RUNTIME_FAILURE_CODES, RUNTIME_FAILURE_REASONS, RuntimeFailure,
  type BindingAttached, type BindingSummary, type BytePage, type ContextRef, type EntryPage, type FileReceipt, type LocalEntry,
  type LocalRef, type LocalWorkspace, type ReadHandle, type RuntimeAdapter, type RuntimeCapabilities, type RuntimeFailureCode,
  type RuntimeFailureReason, type RuntimeWorkspaceOutcome, type RuntimeWorkspaceReceipt, type WorkspaceCreated,
} from './contract';

/** The single IPC command the desktop shell registers. */
export const RUNTIME_IPC_COMMAND = 'local_workspace_runtime';
export type InvokeFn = (command: string, args: { command: string; request: unknown }) => Promise<unknown>;

class Malformed extends Error {}

type Json = Record<string, unknown>;
function record(value: unknown, keys: readonly string[]): Json {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Malformed();
  const item = value as Json;
  // Exact key sets: an unexpected field (for example a path) is never accepted.
  if (Object.keys(item).length !== keys.length || !keys.every((key) => key in item)) throw new Malformed();
  return item;
}
function text(value: unknown, allowEmpty = false): string {
  if (typeof value !== 'string' || (!allowEmpty && value.length === 0)) throw new Malformed();
  return value;
}
function count(value: unknown): number {
  if (typeof value !== 'number' || !Number.isSafeInteger(value) || value < 0) throw new Malformed();
  return value;
}
function flag(value: unknown): boolean {
  if (typeof value !== 'boolean') throw new Malformed();
  return value;
}
function oneOf<T extends string>(value: unknown, allowed: readonly T[]): T {
  if (typeof value !== 'string' || !allowed.includes(value as T)) throw new Malformed();
  return value as T;
}
function locator(value: unknown): string[] {
  if (!Array.isArray(value) || value.length > 32) throw new Malformed();
  return value.map((part) => {
    const name = text(part);
    if (name === '.' || name === '..' || /[/\\\u0000]/u.test(name)) throw new Malformed();
    return name;
  });
}
function localRef(value: unknown): LocalRef {
  const item = record(value, ['bindingId', 'locator']);
  return { bindingId: text(item.bindingId), locator: locator(item.locator) };
}

const availability = ['available', 'unavailable'] as const;
function capabilities(value: unknown): RuntimeCapabilities {
  const item = record(value, ['localResources', 'nativeDirectoryPicker', 'managedWorkspace', 'multiWindow', 'sidecar']);
  if (item.multiWindow !== false || item.sidecar !== false) throw new Malformed();
  return {
    localResources: oneOf(item.localResources, availability),
    nativeDirectoryPicker: oneOf(item.nativeDirectoryPicker, availability),
    managedWorkspace: oneOf(item.managedWorkspace, availability),
    multiWindow: false,
    sidecar: false,
  };
}
function binding(value: unknown): BindingSummary {
  const item = record(value, ['bindingId', 'source', 'label', 'available']);
  return { bindingId: text(item.bindingId), source: oneOf(item.source, ['managed', 'explicit'] as const), label: text(item.label), available: flag(item.available) };
}
function workspace(value: unknown): LocalWorkspace {
  const item = record(value, ['workspaceId', 'name', 'revision', 'effectiveContextRevision', 'scope', 'managedBindingId', 'bindings']);
  if (item.scope !== 'principal_device_local' || !Array.isArray(item.bindings)) throw new Malformed();
  return {
    workspaceId: text(item.workspaceId), name: text(item.name), revision: text(item.revision),
    effectiveContextRevision: text(item.effectiveContextRevision), scope: 'principal_device_local',
    managedBindingId: text(item.managedBindingId), bindings: item.bindings.map(binding),
  };
}
function workspaceList(value: unknown): LocalWorkspace[] {
  if (!Array.isArray(value)) throw new Malformed();
  return value.map(workspace);
}
function workspaceReceipt(value: unknown): RuntimeWorkspaceReceipt {
  const item = record(value, ['operationId', 'workspaceId', 'managedBindingId', 'runtimeRevision']);
  return { operationId: text(item.operationId), workspaceId: text(item.workspaceId), managedBindingId: text(item.managedBindingId), runtimeRevision: text(item.runtimeRevision) };
}
function workspaceCreated(value: unknown, operationId: string): WorkspaceCreated {
  const item = record(value, ['receipt', 'workspace']);
  const created = { receipt: workspaceReceipt(item.receipt), workspace: workspace(item.workspace) };
  if (created.receipt.operationId !== operationId || created.receipt.workspaceId !== created.workspace.workspaceId) throw new Malformed();
  return created;
}
function outcome(value: unknown, operationId: string): RuntimeWorkspaceOutcome {
  const state = oneOf((value as Json | null)?.state, ['ready', 'pending', 'not_found', 'unavailable', 'outcome_unknown'] as const);
  if (state === 'ready') {
    const receipt = workspaceReceipt(record(value, ['state', 'receipt']).receipt);
    if (receipt.operationId !== operationId) throw new Malformed();
    return { state, receipt };
  }
  record(value, ['state']);
  return { state };
}
function selection(value: unknown) {
  if (value === null) return null;
  return { selectionId: text(record(value, ['selectionId']).selectionId) };
}
function attached(value: unknown, operationId: string): BindingAttached {
  const item = record(value, ['receipt', 'workspace']);
  const receipt = record(item.receipt, ['operationId', 'bindingId', 'runtimeRevision']);
  const result = {
    receipt: { operationId: text(receipt.operationId), bindingId: text(receipt.bindingId), runtimeRevision: text(receipt.runtimeRevision) },
    workspace: workspace(item.workspace),
  };
  if (result.receipt.operationId !== operationId || !result.workspace.bindings.some((item) => item.bindingId === result.receipt.bindingId)) throw new Malformed();
  return result;
}
function entry(value: unknown): LocalEntry {
  const item = record(value, ['locator', 'name', 'kind', 'fileIdentity']);
  const path = locator(item.locator);
  const name = text(item.name);
  if (path.at(-1) !== name) throw new Malformed();
  return { locator: path, name, kind: oneOf(item.kind, ['file', 'directory'] as const), fileIdentity: text(item.fileIdentity) };
}
const sameLocator = (a: readonly string[], b: readonly string[]) => a.length === b.length && a.every((part, index) => part === b[index]);
function entryPage(value: unknown, parent: LocalRef): EntryPage {
  const item = record(value, ['entries', 'nextCursor', 'omittedCount']);
  if (!Array.isArray(item.entries) || item.entries.length > 100) throw new Malformed();
  const entries = item.entries.map(entry);
  // Every entry must be a direct child of the directory that was listed.
  if (!entries.every((child) => sameLocator(child.locator.slice(0, -1), parent.locator))) throw new Malformed();
  return { entries, nextCursor: item.nextCursor === null ? null : text(item.nextCursor), omittedCount: count(item.omittedCount) };
}
function readHandle(value: unknown): ReadHandle {
  const item = record(value, ['readHandleId', 'contentGeneration', 'sizeBytes']);
  return { readHandleId: text(item.readHandleId), contentGeneration: text(item.contentGeneration), sizeBytes: count(item.sizeBytes) };
}

const BASE64 = /^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/u;
export function decodeBase64(value: string): Uint8Array {
  if (!BASE64.test(value)) throw new Malformed();
  const binary = atob(value);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index);
  return bytes;
}
export function encodeBase64(bytes: Uint8Array): string {
  let binary = '';
  for (let index = 0; index < bytes.length; index += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(index, index + 0x8000));
  }
  return btoa(binary);
}
function bytePage(value: unknown, handle: ReadHandle, offset: number, length: number): BytePage {
  const item = record(value, ['bytesBase64', 'offset', 'contentGeneration', 'eof']);
  const bytes = decodeBase64(text(item.bytesBase64, true));
  const page = { bytes, offset: count(item.offset), contentGeneration: text(item.contentGeneration), eof: flag(item.eof) };
  // The range must belong to the same handle generation and requested window.
  const end = Math.min(offset + length, handle.sizeBytes);
  if (page.offset !== offset || page.contentGeneration !== handle.contentGeneration || bytes.length !== Math.max(0, end - offset)
    || page.eof !== (end === handle.sizeBytes)) throw new Malformed();
  return page;
}
function fileReceipt(value: unknown, operationId: string, parent: LocalRef, name: string, size: number): FileReceipt {
  const item = record(value, ['operationId', 'ref', 'fileIdentity', 'sizeBytes', 'sha256']);
  const receipt = { operationId: text(item.operationId), ref: localRef(item.ref), fileIdentity: text(item.fileIdentity), sizeBytes: count(item.sizeBytes), sha256: text(item.sha256) };
  if (receipt.operationId !== operationId || receipt.ref.bindingId !== parent.bindingId || !sameLocator(receipt.ref.locator, [...parent.locator, name])
    || receipt.sizeBytes !== size || !/^[0-9a-f]{64}$/u.test(receipt.sha256)) throw new Malformed();
  return receipt;
}
function nothing(value: unknown): void {
  if (value !== null && value !== undefined) throw new Malformed();
}

/**
 * Map an IPC rejection to a typed failure. Only the closed code/reason sets
 * survive; any other value (raw OS text, transport errors) is reduced to
 * `unavailable` for reads and `outcome_unknown` for mutations.
 */
function toFailure(error: unknown, mutation: boolean): RuntimeFailure {
  if (error && typeof error === 'object' && !Array.isArray(error)) {
    const { code, reason } = error as Json;
    if (typeof code === 'string' && (RUNTIME_FAILURE_CODES as readonly string[]).includes(code)) {
      const safeReason = typeof reason === 'string' && (RUNTIME_FAILURE_REASONS as readonly string[]).includes(reason) ? reason as RuntimeFailureReason : undefined;
      return new RuntimeFailure(code as RuntimeFailureCode, safeReason);
    }
  }
  return new RuntimeFailure(mutation ? 'outcome_unknown' : 'unavailable');
}

export function createDesktopRuntime(invoke: InvokeFn): RuntimeAdapter {
  async function call<T>(command: string, request: unknown, parse: (value: unknown) => T, mutation: boolean): Promise<T> {
    let value: unknown;
    try {
      value = await invoke(RUNTIME_IPC_COMMAND, { command, request });
    } catch (error) {
      throw toFailure(error, mutation);
    }
    try {
      return parse(value);
    } catch {
      throw new RuntimeFailure(mutation ? 'outcome_unknown' : 'unavailable');
    }
  }
  const read = <T>(command: string, request: unknown, parse: (value: unknown) => T) => call(command, request, parse, false);
  const mutate = <T>(command: string, request: unknown, parse: (value: unknown) => T) => call(command, request, parse, true);
  const tooLarge = () => Promise.reject(new RuntimeFailure('limit', 'too_large'));

  return {
    kind: 'desktop',
    capabilities: () => read('capabilities', null, capabilities),
    workspace: {
      listWorkspaces: () => read('workspace.list', null, workspaceList),
      createLocalWorkspace: (name, operationId) => mutate('workspace.create', { name, operationId }, (value) => workspaceCreated(value, operationId)),
      renameWorkspace: (context: ContextRef, name, operationId) => mutate('workspace.rename', { context, name, operationId }, (value) => {
        const renamed = workspace(value);
        if (renamed.workspaceId !== context.workspaceId) throw new Malformed();
        return renamed;
      }),
      // Recovery may complete an interrupted creation, so it is a mutation.
      recoverWorkspace: (operationId) => mutate('workspace.recover', { operationId }, (value) => outcome(value, operationId)),
    },
    dialog: {
      chooseDirectory: (context) => read('directory.choose', { context }, selection),
    },
    resources: {
      attachDirectory: (context, chosen, operationId) => mutate('directory.attach', { context, selectionId: chosen.selectionId, operationId }, (value) => attached(value, operationId)),
      detachDirectory: (context, bindingId, operationId) => mutate('directory.detach', { context, bindingId, operationId }, (value) => {
        const detached = workspace(value);
        if (detached.workspaceId !== context.workspaceId || detached.bindings.some((item) => item.bindingId === bindingId)) throw new Malformed();
        return detached;
      }),
      listEntries: (context, ref, cursor) => read('entries.list', { context, ref, cursor: cursor ?? null }, (value) => entryPage(value, ref)),
      openRead: (context, ref, expectedFileIdentity) => read('file.openRead', { context, ref, expectedFileIdentity }, readHandle),
      readFile: (context, handle, offset, length) => length > MAX_READ_RANGE || length < 1
        ? tooLarge()
        : read('file.read', { context, readHandleId: handle.readHandleId, offset, length }, (value) => bytePage(value, handle, offset, length)),
      closeRead: (context, handle) => read('file.closeRead', { context, readHandleId: handle.readHandleId }, nothing),
      createFile: (context, parent, name, bytes, operationId) => bytes.length > MAX_CREATE_BYTES
        ? tooLarge()
        : mutate('file.create', { context, parent, name, bytesBase64: encodeBase64(bytes), operationId }, (value) => fileReceipt(value, operationId, parent, name, bytes.length)),
    },
  };
}
