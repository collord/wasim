import { test, expect } from '@playwright/test'
import path from 'node:path'

const MODEL = path.resolve(
  process.env.HOME!,
  'openvsim/wasim/schema_examples/probabilisticoptimization.json',
)

// Repro for the two reported crashes with probabilisticoptimization.json:
//  1) switching to the Dashboard tab, 2) clicking the dashed submodel block in Graph.
test('probabilisticoptimization: dashboard + submodel-box click do not crash', async ({ page }) => {
  const errors: string[] = []
  page.on('pageerror', (e) => errors.push(String(e)))

  await page.goto('/')

  // Load the model via the file input.
  await page.setInputFiles('input[type=file]', MODEL)

  // Model loads → lands on the Graph tab. Wait for the graph svg.
  await page.waitForSelector('svg', { timeout: 10_000 })

  // (1) Dashboard tab must not crash.
  await page.getByRole('button', { name: 'Dashboard' }).click()
  await page.waitForTimeout(300)
  expect(errors, `Dashboard crashed: ${errors.join('\n')}`).toHaveLength(0)

  // (2) Back to Graph, click the submodel box (the dashed box with a ▸/▾ affordance).
  await page.getByRole('button', { name: 'Graph' }).click()
  await page.waitForSelector('svg', { timeout: 5_000 })
  // The submodel box header carries the submodel name text "SubModel1".
  const box = page.locator('svg text', { hasText: 'SubModel1' }).first()
  await box.click()
  await page.waitForTimeout(300)
  expect(errors, `Submodel-box click crashed: ${errors.join('\n')}`).toHaveLength(0)

  // Click again to collapse — also must not crash.
  const frameHeader = page.locator('svg text', { hasText: 'SubModel1' }).first()
  await frameHeader.click()
  await page.waitForTimeout(300)
  expect(errors, `Submodel-box collapse crashed: ${errors.join('\n')}`).toHaveLength(0)
})
