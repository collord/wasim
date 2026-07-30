import { test, expect } from '@playwright/test'

/** The stock-and-flow lens (Part B, phases A6/A7): switching the lens reprograms the palette
 *  vocabulary AND the validation, and the lens tag round-trips through save. This is the
 *  "delete the visualization and the authoring still changed → it's a lens, not a view" proof. */
test('stock-flow lens reprograms palette + validation and round-trips', async ({ page }) => {
  const errors: string[] = []
  page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()) })
  page.on('pageerror', (e) => errors.push(String(e)))

  // Force the blob-download save path so the round-trip is deterministic in headless.
  await page.addInitScript(() => { delete (window as unknown as Record<string, unknown>).showSaveFilePicker })

  await page.goto('/')
  await page.getByRole('button', { name: /New blank model/ }).click()
  await expect(page.getByRole('button', { name: /Run/ })).toBeVisible({ timeout: 15000 })

  // Switch to the Stock & Flow lens via the toolbar picker.
  await page.getByRole('combobox', { name: 'Lens' }).selectOption('stock-flow')

  // The palette is reprogrammed into the Forrester vocabulary: a "Stocks" section and a "Stock"
  // control appear; the general-only "Failure / Event" control is gone.
  await page.getByRole('button', { name: 'Palette' }).click()
  await expect(page.getByText('Stocks', { exact: true })).toBeVisible()
  await expect(page.getByRole('button', { name: /^Stock$/ })).toBeVisible()
  await expect(page.getByRole('button', { name: /Failure \/ Event/ })).toHaveCount(0)

  // Add a Stock → the stock-flow-consistency invariant fires (a lone stock has no flows) as an
  // author-time warning — validation the general lens does not perform.
  await page.getByRole('button', { name: /^Stock$/ }).first().click()
  await expect(page.getByText('1 elems').first()).toBeVisible({ timeout: 10000 })
  await page.getByText(/show issues/).click()
  await expect(page.getByText(/has no inflows or outflows/)).toBeVisible()

  // Canvas glyph (A5): the stock renders as a Forrester box, not the default node.
  await expect(page.locator('g[data-shape="box"]').first()).toBeAttached()

  // Inspector relabel (A4): add a Constant tagged as an auxiliary → the inspector header reads its
  // lens role "Auxiliary" (not its engine kind "Constant"), driven by lens_role.
  await page.getByRole('button', { name: 'Palette' }).click()
  await page.getByRole('button', { name: /^Constant$/ }).first().click()
  await expect(page.getByTestId('inspector-role')).toHaveText('Auxiliary')

  // Canvas glyph (A5): the auxiliary renders as a pill (circle).
  await expect(page.locator('g[data-shape="circle"]').first()).toBeAttached()

  // Round-trip: saving emits JSON carrying the lens tag and the element's lens_role.
  const [download] = await Promise.all([
    page.waitForEvent('download'),
    page.getByRole('button', { name: 'Save', exact: true }).click(),
  ])
  const fs = await import('node:fs/promises')
  const saved = await download.path()
  const text = await fs.readFile(saved, 'utf8')
  expect(text).toContain('"lens": "stock-flow"')
  expect(text).toContain('"lens_role": "stock"')

  expect(errors.filter((e) => !e.includes('404') && !e.includes('favicon')),
    `console errors:\n${errors.join('\n')}`).toEqual([])
})

/** Part B polish: the stock-flow lens offers canonical templates on the empty canvas, they load
 *  warning-free, and running opens the Results view on a stock's trajectory (not a final number). */
test('stock-flow templates load clean and run to a stock trajectory', async ({ page }) => {
  const errors: string[] = []
  page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()) })
  page.on('pageerror', (e) => errors.push(String(e)))

  await page.goto('/')
  await page.getByRole('button', { name: /New blank model/ }).click()
  await expect(page.getByRole('button', { name: /Run/ })).toBeVisible({ timeout: 15000 })

  // Switch to the lens → the empty canvas offers its templates. Load the bathtub.
  await page.getByRole('combobox', { name: 'Lens' }).selectOption('stock-flow')
  await page.getByRole('button', { name: /Bathtub/ }).click()

  // It loads a consistent, warning-free stock-flow model.
  await expect(page.getByText('3 elems').first()).toBeVisible({ timeout: 10000 })
  await expect(page.getByText('● valid')).toBeVisible()
  await expect(page.getByText(/has no inflows|not connected to a stock/)).toHaveCount(0)

  // Run → Results open on the stock's trajectory ("Water level" is the plotted series).
  await page.getByRole('button', { name: /Run/ }).click()
  await expect(page.getByRole('button', { name: 'Results' })).toBeVisible({ timeout: 20000 })
  await expect(page.getByText('Series to plot')).toBeVisible()
  await expect(page.getByRole('button', { name: /Water level/ })).toBeVisible()

  expect(errors.filter((e) => !e.includes('404') && !e.includes('favicon')),
    `console errors:\n${errors.join('\n')}`).toEqual([])
})
