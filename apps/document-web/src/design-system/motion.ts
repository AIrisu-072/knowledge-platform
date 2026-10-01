export const MOTION_DURATIONS_MS = {
  instant: 0,
  fast: 90,
  standard: 140,
  spatial: 180,
} as const;

export type MotionDuration = keyof typeof MOTION_DURATIONS_MS;

export function resolveMotionDurationMs(duration: MotionDuration, reducedMotion: boolean): number {
  return reducedMotion ? 0 : MOTION_DURATIONS_MS[duration];
}

export function contextPanelTransition(reducedMotion: boolean) {
  const duration = resolveMotionDurationMs('spatial', reducedMotion) / 1000;
  return {
    layout: { duration },
    opacity: { duration },
  };
}
