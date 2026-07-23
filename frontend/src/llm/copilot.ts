// The interactive copilot loop (spec §17.3): the LLM proposes a model, the engine validates it,
// errors are fed back, and it iterates until clean. The engine — not the LLM — is the gate, so a
// hallucinated field/ref/unit is caught by validate() and corrected in-loop, never shipped.

import type { Validation } from '../worker/protocol'
import type { ChatMessage, Provider, ToolDef } from './provider'
import { buildAuthoringGuide } from './guide'

const TOOLS: ToolDef[] = [
  {
    name: 'propose_model',
    description: 'Submit a complete candidate WASiM model.json. It will be parsed and validated by the engine; you receive the diagnostics back.',
    inputSchema: {
      type: 'object',
      properties: {
        model_json: { type: 'string', description: 'The full model.json document as a JSON string.' },
        rationale: { type: 'string', description: 'One paragraph explaining the model.' },
      },
      required: ['model_json'],
    },
  },
]

export interface CopilotEvent {
  kind: 'thinking' | 'proposal' | 'error' | 'done'
  text?: string
  modelJson?: string
  validation?: Validation
}

export interface CopilotResult {
  modelJson: string | null
  rationale: string
  iterations: number
  finalValidation: Validation | null
}

const MAX_ITERATIONS = 6

/**
 * Run the cold-start / refinement loop. `validate` is injected (backed by the engine worker) so
 * the loop stays engine-truthful. `onEvent` streams progress for the UI. Returns the last
 * validated model (or the last proposal if it never fully validated within the cap).
 */
export async function runCopilot(opts: {
  provider: Provider
  userMessage: string
  currentModel?: string | null
  history?: ChatMessage[]
  validate: (json: string) => Promise<Validation>
  onEvent?: (e: CopilotEvent) => void
  signal?: AbortSignal
}): Promise<CopilotResult> {
  const { provider, userMessage, currentModel, validate, onEvent, signal } = opts
  const system = buildAuthoringGuide()

  const messages: ChatMessage[] = [...(opts.history ?? [])]
  const preamble = currentModel
    ? `The current model.json is:\n\`\`\`json\n${currentModel}\n\`\`\`\n\nUser request: ${userMessage}`
    : `User request: ${userMessage}`
  messages.push({ role: 'user', content: preamble })

  let lastModel: string | null = null
  let lastValidation: Validation | null = null
  let rationale = ''

  for (let i = 0; i < MAX_ITERATIONS; i++) {
    if (signal?.aborted) break
    onEvent?.({ kind: 'thinking', text: `Turn ${i + 1}…` })
    const result = await provider.chat(messages, TOOLS, { system, signal })
    if (result.text) rationale = result.text

    const propose = result.toolCalls.find((t) => t.name === 'propose_model')
    if (!propose) {
      // No tool call — the model answered in prose (explain/review turn) or is done.
      onEvent?.({ kind: 'done', text: result.text })
      return { modelJson: lastModel, rationale: result.text || rationale, iterations: i + 1, finalValidation: lastValidation }
    }

    const input = propose.input as { model_json?: string; rationale?: string }
    const modelJson = input.model_json ?? ''
    if (input.rationale) rationale = input.rationale
    lastModel = modelJson

    // Engine truth: validate the candidate and feed diagnostics back.
    const validation = await validate(modelJson)
    lastValidation = validation
    onEvent?.({ kind: 'proposal', modelJson, validation })

    // Record the assistant's tool call + our tool result, then loop.
    messages.push({ role: 'assistant', content: result.text, toolCalls: [propose] })
    if (validation.ok) {
      messages.push({ role: 'tool', toolCallId: propose.id, content: 'VALID: the model parsed and its graph built with no errors. Provide a final one-paragraph rationale and stop.' })
      // One more turn to let it produce a closing rationale, but we already have a valid model.
      onEvent?.({ kind: 'done', text: rationale })
      return { modelJson, rationale, iterations: i + 1, finalValidation: validation }
    }
    const errs = validation.issues.filter((x) => x.severity === 'error').map((x) => `- ${x.message}`).join('\n')
    const warns = validation.issues.filter((x) => x.severity === 'warning').map((x) => `- ${x.message}`).join('\n')
    onEvent?.({ kind: 'error', validation })
    messages.push({
      role: 'tool',
      toolCallId: propose.id,
      content: `INVALID. Fix these and call propose_model again with a corrected model_json.\nErrors:\n${errs || '(none)'}\nWarnings:\n${warns || '(none)'}`,
    })
  }

  return { modelJson: lastModel, rationale, iterations: MAX_ITERATIONS, finalValidation: lastValidation }
}
