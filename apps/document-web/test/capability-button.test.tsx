import { render, screen } from '@testing-library/react';
import type { DocumentDetail } from '../src/application/document-workspace';
import { CapabilityButton } from '../src/components/shared/CapabilityButton';

test.each(['hidden', 'unexpected-status'])('runtimeの未認識capability %s は操作も理由も表示しない', status => {
  const availability = { status, reason: 'PRIVATE_RUNTIME_REASON' } as unknown as DocumentDetail['capabilities']['createVersion'];
  const onClick = jest.fn();
  const { container } = render(<CapabilityButton label="作業版を編集" availability={availability} onClick={onClick} />);
  expect(screen.queryByRole('button', { name: '作業版を編集' })).not.toBeInTheDocument();
  expect(screen.queryByText('PRIVATE_RUNTIME_REASON')).not.toBeInTheDocument();
  expect(container).toBeEmptyDOMElement();
  expect(onClick).not.toHaveBeenCalled();
});
