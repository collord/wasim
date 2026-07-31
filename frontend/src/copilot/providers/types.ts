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
  | { provider: 'azure-openai'; deployment: string; apiKey: string; resource: string; apiVersion: string }

/** A neutral chat message. System is passed separately (not as a message) so each adapter can place
 *  it however its API expects (Anthropic top-level `system`; OpenAI/Azure a leading system message). */
export interface ChatMessage {
  role: 'user' | 'assistant'
  content: string
}

/** One completion call. Returns the assistant's text. Throws with the provider's error message on a
 *  non-2xx or network failure — the loop treats that as fatal. */
export type ChatAdapter = (
  cfg: LlmConfig,
  system: string,
  messages: ChatMessage[],
  signal?: AbortSignal,
) => Promise<string>

/** Shared max output tokens across adapters. */
export const MAX_TOKENS = 8000
