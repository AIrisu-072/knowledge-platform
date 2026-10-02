import type { Page } from '@playwright/test';
import { createRequire } from 'node:module';
import { dirname } from 'node:path';
import { readFile } from 'node:fs/promises';
import type { RuntimeContext } from './support';
const require = createRequire(import.meta.url);
const { assertVisualReadiness, captureVisualCheckpoint } = require('../../../tools/document-poc-runtime/visual-evidence.mjs') as {
  assertVisualReadiness(input: { page: Page; name: string; humanOrigin: string }): Promise<void>;
  captureVisualCheckpoint(input: { runDirectory: string; context: RuntimeContext; phase: string | undefined; page: Page; name: string }): Promise<void>;
};
export async function visualCheckpoint(page: Page, name: string): Promise<void> {
  const path = process.env.KP_POC_RUNTIME_CONTEXT;
  if (!path) throw Error('Owned runtime context required');
  const context = JSON.parse(await readFile(path, 'utf8')) as RuntimeContext;
  // Normal runs qualify settled state and framing bounds before a later capture.
  // This observes the current page without changing focus, scroll or product CSS.
  await assertVisualReadiness({ page, name, humanOrigin: context.human });
  if (!context.visualCapture) return;
  // Called only after the named assertions succeed. Never invoked by afterEach,
  // reporters or failure handlers; default Playwright attachments are excluded.
  await captureVisualCheckpoint({ runDirectory: dirname(path), context, phase: process.env.KP_POC_RUNTIME_PHASE, page, name });
}
