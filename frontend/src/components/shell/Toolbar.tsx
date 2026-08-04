import { useState } from 'react'
import { useStore, useActiveLens, useLensList, type Mode } from '../../store'
import { SettingsDialog } from './SettingsDialog'
import type { LensId } from '../../lenses/types'

// Sentinel <option> value for "return this container to automatic detection" — distinct from any
// real (kebab-case) lens id.
const AUTO_DETECT = '__auto__'

/** Top toolbar (spec §1.1): file ops, run, undo/redo, and the Edit│Result mode switch. */
export function Toolbar() {
  const newModel = useStore((s) => s.newModel)
  const saveModel = useStore((s) => s.saveModel)
  const run = useStore((s) => s.run)
  const undo = useStore((s) => s.undo)
  const redo = useStore((s) => s.redo)
  const canUndo = useStore((s) => s.past.length > 0)
  const canRedo = useStore((s) => s.future.length > 0)
  const status = useStore((s) => s.status)
  const valid = useStore((s) => s.valid)
  const mode = useStore((s) => s.mode)
  const setMode = useStore((s) => s.setMode)
  const filename = useStore((s) => s.modelFilename)
  const dirty = useStore((s) => s.dirty)
  const hasDoc = useStore((s) => !!s.doc)
  const activeLens = useActiveLens()
  const setLens = useStore((s) => s.setLens)
  const setContainerLens = useStore((s) => s.setContainerLens)
  const clearContainerLens = useStore((s) => s.clearContainerLens)
  const activeContainerId = useStore((s) => s.activeContainerId)
  // Whether the drilled container has a manual override (vs. following auto-detection). Drives the
  // "Auto-detect" affordance and the picker's ● marker.
  const hasContainerOverride = useStore((s) =>
    s.activeContainerId != null && s.activeContainerId in s.lensOverrides)
  const lenses = useLensList()
  const importLens = useStore((s) => s.importLens)
  const removeCustomLens = useStore((s) => s.removeCustomLens)
  const customManifests = useStore((s) => s.customManifests) // stable array ref until it changes
  const activeIsCustom = customManifests.some((m) => m.id === activeLens.id)

  const [showSettings, setShowSettings] = useState(false)

  const onImportLens = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0]
    if (!file) return
    const reader = new FileReader()
    reader.onload = (ev) => {
      const err = importLens(ev.target?.result as string)
      if (err) alert(`Could not import lens: ${err}`)
      else setLens((JSON.parse(ev.target?.result as string).id) as LensId)
    }
    reader.readAsText(file)
    e.target.value = ''
  }

  const onOpen = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0]
    if (!file) return
    const reader = new FileReader()
    reader.onload = (ev) => useStore.getState().loadModel(ev.target?.result as string, file.name)
    reader.readAsText(file)
    e.target.value = ''
  }

  const btn = 'rounded border border-slate-300 bg-white px-2.5 py-1 text-xs font-medium text-slate-700 hover:bg-slate-50 disabled:opacity-40'

  return (
    <div className="flex items-center gap-2 border-b border-slate-200 bg-white px-3 py-2">
      <span className="font-mono text-sm font-bold tracking-tight text-slate-900">WASiM</span>
      <span className="text-[11px] text-slate-400">{filename ?? 'untitled'}{dirty ? ' •' : ''}</span>

      <div className="ml-2 flex gap-1">
        <button className={btn} onClick={newModel}>New</button>
        <label className={`${btn} cursor-pointer`}>
          Open
          <input type="file" accept=".json" className="sr-only" onChange={onOpen} />
        </label>
        <button className={btn} onClick={() => saveModel()}>Save</button>
      </div>

      <div className="flex gap-1">
        <button className={btn} onClick={undo} disabled={!canUndo} title="Undo (⌘Z)">↶ undo</button>
        <button className={btn} onClick={redo} disabled={!canRedo} title="Redo (⌘⇧Z)">redo ↷</button>
      </div>

      <button className={btn} onClick={() => setShowSettings(true)}>Settings…</button>

      {hasDoc && (
        <div className="flex items-center gap-1 text-[11px] text-slate-500">
          <label className="flex items-center gap-1"
            title={activeContainerId != null
              ? 'Authoring lens — applies to this drilled submodel only (root sets the doc default). Pick "Auto-detect" to follow the inferred lens.'
              : 'Authoring lens — reprograms the palette and validation for a domain'}>
            <span className="text-slate-400">
              Lens{activeContainerId != null ? ' (here)' : ''}
              {hasContainerOverride ? <span title="Manually set for this submodel (not auto-detected)"> ●</span> : ''}
            </span>
            <select
              value={activeLens.id}
              onChange={(e) => {
                const v = e.target.value
                if (v === AUTO_DETECT) clearContainerLens()
                else setContainerLens(v as LensId)
              }}
              className="rounded border border-slate-300 bg-white px-1.5 py-1 text-xs font-medium text-slate-700"
            >
              {/* Only meaningful inside a drilled container: return it to automatic hinting. */}
              {activeContainerId != null && (
                <option value={AUTO_DETECT}>Auto-detect{hasContainerOverride ? '' : ` (${activeLens.label})`}</option>
              )}
              {lenses.map((l) => (
                <option key={l.id} value={l.id} title={l.tagline}>{l.label}</option>
              ))}
            </select>
          </label>
          {activeIsCustom && (
            <button
              onClick={() => removeCustomLens(activeLens.id)}
              aria-label="Remove this imported lens"
              title="Remove this imported lens"
              className="rounded border border-slate-200 px-1 py-1 text-slate-400 hover:border-red-300 hover:text-red-500"
            >
              <span aria-hidden>✕</span>
            </button>
          )}
          <label
            className="cursor-pointer rounded border border-slate-200 px-1.5 py-1 text-slate-500 hover:bg-slate-50"
            title="Import a custom lens (.json) — re-themes the palette with no code"
          >
            + Lens
            <input type="file" accept=".json" className="sr-only" onChange={onImportLens} />
          </label>
        </div>
      )}

      <button
        onClick={run}
        disabled={status === 'running' || !valid}
        className="rounded bg-emerald-600 px-3 py-1 text-xs font-semibold text-white hover:bg-emerald-500 disabled:opacity-40"
        title={!valid ? 'Fix validation errors to run' : 'Run simulation'}
      >
        {status === 'running' ? '⟳ Running…' : '▸ Run'}
      </button>

      {/* Mode switch */}
      <div className="ml-auto flex overflow-hidden rounded border border-slate-300 text-xs font-medium">
        {(['edit', 'result'] as Mode[]).map((m) => (
          <button key={m} onClick={() => setMode(m)}
            className={`px-3 py-1 ${mode === m ? 'bg-slate-800 text-white' : 'bg-white text-slate-500 hover:bg-slate-50'}`}>
            {m === 'edit' ? 'Edit' : 'Result'}
          </button>
        ))}
      </div>

      {showSettings && <SettingsDialog onClose={() => setShowSettings(false)} />}
    </div>
  )
}
