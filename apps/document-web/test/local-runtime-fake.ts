import { RuntimeFailure, type BindingSummary, type ContextRef, type EntryPage, type LocalRef, type LocalWorkspace, type RuntimeAdapter } from '../src/runtime/contract';

type Calls = Array<{ method: string; args: unknown[] }>;
type FileNode = { kind: 'file'; bytes: Uint8Array; identity: string };
type DirNode = { kind: 'directory'; children: Map<string, FileNode | DirNode>; identity: string };

let identitySequence = 0;
const dir = (): DirNode => ({ kind: 'directory', children: new Map(), identity: `id-${identitySequence += 1}` });
const file = (text: string): FileNode => ({ kind: 'file', bytes: new TextEncoder().encode(text), identity: `id-${identitySequence += 1}` });

/** In-memory desktop runtime that follows the broker contract for UI tests. */
export function createFakeRuntime(seed: { workspaces?: Array<{ name: string; folders?: Record<string, Record<string, string | Record<string, string>>> }> } = {}) {
  const calls: Calls = [];
  const pickerQueue: Array<string | null> = [];
  const failures = new Map<string, RuntimeFailure[]>();
  const omitted = new Map<string, number>();
  const roots = new Map<string, DirNode>();
  const operations = new Map<string, unknown>();
  const workspaces: LocalWorkspace[] = [];
  const pickableFolders = new Map<string, DirNode>();
  let sequence = 0;
  const next = (prefix: string) => `${prefix}${sequence += 1}`;

  function fill(node: DirNode, content: Record<string, string | Record<string, string>>) {
    for (const [name, value] of Object.entries(content)) {
      if (typeof value === 'string') node.children.set(name, file(value));
      else { const child = dir(); fill(child, value); node.children.set(name, child); }
    }
  }
  function addWorkspace(name: string) {
    const managed = next('b_m');
    roots.set(managed, dir());
    const workspace: LocalWorkspace = {
      workspaceId: next('w_'), name, revision: 'r1', effectiveContextRevision: 'c0', scope: 'principal_device_local', managedBindingId: managed,
      bindings: [{ bindingId: managed, source: 'managed', label: '管理フォルダー', available: true }],
    };
    workspaces.push(workspace);
    return workspace;
  }
  function attachFolder(workspace: LocalWorkspace, label: string, node: DirNode): BindingSummary {
    const binding: BindingSummary = { bindingId: next('b_x'), source: 'explicit', label, available: true };
    roots.set(binding.bindingId, node);
    workspace.bindings = [...workspace.bindings, binding];
    workspace.effectiveContextRevision = `c${Number(workspace.effectiveContextRevision.slice(1)) + 1}`;
    return binding;
  }
  for (const item of seed.workspaces ?? []) {
    const workspace = addWorkspace(item.name);
    for (const [label, content] of Object.entries(item.folders ?? {})) {
      const node = dir(); fill(node, content); attachFolder(workspace, label, node);
    }
  }

  function record(method: string, args: unknown[]) {
    calls.push({ method, args });
    const queued = failures.get(method)?.shift();
    if (queued) throw queued;
  }
  function current(context: ContextRef) {
    const workspace = workspaces.find((item) => item.workspaceId === context.workspaceId);
    if (!workspace) throw new RuntimeFailure('not_found');
    if (workspace.effectiveContextRevision !== context.effectiveContextRevision) throw new RuntimeFailure('stale_context');
    return workspace;
  }
  function resolve(context: ContextRef, ref: LocalRef): DirNode {
    const workspace = current(context);
    if (!workspace.bindings.some((binding) => binding.bindingId === ref.bindingId)) throw new RuntimeFailure('not_found');
    let node = roots.get(ref.bindingId)!;
    for (const name of ref.locator) {
      const child = node.children.get(name);
      if (!child || child.kind !== 'directory') throw new RuntimeFailure('not_found');
      node = child;
    }
    return node;
  }
  const clone = <T>(value: T): T => JSON.parse(JSON.stringify(value)) as T;

  const runtime: RuntimeAdapter = {
    kind: 'desktop',
    capabilities: async () => ({ localResources: 'available', nativeDirectoryPicker: 'available', managedWorkspace: 'available', multiWindow: false, sidecar: false }),
    workspace: {
      listWorkspaces: async () => { record('listWorkspaces', []); return clone(workspaces); },
      createLocalWorkspace: async (name, operationId) => {
        record('createLocalWorkspace', [name, operationId]);
        if (!operations.has(operationId)) {
          const workspace = addWorkspace(name.trim());
          operations.set(operationId, { receipt: { operationId, workspaceId: workspace.workspaceId, managedBindingId: workspace.managedBindingId, runtimeRevision: '1' }, workspaceId: workspace.workspaceId });
        }
        const stored = operations.get(operationId) as { receipt: never; workspaceId: string };
        return { receipt: stored.receipt, workspace: clone(workspaces.find((item) => item.workspaceId === stored.workspaceId)!) };
      },
      renameWorkspace: async (context, name, operationId) => {
        record('renameWorkspace', [context, name, operationId]);
        const workspace = current(context);
        workspace.name = name.trim();
        return clone(workspace);
      },
      recoverWorkspace: async (operationId) => { record('recoverWorkspace', [operationId]); return { state: 'not_found' }; },
    },
    dialog: {
      chooseDirectory: async (context) => {
        record('chooseDirectory', [context]);
        current(context);
        const chosen = pickerQueue.shift() ?? null;
        if (chosen === null) return null;
        const selectionId = next('s_');
        pickableFolders.set(selectionId, Object.assign(dir(), { label: chosen }));
        return { selectionId };
      },
    },
    resources: {
      attachDirectory: async (context, selection, operationId) => {
        record('attachDirectory', [context, selection, operationId]);
        const workspace = current(context);
        const node = pickableFolders.get(selection.selectionId);
        if (!node) throw new RuntimeFailure('not_found', 'selection_expired');
        pickableFolders.delete(selection.selectionId);
        const binding = attachFolder(workspace, (node as DirNode & { label: string }).label, node);
        return { receipt: { operationId, bindingId: binding.bindingId, runtimeRevision: 'r2' }, workspace: clone(workspace) };
      },
      detachDirectory: async (context, bindingId, operationId) => {
        record('detachDirectory', [context, bindingId, operationId]);
        const workspace = current(context);
        workspace.bindings = workspace.bindings.filter((binding) => binding.bindingId !== bindingId);
        workspace.effectiveContextRevision = `c${Number(workspace.effectiveContextRevision.slice(1)) + 1}`;
        return clone(workspace);
      },
      listEntries: async (context, ref, cursor) => {
        record('listEntries', [context, ref, cursor]);
        const node = resolve(context, ref);
        const names = [...node.children.keys()].sort();
        const start = cursor ? names.indexOf(cursor) + 1 : 0;
        const slice = names.slice(start, start + 100);
        const page: EntryPage = {
          entries: slice.map((name) => {
            const child = node.children.get(name)!;
            return { locator: [...ref.locator, name], name, kind: child.kind, fileIdentity: child.identity };
          }),
          nextCursor: start + 100 < names.length ? slice.at(-1)! : null,
          omittedCount: omitted.get([ref.bindingId, ...ref.locator].join('/')) ?? 0,
        };
        return page;
      },
      openRead: async (context, ref, expected) => {
        record('openRead', [context, ref, expected]);
        const parent = resolve(context, { ...ref, locator: ref.locator.slice(0, -1) });
        const target = parent.children.get(ref.locator.at(-1)!);
        if (!target || target.kind !== 'file') throw new RuntimeFailure('not_found');
        if (target.identity !== expected) throw new RuntimeFailure('conflict', 'concurrent_change');
        return { readHandleId: `h:${ref.bindingId}:${ref.locator.join('/')}`, contentGeneration: 'g1', sizeBytes: target.bytes.length };
      },
      readFile: async (context, handle, offset, length) => {
        record('readFile', [context, handle, offset, length]);
        const [, bindingId, path] = handle.readHandleId.split(':');
        const parts = path!.split('/');
        const parent = resolve(context, { bindingId: bindingId!, locator: parts.slice(0, -1) });
        const target = parent.children.get(parts.at(-1)!) as FileNode;
        const bytes = target.bytes.slice(offset, offset + length);
        return { bytes, offset, contentGeneration: 'g1', eof: offset + bytes.length >= target.bytes.length };
      },
      closeRead: async (context, handle) => { record('closeRead', [context, handle]); },
      createFile: async (context, parent, name, bytes, operationId) => {
        record('createFile', [context, parent, name, bytes, operationId]);
        if (operations.has(operationId)) return operations.get(operationId) as never;
        const node = resolve(context, parent);
        if (node.children.has(name)) throw new RuntimeFailure('conflict', 'already_exists');
        const created: FileNode = { kind: 'file', bytes, identity: next('id-new') };
        node.children.set(name, created);
        const receipt = { operationId, ref: { ...parent, locator: [...parent.locator, name] }, fileIdentity: created.identity, sizeBytes: bytes.length, sha256: '0'.repeat(64) };
        operations.set(operationId, receipt);
        return receipt;
      },
    },
  };

  return {
    runtime,
    calls,
    callsOf: (method: string) => calls.filter((call) => call.method === method),
    pick: (label: string | null) => pickerQueue.push(label),
    fail: (method: string, failure: RuntimeFailure) => failures.set(method, [...(failures.get(method) ?? []), failure]),
    setOmitted: (key: string, value: number) => omitted.set(key, value),
    bumpContext: (index = 0) => { const workspace = workspaces[index]!; workspace.effectiveContextRevision = `c${Number(workspace.effectiveContextRevision.slice(1)) + 1}`; },
    workspaces,
  };
}
