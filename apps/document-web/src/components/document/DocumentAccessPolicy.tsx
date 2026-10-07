import { formatDateTime } from '../../view-model/date-time';
import { useEffect, useRef, useState, useSyncExternalStore } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { Dialog, Heading, Modal } from 'react-aria-components';
import { documentApi, type AccessPolicyRead, type DocumentDetail, type PolicyGrantInput } from '../../application/document-workspace';
import { canManageDocumentPolicy, documentAccessPolicyOperations, documentPolicySnapshotEqual, sendDocumentAccessPolicyOperation, validateDocumentPolicy, type DocumentPolicySnapshot, type DocumentPolicyRequest } from '../../application/document-access-policy';
import { folderAccessPolicyOperations, policyActions, policyChanged, policyGrantInput, policyReasonValidation, policyRevisionError, policySubjectKey, policySubjectLabel } from '../../application/document-folder-access-policy';
import { rootFolderOperations } from '../../application/document-root-folder';
import { folderRenameOperations } from '../../application/document-folder-rename';
import { folderMoveOperations, refreshFolderMoveReads } from '../../application/document-folder-move';
import { documentMoveOperations } from '../../application/document-move';
import { createOperationId } from '../../application/operation-id';
import { metadataReason } from '../../application/document-metadata';
import { mapApiProblem, problemFromUnknown } from '../../application/problem-mapping';
import { LoadingState } from '../shared/ApiFeedback';
import styles from '../../routes/DocumentDetail.module.css';
import workspace from '../../routes/DocumentWorkspace.module.css';
import modal from './DocumentMetadataEditor.module.css';

const resultTitle = '文書アクセス設定の保存結果';
const readError = '最新の文書とアクセス設定を確認できません。最新の状態を取得して見直してください。';
const otherBlocked = 'アクセス設定・フォルダー操作・文書移動の結果が未確定です。保持されている操作の結果を先に確認してください。';
const actionLabels: Record<typeof policyActions[number], string> = { read: '閲覧', readHistory: '履歴閲覧', write: '編集', publish: '公開', administer: 'アクセス管理' };
function FixedResult() {
  const client = useQueryClient(); const store = documentAccessPolicyOperations(client); const operation = useSyncExternalStore(store.subscribe, store.get);
  if (!operation) return null;
  const problem = problemFromUnknown(operation.error); const unknown = operation.status === 'unknown';
  return <section aria-label={resultTitle}>
    <h3>{resultTitle}</h3>
    <p>対象文書：{operation.context.title}（ID：{operation.targetDocumentId}）</p>
    <p>以下は送信時の固定内容です。現在の設定は再読取で確認してください。</p>
    <p>操作ID：{operation.request.operationId}</p><p>送信時のpolicy revision：{operation.request.expectedPolicyRevision}</p>
    <fieldset disabled><legend>設定方法</legend><label><input type="radio" checked={operation.request.mode === 'inherit'} readOnly />親フォルダーのアクセス権を継承</label><label><input type="radio" checked={operation.request.mode === 'explicit'} readOnly />この文書だけに個別設定</label></fieldset>
    {operation.request.mode === 'explicit' && operation.request.grants.map(grant => <fieldset disabled key={policySubjectKey(grant)}><legend>{policySubjectLabel(grant)}</legend>{policyActions.map(action => <label key={action}><input type="checkbox" checked={grant.actions.includes(action)} readOnly />{actionLabels[action]}</label>)}</fieldset>)}
    <label>変更理由<textarea value={operation.request.reason} disabled /></label>
    {operation.status === 'pending' && <p role="status">アクセス設定の保存結果を確認しています…</p>}
    {unknown && <section role="alert"><h3>アクセス設定の保存結果を確認できません</h3><p>同じ対象・操作ID・内容だけを再送できます。自分の管理権限を失った後の403や404、409は、元操作の失敗を証明しません。現在の設定が一致しても、この操作の成功記録にはなりません。</p><p>要求はアプリのメモリー内だけに保持されます。ページ再読み込みやタブ終了を避け、解決しない場合は操作IDを添えて管理者へ確認してください。</p></section>}
    {operation.status === 'rejected' && <p role="alert">アクセス設定の保存は拒否されました。最新の状態を取得して見直してください。</p>}
    {problem && <p>{unknown ? `再試行の結果：${problem.code}。初回結果は未確定のままです。` : mapApiProblem(problem).message}</p>}
    {operation.status === 'succeeded' && <><p role="status">{operation.result?.changed ? 'アクセス設定を保存しました。' : 'アクセス設定の変更はありませんでした。'}</p><p>保存結果のpolicy revision：{operation.result?.resultingRevision}</p><p>記録日時：{operation.result && formatDateTime(operation.result.occurredAt, 'Asia/Tokyo')}</p><p>この結果は操作時点の記録です。現在の閲覧・管理権限を保証しません。</p>{operation.refresh === 'failed' && <p role="alert">現在の表示を更新できませんでした。保存結果は確定しています。</p>}</>}
    {unknown && <button type="button" onClick={() => void sendDocumentAccessPolicyOperation({ store, ...operation, send: documentApi.setDocumentAccessPolicy, invalidate: () => refreshFolderMoveReads(client) })}>同じ内容で再試行</button>}
    {(operation.status === 'succeeded' || operation.status === 'rejected') && <button type="button" onClick={() => { store.clearSettled(operation); }}>{operation.status === 'succeeded' ? '確認して閉じる' : '拒否された操作を確認して終了'}</button>}
  </section>;
}

