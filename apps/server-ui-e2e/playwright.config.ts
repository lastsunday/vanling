import { defineConfig, devices } from '@playwright/test';
import path from 'path';

const baseURL = process.env['BASE_URL'] || 'http://localhost:4300';

export default defineConfig({
  testDir: './src',
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: process.env.CI ? 1 : undefined,
  reporter: 'html',
  use: {
    baseURL,
    trace: 'on-first-retry',
  },
  webServer: {
    command: 'pnpm run preview',
    url: 'http://localhost:4300',
    reuseExistingServer: !process.env.CI,
    cwd: path.resolve(__dirname, '../server-ui'),
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
});
