import {
  ComposedChart,
  Line,
  Area,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
  BarChart,
  Bar,
} from 'recharts'
import { useStore } from '../../store'
import type { ElementResults, TimeHistoryStats } from '../../types'

// ── Chart helpers ─────────────────────────────────────────────────────────────

function buildTimeChartData(timeAxis: number[], stats: TimeHistoryStats) {
  return timeAxis.map((t, i) => ({
    t: +t.toFixed(4),
    // Outer band (p05→p95) stored as [low, high] for Area range rendering
    band_outer: [stats.p05[i], stats.p95[i]] as [number, number],
    // Inner band (p25→p75)
    band_inner: [stats.p25[i], stats.p75[i]] as [number, number],
    mean: stats.mean[i],
    p50: stats.p50[i],
  }))
}

function buildHistogramData(values: number[], bins = 30) {
  if (values.length === 0) return []
  const lo = Math.min(...values)
  const hi = Math.max(...values)
  if (lo === hi) return [{ x: lo, count: values.length }]
  const width = (hi - lo) / bins
  const counts = Array<number>(bins).fill(0)
  for (const v of values) {
    const idx = Math.min(Math.floor((v - lo) / width), bins - 1)
    counts[idx]++
  }
  return counts.map((count, i) => ({
    x: +(lo + (i + 0.5) * width).toFixed(4),
    count,
  }))
}

function fmt(n: number) {
  if (Math.abs(n) >= 1000 || (Math.abs(n) < 0.01 && n !== 0)) {
    return n.toExponential(3)
  }
  return n.toFixed(4).replace(/\.?0+$/, '')
}

// ── Time history chart ────────────────────────────────────────────────────────

function TimeHistoryChart({
  timeAxis,
  stats,
  unit,
  timeUnit,
}: {
  timeAxis: number[]
  stats: TimeHistoryStats
  unit: string
  timeUnit: string
}) {
  const data = buildTimeChartData(timeAxis, stats)
  return (
    <div>
      <h4 className="mb-2 text-xs font-semibold uppercase tracking-wider text-slate-500">
        Time History
      </h4>
      <ResponsiveContainer width="100%" height={260}>
        <ComposedChart data={data} margin={{ top: 4, right: 8, bottom: 4, left: 8 }}>
          <CartesianGrid strokeDasharray="3 3" stroke="#e2e8f0" />
          <XAxis
            dataKey="t"
            tick={{ fontSize: 11 }}
            label={{ value: timeUnit, position: 'insideBottomRight', offset: -4, fontSize: 11 }}
          />
          <YAxis tick={{ fontSize: 11 }} tickFormatter={(v) => fmt(Number(v))}
            label={{ value: unit, angle: -90, position: 'insideLeft', offset: 8, fontSize: 11 }} />
          <Tooltip formatter={(v) => (Array.isArray(v) ? v.map((x) => fmt(Number(x))).join(' – ') : fmt(+v))} labelFormatter={(l) => `t = ${l} ${timeUnit}`} />
          {/* p05–p95 shaded band */}
          <Area dataKey="band_outer" stroke="none" fill="#bfdbfe" fillOpacity={0.6} legendType="none" />
          {/* p25–p75 shaded band */}
          <Area dataKey="band_inner" stroke="none" fill="#3b82f6" fillOpacity={0.25} legendType="none" />
          {/* Median */}
          <Line dataKey="p50" stroke="#6366f1" dot={false} strokeWidth={1.5} strokeDasharray="4 2" name="p50" />
          {/* Mean */}
          <Line dataKey="mean" stroke="#1d4ed8" dot={false} strokeWidth={2} name="mean" />
        </ComposedChart>
      </ResponsiveContainer>
      <p className="mt-1 text-center text-xs text-slate-400">
        Shaded: p05–p95 (light), p25–p75 (dark) · Solid: mean · Dashed: median
      </p>
    </div>
  )
}

// ── Final values histogram ────────────────────────────────────────────────────

function FinalValuesChart({ values, unit }: { values: number[]; unit: string }) {
  const data = buildHistogramData(values)
  return (
    <div>
      <h4 className="mb-2 text-xs font-semibold uppercase tracking-wider text-slate-500">
        Final Value Distribution ({values.length.toLocaleString()} realizations)
      </h4>
      <ResponsiveContainer width="100%" height={200}>
        <BarChart data={data} margin={{ top: 4, right: 8, bottom: 4, left: 8 }}>
          <CartesianGrid strokeDasharray="3 3" stroke="#e2e8f0" />
          <XAxis dataKey="x" tick={{ fontSize: 11 }} tickFormatter={(v) => fmt(Number(v))}
            label={{ value: unit, position: 'insideBottomRight', offset: -4, fontSize: 11 }} />
          <YAxis tick={{ fontSize: 11 }} />
          <Tooltip formatter={(v) => [v, 'count']} labelFormatter={(l) => `≈ ${l} ${unit}`} />
          <Bar dataKey="count" fill="#3b82f6" radius={[2, 2, 0, 0]} />
        </BarChart>
      </ResponsiveContainer>
    </div>
  )
}

