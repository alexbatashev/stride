import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
  testDir: './test/browser',
  outputDir: '/tmp/stride-playwright-results',
  reporter: 'list',
  use: {
    ...devices['Desktop Chrome'],
  },
});
