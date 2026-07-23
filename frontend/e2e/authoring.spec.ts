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

test('results_spec analysis renders distribution + final-value statistics', async ({ page }) => {
  const errors: string[] = []
  page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()) })
  page.on('pageerror', (e) => errors.push(String(e)))

  const RETIREMENT = path.resolve(__dirname, '../../schema_examples_manual/retirement_planning.json')
  await page.goto('/')
  await page.setInputFiles('input[type=file]', RETIREMENT)
  await expect(page.getByText('● valid')).toBeVisible({ timeout: 15000 })

  // Result mode → Results; enable distribution + final-value statistics, run with analysis.
  await page.getByRole('button', { name: 'Result', exact: true }).click()
  await page.getByRole('button', { name: 'Results', exact: true }).click()
  await page.getByText(/Final-value distribution/i).click()          // toggles the checkbox label
  await page.getByText(/Final-value statistics \(CI/i).click()
  await page.getByRole('button', { name: /Run with analysis/i }).click()

  // Select an element that saves final values (the default output is time-history only), so
  // the final-value distribution + statistics have data.
  await expect(page.getByText(/Statistics for/i)).toBeVisible({ timeout: 25000 })
  await page.locator('select').filter({ hasText: 'Total Post-Tax Portfolio' }).selectOption('total_post_tax')

  // The analysis-driven panels appear (headings unique to the rendered panels).
  await expect(page.getByRole('heading', { name: 'Distribution', exact: true })).toBeVisible({ timeout: 10000 })
  await expect(page.getByText(/Excess kurtosis/i)).toBeVisible()
  // CDF/CCDF view toggle works.
  await page.getByRole('button', { name: 'ccdf' }).click()

  expect(errors.filter((e) => !e.includes('404') && !e.includes('favicon')),
    `console errors:\n${errors.join('\n')}`).toEqual([])
})

test('inserts and configures a Status latch (new node-rule editors)', async ({ page }) => {
  const errors: string[] = []
  page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()) })
  page.on('pageerror', (e) => errors.push(String(e)))

  await page.goto('/')
  await page.getByRole('button', { name: /New blank model/ }).click()
  await expect(page.getByRole('button', { name: /Run/ })).toBeVisible({ timeout: 15000 })

  // Insert a Status latch from the palette (a valid, input-free v2 scaffold).
  await page.getByRole('button', { name: 'Palette' }).click()
  await page.getByRole('button', { name: /Status latch/ }).first().click()
  await expect(page.getByText('1 elems').first()).toBeVisible({ timeout: 10000 })
  await expect(page.getByText('● valid')).toBeVisible()

  // Its inspector shows the set/reset trigger editors; change the set trigger to periodic.
  await expect(page.getByText('Set trigger', { exact: true })).toBeVisible()
  await expect(page.getByText('Reset trigger', { exact: true })).toBeVisible()
  await page.locator('select').filter({ hasText: 'Always' }).first().selectOption('periodic')
  await expect(page.getByText(/Period \(/)).toBeVisible()
  // Still a valid model after the edit.
  await expect(page.getByText('● valid')).toBeVisible({ timeout: 10000 })

  expect(errors.filter((e) => !e.includes('404') && !e.includes('favicon')),
    `console errors:\n${errors.join('\n')}`).toEqual([])
})

test('curates an author dashboard (inputs as sliders + output tiles)', async ({ page }) => {
  const errors: string[] = []
  page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()) })
  page.on('pageerror', (e) => errors.push(String(e)))

  await page.goto('/')
  await page.setInputFiles('input[type=file]', TWO_TANK)   // has editable constants with bounds
  await expect(page.getByText('● valid')).toBeVisible({ timeout: 15000 })

  await page.getByRole('button', { name: 'Result', exact: true }).click()
  await page.getByRole('button', { name: 'Dashboard', exact: true }).click()

  // Enter configure mode and curate one input + one output.
  await page.getByRole('button', { name: /Configure dashboard/i }).click()
  await expect(page.getByRole('heading', { name: /Configure dashboard/i })).toBeVisible()
  await page.getByText('Inputs (editable parameters)').locator('..').locator('input[type=checkbox]').first().check()
  await page.getByText('Outputs (result displays)').locator('..').locator('input[type=checkbox]').first().check()
  await page.getByRole('button', { name: 'Done', exact: true }).click()

  // Curated view: an input slider and an output tile render.
  await expect(page.getByRole('heading', { name: 'Dashboard', exact: true })).toBeVisible()
  await expect(page.locator('input[type=range]').first()).toBeVisible()
  await expect(page.getByText(/run to see output|p05/).first()).toBeVisible()

  expect(errors.filter((e) => !e.includes('404') && !e.includes('favicon')),
    `console errors:\n${errors.join('\n')}`).toEqual([])
})

test('copilot proposes an engine-validated model and applies it on accept (stub provider)', async ({ page }) => {
  const errors: string[] = []
  page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()) })
  page.on('pageerror', (e) => errors.push(String(e)))

  // Inject a stub LLM provider + config BEFORE app load (no network). The stub returns a
  // propose_model tool call with a known-valid v2 model; the engine still validates it.
  await page.addInitScript(() => {
    const model = JSON.stringify({
      wasim_version: '0.1.0',
      simulation_settings: { duration: { value: 100, unit: 's' }, timestep: { value: 1, unit: 's' }, n_realizations: 1, seed: 42 },
      containers: [],
      elements: [
        { id: 'a', name: 'A', primitive: 'node', value_rule: 'fixed', value: { value: 2, unit: '1' }, editable: true },
        { id: 'b', name: 'B', primitive: 'node', value_rule: 'expression', inputs: ['a'],
          expression: { ast: { op: 'multiply', left: { op: 'ref', element_id: 'a' }, right: { op: 'literal', value: 3 } }, display: 'a × 3' } },
      ],
    })
    localStorage.setItem('wasim.llm.config', JSON.stringify({ provider: 'anthropic', model: 'claude-opus-4-8', apiKey: 'stub-key' }))
    ;(window as unknown as { __wasimLlmProvider: unknown }).__wasimLlmProvider = {
      chat: async () => ({ text: 'A constant A and B = A × 3.', toolCalls: [{ id: 't1', name: 'propose_model', input: { model_json: model, rationale: 'A constant A and B = A × 3.' } }] }),
    }
  })

  await page.goto('/')
  await page.getByRole('button', { name: /New blank model/ }).click()
  await expect(page.getByRole('button', { name: /Run/ })).toBeVisible({ timeout: 15000 })

  // Open the copilot, describe a model, send.
  await page.getByRole('button', { name: /Copilot/ }).click()
  await expect(page.getByText('AI Copilot')).toBeVisible()
  await page.getByPlaceholder(/Describe or refine/).fill('build me a small two-element model')
  await page.getByRole('button', { name: 'Send', exact: true }).click()

  // The proposal validates through the engine and is offered for review.
  await expect(page.getByText('Proposed model')).toBeVisible({ timeout: 15000 })
  await expect(page.getByText('engine-valid')).toBeVisible()

  // Accept → the model enters the canonical doc via reconcile and its elements appear.
  await page.getByRole('button', { name: 'Accept', exact: true }).click()
  await expect(page.getByText('2 elems').first()).toBeVisible({ timeout: 10000 })
  await expect(page.getByText('● valid')).toBeVisible()

  expect(errors.filter((e) => !e.includes('404') && !e.includes('favicon')),
    `console errors:\n${errors.join('\n')}`).toEqual([])
})
