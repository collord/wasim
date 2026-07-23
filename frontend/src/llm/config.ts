// LLM endpoint configuration (spec §17.1). Bring-your-own-key, stored in browser localStorage
// only — never written to model.json, never sent anywhere except the chosen provider.

export type LlmProvider = 'anthropic' | 'openai'

export interface LlmConfig {
  provider: LlmProvider
  model: string
  apiKey: string
  /** OpenAI-compatible gateway override (OpenAI only). */
  baseUrl?: string
}

const STORAGE_KEY = 'wasim.llm.config'

export const DEFAULT_MODELS: Record<LlmProvider, string[]> = {
  // Opus is the most capable at one-shot schema-faithful generation; Sonnet/Haiku are cheaper.
  anthropic: ['claude-opus-4-8', 'claude-sonnet-5', 'claude-haiku-4-5-20251001'],
  openai: ['gpt-4o', 'gpt-4o-mini', 'o3-mini'],
}

export function loadLlmConfig(): LlmConfig | null {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    return raw ? (JSON.parse(raw) as LlmConfig) : null
  } catch {
    return null
  }
}

export function saveLlmConfig(cfg: LlmConfig | null): void {
  try {
    if (cfg) localStorage.setItem(STORAGE_KEY, JSON.stringify(cfg))
    else localStorage.removeItem(STORAGE_KEY)
  } catch {
    /* localStorage unavailable — config stays in-memory only */
  }
}
