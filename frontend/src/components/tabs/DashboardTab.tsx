import { useState } from 'react'
import { useStore } from '../../store'
import type { ElementSummary } from '../../types'
import { dispOf, toDisplay, fromDisplay } from '../../display'

function SaveParamsButton() {
  const saveParameters = useStore((s) => s.saveParameters)
  return (
    <button
      onClick={saveParameters}
      className="rounded border border-slate-300 bg-white px-2.5 py-1 text-xs font-medium text-slate-600 hover:bg-slate-50 active:bg-slate-100"
    >
      Save parameters
    </button>
  )
}

// ── Distribution parameter label map ─────────────────────────────────────────

const PARAM_LABELS: Record<string, Record<string, string>> = {
  normal: { mean: 'Mean', stddev: 'Std Dev' },
  lognormal: { mean: 'μ (log-space)', stddev: 'σ (log-space)' },
  lognormal_moments: { mean: 'Mean', stddev: 'Std Dev' },
  uniform: { min: 'Min', max: 'Max' },
  triangular: { min: 'Min', mode: 'Mode', max: 'Max' },
  exponential: { mean: 'Mean' },
  gamma: { shape: 'Shape (α)', scale: 'Scale (β)' },
  beta: { alpha: 'Alpha', beta: 'Beta' },
  weibull: { shape: 'Shape (k)', scale: 'Scale (λ)' },
  pearson_v: { shape: 'Shape', scale: 'Scale' },
  pearson_iii: { mean: 'Mean', stddev: 'Std Dev', skewness: 'Skewness' },
  discrete_uniform: { min: 'Min', max: 'Max' },
  pert: { min: 'Min', mode: 'Mode', max: 'Max' },
  pareto: { scale: 'Scale', shape: 'Shape' },
  extreme_value: { location: 'Location', scale: 'Scale' },
  student_t: { degrees_of_freedom: 'DoF', location: 'Location', scale: 'Scale' },
}

// ── Components ────────────────────────────────────────────────────────────────

function ConstantInput({ elem }: { elem: ElementSummary }) {
  const setConstant = useStore((s) => s.setConstant)
  // Show + edit in display units; store the canonical value.
  const d = dispOf(elem)
  const raw = elem.value
  const current = raw !== null && Number.isFinite(raw) ? toDisplay(raw, d) : ''
  const unit = d.unit !== '1' ? d.unit : ''
  const boundMin = elem.bounds?.min != null ? toDisplay(elem.bounds.min, d) : undefined
  const boundMax = elem.bounds?.max != null ? toDisplay(elem.bounds.max, d) : undefined

  return (
    <div className="flex items-center gap-3">
      <label className="min-w-0 flex-1">
        <span className="block text-sm font-medium text-slate-700">{elem.name}</span>
        {elem.description && (
          <span className="block text-xs text-slate-400">{elem.description}</span>
        )}
      </label>
      <div className="flex items-center gap-1.5">
        <input
          type="number"
          value={current}
          onChange={(e) => {
            const v = parseFloat(e.target.value)
            if (Number.isFinite(v)) setConstant(elem.id, fromDisplay(v, d))
          }}
          min={boundMin}
          max={boundMax}
          step="any"
          className="w-28 rounded border border-slate-300 px-2 py-1 text-right font-mono text-sm focus:border-blue-500 focus:outline-none focus:ring-1 focus:ring-blue-500"
        />
        {unit && <span className="text-xs text-slate-400 whitespace-nowrap">{unit}</span>}
      </div>
    </div>
  )
}

