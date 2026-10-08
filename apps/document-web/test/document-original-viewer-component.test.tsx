import { TextEncoder, TextDecoder } from 'node:util';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { DocumentOriginalViewer } from '../src/components/document/DocumentOriginalViewer';
import { openPdfViewer } from '../src/application/pdf-renderer';
import { documentApi } from '../src/application/document-workspace';
Object.assign(globalThis, { TextEncoder, TextDecoder });
jest.mock('../src/application/document-workspace', () => ({ documentApi: { downloadVersionFile: jest.fn() } }));
jest.mock('../src/application/pdf-renderer', () => ({ openPdfViewer: jest.fn() }), { virtual: true });
const file = { contentItemId: 'item', representationId: 'rep', displayName: '原本.txt', mediaType: 'text/plain', sizeBytes: 40, role: 'authoritative', logicalPath: '原本.txt', ordinal: 0 };
const bytes = new TextEncoder().encode('<!DOCTYPE html><script>alert(1)</script>');
function setup(value = file) {
  const client = new QueryClient(); const manifest = { items: [value] }; const document = { displayVersion: { versionId: 'version' } };
  client.setQueryData(['document', 'doc', 'published'], document);
  client.setQueryData(['document-version-files', 'doc', 'version', 'published'], manifest);
  const api = documentApi.downloadVersionFile as jest.Mock; api.mockReset();
  const responseBytes = value.mediaType === 'application/pdf' ? new Uint8Array([37, 80, 68, 70, 45, 49, 46, 55]) : bytes;
  api.mockResolvedValue({ size: responseBytes.byteLength, type: value.mediaType, arrayBuffer: async () => responseBytes.buffer });
  const view = render(<QueryClientProvider client={client}><DocumentOriginalViewer documentId="doc" versionId="version" purpose="published" file={value} /></QueryClientProvider>);
  return { ...view, client, api };
}
test('explicit text display renders DOCTYPE as inert text using a bounded audited download', async () => {
  const { api } = setup(); expect(api).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button', { name: '原本.txtを表示' }));
  const content = await screen.findByTestId('original-viewer-text');
  expect(content.textContent).toContain('<script>alert(1)</script>'); expect(content.querySelector('script')).toBeNull();
  expect(api).toHaveBeenCalledWith(expect.objectContaining({ purpose: 'published', versionId: 'version' }), expect.objectContaining({ maxBytes: 10485760, signal: expect.any(AbortSignal) }));
});
test('query invalidation clears already displayed plaintext and does not refetch automatically', async () => {
  const { client, api } = setup(); fireEvent.click(screen.getByRole('button', { name: '原本.txtを表示' })); await screen.findByTestId('original-viewer-text');
  act(() => { void client.invalidateQueries({ queryKey: ['document', 'doc'] }); });
  await waitFor(() => expect(screen.queryByTestId('original-viewer-text')).toBeNull()); expect(api).toHaveBeenCalledTimes(1);
});
test('late download after close does not display bytes', async () => {
  const { api } = setup(); let resolve!: (value: unknown) => void; api.mockImplementation(() => new Promise(done => { resolve = done; }));
  fireEvent.click(screen.getByRole('button', { name: '原本.txtを表示' }));
  fireEvent.click(screen.getByRole('button', { name: '原本表示を閉じる' }));
  await act(async () => resolve({ size: bytes.byteLength, type: 'text/plain', arrayBuffer: async () => bytes.buffer }));
  expect(screen.queryByTestId('original-viewer-text')).toBeNull();
});
test('a stale response from a closed display cannot cancel a later reopened display', async () => {
  const { api } = setup(); let old!: (value: unknown) => void;
  api.mockImplementationOnce(() => new Promise(done => { old = done; }));
  fireEvent.click(screen.getByRole('button', { name: '原本.txtを表示' })); fireEvent.click(screen.getByRole('button', { name: '原本表示を閉じる' }));
  fireEvent.click(screen.getByRole('button', { name: '原本.txtを表示' })); await screen.findByTestId('original-viewer-text');
  await act(async () => old({ size: bytes.byteLength, type: 'text/plain', arrayBuffer: async () => bytes.buffer }));
  expect(screen.getByTestId('original-viewer-text')).toHaveTextContent('<!DOCTYPE html>');
});
test('CSS hiding disposes displayed original and returning visible does not fetch it again', async () => {
  const { container, api } = setup(); fireEvent.click(screen.getByRole('button', { name: '原本.txtを表示' })); await screen.findByTestId('original-viewer-text');
  await act(async () => { container.style.display = 'none'; });
  expect(screen.queryByTestId('original-viewer-text')).toBeNull(); await act(async () => { container.style.display = ''; }); expect(api).toHaveBeenCalledTimes(1);
});
test('lost document authorization removes plaintext immediately', async () => {
  const { client } = setup(); fireEvent.click(screen.getByRole('button', { name: '原本.txtを表示' })); await screen.findByTestId('original-viewer-text');
  act(() => { client.removeQueries({ queryKey: ['document', 'doc', 'published'], exact: true }); }); expect(screen.queryByTestId('original-viewer-text')).toBeNull();
});
test('only one original can stay open in a query client even across separate file rows', async () => {
  const { client, api } = setup(); const next = { ...file, contentItemId: 'second', displayName: '別原本.txt' };
  client.setQueryData(['document-version-files', 'doc', 'version', 'published'], { items: [file, next] });
  render(<QueryClientProvider client={client}><DocumentOriginalViewer documentId="doc" versionId="version" purpose="published" file={next} /></QueryClientProvider>);
  fireEvent.click(screen.getByRole('button', { name: '原本.txtを表示' })); await screen.findByTestId('original-viewer-text');
  fireEvent.click(screen.getByRole('button', { name: '別原本.txtを表示' })); await waitFor(() => expect(api).toHaveBeenCalledTimes(2));
  await waitFor(() => expect(screen.getAllByTestId('original-viewer-text')).toHaveLength(1));
  expect(screen.getByRole('button', { name: '原本.txtを表示' })).toBeEnabled();
});
test('same-tick PDF page requests cannot start simultaneous page decoding', async () => {
  const drawing = jest.fn().mockResolvedValueOnce(undefined).mockImplementation(() => new Promise(() => undefined));
  (openPdfViewer as jest.Mock).mockResolvedValue({ pages: 2, render: drawing, destroy: jest.fn() });
  setup({ ...file, mediaType: 'application/pdf' }); fireEvent.click(screen.getByRole('button', { name: '原本.txtを表示' }));
  const next = await screen.findByRole('button', { name: '次のページ' }); await waitFor(() => expect(next).toBeEnabled());
  act(() => { fireEvent.click(next); fireEvent.click(next); }); expect(drawing).toHaveBeenCalledTimes(2);
});
test('a download fallback cannot save a delayed original after document authorization is lost', async () => {
  const { client, api } = setup(); let resolve!: (value: unknown) => void; api.mockImplementation(() => new Promise(done => { resolve = done; }));
  const create = jest.fn(); Object.defineProperty(URL, 'createObjectURL', { configurable: true, value: create });
  fireEvent.click(screen.getByRole('button', { name: '原本.txtをダウンロード' }));
  act(() => { client.removeQueries({ queryKey: ['document', 'doc'], exact: false }); });
  await act(async () => resolve({ size: bytes.byteLength, type: 'text/plain', arrayBuffer: async () => bytes.buffer }));
  expect(create).not.toHaveBeenCalled();
});
test('fallback download does not erase a failed PDF rendering state or restore dead page controls', async () => {
  const drawing = jest.fn().mockResolvedValueOnce(undefined).mockRejectedValueOnce(new Error('PDF描画失敗'));
  (openPdfViewer as jest.Mock).mockResolvedValue({ pages: 2, render: drawing, destroy: jest.fn() });
  const { api } = setup({ ...file, mediaType: 'application/pdf' }); fireEvent.click(screen.getByRole('button', { name: '原本.txtを表示' }));
  const next = await screen.findByRole('button', { name: '次のページ' }); await waitFor(() => expect(next).toBeEnabled()); fireEvent.click(next);
  expect(await screen.findByRole('alert')).toHaveTextContent('PDF描画失敗'); api.mockImplementation(() => new Promise(() => undefined));
  fireEvent.click(screen.getByRole('button', { name: '原本.txtをダウンロード' }));
  expect(screen.getByRole('alert')).toHaveTextContent('PDF描画失敗'); expect(screen.queryByRole('button', { name: '次のページ' })).toBeNull();
});
test('an authorized past-version target uses history purpose without requiring current version equality', async () => {
  const { client, api } = setup(); const past = { versionId: 'past', capabilities: { download: { status: 'available' } } }; const originals = { items: [file] };
  client.setQueryData(['document-version-files', 'doc', 'past', 'history'], originals);
  render(<QueryClientProvider client={client}><DocumentOriginalViewer documentId="doc" versionId="past" purpose="history" file={file} historyRead={() => ({ version: past as never, files: originals })} /></QueryClientProvider>);
  fireEvent.click(screen.getAllByRole('button', { name: '原本.txtを表示' })[1]!);
  await screen.findByTestId('original-viewer-text'); expect(api).toHaveBeenCalledWith(expect.objectContaining({ versionId: 'past', purpose: 'history' }), expect.objectContaining({ maxBytes: 10485760 }));
});
test('binary fallback denial disposes plaintext even when the document metadata read remains authorized', async () => {
  const { api, client } = setup(); fireEvent.click(screen.getByRole('button', { name: '原本.txtを表示' })); await screen.findByTestId('original-viewer-text');
  api.mockRejectedValueOnce({ type: 'about:blank', title: 'denied', status: 403, code: 'FORBIDDEN', traceId: 'synthetic', retryable: false });
  fireEvent.click(screen.getByRole('button', { name: '原本.txtをダウンロード' }));
  await waitFor(() => expect(screen.queryByTestId('original-viewer-text')).toBeNull());
  expect(client.getQueryData(['document', 'doc', 'published'])).toBeUndefined();
  fireEvent.click(screen.getByRole('button', { name: '原本.txtを表示' })); expect(api).toHaveBeenCalledTimes(2);
});
test('native data-only octet-stream PDF is rendered only with the selected PDF manifest and PDF magic', async () => {
  const renderPdf = jest.fn(async () => undefined); (openPdfViewer as jest.Mock).mockResolvedValue({ pages: 1, render: renderPdf, destroy: jest.fn() });
  const { api } = setup({ ...file, mediaType: 'application/pdf' });
  api.mockResolvedValueOnce({ size: 8, type: 'application/octet-stream', arrayBuffer: async () => new Uint8Array([37,80,68,70,45,49,46,55]).buffer });
  fireEvent.click(screen.getByRole('button', { name: '原本.txtを表示' })); await waitFor(() => expect(renderPdf).toHaveBeenCalledTimes(1));
});
test('a data-only PDF response with HTML bytes is refused before invoking the PDF parser', async () => {
  const { api } = setup({ ...file, mediaType: 'application/pdf' }); const parser = openPdfViewer as jest.Mock; parser.mockClear();
  api.mockResolvedValueOnce({ size: bytes.byteLength, type: 'application/octet-stream', arrayBuffer: async () => bytes.buffer });
  fireEvent.click(screen.getByRole('button', { name: '原本.txtを表示' })); expect(await screen.findByRole('alert')).toHaveTextContent(/形式|PDF/); expect(parser).not.toHaveBeenCalled();
});
test('an old binary denial cannot erase a newer document snapshot or its new display', async () => {
  const { api, client } = setup(); let rejectOld!: (error: unknown) => void;
  api.mockImplementationOnce(() => new Promise((_resolve, reject) => { rejectOld = reject; }));
  fireEvent.click(screen.getByRole('button', { name: '原本.txtを表示' }));
  const old = client.getQueryData(['document', 'doc', 'published']) as object;
  act(() => client.setQueryData(['document', 'doc', 'published'], { ...old, revision: 2 }));
  fireEvent.click(screen.getByRole('button', { name: '原本表示を閉じる' }));
  fireEvent.click(screen.getByRole('button', { name: '原本.txtを表示' })); await screen.findByTestId('original-viewer-text');
  await act(async () => rejectOld({ status: 403 }));
  expect(screen.getByTestId('original-viewer-text')).toBeInTheDocument(); expect(client.getQueryData(['document', 'doc', 'published'])).toBeDefined();
});
