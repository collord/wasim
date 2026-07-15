import { defineConfig } from '@playwright/test'
export default defineConfig({
  testDir: './e2e',
  timeout: 30_000,
  // The corpus-load smoke test is ~160 short cases — parallelize across workers, all
  // sharing one dev server. Cap workers so we don't oversubscribe the vite server.
  workers: 4,
  fullyParallel: true,
  reporter: [['line'], ['html', { open: 'never' }]],
  use: { baseURL: 'http://localhost:5188' },
  webServer: {
    command: 'npm run dev -- --port 5188 --strictPort',
    url: 'http://localhost:5188',
    timeout: 120_000,
    reuseExistingServer: true,
  },
})