// Present retained requests outside the permission-dependent Access tab. This
// entry is also available in Home if the operation revoked its own read access.
export function DocumentAccessPolicyRecovery() {
  const client = useQueryClient(); const store = documentAccessPolicyOperations(client); const operation = useSyncExternalStore(store.subscribe, store.get);
  const [open, setOpen] = useState(false);
  useEffect(() => { if (!operation) setOpen(false); }, [operation]);
  if (!operation) return null;
  return <><button type="button" onClick={() => setOpen(true)}>{resultTitle}</button>
    <Modal isOpen={open} onOpenChange={setOpen} className={modal.modal}><Dialog aria-label={resultTitle} className={modal.dialog}><Heading slot="title">文書アクセス設定</Heading><FixedResult /><button type="button" onClick={() => setOpen(false)}>閉じる</button></Dialog></Modal></>;
}

export function DocumentAccessPolicy({ document, view, policy, loading, error }: { document: DocumentDetail; view: 'published' | 'authoring'; policy?: AccessPolicyRead; loading: boolean; error: unknown }) {
  const client = useQueryClient(); const store = documentAccessPolicyOperations(client); const operation = useSyncExternalStore(store.subscribe, store.get);
  return <section><div className={styles.sectionHeading}><div><h2>アクセス設定</h2><p>既存の主体に付与された権限を表示します。</p></div></div>
    {operation?.targetDocumentId === document.documentId ? <FixedResult /> : operation ? <p role="alert">保持されている文書アクセス設定の結果を先に確認してください。</p>
      : <PolicyDraft key={`${document.documentId}:${view}`} document={document} view={view} policy={policy} loading={loading} error={error} />}
  </section>;
}

