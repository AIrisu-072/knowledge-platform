import { useEffect, useRef, useState, useSyncExternalStore } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { Dialog, Heading, Modal } from 'react-aria-components';
import type { PolicyGrantInput } from '../../application/document-workspace';
import { documentApi, type Folder, type FolderDetail } from '../../application/document-workspace';
import { readSelectedFolder, rootFolderOperations, type SelectedFolderContext } from '../../application/document-root-folder';
import { folderRenameOperations } from '../../application/document-folder-rename';
import { folderMoveOperations, refreshFolderMoveReads } from '../../application/document-folder-move';
import { documentMoveOperations } from '../../application/document-move';
import { canManageFolderPolicy, folderAccessPolicyOperations, policyActions, policyChanged, policyGrantInput, policyReasonValidation, policyRevisionError, policySnapshotEqual, policySubjectKey, policySubjectLabel, sendFolderAccessPolicyOperation, validateFolderPolicy, type FolderPolicyRequest, type FolderPolicySnapshot } from '../../application/document-folder-access-policy';
import { metadataReason } from '../../application/document-metadata';
import { createOperationId } from '../../application/operation-id';
import { mapApiProblem, problemFromUnknown } from '../../application/problem-mapping';
import { CapabilityButton } from '../shared/CapabilityButton';
import styles from './DocumentMetadataEditor.module.css';
import workspace from '../../routes/DocumentWorkspace.module.css';

