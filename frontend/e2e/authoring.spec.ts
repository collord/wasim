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
