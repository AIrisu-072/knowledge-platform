import { formatDateTime } from '../src/view-model/date-time';

test.each([
  ['UTC', '2026-10-01T01:00:00Z', '2026/10/01 1:00 (UTC, UTC+00:00)'],
  ['Asia/Tokyo', '2026-10-01T01:00:00Z', '2026/10/01 10:00 (Asia/Tokyo, UTC+09:00)'],
  ['America/New_York', '2026-11-01T05:30:00Z', '2026/11/01 1:30 (America/New_York, UTC-04:00)'],
  ['America/New_York', '2026-11-01T06:30:00Z', '2026/11/01 1:30 (America/New_York, UTC-05:00)'],
  ['America/Argentina/Rio_Gallegos', '2026-10-01T01:00:00Z', '2026/09/30 22:00 (America/Argentina/Rio_Gallegos, UTC-03:00)'],
  ['Asia/Kolkata', '2026-10-01T01:00:00Z', '2026/10/01 6:30 (Asia/Calcutta, UTC+05:30)'],
])('labels %s with the offset at %s', (timeZone, instant, expected) => {
  expect(formatDateTime(instant, timeZone)).toBe(expected);
});

test.each(['invalid-date', '', '2026-13-01T00:00:00Z'])('preserves invalid input %j without inventing a time or zone', value => {
  expect(formatDateTime(value)).toBe(value);
  expect(formatDateTime(value, 'Asia/Tokyo')).toBe(value);
});
