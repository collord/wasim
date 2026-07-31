import type { Validation } from '../worker/protocol'
import type { ModelDoc } from '../model/schema'
import { serializeModel } from '../model/edits'
import type { LlmConfig, ChatMessage, ContentBlock } from './providers/types'
import type { ToolDef, ChatResult } from './providers/tools'
import type { ApplyEditResult } from './tools'
import type { ProposeOptions } from './loop'

/**
 * The tool-dispatch loop (WASIM_AUTHORING_ENVIRONMENT_SPEC.md §17.3). Refine uses this instead of the
 * text loop: the model calls `apply_edit` for each change, we execute it against a working copy of
 * the doc, validate through the engine, and feed the result back — until the model stops calling
 * tools or the iteration cap. The engine stays the arbiter (a bad edit's diagnostics go back as a
 * tool result the model corrects). Deps are injectable so the loop is unit-testable with stubs.
 */

export interface AppliedEdit {
  op: string
  id?: string
  error?: string
  validation: Validation
}

export interface ToolProposeResult {
  /** The accumulated working doc (final). */
  doc: ModelDoc
  /** serializeModel(doc) — what Accept loads. */
  model: string
  /** Validation of the final doc. */
  validation: Validation
  /** LLM calls made. */
  iterations: number
  /** Full conversation (blocks), for debugging. */
  transcript: ChatMessage[]
  /** Ordered log of applied edits, for the diff preview. */
  edits: AppliedEdit[]
}

export interface ToolProposeDeps {
  call: (cfg: LlmConfig, system: string, messages: ChatMessage[], tools: ToolDef[], signal?: AbortSignal) => Promise<ChatResult>
  validate: (json: string) => Promise<Validation>
  execute: (name: string, input: Record<string, unknown>, doc: ModelDoc) => ApplyEditResult
}

/** The element id an apply_edit touched, for the edits log — location varies by op (add nests it
 *  under `element.id`, rename reports the `newId`, others use the top-level `id`). */
function editId(input: Record<string, unknown>): string | undefined {
  if (input.op === 'add') {
    const el = input.element as { id?: unknown } | undefined
    return typeof el?.id === 'string' ? el.id : undefined
  }
  if (input.op === 'rename' && typeof input.newId === 'string') return input.newId
  return typeof input.id === 'string' ? input.id : undefined
}

/** Tool-result text for a completed edit: the error if any, else a compact validation summary. */
function resultText(error: string | undefined, v: Validation): string {
  if (error) return `Edit rejected: ${error}`
  if (v.ok) return 'Applied. The model is valid.'
  const lines = v.issues.map((i) => `- ${i.severity}: ${i.message}${i.element_id ? ` (element: ${i.element_id})` : ''}`)
  return `Applied, but the model is now invalid — fix these:\n${lines.join('\n')}`
}

export async function proposeWithTools(
  instruction: string,
  startDoc: ModelDoc,
  cfg: LlmConfig,
  systemPrompt: string,
  tools: ToolDef[],
  deps: ToolProposeDeps,
  opts: ProposeOptions = {},
): Promise<ToolProposeResult> {
  const max = opts.maxIterations ?? 5
  const messages: ChatMessage[] = [{ role: 'user', content: instruction }]
  const edits: AppliedEdit[] = []
  let doc = startDoc
  let validation: Validation = { ok: true, issues: [], topo: [] }
  let iterations = 0

  for (let i = 1; i <= max; i++) {
    if (opts.signal?.aborted) throw new DOMException('Aborted', 'AbortError')
    opts.onIteration?.(i, null)

    iterations = i
    const res = await deps.call(cfg, systemPrompt, messages, tools, opts.signal)

    // Echo the assistant turn (text + tool_use blocks) so the next request is a valid continuation.
    const assistantBlocks: ContentBlock[] = [
      ...(res.text ? [{ type: 'text' as const, text: res.text }] : []),
      ...res.toolCalls.map((c) => ({ type: 'tool_use' as const, id: c.id, name: c.name, input: c.input })),
    ]
    messages.push({ role: 'assistant', content: assistantBlocks })

    // No tool calls → the model is done.
    if (res.stopReason === 'end' || res.toolCalls.length === 0) break

    // Execute each call sequentially against the working doc, validating after each so the model
    // gets per-edit feedback. One tool_result block per call, all in a single user turn.
    const resultBlocks: ContentBlock[] = []
    for (const call of res.toolCalls) {
      const { doc: nextDoc, error } = deps.execute(call.name, call.input, doc)
      if (!error) {
        doc = nextDoc
        validation = await deps.validate(serializeModel(doc))
      }
      edits.push({ op: String(call.input.op ?? call.name), id: editId(call.input), error, validation })
      resultBlocks.push({ type: 'tool_result', toolUseId: call.id, content: resultText(error, validation), isError: !!error })
    }
    messages.push({ role: 'user', content: resultBlocks })
    opts.onIteration?.(i, validation)
  }

  return { doc, model: serializeModel(doc), validation, iterations, transcript: messages, edits }
}
