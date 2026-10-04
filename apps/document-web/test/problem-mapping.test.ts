import type { Problem } from '@knowledge-platform/document-api-client';

test('stable problem codes map to UI states without using problem prose as control flow', async () => {
  const { mapApiProblem } = await import('../src/application/problem-mapping');
  const problem: Problem = {
    type: 'about:blank',
    title: 'backend title that is not a UI state',
    detail: 'backend detail that is not a UI state',
    status: 409,
    code: 'REVISION_CONFLICT',
    traceId: 'trace-1',
    retryable: true,
    exactRetry: false,
  };

  expect(mapApiProblem(problem)).toMatchObject({
    state: 'conflict',
    message: '文書の状態が更新されています。最新の内容を確認してから再操作してください。',
    retryable: true,
    exactRetry: false,
  });
  expect(mapApiProblem(problem).message).not.toContain(problem.detail);
});

test('unknown problem codes fail to the general error state', async () => {
  const { mapApiProblem } = await import('../src/application/problem-mapping');
  const problem: Problem = {
    type: 'about:blank',
    title: 'unknown title',
    status: 500,
    code: 'NEW_SERVER_CODE',
    traceId: 'trace-2',
    retryable: false,
  };

  expect(mapApiProblem(problem).state).toBe('error');
});
