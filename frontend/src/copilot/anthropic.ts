/**
 * Browser-direct Anthropic Messages API client (WASIM_AUTHORING_ENVIRONMENT_SPEC.md §17.1).
 * Raw `fetch` — no SDK dependency, matching the app's dependency-light, offline-capable style.
 * The user's key is sent straight from the browser to api.anthropic.com; the
 * `anthropic-dangerous-direct-browser-access` header opts into Anthropic's browser CORS path.
 *
 * Request surface for the current models is minimal by design: `{model, max_tokens, system,
 * messages}` plus optional adaptive `thinking`. Sampling params (`temperature`/`top_p`/`top_k`) and
 * `thinking:{type:'enabled',...}` are omitted — they 400 on claude-opus-4-8 (and the 4.7+/5 family).
 */

export interface AnthropicConfig {
  apiKey: string
  model: string
  /** Send adaptive extended thinking. Default off; a data-driven default lives in aiConfig. */
  thinking?: boolean
}

export interface ChatMessage {
  role: 'user' | 'assistant'
  content: string
}

const ENDPOINT = 'https://api.anthropic.com/v1/messages'
const MAX_TOKENS = 8000

/** One Messages API call. Returns the concatenated text of the response's text blocks. Throws with
 *  the API's error message on a non-2xx (or a network failure) — the loop treats that as fatal. */
export async function callAnthropic(
  cfg: AnthropicConfig,
  system: string,
  messages: ChatMessage[],
  signal?: AbortSignal,
): Promise<string> {
  const body: Record<string, unknown> = {
    model: cfg.model,
    max_tokens: MAX_TOKENS,
    system,
    messages,
  }
  if (cfg.thinking) body.thinking = { type: 'adaptive' }

  const res = await fetch(ENDPOINT, {
    method: 'POST',
    signal,
    headers: {
      'content-type': 'application/json',
      'x-api-key': cfg.apiKey,
      'anthropic-version': '2023-06-01',
      'anthropic-dangerous-direct-browser-access': 'true',
    },
    body: JSON.stringify(body),
  })

  if (!res.ok) {
    let detail = `HTTP ${res.status}`
    try {
      const err = (await res.json()) as { error?: { message?: string } }
      if (err.error?.message) detail = err.error.message
    } catch {
      /* non-JSON error body — keep the status */
    }
    throw new Error(`Anthropic API error: ${detail}`)
  }

  const data = (await res.json()) as { content?: { type: string; text?: string }[] }
  return (data.content ?? [])
    .filter((b) => b.type === 'text' && typeof b.text === 'string')
    .map((b) => b.text as string)
    .join('')
}
