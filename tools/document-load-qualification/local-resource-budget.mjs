const GiB = 1024 ** 3;
export const LOCAL_RESOURCE_ENV_KEYS = Object.freeze([
  'KP_DOCUMENT_LOAD_LOCAL_MAX_RSS_BYTES',
  'KP_DOCUMENT_LOAD_LOCAL_MIN_AVAILABLE_MEMORY_BYTES',
]);

/** Only the explicitly approved local80h4 profile is selectable; bytes are binary GiB. */
export function validateLocalResourceBudget(budget) {
  if (budget === undefined) return undefined;
  if (budget === null || typeof budget !== 'object' || Array.isArray(budget)
    || Object.keys(budget).length !== 2
    || budget.maxRssBytes !== 4 * GiB || budget.minAvailableMemoryBytes !== 4 * GiB) {
    throw Error('Invalid local resource budget: approved RSS and available-memory reserve are each 4294967296 bytes (4 GiB)');
  }
  return Object.freeze({maxRssBytes:budget.maxRssBytes,minAvailableMemoryBytes:budget.minAvailableMemoryBytes});
}

/** Omission preserves legacy defaults. Partial, coercible, or unapproved input is rejected. */
export function parseLocalResourceBudget(env) {
  const values = LOCAL_RESOURCE_ENV_KEYS.map(key => env[key]);
  if (values.every(value => value === undefined)) return undefined;
  if (values.some(value => typeof value !== 'string' || !/^[1-9][0-9]*$/.test(value)
    || !Number.isSafeInteger(Number(value)))) throw Error('Invalid local resource budget: explicit integer byte pair is required');
  return validateLocalResourceBudget({maxRssBytes:Number(values[0]),minAvailableMemoryBytes:Number(values[1])});
}