const title = '選択したフォルダーのアクセス設定';
const reselect = 'アクセス設定の対象はフォルダーツリーからもう一度選択してください。';
const readError = '選択したフォルダーの最新のアクセス設定を取得できません。元の親の取得済み範囲と権限を確認し、ツリーで選び直してください。';
const otherBlocked = 'フォルダー作成・改名・移動または文書移動の結果が未確定です。保持されている操作の結果を先に確認してください。';
const actionLabels = { read: '閲覧', readHistory: '履歴閲覧', write: '編集', publish: '公開', administer: 'アクセス管理' };
const actionList = (actions: PolicyGrantInput['actions']) => actions.map(action => actionLabels[action]).join(', ');
const modeLabel = (mode: 'inherit' | 'explicit') => mode === 'inherit' ? '上位の設定を継承' : '個別設定';
export function FolderAccessPolicy({ root, selected, contextKey }: {
  root?: FolderDetail; contextKey: string;
  selected?: { context: SelectedFolderContext; folder: Folder & { capabilities?: FolderDetail['capabilities'] }; readReady: boolean };
}) {
  const client = useQueryClient(); const store = folderAccessPolicyOperations(client);
  const operation = useSyncExternalStore(store.subscribe, store.get);
  const createStore = rootFolderOperations(client); const renameStore = folderRenameOperations(client); const moveStore = folderMoveOperations(client); const documentStore = documentMoveOperations(client);
  const create = useSyncExternalStore(createStore.subscribe, createStore.get); const rename = useSyncExternalStore(renameStore.subscribe, renameStore.get);
  const move = useSyncExternalStore(moveStore.subscribe, moveStore.get); const documentMove = useSyncExternalStore(documentStore.subscribe, documentStore.get);
  const [open, setOpen] = useState(false); const [context, setContext] = useState<SelectedFolderContext>(); const [baseline, setBaseline] = useState<FolderPolicySnapshot>();
  const [mode, setMode] = useState<'inherit' | 'explicit'>('inherit'); const [grants, setGrants] = useState<PolicyGrantInput[]>([]); const [reason, setReason] = useState('');
  const [confirmed, setConfirmed] = useState(false); const impactConfirmed = useRef(false);
  const [reading, setReading] = useState(false); const [stale, setStale] = useState(false); const [blocked, setBlocked] = useState(false); const [localError, setLocalError] = useState('');
  const generation = useRef(0); const refreshing = useRef(false); const liveKey = useRef(contextKey); liveKey.current = contextKey;
  const trigger = useRef<HTMLSpanElement>(null); const returnFocus = useRef<HTMLButtonElement | null>(null);
  const blockedSelection = useRef<SelectedFolderContext | undefined>(undefined);
  const queryKey = ['folder-access-policy', context?.folderId] as const;
  const state = useSyncExternalStore(listener => client.getQueryCache().subscribe(listener), () => context ? client.getQueryState<FolderPolicySnapshot>(queryKey) : undefined);
  const currentRead = Boolean(baseline && state?.status === 'success' && state.fetchStatus === 'idle' && !state.isInvalidated && state.data === baseline);
  function clearConfirmation() { impactConfirmed.current = false; setConfirmed(false); }
  useEffect(() => { generation.current += 1; refreshing.current = false; setReading(false); setOpen(false); }, [contextKey]);
  useEffect(() => () => { generation.current += 1; }, []);
  useEffect(() => { if (!currentRead && !refreshing.current) clearConfirmation(); }, [currentRead]);
  const unresolved = [create, rename, move, documentMove].some(item => item?.status === 'pending' || item?.status === 'unknown');
  const otherUnresolvedNow = () => [createStore.get(), renameStore.get(), moveStore.get(), documentStore.get()].some(item => item?.status === 'pending' || item?.status === 'unknown');
  const pending = operation?.status === 'pending'; const unknown = operation?.status === 'unknown';
  const selectedDetail = selected?.folder.capabilities ? { ...selected.folder, parentFolderId: selected.context.sourceParentId, capabilities: selected.folder.capabilities } : undefined;
  const entryAllowed = Boolean(root && selected?.readReady && selected.context !== blockedSelection.current && canManageFolderPolicy(selectedDetail, root.folderId) && !unresolved);
  const allowed = currentRead && !blocked && !stale && !unresolved;
  const locked = Boolean(operation) || reading || !allowed;
  const target = operation?.context ?? context; const shownMode = operation?.request.mode ?? mode;
  const activeGrants = grants.filter(grant => grant.actions.length > 0);
  const validation = policyReasonValidation(reason) ?? (mode === 'explicit' && activeGrants.length === 0 ? '個別設定には少なくとも1主体の権限が必要です。' : null);
  const draftRequest: FolderPolicyRequest = { operationId: '', expectedPolicyRevision: baseline?.policy.policyRevision ?? 0, reason: metadataReason(reason), ...(mode === 'explicit' ? { mode, grants: activeGrants } : { mode }) };
  const numericError = baseline && policyRevisionError(baseline.policy.policyRevision, policyChanged(baseline.policy, draftRequest));
  const problem = problemFromUnknown(operation?.error);
  const isCurrent = (opening: number, key: string) => opening === generation.current && key === liveKey.current;
  function snapshotReadable(snapshot: FolderPolicySnapshot, folderId: string) {
    const current = client.getQueryState<FolderPolicySnapshot>(['folder-access-policy', folderId]);
    return current?.status === 'success' && current.fetchStatus === 'idle' && !current.isInvalidated && current.data === snapshot;
  }
  async function read(readContext: SelectedFolderContext) {
    const key = ['folder-access-policy', readContext.folderId] as const;
    await client.cancelQueries({ queryKey: key, exact: true }, { revert: false });
    // Never reuse a cached policy as authority; the source row and its own capability are also read afresh.
    await client.fetchQuery({ queryKey: key, staleTime: 0, retry: false, queryFn: async ({ signal }) => {
      const folder = await readSelectedFolder(readContext, documentApi.listFolderChildren);
      if (signal.aborted || !root || !canManageFolderPolicy(folder, root.folderId)) throw new Error(readError);
      const policy = await documentApi.getFolderAccessPolicy(folder.folderId);
      if (signal.aborted || !validateFolderPolicy(policy, folder.folderId)) throw new Error(readError);
      return { folder, policy };
    } });
    const result = client.getQueryData<FolderPolicySnapshot>(key); if (!result || !snapshotReadable(result, readContext.folderId)) throw new Error(readError); return result;
  }
  function adopt(current: FolderPolicySnapshot, readContext: SelectedFolderContext, previous?: readonly PolicyGrantInput[]) {
    setBaseline(current); setContext({ ...readContext, name: current.folder.name }); setStale(false); setBlocked(false); blockedSelection.current = undefined;
    setGrants(current.policy.effectiveGrants.map(grant => policyGrantInput(previous?.find(old => policySubjectKey(old) === policySubjectKey(grant)) ?? grant))); clearConfirmation();
  }
  async function show(button: HTMLButtonElement | null) {
    const saved = store.get(); if (!saved && (!entryAllowed || otherUnresolvedNow())) return;
    const opening = ++generation.current; const key = contextKey; returnFocus.current = button; setOpen(true); setLocalError(''); setStale(false); setBlocked(false); setBaseline(undefined); clearConfirmation();
    if (saved) { refreshing.current = false; setReading(false); return; }
    const readContext = selected!.context; setContext(readContext); setGrants([]); setReason(''); refreshing.current = true; setReading(true);
    try { const current = await read(readContext); if (!isCurrent(opening, key) || store.get()) return; if (otherUnresolvedNow()) { setLocalError(otherBlocked); return; } adopt(current, readContext); setMode(current.policy.bindingMode); }
    catch { if (isCurrent(opening, key)) { setLocalError(readError); setBlocked(true); blockedSelection.current = readContext; } }
    finally { if (isCurrent(opening, key)) { refreshing.current = false; setReading(false); } }
  }
  function close() {
    if (store.get()?.status === 'pending') return;
    generation.current += 1; refreshing.current = false; setReading(false); setOpen(false); setBaseline(undefined); clearConfirmation();
    requestAnimationFrame(() => { if (returnFocus.current?.isConnected && !returnFocus.current.disabled) returnFocus.current.focus(); else trigger.current?.closest('section')?.querySelector<HTMLButtonElement>('button[aria-current="location"]')?.focus(); });
  }
  async function submit(event?: React.FormEvent) {
    event?.preventDefault(); const saved = store.get();
    if (saved) { if (saved.status === 'unknown') await sendFolderAccessPolicyOperation({ store, ...saved, send: documentApi.setFolderAccessPolicy, invalidate: () => refreshFolderMoveReads(client) }); return; }
    if (!context || !baseline || !allowed || validation || numericError || !impactConfirmed.current || refreshing.current || otherUnresolvedNow()) return;
    const opening = generation.current; const key = contextKey; const readContext = context; refreshing.current = true; setReading(true); setLocalError('');
    try {
      const current = await read(readContext); if (!isCurrent(opening, key) || store.get()) return;
      if (otherUnresolvedNow()) { clearConfirmation(); setLocalError(otherBlocked); return; }
      if (!snapshotReadable(current, readContext.folderId) || !policySnapshotEqual(baseline, current)) { setStale(true); clearConfirmation(); return; }
      await sendFolderAccessPolicyOperation({ store, targetFolderId: current.folder.folderId, context: { ...readContext, name: current.folder.name }, baseline: current.policy,
        request: { ...draftRequest, operationId: createOperationId(), expectedPolicyRevision: current.policy.policyRevision }, send: documentApi.setFolderAccessPolicy, invalidate: () => refreshFolderMoveReads(client) });
    } catch { if (isCurrent(opening, key)) { setLocalError(readError); setBlocked(true); blockedSelection.current = selected?.context; clearConfirmation(); } }
    finally { if (isCurrent(opening, key)) { refreshing.current = false; setReading(false); } }
  }
  async function review() {
    const saved = store.get(); if (refreshing.current || saved && saved.status !== 'rejected' || otherUnresolvedNow()) return;
    const readContext = saved ? selected?.context.folderId === saved.targetFolderId ? selected.context : saved.context : context;
    if (!readContext) return;
    const opening = generation.current; const key = contextKey; refreshing.current = true; setReading(true); setLocalError('');
    try {
      const current = await read(readContext); if (!isCurrent(opening, key) || store.get() !== saved) return;
      if (otherUnresolvedNow()) { setLocalError(otherBlocked); return; }
      if (saved && !store.clearSettled(saved)) return;
      const previous = saved ? saved.request.mode === 'explicit' ? saved.request.grants : undefined : grants;
      adopt(current, readContext, previous); if (saved) { setMode(saved.request.mode); setReason(saved.request.reason); }
    } catch { if (isCurrent(opening, key)) { setLocalError(readError); setBlocked(true); clearConfirmation(); } }
    finally { if (isCurrent(opening, key)) { refreshing.current = false; setReading(false); } }
  }
  const visibleGrants = operation ? operation.request.mode === 'explicit' ? operation.request.grants : [] : currentRead && !blocked && !stale ? mode === 'inherit' ? baseline!.policy.effectiveGrants : grants : [];
  const displayName = (grant: PolicyGrantInput) => {
    const presentation = baseline?.policy.effectiveGrants.find(original => policySubjectKey(original) === policySubjectKey(grant))?.presentation;
    return !operation && presentation?.resolution === 'resolved' && presentation.displayName?.trim() ? presentation.displayName : grant.subjectId;
  };
  return <>
    <span ref={trigger}>
      {operation ? <button type="button" onClick={event => void show(event.currentTarget)}>{title}</button> : !selectedDetail || selectedDetail.folderId === root?.folderId ? null : <CapabilityButton label={title} availability={selectedDetail?.capabilities.manageAccess} disabled={!entryAllowed} onClick={() => void show(trigger.current?.querySelector('button') ?? null)} />}
      {!operation && !selected && <small> {reselect}</small>}{!operation && unresolved && <small> {otherBlocked}</small>}
    </span>
    <Modal isOpen={open} onOpenChange={value => { if (!value) close(); }} isDismissable={!pending} isKeyboardDismissDisabled={pending} className={styles.modal}>
      <Dialog aria-labelledby="folder-policy-title" className={styles.dialog}>
        <Heading slot="title" id="folder-policy-title">{title}</Heading>
        {target && <p>対象フォルダー：{target.name}（ID：{target.folderId}）</p>}
        {!operation && currentRead && !blocked && !stale && baseline && <p>現在の設定：{modeLabel(baseline.policy.bindingMode)}、policy revision {baseline.policy.policyRevision}、実効設定元：{baseline.policy.effectiveSource.kind} / {baseline.policy.effectiveSource.id}</p>}
        {operation && <p>以下は送信時の固定内容です。現在の設定は再読取で確認してください。</p>}
        <form onSubmit={submit} aria-busy={pending || reading}>
          <label className={workspace.formField}>設定方式<select aria-label="設定方式" value={shownMode} disabled={locked} onChange={event => { setMode(event.target.value as 'inherit' | 'explicit'); clearConfirmation(); }}><option value="inherit">上位の設定を継承</option><option value="explicit">個別設定</option></select></label>
          {visibleGrants.length > 0 && <p>{operation ? '送信時の権限' : mode === 'inherit' ? '変更前に確認した実効権限' : '保存する権限（編集内容）'}</p>}
          {visibleGrants.map(grant => <fieldset key={policySubjectKey(grant)} aria-label={policySubjectLabel(grant)} disabled={locked || shownMode === 'inherit'}>
            <legend>{policySubjectLabel(grant)}</legend><p>{displayName(grant)}</p>
            {policyActions.map(action => <label key={action}><input type="checkbox" aria-label={actionLabels[action]} checked={grant.actions.includes(action)} onChange={event => { setGrants(rows => rows.map(row => policySubjectKey(row) !== policySubjectKey(grant) ? row : { ...row, actions: event.target.checked ? policyActions.filter(item => item === action || row.actions.includes(item)) : row.actions.filter(item => item !== action) })); clearConfirmation(); }} />{actionLabels[action]}</label>)}
            <button type="button" onClick={() => { setGrants(rows => rows.map(row => policySubjectKey(row) === policySubjectKey(grant) ? { ...row, actions: [] } : row)); clearConfirmation(); }}>この主体を削除</button>
            {grant.actions.length === 0 && <p>この主体は保存時に削除されます。</p>}
          </fieldset>)}
          <label className={workspace.formField}>アクセス設定の変更理由<textarea aria-label="アクセス設定の変更理由" rows={2} value={operation?.request.reason ?? reason} disabled={locked} onChange={event => { setReason(event.target.value); clearConfirmation(); }} /></label>
          <section aria-labelledby="folder-policy-confirmation"><h3 id="folder-policy-confirmation">変更内容の確認</h3>
            <p>保存する設定方式：{modeLabel(shownMode)}</p>
            {shownMode === 'explicit' && visibleGrants.map(grant => <p key={policySubjectKey(grant)}>{policySubjectLabel(grant)}：{!operation && baseline ? `${actionList(baseline.policy.effectiveGrants.find(row => policySubjectKey(row) === policySubjectKey(grant))?.actions ?? [])} → ` : ''}{grant.actions.length ? actionList(grant.actions) : '削除'}</p>)}
            {shownMode === 'inherit' ? <p>このフォルダーの個別設定を解除し、その時点の上位の設定を適用します。以前の個別設定へ戻す操作ではありません。</p> : <p>確認した実効権限を基に、このフォルダーの個別設定全体を保存します。確認から保存までに親の設定や所属が変わる可能性があり、保存時点の上位設定と同一である保証はありません。</p>}
            <p>このフォルダーと設定を継承する子孫に影響する可能性があります。独自の設定を持つ子孫への同じ効果や、正確な対象数は確認できません。</p>
            <p>自分の閲覧・管理権限を失う可能性があります。</p>
            <label><input type="checkbox" aria-label="変更内容と影響範囲を確認しました" checked={confirmed} disabled={Boolean(operation) || reading || !allowed} onChange={event => { impactConfirmed.current = event.target.checked; setConfirmed(event.target.checked); }} />変更内容と影響範囲を確認しました</label>
          </section>
          {!operation && validation && (reason || mode === 'explicit' && !activeGrants.length) && <p role="alert">{validation}</p>}
          {!operation && numericError && <p role="alert">{numericError}</p>}
          {localError && <p role="alert">{localError}</p>}
          {!operation && unresolved && <p role="alert">{otherBlocked}</p>}
          {!operation && (stale || baseline && !currentRead && !reading && !blocked) && <p role="alert">フォルダーまたは実効アクセス設定が変わったか、読取が失効しました。最新の状態を取得し、変更内容をもう一度確認してください。</p>}
          {reading && <p role="status">フォルダーとアクセス設定の最新の状態を確認しています…</p>}
          {pending && <p role="status">アクセス設定の保存結果を確認しています…</p>}
          {unknown && <section role="alert"><h3>アクセス設定の保存結果を確認できません</h3><p>同じ対象・操作ID・内容だけを再送できます。自分の管理権限を失った後の403や409は、元操作の失敗を証明しません。現在の設定の再読取結果が一致しても、この操作の成功記録にはなりません。</p><p>要求はアプリのメモリー内だけに保持されます。ページ再読み込みやタブ終了を避け、解決しない場合は操作IDを添えて管理者へ確認してください。</p></section>}
          {operation && <p>操作ID：{operation.request.operationId}</p>}
          {operation?.status === 'rejected' && <p role="alert">アクセス設定の保存は拒否されました。最新の状態を取得して見直してください。</p>}
          {problem && <p role={unknown ? undefined : 'alert'}>{unknown ? `再試行の結果：${problem.code}。初回結果は未確定のままです。` : mapApiProblem(problem).message}</p>}
          {operation?.status === 'succeeded' && <><p role="status">{operation.result?.changed ? 'アクセス設定を保存しました。' : 'アクセス設定の変更はありませんでした。'}</p><p>この結果は操作時点の記録です。現在の閲覧・管理権限を保証しません。ツリーで対象を選び直し、現在の設定を確認してください。</p>{operation.refresh === 'failed' && <p role="alert">現在の表示を更新できませんでした。保存結果は確定しています。</p>}</>}
          <div className={styles.actions}>
            <button type="button" disabled={pending} onClick={close}>{operation ? '閉じる' : 'キャンセル'}</button>
            {unknown ? <button type="button" onClick={() => void submit()}>同じ内容で再試行</button> : operation?.status === 'succeeded' ? <button type="button" onClick={() => { if (store.clearSettled(operation)) close(); }}>確認して閉じる</button>
              : operation?.status === 'rejected' ? <><button type="button" disabled={reading || unresolved} onClick={() => void review()}>最新の状態を取得して見直す</button><button type="button" disabled={reading} onClick={() => { if (store.clearSettled(operation)) close(); }}>拒否された操作を確認して終了</button></>
                : <>{(stale || baseline && !currentRead && !reading) && <button type="button" disabled={reading || unresolved} onClick={() => void review()}>最新の状態を取得して見直す</button>}<button className={workspace.primaryButton} type="submit" disabled={reading || !allowed || Boolean(validation) || Boolean(numericError) || !confirmed}>アクセス設定を保存</button></>}
          </div>
        </form>
      </Dialog>
    </Modal>
  </>;
}
