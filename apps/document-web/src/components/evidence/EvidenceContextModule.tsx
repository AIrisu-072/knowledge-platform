import { useEffect, useRef, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Link } from '@tanstack/react-router';
import { Dialog, Heading, Modal } from 'react-aria-components';
import { useTaskTransient, emptyEvidenceDraft, type EvidenceDraft } from '../../application/organization-context';
import { documentApi } from '../../application/document-workspace';
import { sourceFromPublishedDocument, toggleReference } from '../../application/evidence-workspace';
import { evidenceRecordsKey, useEvidenceRecords } from '../../application/use-evidence-records';
import { createOperationId } from '../../application/operation-id';
import { validateDetailSearch } from '../../application/search-state';
import { workApi, executeWorkOperation, isDisclosureDenied, isOperationNotFound, isUnknownOutcome, workErrorMessage, type EvidenceRecord, type Finding, type HumanDecision, type TaskDetail, type WorkSession, type WorkResult, type WorkOperation } from '../../application/work-workspace';
import { formatDateTime } from '../../view-model/date-time';
import styles from './EvidenceContextModule.module.css';
import shared from '../../routes/TaskWorkspace.module.css';
const bounded = (value: string) => Boolean(value.trim()) && new TextEncoder().encode(value).length <= 8192;
const optionalBounded = (value: string) => !value || bounded(value);
const deniedSource = (error: unknown) => isDisclosureDenied(error) || Boolean(error && typeof error === 'object' && 'status' in error && [401, 403, 404].includes(Number(error.status)));
const emptyDecision = { decision: 'accepted' as const, adoptedClaim: '', reason: '' };
export const decisionLabel = (decision: HumanDecision['decision']) => ({ accepted: '採用', modified: '修正して採用', rejected: '却下' })[decision];

