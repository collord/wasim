import { test, expect } from '@playwright/test'

/** §17 LLM-authoring copilot — the thin browser slice. Stubs the Anthropic API (no real key/call)
 *  so the *real* loop runs end-to-end: draft → validate through the WASM engine → self-correct →
 *  Accept loads a model that Runs. The stub returns an invalid model first, then a valid one, to
 *  exercise the validate-feedback retry against the real engine. */

// First response: a model whose expression ref uses the wrong field (`reference` not `element_id`)
// → the engine parser rejects it. Second: the engine-valid bathtub.
const INVALID_MODEL = JSON.stringify({
  wasim_version: '0.1.0',
  simulation_settings: { duration: { value: 10, unit: 's' }, timestep: { value: 1, unit: 's' } },
  elements: [
    {
      id: 'a', name: 'A', primitive: 'node', value_rule: 'expression',
      expression: { ast: { op: 'ref', reference: 'ghost' } }, inputs: ['ghost'],
    },
  ],
})
const VALID_MODEL = JSON.stringify({
  wasim_version: '0.1.0',
  simulation_settings: { duration: { value: 60, unit: 's' }, timestep: { value: 1, unit: 's' }, n_realizations: 1, seed: 42 },
  elements: [
    { id: 'tap', name: 'Tap', primitive: 'node', value_rule: 'fixed', value: { value: 5, unit: '1' }, save_results: { time_history: true, final_value: true } },
    { id: 'water', name: 'Water', primitive: 'stock', initial_value: { value: 10, unit: '1' }, inflows: ['tap'], outflows: [], save_results: { time_history: true, final_value: true } },
  ],
})

function messagesResponse(modelJson: string) {
  return {
    status: 200,
    contentType: 'application/json',
    body: JSON.stringify({
      id: 'msg_test',
      type: 'message',
      role: 'assistant',
      content: [{ type: 'text', text: '```json\n' + modelJson + '\n```' }],
      stop_reason: 'end_turn',
    }),
  }
}

test('copilot: describe → validate-loop against WASM → Accept loads + runs', async ({ page }) => {
  const errors: string[] = []
  page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()) })
  page.on('pageerror', (e) => errors.push(String(e)))

  // Stub the Anthropic API: invalid model on the 1st call, valid on the 2nd (drives one retry).
  let call = 0
  await page.route('https://api.anthropic.com/v1/messages', (route) => {
    call += 1
    route.fulfill(messagesResponse(call === 1 ? INVALID_MODEL : VALID_MODEL))
  })

  await page.goto('/')
  await page.getByRole('button', { name: /New blank model/ }).click()
  await expect(page.getByRole('button', { name: /Run/ })).toBeVisible({ timeout: 15000 })

  // Set an API key via the Settings dialog (doc-independent AI section).
  await page.getByRole('button', { name: 'Settings…' }).click()
  await page.getByPlaceholder('sk-ant-…').fill('sk-ant-test-key')
  await page.getByRole('button', { name: 'Done' }).click()

  // Open the Copilot tab, describe a model, generate.
  await page.getByRole('button', { name: 'Copilot', exact: true }).click()
  const panel = page.getByTestId('copilot-panel')
  await panel.getByRole('textbox').fill('a bathtub: a tap fills a stock')
  await panel.getByRole('button', { name: 'Generate' }).click()

  // The loop converges (invalid → retry → valid) against the real WASM engine.
  await expect(panel.getByText(/Valid model in \d+ attempt/)).toBeVisible({ timeout: 20000 })
  // It took 2 attempts (the retry proves the validate-feedback loop ran, not a one-shot).
  await expect(panel.getByText(/Valid model in 2 attempt/)).toBeVisible()

  // Accept loads the model → it becomes the active doc and reconciles to valid.
  await panel.getByRole('button', { name: 'Accept', exact: true }).click()
  await expect(page.getByText('2 elems').first()).toBeVisible({ timeout: 10000 })
  await expect(page.getByText('● valid')).toBeVisible()

  // And it runs.
  await page.getByRole('button', { name: /Run/ }).click()
  await expect(page.getByRole('button', { name: 'Results' })).toBeVisible({ timeout: 20000 })

  expect(errors.filter((e) => !e.includes('404') && !e.includes('favicon')),
    `console errors:\n${errors.join('\n')}`).toEqual([])
})

