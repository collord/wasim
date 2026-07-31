/**
 * Provider-neutral tool-calling vocabulary (WASIM_AUTHORING_ENVIRONMENT_SPEC.md §17.3). The copilot
 * loop speaks these types; each adapter translates them to/from its native wire format (Anthropic
 * `tool_use`/`tool_result` content blocks; OpenAI/Azure `tool_calls` + `role:'tool'` messages). This
 * is what lets a tool like `apply_edit` be defined once and work across providers.
 */

/** A tool the model may call. `inputSchema` is a JSON Schema object both providers accept. */
export interface ToolDef {
  name: string
  description: string
  /** JSON Schema (`{type:'object', …}`). Anthropic sends it as `input_schema`; OpenAI as
   *  `function.parameters`. */
  inputSchema: Record<string, unknown>
}

/** One model-issued tool call, normalized. `input` is the parsed arguments object. `id` is opaque
 *  and MUST round-trip verbatim (both providers match the tool result to the call by id). */
export interface ToolCall {
  id: string
  name: string
  input: Record<string, unknown>
}

/** One completion, normalized. `stopReason:'tool_use'` means the model wants tools run and continued;
 *  `'end'` means it's done (`toolCalls` empty). */
export interface ChatResult {
  text: string
  toolCalls: ToolCall[]
  stopReason: 'end' | 'tool_use'
}
