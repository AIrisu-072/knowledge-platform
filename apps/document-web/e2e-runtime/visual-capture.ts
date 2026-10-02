import type { Page } from '@playwright/test';
import { createRequire } from 'node:module';
import { dirname } from 'node:path';
import { readFile } from 'node:fs/promises';
import type { RuntimeContext } from './support';
const require = createRequire(import.meta.url);
const { captureVisualCheckpoint } = require('../../../tools/document-poc-runtime/visual-evidence.mjs') as {
  captureVisualCheckpoint(input: { runDirectory: string; context: RuntimeContext; phase: string | undefined; page: Page; name: string }): Promise<void>;
};
export async function visualCheckpoint(page: Page, name: string): Promise<void> {
  const path = process.env.KP_POC_RUNTIME_CONTEXT;
  if (!path) throw Error('Owned runtime context required');
  const context = JSON.parse(await readFile(path, 'utf8')) as RuntimeContext;
  if (!context.visualCapture) return;
  // Called only after the named assertions succeed. Never invoked by afterEach,
  // reporters or failure handlers; default Playwright attachments are excluded.
  await captureVisualCheckpoint({ runDirectory: dirname(path), context, phase: process.env.KP_POC_RUNTIME_PHASE, page, name });
}
