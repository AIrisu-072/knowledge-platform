import { test, expect } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import { BinaryTransportBridge, getDocument, getSession } from '@knowledge-platform/document-api-client';
import { hash, options, persistedSnapshot, runtime, type PersistedState } from './support';

test('both restarted composition roots retain document/revision/operation IDs and storage hashes', async ({ page, request }) => {
  const context = await runtime();
  const state = JSON.parse(await readFile(context.statePath, 'utf8')) as PersistedState;
  expect(state.documents.map(item => item.key).sort()).toEqual(['c3-consistency', 'c3-diff-recovery', 'c3-dsi-recovery', 'gui-initial', 'pdf', 'regulation']);
  expect((await getSession(options(context.human))).data.principal.principalId).toBe('poc-human');
  expect((await getSession(options(context.agent))).data.principal.principalId).toBe('poc-agent');
  for (const { snapshot } of state.documents) {
    expect(await persistedSnapshot(context.human, snapshot.documentId)).toEqual(snapshot);
    expect(await persistedSnapshot(context.agent, snapshot.documentId)).toEqual(snapshot);
    await page.goto(`/documents/${snapshot.documentId}?view=published&tab=overview`);
    await expect(page.getByRole('heading', { name: snapshot.title, level: 1 })).toBeVisible();
    await expect(page.getByRole('complementary', { name: '原本と版' })).toBeVisible();
  }
  const drain = JSON.parse(await readFile(context.drainFixturePath, 'utf8')) as { documentId: string; versionId: string; contentItemId: string; representationId: string; sizeBytes: number; sha256: string };
  const drained = (await getDocument({ ...options(context.human), path: { documentId: drain.documentId }, query: { view: 'authoring' } })).data;
  expect(drained.metadata).toMatchObject({ extensions: { runtimeDrainObservation: 'synthetic-in-flight-completed' } });
  const original = await new BinaryTransportBridge({ baseUrl: context.human }).downloadVersionFileBlob({ ...drain, purpose: 'authoring' });
  expect(original.size).toBe(drain.sizeBytes); expect(hash(new Uint8Array(await original.arrayBuffer()))).toBe(drain.sha256);
  const deniedId = context.manifest.documents.humanOnly!.create!.result!.documentId;
  const denied = await request.get(`${context.agent}/v1/documents/${deniedId}?view=published`);
  expect(denied.status()).toBe(404); expect((await denied.json()).code).toBe('DOCUMENT_NOT_FOUND');
  await test.info().attach('restart-persistence.json', { body: Buffer.from(JSON.stringify(state, null, 2)), contentType: 'application/json' });
});
