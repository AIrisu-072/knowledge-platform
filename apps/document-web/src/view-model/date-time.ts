/** Presentation only: omitting timeZone preserves the browser's local conversion. */
export function formatDateTime(value: string, timeZone?: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.valueOf())) return value;

  const formatter = new Intl.DateTimeFormat('ja-JP', {
    dateStyle: 'medium', timeStyle: 'short', ...(timeZone ? { timeZone } : {}),
  });
  const actualZone = formatter.resolvedOptions().timeZone;
  // Resolve the offset at this instant; a zone name alone cannot distinguish a DST fold.
  const offset = new Intl.DateTimeFormat('en-US', {
    timeZone: actualZone, timeZoneName: 'longOffset',
  }).formatToParts(date).find(part => part.type === 'timeZoneName')!.value;
  const utcOffset = offset === 'GMT' ? 'UTC+00:00' : offset.replace('GMT', 'UTC');
  return `${formatter.format(date)} (${actualZone}, ${utcOffset})`;
}
