module.exports = {
  clearMocks: true,
  restoreMocks: true,
  roots: ['<rootDir>/test'],
  setupFilesAfterEnv: ['<rootDir>/test/setup.ts'],
  testEnvironment: 'jsdom',
  testMatch: ['**/*.test.ts', '**/*.test.tsx'],
  transform: { '^.+\\.[tj]sx?$': 'babel-jest' },
  moduleFileExtensions: ['ts', 'tsx', 'js', 'jsx', 'json'],
  moduleNameMapper: { '\\.module\\.css$': '<rootDir>/test/style-mock.cjs' },
};
