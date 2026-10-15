import { serverEnvironment } from '../document-poc-runtime/harness.mjs';

// Closed allowlist of fixed synthetic profiles; each process serves exactly one.
export const ORGANIZATION_PROFILES = ['sales-01', 'office-01', 'review-01', 'approver-01', 'multi-role-01', 'delegate-01'];
export function organizationEnvironment(inputs) {
  if (!ORGANIZATION_PROFILES.includes(inputs.profile)) throw Error('Unknown synthetic Organization profile');
  const env = serverEnvironment({ ...inputs, profile: 'poc-human' });
  delete env.KP_IDENTITY_PROFILE;
  delete env.WORK_POC_TEST_DATABASE_URL;
  return { ...env, KP_RUNTIME_MODE: 'organization-synthetic', KP_ORGANIZATION_PROFILE: inputs.profile };
}
