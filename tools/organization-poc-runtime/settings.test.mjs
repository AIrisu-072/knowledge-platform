import assert from 'node:assert/strict';
import { test } from 'node:test';
import { organizationEnvironment } from './settings.mjs';
const inputs = { inherited: { PATH: '/tools', KP_RUNTIME_MODE: 'production', KP_ORGANIZATION_PROFILE: 'unknown', TEST_DATABASE_URL: 'external', WORK_POC_TEST_DATABASE_URL: 'external', KP_POC_ALLOW_NON_LOOPBACK: 'true' }, database: 'postgres://synthetic@127.0.0.1:1234/kp_document_poc', profile: 'sales-01', port: 8090, storage: '/owned/storage', dsi: '/owned/dsi', diff: '/owned/diff', web: '/owned/web', pdfium: '/owned/pdfium' };
test('both fixed profiles use same-origin GUI with only owned runtime values', () => {
  for (const profile of ['sales-01', 'office-01']) {
    const env = organizationEnvironment({ ...inputs, profile });
    assert.equal(env.KP_RUNTIME_MODE, 'organization-synthetic');
    assert.equal(env.KP_ORGANIZATION_PROFILE, profile);
    assert.equal(env.KP_BIND, '127.0.0.1:8090');
    assert.equal(env.KP_POC_ALLOW_NON_LOOPBACK, 'false');
    assert.equal(env.KP_WEB_DIST, '/owned/web');
    assert.equal(env.PATH, '/tools');
    assert.equal(env.TEST_DATABASE_URL, undefined);
    assert.equal(env.WORK_POC_TEST_DATABASE_URL, undefined);
    assert.equal(env.KP_IDENTITY_PROFILE, undefined);
  }
});
test('unknown runtime profiles are rejected', () => {
  assert.throws(() => organizationEnvironment({ ...inputs, profile: 'poc-human' }));
});
test('Organization composition confirms drain only after the Work pool closes', async () => {
  const { readFile } = await import('node:fs/promises');
  const source = await readFile(new URL('../../crates/organization-server/src/main.rs', import.meta.url), 'utf8');
  assert.match(source, /pool\.close\(\)\.await;\s*if action == "serve" \{\s*eprintln!\("organization-server: graceful drain complete"\);/);
});
