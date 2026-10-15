import { useEffect, useRef, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { isDisclosureDenied, workApi, WorkApiError, workErrorMessage, type AgentExecution, type AgentResult, type RevisionRef } from '../../application/work-workspace';
import shared from '../../routes/TaskWorkspace.module.css';
import styles from './AgentChat.module.css';

const SOURCE_USE = { referenced: '参照情報だけを使用', analyzed: '本文を分析', unavailable: '利用できなかった', unsupported: '扱えない形式' } as const;
const refs = (values: RevisionRef[]) => values.map((ref) => `${ref.id}（版 ${ref.revision}）`).join('、');
export const isPartialResult = (result: AgentResult) => (result.sourceOutcomes ?? []).some((outcome) => outcome.outcome === 'unavailable' || outcome.outcome === 'unsupported');

/** Per-source use is shown next to the result it qualifies, never upgraded to verified. */
export function SourceOutcomes({ result }: { result: AgentResult }) {
  const outcomes = result.sourceOutcomes ?? [];
  if (!outcomes.length) return <p className={shared.muted}>根拠ごとの利用結果は記録されていません（以前の実行）。</p>;
  return <>
    {isPartialResult(result) && <p role="status">一部の根拠を利用できませんでした。利用できた根拠だけで作成した結果であり、全体を確認したものではありません。</p>}
    <ul aria-label="根拠ごとの利用結果">{outcomes.map((outcome) => <li key={outcome.evidenceRevisionRef.id}>根拠 {outcome.evidenceRevisionRef.id}（版 {outcome.evidenceRevisionRef.revision}）：{SOURCE_USE[outcome.outcome]}</li>)}</ul>
  </>;
}

function useMounted() {
  const mounted = useRef(true);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  return mounted;
}

type CandidateProps = { execution: AgentExecution; scope: readonly unknown[]; onDenied: () => void; draftBlocked: string | null; useDraft: (text: string) => void };
type Scoped = { executionId: string; workItemId: string; attemptId: string };
/** A record outside the displayed execution is treated like a hidden one. */
function inScope<T extends Scoped>(record: T, execution: AgentExecution): T {
  if (record.executionId !== execution.id || record.workItemId !== execution.workItemId || record.attemptId !== execution.attemptId) throw new WorkApiError(404, 'WORK_ITEM_NOT_FOUND');
  return record;
}
// A lifecycle transition can invalidate a read's authorization snapshot; re-read only that bounded race.
const retryStale = (failureCount: number, error: unknown) => failureCount < 2 && error instanceof WorkApiError && error.status === 409 && error.code === 'WORK_CONTEXT_STALE';

/** A private draft candidate. Using it re-reads it under current rights and only
 * fills the unsaved Work draft; saving stays the Human's normal draft save. */
export function GeneratedArtifactCard({ id, execution, scope, onDenied, draftBlocked, useDraft }: CandidateProps & { id: string }) {
  const query = useQuery({ queryKey: [...scope, 'generated', id], queryFn: async () => inScope(await workApi.getGeneratedArtifact(id), execution), staleTime: 0, gcTime: 0, retry: retryStale, retryDelay: 250 });
  const [pending, setPending] = useState(false);
  const [failure, setFailure] = useState<unknown>(null);
  const mounted = useMounted();
  useEffect(() => { if (isDisclosureDenied(query.error)) onDenied(); }, [query.error]);
  if (query.isPending) return <p role="status">下書き候補を確認中…</p>;
  if (query.isError) return isDisclosureDenied(query.error) ? null : <p role="alert">{workErrorMessage(query.error)}</p>;
  const candidate = query.data;
  async function use() {
    if (draftBlocked || pending) return;
    setPending(true); setFailure(null);
    try {
      const current = inScope(await workApi.getGeneratedArtifact(id), execution);
      if (mounted.current) useDraft(current.value.text);
    } catch (error) {
      if (!mounted.current) return;
      if (isDisclosureDenied(error)) onDenied(); else setFailure(error);
    } finally { if (mounted.current) setPending(false); }
  }
  return <article className={styles.candidate} aria-label={`下書き候補 ${candidate.id}`}>
    <p className={shared.muted}>Agentの下書き候補 · 未保存・非公開{candidate.simulated ? ' · 模擬処理' : ''} · 作成 {candidate.author}</p>
    <h4>{candidate.title}</h4>
    <p className={shared.text}>{candidate.value.text}</p>
    <p className={shared.muted}>出典の根拠：{refs(candidate.sourceRevisionRefs)}</p>
    <div className={shared.actions}><button type="button" disabled={Boolean(draftBlocked) || pending} aria-busy={pending} onClick={() => void use()}>作業文案に入れる</button></div>
    {draftBlocked && <p className={shared.muted}>{draftBlocked}</p>}
    {Boolean(failure) && <p role="alert">{workErrorMessage(failure)}</p>}
  </article>;
}

/** A typed proposal with no execution authority. Choosing it re-reads the
 * proposal and its target, then opens the normal screen for the Human. */
export function SuggestedActionItem({ id, execution, scope, onDenied, draftBlocked, useDraft, result, reviewFinding }: CandidateProps & { id: string; result: AgentResult; reviewFinding: (ref: RevisionRef) => void }) {
  const query = useQuery({ queryKey: [...scope, 'suggested', id], queryFn: async () => inScope(await workApi.getSuggestedAction(id), execution), staleTime: 0, gcTime: 0, retry: retryStale, retryDelay: 250 });
  const [pending, setPending] = useState(false);
  const [failure, setFailure] = useState<unknown>(null);
  const mounted = useMounted();
  useEffect(() => { if (isDisclosureDenied(query.error)) onDenied(); }, [query.error]);
  if (query.isPending) return <li><p role="status">提案を確認中…</p></li>;
  if (query.isError) return isDisclosureDenied(query.error) ? null : <li><p role="alert">{workErrorMessage(query.error)}</p></li>;
  const suggestion = query.data;
  const blocked = suggestion.action.kind === 'use_generated_artifact' ? draftBlocked : null;
  async function open() {
    if (blocked || pending) return;
    setPending(true); setFailure(null);
    try {
      // Re-read the proposal and its target under current rights before opening anything.
      const current = inScope(await workApi.getSuggestedAction(id), execution);
      if (current.action.kind === 'review_finding') {
        const target = current.action.findingRevisionRef;
        if (!result.findingRevisionRefs.some((ref) => ref.id === target.id && ref.revision === target.revision)) throw new Error('target_mismatch');
        const finding = await workApi.getFinding(target.id);
        if (finding.revision !== target.revision || finding.taskId !== current.workItemId || finding.attemptId !== current.attemptId || finding.originExecutionId !== current.executionId) throw new Error('target_mismatch');
        if (mounted.current) reviewFinding(target);
      } else {
        if (!(result.generatedArtifactIds ?? []).includes(current.action.generatedArtifactId)) throw new Error('target_mismatch');
        const candidate = inScope(await workApi.getGeneratedArtifact(current.action.generatedArtifactId), execution);
        if (mounted.current) useDraft(candidate.value.text);
      }
    } catch (error) {
      if (!mounted.current) return;
      if (isDisclosureDenied(error)) onDenied(); else setFailure(error);
    } finally { if (mounted.current) setPending(false); }
  }
  const label = suggestion.action.kind === 'review_finding' ? `候補 ${suggestion.action.findingRevisionRef.id} を確認し、人間判断を記録する` : '下書き候補を作業文案に使う';
  return <li className={styles.suggestion} aria-label={`提案 ${suggestion.id}`}>
    <p>{label}</p>
    <p className={shared.muted}>理由：{suggestion.rationale}</p>
    <div className={shared.actions}><button type="button" disabled={Boolean(blocked) || pending} aria-busy={pending} onClick={() => void open()}>提案を開く</button></div>
    {blocked && <p className={shared.muted}>{blocked}</p>}
    {Boolean(failure) && <p role="alert">{failure instanceof Error && failure.message === 'target_mismatch' ? '提案の対象が現在の結果と一致しません。実行状態を再読込してください。' : workErrorMessage(failure)}</p>}
  </li>;
}
