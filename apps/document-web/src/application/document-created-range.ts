import { jstDateTimeLocalToUtc } from './schedule-time';

export const createdRangeFields = [
  { key: 'createdFrom', label: '作成日時の開始（含む）', name: '作成日時の開始' },
  { key: 'createdBefore', label: '作成日時の終了（含まない）', name: '作成日時の終了' },
] as const;
export type CreatedRangeKey = typeof createdRangeFields[number]['key'];
export type CreatedRange = Partial<Record<CreatedRangeKey, string>>;
type EndpointDraft = { intent: 'keep' } | { intent: 'clear' } | { intent: 'set'; local: string };
export type CreatedRangeDraft = {
  base: CreatedRange;
  createdFrom: EndpointDraft;
  createdBefore: EndpointDraft;
};

// This is a GUI transfer boundary, not an RFC3339 parser. The API owns date semantics.
export function createdRangeRouteError(range: Partial<Record<CreatedRangeKey, unknown>>): string | null {
  for (const { key, label } of createdRangeFields) {
    const value = range[key];
    if (value === undefined || value === '') continue;
    if (typeof value !== 'string') return `${label}は文字列で指定してください。URLの条件を確認してください。`;
    let bytes = 0;
    for (const character of value) {
      const point = character.codePointAt(0)!;
      if (point >= 0xd800 && point <= 0xdfff) return `${label}に不正なUnicode文字が含まれています。入力を確認してください。`;
      if (point <= 0x1f || (point >= 0x7f && point <= 0x9f)) return `${label}に制御文字を含めることはできません。`;
      bytes += point <= 0x7f ? 1 : point <= 0x7ff ? 2 : point <= 0xffff ? 3 : 4;
    }
    if (bytes > 128) return `${label}は128 UTF-8 bytes以下で入力してください。`;
  }
  return null;
}

export function createdRangeLocalValue(raw: string | undefined): string | null {
  if (!raw) return '';
  // Date only proposes a display value. Exact roundtrip through the existing helper authorizes it.
  const display = new Date(new Date(raw).getTime() + 9 * 60 * 60 * 1000);
  if (!Number.isFinite(display.getTime())) return null;
  const local = display.toISOString().slice(0, 16);
  return jstDateTimeLocalToUtc(local) === raw ? local : null;
}

export function initialCreatedRangeDraft(range: CreatedRange): CreatedRangeDraft {
  return { base: { createdFrom: range.createdFrom || undefined, createdBefore: range.createdBefore || undefined }, createdFrom: { intent: 'keep' }, createdBefore: { intent: 'keep' } };
}

export function currentCreatedRangeDraft(range: CreatedRange, draft: CreatedRangeDraft): CreatedRangeDraft {
  return draft.base.createdFrom === (range.createdFrom || undefined) && draft.base.createdBefore === (range.createdBefore || undefined)
    ? draft : initialCreatedRangeDraft(range);
}

export function resolveCreatedRangeDraft(draft: CreatedRangeDraft): { range: CreatedRange; error: null } | { error: string } {
  const range: CreatedRange = {};
  for (const { key, label } of createdRangeFields) {
    const endpoint = draft[key];
    if (endpoint.intent === 'keep') range[key] = draft.base[key];
    else if (endpoint.intent === 'clear') range[key] = undefined;
    else {
      const instant = jstDateTimeLocalToUtc(endpoint.local);
      if (!instant) return { error: `${label}をJSTのカレンダーと時刻で指定してください。` };
      range[key] = instant;
    }
  }
  return { range, error: null };
}

export const documentListUrlReason = '一覧のURLが81920文字を超えています。URLの条件を短くして再度お試しください。';
export function documentListUrlError(url: string): string | null {
  return url.length > 81920 ? documentListUrlReason : null;
}
