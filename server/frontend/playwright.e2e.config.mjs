import { defineConfig, devices } from '@playwright/test';

const port = Number(process.env.STRIDE_E2E_PORT ?? 3111);

export default defineConfig({
  testDir: './test/e2e',
  outputDir: '/tmp/stride-playwright-e2e-results',
  reporter: process.env.CI ? [['github'], ['html', { open: 'never' }]] : 'list',
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? 1 : 0,
  workers: 1,
  timeout: 90_000,
  expect: {
    timeout: 10_000,
  },
  use: {
    ...devices['Desktop Chrome'],
    baseURL: `http://127.0.0.1:${port}`,
    screenshot: 'only-on-failure',
    trace: 'retain-on-failure',
    video: 'retain-on-failure',
  },
  webServer: {
    command: 'node test/e2e/server.mjs',
    url: `http://127.0.0.1:${port}/auth/login`,
    reuseExistingServer: false,
    timeout: 120_000,
  },
});
