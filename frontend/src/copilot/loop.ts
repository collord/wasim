import type { Validation } from '../worker/protocol'
import type { LlmConfig, ChatMessage } from './providers/types'
import { extractJson } from './extractJson'

/**
 * The copilot's agentic loop (WASIM_AUTHORING_ENVIRONMENT_SPEC.md §17.3): draft a model from a
 * natural-language description, validate it through the engine, and — if invalid — feed the
 * diagnostics back and let the model fix its own prior JSON, until valid or the iteration cap.
 * This is the same propose→validate loop the `claude -p` spike proved converges; here the engine
 * is reached via the WASM validator (`validateCandidate`) and the LLM via `callAnthropic`.
 *
 * Pure orchestration: the LLM call and the validator come in as `deps`, so the whole loop is
 * unit-testable with stubs and the e2e can drive the real WASM validator against a stubbed API.
 */

export interface ProposeResult {
  /** The final candidate JSON (best attempt, even if never valid). */
  model: string
  /** Its validation (ok true/false). */
  validation: Validation
  /** LLM calls made (1..maxIterations). */
  iterations: number
  /** Full conversation, for preview/debugging. */
  transcript: ChatMessage[]
}

export interface ProposeDeps {
  call: (cfg: LlmConfig, system: string, messages: ChatMessage[], signal?: AbortSignal) => Promise<string>
  validate: (json: string) => Promise<Validation>
}

export interface ProposeOptions {
  maxIterations?: number
  signal?: AbortSignal
  /** Progress callback: (attempt n, this attempt's validation once known, else null before it). */
  onIteration?: (n: number, validation: Validation | null) => void
}

/** Format engine diagnostics as a follow-up user message the LLM can act on. */
function diagnosticsMessage(v: Validation): string {
  const lines = v.issues.map((i) => `- ${i.severity}: ${i.message}${i.element_id ? ` (element: ${i.element_id})` : ''}`)
  return (
    'The model failed validation. Fix these issues and return the corrected full model as JSON:\n' +
    (lines.length ? lines.join('\n') : '(no specific diagnostics — the JSON may be malformed)')
  )
}

export async function propose(
  description: string,
  cfg: LlmConfig,
  systemPrompt: string,
  deps: ProposeDeps,
  opts: ProposeOptions = {},
): Promise<ProposeResult> {
  const max = opts.maxIterations ?? 5
  const messages: ChatMessage[] = [{ role: 'user', content: description }]
  let lastModel = ''
  let lastValidation: Validation = { ok: false, issues: [{ severity: 'error', message: 'no model produced' }], topo: [] }

  for (let i = 1; i <= max; i++) {
    if (opts.signal?.aborted) throw new DOMException('Aborted', 'AbortError')
    opts.onIteration?.(i, null)

    const raw = await deps.call(cfg, systemPrompt, messages, opts.signal)
    // Keep the assistant turn so the next iteration corrects *this* draft, not a fresh one.
    messages.push({ role: 'assistant', content: raw })

    let candidate: string
    try {
      candidate = extractJson(raw)
    } catch (e) {
      // No parseable JSON — treat as a validation failure and feed it back.
      const v: Validation = { ok: false, issues: [{ severity: 'error', message: (e as Error).message }], topo: [] }
      opts.onIteration?.(i, v)
      lastValidation = v
      messages.push({ role: 'user', content: diagnosticsMessage(v) })
      continue
    }

    const validation = await deps.validate(candidate)
    opts.onIteration?.(i, validation)
    lastModel = candidate
    lastValidation = validation

    if (validation.ok) return { model: candidate, validation, iterations: i, transcript: messages }
    messages.push({ role: 'user', content: diagnosticsMessage(validation) })
  }

  // Exhausted the cap without a clean model — return the best-effort attempt (Accept stays disabled).
  return { model: lastModel, validation: lastValidation, iterations: max, transcript: messages }
}
