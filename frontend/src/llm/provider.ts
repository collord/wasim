// Provider abstraction (spec §17.1): normalizes Anthropic and OpenAI to one `chat(messages,
// tools)` call returning text and/or tool calls, so the copilot loop is provider-agnostic.
// Browser-direct calls (BYO-key, no server) require each provider's browser-access opt-in.

import type { LlmConfig } from './config'

export interface ChatMessage {
  role: 'user' | 'assistant' | 'tool'
  content: string
  /** For role 'tool': the id of the tool call this result answers. */
  toolCallId?: string
  /** For role 'assistant': tool calls the model requested. */
  toolCalls?: ToolCall[]
}

export interface ToolDef {
  name: string
  description: string
  inputSchema: object
}

export interface ToolCall {
  id: string
  name: string
  input: unknown
}

export interface ChatResult {
  text: string
  toolCalls: ToolCall[]
}

export interface ChatOptions {
  system: string
  maxTokens?: number
  signal?: AbortSignal
}

export interface Provider {
  chat(messages: ChatMessage[], tools: ToolDef[], opts: ChatOptions): Promise<ChatResult>
}

/** A provider injected on `window` for tests / headless use (no network). */
interface StubHook {
  chat: (messages: ChatMessage[], tools: ToolDef[], opts: ChatOptions) => Promise<ChatResult>
}

export function getProvider(cfg: LlmConfig): Provider {
  const stub = (window as unknown as { __wasimLlmProvider?: StubHook }).__wasimLlmProvider
  if (stub) return { chat: (m, t, o) => stub.chat(m, t, o) }
  return cfg.provider === 'anthropic' ? new AnthropicProvider(cfg) : new OpenAIProvider(cfg)
}

// ── Anthropic (Messages API) ────────────────────────────────────────────────────

class AnthropicProvider implements Provider {
  constructor(private cfg: LlmConfig) {}

  async chat(messages: ChatMessage[], tools: ToolDef[], opts: ChatOptions): Promise<ChatResult> {
    const body = {
      model: this.cfg.model,
      max_tokens: opts.maxTokens ?? 4096,
      system: opts.system,
      tools: tools.map((t) => ({ name: t.name, description: t.description, input_schema: t.inputSchema })),
      messages: messages.map((m) => toAnthropicMessage(m)),
    }
    const res = await fetch('https://api.anthropic.com/v1/messages', {
      method: 'POST',
      headers: {
        'content-type': 'application/json',
        'x-api-key': this.cfg.apiKey,
        'anthropic-version': '2023-06-01',
        'anthropic-dangerous-direct-browser-access': 'true',
      },
      body: JSON.stringify(body),
      signal: opts.signal,
    })
    if (!res.ok) throw new Error(`Anthropic ${res.status}: ${await res.text()}`)
    const data = await res.json()
    let text = ''
    const toolCalls: ToolCall[] = []
    for (const block of data.content ?? []) {
      if (block.type === 'text') text += block.text
      else if (block.type === 'tool_use') toolCalls.push({ id: block.id, name: block.name, input: block.input })
    }
    return { text, toolCalls }
  }
}

function toAnthropicMessage(m: ChatMessage): unknown {
  if (m.role === 'tool') {
    return { role: 'user', content: [{ type: 'tool_result', tool_use_id: m.toolCallId, content: m.content }] }
  }
  if (m.role === 'assistant' && m.toolCalls?.length) {
    const content: unknown[] = []
    if (m.content) content.push({ type: 'text', text: m.content })
    for (const tc of m.toolCalls) content.push({ type: 'tool_use', id: tc.id, name: tc.name, input: tc.input })
    return { role: 'assistant', content }
  }
  return { role: m.role, content: m.content }
}

// ── OpenAI (Chat Completions) ───────────────────────────────────────────────────

class OpenAIProvider implements Provider {
  constructor(private cfg: LlmConfig) {}

  async chat(messages: ChatMessage[], tools: ToolDef[], opts: ChatOptions): Promise<ChatResult> {
    const base = this.cfg.baseUrl?.replace(/\/$/, '') ?? 'https://api.openai.com/v1'
    const msgs: unknown[] = [{ role: 'system', content: opts.system }]
    for (const m of messages) {
      if (m.role === 'tool') msgs.push({ role: 'tool', tool_call_id: m.toolCallId, content: m.content })
      else if (m.role === 'assistant' && m.toolCalls?.length) {
        msgs.push({
          role: 'assistant',
          content: m.content || null,
          tool_calls: m.toolCalls.map((tc) => ({ id: tc.id, type: 'function', function: { name: tc.name, arguments: JSON.stringify(tc.input) } })),
        })
      } else msgs.push({ role: m.role, content: m.content })
    }
    const res = await fetch(`${base}/chat/completions`, {
      method: 'POST',
      headers: { 'content-type': 'application/json', authorization: `Bearer ${this.cfg.apiKey}` },
      body: JSON.stringify({
        model: this.cfg.model,
        max_tokens: opts.maxTokens ?? 4096,
        messages: msgs,
        tools: tools.map((t) => ({ type: 'function', function: { name: t.name, description: t.description, parameters: t.inputSchema } })),
      }),
      signal: opts.signal,
    })
    if (!res.ok) throw new Error(`OpenAI ${res.status}: ${await res.text()}`)
    const data = await res.json()
    const msg = data.choices?.[0]?.message ?? {}
    const toolCalls: ToolCall[] = (msg.tool_calls ?? []).map((tc: { id: string; function: { name: string; arguments: string } }) => ({
      id: tc.id,
      name: tc.function.name,
      input: safeParse(tc.function.arguments),
    }))
    return { text: msg.content ?? '', toolCalls }
  }
}

function safeParse(s: string): unknown {
  try {
    return JSON.parse(s)
  } catch {
    return {}
  }
}
