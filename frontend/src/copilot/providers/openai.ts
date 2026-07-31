import type { ChatAdapter, ChatMessage } from './types'
import { MAX_TOKENS } from './types'

/**
 * OpenAI Chat Completions + Azure OpenAI adapters (§17.1). Both use the same request body and
 * response shape (Azure's REST surface is OpenAI's); they differ only in endpoint URL and auth
 * header — OpenAI: `Authorization: Bearer` at api.openai.com; Azure: `api-key` at a deployment URL.
 *
 * NOT LIVE-VERIFIED. Built to the confirmed wire formats and unit-tested for request-shaping and
 * response-parsing against synthetic payloads, but not called against a real OpenAI/Azure endpoint
 * (no keys available at build time). The Anthropic adapter is the only end-to-end-verified path.
 */

/** OpenAI-family request body. The system prompt becomes a leading `system` message. */
function buildBody(model: string, system: string, messages: ChatMessage[]) {
  const withSystem = system ? [{ role: 'system' as const, content: system }, ...messages] : messages
  return { model, max_tokens: MAX_TOKENS, messages: withSystem }
}

/** OpenAI-family response text: `choices[0].message.content`. */
function parseText(data: unknown): string {
  const d = data as { choices?: { message?: { content?: string } }[] }
  return d.choices?.[0]?.message?.content ?? ''
}

/** Human-readable error from a non-2xx OpenAI/Azure response (`error.message`). */
async function errorDetail(res: Response): Promise<string> {
  try {
    const err = (await res.json()) as { error?: { message?: string } }
    if (err.error?.message) return err.error.message
  } catch {
    /* non-JSON body */
  }
  return `HTTP ${res.status}`
}

async function post(url: string, headers: Record<string, string>, body: unknown, signal?: AbortSignal, label = 'OpenAI') {
  const res = await fetch(url, {
    method: 'POST',
    signal,
    headers: { 'content-type': 'application/json', ...headers },
    body: JSON.stringify(body),
  })
  if (!res.ok) throw new Error(`${label} API error: ${await errorDetail(res)}`)
  return parseText(await res.json())
}

export const chatOpenAI: ChatAdapter = async (cfg, system, messages, signal) => {
  if (cfg.provider !== 'openai') throw new Error('chatOpenAI called with non-openai config')
  const base = (cfg.baseUrl ?? 'https://api.openai.com/v1').replace(/\/$/, '')
  return post(
    `${base}/chat/completions`,
    { authorization: `Bearer ${cfg.apiKey}` },
    buildBody(cfg.model, system, messages),
    signal,
    'OpenAI',
  )
}

export const chatAzure: ChatAdapter = async (cfg, system, messages, signal) => {
  if (cfg.provider !== 'azure-openai') throw new Error('chatAzure called with non-azure config')
  const url =
    `https://${cfg.resource}.openai.azure.com/openai/deployments/${cfg.deployment}` +
    `/chat/completions?api-version=${encodeURIComponent(cfg.apiVersion)}`
  // Azure's body takes `deployment` implicitly via the URL, so `model` in the body is ignored — send
  // the deployment name as the model for clarity.
  return post(url, { 'api-key': cfg.apiKey }, buildBody(cfg.deployment, system, messages), signal, 'Azure OpenAI')
}
