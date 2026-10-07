import { useCallback, useRef, useState, useSyncExternalStore } from 'react';
import { useQuery, useQueryClient, type QueryClient } from '@tanstack/react-query';
import { createOperationId } from './operation-id';
import { useRuntime } from '../runtime/runtime-context';
import { contextOf, isRuntimeFailure, RuntimeFailure, type LocalRef, type LocalWorkspace } from '../runtime/contract';

export const localRuntimeKeys = {
  all: ['local-runtime'] as const,
  capabilities: ['local-runtime', 'capabilities'] as const,
  workspaces: ['local-runtime', 'workspaces'] as const,
  entries: (workspace: LocalWorkspace, ref: LocalRef, cursor: string | undefined) =>
    ['local-runtime', 'entries', workspace.workspaceId, workspace.effectiveContextRevision, ref.bindingId, ...ref.locator, `cursor:${cursor ?? ''}`] as const,
};

export function useRuntimeCapabilities() {
  const runtime = useRuntime();
  return useQuery({ queryKey: [...localRuntimeKeys.capabilities, runtime.kind], queryFn: () => runtime.capabilities(), staleTime: Infinity, retry: false });
}

export function useLocalWorkspaces(enabled: boolean) {
  const runtime = useRuntime();
  return useQuery({ queryKey: localRuntimeKeys.workspaces, queryFn: () => runtime.workspace.listWorkspaces(), enabled, retry: false, staleTime: 0 });
}

export function useLocalEntries(workspace: LocalWorkspace | undefined, ref: LocalRef | undefined, cursor: string | undefined) {
  const runtime = useRuntime();
  return useQuery({
    queryKey: workspace && ref ? localRuntimeKeys.entries(workspace, ref, cursor) : ['local-runtime', 'entries', 'none'],
    queryFn: () => runtime.resources.listEntries(contextOf(workspace!), ref!, cursor),
    enabled: Boolean(workspace && ref),
    retry: false,
    staleTime: 0,
  });
}

/** Safe Japanese explanation of a typed runtime failure. Never shows OS text. */
export function runtimeMessage(error: unknown): string {
  if (!isRuntimeFailure(error)) return 'ローカル実行環境に接続できません。デスクトップ版を再起動してください。';
  switch (error.reason) {
    case 'unsupported_platform': return 'この実行環境ではローカルフォルダーを利用できません。';
    case 'instance_locked': return 'デスクトップ版が別に起動しています。もう一方を終了してから開き直してください。';
    case 'registry_unreadable': return 'ローカルWorkspaceの記録を読み取れません。手順書の「記録を読み取れない場合」を確認してください。';
    case 'folder_replaced': return 'フォルダーが移動・削除・置き換えされたため利用できません。解除してから選び直してください。';
    case 'safe_capture_unavailable': return 'このファイルは安全に読み取れる状態を確認できないため開けません。';
    case 'symbolic_link': return 'リンク（シンボリックリンク・ジャンクション）は安全のため開けません。';
    case 'linked_file': return '複数の場所にリンクされたファイルは安全のため開けません。';
    case 'special_file': return '通常のファイルではないため開けません。';
    case 'protected_location': return 'アプリの管理領域は利用できません。';
    case 'managed_binding': return '管理フォルダーは解除できません。';
    case 'already_bound': return 'このフォルダーは既に追加されています。';
    case 'already_exists': return '同じ名前のファイルが既にあります。上書きはしません。別の名前にしてください。';
    case 'concurrent_change': return '操作中に内容が変更されました。一覧を更新してからもう一度試してください。';
    case 'operation_mismatch': return '同じ操作として異なる内容が送られました。入力内容を確認してください。';
    case 'picker_busy': return 'フォルダー選択画面が既に開いています。';
    case 'selection_expired': return 'フォルダーの選択が無効または期限切れです。もう一度選んでください。';
    case 'handle_expired': return '読み取りの有効期限が切れました。もう一度開いてください。';
    case 'too_large': return 'サイズの上限（読み取り・作成とも8MiB）を超えています。';
    case 'too_many': return '上限数に達しました（同時に開ける読み取りは4件まで）。';
    case 'invalid_name': case 'invalid_request': return '名前に使えない文字または予約名が含まれています。';
    default: break;
  }
  switch (error.code) {
    case 'stale_context': return 'Workspaceの状態が更新されました。最新の状態を表示したので、もう一度操作してください。';
    case 'not_found': return '対象が見つかりません。一覧を更新してください。';
    case 'denied': return 'アクセスが拒否されました。';
    case 'conflict': return '状態が競合しました。一覧を更新してください。';
    case 'limit': return '上限を超えています。';
    case 'invalid_locator': return '名前に使えない文字または予約名が含まれています。';
    case 'cancelled': return '取り消しました。';
    case 'outcome_unknown': return '結果を確認できませんでした。「結果を確認」で同じ操作として確認してください。新しい操作としては送りません。';
    default: return 'ローカル実行環境を利用できません。';
  }
}

