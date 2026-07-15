import { defineConfig } from '@playwright/test'
export default defineConfig({
  testDir: './e2e',
  timeout: 30_000,
  // ~160 short cases across parallel workers sharing one server. One retry absorbs the
  // occasional page-load lag under parallel load (verified: any "failure" here passes on
  // its own — the models genuinely load/run; this jitter is infrastructure, not a real bug).
  workers: 4,
  retries: 1,
  fullyParallel: true,
  reporter: [['line'], ['html', { open: 'never' }]],
  use: { baseURL: 'http://localhost:5188' },
  // Serve the production build (vite preview) rather than the dev server: no HMR / on-demand
  // compilation, so it's fast and stable under parallel workers. Requires `npm run build`
  // first (the webServer command builds, then previews).
  webServer: {
    command: 'npm run build && npm run preview -- --port 5188 --strictPort',
    url: 'http://localhost:5188',
    timeout: 180_000,
    reuseExistingServer: true,
  },
})
