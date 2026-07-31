import { test, expect } from '@playwright/test'

/** P5: the Functions reference panel (WASIM_LENS_MANIFEST_SPEC.md §9) makes the expression
 *  builtins discoverable — browsable by category with signatures + docs — and click-inserts into
 *  the focused expression editor. */
test('functions panel lists builtins and inserts into the focused expression', async ({ page }) => {
  const errors: string[] = []
  page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()) })
  page.on('pageerror', (e) => errors.push(String(e)))

  await page.goto('/')
  await page.getByRole('button', { name: /New blank model/ }).click()
  await expect(page.getByRole('button', { name: /Run/ })).toBeVisible({ timeout: 15000 })

  // Add an Expression element → it auto-selects and the inspector shows the expression editor.
  await page.getByRole('button', { name: 'Palette' }).click()
  await page.getByRole('button', { name: /^Expression$/ }).first().click()
  await expect(page.getByText('1 elems').first()).toBeVisible({ timeout: 10000 })

  // Focus the expression textarea (it becomes the insert target) and clear it.
  const editor = page.locator('textarea').first()
  await editor.click()
  await editor.fill('')

  // Open the Functions tab: it lists builtins grouped by category (Math present, sig shown).
  await page.getByRole('button', { name: 'Functions', exact: true }).click()
  const panel = page.getByTestId('functions-panel')
  await expect(panel.getByText('Math', { exact: true })).toBeVisible()
  await expect(panel.getByRole('button', { name: /^max\(a, b\)/ })).toBeVisible()

  // Filter narrows the list.
  await panel.getByPlaceholder('filter functions…').fill('sqrt')
  await expect(panel.getByRole('button', { name: /^sqrt\(x\)/ })).toBeVisible()
  await expect(panel.getByRole('button', { name: /^max\(a, b\)/ })).toHaveCount(0)
  await panel.getByPlaceholder('filter functions…').fill('')

  // Click a function → it inserts `max()` into the focused expression editor.
  await panel.getByRole('button', { name: /^max\(a, b\)/ }).click()
  await expect(editor).toHaveValue('max()')

  expect(errors.filter((e) => !e.includes('404') && !e.includes('favicon')),
    `console errors:\n${errors.join('\n')}`).toEqual([])
})
