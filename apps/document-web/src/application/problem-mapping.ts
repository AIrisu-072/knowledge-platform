import type { FieldError, Problem } from '@knowledge-platform/document-api-client';

export type ApiProblemState =
  | 'authentication'
  | 'forbidden'
  | 'notFound'
  | 'validation'
  | 'conflict'
  | 'pending'
  | 'error';

export type ApiProblemPresentation = {
  state: ApiProblemState;
  message: string;
  retryable: boolean;
  exactRetry?: boolean;
  traceId: string;
  fieldErrors: readonly FieldError[];
  recovery?: Problem['recovery'];
};

export function problemFromUnknown(error: unknown): Problem | null {
  const candidate = typeof error === 'object' && error !== null && 'problem' in error
    ? (error as { problem?: unknown }).problem
    : error;
  if (typeof candidate !== 'object' || candidate === null) return null;
  const value = candidate as Partial<Problem>;
  return typeof value.type === 'string'
    && typeof value.title === 'string'
    && typeof value.status === 'number'
    && typeof value.code === 'string'
    && typeof value.traceId === 'string'
    && typeof value.retryable === 'boolean'
    ? value as Problem
    : null;
}

const problemMessages: Readonly<Record<string, { state: ApiProblemState; message: string }>> = {
  AUTHENTICATION_REQUIRED: {
    state: 'authentication',
    message: 'セッションを確認できません。再読み込みして、もう一度お試しください。',
  },
  FORBIDDEN: {
    state: 'forbidden',
    message: 'この操作を行う権限がありません。',
  },
  DOCUMENT_NOT_FOUND: { state: 'notFound', message: '文書が見つからないか、閲覧できません。' },
  DOCUMENT_VERSION_NOT_FOUND: { state: 'notFound', message: '版が見つからないか、閲覧できません。' },
  REVISION_NOT_FOUND: { state: 'notFound', message: '改訂が見つからないか、閲覧できません。' },
  FOLDER_NOT_FOUND: { state: 'notFound', message: 'フォルダーが見つからないか、閲覧できません。' },
  VALIDATION_FAILED: { state: 'validation', message: '入力内容を確認してください。' },
  REVISION_CONFLICT: {
    state: 'conflict',
    message: '文書の状態が更新されています。最新の内容を確認してから再操作してください。',
  },
  OPERATION_CONFLICT: {
    state: 'conflict',
    message: '同じ操作IDに異なる内容が指定されています。入力を確認してください。',
  },
  CURSOR_STALE: { state: 'conflict', message: '一覧が更新されています。条件を再適用してください。' },
  STALE_VERSION: { state: 'conflict', message: '版が更新されています。最新の版を確認してください。' },
  STALE_COMPARISON_INPUT: {
    state: 'conflict',
    message: '比較対象が更新されています。対象を選び直してください。',
  },
  RESERVED_DOCUMENT: { state: 'conflict', message: '文書は予約公開中のため、この操作を実行できません。' },
  FOLDER_CYCLE: { state: 'validation', message: 'フォルダーの移動先を確認してください。' },
  ROOT_PROTECTED: { state: 'validation', message: 'ルートフォルダーにはこの操作を実行できません。' },
  BUSINESS_RULE_REJECTED: { state: 'validation', message: '現在の文書状態ではこの操作を実行できません。' },
  PUBLISH_QUALITY_REJECTED: { state: 'validation', message: '公開前の確認で修正が必要な項目が見つかりました。' },
  UNSUPPORTED_MEDIA_TYPE: { state: 'validation', message: 'このファイル形式には対応していません。' },
  IDENTITY_UNAVAILABLE: { state: 'pending', message: '認証情報を確認できません。しばらくしてから再読み込みしてください。' },
  DEPENDENCY_UNAVAILABLE: { state: 'pending', message: '文書サービスに接続できません。しばらくしてから再試行してください。' },
  TIMEOUT: { state: 'pending', message: '応答を確認できませんでした。操作結果を確認してから再試行してください。' },
  COMMIT_OUTCOME_UNKNOWN: {
    state: 'pending',
    message: '操作結果を確認できませんでした。操作履歴を確認してから再試行してください。',
  },
  INTEGRITY_VIOLATION: { state: 'error', message: 'データの整合性を確認できません。管理者へお問い合わせください。' },
  INTERNAL: { state: 'error', message: '処理中にエラーが発生しました。時間をおいて再度お試しください。' },
};

export function mapApiProblem(problem: Problem): ApiProblemPresentation {
  const presentation = problemMessages[problem.code] ?? {
    state: 'error' as const,
    message: '処理中にエラーが発生しました。時間をおいて再度お試しください。',
  };

  return {
    ...presentation,
    retryable: problem.retryable,
    ...(problem.exactRetry === undefined ? {} : { exactRetry: problem.exactRetry }),
    traceId: problem.traceId,
    fieldErrors: problem.errors ?? [],
    ...(problem.recovery === undefined ? {} : { recovery: problem.recovery }),
  };
}
