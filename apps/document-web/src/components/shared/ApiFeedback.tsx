import { mapApiProblem, problemFromUnknown } from '../../application/problem-mapping';

export function ApiFeedback({ error, onRetry }: { error: unknown; onRetry?: () => void }) {
  const problem = problemFromUnknown(error);
  const presentation = problem ? mapApiProblem(problem) : {
    state: 'error' as const,
    message: '文書サービスに接続できません。ネットワークを確認して再読み込みしてください。',
    traceId: '',
  };
  const heading = presentation.state === 'authentication'
    ? 'ログインが必要です'
    : presentation.state === 'forbidden'
      ? 'アクセスできません'
      : presentation.state === 'notFound'
        ? '見つかりません'
        : '読み込みに失敗しました';

  return (
    <section className="notice notice-error" role="alert" aria-labelledby="api-feedback-title">
      <h2 id="api-feedback-title">{heading}</h2>
      <p>{presentation.message}</p>
      {presentation.traceId && <small>照会ID: {presentation.traceId}</small>}
      {onRetry && <button type="button" onClick={onRetry}>再読み込み</button>}
    </section>
  );
}

export function LoadingState({ label = '読み込み中' }: { label?: string }) {
  return <p role="status" aria-live="polite">{label}…</p>;
}
