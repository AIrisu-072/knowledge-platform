// Bounded Runtime Contract consumed by the application layer (Domain/API
// design section 12, with the local-Workspace additions recorded in
// docs/superpowers/specs/2026-10-07-desktop-workspace-runtime-amendment.md).
// Presentation never imports a desktop API; it only sees these types.

export type Availability = 'available' | 'unavailable';

export type RuntimeCapabilities = {
  localResources: Availability;
  nativeDirectoryPicker: Availability;
  managedWorkspace: Availability;
  multiWindow: false;
  sidecar: false;
};

export type RuntimeKind = 'browser' | 'desktop';

export type ContextRef = { effectiveContextRevision: string; workspaceId: string };
export type LocalRef = { bindingId: string; locator: string[] };

export type BindingSummary = {
  bindingId: string;
  source: 'managed' | 'explicit';
  /** Leaf folder name or managed-root label for presentation only; never a path. */
  label: string;
  available: boolean;
};

export type LocalWorkspace = {
  workspaceId: string;
  name: string;
  revision: string;
  effectiveContextRevision: string;
  scope: 'principal_device_local';
  managedBindingId: string;
  bindings: BindingSummary[];
};

export type RuntimeWorkspaceReceipt = { operationId: string; workspaceId: string; managedBindingId: string; runtimeRevision: string };
export type RuntimeWorkspaceOutcome =
  | { state: 'ready'; receipt: RuntimeWorkspaceReceipt }
  | { state: 'pending' | 'not_found' | 'unavailable' | 'outcome_unknown' };
export type WorkspaceCreated = { receipt: RuntimeWorkspaceReceipt; workspace: LocalWorkspace };
export type DirectorySelection = { selectionId: string };
export type BindingReceipt = { operationId: string; bindingId: string; runtimeRevision: string };
export type BindingAttached = { receipt: BindingReceipt; workspace: LocalWorkspace };
export type LocalEntry = { locator: string[]; name: string; kind: 'file' | 'directory'; fileIdentity: string };
export type EntryPage = { entries: LocalEntry[]; nextCursor: string | null; omittedCount: number };
export type ReadHandle = { readHandleId: string; contentGeneration: string; sizeBytes: number };
export type BytePage = { bytes: Uint8Array; offset: number; contentGeneration: string; eof: boolean };
export type FileReceipt = { operationId: string; ref: LocalRef; fileIdentity: string; sizeBytes: number; sha256: string };

export const RUNTIME_FAILURE_CODES = [
  'unavailable', 'invalid_locator', 'denied', 'stale_context', 'not_found', 'conflict', 'limit', 'cancelled', 'outcome_unknown',
] as const;
export type RuntimeFailureCode = typeof RUNTIME_FAILURE_CODES[number];

export const RUNTIME_FAILURE_REASONS = [
  'unsupported_platform', 'instance_locked', 'registry_unreadable', 'registry_write_failed', 'folder_replaced', 'symbolic_link',
  'linked_file', 'special_file', 'protected_location', 'managed_binding', 'already_bound', 'already_exists', 'concurrent_change',
  'safe_capture_unavailable', 'operation_mismatch', 'picker_busy', 'selection_expired', 'handle_expired', 'too_large', 'too_many',
  'invalid_name', 'invalid_cursor', 'invalid_request', 'unknown_command', 'io',
] as const;
export type RuntimeFailureReason = typeof RUNTIME_FAILURE_REASONS[number];

/** Typed, path-free runtime outcome. Raw OS diagnostics never reach here. */
export class RuntimeFailure extends Error {
  readonly code: RuntimeFailureCode;
  readonly reason: RuntimeFailureReason | undefined;
  constructor(code: RuntimeFailureCode, reason?: RuntimeFailureReason) {
    super(reason ? `${code}:${reason}` : code);
    this.name = 'RuntimeFailure';
    this.code = code;
    this.reason = reason;
  }
}

export const isRuntimeFailure = (value: unknown): value is RuntimeFailure => value instanceof RuntimeFailure;

export interface WorkspaceRuntime {
  listWorkspaces(): Promise<LocalWorkspace[]>;
  /** Creates a principal/device-local logical Workspace and its managed root. */
  createLocalWorkspace(name: string, operationId: string): Promise<WorkspaceCreated>;
  renameWorkspace(context: ContextRef, name: string, operationId: string): Promise<LocalWorkspace>;
  recoverWorkspace(operationId: string): Promise<RuntimeWorkspaceOutcome>;
}

export interface NativeDialogCapability {
  /** Resolves `null` when the user cancels; no other request is sent. */
  chooseDirectory(context: ContextRef): Promise<DirectorySelection | null>;
}

export interface LocalResourceCapability {
  attachDirectory(context: ContextRef, selection: DirectorySelection, operationId: string): Promise<BindingAttached>;
  detachDirectory(context: ContextRef, bindingId: string, operationId: string): Promise<LocalWorkspace>;
  listEntries(context: ContextRef, ref: LocalRef, cursor?: string): Promise<EntryPage>;
  openRead(context: ContextRef, ref: LocalRef, expectedFileIdentity: string): Promise<ReadHandle>;
  readFile(context: ContextRef, handle: ReadHandle, offset: number, length: number): Promise<BytePage>;
  closeRead(context: ContextRef, handle: ReadHandle): Promise<void>;
  createFile(context: ContextRef, parent: LocalRef, name: string, bytes: Uint8Array, operationId: string): Promise<FileReceipt>;
}

export type RuntimeAdapter = {
  kind: RuntimeKind;
  capabilities(): Promise<RuntimeCapabilities>;
  workspace: WorkspaceRuntime;
  dialog: NativeDialogCapability;
  resources: LocalResourceCapability;
};

export const contextOf = (workspace: LocalWorkspace): ContextRef => ({
  workspaceId: workspace.workspaceId,
  effectiveContextRevision: workspace.effectiveContextRevision,
});

export const MAX_READ_RANGE = 1024 * 1024;
export const MAX_CREATE_BYTES = 8 * 1024 * 1024;
