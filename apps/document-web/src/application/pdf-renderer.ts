export type PdfViewerSession = { pages: number; render: (page: number, canvas: HTMLCanvasElement) => Promise<void>; destroy: () => void };
export async function openPdfViewer(bytes: Uint8Array, signal: AbortSignal): Promise<PdfViewerSession> {
  const runtime = await import('./pdf-runtime');
  if (signal.aborted) throw new Error('原本表示を中止しました。');
  return runtime.openPdfViewer(bytes, signal);
}
