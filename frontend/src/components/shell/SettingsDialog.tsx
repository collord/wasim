import { useState } from 'react'
import { useStore } from '../../store'
import type { DimensionDef } from '../../model/schema'
import { PROVIDERS, testConnection, type TestResult } from '../../copilot/providers'
import type { LlmConfig, ProviderId } from '../../copilot/providers/types'
import { Field, NumInput, Select, TextInput, Toggle } from '../inspector/fields'

/** Settings dialog: simulation settings (spec §9) and copilot/AI config (§17.1). Sim settings
 *  edit the canonical `simulation_settings` and only render with a doc loaded; AI config is
 *  doc-independent so a key can be set on an empty workspace. */
export function SettingsDialog({ onClose }: { onClose: () => void }) {
  const doc = useStore((s) => s.doc)
  const editSettings = useStore((s) => s.editSettings)
  const ss = doc?.simulation_settings

  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/30 p-4" onClick={onClose}>
      <div className="w-full max-w-md rounded-lg bg-white shadow-xl" onClick={(e) => e.stopPropagation()}>
        <div className="flex items-center justify-between border-b border-slate-200 px-4 py-3">
          <h3 className="text-sm font-semibold text-slate-800">Settings</h3>
          <button onClick={onClose} className="text-slate-400 hover:text-slate-600">✕</button>
        </div>
        <div className="max-h-[70vh] space-y-4 overflow-auto p-4">
          {ss ? (
            <>
              <div>
                <h4 className="mb-2 text-[10px] font-semibold uppercase tracking-wide text-slate-400">Time</h4>
                <div className="grid grid-cols-2 gap-3">
                  <Field label="Duration">
                    <NumInput value={ss.duration.value} unit={ss.duration.unit}
                      onChange={(v) => editSettings({ duration: { ...ss.duration, value: v } })} />
                  </Field>
                  <Field label="Duration unit">
                    <TextInput value={ss.duration.unit} mono onChange={(unit) => editSettings({ duration: { ...ss.duration, unit } })} />
                  </Field>
                  <Field label="Timestep">
                    <NumInput value={ss.timestep.value} unit={ss.timestep.unit}
                      onChange={(v) => editSettings({ timestep: { ...ss.timestep, value: v } })} />
                  </Field>
                  <Field label="Timestep unit">
                    <TextInput value={ss.timestep.unit} mono onChange={(unit) => editSettings({ timestep: { ...ss.timestep, unit } })} />
                  </Field>
                </div>
              </div>

              <div>
                <h4 className="mb-2 text-[10px] font-semibold uppercase tracking-wide text-slate-400">Monte Carlo</h4>
                <div className="grid grid-cols-2 gap-3">
                  <Field label="Realizations">
                    <NumInput value={ss.n_realizations} step={1}
                      onChange={(v) => editSettings({ n_realizations: Math.max(1, Math.round(v)) })} />
                  </Field>
                  <Field label="Sampling">
                    <Select value={ss.sampling_method ?? 'monte_carlo'}
                      onChange={(sampling_method) => editSettings({ sampling_method: sampling_method as 'monte_carlo' | 'lhs' })}
                      options={[{ value: 'monte_carlo', label: 'Monte Carlo' }, { value: 'lhs', label: 'Latin Hypercube' }]} />
                  </Field>
                  <Field label="Seed" hint="Blank = nondeterministic">
                    <NumInput value={ss.seed ?? NaN} step={1}
                      onChange={(v) => editSettings({ seed: isNaN(v) ? null : Math.round(v) })} />
                  </Field>
                </div>
              </div>

              <DimensionsSection />
            </>
          ) : (
            <p className="text-[11px] text-slate-400">Load or create a model to edit its simulation settings.</p>
          )}

          <AiSettingsSection />
        </div>
        <div className="flex justify-end border-t border-slate-200 px-4 py-3">
          <button onClick={onClose} className="rounded bg-blue-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-blue-500">Done</button>
        </div>
      </div>
    </div>
  )
}

