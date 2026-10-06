import { useEffect, useRef, useState, useSyncExternalStore } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { Dialog, Heading, Modal } from 'react-aria-components';
import { documentApi, type Folder, type FolderDetail } from '../../application/document-workspace';
import { canCreateRootFolder, folderName, rootFolderOperations, rootFolderValidation, sendRootFolderOperation, readSelectedFolder, type FolderCreateContext, type SelectedFolderContext } from '../../application/document-root-folder';
import { folderRenameOperations } from '../../application/document-folder-rename';
import { folderMoveOperations } from '../../application/document-folder-move';
import { metadataReason } from '../../application/document-metadata';
import { createOperationId } from '../../application/operation-id';
import { mapApiProblem, problemFromUnknown } from '../../application/problem-mapping';
import { CapabilityButton, availabilityReason } from '../shared/CapabilityButton';
import styles from './DocumentMetadataEditor.module.css';
import workspace from '../../routes/DocumentWorkspace.module.css';

const rootTitle = 'System Rootにフォルダーを作成';
const selectedTitle = '選択したフォルダーに子フォルダーを作成';
const rootContext: FolderCreateContext = { kind: 'root', name: 'System Root' };
export function RootFolderCreate({ root, readReady, reload, contextKey, selected, selectedFolderId }: {
  root?: FolderDetail; readReady: boolean; reload: () => Promise<FolderDetail>; contextKey: string;
  selectedFolderId?: string;
  selected?: { context: SelectedFolderContext; folder: Folder & { capabilities?: FolderDetail['capabilities'] }; readReady: boolean };
}) {
  const client = useQueryClient();
  const store = rootFolderOperations(client);
  const operation = useSyncExternalStore(store.subscribe, store.get);
  const renameStore = folderRenameOperations(client);
  const rename = useSyncExternalStore(renameStore.subscribe, renameStore.get);
  const moveStore = folderMoveOperations(client);
  const move = useSyncExternalStore(moveStore.subscribe, moveStore.get);
  const renameUnresolved = [rename, move].some(item => item?.status === 'pending' || item?.status === 'unknown');
  const otherUnresolvedNow = () => [renameStore.get(), moveStore.get()].some(item => item?.status === 'pending' || item?.status === 'unknown');
  const otherBlocked = move?.status === 'pending' || move?.status === 'unknown'
    ? '移動結果が未確定です。保持されている移動操作の結果を先に確認してください。'
    : '改名結果が未確定です。保持されている改名操作の結果を先に確認してください。';
  const [open, setOpen] = useState(false);
  const [name, setName] = useState('');
  const [reason, setReason] = useState('');
  const [reading, setReading] = useState(false);
  const [localError, setLocalError] = useState('');
  const [draftContext, setDraftContext] = useState<FolderCreateContext>(rootContext);
  const [reviewed, setReviewed] = useState<FolderDetail>();
  const [blocked, setBlocked] = useState(false);
  const opening = useRef(0);
  const refreshing = useRef(false);
  const blockedSelection = useRef<SelectedFolderContext | undefined>(undefined);
  const triggerContainer = useRef<HTMLSpanElement>(null);
  const returnFocus = useRef<HTMLButtonElement | null>(null);
  const liveContextKey = useRef(contextKey); liveContextKey.current = contextKey;
  useEffect(() => { opening.current += 1; refreshing.current = false; setReading(false); setOpen(false); setReviewed(undefined); setBlocked(false); }, [contextKey]);
  useEffect(() => () => { opening.current += 1; }, []);
  const pending = operation?.status === 'pending';
  const unknown = operation?.status === 'unknown';
  const selectedDetail = selected?.folder.capabilities
    ? { ...selected.folder, parentFolderId: selected.context.sourceParentId, capabilities: selected.folder.capabilities } : undefined;
  const rootAllowed = !renameUnresolved && readReady && canCreateRootFolder(root);
  const reselectRequired = Boolean(selected && selected.context === blockedSelection.current);
  const selectedAllowed = Boolean(!renameUnresolved && selected?.readReady && !reselectRequired && canCreateRootFolder(selectedDetail));
  const target = operation ? operation.context ?? rootContext : draftContext;
  const title = target.kind === 'root' ? rootTitle : selectedTitle;
  const targetDetail = reviewed ?? (target.kind === 'root' ? root : selected?.context.folderId === target.folderId ? selectedDetail : undefined);
  const allowed = !blocked && !renameUnresolved && (reviewed ? canCreateRootFolder(reviewed) : target.kind === 'root' ? rootAllowed : selectedAllowed);
  const locked = Boolean(operation) || reading;
  const validation = rootFolderValidation(name, reason);
  const problem = problemFromUnknown(operation?.error);

  function show(context: FolderCreateContext, button: HTMLButtonElement | null) {
    if (!store.get() && otherUnresolvedNow()) return;
    opening.current += 1; refreshing.current = false; setReading(false);
    returnFocus.current = button; setDraftContext(context); setReviewed(undefined); setBlocked(false);
    setName(''); setReason(''); setLocalError(''); setOpen(true);
  }
  function close() {
    if (store.get()?.status === 'pending') return;
    opening.current += 1; refreshing.current = false; setReading(false); setOpen(false);
    requestAnimationFrame(() => returnFocus.current?.focus());
  }
  async function invalidate(parentId: string) {
    await Promise.all([
      client.invalidateQueries({ queryKey: ['folder-tree', 'root'] }),
      client.invalidateQueries({ queryKey: ['folder-tree', parentId] }),
    ]);
  }
  async function readTarget(context: FolderCreateContext) {
    return context.kind === 'root' ? reload() : readSelectedFolder(context, documentApi.listFolderChildren);
  }
  function blockTarget(context: FolderCreateContext) {
    setBlocked(true);
    if (context.kind === 'selected') {
      // Keep an invalid selection stopped across dialog/list navigation; clicking its tree row creates new provenance.
      blockedSelection.current = selected?.context.folderId === context.folderId ? selected.context : context;
    }
  }
  function readError(context: FolderCreateContext) {
    return context.kind === 'root' ? '最新のSystem Rootを取得できません。状態を読み直してから作成してください。'
      : '選択したフォルダーの最新の状態を取得できません。元の親の取得済み範囲を確認し、ツリーで選び直してください。';
  }
  function unavailable(context: FolderCreateContext, current: FolderDetail) {
    return context.kind === 'root' ? '現在のSystem Rootでは作成できません。権限と最新の状態を確認してください。'
      : current.capabilities?.createFolder?.status === 'disabled' ? availabilityReason(current.capabilities.createFolder.reason)
        : '現在のフォルダーでは作成できません。ツリーで選び直してください。';
  }
  async function submit(event?: React.FormEvent) {
    event?.preventDefault();
    const saved = store.get();
    if (saved) {
      if (saved.status === 'unknown') await sendRootFolderOperation({ store, request: saved.request, send: documentApi.createFolder, invalidate: () => invalidate(saved.request.parentFolderId) });
      return;
    }
    if (!allowed || validation || refreshing.current || otherUnresolvedNow()) return;
    refreshing.current = true; setReading(true); setLocalError('');
    const generation = opening.current; const key = contextKey; const context = draftContext;
    try {
      const current = await readTarget(context);
      if (generation !== opening.current || key !== liveContextKey.current || store.get()) return;
      if (otherUnresolvedNow()) { setLocalError(otherBlocked); return; }
      if (!canCreateRootFolder(current)) { setLocalError(unavailable(context, current)); blockTarget(context); return; }
      await sendRootFolderOperation({ store, context: { ...context, name: current.name }, request: {
        operationId: createOperationId(), folderId: createOperationId(), parentFolderId: current.folderId,
        expectedParentRevision: current.revision, name: folderName(name), reason: metadataReason(reason),
      }, send: documentApi.createFolder, invalidate: () => invalidate(current.folderId) });
    } catch {
      if (generation === opening.current && key === liveContextKey.current) { setLocalError(readError(context)); blockTarget(context); }
    } finally {
      if (generation === opening.current) { refreshing.current = false; setReading(false); }
    }
  }
  async function review() {
    const saved = store.get();
    if (saved?.status !== 'rejected' || refreshing.current) return;
    const generation = opening.current; const key = contextKey;
    const savedContext = saved.context ?? rootContext;
    // A definitive rejection may use a real reselection of the same parent, never another target or a URL alone.
    const context = savedContext.kind === 'selected' && selected?.context.folderId === saved.request.parentFolderId
      ? selected.context : savedContext;
    refreshing.current = true; setReading(true); setLocalError('');
    try {
      const current = await readTarget(context);
      if (generation !== opening.current || key !== liveContextKey.current || store.get() !== saved) return;
      if (otherUnresolvedNow()) { setLocalError(otherBlocked); return; }
      if (!canCreateRootFolder(current)) { setLocalError(unavailable(context, current)); return; }
      if (store.clearSettled(saved)) { blockedSelection.current = undefined; setDraftContext({ ...context, name: current.name }); setReviewed(current); setBlocked(false); setName(saved.request.name); setReason(saved.request.reason); }
    } catch {
      if (generation === opening.current && key === liveContextKey.current) { setLocalError(readError(context)); blockTarget(context); }
    } finally {
      if (generation === opening.current) { refreshing.current = false; setReading(false); }
    }
  }
  return <>
    <span ref={triggerContainer}>
      {operation ? <button type="button" onClick={event => show(rootContext, event.currentTarget)}>{rootTitle}</button>
        : <CapabilityButton label={rootTitle} availability={root?.capabilities?.createFolder} disabled={!rootAllowed}
          onClick={() => show(rootContext, triggerContainer.current?.querySelector('button') ?? null)} />}
      {(selectedFolderId || operation?.context?.kind === 'selected') && (operation
        ? <button type="button" onClick={event => show(selected?.context ?? rootContext, event.currentTarget)}>{selectedTitle}</button>
        : selected ? <CapabilityButton label={selectedTitle} availability={selectedDetail?.capabilities.createFolder} disabled={!selectedAllowed}
          onClick={() => show(selected.context, triggerContainer.current?.querySelectorAll('button')[1] ?? null)} />
          : <><button type="button" disabled>{selectedTitle}</button><small> 作成先をツリーで選び直してください。</small></>)}
      {!operation && renameUnresolved && <small> {otherBlocked}</small>}
      {!operation && reselectRequired && <small> 作成先をツリーで選び直してください。</small>}
    </span>
    <Modal isOpen={open} onOpenChange={value => { if (!value) close(); }} isDismissable={!pending} isKeyboardDismissDisabled={pending} className={styles.modal}>
      <Dialog aria-labelledby="root-folder-create-title" className={styles.dialog}>
        <Heading slot="title" id="root-folder-create-title">{title}</Heading>
        {target.kind === 'root' ? <p>登録先：System Root直下（選択中のフォルダーには作成しません）</p>
          : <p>登録先：{target.name}（{target.folderId}）直下</p>}
        {operation?.context?.kind === 'selected' && <p>登録先は送信時の表示名です。現在名と異なる場合があります。フォルダーIDで確認してください。</p>}
        <form onSubmit={submit} aria-busy={pending || reading}>
          <label className={workspace.formField}>フォルダー名<textarea aria-label="フォルダー名" rows={1} autoFocus value={operation?.request.name ?? name} disabled={locked || !allowed} onChange={event => { if (!locked) setName(event.target.value); }} /></label>
          <label className={workspace.formField}>作成理由<textarea aria-label="作成理由" rows={2} value={operation?.request.reason ?? reason} disabled={locked || !allowed} onChange={event => { if (!locked) setReason(event.target.value); }} /></label>
          {!operation && (name || reason) && validation && <p role="alert">{validation}</p>}
          {!operation && !allowed && !localError && <p role="alert">{targetDetail?.capabilities?.createFolder?.status === 'disabled' ? availabilityReason(targetDetail.capabilities.createFolder.reason) : target.kind === 'root' ? 'System Rootの現在の状態を確認してください。' : '作成先をツリーで選び直してください。'}</p>}
          {localError && <p role="alert">{localError}</p>}
          {pending && <p role="status">作成結果を確認しています…</p>}
          {reading && !operation && <p role="status">{target.kind === 'root' ? 'System Root' : target.name}の最新の状態を確認しています…</p>}
          {unknown && <section role="alert"><h3>作成結果を確認できません</h3><p>同じ操作ID・同じ内容で再試行して結果を確認してください。新しい作成は開始できません。</p><p>要求はこのアプリのメモリー内だけに保持されます。ページ再読み込みやタブ終了で失われるため、このページからの離脱を避け、解決しない場合は操作IDを添えて管理者へ結果を確認してください。</p></section>}
          {operation && <p>操作ID：{operation.request.operationId}<br />作成先フォルダーID：{operation.request.parentFolderId}<br />新しいフォルダーID：{operation.request.folderId}</p>}
          {operation?.status === 'rejected' && <p role="alert">作成は拒否されました。最新の状態を取得し、名前と状態を見直してください。</p>}
          {problem && <p role={unknown ? undefined : 'alert'}>{unknown ? `再試行の結果：${problem.code}。初回の作成結果は未確定です。同じ要求を保持して管理者へ確認してください。` : problem.code === 'REVISION_CONFLICT' ? '同名のフォルダーがあるか、フォルダーの状態が更新されています。名前と最新の状態を見直してください。' : mapApiProblem(problem).message}</p>}
          {operation?.status === 'succeeded' && <p role="status">フォルダーを作成しました。</p>}
          <div className={styles.actions}>
            <button type="button" disabled={pending} onClick={close}>{operation && !pending ? '閉じる' : 'キャンセル'}</button>
            {unknown ? <button type="button" onClick={() => void submit()}>同じ内容で再試行</button>
              : operation?.status === 'rejected' ? <button type="button" disabled={reading} onClick={() => void review()}>最新の状態を取得して見直す</button>
                : operation?.status === 'succeeded' ? <button type="button" onClick={() => { if (store.clearSettled(operation)) close(); }}>確認して閉じる</button>
                  : <button className={workspace.primaryButton} type="submit" disabled={pending || reading || !allowed || Boolean(validation)}>作成する</button>}
          </div>
        </form>
      </Dialog>
    </Modal>
  </>;
}
