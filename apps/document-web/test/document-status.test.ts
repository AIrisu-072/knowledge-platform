import {
  documentListStatusLabel,
  documentStatusLabel,
  versionStatusLabel,
  type DocumentStatusProjection,
  type VersionStatusProjection,
} from '../src/view-model/document-status';

function guiVersion(overrides: Partial<DocumentStatusProjection['displayVersion']> = {}): DocumentStatusProjection['displayVersion'] {
  return {
    lifecycleState: 'WORKING',
    isCurrent: false,
    approvedAt: null,
    scheduledPublishAt: null,
    ...overrides,
  };
}

test('document labels describe the authoritative lifecycle projection', () => {
  expect(documentStatusLabel({ displayVersion: guiVersion() })).toBe('下書き');
  expect(documentStatusLabel({ displayVersion: guiVersion({ approvedAt: '2026-10-01T00:00:00Z' }) })).toBe('非公開');
  expect(documentStatusLabel({ displayVersion: guiVersion({ scheduledPublishAt: '2026-10-02T00:00:00Z' }) })).toBe('公開待ち');
  expect(documentStatusLabel({ displayVersion: guiVersion({ lifecycleState: 'PUBLISHED', isCurrent: true }) })).toBe('現行版');
  expect(documentStatusLabel({ displayVersion: guiVersion({ lifecycleState: 'PUBLISHED' }) })).toBe('過去版');
  expect(documentStatusLabel({ displayVersion: guiVersion({ lifecycleState: 'WITHDRAWN' }) })).toBe('公開終了');
});

test('list and version labels preserve their source lifecycle states', () => {
  expect(documentListStatusLabel({ displayVersion: guiVersion({ lifecycleState: 'PUBLISHED', isCurrent: true }) })).toBe('現行版');
  expect(documentListStatusLabel({ lifecycleState: 'withdrawn', displayVersion: guiVersion({ lifecycleState: 'PUBLISHED', isCurrent: true }) })).toBe('公開終了');

  const version: VersionStatusProjection = {
    lifecycleState: 'working',
    isCurrent: false,
    approvedAt: null,
    scheduledPublishAt: '2026-10-02T00:00:00Z',
  };
  expect(versionStatusLabel(version)).toBe('公開待ち');
});
