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

/** A5 draw-flow gesture: dragging from a flow's connection handle onto a stock wires it as an
 *  inflow, which resolves the stock-flow-consistency warnings. */
test('draw-flow gesture wires a flow into a stock', async ({ page }) => {
  const errors: string[] = []
  page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()) })
  page.on('pageerror', (e) => errors.push(String(e)))

  const centerDrag = async (from: { x: number; y: number }, to: { x: number; y: number }) => {
    await page.mouse.move(from.x, from.y)
    await page.mouse.down()
    await page.mouse.move(to.x, to.y, { steps: 12 })
    await page.mouse.up()
  }
  const center = (b: { x: number; y: number; width: number; height: number }) => ({ x: b.x + b.width / 2, y: b.y + b.height / 2 })

  await page.goto('/')
  await page.getByRole('button', { name: /New blank model/ }).click()
  await expect(page.getByRole('button', { name: /Run/ })).toBeVisible({ timeout: 15000 })
  await page.getByRole('combobox', { name: 'Lens' }).selectOption('stock-flow')

  // Add a Stock and a Flow (both land at the same insert point, so move the flow clear first).
  await page.getByRole('button', { name: 'Palette' }).click()
  await page.getByRole('button', { name: /^Stock$/ }).first().click()
  await expect(page.getByText('1 elems').first()).toBeVisible({ timeout: 10000 })
  await page.getByRole('button', { name: 'Palette' }).click()
  await page.getByRole('button', { name: /^Flow$/ }).first().click()
  await expect(page.getByText('2 elems').first()).toBeVisible({ timeout: 10000 })

  // Move the flow straight down so it clears the stock (staying inside the canvas — a large
  // sideways move would push its handle under the Inspector pane).
  const flowBox = await page.getByTestId('node-flow').boundingBox()
  await centerDrag(center(flowBox!), { x: center(flowBox!).x, y: center(flowBox!).y + 190 })

  // Both SFC invariants fire (stock has no flows; flow not connected).
  await page.getByText(/show issues/).click()
  await expect(page.getByText(/not connected to a stock/)).toBeVisible()

  // Draw from the flow's handle onto the stock → wires it as an inflow, clearing the warnings.
  const handleBox = await page.getByTestId('link-handle-flow').boundingBox()
  const stockBox = await page.getByTestId('node-stock').boundingBox()
  await centerDrag(center(handleBox!), center(stockBox!))

  await expect(page.getByText(/not connected to a stock|has no inflows/)).toHaveCount(0, { timeout: 10000 })
  // The connection is visible: a flow edge is drawn from the flow to the stock.
  await expect(page.locator('line[data-flow-edge]')).not.toHaveCount(0)

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

/** Part D: the reliability lens reprograms the palette to components/state, reuses the Event FSM
 *  authoring, and its template runs to a component status trajectory — proving the "second lens is
 *  a spec file" claim over already-built primitives. */
test('reliability lens reprograms authoring and its template runs', async ({ page }) => {
  const errors: string[] = []
  page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()) })
  page.on('pageerror', (e) => errors.push(String(e)))

  await page.addInitScript(() => { delete (window as unknown as Record<string, unknown>).showSaveFilePicker })
  await page.goto('/')
  await page.getByRole('button', { name: /New blank model/ }).click()
  await expect(page.getByRole('button', { name: /Run/ })).toBeVisible({ timeout: 15000 })

  // Switch to the reliability lens → the palette shows Components, not the stock-flow vocabulary.
  await page.getByRole('combobox', { name: 'Lens' }).selectOption('reliability')
  await page.getByRole('button', { name: 'Palette' }).click()
  await expect(page.getByText('Components', { exact: true })).toBeVisible()
  await expect(page.getByRole('button', { name: /^Component$/ })).toBeVisible()
  await expect(page.getByRole('button', { name: /^Stock$/ })).toHaveCount(0)

  // Load the template → a valid, runnable reliability model.
  await page.getByRole('button', { name: /Repairable component/ }).click()
  await expect(page.getByText('3 elems').first()).toBeVisible({ timeout: 10000 })
  await expect(page.getByText('● valid')).toBeVisible()

  // Run → Results open on the component's status trajectory.
  await page.getByRole('button', { name: /Run/ }).click()
  await expect(page.getByRole('button', { name: 'Results' })).toBeVisible({ timeout: 20000 })
  await expect(page.getByRole('button', { name: /Component/ }).first()).toBeVisible()

  // Round-trip: the lens + the component role survive save.
  const [download] = await Promise.all([
    page.waitForEvent('download'),
    page.getByRole('button', { name: 'Save', exact: true }).click(),
  ])
  const fs = await import('node:fs/promises')
  const text = await fs.readFile((await download.path()), 'utf8')
  expect(text).toContain('"lens": "reliability"')
  expect(text).toContain('"lens_role": "component"')

  expect(errors.filter((e) => !e.includes('404') && !e.includes('favicon')),
    `console errors:\n${errors.join('\n')}`).toEqual([])
})
