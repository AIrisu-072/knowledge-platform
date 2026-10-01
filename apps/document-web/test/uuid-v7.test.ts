test('operation identifiers use the required UUIDv7 layout', async () => {
  const { createOperationId } = await import('../src/application/operation-id');

  expect(createOperationId()).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i);
});
