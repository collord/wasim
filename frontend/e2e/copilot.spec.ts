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
  // The provider picker defaults to Anthropic (multi-provider abstraction, §17.1).
  await expect(page.getByRole('combobox').filter({ hasText: 'Anthropic' })).toBeVisible()
  await page.getByPlaceholder('sk-ant-…').fill('sk-ant-test-key')
  await page.getByRole('button', { name: 'Done' }).click()

  // Open the Copilot tab, describe a model, generate.
  await page.getByRole('button', { name: 'Copilot', exact: true }).click()
  const panel = page.getByTestId('copilot-panel')
  // A blank doc is loaded, so the panel defaults to Refine; switch to New for the cold-start path.
  await panel.getByRole('tab', { name: 'New model' }).click()
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

// The Refine turn (§17.3): the copilot calls the apply_edit TOOL to edit the current model. This
// test seeds a 2-element model (cold-start text), switches to Refine, and stubs an apply_edit(add)
// tool call → the real edits.ts adds the element and real WASM validates → then end_turn → Accept
// loads the 3-element result. It also asserts the tool round-trip: the 2nd request must echo the
// tool_use assistant turn + a tool_result user turn.

/** A tool_use response for apply_edit(add rain). */
function toolUseResponse() {
  return {
    status: 200,
    contentType: 'application/json',
    body: JSON.stringify({
      id: 'msg_tool', type: 'message', role: 'assistant', stop_reason: 'tool_use',
      content: [{
        type: 'tool_use', id: 'tu_add_rain', name: 'apply_edit',
        input: {
          op: 'add',
          element: { id: 'rain', name: 'Rain', primitive: 'node', value_rule: 'fixed', value: { value: 2, unit: '1' }, save_results: { time_history: true, final_value: true } },
        },
      }],
    }),
  }
}

test('copilot refine: apply_edit tool round-trip (edits current model via WASM, echoes tool turns)', async ({ page }) => {
  const errors: string[] = []
  page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()) })
  page.on('pageerror', (e) => errors.push(String(e)))

  let refineCall = 0
  let echoedToolTurns = false
  await page.route('https://api.anthropic.com/v1/messages', (route) => {
    const body = route.request().postDataJSON() as { tools?: unknown[]; messages?: { role: string; content: unknown }[] }
    if (!body.tools) {
      // Cold-start (no tools) → the seed model (VALID_MODEL: tap + water).
      route.fulfill(messagesResponse(VALID_MODEL))
      return
    }
    // Refine (tools present).
    refineCall += 1
    if (refineCall === 1) {
      route.fulfill(toolUseResponse()) // ask to add rain
      return
    }
    // 2nd refine request must echo the tool_use assistant turn + a tool_result user turn.
    const s = JSON.stringify(body.messages ?? [])
    if (s.includes('"type":"tool_use"') && s.includes('"tool_use_id":"tu_add_rain"')) echoedToolTurns = true
    route.fulfill(messagesResponse('done')) // a final text turn (stop_reason end_turn) → no more tools
  })

  await page.goto('/')
  await page.getByRole('button', { name: /New blank model/ }).click()
  await expect(page.getByRole('button', { name: /Run/ })).toBeVisible({ timeout: 15000 })

  await page.getByRole('button', { name: 'Settings…' }).click()
  await page.getByPlaceholder('sk-ant-…').fill('sk-ant-test-key')
  await page.getByRole('button', { name: 'Done' }).click()

  await page.getByRole('button', { name: 'Copilot', exact: true }).click()
  const panel = page.getByTestId('copilot-panel')

  // Seed: cold-start the 2-element bathtub, Accept to make it the current doc.
  // A blank doc is loaded so the panel defaults to Refine; switch to New for the cold-start seed.
  await panel.getByRole('tab', { name: 'New model' }).click()
  await panel.getByRole('textbox').fill('a bathtub')
  await panel.getByRole('button', { name: 'Generate' }).click()
  await expect(panel.getByText(/Valid model in \d+ attempt/)).toBeVisible({ timeout: 20000 })
  await panel.getByRole('button', { name: 'Accept', exact: true }).click()
  await expect(page.getByText('2 elems').first()).toBeVisible({ timeout: 10000 })

  // Refine via the apply_edit tool: add a rain inflow.
  await panel.getByRole('tab', { name: 'Refine current model' }).click()
  await panel.getByRole('textbox').fill('add a rain inflow')
  await panel.getByRole('button', { name: 'Generate' }).click()
  // The edits log shows the applied add.
  await expect(panel.getByText(/add rain · valid/)).toBeVisible({ timeout: 20000 })
  await expect(panel.getByText(/Valid model in \d+ attempt/)).toBeVisible()

  // The tool round-trip happened: the 2nd request echoed the tool turns.
  expect(echoedToolTurns).toBe(true)

  // Accept → the doc now has 3 elements (rain added) and is valid + runs.
  await panel.getByRole('button', { name: 'Accept', exact: true }).click()
  await expect(page.getByText('3 elems').first()).toBeVisible({ timeout: 10000 })
  await expect(page.getByText('● valid')).toBeVisible()

  expect(errors.filter((e) => !e.includes('404') && !e.includes('favicon')),
    `console errors:\n${errors.join('\n')}`).toEqual([])
})
