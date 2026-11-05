// Runs with bun: WCAG contrast of caption colours, shared by the settings warning and its tests.
const AA_MINIMUM_RATIO: number = 4.5;
const LUMINANCE_OFFSET: number = 0.05;
const LINEAR_LIMIT: number = 0.03928;
const LINEAR_DIVISOR: number = 12.92;
const GAMMA_OFFSET: number = 0.055;
const GAMMA_DIVISOR: number = 1.055;
const GAMMA: number = 2.4;
const BYTE_MAX: number = 255;
const RED_WEIGHT: number = 0.2126;
const GREEN_WEIGHT: number = 0.7152;
const BLUE_WEIGHT: number = 0.0722;
const HEX_RADIX: number = 16;

const linearChannel = (hex: string, start: number): number => {
  const value: number = Number.parseInt(hex.slice(start, start + 2), HEX_RADIX) / BYTE_MAX;
  return value <= LINEAR_LIMIT ? value / LINEAR_DIVISOR : ((value + GAMMA_OFFSET) / GAMMA_DIVISOR) ** GAMMA;
};

const luminance = (hex: string): number =>
  RED_WEIGHT * linearChannel(hex, 1) +
  GREEN_WEIGHT * linearChannel(hex, 3) +
  BLUE_WEIGHT * linearChannel(hex, 5);

export const contrastRatio = (first: string, second: string): number => {
  const [one, other]: [number, number] = [luminance(first), luminance(second)];
  return (Math.max(one, other) + LUMINANCE_OFFSET) / (Math.min(one, other) + LUMINANCE_OFFSET);
};

export const meetsAa = (ratio: number): boolean => ratio >= AA_MINIMUM_RATIO;
