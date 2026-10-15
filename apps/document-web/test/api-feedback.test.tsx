import { render, screen } from '@testing-library/react';
import { ApiFeedback } from '../src/components/shared/ApiFeedback';

test('同時に表示する履歴の拒否案内はそれぞれの見出しでラベル付けする', () => {
  const problem = (status: number, code: string) => ({ type: 'about:blank', title: 'Synthetic', status, code, traceId: 'synthetic', retryable: false });
  render(<><ApiFeedback error={problem(401, 'AUTHENTICATION_REQUIRED')} /><ApiFeedback error={problem(403, 'FORBIDDEN')} /></>);
  const authentication = screen.getByRole('alert', { name: 'ログインが必要です' });
  const refusal = screen.getByRole('alert', { name: 'アクセスできません' });
  expect(authentication.getAttribute('aria-labelledby')).not.toBe(refusal.getAttribute('aria-labelledby'));
});