function RvParamInput({ elem }: { elem: ElementSummary }) {
  const setRvParam = useStore((s) => s.setRvParam)
  const dist = elem.dist
  if (!dist) return null
  const labels = PARAM_LABELS[dist.family] ?? {}

  return (
    <div className="space-y-2">
      <div className="flex items-baseline gap-2">
        <span className="text-sm font-medium text-slate-700">{elem.name}</span>
        <span className="rounded bg-blue-50 px-1.5 py-0.5 text-xs font-medium text-blue-600">
          {dist.family}
        </span>
        {dist.truncation && (
          <span className="text-xs text-slate-400">
            truncated [{dist.truncation.min ?? '−∞'}, {dist.truncation.max ?? '+∞'}]
          </span>
        )}
      </div>
      <div className="grid grid-cols-2 gap-x-4 gap-y-2 pl-3 sm:grid-cols-3">
        {Object.entries(dist.parameters).flatMap(([pname, pval]) => {
          // A numeric param is either a bare number or a {value, unit} quantity. Some
          // distributions (e.g. `external`) carry non-numeric or null params — those aren't
          // editable numbers, so skip them rather than crash.
          const num =
            typeof pval === 'number'
              ? pval
              : (typeof pval === 'object' && pval !== null && 'value' in pval &&
                 typeof (pval as { value: unknown }).value === 'number'
                  ? (pval as { value: number }).value
                  : null)
          if (num === null) return []
          const unit =
            typeof pval === 'object' && pval !== null && 'unit' in pval && pval.unit !== '1'
              ? (pval as { unit: string }).unit
              : ''
          return [(
            <div key={pname} className="flex flex-col gap-0.5">
              <label className="text-xs text-slate-500">{labels[pname] ?? pname}</label>
              <div className="flex items-center gap-1">
                <input
                  type="number"
                  value={num}
                  onChange={(e) => setRvParam(elem.id, pname, parseFloat(e.target.value))}
                  step="any"
                  className="w-full rounded border border-slate-300 px-2 py-1 font-mono text-sm focus:border-blue-500 focus:outline-none focus:ring-1 focus:ring-blue-500"
                />
                {unit && <span className="whitespace-nowrap text-xs text-slate-400">{unit}</span>}
              </div>
            </div>
          )]
        })}
      </div>
    </div>
  )
}

// ── Curated dashboard: slider input + output tile (§12) ────────────────────────

/** A bounded editable constant rendered as a slider + number (uses its bounds). Falls back
 *  to the plain number field when the element has no finite bounds. */
function SliderInput({ elem }: { elem: ElementSummary }) {
  const setConstant = useStore((s) => s.setConstant)
  const d = dispOf(elem)
  const lo = elem.bounds?.min, hi = elem.bounds?.max
  if (lo == null || hi == null || !Number.isFinite(lo) || !Number.isFinite(hi) || hi <= lo) {
    return <ConstantInput elem={elem} />
  }
  const dLo = toDisplay(lo, d), dHi = toDisplay(hi, d)
  const cur = elem.value != null && Number.isFinite(elem.value) ? toDisplay(elem.value, d) : dLo
  const step = (dHi - dLo) / 100
  const unit = d.unit !== '1' ? d.unit : ''
  return (
    <div>
      <div className="mb-1 flex items-baseline justify-between">
        <span className="text-sm font-medium text-slate-700">{elem.name}</span>
        <span className="font-mono text-sm text-slate-600">{(+cur.toPrecision(6))}{unit ? ` ${unit}` : ''}</span>
      </div>
      <input
        type="range" min={dLo} max={dHi} step={step || 'any'} value={cur}
        onChange={(e) => setConstant(elem.id, fromDisplay(parseFloat(e.target.value), d))}
        className="w-full accent-blue-600"
      />
      <div className="flex justify-between text-[10px] text-slate-400"><span>{+dLo.toPrecision(4)}</span><span>{+dHi.toPrecision(4)}</span></div>
    </div>
  )
}

