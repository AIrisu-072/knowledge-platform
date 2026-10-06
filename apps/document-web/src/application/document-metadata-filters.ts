// List filters are exact-match text, unlike metadata mutation values/reasons.
export const metadataFilterFields = [
  { key: 'documentType', label: '文書種別' },
  { key: 'owningDepartment', label: '所管部署' },
  { key: 'category', label: 'カテゴリ' },
] as const;
export type MetadataFilterKey = typeof metadataFilterFields[number]['key'];
export type MetadataFilters = Partial<Record<MetadataFilterKey, string>>;

export function metadataFilterRouteError(filters: Partial<Record<MetadataFilterKey, unknown>>): string | null {
  for (const { key, label } of metadataFilterFields) {
    const value = filters[key];
    if (value === undefined) continue;
    if (typeof value !== 'string') return `${label}は文字列で指定してください。URLの条件を確認してください。`;
    for (const character of value) {
      const point = character.codePointAt(0)!;
      if (point >= 0xd800 && point <= 0xdfff) return `${label}に不正なUnicode文字が含まれています。入力を確認してください。`;
    }
  }
  return null;
}

export function metadataFilterValidation(filters: MetadataFilters): string | null {
  const unicodeError = metadataFilterRouteError(filters);
  if (unicodeError) return unicodeError;
  for (const { key, label } of metadataFilterFields) {
    const value = filters[key];
    // Only the empty string is unspecified. Never trim, normalize or truncate.
    if (value === undefined || value === '') continue;
    let bytes = 0;
    for (const character of value) {
      const point = character.codePointAt(0)!;
      if (point <= 0x1f || (point >= 0x7f && point <= 0x9f)) return `${label}に制御文字を含めることはできません。`;
      bytes += point <= 0x7f ? 1 : point <= 0x7ff ? 2 : point <= 0xffff ? 3 : 4;
    }
    if (bytes > 1024) return `${label}は1024 UTF-8 bytes以下で入力してください。`;
  }
  return null;
}
