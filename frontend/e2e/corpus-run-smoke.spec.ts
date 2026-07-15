import { test, expect } from '@playwright/test'
import fs from 'node:fs'
import path from 'node:path'

// Smoke test: every corpus model that LOADS must also RUN with (reduced) default parameters
// without crashing or hanging. We don't validate outputs yet — just that the run reaches a
// terminal state (done or a clean error) within a hard timeout, and throws no uncaught error.
// A run that never leaves the "running" state (an infinite loop / non-terminating model) fails
// via the timeout. Realizations are forced low so the smoke pass stays fast.

const CORPUS_DIR = path.resolve(process.env.HOME!, 'openvsim/wasim/schema_examples')
const REALIZATIONS = '5'
const RUN_TIMEOUT_MS = 25_000 // hard cap: past this, the model is treated as hung → fail

const models = fs.existsSync(CORPUS_DIR)
  ? fs.readdirSync(CORPUS_DIR).filter((f) => f.endsWith('.json')).sort()
  : []

test.describe('corpus runs without crashing or hanging', () => {
  if (models.length === 0) {
    test('corpus present', () => test.skip(true, `no corpus at ${CORPUS_DIR}`))
  }

  for (const name of models) {
    test(name, async ({ page }) => {
      const errors: string[] = []
      page.on('pageerror', (e) => errors.push(String(e)))

      await page.goto('/')
      await page.setInputFiles('input[type=file]', path.join(CORPUS_DIR, name))

      // Reach a load terminal state. If the model doesn't load (engine-rejected), skip the run
      // — the load-smoke suite covers those; there's nothing to run.
      const svg = page.locator('svg').first()
      const errorFlag = page.getByText('Error — see dashboard')
      await expect(svg.or(errorFlag)).toBeVisible({ timeout: 20_000 })
      if (await errorFlag.isVisible().catch(() => false)) {
        test.skip(true, `${name} does not load (engine-rejected); nothing to run`)
        return
      }

      // Configure a small realization count, then run.
      await page.getByRole('button', { name: 'Dashboard', exact: true }).click()
      await page.getByLabel('Realizations').fill(REALIZATIONS)
      await page.getByRole('button', { name: /Run Simulation/i }).click()

      // The button reads "Running…" while the run is in flight; it returns to "Run Simulation"
      // when done (and the app switches to Results), or the harness shows an error. Watch that
      // transition: the run is terminal once "Running…" is gone. If it never clears within the
      // hard timeout, the model hung / did not terminate → fail.
      const running = page.getByRole('button', { name: /Running/i })
      const errored = page.getByText('Error — see dashboard')
      // A fast run may finish before we observe "Running…", so don't require it to appear.
      try {
        await expect(running.or(errored)).toBeVisible({ timeout: 3_000 }).catch(() => {})
        await expect
          .poll(async () => (await errored.isVisible()) || !(await running.isVisible()), {
            timeout: RUN_TIMEOUT_MS,
            intervals: [250, 500, 1000],
          })
          .toBe(true)
      } catch {
        throw new Error(`${name}: run did not terminate within ${RUN_TIMEOUT_MS}ms (hang / non-terminating)`)
      }

      // Let any pending pageerror land before asserting.
      await page.waitForTimeout(200)
      expect(errors, `${name} threw during run:\n${errors.join('\n')}`).toHaveLength(0)
    })
  }
})
