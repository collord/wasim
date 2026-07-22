import { useMemo, useState } from 'react'
import { useStore, useElements } from '../../store'
import type { ObjectiveStatKind, OptDirection, OptimizationSpec, OptVariable } from '../../types'

// Statistic options; 'final' means deterministic (no statistic → single value).
const STAT_OPTIONS: { value: ObjectiveStatKind | 'final'; label: string }[] = [
  { value: 'final', label: 'Final value (deterministic)' },
  { value: 'mean', label: 'Mean' },
  { value: 'percentile', label: 'Percentile' },
  { value: 'peak', label: 'Peak (max over time)' },
  { value: 'valley', label: 'Valley (min over time)' },
  { value: 'sum', label: 'Sum' },
]

/**
 * The optimization UI (spec §11) over the engine's `optimize_v2` (Box's complex). Pick an
 * objective (element + statistic + max/min) and decision variables (editable Fixed nodes with
 * bounds), run, and see the optimum + search trace. Constraints are engine-supported but need
 * an expression condition; that editor is deferred (empty constraint set for now).
 */
export function OptimizationTab() {
  const elements = useElements()
  const runOptimization = useStore((s) => s.runOptimization)
  const status = useStore((s) => s.optStatus)
  const results = useStore((s) => s.optResults)
  const error = useStore((s) => s.optError)
  const setConstant = useStore((s) => s.setConstant)

  // Candidate decision variables: editable fixed scalars with bounds (min & max).
  const candidates = useMemo(
    () => elements.filter((e) => e.value_rule === 'fixed' && e.editable && e.value !== null
      && e.bounds && e.bounds.min != null && e.bounds.max != null),
    [elements],
  )

  const [selected, setSelected] = useState<Set<string>>(new Set())
  const [objectiveId, setObjectiveId] = useState('')
  const [direction, setDirection] = useState<OptDirection>('minimize')
  const [stat, setStat] = useState<ObjectiveStatKind | 'final'>('final')
  const [percentile, setPercentile] = useState(50)

  const objectiveOptions = elements

  const toggle = (id: string) =>
    setSelected((prev) => {
      const next = new Set(prev)
      next.has(id) ? next.delete(id) : next.add(id)
      return next
    })

  const canRun = objectiveId !== '' && selected.size > 0 && status !== 'running'

  const run = () => {
    const variables: OptVariable[] = candidates
      .filter((e) => selected.has(e.id))
      .map((e) => ({
        element_id: e.id,
        lower: { value: e.bounds!.min!, unit: e.unit },
        upper: { value: e.bounds!.max!, unit: e.unit },
        initial: { value: e.value!, unit: e.unit },
      }))
    const spec: OptimizationSpec = {
      objective: {
        element_id: objectiveId,
        direction,
        statistic: stat === 'final' ? null : { kind: stat, ...(stat === 'percentile' ? { p: percentile } : {}) },
      },
      variables,
      constraints: [],
    }
    runOptimization(spec)
  }

  const applyOptimum = () => {
    if (!results) return
    for (const v of results.variables) setConstant(v.element_id, v.value)
  }

  const label = (id: string) => elements.find((e) => e.id === id)?.name ?? id

  return (
    <div className="mx-auto max-w-3xl space-y-5">
      <div>
        <h2 className="text-lg font-semibold text-slate-800">Optimization</h2>
        <p className="text-sm text-slate-500">
          Box’s-complex search over editable variables. Choose an objective and the variables to vary.
        </p>
      </div>

      {/* Objective */}
      <section className="rounded-lg border border-slate-200 bg-white p-4">
        <h3 className="mb-3 text-sm font-semibold text-slate-700">Objective</h3>
        <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
          <label className="col-span-2 block">
            <span className="mb-0.5 block text-[11px] font-medium text-slate-500">Element</span>
            <select value={objectiveId} onChange={(e) => setObjectiveId(e.target.value)}
              className="w-full rounded border border-slate-300 px-2 py-1 text-xs">
              <option value="">— select —</option>
              {objectiveOptions.map((e) => <option key={e.id} value={e.id}>{e.name}</option>)}
            </select>
          </label>
          <label className="block">
            <span className="mb-0.5 block text-[11px] font-medium text-slate-500">Direction</span>
            <select value={direction} onChange={(e) => setDirection(e.target.value as OptDirection)}
              className="w-full rounded border border-slate-300 px-2 py-1 text-xs">
              <option value="minimize">Minimize</option>
              <option value="maximize">Maximize</option>
            </select>
          </label>
          <label className="block">
            <span className="mb-0.5 block text-[11px] font-medium text-slate-500">Statistic</span>
            <select value={stat} onChange={(e) => setStat(e.target.value as ObjectiveStatKind | 'final')}
              className="w-full rounded border border-slate-300 px-2 py-1 text-xs">
              {STAT_OPTIONS.map((o) => <option key={o.value} value={o.value}>{o.label}</option>)}
            </select>
          </label>
          {stat === 'percentile' && (
            <label className="block">
              <span className="mb-0.5 block text-[11px] font-medium text-slate-500">Percentile</span>
              <input type="number" min={0} max={100} value={percentile}
                onChange={(e) => setPercentile(parseFloat(e.target.value))}
                className="w-full rounded border border-slate-300 px-2 py-1 text-xs" />
            </label>
          )}
        </div>
      </section>

      {/* Variables */}
      <section className="rounded-lg border border-slate-200 bg-white p-4">
        <h3 className="mb-3 text-sm font-semibold text-slate-700">Decision variables</h3>
        {candidates.length === 0 ? (
          <p className="text-xs text-slate-400">
            No eligible variables. Mark a Constant as <em>editable</em> with min/max bounds in the Inspector to use it here.
          </p>
        ) : (
          <div className="space-y-1">
            {candidates.map((e) => (
              <label key={e.id} className="flex items-center gap-2 rounded px-2 py-1 text-xs hover:bg-slate-50">
                <input type="checkbox" checked={selected.has(e.id)} onChange={() => toggle(e.id)} className="h-3.5 w-3.5" />
                <span className="flex-1 font-medium text-slate-700">{e.name}</span>
                <span className="text-slate-400">[{e.bounds!.min} … {e.bounds!.max}]{e.unit !== '1' ? ` ${e.unit}` : ''}</span>
              </label>
            ))}
          </div>
        )}
      </section>

      <div className="flex items-center gap-3">
        <button onClick={run} disabled={!canRun}
          className="rounded bg-blue-600 px-4 py-1.5 text-sm font-semibold text-white hover:bg-blue-500 disabled:opacity-40">
          {status === 'running' ? '⟳ Optimizing…' : 'Run optimization'}
        </button>
        {!canRun && status !== 'running' && (
          <span className="text-xs text-slate-400">Pick an objective and at least one variable.</span>
        )}
      </div>

      {error && <p className="rounded bg-red-50 px-3 py-2 text-xs text-red-600">{error}</p>}

      {/* Results */}
      {results && (
        <section className="rounded-lg border border-slate-200 bg-white p-4">
          <div className="mb-3 flex items-center justify-between">
            <h3 className="text-sm font-semibold text-slate-700">Optimum</h3>
            <span className={`text-xs font-medium ${results.converged ? 'text-emerald-600' : 'text-amber-600'}`}>
              {results.converged ? '✓ converged' : '⚠ hit iteration cap'} · {results.evaluations} evaluations
            </span>
          </div>
          <div className="mb-3 rounded bg-slate-50 px-3 py-2 text-sm">
            <span className="text-slate-500">Objective ({label(objectiveId)}, {direction}):</span>{' '}
            <span className="font-mono font-semibold text-slate-800">{fmt(results.objective)}</span>
          </div>
          <table className="w-full text-xs">
            <thead>
              <tr className="text-left text-slate-400">
                <th className="pb-1 font-medium">Variable</th>
                <th className="pb-1 text-right font-medium">Optimal value</th>
              </tr>
            </thead>
            <tbody>
              {results.variables.map((v) => (
                <tr key={v.element_id} className="border-t border-slate-100">
                  <td className="py-1 text-slate-600">{label(v.element_id)}</td>
                  <td className="py-1 text-right font-mono text-slate-800">{fmt(v.value)}</td>
                </tr>
              ))}
            </tbody>
          </table>
          <button onClick={applyOptimum}
            className="mt-3 rounded border border-slate-300 px-3 py-1 text-xs font-medium text-slate-600 hover:bg-slate-50">
            Apply optimum to model
          </button>
        </section>
      )}
    </div>
  )
}

function fmt(v: number): string {
  if (!isFinite(v)) return String(v)
  return Number.isInteger(v) ? String(v) : String(+v.toPrecision(6))
}
