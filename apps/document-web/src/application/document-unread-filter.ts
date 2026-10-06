export const unreadFilterReasons = {
  type: '未読のみはtrueまたはfalseで指定してください。URLの条件を確認してください。',
  scope: '未読のみは公開一覧でのみ指定できます。URLの条件を確認してください。',
} as const;

// Run before AJV coercion/defaults, using the standard router's decoded raw values.
export function unreadFilterRouteError(search: { unreadOnly?: unknown; view?: unknown }): string | null {
  if (search.unreadOnly === undefined) return null;
  if (typeof search.unreadOnly !== 'boolean') return unreadFilterReasons.type;
  if (search.view !== undefined && search.view !== 'published') return unreadFilterReasons.scope;
  return null;
}
