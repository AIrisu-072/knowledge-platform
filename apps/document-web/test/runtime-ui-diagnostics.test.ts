import type { Page } from '@playwright/test';
import { startDiagnostics, captureUiDiagnostics, finishDiagnostics } from '../e2e-runtime/startup-diagnostics';

test('a silent renderer cannot prevent bounded unavailable diagnostics', async () => {
  let silent = false;
  const page = { on() {}, async addInitScript() {}, evaluate: async () => {
    if (silent) return new Promise(() => {});
    return { rootChildren: 1, sharedFolderButtonCount: 1 };
  } } as unknown as Page;
  await startDiagnostics(page);
  await captureUiDiagnostics(page, 'before-folder-click');
  silent = true;
  const record = await finishDiagnostics(page);
  expect(record?.uiSnapshots[0]).toMatchObject({ stage: 'before-folder-click', availability: 'available', sharedFolderButtonCount: 1 });
  expect(record?.uiSnapshots[1]).toEqual({ stage: 'test-end', availability: 'unavailable' });
  expect(record?.domObservation).toBe('unavailable');
}, 4000);

test('pre-action DOM evidence survives an unavailable end-of-test observation', async () => {
  const events = new Map<string, (value: any) => void>();
  let closed = false;
  const page = {
    on: (name: string, handler: (value: any) => void) => events.set(name, handler),
    addInitScript: async () => {},
    evaluate: async (fn: () => unknown) => { if (closed) throw new Error('closed'); return fn(); },
  } as unknown as Page;
  const root = document.createElement('div');
  root.id = 'root';
  root.innerHTML = '<section aria-label="フォルダー"><button>PoC Shared</button></section><table></table>';
  document.body.append(root);
  const button = root.querySelector('button')!;
  jest.spyOn(button, 'getBoundingClientRect').mockReturnValue({ x: 10, y: 10, left: 10, top: 10,
    right: 100, bottom: 35, width: 90, height: 25, toJSON() {} });
  const original = Object.getOwnPropertyDescriptor(document, 'elementFromPoint');
  Object.defineProperty(document, 'elementFromPoint', { configurable: true, value: () => button });
  try {
    await startDiagnostics(page);
    events.get('response')!({ request: () => ({ resourceType: () => 'fetch' }), status: () => 200,
      url: () => 'http://synthetic.invalid/v1/folders/private-identifier/children?secret=do-not-record' });
    await captureUiDiagnostics(page, 'before-folder-click');
    closed = true;
    const record = await finishDiagnostics(page);
    expect(record?.domObservation).toBe('unavailable');
    expect(record?.apiEvents).toEqual([{ route: 'folder-children', status: 200 }]);
    expect(record?.uiSnapshots[0]).toMatchObject({ stage: 'before-folder-click', availability: 'available',
      sharedFolderButtonCount: 1, inViewportCount: 1, receivesPointerCount: 1, disabledCount: 0 });
    expect(record?.uiSnapshots[1]).toEqual({ stage: 'test-end', availability: 'unavailable' });
    expect(JSON.stringify(record)).not.toMatch(/private-identifier|do-not-record|synthetic.invalid/);
  } finally {
    root.remove();
    if (original) Object.defineProperty(document, 'elementFromPoint', original);
    else delete (document as unknown as { elementFromPoint?: unknown }).elementFromPoint;
  }
});