/** AI copilot config (§17.1): API key, model, and generation options. Doc-independent — the key
 *  can be entered before any model is loaded. The key is memory-only unless "remember" is on. */
function AiSettingsSection() {
  const cfg = useStore((s) => s.aiConfig)
  const setAiConfig = useStore((s) => s.setAiConfig)

  const llm = cfg.llm
  const [test, setTest] = useState<TestResult | 'running' | null>(null)

  // Merge a patch into the active provider's LlmConfig (the variants share `apiKey`, so a partial
  // over the current variant stays well-typed).
  const setLlm = (patch: Partial<LlmConfig>) => setAiConfig({ llm: { ...llm, ...patch } as LlmConfig })
  const switchProvider = (id: ProviderId) => {
    setTest(null)
    setAiConfig({ llm: PROVIDERS[id].blank() })
  }

  const runTest = async () => {
    setTest('running')
    setTest(await testConnection(llm))
  }

  const keyField = (
    <>
      <Field label="API key" hint="Sent directly from your browser to the provider — nowhere else.">
        <input
          type="password"
          value={llm.apiKey}
          placeholder={llm.provider === 'anthropic' ? 'sk-ant-…' : 'sk-…'}
          onChange={(e) => setLlm({ apiKey: e.target.value })}
          className="w-full rounded border border-slate-300 px-2 py-1 font-mono text-xs outline-none focus:border-blue-400"
        />
      </Field>
      <Toggle
        label="Remember key on this device (stores it in this browser)"
        checked={cfg.rememberKey}
        onChange={(rememberKey) => setAiConfig({ rememberKey })}
      />
    </>
  )

  return (
    <div className="border-t border-slate-100 pt-4">
      <h4 className="mb-2 text-[10px] font-semibold uppercase tracking-wide text-slate-400">AI copilot</h4>
      <div className="space-y-3">
        <Field label="Provider">
          <Select
            value={llm.provider}
            onChange={(p) => switchProvider(p as ProviderId)}
            options={(Object.keys(PROVIDERS) as ProviderId[]).map((id) => ({ value: id, label: PROVIDERS[id].label }))}
          />
        </Field>

        {llm.provider === 'anthropic' && (
          <>
            <Field label="Model">
              <Select value={llm.model} onChange={(model) => setLlm({ model })}
                options={[
                  { value: 'claude-opus-4-8', label: 'Claude Opus 4.8 (most capable)' },
                  { value: 'claude-sonnet-5', label: 'Claude Sonnet 5 (faster)' },
                  { value: 'claude-haiku-4-5', label: 'Claude Haiku 4.5 (cheapest)' },
                ]} />
            </Field>
            {keyField}
          </>
        )}

        {llm.provider === 'openai' && (
          <>
            <Field label="Model"><TextInput value={llm.model} mono onChange={(model) => setLlm({ model })} /></Field>
            <Field label="Base URL" hint="Optional — for OpenAI-compatible gateways.">
              <TextInput value={llm.baseUrl ?? ''} mono onChange={(baseUrl) => setLlm({ baseUrl: baseUrl || undefined })} />
            </Field>
            {keyField}
          </>
        )}

        {llm.provider === 'azure-openai' && (
          <>
            <Field label="Endpoint" hint="New v1 surface, e.g. https://{host}/openai/v1/ — overrides Resource/Deployment/API version below. Leave blank for the classic surface.">
              <TextInput value={llm.endpoint ?? ''} mono onChange={(endpoint) => setLlm({ endpoint: endpoint || undefined })} />
            </Field>
            {llm.endpoint?.trim() ? (
              <Field label="Model" hint="e.g. gpt-5-mini (sent in the request body on the v1 surface).">
                <TextInput value={llm.model ?? ''} mono onChange={(model) => setLlm({ model: model || undefined })} />
              </Field>
            ) : (
              <>
                <Field label="Resource" hint="{resource}.openai.azure.com">
                  <TextInput value={llm.resource} mono onChange={(resource) => setLlm({ resource })} />
                </Field>
                <Field label="Deployment"><TextInput value={llm.deployment} mono onChange={(deployment) => setLlm({ deployment })} /></Field>
                <Field label="API version"><TextInput value={llm.apiVersion} mono onChange={(apiVersion) => setLlm({ apiVersion })} /></Field>
              </>
            )}
            {keyField}
          </>
        )}

        <div className="flex items-center gap-2">
          <button
            onClick={runTest}
            disabled={test === 'running' || !PROVIDERS[llm.provider].hasKey(llm)}
            className="rounded border border-slate-300 px-2 py-1 text-xs text-slate-600 hover:bg-slate-50 disabled:opacity-40"
          >
            {test === 'running' ? 'Testing…' : 'Test connection'}
          </button>
          {test && test !== 'running' && (
            <span className={`text-[11px] ${test.ok ? 'text-emerald-600' : 'text-rose-600'}`}>
              {test.ok ? `✓ ${test.model} · ${test.ms} ms` : `✗ ${test.error}`}
            </span>
          )}
        </div>
      </div>
    </div>
  )
}

