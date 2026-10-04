export interface GuiVersionStatusProjection {
  lifecycleState: 'WORKING' | 'PUBLISHED' | 'WITHDRAWN';
  isCurrent: boolean;
  approvedAt: string | null;
  scheduledPublishAt: string | null;
}

export interface DocumentListStatusProjection {
  displayVersion: GuiVersionStatusProjection;
  lifecycleState?: 'working' | 'published' | 'withdrawn';
}

export interface DocumentStatusProjection {
  displayVersion: GuiVersionStatusProjection;
}

export interface VersionStatusProjection {
  lifecycleState: 'working' | 'published' | 'withdrawn';
  isCurrent: boolean;
  approvedAt: string | null;
  scheduledPublishAt: string | null;
}

export function documentListStatusLabel(document: DocumentListStatusProjection): string {
  const version = document.displayVersion;
  if (version.lifecycleState === 'WORKING') return workingStatusLabel(version);
  if (document.lifecycleState === 'withdrawn' || version.lifecycleState === 'WITHDRAWN') return '公開終了';
  return version.isCurrent ? '現行版' : '過去版';
}

export function documentStatusLabel(document: DocumentStatusProjection): string {
  const version = document.displayVersion;
  if (version.lifecycleState === 'WORKING') return workingStatusLabel(version);
  if (version.lifecycleState === 'PUBLISHED') return version.isCurrent ? '現行版' : '過去版';
  return '公開終了';
}

export function versionStatusLabel(version: VersionStatusProjection): string {
  if (version.lifecycleState === 'working') {
    if (version.scheduledPublishAt) return '公開待ち';
    return version.approvedAt ? '非公開' : '下書き';
  }
  if (version.lifecycleState === 'published') return version.isCurrent ? '現行版' : '過去版';
  return '公開終了';
}

function workingStatusLabel(version: GuiVersionStatusProjection): string {
  if (version.scheduledPublishAt) return '公開待ち';
  return version.approvedAt ? '非公開' : '下書き';
}
