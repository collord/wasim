import { describe, it, expect, vi, afterEach } from 'vitest'
import type { LlmConfig } from './types'
import { chat, hasKey, testConnection, PROVIDERS } from './index'

/**
 * Adapter request-shaping and response-parsing for all three providers, via a stubbed `fetch`.
 * Only the Anthropic path is live-verified elsewhere (the copilot e2e); OpenAI/Azure are built to
 * their confirmed wire formats and covered here for shaping/parsing, not by a real call.
 */

interface Captured {
  url: string
  headers: Record<string, string>
  body: Record<string, unknown>
}

function stubFetch(response: unknown, ok = true, status = 200): { captured: Captured | null } {
  const box: { captured: Captured | null } = { captured: null }
  vi.stubGlobal('fetch', async (url: string, init: RequestInit) => {
    box.captured = {
      url,
      headers: init.headers as Record<string, string>,
      body: JSON.parse(init.body as string),
    }
    return {
      ok,
      status,
      json: async () => response,
    } as Response
  })
  return box
}

afterEach(() => vi.unstubAllGlobals())

const ANTHROPIC: LlmConfig = { provider: 'anthropic', model: 'claude-opus-4-8', apiKey: 'sk-ant-x' }
const OPENAI: LlmConfig = { provider: 'openai', model: 'gpt-4o', apiKey: 'sk-o-x' }
const AZURE: LlmConfig = {
  provider: 'azure-openai', deployment: 'my-deploy', apiKey: 'az-x', resource: 'my-res', apiVersion: '2024-06-01',
}

describe('Anthropic adapter', () => {
  it('shapes the request and parses text', async () => {
    const box = stubFetch({ content: [{ type: 'text', text: 'hello' }] })
    const out = await chat(ANTHROPIC, 'SYS', [{ role: 'user', content: 'hi' }])
    expect(out).toBe('hello')
    expect(box.captured!.url).toBe('https://api.anthropic.com/v1/messages')
    expect(box.captured!.headers['x-api-key']).toBe('sk-ant-x')
    expect(box.captured!.headers['anthropic-dangerous-direct-browser-access']).toBe('true')
    expect(box.captured!.body).toMatchObject({ model: 'claude-opus-4-8', system: 'SYS', messages: [{ role: 'user', content: 'hi' }] })
  })

  it('throws with the API error message on non-2xx', async () => {
    stubFetch({ error: { message: 'bad key' } }, false, 401)
    await expect(chat(ANTHROPIC, '', [])).rejects.toThrow(/Anthropic API error: bad key/)
  })
})

describe('OpenAI adapter', () => {
  it('shapes the request (system as leading message) and parses choices[0]', async () => {
    const box = stubFetch({ choices: [{ message: { content: 'world' } }] })
    const out = await chat(OPENAI, 'SYS', [{ role: 'user', content: 'hi' }])
    expect(out).toBe('world')
    expect(box.captured!.url).toBe('https://api.openai.com/v1/chat/completions')
    expect(box.captured!.headers.authorization).toBe('Bearer sk-o-x')
    expect(box.captured!.body.messages).toEqual([
      { role: 'system', content: 'SYS' },
      { role: 'user', content: 'hi' },
    ])
    // Reasoning models reject max_tokens — the adapter sends max_completion_tokens.
    expect(box.captured!.body.max_completion_tokens).toBeGreaterThan(0)
    expect(box.captured!.body.max_tokens).toBeUndefined()
  })

  it('honors a baseUrl override', async () => {
    const box = stubFetch({ choices: [{ message: { content: 'x' } }] })
    await chat({ ...OPENAI, baseUrl: 'https://gw.example/v1/' }, '', [])
    expect(box.captured!.url).toBe('https://gw.example/v1/chat/completions')
  })

  it('throws with the API error message on non-2xx', async () => {
    stubFetch({ error: { message: 'quota' } }, false, 429)
    await expect(chat(OPENAI, '', [])).rejects.toThrow(/OpenAI API error: quota/)
  })
})

describe('Azure OpenAI adapter', () => {
  it('shapes the deployment URL + api-key header, parses choices[0]', async () => {
    const box = stubFetch({ choices: [{ message: { content: 'az-out' } }] })
    const out = await chat(AZURE, 'SYS', [{ role: 'user', content: 'hi' }])
    expect(out).toBe('az-out')
    expect(box.captured!.url).toBe(
      'https://my-res.openai.azure.com/openai/deployments/my-deploy/chat/completions?api-version=2024-06-01',
    )
    expect(box.captured!.headers['api-key']).toBe('az-x')
    expect(box.captured!.body.messages).toEqual([
      { role: 'system', content: 'SYS' },
      { role: 'user', content: 'hi' },
    ])
  })

  it('uses the v1 endpoint override + body model when endpoint is set', async () => {
    const box = stubFetch({ choices: [{ message: { content: 'v1-out' } }] })
    const cfg: LlmConfig = { ...AZURE, endpoint: 'https://host.cognitiveservices.azure.com/openai/v1/', model: 'gpt-5-mini' }
    const out = await chat(cfg, 'SYS', [{ role: 'user', content: 'hi' }])
    expect(out).toBe('v1-out')
    // Endpoint wins over the classic {resource}.openai.azure.com deployment URL (trailing slash trimmed).
    expect(box.captured!.url).toBe('https://host.cognitiveservices.azure.com/openai/v1/chat/completions')
    expect(box.captured!.headers['api-key']).toBe('az-x')
    expect(box.captured!.body.model).toBe('gpt-5-mini') // v1 surface carries the model in the body
  })
})

describe('registry', () => {
  it('hasKey reflects per-provider required fields', () => {
    expect(hasKey(ANTHROPIC)).toBe(true)
    expect(hasKey({ ...ANTHROPIC, apiKey: '' })).toBe(false)
    expect(hasKey(AZURE)).toBe(true)
    expect(hasKey({ ...AZURE, resource: '' })).toBe(false) // classic azure needs resource+deployment+key
    expect(hasKey({ ...AZURE, resource: '', deployment: '', endpoint: 'https://h/openai/v1/' })).toBe(true) // v1 surface: endpoint suffices
    expect(hasKey({ ...AZURE, apiKey: '', endpoint: 'https://h/openai/v1/' })).toBe(false) // still needs a key
  })

  it('blank() produces a valid empty config per provider', () => {
    expect(PROVIDERS.anthropic.blank().provider).toBe('anthropic')
    expect(PROVIDERS.openai.blank().provider).toBe('openai')
    expect(PROVIDERS['azure-openai'].blank().provider).toBe('azure-openai')
  })
})

describe('testConnection', () => {
  it('reports ok + latency + model on success', async () => {
    stubFetch({ content: [{ type: 'text', text: 'ok' }] })
    let t = 100
    const r = await testConnection(ANTHROPIC, () => (t += 20))
    expect(r.ok).toBe(true)
    expect(r.model).toBe('claude-opus-4-8')
    expect(r.ms).toBeGreaterThanOrEqual(0)
  })

  it('reports the error (never throws) on failure', async () => {
    stubFetch({ error: { message: 'nope' } }, false, 403)
    const r = await testConnection(OPENAI)
    expect(r.ok).toBe(false)
    expect(r.error).toMatch(/nope/)
  })

  it('uses the deployment name as the model for Azure', async () => {
    stubFetch({ choices: [{ message: { content: 'ok' } }] })
    const r = await testConnection(AZURE)
    expect(r.model).toBe('my-deploy')
  })
})