/** Declared array axes (spec §7): fixed-size, optionally-labelled indices that per-member
 *  (`vector_map`) elements range over — e.g. a fleet of N trucks. */
function DimensionsSection() {
  const doc = useStore((s) => s.doc)
  const editDimensions = useStore((s) => s.editDimensions)
  const dims: DimensionDef[] = doc?.dimensions ?? []

  const update = (i: number, patch: Partial<DimensionDef>) =>
    editDimensions(dims.map((d, j) => (j === i ? { ...d, ...patch } : d)))
  const add = () => {
    const taken = new Set(dims.map((d) => d.id))
    let id = 'Fleet', n = 2
    while (taken.has(id)) id = `Fleet_${n++}`
    editDimensions([...dims, { id, name: id, size: 5 }])
  }
  const remove = (i: number) => editDimensions(dims.filter((_, j) => j !== i))

  return (
    <div>
      <div className="mb-2 flex items-center justify-between">
        <h4 className="text-[10px] font-semibold uppercase tracking-wide text-slate-400">Dimensions (array axes)</h4>
        <button onClick={add} className="rounded border border-slate-200 px-1.5 py-0.5 text-[11px] text-slate-500 hover:bg-slate-50">+ add</button>
      </div>
      {dims.length === 0 && (
        <p className="text-[11px] text-slate-400">None. Add one to build per-member (fleet) arrays with <span className="font-mono">vector_map</span>.</p>
      )}
      <div className="space-y-2">
        {dims.map((d, i) => (
          <div key={i} className="rounded border border-slate-200 p-2">
            <div className="grid grid-cols-3 gap-2">
              <Field label="ID"><TextInput value={d.id} mono onChange={(id) => update(i, { id })} /></Field>
              <Field label="Name"><TextInput value={d.name} onChange={(name) => update(i, { name })} /></Field>
              <Field label="Size"><NumInput value={d.size} step={1} onChange={(v) => update(i, { size: Math.max(1, Math.round(v)) })} /></Field>
            </div>
            <div className="mt-1 flex items-end gap-2">
              <div className="flex-1">
                <Field label="Member labels" hint="Optional, comma-separated (for subscript-by-label).">
                  <TextInput mono value={(d.labels ?? []).join(', ')}
                    onChange={(v) => {
                      const labels = v.split(',').map((s) => s.trim()).filter(Boolean)
                      update(i, { labels: labels.length ? labels : undefined })
                    }} />
                </Field>
              </div>
              <button onClick={() => remove(i)} className="mb-1 px-1 text-slate-400 hover:text-rose-500" title="Remove dimension">✕</button>
            </div>
          </div>
        ))}
      </div>
    </div>
  )
}
