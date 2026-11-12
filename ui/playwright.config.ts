import { defineConfig } from '@playwright/test';
export default defineConfig({
  testDir: './tests', fullyParallel: false, workers: 1,
  reporter: [['list'], ['json', { outputFile: 'test-results/results.json' }]],
  use: { baseURL: 'http://127.0.0.1:5186', headless: true, launchOptions: { executablePath: process.env.SENATE_TEST_BROWSER || undefined } },
  webServer: { command: 'npm run dev -- --host 127.0.0.1 --port 5186', url: 'http://127.0.0.1:5186', reuseExistingServer: true },
});
