import { prepareWorkingVersion, type EditManifest } from '../src/application/document-working-version';
import { documentApi } from '../src/application/document-workspace';
jest.mock('../src/application/document-workspace', () => ({ documentApi: { downloadVersionFile: jest.fn(), prepareVersionUpload: jest.fn() } }));
const manifest: EditManifest = { documentId: 'document', sourceVersionId: 'source', documentRevision: 3, purpose: 'authoring', title: 'Title', items: [
  { contentItemId: 'a', logicalPath: 'one', ordinal: 2, representations: [
    { representationId: 'a1', role: 'authoritative', fileId: 'a-file', originalFilename: 'original-A.txt', mediaType: 'text/plain', sizeBytes: 3 },
    { representationId: 'a2', role: 'rendition', fileId: 'a-r-file', originalFilename: 'rendition-A.txt', mediaType: 'text/plain', sizeBytes: 3 },
  ] },
  { contentItemId: 'b', logicalPath: 'two', ordinal: 7, representations: [
    { representationId: 'b1', role: 'authoritative', fileId: 'b-file', originalFilename: 'original-B.txt', mediaType: 'text/plain', sizeBytes: 3 },
    { representationId: 'b2', role: 'rendition', fileId: 'b-r-file', originalFilename: 'rendition-B.txt', mediaType: 'text/plain', sizeBytes: 3 },
  ] },
] };
function prepare(source = manifest, replacements = new Map<string, File>(), signal = new AbortController().signal) {
  return prepareWorkingVersion({ mode: 'update', manifest: source, title: 'Title', replacements, signal });
}
beforeEach(() => {
  (documentApi.downloadVersionFile as jest.Mock).mockReset().mockResolvedValue(new Blob(['old']));
  (documentApi.prepareVersionUpload as jest.Mock).mockReset().mockImplementation(() => ({ body: new Blob(['wire']), contentType: 'multipart/form-data; boundary=test' }));
});
test('exact original names and ordered anchors survive an unchanged initial working save', async () => {
  const intent = await prepare();
  expect(intent.body.items.map(item => [item.logicalPath, item.ordinal, item.fileId, item.originalFilename])).toEqual([
    ['one', 2, 'a-file', 'original-A.txt'], ['two', 7, 'b-file', 'original-B.txt'],
  ]);
  expect(intent.files.size).toBe(4); expect(intent.body.targetVersionId).toBe('source');
  expect(intent.body.items[0]!.renditions![0]!.originalFilename).toBe('rendition-A.txt');
});
test('replacement removes only its own renditions and gives only the replaced original a fresh FileID', async () => {
  const file = new File(['new'], 'new.txt', { type: 'text/plain' });
  const intent = await prepare(manifest, new Map([['a', file]]));
  expect(intent.body.items[0]).toMatchObject({ logicalPath: 'one', ordinal: 2, originalFilename: 'new.txt', renditions: [] });
  expect(intent.body.items[0]!.fileId).not.toBe('a-file'); expect(intent.body.items[1]!.fileId).toBe('b-file');
  expect(intent.body.items[1]!.renditions![0]!.fileId).toBe('b-r-file'); expect(intent.files.size).toBe(3);
  expect(intent.files.get(intent.body.items[0]!.partId)).toBe(file);
});
test('failed audited download prevents preparation of a writable payload', async () => {
  (documentApi.downloadVersionFile as jest.Mock).mockRejectedValueOnce(new Error('audit unknown'));
  await expect(prepare()).rejects.toThrow('audit unknown');
  expect(documentApi.prepareVersionUpload).not.toHaveBeenCalled();
});
test('cancellation after late download stops remaining reads and produces no writable payload', async () => {
  const controller = new AbortController();
  (documentApi.downloadVersionFile as jest.Mock).mockImplementationOnce(async () => { controller.abort(); return new Blob(['old']); });
  await expect(prepare(manifest, new Map(), controller.signal)).rejects.toThrow();
  expect(documentApi.downloadVersionFile).toHaveBeenCalledTimes(1); expect(documentApi.prepareVersionUpload).not.toHaveBeenCalled();
});
test('shared kept FileID fails closed before any audited download', async () => {
  const source = JSON.parse(JSON.stringify(manifest)) as EditManifest; source.items[1]!.representations[0]!.fileId = 'a-file';
  await expect(prepare(source)).rejects.toThrow(/FileID/); expect(documentApi.downloadVersionFile).not.toHaveBeenCalled();
});
test('all authoritative and rendition parts count toward the 63 binary limit', async () => {
  const source = JSON.parse(JSON.stringify(manifest)) as EditManifest; source.items = Array.from({ length: 32 }, (_, n) => ({ ...source.items[0]!, contentItemId: `item-${n}`, logicalPath: `path-${n}`, ordinal: n,
    representations: source.items[0]!.representations.map((part, i) => ({ ...part, fileId: `f-${n}-${i}`, representationId: `r-${n}-${i}` })) }));
  await expect(prepare(source)).rejects.toThrow(/63/); expect(documentApi.downloadVersionFile).not.toHaveBeenCalled();
});
test('advertised oversized unchanged original blocks before downloading', async () => {
  const source = JSON.parse(JSON.stringify(manifest)) as EditManifest; source.items[1]!.representations[0]!.sizeBytes = 256 * 1024 * 1024 + 1;
  await expect(prepare(source)).rejects.toThrow(/256/); expect(documentApi.downloadVersionFile).not.toHaveBeenCalled();
});
test('kept-file byte length mismatch fails closed', async () => {
  (documentApi.downloadVersionFile as jest.Mock).mockResolvedValue(new Blob(['wrong size']));
  await expect(prepare()).rejects.toThrow(/サイズ/); expect(documentApi.prepareVersionUpload).not.toHaveBeenCalled();
});
test('a format-changing replacement is rejected locally without guessed conversion', async () => {
  await expect(prepare(manifest, new Map([['a', new File(['pdf'], 'new.pdf', { type: 'application/pdf' })]]))).rejects.toThrow(/形式/);
  expect(documentApi.downloadVersionFile).not.toHaveBeenCalled();
});
