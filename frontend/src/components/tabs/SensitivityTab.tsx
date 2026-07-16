import { useMemo, useState } from 'react'
import {
  LineChart,
  Line,
  BarChart,
  Bar,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  Legend,
  ReferenceDot,
  ResponsiveContainer,
  Cell,
} from 'recharts'
import { useStore } from '../../store'
import { dispOf, toDisplay, fromDisplay, unitLabel } from '../../display'
import type {
  ElementSummary,
  SensitivityMethod,
  SensitivitySpec,
  SensitivityStatKind,
  SweepVar,
} from '../../types'

const PALETTE = [
  '#2563eb', '#dc2626', '#16a34a', '#d97706',
  '#7c3aed', '#0891b2', '#db2777', '#65a30d',
]

const fmt = (v: number) =>
  Math.abs(v) >= 1e4 || (v !== 0 && Math.abs(v) < 1e-3)
    ? v.toExponential(2)
    : Number(v.toFixed(4)).toString()

const STAT_LABELS: Record<SensitivityStatKind, string> = {
  mean: 'Mean',
  percentile: 'Percentile',
  peak: 'Peak (max)',
  valley: 'Valley (min)',
  sum: 'Sum',
}

// Per-variable config held in the tab (transient — never persisted into the model).
interface VarConfig {
  enabled: boolean
  lower: number // display units
  upper: number // display units
  steps: number
  base: number // display units (defaults to current value)
}

/** Sweepable inputs are **fixed-scalar** nodes — the engine can only set a single scalar per
 *  input (`set_variable`). This deliberately does NOT require the dashboard's `editable` flag:
 *  that flag is a curatorial "surface this for hand-editing" marker, and no corpus model sets
 *  it on fixed scalars, yet those scalars are exactly what optimization already sweeps. Sample
 *  nodes are excluded — varying a distribution isn't a single-scalar sweep. */
function sweepableInputs(elems: ElementSummary[]): ElementSummary[] {
  return elems.filter(
    (e) => e.value_rule === 'fixed' && e.value !== null && Number.isFinite(e.value),
  )
}

function defaultVarConfig(e: ElementSummary): VarConfig {
  const d = dispOf(e)
  const cur = toDisplay(e.value as number, d)
  // Range from declared bounds if present, else ±50% of the current value (or ±1 at zero).
  const lo = e.bounds?.min != null ? toDisplay(e.bounds.min, d) : cur >= 0 ? cur * 0.5 : cur * 1.5
  const hi = e.bounds?.max != null ? toDisplay(e.bounds.max, d) : cur >= 0 ? cur * 1.5 : cur * 0.5
  const span = hi - lo
  return {
    enabled: false,
    lower: span === 0 ? cur - 1 : lo,
    upper: span === 0 ? cur + 1 : hi,
    steps: 9,
    base: cur,
  }
}

