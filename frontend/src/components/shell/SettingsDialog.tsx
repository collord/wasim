import { useStore } from '../../store'
import type { DimensionDef } from '../../model/schema'
import { Field, NumInput, Select, TextInput } from '../inspector/fields'

/** Simulation settings dialog (spec §9): time, Monte Carlo, sampling. Edits the canonical
 *  `simulation_settings`. Only options the engine supports are offered (§14). */
export function SettingsDialog({ onClose }: { onClose: () => void }) {
  const doc = useStore((s) => s.doc)
  const editSettings = useStore((s) => s.editSettings)
  if (!doc) return null
  const ss = doc.simulation_settings

  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/30 p-4" onClick={onClose}>
      <div className="w-full max-w-md rounded-lg bg-white shadow-xl" onClick={(e) => e.stopPropagation()}>
        <div className="flex items-center justify-between border-b border-slate-200 px-4 py-3">
          <h3 className="text-sm font-semibold text-slate-800">Simulation settings</h3>
          <button onClick={onClose} className="text-slate-400 hover:text-slate-600">✕</button>
        </div>
        <div className="space-y-4 p-4">
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
        </div>
        <div className="flex justify-end border-t border-slate-200 px-4 py-3">
          <button onClick={onClose} className="rounded bg-blue-600 px-3 py-1.5 text-xs font-medium text-white hover:bg-blue-500">Done</button>
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
