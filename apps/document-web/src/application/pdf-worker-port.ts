export function createPdfWorkerPort(): Worker {
  return new Worker(new URL('pdfjs-dist/build/pdf.worker.mjs', import.meta.url), { type: 'module' });
}
