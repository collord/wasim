import { defineConfig } from 'vitest/config'

// Unit tests live under `src/` (co-located `*.test.ts`). The `e2e/` directory is Playwright's
// (browser + WASM), driven by playwright.config.ts — keep vitest out of it.
export default defineConfig({
  test: {
    include: ['src/**/*.test.ts'],
    exclude: ['e2e/**', 'node_modules/**'],
  },
})