export function SensitivityTab() {
  const summary = useStore((s) => s.modelSummary)
  const runSensitivity = useStore((s) => s.runSensitivity)
  const sensStatus = useStore((s) => s.sensStatus)
  const sensResults = useStore((s) => s.sensResults)
  const sensError = useStore((s) => s.sensError)

  const inputs = useMemo(() => (summary ? sweepableInputs(summary.elements) : []), [summary])
  const allElems = summary?.elements ?? []

  // Config state.
  const [cfg, setCfg] = useState<Record<string, VarConfig>>({})
  const [resultId, setResultId] = useState<string>('')
  const [statKind, setStatKind] = useState<SensitivityStatKind | ''>('')
  const [percentile, setPercentile] = useState<number>(50)
  const [method, setMethod] = useState<SensitivityMethod>('one_at_a_time')
  // Plot the one-at-a-time X axis normalized 0→1 over each variable's [lower,upper]
  // (GoldSim's "Normalized Values" convention) instead of raw input units.
  const [normalizedX, setNormalizedX] = useState(false)

  if (!summary) {
    return <p className="py-12 text-center text-sm text-slate-400">No model loaded.</p>
  }

  const getCfg = (e: ElementSummary): VarConfig => cfg[e.id] ?? defaultVarConfig(e)
  const setVarCfg = (id: string, patch: Partial<VarConfig>) =>
    setCfg((c) => ({ ...c, [id]: { ...(c[id] ?? defaultVarConfig(inputs.find((e) => e.id === id)!)), ...patch } }))

  const enabledVars = inputs.filter((e) => getCfg(e).enabled)
  const resultElem = allElems.find((e) => e.id === resultId)
  const canRun = enabledVars.length > 0 && !!resultId && sensStatus !== 'running'

  const buildSpec = (): SensitivitySpec => {
    const variables: SweepVar[] = enabledVars.map((e) => {
      const d = dispOf(e)
      const c = getCfg(e)
      // Config is in display units; the engine sweeps canonical values.
      return {
        element_id: e.id,
        lower: fromDisplay(c.lower, d),
        upper: fromDisplay(c.upper, d),
        base: fromDisplay(c.base, d),
        steps: Math.max(2, Math.round(c.steps)),
      }
    })
    const statistic =
      statKind === ''
        ? null
        : { kind: statKind, ...(statKind === 'percentile' ? { p: percentile } : {}) }
    return { result: { element_id: resultId, statistic }, variables, method }
  }

  if (inputs.length === 0) {
    return (
      <p className="rounded-lg border border-slate-200 bg-white p-6 text-center text-sm text-slate-400">
        No sweepable inputs in this model. Sensitivity analysis needs at least one
        fixed-value (scalar) parameter to vary.
      </p>
    )
  }

  return (
    <div className="flex flex-col gap-6">
      {/* Config */}
      <div className="rounded-lg border border-slate-200 bg-white">
        <div className="border-b border-slate-100 px-4 py-2">
          <h3 className="text-sm font-semibold text-slate-700">Input variables to sweep</h3>
        </div>
        <div className="divide-y divide-slate-50">
          {inputs.map((e) => {
            const c = getCfg(e)
            const unit = unitLabel(e)
            const u = unit !== '1' ? unit : ''
            return (
              <div key={e.id} className="flex flex-wrap items-center gap-3 px-4 py-3">
                <label className="flex min-w-0 flex-1 items-center gap-2">
                  <input
                    type="checkbox"
                    checked={c.enabled}
                    onChange={(ev) => setVarCfg(e.id, { enabled: ev.target.checked })}
                  />
                  <span className="truncate text-sm font-medium text-slate-700">{e.name}</span>
                </label>
                <NumField label="Base" value={c.base} unit={u} disabled={!c.enabled}
                  onChange={(v) => setVarCfg(e.id, { base: v })} />
                <NumField label="Lower" value={c.lower} unit={u} disabled={!c.enabled}
                  onChange={(v) => setVarCfg(e.id, { lower: v })} />
                <NumField label="Upper" value={c.upper} unit={u} disabled={!c.enabled}
                  onChange={(v) => setVarCfg(e.id, { upper: v })} />
                {method === 'one_at_a_time' && (
                  <NumField label="Steps" value={c.steps} unit="" disabled={!c.enabled} intOnly
                    onChange={(v) => setVarCfg(e.id, { steps: v })} />
                )}
              </div>
            )
          })}
        </div>
      </div>

      {/* Result + method */}
      <div className="flex flex-wrap items-end gap-4 rounded-lg border border-slate-200 bg-white px-4 py-3">
        <label className="flex flex-col gap-1">
          <span className="text-xs font-medium text-slate-500">Result element</span>
          <select
            value={resultId}
            onChange={(e) => setResultId(e.target.value)}
            className="rounded border border-slate-300 px-2 py-1 text-sm focus:border-blue-500 focus:outline-none"
          >
            <option value="">— pick —</option>
            {allElems.map((e) => (
              <option key={e.id} value={e.id}>{e.name}</option>
            ))}
          </select>
        </label>

        <label className="flex flex-col gap-1">
          <span className="text-xs font-medium text-slate-500">Statistic</span>
          <select
            value={statKind}
            onChange={(e) => setStatKind(e.target.value as SensitivityStatKind | '')}
            className="rounded border border-slate-300 px-2 py-1 text-sm focus:border-blue-500 focus:outline-none"
          >
            <option value="">Deterministic (single value)</option>
            {(Object.keys(STAT_LABELS) as SensitivityStatKind[]).map((k) => (
              <option key={k} value={k}>{STAT_LABELS[k]}</option>
            ))}
          </select>
        </label>

        {statKind === 'percentile' && (
          <label className="flex flex-col gap-1">
            <span className="text-xs font-medium text-slate-500">Percentile</span>
            <input
              type="number" min={0} max={100} value={percentile}
              onChange={(e) => setPercentile(parseFloat(e.target.value) || 0)}
              className="w-20 rounded border border-slate-300 px-2 py-1 text-right font-mono text-sm focus:border-blue-500 focus:outline-none"
            />
          </label>
        )}

        <label className="flex flex-col gap-1">
          <span className="text-xs font-medium text-slate-500">Method</span>
          <select
            value={method}
            onChange={(e) => setMethod(e.target.value as SensitivityMethod)}
            className="rounded border border-slate-300 px-2 py-1 text-sm focus:border-blue-500 focus:outline-none"
          >
            <option value="one_at_a_time">One-at-a-time (line sweep)</option>
            <option value="tornado">Tornado (rank by influence)</option>
          </select>
        </label>

        <button
          onClick={() => runSensitivity(buildSpec())}
          disabled={!canRun}
          className="ml-auto rounded bg-blue-600 px-4 py-1.5 text-sm font-medium text-white hover:bg-blue-700 disabled:cursor-not-allowed disabled:bg-slate-300"
        >
          {sensStatus === 'running' ? 'Running…' : 'Run sweep'}
        </button>
      </div>

      {!canRun && sensStatus !== 'running' && (
        <p className="-mt-3 text-xs text-slate-400">
          Enable at least one variable and pick a result element to run.
        </p>
      )}

      {sensStatus === 'error' && (
        <p className="rounded-lg border border-red-200 bg-red-50 p-4 text-sm text-red-700">
          {sensError}
        </p>
      )}

      {/* Results */}
      {sensStatus === 'done' && sensResults && resultElem && (
        <>
          {sensResults.curves.length > 0 && (
            <label className="-mb-2 flex items-center gap-2 self-start text-xs text-slate-500">
              <input
                type="checkbox"
                checked={normalizedX}
                onChange={(e) => setNormalizedX(e.target.checked)}
              />
              Normalize X axis (0–1 over each variable&apos;s range)
            </label>
          )}
          <SensitivityResultsView
            method={sensResults.tornado.length > 0 ? 'tornado' : 'one_at_a_time'}
            results={sensResults}
            resultUnit={unitLabel(resultElem)}
            normalizedX={normalizedX}
            baseInputOf={(id) => {
              const e = inputs.find((x) => x.id === id)
              return e ? getCfg(e).base : 0
            }}
            nameOf={(id) => allElems.find((e) => e.id === id)?.name ?? id}
          />
        </>
      )}
    </div>
  )
}

