import { test } from '@playwright/test';
import { runtime } from './support';
import { assertTimestampBrowserLayout } from './timestamp-layout';

// This synthetic display-only proof adds no images, traces, video or attachments.
test.use({ trace: 'off', screenshot: 'off', video: 'off' });
test('browser-only timestamp layout covers long IANA and both DST folds at1280/1440', async ({ browser }) => {
  const context = await runtime();
  await assertTimestampBrowserLayout(browser, context);
  test.info().annotations.push({ type: 'runtime-timestamp-layout', description: 'long-iana-both-folds-1280-1440' });
});
