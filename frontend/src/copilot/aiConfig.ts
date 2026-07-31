import type { LlmConfig } from './providers/types'
import { PROVIDERS } from './providers'

/**
 * Persisted copilot config (WASIM_AUTHORING_ENVIRONMENT_SPEC.md §17.1). Wraps a provider-specific
 * `LlmConfig` (Anthropic / OpenAI / Azure) plus the cross-cutting `rememberKey`. Mirrors the
 * localStorage pattern in `lenses/customLenses.ts`: a namespaced key, a storage() accessor guarded
 * for SSR / sandboxed-iframe / test environments, and silent-on-failure read/write.
 *
 * Key handling is user-controlled (the security tradeoff §17.5 flags): `rememberKey` gates whether
 * the credential(s) are written to disk. Off by default → the config's `apiKey` lives only in store
 * memory. Everything else (provider, model, non-secret fields) persists regardless, so preferences
 * survive a reload even when the key is session-only.
 */

const STORAGE_KEY = 'wasim.ai.v1'

function storage(): Storage | null {
  try {
    return typeof localStorage !== 'undefined' ? localStorage : null
  } catch {
    return null
  }
}

export interface AiConfig {
  /** The active provider connection config. */
  llm: LlmConfig
  /** Persist the credential to localStorage. Off = memory-only (cleared on reload). */
  rememberKey: boolean
}

export function defaultAiConfig(): AiConfig {
  return { llm: PROVIDERS.anthropic.blank(), rememberKey: false }
}

/** Strip the secret credential from an LlmConfig (for persistence when rememberKey is off). */
function withoutSecret(llm: LlmConfig): LlmConfig {
  return { ...llm, apiKey: '' }
}

/**
 * Read persisted config. Handles both the current shape ({llm, rememberKey}) and the pre-abstraction
 * shape ({apiKey, model, rememberKey, thinking?} — implicitly Anthropic). The credential rehydrates
 * only if it was saved under rememberKey: true.
 */
export function loadAiConfig(): AiConfig {
  const s = storage()
  const base = defaultAiConfig()
  if (!s) return base
  let stored: unknown
  try {
    const raw = s.getItem(STORAGE_KEY)
    if (!raw) return base
    stored = JSON.parse(raw)
  } catch {
    return base
  }

  const o = stored as Record<string, unknown>
  const rememberKey = !!o.rememberKey

  // Current shape.
  if (o.llm && typeof o.llm === 'object') {
    const llm = o.llm as LlmConfig
    return { llm: rememberKey ? llm : withoutSecret(llm), rememberKey }
  }

  // Legacy shape (pre-provider-abstraction): flat {apiKey, model} → Anthropic.
  if (typeof o.model === 'string' || typeof o.apiKey === 'string') {
    const fallback = PROVIDERS.anthropic.blank() // provider:'anthropic', so it has a `.model`
    const llm: LlmConfig = {
      provider: 'anthropic',
      model: typeof o.model === 'string' ? o.model : (fallback as { model: string }).model,
      apiKey: rememberKey && typeof o.apiKey === 'string' ? o.apiKey : '',
    }
    return { llm, rememberKey }
  }

  return base
}

/** Persist config. The credential is written ONLY when rememberKey is true. */
export function saveAiConfig(cfg: AiConfig): void {
  const s = storage()
  if (!s) return
  try {
    const llm = cfg.rememberKey ? cfg.llm : withoutSecret(cfg.llm)
    s.setItem(STORAGE_KEY, JSON.stringify({ llm, rememberKey: cfg.rememberKey }))
  } catch {
    // storage full / unavailable (private mode) — config still works this session, just isn't saved
  }
}
