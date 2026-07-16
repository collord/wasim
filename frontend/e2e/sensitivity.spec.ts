import { test, expect } from '@playwright/test'
import fs from 'node:fs'
import path from 'node:path'

// End-to-end for the runtime sensitivity sweep. Loads a deterministic corpus model with a
// fixed-scalar input that drives an expression output (expression.json: Length_in_ft →
// Length_in_km), configures a one-at-a-time sweep, runs it, and asserts a non-degenerate
// response curve renders with no uncaught page error.
//
// Non-degeneracy is checked structurally: the swept variable feeds a linear conversion, so
// the curve must span more than one distinct Y — we read the rendered dot positions off the
// recharts SVG and assert they are not all equal (a flat curve would mean the sweep did
// nothing, e.g. the input wasn't actually varied).

const CORPUS_DIR = path.resolve(process.env.HOME!, 'openvsim/wasim/schema_examples')
const MODEL = 'expression.json'

test.describe('sensitivity sweep', () => {
  test.skip(
    !fs.existsSync(path.join(CORPUS_DIR, MODEL)),
    `corpus model ${MODEL} not present`,
  )

  test('one-at-a-time produces a non-degenerate curve', async ({ page }) => {
    const errors: string[] = []
    page.on('pageerror', (e) => errors.push(String(e)))

    await page.goto('/')
    await page.setInputFiles('input[type=file]', path.join(CORPUS_DIR, MODEL))

    // Model loaded once the graph svg renders.
    await expect(page.locator('svg').first()).toBeVisible({ timeout: 20_000 })

    // Go to the Sensitivity tab.
    await page.getByRole('button', { name: 'Sensitivity', exact: true }).click()

    // The tab must offer at least one sweepable input (fixed scalar). If it says "No
    // sweepable inputs" for this model, the filter regressed.
    await expect(page.getByText(/No sweepable inputs/i)).toHaveCount(0)

    // Enable the first variable (its checkbox), pick a result element, run.
    const firstCheckbox = page.locator('input[type=checkbox]').first()
    await firstCheckbox.check()

    // Pick the result element: the first <select> is "Result element". Choose the last option
    // (an expression output, driven by the inputs) so the sweep produces a real response.
    const resultSelect = page.locator('select').first()
    const optionValues = await resultSelect.locator('option').evaluateAll(
      (opts) => opts.map((o) => (o as HTMLOptionElement).value).filter((v) => v),
    )
    expect(optionValues.length).toBeGreaterThan(0)
    // Length_in_km is the expression output; select it by its id if present, else first real option.
    const km = optionValues.find((v) => /km/i.test(v)) ?? optionValues[0]
    await resultSelect.selectOption(km)

    await page.getByRole('button', { name: /Run sweep/i }).click()

    // The results heading appears when the sweep completes.
    await expect(page.getByText(/→ result/i).first()).toBeVisible({ timeout: 20_000 })

    // Non-degenerate: the curve's rendered dots must span more than one Y position.
    // recharts draws each line point as <circle class="recharts-line-dot">. Wait for the
    // chart to lay out (ResponsiveContainer needs a tick after the container is visible).
    const dots = page.locator('circle.recharts-line-dot')
    await expect(dots.first()).toBeVisible({ timeout: 10_000 })
    const cys = await dots.evaluateAll(
      (nodes) => nodes.map((n) => Number((n as SVGCircleElement).getAttribute('cy'))),
    )
    expect(cys.length, 'curve should have multiple dots').toBeGreaterThan(1)
    const distinct = new Set(cys.map((y) => Math.round(y)))
    expect(distinct.size, `curve is flat (all cy equal): ${cys.join(',')}`).toBeGreaterThan(1)

    // Toggle GoldSim-style normalized X (0–1) and confirm the curve re-renders without error
    // and stays non-degenerate (the Y variation is X-axis-independent).
    await page.getByText(/Normalize X axis/i).click()
    await expect(dots.first()).toBeVisible({ timeout: 10_000 })
    const cysNorm = await dots.evaluateAll(
      (nodes) => nodes.map((n) => Number((n as SVGCircleElement).getAttribute('cy'))),
    )
    expect(new Set(cysNorm.map((y) => Math.round(y))).size, 'normalized curve is flat').toBeGreaterThan(1)

    await page.waitForTimeout(200)
    expect(errors, `sensitivity threw:\n${errors.join('\n')}`).toHaveLength(0)
  })
})
