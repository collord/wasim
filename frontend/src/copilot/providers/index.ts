import type { ChatAdapter, ChatToolAdapter, ChatMessage, LlmConfig, ProviderId } from './types'
import type { ToolDef, ChatResult } from './tools'
import { chatAnthropic, chatAnthropicTools } from './anthropic'
import { chatOpenAI, chatAzure, chatOpenAITools, chatAzureTools } from './openai'

/**
 * Provider registry + the neutral `chat()` the copilot loop consumes (§17.1). Each provider carries
 * an adapter plus the metadata Settings needs to render its config form: a label, a fixed model list
 * (Anthropic) or free-text model (OpenAI/Azure), config defaults, a `hasKey` check, and a one-line
 * summary. This is the single place that knows the set of providers.
 */

export interface ProviderMeta {
  id: ProviderId
  label: string
  adapter: ChatAdapter
  /** Tool-aware adapter (§17.3). */
  toolAdapter: ChatToolAdapter
  /** Fixed model choices, or null for a free-text model/deployment field. */
  models: string[] | null
  /** A blank config for this provider (used when the user switches providers in Settings). */
  blank: () => LlmConfig
  /** Whether this config has the credential(s) needed to make a call. */
  hasKey: (cfg: LlmConfig) => boolean
}

export const PROVIDERS: Record<ProviderId, ProviderMeta> = {
  anthropic: {
    id: 'anthropic',
    label: 'Anthropic',
    adapter: chatAnthropic,
    toolAdapter: chatAnthropicTools,
    models: ['claude-opus-4-8', 'claude-sonnet-5', 'claude-haiku-4-5'],
    blank: () => ({ provider: 'anthropic', model: 'claude-opus-4-8', apiKey: '' }),
    hasKey: (cfg) => cfg.provider === 'anthropic' && cfg.apiKey.trim().length > 0,
  },
  openai: {
    id: 'openai',
    label: 'OpenAI',
    adapter: chatOpenAI,
    toolAdapter: chatOpenAITools,
    models: null, // free-text model id (gpt-4o, gpt-4.1, …) — evolves faster than a fixed list
    blank: () => ({ provider: 'openai', model: 'gpt-4o', apiKey: '' }),
    hasKey: (cfg) => cfg.provider === 'openai' && cfg.apiKey.trim().length > 0,
  },
  'azure-openai': {
    id: 'azure-openai',
    label: 'Azure OpenAI',
    adapter: chatAzure,
    toolAdapter: chatAzureTools,
    models: null, // deployment name, not a model id
    blank: () => ({ provider: 'azure-openai', deployment: '', apiKey: '', resource: '', apiVersion: '2024-06-01' }),
    hasKey: (cfg) =>
      cfg.provider === 'azure-openai' &&
      cfg.apiKey.trim().length > 0 &&
      // v1 surface: an endpoint is enough. Classic surface: needs resource + deployment.
      (!!cfg.endpoint?.trim() || (cfg.resource.trim().length > 0 && cfg.deployment.trim().length > 0)),
  },
}

/** Dispatch a text completion to the config's provider adapter. */
export const chat: ChatAdapter = (cfg, system, messages, signal) =>
  PROVIDERS[cfg.provider].adapter(cfg, system, messages, signal)

/** Dispatch a tool-aware completion to the config's provider tool adapter (§17.3). */
export const chatWithTools: ChatToolAdapter = (cfg, system, messages, tools, signal) =>
  PROVIDERS[cfg.provider].toolAdapter(cfg, system, messages, tools, signal)

/** Whether the given config can make a call (has its required credentials). */
export function hasKey(cfg: LlmConfig): boolean {
  return PROVIDERS[cfg.provider].hasKey(cfg)
}

export interface TestResult {
  ok: boolean
  ms: number
  model: string
  error?: string
}

/** Issue a trivial completion to verify the connection (§17.1 "Test connection"). Reports latency
 *  and the configured model/deployment, or the provider's error. Never throws. */
export async function testConnection(cfg: LlmConfig, now: () => number = () => Date.now()): Promise<TestResult> {
  const model = cfg.provider === 'azure-openai' ? cfg.deployment : cfg.model
  const start = now()
  try {
    await chat(cfg, 'Reply with the single word: ok', [{ role: 'user', content: 'ok' }])
    return { ok: true, ms: Math.round(now() - start), model }
  } catch (e) {
    return { ok: false, ms: Math.round(now() - start), model, error: (e as Error).message }
  }
}

export type { ChatMessage, LlmConfig, ProviderId, ToolDef, ChatResult }
