import { TextEncoder, TextDecoder } from 'node:util';
Object.assign(globalThis, { TextEncoder, TextDecoder });
import { viewerFileProblem, decodeViewerText, boundedPdfViewport, VIEWER_MAX_BYTES } from '../src/application/document-original-viewer';

test('viewer rejects unsupported and oversized originals before fetching', () => {
  expect(viewerFileProblem({ mediaType: 'text/html', sizeBytes: 1 })).toMatch(/ダウンロード/);
  expect(viewerFileProblem({ mediaType: 'text/plain', sizeBytes: VIEWER_MAX_BYTES + 1 })).toMatch(/10 MiB/);
  expect(viewerFileProblem({ mediaType: 'application/pdf', sizeBytes: VIEWER_MAX_BYTES })).toBeUndefined();
});
test('text preserves DOCTYPE as text and rejects malformed UTF-8', () => {
  expect(decodeViewerText(new TextEncoder().encode('<!DOCTYPE html><script>x</script>'))).toBe('<!DOCTYPE html><script>x</script>');
  expect(() => decodeViewerText(new Uint8Array([0xff]))).toThrow(/UTF-8/);
});
test('PDF viewport has finite dimensions and bounded total pixel allocation', () => {
  expect(() => boundedPdfViewport(Infinity, 2)).toThrow();
  expect(() => boundedPdfViewport(0, 2)).toThrow();
  const size = boundedPdfViewport(100000, 100000);
  expect(size.width * size.height).toBeLessThanOrEqual(4000000);
  expect(size.width).toBeLessThanOrEqual(4096);
});
