import { test, expect } from '@playwright/test'
import { fileURLToPath } from 'node:url'
import path from 'node:path'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const TWO_TANK = path.resolve(__dirname, '../../schema_examples_manual/two_tank_hydraulic.json')

test('loads a model, reconciles, edits, and builds a new element', async ({ page }) => {
  const errors: string[] = []
  page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()) })
  page.on('pageerror', (e) => errors.push(String(e)))

  await page.goto('/')

  // Load the two-tank (v1) model via the hidden file input on the drop zone.
  await page.setInputFiles('input[type=file]', TWO_TANK)

  // Readiness: the reconcile round-trip populated the engine summary → the browser lists
  // elements. This is the real "model loaded and valid" signal.
  await expect(page.getByText('Inflow Rate (L/s)')).toBeVisible({ timeout: 15000 })
  await expect(page.getByText('31 elems').first()).toBeVisible()
  await expect(page.getByText('● valid')).toBeVisible()

  // Select an element → the Inspector opens and shows its per-rule editor.
  await page.getByText('Inflow Rate (L/s)').first().click()
  await expect(page.getByText('Definition')).toBeVisible()
  await expect(page.getByText('Editable', { exact: false })).toBeVisible()

  // Insert a new element from the palette; it reconciles to a valid model and appears.
  await page.getByRole('button', { name: 'Palette' }).click()
  await page.getByRole('button', { name: /Constant/ }).first().click()
  await expect(page.getByText('Info')).toBeVisible() // new element auto-selected
  await expect(page.getByText('32 elems').first()).toBeVisible({ timeout: 10000 })

  // Run the model — mode switches to Result and results appear.
  await page.getByRole('button', { name: /Run/ }).click()
  await expect(page.getByRole('button', { name: 'Results' })).toBeVisible({ timeout: 20000 })

  expect(errors.filter((e) => !e.includes('404') && !e.includes('favicon')),
    `console errors:\n${errors.join('\n')}`).toEqual([])
})

test('builds a model from scratch (new → add → wire → run)', async ({ page }) => {
  const errors: string[] = []
  page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()) })
  page.on('pageerror', (e) => errors.push(String(e)))

  await page.goto('/')
  await page.getByRole('button', { name: /New blank model/ }).click()

  // Toolbar appears → we're in the workspace. Add a Constant from the palette.
  await expect(page.getByRole('button', { name: /Run/ })).toBeVisible({ timeout: 15000 })
  await page.getByRole('button', { name: 'Palette' }).click()
  await page.getByRole('button', { name: /Constant/ }).first().click()

  // A fixed node auto-selects; the model reconciles to valid with 1 element.
  await expect(page.getByText('1 elems').first()).toBeVisible({ timeout: 10000 })
  await expect(page.getByText('● valid')).toBeVisible()

  // Add an Expression node too; still a valid v2-native model.
  await page.getByRole('button', { name: 'Palette' }).click()
  await page.getByRole('button', { name: /Expression/ }).first().click()
  await expect(page.getByText('2 elems').first()).toBeVisible({ timeout: 10000 })
  await expect(page.getByText('● valid')).toBeVisible()

  // Run the from-scratch model.
  await page.getByRole('button', { name: /Run/ }).click()
  await expect(page.getByRole('button', { name: 'Results' })).toBeVisible({ timeout: 20000 })

  expect(errors.filter((e) => !e.includes('404') && !e.includes('favicon')),
    `console errors:\n${errors.join('\n')}`).toEqual([])
})

test('runs an optimization over an editable variable', async ({ page }) => {
  const errors: string[] = []
  page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()) })
  page.on('pageerror', (e) => errors.push(String(e)))

  await page.goto('/')
  // Load two-tank: it has editable constants with bounds (eligible optimization variables).
  await page.setInputFiles('input[type=file]', TWO_TANK)
  await expect(page.getByText('● valid')).toBeVisible({ timeout: 15000 })

  // Enter Result mode → Optimization tab.
  await page.getByRole('button', { name: 'Result', exact: true }).click()
  await page.getByRole('button', { name: 'Optimization', exact: true }).click()

  // Objective: pick the first real element; minimize its final value (deterministic model).
  const objSelect = page.locator('select').first()
  const opts = await objSelect.locator('option').evaluateAll(
    (o) => o.map((x) => (x as HTMLOptionElement).value).filter((v) => v))
  await objSelect.selectOption(opts[opts.length - 1])

  // Select the first eligible decision variable.
  await page.locator('input[type=checkbox]').first().check()

  // Run and expect an optimum (objective + evaluations) to render.
  await page.getByRole('button', { name: /Run optimization/i }).click()
  await expect(page.getByText(/evaluations/i)).toBeVisible({ timeout: 25000 })
  await expect(page.getByRole('button', { name: /Apply optimum to model/i })).toBeVisible()

  expect(errors.filter((e) => !e.includes('404') && !e.includes('favicon')),
    `console errors:\n${errors.join('\n')}`).toEqual([])
})
