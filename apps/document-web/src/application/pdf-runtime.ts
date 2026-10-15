import { getDocument, PDFWorker, AnnotationMode } from 'pdfjs-dist';
import type { RenderTask } from 'pdfjs-dist';
import type { PdfViewerSession } from './pdf-renderer';
import { boundedPdfViewport, VIEWER_DEADLINE_MS, VIEWER_MAX_BYTES, VIEWER_MAX_PIXELS } from './document-original-viewer';

export async function openPdfViewer(bytes: Uint8Array, signal: AbortSignal): Promise<PdfViewerSession> {
  if (signal.aborted || bytes.byteLength > VIEWER_MAX_BYTES) throw new Error('原本を表示できません。ダウンロードして確認してください。');
  // An explicit same-origin module port avoids PDF.js's custom-scheme blob/fake-worker fallback.
  const { createPdfWorkerPort } = await import('./pdf-worker-port');
  if (signal.aborted) throw new Error('原本表示を中止しました。');
  const port = createPdfWorkerPort();
  let worker: PDFWorker;
  try { worker = PDFWorker.create({ port }); } catch (error) { port.terminate(); throw error; }
  let loading: ReturnType<typeof getDocument>;
  try { loading = getDocument({ data: bytes, worker, enableXfa: false, disableFontFace: true, useSystemFonts: false,
    useWasm: false, useWorkerFetch: false, stopAtErrors: true, maxImageSize: VIEWER_MAX_PIXELS,
    canvasMaxAreaInBytes: VIEWER_MAX_PIXELS * 4, isOffscreenCanvasSupported: false, isImageDecoderSupported: false }); } catch (error) { worker.destroy(); port.terminate(); throw error; }
  let render: RenderTask | undefined; let dead = false; let timer: ReturnType<typeof setTimeout> | undefined;
  let rejectActive: ((error: Error) => void) | undefined;
  let activeCanvas: HTMLCanvasElement | undefined;
  const destroy = () => { if (dead) return; dead = true; rejectActive?.(new Error('原本表示を中止しました。')); if (timer) clearTimeout(timer); render?.cancel();
    if (activeCanvas) { activeCanvas.width = 0; activeCanvas.height = 0; }
    void loading.destroy().catch(() => undefined); worker.destroy(); port.terminate(); signal.removeEventListener('abort', destroy); };
  signal.addEventListener('abort', destroy, { once: true });
  const budget = async <T>(run: () => Promise<T>): Promise<T> => {
    if (dead || signal.aborted) throw new Error('原本表示を中止しました。');
    try { return await Promise.race([run(), new Promise<never>((_, reject) => { rejectActive = reject; timer = setTimeout(() => { reject(new Error('PDF表示の時間上限を超えました。原本をダウンロードしてください。')); destroy(); }, VIEWER_DEADLINE_MS); })]); }
    finally { if (timer) clearTimeout(timer); timer = undefined; rejectActive = undefined; }
  };
  try {
    const pdf = await budget(() => loading.promise);
    if (dead || !Number.isSafeInteger(pdf.numPages) || pdf.numPages < 1 || pdf.numPages > 10000) throw new Error('PDFのページ数を確認できません。');
    return { pages: pdf.numPages, destroy, async render(number, canvas) {
      if (!Number.isSafeInteger(number) || number < 1 || number > pdf.numPages) throw new Error('PDFのページ指定が不正です。');
      await budget(async () => {
        render?.cancel(); activeCanvas = canvas; canvas.width = 0; canvas.height = 0;
        const page = await pdf.getPage(number);
        if (dead) return;
        try {
          const raw = page.getViewport({ scale: 1 }); const size = boundedPdfViewport(raw.width, raw.height);
          const viewport = page.getViewport({ scale: size.scale }); canvas.width = size.width; canvas.height = size.height;
          const context = canvas.getContext('2d'); if (!context) throw new Error('この環境ではPDFを表示できません。');
          render = page.render({ canvas, canvasContext: context, viewport, annotationMode: AnnotationMode.DISABLE });
          await render.promise;
        } finally { page.cleanup(); render = undefined; }
      });
      if (dead) throw new Error('原本表示を中止しました。');
    } };
  } catch (error) { destroy(); throw error; }
}