/** A single output display: mean + p05–p95 band of the element's final values from the last run. */
function OutputTile({ id }: { id: string }) {
  const results = useStore((s) => s.results)
  const el = results?.elements[id]
  const label = el?.label ?? id
  if (!el || el.final_values.length === 0) {
    return (
      <div className="rounded-lg border border-slate-200 bg-white p-3">
        <div className="text-xs font-medium text-slate-500">{label}</div>
        <div className="mt-1 text-sm text-slate-400">run to see output</div>
      </div>
    )
  }
  const vals = el.final_values.filter(Number.isFinite).sort((a, b) => a - b)
  const n = vals.length
  const mean = vals.reduce((a, b) => a + b, 0) / n
  const p = (q: number) => vals[Math.min(n - 1, Math.max(0, Math.round((q / 100) * (n - 1))))]
  const unit = el.unit && el.unit !== '1' ? ` ${el.unit}` : ''
  const f = (v: number) => (Number.isFinite(v) ? +v.toPrecision(5) : v)
  return (
    <div className="rounded-lg border border-slate-200 bg-white p-3">
      <div className="truncate text-xs font-medium text-slate-500">{label}</div>
      <div className="mt-1 font-mono text-xl font-semibold text-slate-800">{f(mean)}<span className="text-xs font-normal text-slate-400">{unit}</span></div>
      <div className="mt-0.5 text-[11px] text-slate-400">p05 {f(p(5))} · p95 {f(p(95))}</div>
    </div>
  )
}

// ── Run controls ──────────────────────────────────────────────────────────────

function RunControls() {
  const status = useStore((s) => s.status)
  const nRealizations = useStore((s) => s.nRealizations)
  const seed = useStore((s) => s.seed)
  const simDuration = useStore((s) => s.simDuration)
  const simDurationDisp = useStore((s) => s.simDurationDisp)
  const simTimestep = useStore((s) => s.simTimestep)
  const simTimestepDisp = useStore((s) => s.simTimestepDisp)
  const setNRealizations = useStore((s) => s.setNRealizations)
  const setSeed = useStore((s) => s.setSeed)
  const setSimDuration = useStore((s) => s.setSimDuration)
  const setSimTimestep = useStore((s) => s.setSimTimestep)
  const run = useStore((s) => s.run)
  const errorMessage = useStore((s) => s.errorMessage)

  const isRunning = status === 'running'

  const inputCls = "rounded border border-slate-300 px-2 py-1 font-mono text-sm focus:border-blue-500 focus:outline-none focus:ring-1 focus:ring-blue-500"

  return (
    <div className="sticky bottom-0 rounded-b-lg border-t border-slate-200 bg-white px-4 py-3">
      <div className="mb-3 grid grid-cols-2 gap-x-6 gap-y-2 sm:grid-cols-4">
        <div className="flex flex-col gap-0.5">
          <label htmlFor="run-duration" className="text-xs text-slate-500">Duration</label>
          <div className="flex items-center gap-1">
            <input
              id="run-duration"
              type="number"
              value={simDuration === null ? '' : simDuration * simDurationDisp.factor + simDurationDisp.offset}
              onChange={(e) => setSimDuration(parseFloat(e.target.value))}
              min={0}
              step="any"
              className={`w-full ${inputCls}`}
            />
            <span className="text-xs text-slate-400 whitespace-nowrap">{simDurationDisp.unit}</span>
          </div>
        </div>
        <div className="flex flex-col gap-0.5">
          <label htmlFor="run-timestep" className="text-xs text-slate-500">Timestep</label>
          <div className="flex items-center gap-1">
            <input
              id="run-timestep"
              type="number"
              value={simTimestep === null ? '' : simTimestep * simTimestepDisp.factor + simTimestepDisp.offset}
              onChange={(e) => setSimTimestep(parseFloat(e.target.value))}
              min={0}
              step="any"
              className={`w-full ${inputCls}`}
            />
            <span className="text-xs text-slate-400 whitespace-nowrap">{simTimestepDisp.unit}</span>
          </div>
        </div>
        <div className="flex flex-col gap-0.5">
          <label htmlFor="run-realizations" className="text-xs text-slate-500">Realizations</label>
          <input
            id="run-realizations"
            type="number"
            value={nRealizations}
            onChange={(e) => setNRealizations(parseInt(e.target.value, 10) || 1)}
            min={1}
            max={100000}
            className={inputCls}
          />
        </div>
        <div className="flex flex-col gap-0.5">
          <label htmlFor="run-seed" className="text-xs text-slate-500">Seed</label>
          <input
            id="run-seed"
            type="number"
            value={seed ?? ''}
            placeholder="random"
            onChange={(e) => setSeed(e.target.value ? parseInt(e.target.value, 10) : null)}
            className={inputCls}
          />
        </div>
      </div>
      <div className="flex flex-wrap items-center gap-4">
        <button
          onClick={run}
          disabled={isRunning}
          className={`ml-auto flex items-center gap-2 rounded-md px-5 py-2 text-sm font-semibold text-white transition-colors ${
            isRunning
              ? 'cursor-not-allowed bg-blue-300'
              : 'bg-blue-600 hover:bg-blue-700 active:bg-blue-800'
          }`}
        >
          {isRunning && (
            <svg className="h-4 w-4 animate-spin" viewBox="0 0 24 24" fill="none">
              <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
              <path className="opacity-75" fill="currentColor"
                d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
            </svg>
          )}
          {isRunning ? 'Running…' : 'Run Simulation'}
        </button>
      </div>
      {errorMessage && (
        <p className="mt-2 text-sm text-red-600">{errorMessage}</p>
      )}
    </div>
  )
}

