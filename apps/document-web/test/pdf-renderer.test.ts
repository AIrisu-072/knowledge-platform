import { openPdfViewer } from '../src/application/pdf-runtime';
import { getDocument, PDFWorker } from 'pdfjs-dist';
import { createPdfWorkerPort } from '../src/application/pdf-worker-port';
jest.mock('pdfjs-dist', () => ({ getDocument: jest.fn(), PDFWorker: { create: jest.fn() }, AnnotationMode: { DISABLE: 0 } }));
jest.mock('../src/application/pdf-worker-port', () => ({ createPdfWorkerPort: jest.fn() }));
function setup(size = [600, 800]) {
  const terminate = jest.fn(); const workerDestroy = jest.fn(); const loadingDestroy = jest.fn(async () => undefined); const cleanup = jest.fn();
  const render = jest.fn(() => ({ promise: Promise.resolve(), cancel: jest.fn() }));
  const page = { getViewport: ({ scale }: { scale: number }) => ({ width: size[0]! * scale, height: size[1]! * scale }), render, cleanup };
  (createPdfWorkerPort as jest.Mock).mockReturnValue({ terminate });
  (PDFWorker.create as jest.Mock).mockReturnValue({ destroy: workerDestroy });
  (getDocument as jest.Mock).mockReturnValue({ promise: Promise.resolve({ numPages: 2, getPage: async () => page }), destroy: loadingDestroy });
  const canvas = document.createElement('canvas'); jest.spyOn(canvas, 'getContext').mockReturnValue({} as never);
  return { canvas, render, cleanup, terminate, workerDestroy, loadingDestroy };
}
test('PDF renderer disables interactive annotations/XFA and external assets with a same-origin explicit port', async () => {
  const result = setup(); const controller = new AbortController(); const session = await openPdfViewer(new Uint8Array([37,80,68,70]), controller.signal);
  expect(getDocument).toHaveBeenCalledWith(expect.objectContaining({ data: expect.any(Uint8Array), enableXfa: false, disableFontFace: true, useWasm: false, useWorkerFetch: false, useSystemFonts: false, stopAtErrors: true, maxImageSize: 4000000 }));
  const config = (getDocument as jest.Mock).mock.calls[0]![0]; expect(config.url).toBeUndefined(); expect(config.cMapUrl).toBeUndefined();
  await session.render(1, result.canvas); expect(result.render).toHaveBeenCalledWith(expect.objectContaining({ annotationMode: 0 }));
  expect(result.cleanup).toHaveBeenCalled(); controller.abort(); expect(result.terminate).toHaveBeenCalledTimes(1); expect(result.canvas.width).toBe(0);
});
test('PDF page canvas dimensions cannot exceed four million pixels', async () => {
  const result = setup([100000, 100000]); const session = await openPdfViewer(new Uint8Array([1]), new AbortController().signal);
  await session.render(1, result.canvas); expect(result.canvas.width * result.canvas.height).toBeLessThanOrEqual(4000000); session.destroy();
});
test('worker is terminated when PDF parser misses its deadline', async () => {
  jest.useFakeTimers(); const result = setup(); (getDocument as jest.Mock).mockReturnValue({ promise: new Promise(() => undefined), destroy: result.loadingDestroy });
  const loading = openPdfViewer(new Uint8Array([1]), new AbortController().signal); const rejected = expect(loading).rejects.toThrow(/時間上限/);
  await jest.advanceTimersByTimeAsync(20000); await rejected; expect(result.terminate).toHaveBeenCalledTimes(1); jest.useRealTimers();
});
test('malformed page dimensions fail before allocating canvas and release page data', async () => {
  const result = setup([Infinity, 1]); const session = await openPdfViewer(new Uint8Array([1]), new AbortController().signal);
  await expect(session.render(1, result.canvas)).rejects.toThrow(/寸法/); expect(result.render).not.toHaveBeenCalled(); expect(result.cleanup).toHaveBeenCalled(); session.destroy();
});
test('abort settles a stalled PDF parser immediately instead of leaving a pending display', async () => {
  jest.useFakeTimers(); const result = setup(); (getDocument as jest.Mock).mockReturnValue({ promise: new Promise(() => undefined), destroy: result.loadingDestroy });
  const controller = new AbortController(); const rejected = jest.fn(); const loading = openPdfViewer(new Uint8Array([1]), controller.signal).catch(rejected);
  await jest.advanceTimersByTimeAsync(0); controller.abort(); await jest.advanceTimersByTimeAsync(0);
  expect(rejected).toHaveBeenCalled(); expect(result.terminate).toHaveBeenCalledTimes(1); void loading; jest.useRealTimers();
});
test('synchronous PDF document loading failure does not leak the explicitly owned worker', async () => {
  const result = setup(); (getDocument as jest.Mock).mockImplementation(() => { throw new Error('malformed PDF'); });
  await expect(openPdfViewer(new Uint8Array([1]), new AbortController().signal)).rejects.toThrow('malformed PDF'); expect(result.terminate).toHaveBeenCalledTimes(1);
});