function NumField({
  label, value, unit, onChange, disabled, intOnly,
}: {
  label: string
  value: number
  unit: string
  onChange: (v: number) => void
  disabled?: boolean
  intOnly?: boolean
}) {
  return (
    <label className="flex flex-col gap-0.5">
      <span className="text-[10px] uppercase tracking-wide text-slate-400">{label}{unit ? ` (${unit})` : ''}</span>
      <input
        type="number"
        value={value}
        disabled={disabled}
        step={intOnly ? 1 : 'any'}
        onChange={(e) => {
          const v = parseFloat(e.target.value)
          if (Number.isFinite(v)) onChange(intOnly ? Math.round(v) : v)
        }}
        className="w-24 rounded border border-slate-300 px-2 py-1 text-right font-mono text-sm focus:border-blue-500 focus:outline-none disabled:bg-slate-50 disabled:text-slate-400"
      />
    </label>
  )
}

// ── Results view ─────────────────────────────────────────────────────────────

function SensitivityResultsView({
  method, results, resultUnit, normalizedX, baseInputOf, nameOf,
}: {
  method: SensitivityMethod
  results: import('../../types').SensitivityResults
  resultUnit: string
  normalizedX: boolean
  baseInputOf: (id: string) => number
  nameOf: (id: string) => string
}) {
  const u = resultUnit !== '1' ? resultUnit : ''

  if (method === 'tornado') {
    const data = results.tornado.map((b) => ({
      name: nameOf(b.element_id),
      swing: b.swing,
      low: b.low,
      high: b.high,
    }))
    return (
      <div className="rounded-lg border border-slate-200 bg-white p-4">
        <h3 className="mb-1 text-sm font-semibold text-slate-700">Tornado — inputs ranked by influence</h3>
        <p className="mb-3 text-xs text-slate-400">
          Base result: <span className="font-mono">{fmt(results.base_result)}</span> {u}. Bar = |result(upper) − result(lower)|.
        </p>
        <ResponsiveContainer width="100%" height={Math.max(120, data.length * 44)}>
          <BarChart data={data} layout="vertical" margin={{ top: 4, right: 24, bottom: 4, left: 8 }}>
            <CartesianGrid strokeDasharray="3 3" stroke="#e2e8f0" />
            <XAxis type="number" tick={{ fontSize: 11 }} tickFormatter={(v) => fmt(Number(v))}
              label={{ value: `swing${u ? ` (${u})` : ''}`, position: 'insideBottomRight', offset: -4, fontSize: 11 }} />
            <YAxis type="category" dataKey="name" tick={{ fontSize: 11 }} width={120} />
            <Tooltip formatter={(v: number, k) => [fmt(Number(v)), k]}
              labelFormatter={(l) => l} />
            <Bar dataKey="swing" radius={[0, 2, 2, 0]}>
              {data.map((_, i) => <Cell key={i} fill={PALETTE[i % PALETTE.length]} />)}
            </Bar>
          </BarChart>
        </ResponsiveContainer>
      </div>
    )
  }

  // One-at-a-time: one line chart per variable, with a base-case marker.
  return (
    <div className="flex flex-col gap-4">
      {results.curves.map((curve, i) => {
        const color = PALETTE[i % PALETTE.length]
        // Normalize X over the swept range [lower,upper] = this curve's min/max input, matching
        // GoldSim's "Normalized Values" 0→1 axis. norm(v) is identity when not normalized.
        const lo = Math.min(...curve.points.map((p) => p.input))
        const hi = Math.max(...curve.points.map((p) => p.input))
        const span = hi - lo
        const norm = (v: number) => (normalizedX && span !== 0 ? (v - lo) / span : v)
        const data = curve.points.map((p) => ({ x: norm(p.input), result: p.result }))
        // The base case lies on this curve at the swept variable's base input; base_result is
        // the model value there (all vars at base). Anchor the marker at (base input, base_result).
        const baseInput = norm(baseInputOf(curve.element_id))
        const xLabel = normalizedX
          ? `${nameOf(curve.element_id)} (normalized 0–1)`
          : `${nameOf(curve.element_id)} (input)`
        return (
          <div key={curve.element_id} className="rounded-lg border border-slate-200 bg-white p-4">
            <h3 className="mb-3 text-sm font-semibold text-slate-700">
              {nameOf(curve.element_id)} → result
            </h3>
            <ResponsiveContainer width="100%" height={240}>
              <LineChart data={data} margin={{ top: 8, right: 24, bottom: 20, left: 8 }}>
                <CartesianGrid strokeDasharray="3 3" stroke="#e2e8f0" />
                <XAxis dataKey="x" type="number" tick={{ fontSize: 11 }}
                  tickFormatter={(v) => fmt(Number(v))}
                  domain={normalizedX ? [0, 1] : ['dataMin', 'dataMax']}
                  label={{ value: xLabel, position: 'insideBottom', offset: -12, fontSize: 11 }} />
                <YAxis tick={{ fontSize: 11 }} tickFormatter={(v) => fmt(Number(v))}
                  label={{ value: u, angle: -90, position: 'insideLeft', fontSize: 11 }} />
                <Tooltip formatter={(v: number) => [fmt(Number(v)), 'result']}
                  labelFormatter={(l) => `${normalizedX ? 'norm' : 'input'} ${fmt(Number(l))}`} />
                <Legend />
                <Line type="monotone" dataKey="result" name={nameOf(curve.element_id)}
                  stroke={color} strokeWidth={2} dot={{ r: 3 }} />
                <ReferenceDot y={results.base_result} x={baseInput}
                  r={5} fill="#0f172a" stroke="#fff" strokeWidth={1.5} isFront />
              </LineChart>
            </ResponsiveContainer>
          </div>
        )
      })}
    </div>
  )
}
