/** Test-only sRGB contrast math shared by the browser token audit and pure regressions. */
export function tokenContrastRatio(foreground: string, background: string): number {
  const parse = (value: string) => {
    const token = value.trim();
    if (!/^#(?:[\da-f]{3}|[\da-f]{6})$/i.test(token)) {
      throw new Error(`Unsupported color token: ${JSON.stringify(value)}`);
    }
    const hex = token.length === 4
      ? [...token.slice(1)].map((digit) => `${digit}${digit}`).join('')
      : token.slice(1);
    return [0, 2, 4].map((offset) => Number.parseInt(hex.slice(offset, offset + 2), 16) / 255);
  };
  const luminance = (value: string) => {
    const channels = parse(value).map((channel) => channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4);
    return channels[0]! * 0.2126 + channels[1]! * 0.7152 + channels[2]! * 0.0722;
  };
  const values = [luminance(foreground), luminance(background)].sort((a, b) => b - a);
  return (values[0]! + 0.05) / (values[1]! + 0.05);
}
