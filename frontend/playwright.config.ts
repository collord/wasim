import { defineConfig } from '@playwright/test'
export default defineConfig({
  testDir: './e2e',
  timeout: 60_000,
  use: { baseURL: 'http://localhost:5188' },
  webServer: {
    command: 'npm run dev -- --port 5188 --strictPort',
    url: 'http://localhost:5188',
    timeout: 120_000,
    reuseExistingServer: false,
  },
})
