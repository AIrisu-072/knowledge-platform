import { jstDateTimeLocalToUtc } from '../src/application/schedule-time';

test('scheduled local date and time is sent as an explicit JST instant', () => {
  expect(jstDateTimeLocalToUtc('2026-10-15T09:30')).toBe('2026-10-15T00:30:00.000Z');
});

test('invalid local dates fail closed', () => {
  expect(jstDateTimeLocalToUtc('2026-02-30T09:30')).toBeNull();
  expect(jstDateTimeLocalToUtc('2026-10-15 09:30')).toBeNull();
});
