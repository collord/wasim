import type { ChatAdapter, ChatToolAdapter, ChatMessage, ContentBlock, LlmConfig } from './types'
import { MAX_TOKENS } from './types'
import type { ChatResult, ToolCall } from './tools'

/**
 * OpenAI Chat Completions + Azure OpenAI adapters (§17.1). Both use the same request body and
 * response shape (Azure's REST surface is OpenAI's); they differ only in endpoint URL and auth
 * header — OpenAI: `Authorization: Bearer` at api.openai.com; Azure: `api-key` at a deployment URL.
 *
 * Azure (v1 surface) is LIVE-VERIFIED — the chat + tool round-trip was exercised against a real
 * Azure OpenAI resource (gpt-5-mini on the /openai/v1/ endpoint), which is why the adapter sends
 * `max_completion_tokens` (reasoning models reject `max_tokens`) and resolves both URL surfaces.
 * The plain OpenAI (api.openai.com) path is still built-to-format only, not live-called (no key).
 */

/** OpenAI-family request body. The system prompt becomes a leading `system` message.
 *  Uses `max_completion_tokens` (not the deprecated `max_tokens`, which the reasoning models
 *  — gpt-5* etc. — reject with a 400). */
function buildBody(model: string, system: string, messages: ChatMessage[]) {
  const withSystem = system ? [{ role: 'system' as const, content: system }, ...messages] : messages
  return { model, max_completion_tokens: MAX_TOKENS, messages: withSystem }
}

/** Resolve the Azure endpoint URL + body model for both surfaces (§17.1). New `/openai/v1/` surface
 *  (when `endpoint` is set): `{endpoint}/chat/completions`, body model = cfg.model. Classic surface:
 *  the deployment URL with `?api-version=`, body model = deployment name (URL selects the model). */
function azureTarget(cfg: Extract<LlmConfig, { provider: 'azure-openai' }>): { url: string; model: string } {
  if (cfg.endpoint && cfg.endpoint.trim()) {
    const base = cfg.endpoint.trim().replace(/\/$/, '')
    return { url: `${base}/chat/completions`, model: cfg.model?.trim() || cfg.deployment }
  }
  return {
    url:
      `https://${cfg.resource}.openai.azure.com/openai/deployments/${cfg.deployment}` +
      `/chat/completions?api-version=${encodeURIComponent(cfg.apiVersion)}`,
    model: cfg.deployment, // classic: the URL selects the model; body model is cosmetic
  }
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
  const { url, model } = azureTarget(cfg)
  return post(url, { 'api-key': cfg.apiKey }, buildBody(model, system, messages), signal, 'Azure OpenAI')
}

// ── Tool-aware variants (§17.3) ───────────────────────────────────────────────────
// OpenAI/Azure represent tool turns differently from Anthropic: the assistant echo carries a
// `tool_calls` array (arguments as a JSON string), and each tool result is its OWN `role:'tool'`
// message keyed by `tool_call_id` — so one neutral tool-result turn FANS OUT to N messages.

interface OpenAiMessage {
  role: string
  content: string | null
  tool_calls?: { id: string; type: 'function'; function: { name: string; arguments: string } }[]
  tool_call_id?: string
}

/** Serialize a neutral message to one or more OpenAI-family messages. A tool-result turn fans out. */
function toOpenAiMessages(m: ChatMessage): OpenAiMessage[] {
  if (typeof m.content === 'string') return [{ role: m.role, content: m.content }]

  const toolResults = m.content.filter((b): b is Extract<ContentBlock, { type: 'tool_result' }> => b.type === 'tool_result')
  if (toolResults.length > 0) {
    // A user turn of tool_result blocks → one `role:'tool'` message per result.
    return toolResults.map((b) => ({ role: 'tool', tool_call_id: b.toolUseId, content: b.content }))
  }

  // An assistant turn: text (may be null) + a `tool_calls` array reconstructed from tool_use blocks.
  const text = m.content.filter((b): b is Extract<ContentBlock, { type: 'text' }> => b.type === 'text').map((b) => b.text).join('')
  const toolUses = m.content.filter((b): b is Extract<ContentBlock, { type: 'tool_use' }> => b.type === 'tool_use')
  const msg: OpenAiMessage = { role: m.role, content: text || null }
  if (toolUses.length > 0) {
    msg.tool_calls = toolUses.map((b) => ({
      id: b.id,
      type: 'function',
      function: { name: b.name, arguments: JSON.stringify(b.input) },
    }))
  }
  return [msg]
}

/** OpenAI-family tool body: prepend system, flat-map neutral messages, add `function`-typed tools. */
function buildToolBody(model: string, system: string, messages: ChatMessage[], tools: import('./tools').ToolDef[]) {
  const base: OpenAiMessage[] = system ? [{ role: 'system', content: system }] : []
  const msgs = [...base, ...messages.flatMap(toOpenAiMessages)]
  return {
    model,
    max_completion_tokens: MAX_TOKENS,
    messages: msgs,
    tools: tools.map((t) => ({ type: 'function', function: { name: t.name, description: t.description, parameters: t.inputSchema } })),
  }
}

/** Normalize an OpenAI-family response to a ChatResult. Malformed tool `arguments` become an empty
 *  input rather than throwing (the loop feeds the resulting validation error back to the model). */
function parseToolResult(data: unknown): ChatResult {
  const d = data as {
    choices?: {
      message?: { content?: string | null; tool_calls?: { id: string; function: { name: string; arguments: string } }[] }
      finish_reason?: string
    }[]
  }
  const choice = d.choices?.[0]
  const msg = choice?.message
  const toolCalls: ToolCall[] = (msg?.tool_calls ?? []).map((tc) => {
    let input: Record<string, unknown> = {}
    try {
      input = JSON.parse(tc.function.arguments || '{}')
    } catch {
      /* malformed args → empty input; validate will surface the resulting model error */
    }
    return { id: tc.id, name: tc.function.name, input }
  })
  return { text: msg?.content ?? '', toolCalls, stopReason: choice?.finish_reason === 'tool_calls' ? 'tool_use' : 'end' }
}

async function postTools(url: string, headers: Record<string, string>, body: unknown, signal: AbortSignal | undefined, label: string): Promise<ChatResult> {
  const res = await fetch(url, {
    method: 'POST',
    signal,
    headers: { 'content-type': 'application/json', ...headers },
    body: JSON.stringify(body),
  })
  if (!res.ok) throw new Error(`${label} API error: ${await errorDetail(res)}`)
  return parseToolResult(await res.json())
}

export const chatOpenAITools: ChatToolAdapter = async (cfg, system, messages, tools, signal) => {
  if (cfg.provider !== 'openai') throw new Error('chatOpenAITools called with non-openai config')
  const base = (cfg.baseUrl ?? 'https://api.openai.com/v1').replace(/\/$/, '')
  return postTools(`${base}/chat/completions`, { authorization: `Bearer ${cfg.apiKey}` }, buildToolBody(cfg.model, system, messages, tools), signal, 'OpenAI')
}

export const chatAzureTools: ChatToolAdapter = async (cfg, system, messages, tools, signal) => {
  if (cfg.provider !== 'azure-openai') throw new Error('chatAzureTools called with non-azure config')
  const { url, model } = azureTarget(cfg)
  return postTools(url, { 'api-key': cfg.apiKey }, buildToolBody(model, system, messages, tools), signal, 'Azure OpenAI')
}
