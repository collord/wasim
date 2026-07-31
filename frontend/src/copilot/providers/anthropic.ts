import type { ChatAdapter, ChatToolAdapter, ChatMessage, ContentBlock, LlmConfig } from './types'
import { MAX_TOKENS } from './types'
import type { ChatResult, ToolCall } from './tools'

/**
 * Anthropic Messages API adapter (§17.1). Raw `fetch` — no SDK dependency. The user's key goes
 * straight from the browser to api.anthropic.com; `anthropic-dangerous-direct-browser-access` opts
 * into Anthropic's browser CORS path. Minimal request surface — no sampling params (they 400 on the
 * opus-4-8 / 4.7+/5 family) and no thinking (dropped with the provider abstraction's LCD interface).
 * This is the only live-verified adapter in this codebase.
 */

const ENDPOINT = 'https://api.anthropic.com/v1/messages'

export const chatAnthropic: ChatAdapter = async (cfg, system, messages, signal) => {
  if (cfg.provider !== 'anthropic') throw new Error('chatAnthropic called with non-anthropic config')
  const res = await fetch(ENDPOINT, {
    method: 'POST',
    signal,
    headers: {
      'content-type': 'application/json',
      'x-api-key': cfg.apiKey,
      'anthropic-version': '2023-06-01',
      'anthropic-dangerous-direct-browser-access': 'true',
    },
    body: JSON.stringify({ model: cfg.model, max_tokens: MAX_TOKENS, system, messages }),
  })

  if (!res.ok) throw new Error(`Anthropic API error: ${await errorDetail(res)}`)

  const data = (await res.json()) as { content?: { type: string; text?: string }[] }
  return (data.content ?? [])
    .filter((b) => b.type === 'text' && typeof b.text === 'string')
    .map((b) => b.text as string)
    .join('')
}

/** Serialize a neutral message to Anthropic wire content (string passes through; blocks map 1:1). */
function toAnthropicMessage(m: ChatMessage): { role: string; content: unknown } {
  if (typeof m.content === 'string') return { role: m.role, content: m.content }
  const content = m.content.map((b: ContentBlock) => {
    if (b.type === 'text') return { type: 'text', text: b.text }
    if (b.type === 'tool_use') return { type: 'tool_use', id: b.id, name: b.name, input: b.input }
    return { type: 'tool_result', tool_use_id: b.toolUseId, content: b.content, ...(b.isError ? { is_error: true } : {}) }
  })
  return { role: m.role, content }
}

/** Tool-aware Anthropic completion. Sends `tools` (as `input_schema`), serializes tool-call/result
 *  turns, and normalizes the response's `content` blocks + `stop_reason` into a ChatResult. */
export const chatAnthropicTools: ChatToolAdapter = async (cfg, system, messages, tools, signal) => {
  if (cfg.provider !== 'anthropic') throw new Error('chatAnthropicTools called with non-anthropic config')
  const res = await fetch(ENDPOINT, {
    method: 'POST',
    signal,
    headers: {
      'content-type': 'application/json',
      'x-api-key': cfg.apiKey,
      'anthropic-version': '2023-06-01',
      'anthropic-dangerous-direct-browser-access': 'true',
    },
    body: JSON.stringify({
      model: cfg.model,
      max_tokens: MAX_TOKENS,
      system,
      messages: messages.map(toAnthropicMessage),
      tools: tools.map((t) => ({ name: t.name, description: t.description, input_schema: t.inputSchema })),
    }),
  })

  if (!res.ok) throw new Error(`Anthropic API error: ${await errorDetail(res)}`)

  const data = (await res.json()) as {
    content?: ({ type: 'text'; text: string } | { type: 'tool_use'; id: string; name: string; input: Record<string, unknown> })[]
    stop_reason?: string
  }
  const blocks = data.content ?? []
  const text = blocks.filter((b): b is { type: 'text'; text: string } => b.type === 'text').map((b) => b.text).join('')
  const toolCalls: ToolCall[] = blocks
    .filter((b): b is { type: 'tool_use'; id: string; name: string; input: Record<string, unknown> } => b.type === 'tool_use')
    .map((b) => ({ id: b.id, name: b.name, input: b.input }))
  const result: ChatResult = { text, toolCalls, stopReason: data.stop_reason === 'tool_use' ? 'tool_use' : 'end' }
  return result
}

/** Extract a human-readable error from a non-2xx Anthropic response. */
async function errorDetail(res: Response): Promise<string> {
  try {
    const err = (await res.json()) as { error?: { message?: string } }
    if (err.error?.message) return err.error.message
  } catch {
    /* non-JSON body */
  }
  return `HTTP ${res.status}`
}

// Re-exported for adapters/tests that build Anthropic requests.
export type { ChatMessage, LlmConfig }
