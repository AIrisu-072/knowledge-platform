import { folderAccessPolicyOperations } from '../../application/document-folder-access-policy';
import { useEffect, useRef, useState, useSyncExternalStore } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Dialog, Heading, Modal } from 'react-aria-components';
import { documentApi, type DocumentDetail, type Folder, type FolderDetail } from '../../application/document-workspace';
import { readSelectedFolder, rootFolderOperations, type SelectedFolderContext } from '../../application/document-root-folder';
import { folderRenameOperations } from '../../application/document-folder-rename';
import { folderMoveOperations, refreshFolderMoveReads, type FolderMoveDestination } from '../../application/document-folder-move';
import { canMoveDocument, documentMoveOperations, documentMoveReasonValidation, documentMoveRevisionError, sendDocumentMoveOperation, type DocumentMoveContext } from '../../application/document-move';
import { metadataReason } from '../../application/document-metadata';
import { createOperationId } from '../../application/operation-id';
import { mapApiProblem, problemFromUnknown } from '../../application/problem-mapping';
import { CapabilityButton, availabilityReason } from '../shared/CapabilityButton';
import { FolderNode } from './FolderNode';
import styles from './DocumentMetadataEditor.module.css';
import workspace from '../../routes/DocumentWorkspace.module.css';

const title = '文書を移動';
const sourceError = '文書の最新の状態と元所属を確認できません。最新の状態を取得して見直してください。';
const destinationError = '移動先の最新の状態を取得できません。移動先ツリーで選び直してください。';
const otherBlocked = 'アクセス設定・フォルダー作成・改名・移動の結果が未確定です。保持されている操作の結果を先に確認してください。';
function changedSource(current: DocumentDetail, baseline: DocumentDetail): boolean {
  return current.documentId !== baseline.documentId || current.title !== baseline.title || current.folderId !== baseline.folderId
    || current.folderName !== baseline.folderName || current.revision !== baseline.revision;
}
function displayContext(current: DocumentDetail & { folderId: string; folderName: string }, view: DocumentMoveContext['view']): DocumentMoveContext {
  return { documentId: current.documentId, title: current.title, folderId: current.folderId, folderName: current.folderName, view };
}
export function DocumentMove({ document, purpose = 'published', currentRead = false, contextKey }: {
  document?: DocumentDetail; purpose?: 'published' | 'authoring'; currentRead?: boolean; contextKey: string;
}) {
  const client = useQueryClient();
  const policyStore = folderAccessPolicyOperations(client);
  const policyOperation = useSyncExternalStore(policyStore.subscribe, policyStore.get); const store = documentMoveOperations(client);
  const operation = useSyncExternalStore(store.subscribe, store.get);
  const createStore = rootFolderOperations(client); const renameStore = folderRenameOperations(client); const folderStore = folderMoveOperations(client);
  const create = useSyncExternalStore(createStore.subscribe, createStore.get); const rename = useSyncExternalStore(renameStore.subscribe, renameStore.get); const folderMove = useSyncExternalStore(folderStore.subscribe, folderStore.get);
  const [open, setOpen] = useState(false); const [reason, setReason] = useState(''); const [confirmed, setConfirmed] = useState(false);
  const [baseline, setBaseline] = useState<DocumentDetail>(); const [context, setContext] = useState<DocumentMoveContext>();
  const [destination, setDestination] = useState<FolderMoveDestination>(); const [destinationBaseline, setDestinationBaseline] = useState<FolderDetail>();
  const [staleSource, setStaleSource] = useState<DocumentDetail>(); const [staleDestination, setStaleDestination] = useState<FolderDetail>();
  const [reading, setReading] = useState(false); const [destinationReading, setDestinationReading] = useState(false); const [blocked, setBlocked] = useState(false); const [localError, setLocalError] = useState('');
  const generation = useRef(0); const destinationGeneration = useRef(0); const refreshing = useRef(false); const destinationBusy = useRef(false); const impactConfirmed = useRef(false);
  const liveKey = useRef(contextKey); liveKey.current = contextKey;
  const trigger = useRef<HTMLSpanElement>(null); const returnFocus = useRef<HTMLButtonElement | null>(null);
  const rootQuery = useQuery({ queryKey: ['folder-tree', 'root'], queryFn: documentApi.getRootFolder, enabled: open && !operation });
  const root = rootQuery.error ? undefined : rootQuery.data;
  const clearConfirmation = () => { impactConfirmed.current = false; setConfirmed(false); };
  useEffect(() => { generation.current += 1; destinationGeneration.current += 1; refreshing.current = false; destinationBusy.current = false; setReading(false); setDestinationReading(false); setOpen(false); }, [contextKey]);
  useEffect(() => () => { generation.current += 1; destinationGeneration.current += 1; }, []);
  const otherUnresolved = [create, rename, folderMove, policyOperation].some(item => item?.status === 'pending' || item?.status === 'unknown');
  const otherUnresolvedNow = () => [createStore.get(), renameStore.get(), folderStore.get(), policyStore.get()].some(item => item?.status === 'pending' || item?.status === 'unknown');
  const pending = operation?.status === 'pending'; const unknown = operation?.status === 'unknown';
  const entryAllowed = currentRead && canMoveDocument(document) && !otherUnresolved;
  const allowed = !blocked && !staleSource && !staleDestination && !otherUnresolved && canMoveDocument(baseline) && Boolean(destinationBaseline);
  const locked = Boolean(operation) || reading;
  const validation = documentMoveReasonValidation(reason);
  const revisionError = baseline && destination && documentMoveRevisionError(baseline.revision, baseline.folderId !== destination.folderId);
  const target = operation?.context ?? context; const destinationContext = operation?.destination ?? destination;
  const problem = problemFromUnknown(operation?.error);
  const isCurrent = (opening: number, key: string) => opening === generation.current && key === liveKey.current;
  const unavailable = (current?: DocumentDetail) => current?.capabilities?.moveDocument?.status === 'disabled' ? availabilityReason(current.capabilities.moveDocument.reason) : sourceError;
  async function readSource(id: string, view: DocumentMoveContext['view']) {
    const current = await documentApi.getDocument(id, view);
    if (!canMoveDocument(current) || current.documentId !== id) throw new Error(unavailable(current));
    return current;
  }
  async function readDestination(value: FolderMoveDestination): Promise<FolderDetail> {
    const current = value.kind === 'root' ? await documentApi.getRootFolder() : await readSelectedFolder(value, documentApi.listFolderChildren);
    if (typeof current !== 'object' || current === null || typeof current.folderId !== 'string' || !current.folderId
      || typeof current.name !== 'string' || !current.name || current.folderId !== value.folderId
      || !Number.isSafeInteger(current.revision) || current.revision < 0
      || value.kind === 'root' && current.parentFolderId !== null) throw new Error(destinationError);
    return current;
  }
  async function show(button: HTMLButtonElement | null) {
    const saved = store.get(); if (!saved && (!entryAllowed || otherUnresolvedNow())) return;
    const opening = ++generation.current; const key = contextKey; destinationGeneration.current += 1; destinationBusy.current = false;
    returnFocus.current = button; setOpen(true); setLocalError(''); setStaleSource(undefined); setStaleDestination(undefined); setBlocked(false); clearConfirmation();
    if (saved) { refreshing.current = false; setReading(false); setDestinationReading(false); return; }
    const shown = document!;
    if (context?.documentId !== shown.documentId) setReason('');
    setContext(canMoveDocument(shown) ? displayContext(shown, purpose) : undefined); setBaseline(undefined); setDestination(undefined); setDestinationBaseline(undefined);
    refreshing.current = true; setReading(true);
    try {
      const current = await readSource(shown.documentId, purpose);
      if (!isCurrent(opening, key) || store.get()) return;
      if (otherUnresolvedNow()) { setLocalError(otherBlocked); return; }
      setBaseline(current);
      if (changedSource(current, shown)) setStaleSource(current); else setContext(displayContext(current, purpose));
    } catch (error) { if (isCurrent(opening, key)) { setLocalError(error instanceof Error ? error.message : sourceError); setBlocked(true); clearConfirmation(); } }
    finally { if (isCurrent(opening, key)) { refreshing.current = false; setReading(false); } }
  }
  function close() {
    if (store.get()?.status === 'pending') return;
    const closing = ++generation.current; const key = contextKey; const preferred = returnFocus.current; const container = trigger.current;
    destinationGeneration.current += 1; refreshing.current = false; destinationBusy.current = false; setReading(false); setDestinationReading(false); setOpen(false);
    requestAnimationFrame(() => {
      if (!isCurrent(closing, key) || !container?.isConnected || container.ownerDocument.activeElement !== container.ownerDocument.body) return;
      const target = preferred?.isConnected && !preferred.disabled ? preferred : container.querySelector<HTMLButtonElement>('button:not(:disabled)');
      target?.focus();
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
      if (current.name !== folder.name || current.revision !== folder.revision) setStaleDestination(current); else setDestination({ ...value, name: current.name });
    } catch { if (isCurrent(opening, key) && selection === destinationGeneration.current) { setLocalError(destinationError); clearConfirmation(); } }
    finally { if (isCurrent(opening, key) && selection === destinationGeneration.current) { destinationBusy.current = false; setDestinationReading(false); } }
  }
  async function submit(event?: React.FormEvent) {
    event?.preventDefault(); const saved = store.get();
    if (saved) { if (saved.status === 'unknown') await sendDocumentMoveOperation({ store, ...saved, send: documentApi.moveDocument, invalidate: () => refreshFolderMoveReads(client) }); return; }
    if (!context || !baseline || !destination || !destinationBaseline || !allowed || !impactConfirmed.current || validation || revisionError || refreshing.current || destinationBusy.current || otherUnresolvedNow()) return;
    const opening = generation.current; const key = contextKey; const selection = destinationGeneration.current; const sourceContext = context; const destinationToRead = destination;
    refreshing.current = true; setReading(true); setLocalError('');
    try {
      const current = await readSource(sourceContext.documentId, sourceContext.view);
      if (!isCurrent(opening, key) || selection !== destinationGeneration.current || store.get()) return;
      if (otherUnresolvedNow()) { setLocalError(otherBlocked); return; }
      if (changedSource(current, baseline)) { setStaleSource(current); clearConfirmation(); return; }
      let currentDestination: FolderDetail;
      try { currentDestination = await readDestination(destinationToRead); }
      catch { if (isCurrent(opening, key)) { setDestinationBaseline(undefined); setLocalError(destinationError); clearConfirmation(); } return; }
      if (!isCurrent(opening, key) || selection !== destinationGeneration.current || store.get()) return;
      if (otherUnresolvedNow()) { setLocalError(otherBlocked); return; }
      if (currentDestination.name !== destinationBaseline.name || currentDestination.revision !== destinationBaseline.revision) { setStaleDestination(currentDestination); clearConfirmation(); return; }
      await sendDocumentMoveOperation({ store, targetDocumentId: current.documentId, context: displayContext(current, sourceContext.view), destination: { ...destinationToRead, name: currentDestination.name },
        request: { operationId: createOperationId(), fromFolderId: current.folderId, toFolderId: currentDestination.folderId, expectedDocumentRevision: current.revision, reason: metadataReason(reason) },
        send: documentApi.moveDocument, invalidate: () => refreshFolderMoveReads(client) });
    } catch (error) { if (isCurrent(opening, key)) { setLocalError(error instanceof Error ? error.message : sourceError); setBlocked(true); clearConfirmation(); } }
    finally { if (isCurrent(opening, key)) { refreshing.current = false; setReading(false); } }
  }
  async function review() {
    const saved = store.get(); if (refreshing.current || destinationBusy.current || saved && saved.status !== 'rejected') return;
    const sourceContext = saved?.context ?? context; const destinationToRead = saved?.destination ?? destination; if (!sourceContext) return;
    const opening = generation.current; const key = contextKey; const selection = destinationGeneration.current;
    refreshing.current = true; setReading(true); setLocalError('');
    try {
      const current = await readSource(sourceContext.documentId, sourceContext.view);
      if (!isCurrent(opening, key) || store.get() !== saved) return;
      if (otherUnresolvedNow()) { setLocalError(otherBlocked); return; }
      const currentDestination = destinationToRead ? await readDestination(destinationToRead) : undefined;
      if (!isCurrent(opening, key) || selection !== destinationGeneration.current || store.get() !== saved) return;
      if (otherUnresolvedNow()) { setLocalError(otherBlocked); return; }
      if (saved && !store.clearSettled(saved)) return;
      if (saved) setReason(saved.request.reason);
      setContext(displayContext(current, sourceContext.view)); setBaseline(current);
      setDestination(destinationToRead && currentDestination ? { ...destinationToRead, name: currentDestination.name } : undefined); setDestinationBaseline(currentDestination);
      setStaleSource(undefined); setStaleDestination(undefined); setBlocked(false); clearConfirmation();
    } catch { if (isCurrent(opening, key)) { setLocalError('最新の文書と移動先を確認できません。状態を読み直してから見直してください。'); clearConfirmation(); } }
    finally { if (isCurrent(opening, key)) { refreshing.current = false; setReading(false); } }
  }
  return <>
    <span ref={trigger}>
      {operation ? <button type="button" onClick={event => void show(event.currentTarget)}>文書移動の保持結果を確認</button>
        : document && <CapabilityButton label={title} availability={document.capabilities.moveDocument} disabled={!entryAllowed} onClick={() => void show(trigger.current?.querySelector('button') ?? null)} />}
      {!operation && document && (!document.folderId || !document.folderName) && <small> 元所属を確認できない文書はこの画面から移動できません。</small>}
      {!operation && document && otherUnresolved && <small> {otherBlocked}</small>}
    </span>
    <Modal isOpen={open} onOpenChange={value => { if (!value) close(); }} isDismissable={!pending} isKeyboardDismissDisabled={pending} className={styles.modal}>
      <Dialog aria-labelledby="document-move-title" className={styles.dialog}>
        <Heading slot="title" id="document-move-title">{title}</Heading>
        {target && <><p>対象文書ID：{target.documentId}</p><p>元フォルダーID：{target.folderId}</p><p>{operation ? '送信時の元所属名' : '現在の元所属名'}：{target.folderName}</p></>}
        {(operation || baseline) && <p>{operation ? `送信時の対象名：${operation.context.title}` : `現在の対象名：${baseline!.title}（revision ${baseline!.revision}）`}</p>}
        <p>明示アクセス設定は保持。継承中は移動先の設定が適用され、自分を含む閲覧・編集権限が変わり得るため、対象と移動先を確認してください。最終の認可・公開予約・revisionは送信時にサーバーで確認します。</p>
        <form onSubmit={submit} aria-busy={pending || reading || destinationReading}>
          {!operation && root && <fieldset disabled={locked || blocked || !baseline} aria-label="移動先フォルダー"><legend>移動先を選択</legend><ul className={workspace.folderTree}>
            <FolderNode folder={root} isRoot selectedFolderId={destination?.folderId} rootSelected={destination?.kind === 'root'} onSelect={(_id, folder, rowContext) => { if (folder) void chooseDestination(folder, rowContext); }} />
          </ul></fieldset>}
          {!operation && rootQuery.error && <p role="alert">移動先のSystem Rootを取得できません。最新の状態を読み直してください。</p>}
          {(destinationContext && destinationBaseline) || operation ? <><p>移動先フォルダーID：{destinationContext?.folderId}</p><p>移動先名：{destinationContext?.name}</p></> : null}
          <label className={workspace.formField}>移動理由<textarea aria-label="移動理由" rows={2} value={operation?.request.reason ?? reason} disabled={locked || !baseline && !operation} onChange={event => { if (!locked) setReason(event.target.value); }} /></label>
          {!operation && <label className={workspace.checkLine}><input type="checkbox" checked={confirmed} disabled={reading || destinationReading || !allowed} onChange={event => { if (!refreshing.current && !destinationBusy.current && allowed) { impactConfirmed.current = event.target.checked; setConfirmed(event.target.checked); } }} />アクセス設定への影響を確認しました</label>}
          {!operation && reason && validation && <p role="alert">{validation}</p>}
          {!operation && revisionError && <p role="alert">{revisionError}</p>}
          {!operation && otherUnresolved && <p role="alert">{otherBlocked}</p>}
          {localError && <p role="alert">{localError}</p>}
          {staleSource && <section role="alert"><p>対象の名前・所属またはrevisionが変わりました。理由を保持しています。最新の状態を取得して見直してください。</p><p>最新の対象名：{staleSource.title}（revision {staleSource.revision}）</p><p>最新の元所属名：{staleSource.folderName}</p><p>最新の元フォルダーID：{staleSource.folderId}</p></section>}
          {staleDestination && <section role="alert"><p>移動先の状態が変わりました。理由を保持しています。最新の状態を取得して見直してください。</p><p>最新の移動先名：{staleDestination.name}</p></section>}
          {(reading || destinationReading) && <p role="status">文書とフォルダーの最新の状態を確認しています…</p>}
          {pending && <p role="status">移動結果を確認しています…</p>}
          {unknown && <section role="alert"><h3>移動結果を確認できません</h3><p>同じ対象ID・操作ID・移動元・移動先・理由で再試行して結果を確認してください。新しい文書移動・フォルダー作成・改名・移動は開始できません。</p><p>要求はこのアプリのメモリー内だけに保持されます。ページ再読み込みやタブ終了で失われるため、離脱を避け、解決しない場合は操作IDを添えて管理者へ確認してください。</p></section>}
          {operation && <p>操作ID：{operation.request.operationId}</p>}
          {operation?.status === 'rejected' && <p role="alert">移動は拒否されました。最新の文書と移動先を取得して見直してください。</p>}
          {problem && <p role={unknown ? undefined : 'alert'}>{unknown ? `再試行の結果：${problem.code}。初回の移動結果は未確定です。同じ要求を保持して管理者へ確認してください。` : mapApiProblem(problem).message}</p>}
          {operation?.status === 'succeeded' && <><p role="status">{operation.result?.changed ? '文書を移動しました。' : '所属の変更はありませんでした。'}</p>
            {operation.refresh === 'pending' ? <p>表示を更新中です。</p> : operation.refresh === 'failed' ? <p role="alert">表示を更新できませんでした。移動結果は確定しています。読取を再試行してください。</p> : null}
            <p>対象名・移動元・移動先は送信時点の表示です。この結果は操作時点の記録です。通常の詳細を読み直して現在の状態を確認してください。</p></>}
          <div className={styles.actions}>
            <button type="button" disabled={pending} onClick={close}>{operation ? '閉じる' : 'キャンセル'}</button>
            {unknown ? <button type="button" onClick={() => void submit()}>同じ内容で再試行</button>
              : operation?.status === 'rejected' ? <><button type="button" disabled={reading || destinationReading || otherUnresolved} onClick={() => void review()}>最新の状態を取得して見直す</button><button type="button" disabled={reading} onClick={() => { if (store.clearSettled(operation)) close(); }}>拒否された操作を確認して終了</button></>
                : operation?.status === 'succeeded' ? <button type="button" onClick={() => { if (store.clearSettled(operation)) close(); }}>確認して閉じる</button>
                  : staleSource || staleDestination || blocked ? <button type="button" disabled={reading || destinationReading || otherUnresolved} onClick={() => void review()}>最新の状態を取得して見直す</button>
                    : <button type="submit" className={workspace.primaryButton} disabled={reading || destinationReading || !allowed || !confirmed || Boolean(validation) || Boolean(revisionError)}>移動する</button>}
          </div>
        </form>
      </Dialog>
    </Modal>
  </>;
}