export type RuntimeOperationState<T> =
  | { status: 'idle' }
  | { status: 'pending'; operationId: string; input: T }
  | { status: 'unknown'; operationId: string; input: T; error: RuntimeFailure }
  | { status: 'failed'; error: unknown };

/** Where an unresolved operation belongs, so the screen can return to it. */
export type OperationPlace = { workspaceId?: string; browse?: LocalRef };
export type UnresolvedOperation = { key: string; status: 'pending' | 'unknown'; operationId: string; input: unknown; error?: RuntimeFailure } & OperationPlace;

type Store = {
  get: (key: string) => UnresolvedOperation | undefined;
  set: (key: string, value: UnresolvedOperation | undefined) => void;
  all: () => readonly UnresolvedOperation[];
  subscribe: (listener: () => void) => () => void;
};
const stores = new WeakMap<QueryClient, Store>();

/**
 * Unresolved (pending or uncertain) runtime operations live with the app-wide
 * QueryClient, not in a component, so leaving the screen or a folder never
 * drops a retained operation ID. Nothing here is persisted or authoritative.
 */
function operationStore(client: QueryClient): Store {
  const existing = stores.get(client);
  if (existing) return existing;
  const items = new Map<string, UnresolvedOperation>();
  const listeners = new Set<() => void>();
  let snapshot: readonly UnresolvedOperation[] = [];
  const store: Store = {
    get: (key) => items.get(key),
    set: (key, value) => {
      if (value) items.set(key, value); else items.delete(key);
      snapshot = [...items.values()];
      listeners.forEach((listener) => listener());
    },
    all: () => snapshot,
    subscribe: (listener) => { listeners.add(listener); return () => listeners.delete(listener); },
  };
  stores.set(client, store);
  return store;
}

export function useUnresolvedRuntimeOperations(): readonly UnresolvedOperation[] {
  const store = operationStore(useQueryClient());
  return useSyncExternalStore(store.subscribe, store.all, store.all);
}

/**
 * One mutation with a retained operation ID. A pending operation blocks only
 * a duplicate of itself; an uncertain result keeps the same operation ID and
 * input so "結果を確認" replays exactly that operation, never a new one.
 */
export function useRuntimeOperation<T, R>(key: string, place: OperationPlace, run: (input: T, operationId: string) => Promise<R>, onDone?: (result: R) => void | Promise<void>) {
  const client = useQueryClient();
  const store = operationStore(client);
  const retained = useSyncExternalStore(store.subscribe, () => store.get(key), () => store.get(key));
  const [failure, setFailure] = useState<unknown>();
  const inFlight = useRef(false);
  const state: RuntimeOperationState<T> = retained?.status === 'unknown'
    ? { status: 'unknown', operationId: retained.operationId, input: retained.input as T, error: retained.error! }
    : retained?.status === 'pending'
      ? { status: 'pending', operationId: retained.operationId, input: retained.input as T }
      : failure !== undefined ? { status: 'failed', error: failure } : { status: 'idle' };
  const submit = useCallback(async (input: T) => {
    const current = store.get(key);
    if (inFlight.current || current?.status === 'pending') return undefined;
    const operationId = current?.operationId ?? createOperationId();
    const value = (current?.input as T | undefined) ?? input;
    inFlight.current = true;
    setFailure(undefined);
    store.set(key, { key, status: 'pending', operationId, input: value, ...place });
    try {
      const result = await run(value, operationId);
      store.set(key, undefined);
      await onDone?.(result);
      return result;
    } catch (error) {
      if (isRuntimeFailure(error) && error.code === 'outcome_unknown') {
        store.set(key, { key, status: 'unknown', operationId, input: value, error, ...place });
      } else {
        store.set(key, undefined);
        setFailure(error);
        if (isRuntimeFailure(error) && (error.code === 'stale_context' || error.code === 'not_found' || error.reason === 'folder_replaced')) {
          await client.invalidateQueries({ queryKey: localRuntimeKeys.all });
        }
      }
      return undefined;
    } finally {
      inFlight.current = false;
    }
  }, [client, key, onDone, place, run, store]);
  const reset = useCallback(() => { if (!store.get(key)) setFailure(undefined); }, [key, store]);
  return { state, submit, reset };
}

export function decodePreview(bytes: Uint8Array): { text: string } | { binary: true } {
  if (bytes.includes(0)) return { binary: true };
  return { text: new TextDecoder('utf-8', { fatal: false }).decode(bytes) };
}

export function formatBytes(size: number): string {
  if (size < 1024) return `${size} B`;
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KiB`;
  return `${(size / (1024 * 1024)).toFixed(1)} MiB`;
}
