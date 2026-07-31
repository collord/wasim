import { describe, it, expect, beforeEach, vi } from 'vitest'
import { loadAiConfig, saveAiConfig, defaultAiConfig } from './aiConfig'

/** localStorage shim for node vitest, so persistence + migration are testable. */
function installStorage(seed?: Record<string, string>) {
  const map = new Map<string, string>(Object.entries(seed ?? {}))
  vi.stubGlobal('localStorage', {
    getItem: (k: string) => map.get(k) ?? null,
    setItem: (k: string, v: string) => void map.set(k, v),
    removeItem: (k: string) => void map.delete(k),
    clear: () => map.clear(),
    key: () => null,
    length: 0,
  })
  return map
}

beforeEach(() => vi.unstubAllGlobals())

describe('aiConfig', () => {
  it('defaults to Anthropic, no key, remember off', () => {
    installStorage()
    const c = defaultAiConfig()
    expect(c.llm.provider).toBe('anthropic')
    expect(c.llm.apiKey).toBe('')
    expect(c.rememberKey).toBe(false)
  })

  it('persists the key only when rememberKey is true', () => {
    const store = installStorage()
    saveAiConfig({ llm: { provider: 'anthropic', model: 'claude-opus-4-8', apiKey: 'secret' }, rememberKey: false })
    expect(store.get('wasim.ai.v1')).not.toContain('secret') // memory-only

    saveAiConfig({ llm: { provider: 'anthropic', model: 'claude-opus-4-8', apiKey: 'secret' }, rememberKey: true })
    expect(store.get('wasim.ai.v1')).toContain('secret')
  })

  it('rehydrates the key only when it was remembered', () => {
    installStorage({
      'wasim.ai.v1': JSON.stringify({ llm: { provider: 'openai', model: 'gpt-4o', apiKey: 'k' }, rememberKey: true }),
    })
    expect(loadAiConfig().llm).toMatchObject({ provider: 'openai', apiKey: 'k' })

    installStorage({
      'wasim.ai.v1': JSON.stringify({ llm: { provider: 'openai', model: 'gpt-4o', apiKey: 'k' }, rememberKey: false }),
    })
    // rememberKey false → the key should NOT be present even if it somehow got stored.
    const loaded = loadAiConfig().llm
    expect(loaded.provider).toBe('openai')
    expect('apiKey' in loaded && loaded.apiKey).toBe('')
  })

  it('migrates the legacy flat shape ({apiKey, model}) to an Anthropic LlmConfig', () => {
    installStorage({
      'wasim.ai.v1': JSON.stringify({ apiKey: 'legacy-key', model: 'claude-sonnet-5', rememberKey: true }),
    })
    const c = loadAiConfig()
    expect(c.llm.provider).toBe('anthropic')
    expect(c.llm).toMatchObject({ model: 'claude-sonnet-5', apiKey: 'legacy-key' })
    expect(c.rememberKey).toBe(true)
  })

  it('migrates legacy shape without remembered key → empty key', () => {
    installStorage({
      'wasim.ai.v1': JSON.stringify({ apiKey: 'legacy-key', model: 'claude-opus-4-8', rememberKey: false }),
    })
    const c = loadAiConfig()
    expect(c.llm.provider).toBe('anthropic')
    expect('apiKey' in c.llm && c.llm.apiKey).toBe('')
  })
})