// ── Dashboard tab ─────────────────────────────────────────────────────────────

export function DashboardTab() {
  const summary = useStore((s) => s.modelSummary)
  const dashboard = useStore((s) => s.doc?.view?.dashboard)
  const toggleItem = useStore((s) => s.toggleDashboardItem)
  const results = useStore((s) => s.results)
  const [configuring, setConfiguring] = useState(false)

  if (!summary) {
    return (
      <p className="py-12 text-center text-sm text-slate-400">
        No model loaded.
      </p>
    )
  }

  const editableElems = summary.elements.filter((e) => e.editable)
  const curatedInputs = (dashboard?.inputs ?? []).map((id) => summary.elements.find((e) => e.id === id)).filter(Boolean) as ElementSummary[]
  const curatedOutputs = dashboard?.outputs ?? []
  const outputCandidates = results ? results.output_ids : summary.elements.map((e) => e.id)
  const hasCurated = (dashboard?.inputs.length ?? 0) > 0 || (dashboard?.outputs.length ?? 0) > 0

  // ── Author configure mode: pick which inputs/outputs appear on the dashboard ──
  if (configuring) {
    return (
      <div className="space-y-5">
        <div className="flex items-center justify-between">
          <div>
            <h2 className="text-lg font-semibold text-slate-800">Configure dashboard</h2>
            <p className="text-sm text-slate-500">Curate a what-if panel: pick input controls and output displays.</p>
          </div>
          <button onClick={() => setConfiguring(false)} className="rounded bg-blue-600 px-3 py-1.5 text-xs font-semibold text-white hover:bg-blue-500">Done</button>
        </div>
        <div className="grid gap-4 sm:grid-cols-2">
          <div className="rounded-lg border border-slate-200 bg-white p-4">
            <h3 className="mb-2 text-sm font-semibold text-slate-700">Inputs (editable parameters)</h3>
            {editableElems.length === 0 ? <p className="text-xs text-slate-400">No editable parameters. Mark constants editable in the Inspector.</p> : (
              <div className="space-y-1">
                {editableElems.map((e) => (
                  <label key={e.id} className="flex items-center gap-2 text-xs text-slate-600">
                    <input type="checkbox" checked={dashboard?.inputs.includes(e.id) ?? false} onChange={() => toggleItem('inputs', e.id)} className="h-3.5 w-3.5" />
                    <span className="flex-1">{e.name}</span>
                    <span className="text-slate-400">{e.value_rule}</span>
                  </label>
                ))}
              </div>
            )}
          </div>
          <div className="rounded-lg border border-slate-200 bg-white p-4">
            <h3 className="mb-2 text-sm font-semibold text-slate-700">Outputs (result displays)</h3>
            <div className="space-y-1">
              {outputCandidates.map((id) => {
                const el = summary.elements.find((e) => e.id === id)
                return (
                  <label key={id} className="flex items-center gap-2 text-xs text-slate-600">
                    <input type="checkbox" checked={curatedOutputs.includes(id)} onChange={() => toggleItem('outputs', id)} className="h-3.5 w-3.5" />
                    <span className="flex-1">{el?.name ?? id}</span>
                  </label>
                )
              })}
            </div>
          </div>
        </div>
      </div>
    )
  }

  // ── Curated dashboard view (when configured) ──
  if (hasCurated) {
    return (
      <div className="flex flex-col">
        <div className="mb-4 flex items-center justify-between">
          <h2 className="text-lg font-semibold text-slate-800">Dashboard</h2>
          <div className="flex gap-2">
            <SaveParamsButton />
            <button onClick={() => setConfiguring(true)} className="rounded border border-slate-300 bg-white px-2.5 py-1 text-xs font-medium text-slate-600 hover:bg-slate-50">Configure</button>
          </div>
        </div>
        {curatedOutputs.length > 0 && (
          <div className="mb-5 grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-4">
            {curatedOutputs.map((id) => <OutputTile key={id} id={id} />)}
          </div>
        )}
        {curatedInputs.length > 0 && (
          <div className="rounded-lg border border-slate-200 bg-white">
            <div className="border-b border-slate-100 px-4 py-2"><h3 className="text-sm font-semibold text-slate-700">Inputs</h3></div>
            <div className="space-y-4 p-4">
              {curatedInputs.map((e) => (
                <div key={e.id}>{e.value_rule === 'sample' ? <RvParamInput elem={e} /> : <SliderInput elem={e} />}</div>
              ))}
            </div>
          </div>
        )}
        <RunControls />
      </div>
    )
  }

  const containerNames = Object.fromEntries(summary.containers.map((c) => [c.id, c.name]))

  // `editableElems` (editable fixed + sample nodes) is computed above; group by container.
  const groups = new Map<string, ElementSummary[]>()
  groups.set('(top level)', [])
  for (const c of summary.containers) groups.set(c.id, [])
  for (const e of editableElems) {
    const key = e.container ?? '(top level)'
    if (!groups.has(key)) groups.set(key, [])
    groups.get(key)!.push(e)
  }

  const nonEmptyGroups = Array.from(groups.entries()).filter(([, elems]) => elems.length > 0)

  const loadedLabel = `${summary.element_count} elements`

  return (
    <div className="flex flex-col">
      <div className="mb-4 flex items-center justify-between">
        <p className="text-xs text-slate-400">{loadedLabel}</p>
        <div className="flex gap-2">
          <SaveParamsButton />
          <button onClick={() => setConfiguring(true)} className="rounded border border-slate-300 bg-white px-2.5 py-1 text-xs font-medium text-slate-600 hover:bg-slate-50">
            Configure dashboard
          </button>
        </div>
      </div>

      <div className="space-y-6">
        {nonEmptyGroups.length === 0 ? (
          <p className="rounded-lg border border-slate-200 bg-white p-6 text-center text-sm text-slate-400">
            No editable parameters in this model.
          </p>
        ) : (
          nonEmptyGroups.map(([containerId, elems]) => (
            <div key={containerId} className="rounded-lg border border-slate-200 bg-white">
              <div className="border-b border-slate-100 px-4 py-2">
                <h3 className="text-sm font-semibold text-slate-700">
                  {containerId === '(top level)'
                    ? 'Parameters'
                    : (containerNames[containerId] ?? containerId)}
                </h3>
              </div>
              <div className="divide-y divide-slate-50">
                {elems.map((e) => (
                  <div key={e.id} className="px-4 py-3">
                    {e.value_rule === 'sample' ? (
                      <RvParamInput elem={e} />
                    ) : (
                      <ConstantInput elem={e} />
                    )}
                  </div>
                ))}
              </div>
            </div>
          ))
        )}
      </div>

      <RunControls />
    </div>
  )
}
