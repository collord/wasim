import { useState } from 'react'
import { useStore, type Mode } from '../../store'
import { SettingsDialog } from './SettingsDialog'

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

  const [showSettings, setShowSettings] = useState(false)

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

      <button
        onClick={run}
        disabled={status === 'running' || !valid}
        className="rounded bg-emerald-600 px-3 py-1 text-xs font-semibold text-white hover:bg-emerald-500 disabled:opacity-40"
        title={!valid ? 'Fix validation errors to run' : 'Run simulation'}
      >
        {status === 'running' ? '⟳ Running…' : '▸ Run'}
      </button>

      <button className={btn} onClick={() => useStore.getState().toggleCopilot()} title="AI Copilot — describe a model in words">
        ✦ Copilot
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