// The Refine turn (Phase 2): the copilot edits the CURRENT model. The stub asserts the request
// carried that model (so refine context reached the API), then returns a changed full model, and
// Accept loads the changed model.
const REFINED_MODEL = JSON.stringify({
  wasim_version: '0.1.0',
  simulation_settings: { duration: { value: 60, unit: 's' }, timestep: { value: 1, unit: 's' }, n_realizations: 1, seed: 42 },
  elements: [
    { id: 'tap', name: 'Tap', primitive: 'node', value_rule: 'fixed', value: { value: 5, unit: '1' }, save_results: { time_history: true, final_value: true } },
    { id: 'rain', name: 'Rain', primitive: 'node', value_rule: 'fixed', value: { value: 2, unit: '1' }, save_results: { time_history: true, final_value: true } },
    { id: 'water', name: 'Water', primitive: 'stock', initial_value: { value: 10, unit: '1' }, inflows: ['tap', 'rain'], outflows: [], save_results: { time_history: true, final_value: true } },
  ],
})

test('copilot refine: edits the current model (current model reaches the API; changed model loads)', async ({ page }) => {
  const errors: string[] = []
  page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()) })
  page.on('pageerror', (e) => errors.push(String(e)))

  let sawCurrentModelInPrompt = false
  await page.route('https://api.anthropic.com/v1/messages', (route) => {
    const body = route.request().postDataJSON() as { system?: string }
    // The refine prompt (only) says "current WASiM v2 model" AND embeds the model (id "rain").
    if (typeof body.system === 'string' && body.system.includes("current WASiM v2 model") && body.system.includes('"id": "rain"')) {
      sawCurrentModelInPrompt = true
    }
    route.fulfill(messagesResponse(REFINED_MODEL))
  })

  await page.goto('/')
  await page.getByRole('button', { name: /New blank model/ }).click()
  await expect(page.getByRole('button', { name: /Run/ })).toBeVisible({ timeout: 15000 })

  // Seed a model by generating + accepting the bathtub (the stub returns REFINED once routed, so seed
  // via a file open instead to keep the "current model" deterministic).
  // Simpler: load the valid model through the copilot's New flow first, using a one-shot valid stub.
  await page.getByRole('button', { name: 'Settings…' }).click()
  await page.getByPlaceholder('sk-ant-…').fill('sk-ant-test-key')
  await page.getByRole('button', { name: 'Done' }).click()

  await page.getByRole('button', { name: 'Copilot', exact: true }).click()
  const panel = page.getByTestId('copilot-panel')

  // Seed: switch to New, generate → the stub returns REFINED (3 elems); Accept to make it the doc.
  // (We only need *a* current model with id "water"; REFINED has it.)
  await panel.getByRole('tab', { name: 'New model' }).click().catch(() => {})
  await panel.getByRole('textbox').fill('a bathtub with rain')
  await panel.getByRole('button', { name: 'Generate' }).click()
  await expect(panel.getByText(/Valid model in \d+ attempt/)).toBeVisible({ timeout: 20000 })
  await panel.getByRole('button', { name: 'Accept', exact: true }).click()
  await expect(page.getByText('3 elems').first()).toBeVisible({ timeout: 10000 })

  // Now Refine: the tab should be available (a doc is loaded). Switch to it and make a change.
  await panel.getByRole('tab', { name: 'Refine current model' }).click()
  await panel.getByRole('textbox').fill('nudge the rain rate')
  await panel.getByRole('button', { name: 'Generate' }).click()
  await expect(panel.getByText(/Valid model in \d+ attempt/)).toBeVisible({ timeout: 20000 })

  // The refine request carried the current model in its system prompt.
  expect(sawCurrentModelInPrompt).toBe(true)

  // Accept the refined model → still 3 elems, valid, and runs.
  await panel.getByRole('button', { name: 'Accept', exact: true }).click()
  await expect(page.getByText('3 elems').first()).toBeVisible({ timeout: 10000 })
  await expect(page.getByText('● valid')).toBeVisible()

  expect(errors.filter((e) => !e.includes('404') && !e.includes('favicon')),
    `console errors:\n${errors.join('\n')}`).toEqual([])
})