// ── Stats summary table ───────────────────────────────────────────────────────

function StatsSummary({ values, unit }: { values: number[]; unit: string }) {
  const sorted = [...values].sort((a, b) => a - b)
  const n = sorted.length
  if (n === 0) return null
  const p = (q: number) => sorted[Math.round((q / 100) * (n - 1))]
  const mean = values.reduce((a, b) => a + b, 0) / n

  const rows = [
    ['Mean', fmt(mean)],
    ['p05', fmt(p(5))],
    ['p25', fmt(p(25))],
    ['Median', fmt(p(50))],
    ['p75', fmt(p(75))],
    ['p95', fmt(p(95))],
    ['Min', fmt(sorted[0])],
    ['Max', fmt(sorted[n - 1])],
  ]

  return (
    <div>
      <h4 className="mb-2 text-xs font-semibold uppercase tracking-wider text-slate-500">
        Final Value Statistics ({unit})
      </h4>
      <div className="grid grid-cols-4 gap-px overflow-hidden rounded-lg border border-slate-200 bg-slate-200 text-sm sm:grid-cols-8">
        {rows.map(([label, val]) => (
          <div key={label} className="flex flex-col items-center bg-white px-2 py-2">
            <span className="text-xs text-slate-400">{label}</span>
            <span className="font-mono font-medium">{val}</span>
          </div>
        ))}
      </div>
    </div>
  )
}

// ── Results tab ───────────────────────────────────────────────────────────────

export function ResultsTab() {
  const results = useStore((s) => s.results)
  const selectedId = useStore((s) => s.selectedResultId)
  const setSelected = useStore((s) => s.setSelectedResultId)
  const parsedModel = useStore((s) => s.parsedModel)
  const status = useStore((s) => s.status)

  if (status === 'running') {
    return (
      <div className="flex flex-col items-center justify-center gap-3 py-24">
        <svg className="h-8 w-8 animate-spin text-blue-500" viewBox="0 0 24 24" fill="none">
          <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
          <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
        </svg>
        <p className="text-sm text-slate-500">Running simulation…</p>
      </div>
    )
  }

  if (!results) {
    return (
      <p className="py-12 text-center text-sm text-slate-400">
        Run a simulation to see results.
      </p>
    )
  }

  const outputIds = results.output_ids
  const activeId = selectedId ?? outputIds[0]
  const elem: ElementResults | undefined = activeId ? results.elements[activeId] : undefined
  const timeUnit = parsedModel?.simulation_settings.timestep.unit ?? ''

  return (
    <div className="space-y-6">
      {/* Element selector */}
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-sm text-slate-500">Output:</span>
        <select
          value={activeId ?? ''}
          onChange={(e) => setSelected(e.target.value)}
          className="rounded border border-slate-300 px-2 py-1 text-sm focus:border-blue-500 focus:outline-none focus:ring-1 focus:ring-blue-500"
        >
          {outputIds.map((id) => {
            const el = results.elements[id]
            const unitSuffix = el.unit && el.unit !== '1' ? ` [${el.unit}]` : ''
            return (
              <option key={id} value={id}>
                {el.label}{unitSuffix}
              </option>
            )
          })}
        </select>
        <span className="ml-auto text-xs text-slate-400">
          {results.n_realizations.toLocaleString()} realizations · {results.n_steps} steps
        </span>
      </div>

      {elem ? (
        <>
          {elem.time_history && (
            <div className="rounded-lg border border-slate-200 bg-white p-4">
              <TimeHistoryChart
                timeAxis={results.time_axis}
                stats={elem.time_history}
                unit={elem.unit}
                timeUnit={timeUnit}
              />
            </div>
          )}

          {elem.final_values.length > 0 && (
            <div className="rounded-lg border border-slate-200 bg-white p-4 space-y-4">
              <StatsSummary values={elem.final_values} unit={elem.unit} />
              <FinalValuesChart values={elem.final_values} unit={elem.unit} />
            </div>
          )}
        </>
      ) : (
        <p className="text-center text-sm text-slate-400">Select an output element above.</p>
      )}
    </div>
  )
}