export function EvidenceContextModule({ session, task, applyResult, onDenied }: { session: WorkSession; task: TaskDetail; applyResult: (result: WorkResult) => void; onDenied: () => void }) {
  const client = useQueryClient();
  const scope = `${session.principalId}:${session.actingAssignmentId}:${task.id}:${task.attemptId}`;
  const [transient, setTransient] = useTaskTransient(scope);
  const draft = transient.evidence ?? emptyEvidenceDraft;
  const update = (patch: Partial<EvidenceDraft>) => setTransient((previous) => ({ ...previous, evidence: { ...(previous.evidence ?? emptyEvidenceDraft), ...patch }, notice: '' }));
  const records = useEvidenceRecords(session, task);
  const [confirmation, setConfirmation] = useState<Finding | null>(null);
  const active = useRef(true);
  useEffect(() => { active.current = true; return () => { active.current = false; }; }, []);
  const sourceKey = [...evidenceRecordsKey(session, task), 'source'];
  const document = useQuery({ queryKey: [...sourceKey, draft.documentId], queryFn: () => documentApi.getDocument(draft.documentId, 'published'), enabled: task.canRegisterEvidence && task.inputResources.some((item) => item.documentId === draft.documentId), retry: false, staleTime: 0, gcTime: 0 });
  const sourceVersion = document.isSuccess ? document.data.displayRevision?.documentVersionId : undefined;
  const files = useQuery({ queryKey: [...sourceKey, draft.documentId, sourceVersion, 'files'], queryFn: () => documentApi.listVersionFiles(draft.documentId, sourceVersion!, 'published'), enabled: Boolean(sourceVersion), retry: false, staleTime: 0, gcTime: 0 });
  const file = files.isSuccess ? files.data.items.find((item) => `${item.contentItemId}:${item.representationId}` === draft.fileKey && item.role === 'AUTHORITATIVE') : undefined;
  const source = document.isSuccess && file ? sourceFromPublishedDocument(draft.documentId, document.data, file) : null;
  const sourceError = document.error || files.error;
  useEffect(() => {
    if (deniedSource(sourceError) || isDisclosureDenied(records.error)) { active.current = false; client.removeQueries({ queryKey: sourceKey }); onDenied(); }
  }, [sourceError, records.error]);
  const mutation = useMutation({ retry: false, mutationFn: async ({ operation, recovery = false }: { operation: WorkOperation; recovery?: boolean }) => recovery ? workApi.getOperation(operation.input.operationId) : executeWorkOperation(operation),
    onSuccess: (result, request) => {
      if (!active.current) return;
      if (result.task.id !== task.id || result.task.attemptId !== task.attemptId || result.kind !== request.operation.kind) { setTransient((previous) => ({ ...previous, unknown: true, notice: '応答と操作が一致しません。同じ操作IDで結果を確認してください。' })); return; }
      applyResult(result);
      setTransient((previous) => ({ ...previous, operation: null, unknown: false, error: null, notice: result.kind === 'evidence_registered' ? '根拠を登録しました' : result.kind === 'finding_registered' ? '候補を登録しました' : '人間判断を記録しました', evidence: { ...(previous.evidence ?? emptyEvidenceDraft), ...(result.kind === 'finding_registered' ? { claim: '', support: [] } : {}), ...(result.kind === 'decision_recorded' ? { decisions: { ...(previous.evidence?.decisions ?? {}), [result.decision.findingId]: emptyDecision } } : {}) } }));
      setConfirmation(null); void client.invalidateQueries({ queryKey: evidenceRecordsKey(session, task) });
    },
    onError: (error, request) => {
      if (!active.current) return;
      if (isDisclosureDenied(error) && !(request.recovery && isOperationNotFound(error))) { active.current = false; onDenied(); return; }
      const unknown = Boolean(request.recovery) || isUnknownOutcome(error);
      setTransient((previous) => ({ ...previous, error, unknown, operation: unknown ? previous.operation : null })); setConfirmation(null);
    },
  });
  const busy = transient.unknown || mutation.isPending;
  const common = () => ({ operationId: createOperationId(), expectedRevision: task.revision, expectedAttemptId: task.attemptId, actingAssignmentId: session.actingAssignmentId });
  const start = (operation: WorkOperation) => { if (busy) return; setTransient((previous) => ({ ...previous, operation, unknown: true, notice: '', error: null })); mutation.mutate({ operation }); };
  const confirmationDraft = confirmation ? draft.decisions[confirmation.id] ?? emptyDecision : emptyDecision;
  const canDecide = (value: typeof confirmationDraft) => optionalBounded(value.reason) && (value.decision !== 'modified' || bounded(value.adoptedClaim));
  if (records.isError || deniedSource(sourceError)) return <p role="alert">{workErrorMessage(records.error || sourceError)}</p>;
  return <section className={styles.module} aria-label="根拠・候補・人間判断">
    <h2>根拠・候補・人間判断</h2><p>根拠は原本への参照、候補は主張、人間判断は別の記録です。判断だけでは提出・差戻は行いません。</p><p className={shared.muted}>根拠・候補・各候補の判断はこのPoCでは可視16件までです</p>
    {records.isPending && <p role="status">根拠・候補を読み込み中…</p>}
    {task.canRegisterEvidence && <fieldset disabled={busy}><legend>根拠を登録</legend>
      <label>根拠にする入力文書<select aria-label="根拠にする入力文書" value={draft.documentId} onChange={(event) => update({ documentId: event.target.value, fileKey: '' })}><option value="">文書を選択</option>{task.inputResources.map((item) => <option key={item.documentId} value={item.documentId}>{item.label}</option>)}</select></label>
      <p className={shared.muted}>新規登録は現在の公開版のみ対応しています。原本本文は保持しません。該当箇所は人間の記載で、未検証です。</p>
      {document.isFetching && <p role="status">公開版を確認中…</p>}
      {sourceError && <p role="alert">原本情報を取得できません。再読込して確認してください。</p>}
      {document.isSuccess && <><p className={shared.muted}>改訂 {document.data.displayRevision?.label ?? '利用できません'} · {document.data.displayRevision?.revisionId}<br />版 {sourceVersion}</p><Link to="/documents/$documentId" params={{ documentId: draft.documentId }} search={validateDetailSearch({ view: 'published' })}>版・改訂を確認</Link></>}
      {files.isSuccess && <label>原本ファイル<select aria-label="原本ファイル" value={draft.fileKey} onChange={(event) => update({ fileKey: event.target.value })}><option value="">ファイルを選択</option>{files.data.items.filter((item) => item.role === 'AUTHORITATIVE').map((item) => <option key={`${item.contentItemId}:${item.representationId}`} value={`${item.contentItemId}:${item.representationId}`}>{item.displayName}</option>)}</select></label>}
      {file && <p className={shared.muted}>{file.displayName}<br />内容 {file.contentItemId}<br />表現 {file.representationId}</p>}
      <label>該当箇所（人間の記載・未検証）<textarea aria-label="該当箇所（人間の記載・未検証）" value={draft.relevantLocation} onChange={(event) => update({ relevantLocation: event.target.value })} /></label>
      <p className={shared.muted}>UTF-8で8192バイト以内 · 保持本文なし（not_retained） · 検証範囲は不明（unknown）</p>
      <button type="button" disabled={!source || !bounded(draft.relevantLocation)} onClick={() => source && start({ kind: 'evidence_registered', taskId: task.id, input: { ...common(), ...source, relevantLocation: draft.relevantLocation } })}>根拠を登録</button>
    </fieldset>}
    <h3>登録された根拠</h3>{records.isSuccess && records.data.evidence.length === 0 && <p>登録された根拠はありません</p>}
    {records.data?.evidence.map((evidence) => <EvidenceView key={evidence.id} evidence={evidence} onDenied={onDenied} />)}
    {task.canRegisterFinding && <fieldset disabled={busy || !records.isSuccess}><legend>候補を登録</legend><label>候補の主張<textarea aria-label="候補の主張" value={draft.claim} onChange={(event) => update({ claim: event.target.value })} /></label><p className={shared.muted}>原本の事実として確定しない候補です。根拠を1件以上、100件以下選んでください。UTF-8で8192バイト以内</p>{records.data?.evidence.map((evidence) => <label key={evidence.id}><input type="checkbox" aria-label={`候補の根拠 ${evidence.id}`} checked={draft.support.some((ref) => ref.id === evidence.id && ref.revision === evidence.revision)} onChange={(event) => update({ support: toggleReference(draft.support, evidence, event.target.checked) })} />根拠 {evidence.id} · 版 {evidence.revision}</label>)}<button type="button" disabled={!bounded(draft.claim) || !draft.support.length || draft.support.length > 100 || draft.support.some((ref) => !records.data?.evidence.some((record) => record.id === ref.id && record.revision === ref.revision))} onClick={() => start({ kind: 'finding_registered', taskId: task.id, input: { ...common(), claim: draft.claim, evidenceRevisionRefs: draft.support } })}>候補を登録</button></fieldset>}
    <h3>候補と人間判断</h3>{records.isSuccess && records.data.findings.length === 0 && <p>登録された候補はありません</p>}
    {records.data?.findings.map((finding) => { const value = draft.decisions[finding.id] ?? emptyDecision; const change = (patch: Partial<typeof value>) => update({ decisions: { ...draft.decisions, [finding.id]: { ...value, ...patch } } }); return <section key={finding.id} className={shared.snapshot} aria-label={`候補 ${finding.id}`}><h4>候補</h4><p className={shared.text}>{finding.claim}</p><p className={shared.muted}>{finding.id} · 候補版 {finding.revision}<br />作成 {finding.author} · 試行 {finding.attemptId}</p>{finding.originExecutionId && <p className={shared.muted}>合成実行 · 生成元の実行 {finding.originExecutionId}<br />原本本文の分析なし · 実LLM・MCP通信なし</p>}<p>支える根拠：{finding.evidenceRevisionRefs.map((ref) => `${ref.id}（版 ${ref.revision}）`).join('、')}</p>
      {finding.uncertainty.length > 0 && <p>不確実な点：{finding.uncertainty.join('、')}</p>}{finding.conflicts.length > 0 && <p>競合参照：{finding.conflicts.map((ref) => `${ref.id}（版 ${ref.revision}）`).join('、')}</p>}
      {records.data.decisions.filter((decision) => decision.findingId === finding.id).map((decision) => <DecisionView key={decision.id} decision={decision} />)}
      {task.canRecordDecision && <fieldset disabled={busy}><legend>別記録として人間判断</legend><label>候補の判断 {finding.id}<select aria-label={`候補の判断 ${finding.id}`} value={value.decision} onChange={(event) => change({ decision: event.target.value as HumanDecision['decision'] })}><option value="accepted">採用</option><option value="modified">修正して採用</option><option value="rejected">却下</option></select></label>{value.decision === 'modified' && <label>採用文<textarea aria-label="採用文" value={value.adoptedClaim} onChange={(event) => change({ adoptedClaim: event.target.value })} /></label>}<label>判断理由<textarea aria-label="判断理由" value={value.reason} onChange={(event) => change({ reason: event.target.value })} /></label><p className={shared.muted}>修正時は採用文が必須です。各入力はUTF-8で8192バイト以内</p><button type="button" disabled={!canDecide(value)} onClick={() => setConfirmation(finding)}>判断内容を確認</button></fieldset>}
    </section>; })}
    {confirmation && <Modal className={shared.dialogScrim} isOpen isDismissable={false} isKeyboardDismissDisabled={mutation.isPending} onOpenChange={(open) => { if (!open && !mutation.isPending) setConfirmation(null); }}><Dialog className={shared.dialog} aria-label="人間判断の確認"><Heading slot="title">人間判断の確認</Heading><p>候補版 {confirmation.revision} · {confirmation.id}</p><p className={shared.text}>{confirmation.claim}</p><p>支える根拠：{confirmation.evidenceRevisionRefs.map((ref) => `${ref.id}（版 ${ref.revision}）`).join('、')}</p><p>判断：{decisionLabel(confirmationDraft.decision)}</p>{confirmationDraft.decision === 'modified' && <p className={shared.text}>採用文：{confirmationDraft.adoptedClaim}</p>}{confirmationDraft.reason && <p className={shared.text}>理由：{confirmationDraft.reason}</p>}<p>実行する担当 {session.principalId} · {session.actingAssignmentId}<br />現在のタスク {task.id} · 試行 {task.attemptId}</p><p>元の候補・根拠・過去の判断を変更せずに記録します。</p><div className={shared.actions}><button type="button" autoFocus disabled={mutation.isPending} onClick={() => setConfirmation(null)}>キャンセル</button><button type="button" disabled={mutation.isPending || !canDecide(confirmationDraft)} onClick={() => start({ kind: 'decision_recorded', taskId: task.id, findingId: confirmation.id, input: { ...common(), taskId: task.id, findingRevision: confirmation.revision, decision: confirmationDraft.decision, ...(confirmationDraft.decision === 'modified' ? { adoptedClaim: confirmationDraft.adoptedClaim } : {}), ...(confirmationDraft.reason ? { reason: confirmationDraft.reason } : {}), evidenceRevisionRefs: confirmation.evidenceRevisionRefs } })}>判断を確定</button></div></Dialog></Modal>}
  </section>;
}
function DecisionView({ decision }: { decision: HumanDecision }) { return <div className={shared.notice}><h5>人間判断：{decisionLabel(decision.decision)}</h5>{decision.adoptedClaim && <p className={shared.text}>採用文：{decision.adoptedClaim}</p>}{decision.reason && <p className={shared.text}>理由：{decision.reason}</p>}<p className={shared.muted}>{decision.id} · 判断版 {decision.revision}<br />{decision.humanPrincipal} · {decision.actingAssignmentId}<br />試行 {decision.attemptId} · <time dateTime={decision.createdAt}>{formatDateTime(decision.createdAt)}</time></p></div>; }
function EvidenceView({ evidence, onDenied }: { evidence: EvidenceRecord; onDenied: () => void }) {
  const [pending, setPending] = useState(false), [error, setError] = useState<unknown>(null);
  const mounted = useRef(true); useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  async function original() { if (pending) return; setPending(true); setError(null); try { const current = await workApi.getEvidence(evidence.id); if (!mounted.current) return; const blob = await documentApi.downloadVersionFile({ documentId: current.sourceRef.resourceId, versionId: current.sourceRef.versionId, contentItemId: current.authoritativeLocator.contentItemId, representationId: current.authoritativeLocator.representationId, purpose: 'history' }); if (!mounted.current) return; const url = URL.createObjectURL(blob), anchor = window.document.createElement('a'); anchor.href = url; anchor.download = `evidence-${evidence.id}`; anchor.click(); window.setTimeout(() => URL.revokeObjectURL(url), 1000); } catch (caught) { if (!mounted.current) return; if (deniedSource(caught)) onDenied(); else setError(caught); } finally { if (mounted.current) setPending(false); } }
  return <section className={shared.snapshot} aria-label={`根拠 ${evidence.id}`}><h4>根拠への参照</h4><p className={shared.muted}>{evidence.id} · 根拠版 {evidence.revision}<br />Document {evidence.sourceRef.resourceId}<br />改訂 {evidence.sourceRef.revisionId}<br />版 {evidence.sourceRef.versionId}<br />内容 {evidence.authoritativeLocator.contentItemId}<br />表現 {evidence.authoritativeLocator.representationId}</p><p className={shared.text}>該当箇所（人間の記載・未検証）：{evidence.relevantLocation}</p><p>本文は保持していません（not_retained）。検証範囲は不明（unknown）です。</p>{evidence.uncertainty.length > 0 && <p>不確実な点：{evidence.uncertainty.join('、')}</p>}{evidence.conflictReferences.length > 0 && <p>競合参照：{evidence.conflictReferences.map((ref) => `${ref.id}（版 ${ref.revision}）`).join('、')}</p>}<p className={shared.muted}>登録者 {evidence.createdBy} · {evidence.actingAssignmentId}<br />取得 <time dateTime={evidence.retrievedAt}>{formatDateTime(evidence.retrievedAt)}</time><br />登録 <time dateTime={evidence.recordedAt}>{formatDateTime(evidence.recordedAt)}</time></p><button type="button" disabled={pending} onClick={() => void original()}>この根拠の原本を取得</button>{Boolean(error) && <p role="alert">原本を取得できません。現在の権限を確認してください。</p>}<p><Link to="/documents/$documentId" params={{ documentId: evidence.sourceRef.resourceId }} search={validateDetailSearch({ view: 'published' })}>版・改訂を確認</Link></p></section>;
}
