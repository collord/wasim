/**
 * The provider abstraction for the copilot (WASIM_AUTHORING_ENVIRONMENT_SPEC.md §17.1). One neutral
 * `chat()` interface with per-provider adapters, so the copilot loop (which only needs "send
 * messages, get text back") is provider-agnostic. Lowest-common-denominator by design: the interface
 * carries only what every provider shares (system, messages, model) — provider-specific knobs live
 * inside each adapter.
 */

export type ProviderId = 'anthropic' | 'openai' | 'azure-openai'

/**
 * Per-provider connection config (§17.1). A discriminated union on `provider`; the active variant is
 * what the store persists and the adapters consume.
 */
export type LlmConfig =
  | { provider: 'anthropic'; model: string; apiKey: string }
  | { provider: 'openai'; model: string; apiKey: string; baseUrl?: string }
  | {
      provider: 'azure-openai'
      deployment: string
      apiKey: string
      resource: string
      apiVersion: string
      /** New /openai/v1/ surface: the full base URL (e.g. https://{host}/openai/v1/). When set it
       *  overrides the classic {resource}.openai.azure.com/openai/deployments/… URL, and `model` (if
       *  set) is sent in the body. Leave empty to use the classic deployment URL. */
      endpoint?: string
      /** Model id for the v1 surface body (e.g. gpt-5-mini). Ignored on the classic surface, where
       *  the deployment name in the URL selects the model. */
      model?: string
    }

/** A content block within a message — text, or (for tool-calling turns) a tool call / tool result.
 *  A tool-call turn preserves the raw `id`+`input` each provider needs to reconstruct its native
 *  echo (Anthropic re-sends the `tool_use` block; OpenAI rebuilds `tool_calls` with `arguments`). */
export type ContentBlock =
  | { type: 'text'; text: string }
  | { type: 'tool_use'; id: string; name: string; input: Record<string, unknown> }
  | { type: 'tool_result'; toolUseId: string; content: string; isError?: boolean }

/** A neutral chat message. `content` is a plain string (text loop) or blocks (tool-calling turns).
 *  System is passed separately so each adapter can place it however its API expects (Anthropic
 *  top-level `system`; OpenAI/Azure a leading system message). */
export interface ChatMessage {
  role: 'user' | 'assistant'
  content: string | ContentBlock[]
}

/** Text-only completion (the original interface). Returns the assistant's text; throws with the
 *  provider's error message on a non-2xx or network failure. */
export type ChatAdapter = (
  cfg: LlmConfig,
  system: string,
  messages: ChatMessage[],
  signal?: AbortSignal,
) => Promise<string>

/** Tool-aware completion. Same as `ChatAdapter` plus a tool list; returns a normalized `ChatResult`
 *  (text + tool calls + stop reason). See `tools.ts`. */
export type ChatToolAdapter = (
  cfg: LlmConfig,
  system: string,
  messages: ChatMessage[],
  tools: import('./tools').ToolDef[],
  signal?: AbortSignal,
) => Promise<import('./tools').ChatResult>

/** Shared max output tokens across adapters. */
export const MAX_TOKENS = 8000
