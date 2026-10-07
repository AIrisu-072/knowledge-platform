import { documentAccessPolicyOperations } from '../../application/document-access-policy';
import { folderAccessPolicyOperations } from '../../application/document-folder-access-policy';
import { useEffect, useRef, useState, useSyncExternalStore } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { Dialog, Heading, Modal } from 'react-aria-components';
import { documentApi, type Folder, type FolderDetail } from '../../application/document-workspace';
import { readSelectedFolder, rootFolderOperations, type SelectedFolderContext } from '../../application/document-root-folder';
import { folderRenameOperations } from '../../application/document-folder-rename';
import { canMoveFolder, folderMoveOperations, moveReasonValidation, moveRevisionError, refreshFolderMoveReads, sendFolderMoveOperation, type FolderMoveDestination } from '../../application/document-folder-move';
import { documentMoveOperations } from '../../application/document-move';
import { metadataReason } from '../../application/document-metadata';
import { createOperationId } from '../../application/operation-id';
import { mapApiProblem, problemFromUnknown } from '../../application/problem-mapping';
import { CapabilityButton, availabilityReason } from '../shared/CapabilityButton';
import { FolderNode } from './FolderNode';
import styles from './DocumentMetadataEditor.module.css';
import workspace from '../../routes/DocumentWorkspace.module.css';