function PolicyDraft({ document, view, policy, loading, error }: { document: DocumentDetail; view: 'published' | 'authoring'; policy?: AccessPolicyRead; loading: boolean; error: unknown }) {
  const client = useQueryClient(); const store = documentAccessPolicyOperations(client);
  const folderPolicy = folderAccessPolicyOperations(client); const create = rootFolderOperations(client); const rename = folderRenameOperations(client); const folderMove = folderMoveOperations(client); const documentMove = documentMoveOperations(client);
  const operations = [useSyncExternalStore(folderPolicy.subscribe, folderPolicy.get), useSyncExternalStore(create.subscribe, create.get), useSyncExternalStore(rename.subscribe, rename.get), useSyncExternalStore(folderMove.subscribe, folderMove.get), useSyncExternalStore(documentMove.subscribe, documentMove.get)];
  const otherUnresolved = operations.some(item => item?.status === 'pending' || item?.status === 'unknown');
  const otherUnresolvedNow = () => [folderPolicy, create, rename, folderMove, documentMove].some(item => item.get()?.status === 'pending' || item.get()?.status === 'unknown');
  const [baseline, setBaseline] = useState<DocumentPolicySnapshot>(); const [mode, setMode] = useState<'inherit' | 'explicit'>('inherit'); const [grants, setGrants] = useState<PolicyGrantInput[]>([]); const [reason, setReason] = useState('');
  const [reading, setReading] = useState(false); const [stale, setStale] = useState(false); const [localError, setLocalError] = useState('');
  const busy = useRef(false); const generation = useRef(0);
  useEffect(() => () => { generation.current += 1; }, []);
  const policyKey = ['document-access-policy', document.documentId] as const; const documentKey = ['document', document.documentId, view] as const;
  const policyState = useSyncExternalStore(listener => client.getQueryCache().subscribe(listener), () => client.getQueryState<AccessPolicyRead>(policyKey));
  const documentState = useSyncExternalStore(listener => client.getQueryCache().subscribe(listener), () => client.getQueryState<DocumentDetail>(documentKey));
  function currentRead() { return [client.getQueryState(policyKey), client.getQueryState(documentKey)].every(state => state?.status === 'success' && state.fetchStatus === 'idle' && !state.isInvalidated); }
  const valid = !error && validateDocumentPolicy(policy, document.documentId) && canManageDocumentPolicy(document, document.documentId);
  const current = valid && policyState?.status === 'success' && policyState.fetchStatus === 'idle' && !policyState.isInvalidated && documentState?.status === 'success' && documentState.fetchStatus === 'idle' && !documentState.isInvalidated;
  const changed = Boolean(baseline && valid && !documentPolicySnapshotEqual(baseline, { document, policy: policy! }));
  function adopt(snapshot: DocumentPolicySnapshot, preserve: boolean) {
    setBaseline(snapshot); setStale(false); setLocalError('');
    if (!preserve) { setMode(snapshot.policy.bindingMode); setGrants(snapshot.policy.effectiveGrants.map(policyGrantInput)); }
    else setGrants(previous => snapshot.policy.effectiveGrants.map(grant => policyGrantInput(previous.find(row => policySubjectKey(row) === policySubjectKey(grant)) ?? grant)));
  }
  useEffect(() => { if (!baseline && current) adopt({ document, policy: policy! }, false); }, [baseline, current, document, policy]);
  async function read(): Promise<DocumentPolicySnapshot> {
    await client.cancelQueries({ queryKey: documentKey, exact: true }, { revert: false });
    // Reuse the mounted detail observer's canonical read, including its denial barriers.
    // A successful read with manageAccess disabled is still a valid Document read.
    await client.fetchQuery({ queryKey: documentKey, staleTime: 0, retry: false });
    if (!canManageDocumentPolicy(client.getQueryData<DocumentDetail>(documentKey), document.documentId)) throw new Error(readError);
    await client.cancelQueries({ queryKey: policyKey, exact: true }, { revert: false });
    await client.fetchQuery({ queryKey: policyKey, staleTime: 0, retry: false, queryFn: async ({ signal }) => { const value = await documentApi.getDocumentAccessPolicy(document.documentId); if (signal.aborted || !validateDocumentPolicy(value, document.documentId)) throw new Error(readError); return value; } });
    const freshDocument = client.getQueryData<DocumentDetail>(documentKey); const freshPolicy = client.getQueryData<AccessPolicyRead>(policyKey);
    if (!currentRead() || !canManageDocumentPolicy(freshDocument, document.documentId) || !validateDocumentPolicy(freshPolicy, document.documentId)) throw new Error(readError);
    return { document: freshDocument, policy: freshPolicy };
  }
  const activeGrants = grants.filter(grant => grant.actions.length > 0);
  const validation = policyReasonValidation(reason) ?? (mode === 'explicit' && !activeGrants.length ? '個別設定には少なくとも1主体の権限が必要です。' : null);
  const draft: DocumentPolicyRequest = { operationId: '', expectedPolicyRevision: baseline?.policy.policyRevision ?? 0, reason: metadataReason(reason), ...(mode === 'explicit' ? { mode, grants: activeGrants } : { mode }) };
  const revisionError = baseline && policyRevisionError(baseline.policy.policyRevision, policyChanged(baseline.policy, draft));
  async function save(event: React.FormEvent) {
    event.preventDefault(); if (busy.current || store.get() || !baseline || !currentRead() || !current || changed || stale || localError || validation || revisionError || otherUnresolvedNow()) return;
    busy.current = true; setReading(true); const token = generation.current;
    try {
      const fresh = await read(); if (generation.current !== token || store.get()) return;
      if (otherUnresolvedNow()) { setLocalError(otherBlocked); return; }
      if (!documentPolicySnapshotEqual(baseline, fresh)) { setStale(true); return; }
      await sendDocumentAccessPolicyOperation({ store, targetDocumentId: fresh.document.documentId, context: { documentId: fresh.document.documentId, title: fresh.document.title, view }, baseline: fresh.policy, request: { ...draft, operationId: createOperationId() }, send: documentApi.setDocumentAccessPolicy, invalidate: () => refreshFolderMoveReads(client) });
    } catch { if (generation.current === token) setLocalError(readError); }
    finally { if (generation.current === token) { busy.current = false; setReading(false); } }
  }
  async function review() {
    if (busy.current || store.get() || otherUnresolvedNow()) return; busy.current = true; setReading(true); const token = generation.current;
    try { const fresh = await read(); if (generation.current === token && !store.get()) { if (otherUnresolvedNow()) setLocalError(otherBlocked); else adopt(fresh, Boolean(baseline)); } }
    catch { if (generation.current === token) setLocalError(readError); }
    finally { if (generation.current === token) { busy.current = false; setReading(false); } }
  }
  const locked = reading || !current || changed || stale || Boolean(localError) || otherUnresolved;
  return <>
    {loading && <LoadingState label="アクセス設定を読み込み中" />}
    {(!loading && !valid || localError) && <p role="alert">{localError || readError}</p>}
    {baseline && current && !changed && !stale && !localError && <>
      <dl className={styles.resultFacts}><dt>設定方式</dt><dd>{policy!.bindingMode === 'inherit' ? '上位フォルダーから継承' : 'この文書に明示'}</dd><dt>適用元</dt><dd>{policy!.effectiveSource.kind} / {policy!.effectiveSource.id}</dd><dt>policy revision</dt><dd>{policy!.policyRevision}</dd></dl>
      <section aria-label="現在有効なアクセス権"><h3>現在有効なアクセス権</h3><table className={styles.grantMatrix}><thead><tr><th scope="col">対象</th>{policyActions.map(action => <th scope="col" key={action}>{actionLabels[action]}</th>)}</tr></thead><tbody>{policy!.effectiveGrants.map(grant => <tr key={policySubjectKey(grant)}><th scope="row">{grant.presentation.displayName ?? grant.subjectId}</th>{policyActions.map(action => <td key={action}>{grant.actions.includes(action) ? '許可' : '—'}</td>)}</tr>)}</tbody></table></section>
    </>}
    {baseline && <form className={styles.policyForm} onSubmit={event => void save(event)}>
      <fieldset className={styles.policyModes} disabled={locked}><legend>設定方法</legend><label><input type="radio" name="policy-mode" checked={mode === 'inherit'} onChange={() => setMode('inherit')} />親フォルダーのアクセス権を継承</label><label><input type="radio" name="policy-mode" checked={mode === 'explicit'} onChange={() => setMode('explicit')} />この文書だけに個別設定</label></fieldset>
      {mode === 'explicit' && current && !changed && !stale && !localError && <><p>確認した既存の主体だけを編集します。個別設定全体を保存し、保存時点の上位設定との一致は保証しません。</p>{grants.map(grant => <fieldset disabled={locked} key={policySubjectKey(grant)} aria-label={policySubjectLabel(grant)}><legend>{policySubjectLabel(grant)}</legend><p>{baseline.policy.effectiveGrants.find(row => policySubjectKey(row) === policySubjectKey(grant))?.presentation.displayName ?? grant.subjectId}</p>{policyActions.map(action => <label key={action}><input type="checkbox" checked={grant.actions.includes(action)} onChange={event => setGrants(rows => rows.map(row => policySubjectKey(row) === policySubjectKey(grant) ? { ...row, actions: event.target.checked ? policyActions.filter(value => value === action || row.actions.includes(value)) : row.actions.filter(value => value !== action) } : row))} />{actionLabels[action]}</label>)}<button type="button" onClick={() => setGrants(rows => rows.map(row => policySubjectKey(row) === policySubjectKey(grant) ? { ...row, actions: [] } : row))}>この主体を削除</button>{!grant.actions.length && <p>この主体は保存時に削除されます。</p>}</fieldset>)}</>}
      {mode === 'inherit' && <p>この文書の個別設定を解除し、その時点の上位フォルダーの設定を適用します。以前の個別設定へ戻す操作ではありません。</p>}
      <p>自分の閲覧・管理権限を失う可能性があります。</p>
      <label className={workspace.formField}>変更理由<textarea rows={3} value={reason} disabled={locked} onChange={event => setReason(event.target.value)} /></label>
      {validation && reason && <p role="alert">{validation}</p>}{revisionError && <p role="alert">{revisionError}</p>}
      <button type="submit" disabled={locked || Boolean(validation) || Boolean(revisionError)}>アクセス設定を保存</button>
    </form>}
    {otherUnresolved && <p role="alert">{otherBlocked}</p>}
    {(changed || stale || baseline && !current && !reading) && <p role="alert">文書または実効アクセス設定が変わったか、読取が失効しました。変更案を保持しています。最新の状態を取得し、もう一度確認してください。</p>}
    {reading && <p role="status">文書とアクセス設定の最新の状態を確認しています…</p>}
    {(!valid || changed || stale || localError || baseline && !current && !reading) && <button type="button" disabled={reading || otherUnresolved} onClick={() => void review()}>最新の状態を取得して見直す</button>}
  </>;
}
