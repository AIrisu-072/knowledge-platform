test('motion durations match the design tokens and reduce to zero', async () => {
  const {
    MOTION_DURATIONS_MS,
    resolveMotionDurationMs,
    contextPanelTransition,
  } = await import('../src/design-system/motion');

  expect(MOTION_DURATIONS_MS).toEqual({ instant: 0, fast: 90, standard: 140, spatial: 180 });
  expect(resolveMotionDurationMs('fast', false)).toBe(90);
  expect(resolveMotionDurationMs('spatial', true)).toBe(0);

  const transition = contextPanelTransition(false);
  expect(transition.layout.duration).toBe(0.18);
  expect(transition.opacity.duration).toBe(0.14);
  expect(contextPanelTransition(true).layout.duration).toBe(0);
  expect(contextPanelTransition(true).opacity.duration).toBe(0);
});