const title = '選択したフォルダーを移動';
const reselect = '移動対象はフォルダーツリーからもう一度選択してください。';
const sourceError = '対象の最新の状態を取得できません。元の親の取得済み範囲を確認し、ツリーで選び直してください。';
const destinationError = '移動先の最新の状態を取得できません。移動先ツリーで選び直してください。';
const otherBlocked = 'アクセス設定・文書移動・作成または改名結果が未確定です。保持されている操作の結果を先に確認してください。';
export function FolderMove({ root, selected, contextKey }: {
  root?: FolderDetail; contextKey: string;
  selected?: { context: SelectedFolderContext; folder: Folder & { capabilities?: FolderDetail['capabilities'] }; readReady: boolean };
}) {
  const client = useQueryClient();
  const documentPolicyStore = documentAccessPolicyOperations(client);
  const documentPolicyOperation = useSyncExternalStore(documentPolicyStore.subscribe, documentPolicyStore.get);
  const policyStore = folderAccessPolicyOperations(client);
  const policyOperation = useSyncExternalStore(policyStore.subscribe, policyStore.get);
  const documentMoveStore = documentMoveOperations(client);
  const documentMove = useSyncExternalStore(documentMoveStore.subscribe, documentMoveStore.get);
  const store = folderMoveOperations(client); const createStore = rootFolderOperations(client); const renameStore = folderRenameOperations(client);
  const operation = useSyncExternalStore(store.subscribe, store.get);
  const create = useSyncExternalStore(createStore.subscribe, createStore.get); const rename = useSyncExternalStore(renameStore.subscribe, renameStore.get);
  const [open, setOpen] = useState(false); const [reason, setReason] = useState(''); const [confirmed, setConfirmed] = useState(false);
  const [context, setContext] = useState<SelectedFolderContext>(); const [baseline, setBaseline] = useState<FolderDetail>();
  const [destination, setDestination] = useState<FolderMoveDestination>(); const [destinationBaseline, setDestinationBaseline] = useState<FolderDetail>();
  const [staleSource, setStaleSource] = useState<FolderDetail>(); const [staleDestination, setStaleDestination] = useState<FolderDetail>();
  const [reading, setReading] = useState(false); const [destinationReading, setDestinationReading] = useState(false);
  const [localError, setLocalError] = useState(''); const [blocked, setBlocked] = useState(false);
  const impactConfirmed = useRef(false); const destinationBusy = useRef(false);
  const clearConfirmation = () => { impactConfirmed.current = false; setConfirmed(false); };
  const blockedSelection = useRef<SelectedFolderContext | undefined>(undefined); const generation = useRef(0); const destinationGeneration = useRef(0); const refreshing = useRef(false);
  const liveKey = useRef(contextKey); liveKey.current = contextKey;
  const trigger = useRef<HTMLSpanElement>(null); const returnFocus = useRef<HTMLButtonElement | null>(null);
  useEffect(() => { generation.current += 1; destinationGeneration.current += 1; destinationBusy.current = false; refreshing.current = false; setReading(false); setDestinationReading(false); setOpen(false); }, [contextKey]);
  useEffect(() => () => { generation.current += 1; destinationGeneration.current += 1; }, []);
  const otherUnresolved = [documentPolicyOperation, create, rename, documentMove, policyOperation].some(item => item?.status === 'pending' || item?.status === 'unknown');
  const otherUnresolvedNow = () => [documentPolicyStore.get(), createStore.get(), renameStore.get(), documentMoveStore.get(), policyStore.get()].some(item => item?.status === 'pending' || item?.status === 'unknown');
  const pending = operation?.status === 'pending'; const unknown = operation?.status === 'unknown';
  const selectedDetail = selected?.folder.capabilities ? { ...selected.folder, parentFolderId: selected.context.sourceParentId, capabilities: selected.folder.capabilities } : undefined;
  const reselectRequired = selected?.context === blockedSelection.current;
  const entryAllowed = Boolean(selected?.readReady && !reselectRequired && canMoveFolder(selectedDetail, root?.folderId) && !otherUnresolved);
  const allowed = !blocked && !staleSource && !staleDestination && !otherUnresolved && canMoveFolder(baseline, root?.folderId) && Boolean(destinationBaseline);
  const locked = Boolean(operation) || reading;
  const validation = moveReasonValidation(reason);
  const revisionError = baseline && destination && moveRevisionError(baseline.revision, baseline.parentFolderId !== destination.folderId);
  const target = operation?.context ?? context; const destinationContext = operation?.destination ?? destination;
  const problem = problemFromUnknown(operation?.error);
  const isCurrent = (opening: number, key: string) => opening === generation.current && key === liveKey.current;
  const availableTarget = (current: FolderDetail) => current.capabilities?.moveFolder?.status === 'disabled' ? availabilityReason(current.capabilities.moveFolder.reason) : reselect;
  function blockTarget(readContext: SelectedFolderContext) { setBlocked(true); clearConfirmation(); blockedSelection.current = selected?.context.folderId === readContext.folderId ? selected.context : readContext; }
  async function readDestination(value: FolderMoveDestination): Promise<FolderDetail> {
    const current = value.kind === 'root' ? await documentApi.getRootFolder() : await readSelectedFolder(value, documentApi.listFolderChildren);
    if (current.folderId !== value.folderId || !current.name || !Number.isSafeInteger(current.revision) || current.revision < 0
      || value.kind === 'root' && current.parentFolderId !== null) throw new Error(destinationError);
    return current;
  }
  async function show(button: HTMLButtonElement | null) {
    const saved = store.get(); if (!saved && (!entryAllowed || otherUnresolvedNow())) return;
    const opening = ++generation.current; const key = contextKey; destinationGeneration.current += 1; destinationBusy.current = false;
    returnFocus.current = button; setOpen(true); setLocalError(''); setStaleSource(undefined); setStaleDestination(undefined); setBlocked(false); clearConfirmation();
    if (saved) { refreshing.current = false; setReading(false); setDestinationReading(false); return; }
    const readContext = selected!.context;
    // Reselecting this target after a move/read failure retains the reason, with a fresh source
    // and an explicitly chosen destination/impact confirmation. A different target starts anew.
    if (context?.folderId !== readContext.folderId) setReason('');
    setContext(readContext); setBaseline(undefined); setDestination(undefined); setDestinationBaseline(undefined);
    refreshing.current = true; setReading(true);
    try {
      const current = await readSelectedFolder(readContext, documentApi.listFolderChildren);
      if (!isCurrent(opening, key) || store.get()) return;
      if (otherUnresolvedNow()) { setLocalError(otherBlocked); return; }
      if (!canMoveFolder(current, root?.folderId)) { setLocalError(availableTarget(current)); blockTarget(readContext); return; }
      setBaseline(current);
      if (current.name !== readContext.name || current.revision !== selected!.folder.revision) setStaleSource(current);
      else setContext({ ...readContext, name: current.name });
    } catch { if (isCurrent(opening, key)) { setLocalError(sourceError); blockTarget(readContext); } }
    finally { if (isCurrent(opening, key)) { refreshing.current = false; setReading(false); } }
  }
  function close() {
    if (store.get()?.status === 'pending') return;
    generation.current += 1; destinationGeneration.current += 1; destinationBusy.current = false; refreshing.current = false; setReading(false); setDestinationReading(false); setOpen(false);
    requestAnimationFrame(() => {
      if (returnFocus.current?.isConnected && !returnFocus.current.disabled) { returnFocus.current.focus(); return; }
      const rail = trigger.current?.closest('section');
      const row = rail?.querySelector<HTMLButtonElement>('button[aria-current="location"]')
        ?? rail?.querySelector<HTMLButtonElement>('ul > li > div > button:nth-child(2)');
      row?.focus();
    });
  }
  async function chooseDestination(folder: Folder, rowContext?: SelectedFolderContext) {
    if (store.get() || refreshing.current) return;
    const value: FolderMoveDestination | undefined = folder.folderId === root?.folderId ? { kind: 'root', folderId: folder.folderId, name: folder.name } : rowContext;
    if (!value) return;
    const opening = generation.current; const key = contextKey; const selection = ++destinationGeneration.current; destinationBusy.current = true;
    setDestination(value); setDestinationBaseline(undefined); setStaleDestination(undefined); clearConfirmation(); setDestinationReading(true); setLocalError('');
    try {
      const current = await readDestination(value);
      if (!isCurrent(opening, key) || selection !== destinationGeneration.current || store.get()) return;
      if (otherUnresolvedNow()) { setLocalError(otherBlocked); return; }
      setDestinationBaseline(current);
      if (current.name !== folder.name || current.revision !== folder.revision) setStaleDestination(current);
      else setDestination({ ...value, name: current.name });
    } catch { if (isCurrent(opening, key) && selection === destinationGeneration.current) { setLocalError(destinationError); clearConfirmation(); } }
    finally { if (isCurrent(opening, key) && selection === destinationGeneration.current) { destinationBusy.current = false; setDestinationReading(false); } }
  }
  async function submit(event?: React.FormEvent) {
    event?.preventDefault(); const saved = store.get();
    if (saved) { if (saved.status === 'unknown') await sendFolderMoveOperation({ store, ...saved, send: documentApi.moveFolder, invalidate: () => refreshFolderMoveReads(client) }); return; }
    if (!context || !baseline || !destination || !destinationBaseline || !allowed || !impactConfirmed.current || validation || revisionError || refreshing.current || destinationBusy.current || otherUnresolvedNow()) return;
    const opening = generation.current; const key = contextKey; const selection = destinationGeneration.current; const readContext = context; const readDestinationContext = destination;
    refreshing.current = true; setReading(true); setLocalError('');
    try {
      const current = await readSelectedFolder(readContext, documentApi.listFolderChildren);
      if (!isCurrent(opening, key) || selection !== destinationGeneration.current || store.get()) return;
      if (otherUnresolvedNow()) { setLocalError(otherBlocked); return; }
      if (!canMoveFolder(current, root?.folderId)) { setLocalError(availableTarget(current)); blockTarget(readContext); return; }
      if (current.name !== baseline.name || current.revision !== baseline.revision) { setStaleSource(current); clearConfirmation(); return; }
      let currentDestination: FolderDetail;
      try { currentDestination = await readDestination(readDestinationContext); }
      catch { if (isCurrent(opening, key)) { setDestinationBaseline(undefined); setLocalError(destinationError); clearConfirmation(); } return; }
      if (!isCurrent(opening, key) || selection !== destinationGeneration.current || store.get()) return;
      if (otherUnresolvedNow()) { setLocalError(otherBlocked); return; }
      if (currentDestination.name !== destinationBaseline.name || currentDestination.revision !== destinationBaseline.revision) { setStaleDestination(currentDestination); clearConfirmation(); return; }
      await sendFolderMoveOperation({ store, targetFolderId: current.folderId, context: { ...readContext, name: current.name }, currentName: current.name, destination: { ...readDestinationContext, name: currentDestination.name },
        request: { operationId: createOperationId(), fromParentId: readContext.sourceParentId, toParentId: currentDestination.folderId, expectedFolderRevision: current.revision, reason: metadataReason(reason) },
        send: documentApi.moveFolder, invalidate: () => refreshFolderMoveReads(client) });
    } catch { if (isCurrent(opening, key)) { setLocalError(sourceError); blockTarget(readContext); } }
    finally { if (isCurrent(opening, key)) { refreshing.current = false; setReading(false); } }
  }
  async function review() {
    const saved = store.get(); if (refreshing.current || destinationBusy.current || saved && saved.status !== 'rejected') return;
    const readContext = saved ? selected?.context.folderId === saved.targetFolderId ? selected.context : saved.context : context;
    const readDestinationContext = saved?.destination ?? destination; if (!readContext || !readDestinationContext) return;
    const opening = generation.current; const key = contextKey; const selection = destinationGeneration.current;
    refreshing.current = true; setReading(true); setLocalError('');
    try {
      const current = await readSelectedFolder(readContext, documentApi.listFolderChildren);
      if (!isCurrent(opening, key) || store.get() !== saved) return;
      if (otherUnresolvedNow()) { setLocalError(otherBlocked); return; }
      if (!canMoveFolder(current, root?.folderId)) { setLocalError(availableTarget(current)); blockTarget(readContext); return; }
      const currentDestination = await readDestination(readDestinationContext);
      if (!isCurrent(opening, key) || selection !== destinationGeneration.current || store.get() !== saved) return;
      if (otherUnresolvedNow()) { setLocalError(otherBlocked); return; }
      if (saved && !store.clearSettled(saved)) return;
      if (saved) setReason(saved.request.reason);
      setContext({ ...readContext, name: current.name }); setBaseline(current); setDestination({ ...readDestinationContext, name: currentDestination.name }); setDestinationBaseline(currentDestination);
      setStaleSource(undefined); setStaleDestination(undefined); setBlocked(false); blockedSelection.current = undefined; clearConfirmation();
    } catch { if (isCurrent(opening, key)) { setLocalError('最新の対象と移動先を確認できません。ツリーで選び直してください。'); clearConfirmation(); } }
    finally { if (isCurrent(opening, key)) { refreshing.current = false; setReading(false); } }
  }
  return <>
    <span ref={trigger}>
      {operation ? <button type="button" onClick={event => void show(event.currentTarget)}>{title}</button>
        : !selectedDetail?.capabilities.moveFolder ? <button type="button" disabled>{title}</button>
          : <CapabilityButton label={title} availability={selectedDetail.capabilities.moveFolder} disabled={!entryAllowed} onClick={() => void show(trigger.current?.querySelector('button') ?? null)} />}
      {!operation && (!selected || reselectRequired) && <small> {reselect}</small>}
      {!operation && otherUnresolved && <small> {otherBlocked}</small>}
    </span>
    <Modal isOpen={open} onOpenChange={value => { if (!value) close(); }} isDismissable={!pending} isKeyboardDismissDisabled={pending} className={styles.modal}>
      <Dialog aria-labelledby="folder-move-title" className={styles.dialog}>
        <Heading slot="title" id="folder-move-title">{title}</Heading>
        {target && <><p>対象フォルダーID：{target.folderId}</p><p>{operation ? '送信時の親ID' : '現在の親ID'}：{operation?.request.fromParentId ?? target.sourceParentId}</p></>}
        {(operation || baseline) && <p>{operation ? `送信時の対象名：${operation.currentName}` : `現在の対象名：${baseline!.name}（revision ${baseline!.revision}）`}</p>}
        <p>継承アクセス設定の変化により、配下や自分の閲覧・編集権限が変わる可能性があります。対象と移動先を確認してください。最終の認可・循環・公開予約・revisionは送信時にサーバーで確認します。</p>
        <form onSubmit={submit} aria-busy={pending || reading || destinationReading}>
          {!operation && root && <fieldset disabled={locked || blocked || !baseline} aria-label="移動先フォルダー"><legend>移動先を選択</legend><ul className={workspace.folderTree}>
            <FolderNode folder={root} isRoot selectedFolderId={destination?.folderId} rootSelected={destination?.kind === 'root'} onSelect={(_id, folder, rowContext) => { if (folder) void chooseDestination(folder, rowContext); }} />
          </ul></fieldset>}
          {(destinationContext && destinationBaseline) || operation ? <><p>移動先フォルダーID：{destinationContext?.folderId}</p><p>移動先名：{destinationContext?.name}</p></> : null}
          <label className={workspace.formField}>移動理由<textarea aria-label="移動理由" rows={2} value={operation?.request.reason ?? reason} disabled={locked || !baseline && !operation} onChange={event => { if (!locked) setReason(event.target.value); }} /></label>
          {!operation && <label className={workspace.checkLine}><input type="checkbox" checked={confirmed} disabled={reading || destinationReading || !allowed} onChange={event => { if (!refreshing.current && !destinationBusy.current && allowed) { impactConfirmed.current = event.target.checked; setConfirmed(event.target.checked); } }} />継承アクセス設定への影響を確認しました</label>}
          {!operation && reason && validation && <p role="alert">{validation}</p>}
          {!operation && revisionError && <p role="alert">{revisionError}</p>}
          {!operation && otherUnresolved && <p role="alert">{otherBlocked}</p>}
          {localError && <p role="alert">{localError}</p>}
          {staleSource && <section role="alert"><p>対象の名前またはrevisionが変わりました。理由を保持しています。最新の状態を取得して見直してください。</p><p>最新の対象名：{staleSource.name}（revision {staleSource.revision}）</p></section>}
          {staleDestination && <section role="alert"><p>移動先の状態が変わりました。理由を保持しています。最新の状態を取得して見直してください。</p><p>最新の移動先名：{staleDestination.name}</p></section>}
          {(reading || destinationReading) && <p role="status">フォルダーの最新の状態を確認しています…</p>}
          {pending && <p role="status">移動結果を確認しています…</p>}
          {unknown && <section role="alert"><h3>移動結果を確認できません</h3><p>同じ対象ID・操作ID・移動元・移動先・理由で再試行して結果を確認してください。新しい移動・作成・改名は開始できません。</p><p>要求はこのアプリのメモリー内だけに保持されます。ページ再読み込みやタブ終了で失われるため、離脱を避け、解決しない場合は操作IDを添えて管理者へ確認してください。</p></section>}
          {operation && <p>操作ID：{operation.request.operationId}</p>}
          {operation?.status === 'rejected' && <p role="alert">移動は拒否されました。最新の対象と移動先を取得して見直してください。</p>}
          {problem && <p role={unknown ? undefined : 'alert'}>{unknown ? `再試行の結果：${problem.code}。初回の移動結果は未確定です。同じ要求を保持して管理者へ確認してください。` : problem.code === 'REVISION_CONFLICT' ? 'フォルダーの状態または移動先との整合性を確認できません。最新の状態を取得して見直してください。' : mapApiProblem(problem).message}</p>}
          {operation?.status === 'succeeded' && <><p role="status">{operation.result?.changed ? 'フォルダーを移動しました。' : '親の変更はありませんでした。'}</p>
            {operation.refresh === 'pending' ? <p>表示を更新中です。</p> : operation.refresh === 'failed' ? <p role="alert">表示を更新できませんでした。移動結果は確定しています。読取を再試行してください。</p> : null}
            <p>対象名・移動元・移動先は送信時点の表示です。この結果は操作時点の記録です。ツリーで選び直して現在の状態を確認してください。</p></>}
          <div className={styles.actions}>
            <button type="button" disabled={pending} onClick={close}>{operation ? '閉じる' : 'キャンセル'}</button>
            {unknown ? <button type="button" onClick={() => void submit()}>同じ内容で再試行</button>
              : operation?.status === 'rejected' ? <><button type="button" disabled={reading || destinationReading || otherUnresolved} onClick={() => void review()}>最新の状態を取得して見直す</button><button type="button" disabled={reading} onClick={() => { if (store.clearSettled(operation)) close(); }}>拒否された操作を確認して終了</button></>
                : operation?.status === 'succeeded' ? <button type="button" onClick={() => { if (store.clearSettled(operation)) close(); }}>確認して閉じる</button>
                  : staleSource || staleDestination ? <button type="button" disabled={reading || destinationReading || !destination || otherUnresolved} onClick={() => void review()}>最新の状態を取得して見直す</button>
                    : <button type="submit" className={workspace.primaryButton} disabled={reading || destinationReading || !allowed || !confirmed || Boolean(validation) || Boolean(revisionError)}>移動する</button>}
          </div>
        </form>
      </Dialog>
    </Modal>
  </>;
}
