/**
 * Persisted config for the LLM-authoring copilot (WASIM_AUTHORING_ENVIRONMENT_SPEC.md §17.1).
 * Mirrors the localStorage pattern in `lenses/customLenses.ts` — a single namespaced key, a
 * storage() accessor guarded for SSR / sandboxed-iframe / test environments, and silent-on-failure
 * read/write so storage being unavailable never crashes the app.
 *
 * Key handling is user-controlled (the security tradeoff §17.5 flags): `rememberKey` gates whether
 * the API key is written to disk at all. Off by default → the key lives only in store memory and is
 * never persisted; on → it rides along in localStorage like any other field. Everything *except*
 * the key persists regardless, so model/thinking/rememberKey preferences survive a reload even when
 * the key is session-only.
 */

const STORAGE_KEY = 'wasim.ai.v1'

/** localStorage if available (absent under SSR / node test env), else null. */
function storage(): Storage | null {
  try {
    return typeof localStorage !== 'undefined' ? localStorage : null
  } catch {
    return null // access can throw in sandboxed iframes
  }
}

export const DEFAULT_MODEL = 'claude-opus-4-8'

/** Whether to send adaptive reasoning on the generation request. The default is set by the
 *  thinking-default A/B (LLM_AUTHORING_SPIKE_FINDINGS.md) — not asserted. `undefined` = use the
 *  build-time default; a boolean is an explicit user choice. */
export interface AiConfig {
  apiKey: string
  model: string
  /** Persist the API key to localStorage. Off = the key is memory-only (cleared on reload). */
  rememberKey: boolean
  /** Adaptive thinking on the generation request. Undefined defers to DEFAULT_THINKING. */
  thinking?: boolean
}

/**
 * Default for adaptive thinking on the generation request. Set conservatively to `false` (cheaper,
 * faster, and the feasibility spike showed convergence is achievable without isolating thinking).
 * It is NOT asserted to be optimal — the A/B that would settle it (`tools/copilot_thinking_ab.mjs`)
 * needs an ANTHROPIC_API_KEY, which wasn't available when this was built. Run that harness and flip
 * this to the lower-iteration / higher-validity winner. The user can always opt in via the toggle.
 */
export const DEFAULT_THINKING = false

export function defaultAiConfig(): AiConfig {
  return { apiKey: '', model: DEFAULT_MODEL, rememberKey: false }
}

/** Read persisted config. The key rehydrates only if it was saved under `rememberKey: true`. */
export function loadAiConfig(): AiConfig {
  const s = storage()
  const base = defaultAiConfig()
  if (!s) return base
  try {
    const raw = s.getItem(STORAGE_KEY)
    if (!raw) return base
    const stored = JSON.parse(raw) as Partial<AiConfig>
    return {
      apiKey: stored.rememberKey ? stored.apiKey ?? '' : '',
      model: typeof stored.model === 'string' ? stored.model : base.model,
      rememberKey: !!stored.rememberKey,
      thinking: typeof stored.thinking === 'boolean' ? stored.thinking : undefined,
    }
  } catch {
    return base // corrupt storage → start clean
  }
}

/** Persist config. The API key is written ONLY when `rememberKey` is true. */
export function saveAiConfig(cfg: AiConfig): void {
  const s = storage()
  if (!s) return
  try {
    const toStore: Partial<AiConfig> = {
      model: cfg.model,
      rememberKey: cfg.rememberKey,
      thinking: cfg.thinking,
      ...(cfg.rememberKey ? { apiKey: cfg.apiKey } : {}),
    }
    s.setItem(STORAGE_KEY, JSON.stringify(toStore))
  } catch {
    // storage full / unavailable (private mode) — config still works this session, just isn't saved
  }
}

/** The effective thinking setting for a request: explicit user choice, else the build default. */
export function effectiveThinking(cfg: AiConfig): boolean {
  return cfg.thinking ?? DEFAULT_THINKING
}
