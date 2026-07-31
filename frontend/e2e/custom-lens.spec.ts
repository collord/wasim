import { test, expect } from '@playwright/test'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const HERE = path.dirname(fileURLToPath(import.meta.url))

/** P6: a user imports a custom lens as pure JSON (WASIM_LENS_MANIFEST_SPEC.md §7). It appears in
 *  the picker, reprograms the palette, and round-trips `view.lens` — with zero TypeScript. This is
 *  the browser-SPA delivery of the spec's "overlay" (file import + localStorage, not repo FS). */
test('imports a custom lens: appears in picker, reprograms palette, round-trips', async ({ page }) => {
  const errors: string[] = []
  page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()) })
  page.on('pageerror', (e) => errors.push(String(e)))
  // Deterministic download path in headless. Playwright gives each test a fresh browser context,
  // so localStorage starts empty — no prior-run lens leaks in.
  await page.addInitScript(() => { delete (window as unknown as Record<string, unknown>).showSaveFilePicker })

  await page.goto('/')
  await page.getByRole('button', { name: /New blank model/ }).click()
  await expect(page.getByRole('button', { name: /Run/ })).toBeVisible({ timeout: 15000 })

  // Import the lens file via the "+ Lens" picker.
  await page.locator('input[type="file"][accept=".json"]').last()
    .setInputFiles(path.join(HERE, 'fixtures', 'demo-lens.json'))

  // It becomes the active lens and appears in the picker.
  const picker = page.getByRole('combobox', { name: 'Lens' })
  await expect(picker).toHaveValue('demo-widgets')
  await expect(picker.getByRole('option', { name: 'Widgets' })).toHaveCount(1)

  // The palette is reprogrammed into the custom vocabulary: a "Gadgets" section + "Gizmo" control,
  // and the general-only "Constant" label is gone (it's relabeled to Gizmo).
  await page.getByRole('button', { name: 'Palette' }).click()
  await expect(page.getByText('Gadgets', { exact: true })).toBeVisible()
  await expect(page.getByRole('button', { name: /^Gizmo$/ })).toBeVisible()

  // Insert the custom-role element and save → the lens tag + role round-trip.
  await page.getByRole('button', { name: /^Gizmo$/ }).first().click()
  await expect(page.getByText('1 elems').first()).toBeVisible({ timeout: 10000 })
  const [download] = await Promise.all([
    page.waitForEvent('download'),
    page.getByRole('button', { name: 'Save', exact: true }).click(),
  ])
  const fs = await import('node:fs/promises')
  const text = await fs.readFile((await download.path()), 'utf8')
  expect(text).toContain('"lens": "demo-widgets"')
  expect(text).toContain('"lens_role": "gizmo"')

  // Persistence: reload keeps the imported lens in the picker (localStorage-backed).
  await page.reload()
  await page.getByRole('button', { name: /New blank model/ }).click()
  await expect(page.getByRole('button', { name: /Run/ })).toBeVisible({ timeout: 15000 })
  await expect(page.getByRole('combobox', { name: 'Lens' }).getByRole('option', { name: 'Widgets' })).toHaveCount(1)

  // Remove it → it disappears from the picker.
  await page.getByRole('combobox', { name: 'Lens' }).selectOption('demo-widgets')
  await page.getByTitle('Remove this imported lens').click()
  await expect(page.getByRole('combobox', { name: 'Lens' }).getByRole('option', { name: 'Widgets' })).toHaveCount(0)

  // Cleanup so the persisted lens doesn't leak into other specs.
  await page.evaluate(() => localStorage.clear())

  expect(errors.filter((e) => !e.includes('404') && !e.includes('favicon')),
    `console errors:\n${errors.join('\n')}`).toEqual([])
})
