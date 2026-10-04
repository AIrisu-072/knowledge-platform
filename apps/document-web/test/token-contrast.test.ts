import { tokenContrastRatio } from '../e2e/token-contrast';

test.each([
  ['#123', '#fff', '#112233', '#ffffff'],
  ['#abc', '#def', '#aabbcc', '#ddeeff'],
  [' #AbC ', '#FFF', '#aabbcc', '#ffffff'],
])('equivalent short and full hex colors have the same contrast: %s / %s', (shortForeground, shortBackground, fullForeground, fullBackground) => {
  expect(tokenContrastRatio(shortForeground, shortBackground)).toBeCloseTo(tokenContrastRatio(fullForeground, fullBackground), 12);
});

test('black and white have contrast 21 in both orders and representations', () => {
  expect(tokenContrastRatio('#000000', '#ffffff')).toBe(21);
  expect(tokenContrastRatio('#fff', '#000')).toBe(21);
});

test('low contrast fails the unchanged 4.5 acceptance threshold', () => {
  const ratios = [tokenContrastRatio('#eee', '#fff'), tokenContrastRatio('#eeeeee', '#ffffff')];
  expect(ratios.every((ratio) => ratio >= 4.5)).toBe(false);
  for (const ratio of ratios) {
    expect(Number.isFinite(ratio)).toBe(true);
    expect(ratio).toBeLessThan(4.5);
  }
  expect(tokenContrastRatio('#fff', '#ffffff')).toBe(1);
});

test.each(['', '#ff', '#ffff', '#fffff', '#ffffffff', '#ggg', 'rgb(0, 0, 0)', 'transparent', '#ffffff extra'])('unsupported token explicitly fails rather than returning NaN: %s', (value) => {
  expect(() => tokenContrastRatio(value, '#ffffff')).toThrow('Unsupported color token');
  expect(() => tokenContrastRatio('#000000', value)).toThrow('Unsupported color token');
});
