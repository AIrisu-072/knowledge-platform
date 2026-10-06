import { useEffect, useRef, useState, useSyncExternalStore } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { Dialog, Heading, Modal } from 'react-aria-components';
import { documentApi, type Folder, type FolderDetail } from '../../application/document-workspace';
import { folderName, readSelectedFolder, rootFolderOperations, type SelectedFolderContext } from '../../application/document-root-folder';
import { canRenameFolder, folderRenameOperations, renameFolderValidation, renameRevisionError, sendFolderRenameOperation, type FolderRenameOperation } from '../../application/document-folder-rename';
import { folderMoveOperations } from '../../application/document-folder-move';
import { documentMoveOperations } from '../../application/document-move';
import { metadataReason } from '../../application/document-metadata';
import { createOperationId } from '../../application/operation-id';
import { mapApiProblem, problemFromUnknown } from '../../application/problem-mapping';
import { CapabilityButton, availabilityReason } from '../shared/CapabilityButton';
import styles from './DocumentMetadataEditor.module.css';
import workspace from '../../routes/DocumentWorkspace.module.css';

const title = '選択したフォルダー名を変更';
const reselect = '改名対象はフォルダーツリーからもう一度選択してください。';
const readError = '選択したフォルダーの最新の状態を取得できません。元の親の取得済み範囲を確認し、ツリーで選び直してください。';
const createBlocked = '作成結果が未確定です。保持されている作成操作の結果を先に確認してください。';
export function FolderRename({ root, selected, contextKey }: {
  root?: FolderDetail; contextKey: string;
  selected?: { context: SelectedFolderContext; folder: Folder & { capabilities?: FolderDetail['capabilities'] }; readReady: boolean };
}) {
  const client = useQueryClient();
  const documentMoveStore = documentMoveOperations(client);
  const documentMove = useSyncExternalStore(documentMoveStore.subscribe, documentMoveStore.get);
  const store = folderRenameOperations(client);
  const createStore = rootFolderOperations(client);
  const operation = useSyncExternalStore(store.subscribe, store.get);
  const create = useSyncExternalStore(createStore.subscribe, createStore.get);
  const moveStore = folderMoveOperations(client);
  const move = useSyncExternalStore(moveStore.subscribe, moveStore.get);
  const [open, setOpen] = useState(false);
  const [name, setName] = useState('');
  const [reason, setReason] = useState('');
  const [context, setContext] = useState<SelectedFolderContext>();
  const [baseline, setBaseline] = useState<FolderDetail>();
  const [stale, setStale] = useState<FolderDetail>();
  const [reading, setReading] = useState(false);
  const [localError, setLocalError] = useState('');
  const [blocked, setBlocked] = useState(false);
  const blockedSelection = useRef<SelectedFolderContext | undefined>(undefined);
  const generation = useRef(0);
  const refreshing = useRef(false);
  const liveKey = useRef(contextKey); liveKey.current = contextKey;
  const returnFocus = useRef<HTMLButtonElement | null>(null);
  const trigger = useRef<HTMLSpanElement>(null);
  useEffect(() => { generation.current += 1; refreshing.current = false; setReading(false); setOpen(false); }, [contextKey]);
  useEffect(() => () => { generation.current += 1; }, []);
  const pending = operation?.status === 'pending';
  const unknown = operation?.status === 'unknown';
  const createUnresolved = [create, move, documentMove].some(item => item?.status === 'pending' || item?.status === 'unknown');
  const otherUnresolvedNow = () => [createStore.get(), moveStore.get(), documentMoveStore.get()].some(item => item?.status === 'pending' || item?.status === 'unknown');
  const otherBlocked = move?.status === 'pending' || move?.status === 'unknown' || documentMove?.status === 'pending' || documentMove?.status === 'unknown'
    ? '移動結果が未確定です。保持されている移動操作の結果を先に確認してください。' : createBlocked;
  const selectedDetail = selected?.folder.capabilities ? { ...selected.folder, parentFolderId: selected.context.sourceParentId, capabilities: selected.folder.capabilities } : undefined;
  const reselectRequired = Boolean(selected && selected.context === blockedSelection.current);
  const entryAllowed = Boolean(selected?.readReady && !reselectRequired && canRenameFolder(selectedDetail) && !createUnresolved);
  const allowed = !blocked && !stale && !createUnresolved && canRenameFolder(baseline);
  const locked = Boolean(operation) || reading;
  const validation = renameFolderValidation(name, reason);
  const revisionError = baseline && renameRevisionError(baseline, name);
  const target = operation?.context ?? context;
  const problem = problemFromUnknown(operation?.error);
  const isCurrent = (opening: number, key: string) => opening === generation.current && key === liveKey.current;
  function unavailable(current: FolderDetail) {
    return current.capabilities?.renameFolder?.status === 'disabled' ? availabilityReason(current.capabilities.renameFolder.reason) : reselect;
  }
  function blockTarget(readContext: SelectedFolderContext) {
    setBlocked(true); blockedSelection.current = selected?.context.folderId === readContext.folderId ? selected.context : readContext;
  }
  async function invalidate(saved: FolderRenameOperation) {
    await Promise.all([
      client.invalidateQueries({ queryKey: ['folder-tree', saved.context.sourceParentId] }, { throwOnError: true }),
      client.invalidateQueries({ queryKey: ['folder-tree', saved.targetFolderId] }, { throwOnError: true }),
      client.invalidateQueries({ queryKey: ['documents'] }, { throwOnError: true }),
      client.invalidateQueries({ queryKey: ['document'] }, { throwOnError: true }),
    ]);
  }
  async function show(button: HTMLButtonElement | null) {
    const saved = store.get();
    if (!saved && (!entryAllowed || otherUnresolvedNow())) return;
    const opening = ++generation.current; const key = contextKey;
    returnFocus.current = button; setOpen(true); setLocalError(''); setStale(undefined); setBaseline(undefined); setBlocked(false);
    if (saved) { refreshing.current = false; setReading(false); return; }
    const readContext = selected!.context; setContext(readContext); setName(''); setReason(''); refreshing.current = true; setReading(true);
    try {
      const current = await readSelectedFolder(readContext, documentApi.listFolderChildren);
      if (!isCurrent(opening, key) || store.get()) return;
      if (otherUnresolvedNow()) { setLocalError(otherBlocked); return; }
      if (!canRenameFolder(current)) { setLocalError(unavailable(current)); blockTarget(readContext); return; }
      setBaseline(current); setName(current.name); setContext({ ...readContext, name: current.name });
    } catch {
      if (isCurrent(opening, key)) { setLocalError(readError); blockTarget(readContext); }
    } finally { if (isCurrent(opening, key)) { refreshing.current = false; setReading(false); } }
  }
  function close() {
    if (store.get()?.status === 'pending') return;
    generation.current += 1; refreshing.current = false; setReading(false); setOpen(false);
    requestAnimationFrame(() => {
      if (returnFocus.current?.isConnected && !returnFocus.current.disabled) { returnFocus.current.focus(); return; }
      const rail = trigger.current?.closest('section');
      // The receipt may remove the target's selection copy and disable the entry. Return to its visible tree row.
      const row = rail?.querySelector<HTMLButtonElement>('button[aria-current="location"]')
        ?? rail?.querySelector<HTMLButtonElement>('ul > li > div > button:nth-child(2)');
      row?.focus();
    });
  }
  async function submit(event?: React.FormEvent) {
    event?.preventDefault(); const saved = store.get();
    if (saved) {
      if (saved.status === 'unknown') await sendFolderRenameOperation({ store, ...saved, send: documentApi.renameFolder, invalidate });
      return;
    }
    if (!context || !baseline || !allowed || validation || revisionError || refreshing.current || otherUnresolvedNow()) return;
    const opening = generation.current; const key = contextKey; const readContext = context;
    refreshing.current = true; setReading(true); setLocalError('');
    try {
      const current = await readSelectedFolder(readContext, documentApi.listFolderChildren);
      if (!isCurrent(opening, key) || store.get()) return;
      // The other operation can start while this fresh read is awaiting its response.
      if (otherUnresolvedNow()) { setLocalError(otherBlocked); return; }
      if (!canRenameFolder(current)) { setLocalError(unavailable(current)); blockTarget(readContext); return; }
      if (current.name !== baseline.name || current.revision !== baseline.revision) { setStale(current); return; }
      const numericError = renameRevisionError(current, name);
      if (numericError) { setLocalError(numericError); return; }
      await sendFolderRenameOperation({ store, targetFolderId: current.folderId, context: { ...readContext, name: current.name }, currentName: current.name,
        request: { operationId: createOperationId(), expectedFolderRevision: current.revision, name: folderName(name), reason: metadataReason(reason) },
        send: documentApi.renameFolder, invalidate });
    } catch {
      if (isCurrent(opening, key)) { setLocalError(readError); blockTarget(readContext); }
    } finally { if (isCurrent(opening, key)) { refreshing.current = false; setReading(false); } }
  }
  async function review() {
    const saved = store.get();
    if (refreshing.current || (saved && saved.status !== 'rejected') || (!saved && !stale)) return;
    const opening = generation.current; const key = contextKey;
    // Only a definitive rejection can adopt a real reselection of the same saved target.
    const readContext = saved ? selected?.context.folderId === saved.targetFolderId ? selected.context : saved.context : context!;
    refreshing.current = true; setReading(true); setLocalError('');
    try {
      const current = await readSelectedFolder(readContext, documentApi.listFolderChildren);
      if (!isCurrent(opening, key) || store.get() !== saved) return;
      if (otherUnresolvedNow()) { setLocalError(otherBlocked); return; }
      if (!canRenameFolder(current)) { setLocalError(unavailable(current)); return; }
      if (saved && !store.clearSettled(saved)) return;
      if (saved) { setName(saved.request.name); setReason(saved.request.reason); }
      blockedSelection.current = undefined; setContext({ ...readContext, name: current.name }); setBaseline(current); setStale(undefined); setBlocked(false);
    } catch { if (isCurrent(opening, key)) { setLocalError(readError); blockTarget(readContext); } }
    finally { if (isCurrent(opening, key)) { refreshing.current = false; setReading(false); } }
  }
  return <>
    <span ref={trigger}>
      {operation ? <button type="button" onClick={event => void show(event.currentTarget)}>{title}</button>
        : !selectedDetail?.capabilities.renameFolder && !root?.capabilities.renameFolder ? <button type="button" disabled>{title}</button>
        : <CapabilityButton label={title} availability={selectedDetail?.capabilities.renameFolder ?? (!selected ? root?.capabilities.renameFolder : undefined)} disabled={!entryAllowed}
          onClick={() => void show(trigger.current?.querySelector('button') ?? null)} />}
      {!operation && !selected && <small> {reselect}</small>}
      {!operation && reselectRequired && <small> {reselect}</small>}
      {!operation && createUnresolved && <small> {otherBlocked}</small>}
    </span>
    <Modal isOpen={open} onOpenChange={value => { if (!value) close(); }} isDismissable={!pending} isKeyboardDismissDisabled={pending} className={styles.modal}>
      <Dialog aria-labelledby="folder-rename-title" className={styles.dialog}>
        <Heading slot="title" id="folder-rename-title">{title}</Heading>
        {target && <p>対象フォルダーID：{target.folderId}</p>}
        {operation ? <p>送信時の現在名：{operation.currentName}（現在名はツリーで再確認してください）</p> : baseline && <p>現在名：{baseline.name}（revision {baseline.revision}）</p>}
        <form onSubmit={submit} aria-busy={pending || reading}>
          <label className={workspace.formField}>変更先のフォルダー名<textarea aria-label="変更先のフォルダー名" rows={1} autoFocus value={operation?.request.name ?? name} disabled={locked || blocked || !baseline && !operation} onChange={event => { if (!locked) setName(event.target.value); }} /></label>
          <label className={workspace.formField}>変更理由<textarea aria-label="変更理由" rows={2} value={operation?.request.reason ?? reason} disabled={locked || blocked || !baseline && !operation} onChange={event => { if (!locked) setReason(event.target.value); }} /></label>
          {!operation && (name || reason) && validation && <p role="alert">{validation}</p>}
          {!operation && revisionError && <p role="alert">{revisionError}</p>}
          {!operation && createUnresolved && <p role="alert">{otherBlocked}</p>}
          {localError && <p role="alert">{localError}</p>}
          {stale && <section role="alert"><p>編集中にフォルダーの名前またはrevisionが変わりました。希望する変更先名と理由を保持しています。最新の状態を取得して明示的に見直してください。</p><p>最新の現在名：{stale.name}（revision {stale.revision}）</p></section>}
          {reading && <p role="status">フォルダーの最新の状態を確認しています…</p>}
          {pending && <p role="status">改名結果を確認しています…</p>}
          {unknown && <section role="alert"><h3>改名結果を確認できません</h3><p>同じ対象ID・操作ID・内容で再試行して結果を確認してください。新しい改名や作成は開始できません。</p><p>要求はこのアプリのメモリー内だけに保持されます。ページ再読み込みやタブ終了で失われるため、離脱を避け、解決しない場合は操作IDを添えて管理者へ結果を確認してください。</p></section>}
          {operation && <p>操作ID：{operation.request.operationId}</p>}
          {operation?.status === 'rejected' && <p role="alert">改名は拒否されました。最新の状態を取得し、名前と状態を見直してください。</p>}
          {problem && <p role={unknown ? undefined : 'alert'}>{unknown ? `再試行の結果：${problem.code}。初回の改名結果は未確定です。同じ要求を保持して管理者へ確認してください。` : problem.code === 'REVISION_CONFLICT' ? '同名のフォルダーがあるか、フォルダーの状態が更新されています。名前と最新の状態を見直してください。' : mapApiProblem(problem).message}</p>}
          {operation?.status === 'succeeded' && <>
            <p role="status">{operation.result?.changed ? 'フォルダー名を変更しました。' : '名前の変更はありませんでした。'}</p>
            {operation.refresh === 'pending' ? <p>表示を更新中です。</p> : operation.refresh === 'failed' ? <p role="alert">表示を更新できませんでした。改名結果は確定しています。読取を再試行してください。</p> : null}
            <p>この結果は操作時点の記録です。閉じたフォルダーは展開し、続きに移った場合はさらに表示して、ツリーで対象を選び直して現在名を確認してください。</p>
          </>}
          <div className={styles.actions}>
            <button type="button" disabled={pending} onClick={close}>{operation ? '閉じる' : 'キャンセル'}</button>
            {unknown ? <button type="button" onClick={() => void submit()}>同じ内容で再試行</button>
              : operation?.status === 'rejected' ? <><button type="button" disabled={reading} onClick={() => void review()}>最新の状態を取得して見直す</button><button type="button" disabled={reading} onClick={() => { if (store.clearSettled(operation)) close(); }}>拒否された操作を確認して終了</button></>
                : operation?.status === 'succeeded' ? <button type="button" onClick={() => { if (store.clearSettled(operation)) close(); }}>確認して閉じる</button>
                  : stale ? <button type="button" disabled={reading} onClick={() => void review()}>最新の状態を取得して見直す</button>
                    : <button className={workspace.primaryButton} type="submit" disabled={reading || !allowed || Boolean(validation) || Boolean(revisionError)}>変更を保存する</button>}
          </div>
        </form>
      </Dialog>
    </Modal>
  </>;
}
