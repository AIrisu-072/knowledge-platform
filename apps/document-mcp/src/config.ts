export interface McpConfig { readonly apiBaseUrl: string }
export function loadConfig(env: NodeJS.ProcessEnv): McpConfig {
  const url = new URL(env.KP_DOCUMENT_API_BASE_URL ?? 'http://127.0.0.1:8081/');
  if (!['http:', 'https:'].includes(url.protocol) || url.username || url.password || url.hash) {
    throw new Error('Invalid Document API configuration');
  }
  return Object.freeze({ apiBaseUrl: url.href });
}
