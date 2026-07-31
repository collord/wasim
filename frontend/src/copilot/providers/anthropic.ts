import type { ChatAdapter, ChatMessage, LlmConfig } from './types'
import { MAX_TOKENS } from './types'

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
