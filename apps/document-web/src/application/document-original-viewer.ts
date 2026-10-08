export const VIEWER_MAX_BYTES = 10 * 1024 * 1024;
export const VIEWER_MAX_PIXELS = 4_000_000;
export const VIEWER_DEADLINE_MS = 20_000;
export function viewerFileProblem(file: { mediaType: string; sizeBytes: number }): string | undefined {
  if (!Number.isSafeInteger(file.sizeBytes) || file.sizeBytes < 0 || file.sizeBytes > VIEWER_MAX_BYTES) return '表示は10 MiBまでです。原本をダウンロードして確認してください。';
  if (!['text/plain', 'application/pdf'].includes(file.mediaType.split(';')[0]!.trim().toLowerCase())) return 'この形式は表示に対応していません。原本をダウンロードして確認してください。';
  return undefined;
}
export function decodeViewerText(bytes: Uint8Array): string {
  if (bytes.byteLength > VIEWER_MAX_BYTES) throw new Error('表示は10 MiBまでです。原本をダウンロードしてください。');
  try { return new TextDecoder('utf-8', { fatal: true }).decode(bytes); }
  catch { throw new Error('UTF-8のテキストとして表示できません。原本をダウンロードして確認してください。'); }
}
export function boundedPdfViewport(width: number, height: number) {
  if (!Number.isFinite(width) || !Number.isFinite(height) || width <= 0 || height <= 0) throw new Error('PDFのページ寸法を確認できません。');
  const scale = Math.min(1.5, 4096 / width, 4096 / height, Math.sqrt(VIEWER_MAX_PIXELS / width / height));
  return { scale, width: Math.max(1, Math.floor(width * scale)), height: Math.max(1, Math.floor(height * scale)) };
}
export function viewerElementVisible(element: HTMLElement | null): boolean {
  if (!element?.isConnected || document.visibilityState === 'hidden') return false;
  for (let node: HTMLElement | null = element; node; node = node.parentElement) {
    const style = getComputedStyle(node);
    if (node.hidden || style.display === 'none' || style.visibility === 'hidden' || style.visibility === 'collapse' || style.opacity === '0') return false;
  }
  return true;
}
