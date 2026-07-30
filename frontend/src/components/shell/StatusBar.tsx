import { useMemo, useState } from 'react'
import { useStore, useActiveLens } from '../../store'

/** Status bar + expandable issues panel (spec §8). Fed by the engine's validation round-trip
 *  (the FE never re-derives schema truth) *plus* the active lens's author-time invariants — the
 *  lens's governance warnings (e.g. stock-flow consistency) are merged in but never gate the run.
 *  Clicking an issue jumps to its element. */
export function StatusBar() {
  const issues = useStore((s) => s.issues)
  const valid = useStore((s) => s.valid)
  const reconciling = useStore((s) => s.reconciling)
  const topo = useStore((s) => s.topo)
  const count = useStore((s) => s.modelSummary?.element_count ?? 0)
  const select = useStore((s) => s.select)
  const dirty = useStore((s) => s.dirty)
  const [open, setOpen] = useState(false)

  // Active lens invariants → warnings, computed over the engine summary + canonical doc. Memoized
  // so the derived array keeps a stable reference (avoids the useSyncExternalStore churn a fresh
  // array in a selector would cause).
  const lens = useActiveLens()
  const summary = useStore((s) => s.modelSummary)
  const doc = useStore((s) => s.doc)
  const lensIssues = useMemo(
    () => (lens.invariants && summary && doc ? lens.invariants(summary, doc) : []),
    [lens, summary, doc],
  )
  const issuesAll = useMemo(() => [...issues, ...lensIssues], [issues, lensIssues])

  const errors = issuesAll.filter((i) => i.severity === 'error')
  const warnings = issuesAll.filter((i) => i.severity === 'warning')

  return (
    <div className="border-t border-slate-200 bg-white text-xs">
      {open && issuesAll.length > 0 && (
        <div className="max-h-48 overflow-auto border-b border-slate-100">
          {issuesAll.map((iss, i) => (
            <button
              key={i}
              onClick={() => iss.element_id && select(iss.element_id)}
              className="flex w-full items-start gap-2 px-3 py-1.5 text-left hover:bg-slate-50"
            >
              <span className={iss.severity === 'error' ? 'text-red-500' : 'text-amber-500'}>
                {iss.severity === 'error' ? '⛔' : '⚠'}
              </span>
              <span className="flex-1 text-slate-600">{iss.message}</span>
              {iss.element_id && <span className="font-mono text-[10px] text-blue-500">{iss.element_id}</span>}
            </button>
          ))}
        </div>
      )}
      <div className="flex items-center gap-3 px-3 py-1.5">
        <button onClick={() => setOpen((o) => !o)} className="flex items-center gap-1.5 font-medium">
          {reconciling ? (
            <span className="text-slate-400">⟳ validating…</span>
          ) : valid && errors.length === 0 ? (
            <span className="text-emerald-600">● valid</span>
          ) : (
            <span className="text-red-600">⚠ {errors.length} error{errors.length === 1 ? '' : 's'}</span>
          )}
          {warnings.length > 0 && <span className="text-amber-600">· {warnings.length} warning{warnings.length === 1 ? '' : 's'}</span>}
        </button>
        <span className="text-slate-300">|</span>
        <span className="text-slate-500">topo {topo.length ? 'OK' : '—'}</span>
        <span className="text-slate-300">|</span>
        <span className="text-slate-500">{count} elems</span>
        <span className="ml-auto text-slate-400">{dirty ? 'unsaved changes' : 'saved'}</span>
        {issuesAll.length > 0 && (
          <button onClick={() => setOpen((o) => !o)} className="text-slate-400 hover:text-slate-600">
            {open ? 'hide issues ▾' : 'show issues ▸'}
          </button>
        )}
      </div>
    </div>
  )
}
