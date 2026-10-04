import { serverEnvironment } from '../document-poc-runtime/harness.mjs';

export function organizationEnvironment(inputs) {
  if (!['sales-01', 'office-01'].includes(inputs.profile)) throw Error('Unknown synthetic Organization profile');
  const env = serverEnvironment({ ...inputs, profile: 'poc-human' });
  delete env.KP_IDENTITY_PROFILE;
  delete env.WORK_POC_TEST_DATABASE_URL;
  return { ...env, KP_RUNTIME_MODE: 'organization-synthetic', KP_ORGANIZATION_PROFILE: inputs.profile };
}
