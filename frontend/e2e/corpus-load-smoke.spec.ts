import { test, expect } from '@playwright/test'
import fs from 'node:fs'
import path from 'node:path'

// Smoke test: every corpus model must load in the harness without the page throwing an
// uncaught error, and must reach a terminal state — either the Graph renders (loaded OK) or
// the harness shows a clean error (a model the engine legitimately rejects, e.g. the
// stale-format files). A hang, white-screen, or uncaught pageerror is a failure. For loaded
// models we also visit every tab, since crashes have surfaced on Dashboard/Model, not Graph.

const CORPUS_DIR = path.resolve(process.env.HOME!, 'openvsim/wasim/schema_examples')

const models = fs.existsSync(CORPUS_DIR)
  ? fs.readdirSync(CORPUS_DIR).filter((f) => f.endsWith('.json')).sort()
  : []

test.describe('corpus loads without crashing', () => {
  if (models.length === 0) {
    test('corpus present', () => {
      test.skip(true, `no corpus at ${CORPUS_DIR}`)
    })
  }

  for (const name of models) {
    test(name, async ({ page }) => {
      const errors: string[] = []
      page.on('pageerror', (e) => errors.push(String(e)))

      await page.goto('/')
      await page.setInputFiles('input[type=file]', path.join(CORPUS_DIR, name))

      // Terminal state: the reconcile round-trip settles the status bar to valid OR errors.
      const okFlag = page.getByText('● valid')
      const errorFlag = page.getByText(/⚠ \d+ error/)
      await expect(okFlag.or(errorFlag)).toBeVisible({ timeout: 20_000 })

      const rejected = await errorFlag.isVisible().catch(() => false)
      if (!rejected) {
        // Result mode exposes the original views; visit each — a render crash on any surfaces
        // as an uncaught pageerror (there is no React error boundary). Settle after each.
        await page.getByRole('button', { name: 'Result', exact: true }).click()
        for (const tab of ['Model', 'Dashboard', 'Results', 'Graph']) {
          await page.getByRole('button', { name: tab, exact: true }).click()
          await page.waitForTimeout(250)
        }
      }
      // Let any pending microtask-dispatched pageerror land before asserting.
      await page.waitForTimeout(200)

      expect(errors, `${name} threw:\n${errors.join('\n')}`).toHaveLength(0)
    })
  }
})
