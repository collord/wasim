import { describe, it, expect, vi, afterEach } from 'vitest'
import type { LlmConfig, ChatMessage } from './types'
import type { ToolDef } from './tools'
import { chatWithTools } from './index'

/**
 * Tool-calling request-shaping + response-parsing for all three providers, via a stubbed `fetch`.
 * The load-bearing, provider-divergent part is the ECHO — re-sending an assistant tool-call turn and
 * a tool-result turn on a follow-up request — so these assert the *serialized messages*, not just
 * the first request. Anthropic is live-verified elsewhere (e2e); OpenAI/Azure are unit-only.
 */

interface Captured {
  url: string
  headers: Record<string, string>
  body: { messages: unknown[]; tools?: unknown[]; model?: string; max_completion_tokens?: number; max_tokens?: number }
}

function stubFetch(response: unknown, ok = true, status = 200): { captured: Captured | null } {
  const box: { captured: Captured | null } = { captured: null }
  vi.stubGlobal('fetch', async (url: string, init: RequestInit) => {
    box.captured = { url, headers: init.headers as Record<string, string>, body: JSON.parse(init.body as string) }
    return { ok, status, json: async () => response } as Response
  })
  return box
}

afterEach(() => vi.unstubAllGlobals())

const TOOL: ToolDef = { name: 'apply_edit', description: 'edit', inputSchema: { type: 'object', properties: {} } }
const ANTHROPIC: LlmConfig = { provider: 'anthropic', model: 'claude-opus-4-8', apiKey: 'k' }
const OPENAI: LlmConfig = { provider: 'openai', model: 'gpt-4o', apiKey: 'k' }
const AZURE: LlmConfig = { provider: 'azure-openai', deployment: 'd', apiKey: 'k', resource: 'r', apiVersion: '2024-06-01' }

// An echo conversation: user → assistant(tool_use) → user(tool_result).
const ECHO_MESSAGES: ChatMessage[] = [
  { role: 'user', content: 'add a rain inflow' },
  { role: 'assistant', content: [{ type: 'tool_use', id: 'tu_1', name: 'apply_edit', input: { op: 'add' } }] },
  { role: 'user', content: [{ type: 'tool_result', toolUseId: 'tu_1', content: 'Applied. Valid.' }] },
]

describe('Anthropic tool adapter', () => {
  it('shapes tools as input_schema and parses tool_use', async () => {
    const box = stubFetch({ content: [{ type: 'tool_use', id: 'tu_9', name: 'apply_edit', input: { op: 'modify', id: 'x' } }], stop_reason: 'tool_use' })
    const r = await chatWithTools(ANTHROPIC, 'SYS', [{ role: 'user', content: 'hi' }], [TOOL])
    expect(box.captured!.body.tools).toEqual([{ name: 'apply_edit', description: 'edit', input_schema: { type: 'object', properties: {} } }])
    expect(r.stopReason).toBe('tool_use')
    expect(r.toolCalls).toEqual([{ id: 'tu_9', name: 'apply_edit', input: { op: 'modify', id: 'x' } }])
  })

  it('echoes tool_use + tool_result blocks in native Anthropic shape', async () => {
    const box = stubFetch({ content: [{ type: 'text', text: 'done' }], stop_reason: 'end_turn' })
    const r = await chatWithTools(ANTHROPIC, 'SYS', ECHO_MESSAGES, [TOOL])
    expect(r.stopReason).toBe('end')
    expect(r.text).toBe('done')
    // The assistant turn re-sends the tool_use block; the user turn re-sends tool_result.
    expect(box.captured!.body.messages).toEqual([
      { role: 'user', content: 'add a rain inflow' },
      { role: 'assistant', content: [{ type: 'tool_use', id: 'tu_1', name: 'apply_edit', input: { op: 'add' } }] },
      { role: 'user', content: [{ type: 'tool_result', tool_use_id: 'tu_1', content: 'Applied. Valid.' }] },
    ])
  })
})

describe('OpenAI/Azure tool adapter', () => {
  it('shapes tools as function-typed and parses tool_calls', async () => {
    const box = stubFetch({ choices: [{ message: { content: null, tool_calls: [{ id: 'c1', type: 'function', function: { name: 'apply_edit', arguments: '{"op":"delete","id":"x"}' } }] }, finish_reason: 'tool_calls' }] })
    const r = await chatWithTools(OPENAI, 'SYS', [{ role: 'user', content: 'hi' }], [TOOL])
    expect(box.captured!.body.tools).toEqual([{ type: 'function', function: { name: 'apply_edit', description: 'edit', parameters: { type: 'object', properties: {} } } }])
    expect(r.stopReason).toBe('tool_use')
    expect(r.toolCalls).toEqual([{ id: 'c1', name: 'apply_edit', input: { op: 'delete', id: 'x' } }])
  })

  it('echoes the assistant tool_calls turn and FANS OUT the tool_result to a role:tool message', async () => {
    const box = stubFetch({ choices: [{ message: { content: 'ok' }, finish_reason: 'stop' }] })
    await chatWithTools(OPENAI, 'SYS', ECHO_MESSAGES, [TOOL])
    // system prepended; assistant carries tool_calls (arguments re-stringified); tool_result → role:'tool'.
    expect(box.captured!.body.messages).toEqual([
      { role: 'system', content: 'SYS' },
      { role: 'user', content: 'add a rain inflow' },
      { role: 'assistant', content: null, tool_calls: [{ id: 'tu_1', type: 'function', function: { name: 'apply_edit', arguments: '{"op":"add"}' } }] },
      { role: 'tool', tool_call_id: 'tu_1', content: 'Applied. Valid.' },
    ])
  })

  it('malformed tool arguments become empty input, not a throw', async () => {
    stubFetch({ choices: [{ message: { tool_calls: [{ id: 'c1', type: 'function', function: { name: 'apply_edit', arguments: '{not json' } }] }, finish_reason: 'tool_calls' }] })
    const r = await chatWithTools(OPENAI, '', [], [TOOL])
    expect(r.toolCalls[0].input).toEqual({})
  })

  it('Azure targets the deployment URL with the api-key header', async () => {
    const box = stubFetch({ choices: [{ message: { content: 'x' }, finish_reason: 'stop' }] })
    await chatWithTools(AZURE, '', [{ role: 'user', content: 'hi' }], [TOOL])
    expect(box.captured!.url).toBe('https://r.openai.azure.com/openai/deployments/d/chat/completions?api-version=2024-06-01')
    expect(box.captured!.headers['api-key']).toBe('k')
    // Reasoning models reject max_tokens — tool body uses max_completion_tokens too.
    expect(box.captured!.body.max_completion_tokens).toBeGreaterThan(0)
    expect(box.captured!.body.max_tokens).toBeUndefined()
  })

  it('Azure v1 endpoint override targets {endpoint}/chat/completions with the body model', async () => {
    const box = stubFetch({ choices: [{ message: { content: 'x' }, finish_reason: 'stop' }] })
    await chatWithTools(
      { ...AZURE, endpoint: 'https://h.cognitiveservices.azure.com/openai/v1/', model: 'gpt-5-mini' },
      '', [{ role: 'user', content: 'hi' }], [TOOL],
    )
    expect(box.captured!.url).toBe('https://h.cognitiveservices.azure.com/openai/v1/chat/completions')
    expect(box.captured!.body.model).toBe('gpt-5-mini')
  })
})
