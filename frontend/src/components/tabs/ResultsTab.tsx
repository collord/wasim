import { useState } from 'react'
import {
  ComposedChart,
  Line,
  Area,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  Legend,
  ResponsiveContainer,
  BarChart,
  Bar,
} from 'recharts'
import { useStore } from '../../store'
import type { ElementResults, TimeHistoryStats } from '../../types'

// ── Color palette ─────────────────────────────────────────────────────────────

const PALETTE = [
  '#2563eb', // blue
  '#dc2626', // red
  '#16a34a', // green
  '#d97706', // amber
  '#7c3aed', // violet
  '#0891b2', // cyan
  '#db2777', // pink
  '#65a30d', // lime
]

// ── Chart helpers ─────────────────────────────────────────────────────────────

function isFiniteNum(n: number): boolean {
  return Number.isFinite(n)
}

function buildTimeChartData(
  timeAxis: number[],
  series: { id: string; stats: TimeHistoryStats }[],
) {
  return timeAxis.map((t, i) => {
    const point: Record<string, number | null | [number, number]> = {
      t: +t.toFixed(4),
    }
    for (const { id, stats } of series) {
      point[`${id}_mean`] = isFiniteNum(stats.mean[i]) ? stats.mean[i] : null
      if (series.length === 1) {
        point['band_outer'] =
          isFiniteNum(stats.p05[i]) && isFiniteNum(stats.p95[i])
            ? [stats.p05[i], stats.p95[i]]
            : null
        point['band_inner'] =
          isFiniteNum(stats.p25[i]) && isFiniteNum(stats.p75[i])
            ? [stats.p25[i], stats.p75[i]]
            : null
        point['p50'] = isFiniteNum(stats.p50[i]) ? stats.p50[i] : null
      }
    }
    return point
  })
}

function buildHistogramData(values: number[], bins = 30) {
  const finite = values.filter(isFiniteNum)
  if (finite.length === 0) return []
  const lo = Math.min(...finite)
  const hi = Math.max(...finite)
  if (lo === hi) return [{ x: lo, count: finite.length }]
  const width = (hi - lo) / bins
  const counts = Array<number>(bins).fill(0)
  for (const v of finite) {
    const idx = Math.min(Math.floor((v - lo) / width), bins - 1)
    counts[idx]++
  }
  return counts.map((count, i) => ({
    x: +(lo + (i + 0.5) * width).toFixed(4),
    count,
  }))
}

function fmt(n: number) {
  if (!Number.isFinite(n)) return String(n)
  if (Math.abs(n) >= 1000 || (Math.abs(n) < 0.01 && n !== 0)) {
    return n.toExponential(3)
  }
  return n.toFixed(4).replace(/\.?0+$/, '')
}

function hasNonFinite(stats: TimeHistoryStats): boolean {
  return stats.mean.some((v) => !Number.isFinite(v))
}

// ── Axis grouping ─────────────────────────────────────────────────────────────

type SeriesEntry = { id: string; label: string; unit: string; stats: TimeHistoryStats; color: string }

function seriesRange(stats: TimeHistoryStats): [number, number] {
  const vals = stats.mean.filter(isFiniteNum)
  if (vals.length === 0) return [0, 1]
  const lo = Math.min(...vals)
  const hi = Math.max(...vals)
  // pad degenerate (constant) range so overlap math works
  if (lo === hi) {
    const pad = Math.abs(lo) * 0.1 + 1
    return [lo - pad, hi + pad]
  }
  return [lo, hi]
}

function rangeOverlapRatio(a: [number, number], b: [number, number]): number {
  const iLo = Math.max(a[0], b[0])
  const iHi = Math.min(a[1], b[1])
  if (iLo >= iHi) return 0
  const uLo = Math.min(a[0], b[0])
  const uHi = Math.max(a[1], b[1])
  return uHi > uLo ? (iHi - iLo) / (uHi - uLo) : 1
}

interface AxisGroup {
  axisId: string
  range: [number, number]
  members: SeriesEntry[]
  orientation: 'left' | 'right'
}

function buildAxisGroups(series: SeriesEntry[], threshold = 0.5): AxisGroup[] {
  const raw: { range: [number, number]; members: SeriesEntry[] }[] = []
  for (const s of series) {
    const r = seriesRange(s.stats)
    const idx = raw.findIndex((g) => rangeOverlapRatio(g.range, r) >= threshold)
    if (idx !== -1) {
      raw[idx].members.push(s)
      raw[idx].range = [Math.min(raw[idx].range[0], r[0]), Math.max(raw[idx].range[1], r[1])]
    } else {
      raw.push({ range: r, members: [s] })
    }
  }
  return raw.map((g, i) => ({
    axisId: `a${i}`,
    range: g.range,
    members: g.members,
    orientation: (i % 2 === 0 ? 'left' : 'right') as 'left' | 'right',
  }))
}

// ── Multi-series time history chart ──────────────────────────────────────────

function TimeHistoryChart({
  timeAxis,
  series,
  timeUnit,
}: {
  timeAxis: number[]
  series: SeriesEntry[]
  timeUnit: string
}) {
  if (series.length === 0) return null
  const single = series.length === 1
  const data = buildTimeChartData(timeAxis, series)
  const anyNonFinite = series.some((s) => hasNonFinite(s.stats))

  const axisGroups = buildAxisGroups(series)
  const seriesAxisId = new Map(
    axisGroups.flatMap((g) => g.members.map((s) => [s.id, g.axisId]))
  )

  const leftCount = axisGroups.filter((g) => g.orientation === 'left').length
  const rightCount = axisGroups.filter((g) => g.orientation === 'right').length
  // Each extra axis on a side needs ~55px of margin
  const leftMargin = 8 + Math.max(0, leftCount - 1) * 55
  const rightMargin = 16 + rightCount * 55

  return (
    <div>
      <h4 className="mb-2 text-xs font-semibold uppercase tracking-wider text-slate-500">
        Time History
      </h4>
      {anyNonFinite && (
        <p className="mb-2 rounded bg-amber-50 px-3 py-1.5 text-xs text-amber-700">
          One or more elements produced non-finite values (NaN or Infinity).
        </p>
      )}
      <ResponsiveContainer width="100%" height={280}>
        <ComposedChart data={data} margin={{ top: 4, right: rightMargin, bottom: 4, left: leftMargin }}>
          <CartesianGrid strokeDasharray="3 3" stroke="#e2e8f0" />
          <XAxis
            dataKey="t"
            tick={{ fontSize: 11 }}
            label={{ value: timeUnit, position: 'insideBottomRight', offset: -4, fontSize: 11 }}
          />

          {axisGroups.map((group) => {
            const oneSeries = group.members.length === 1
            const axisColor = oneSeries ? group.members[0].color : '#64748b'
            const units = [...new Set(group.members.map((s) => s.unit).filter((u) => u && u !== '1'))]
            const unitLabel = units.length === 1 ? units[0] : ''
            return (
              <YAxis
                key={group.axisId}
                yAxisId={group.axisId}
                orientation={group.orientation}
                tick={{ fontSize: 11, fill: axisColor }}
                tickFormatter={(v) => fmt(Number(v))}
                width={55}
                label={unitLabel ? {
                  value: unitLabel,
                  angle: group.orientation === 'left' ? -90 : 90,
                  position: group.orientation === 'left' ? 'insideLeft' : 'insideRight',
                  offset: group.orientation === 'left' ? 12 : -12,
                  fontSize: 11,
                  fill: axisColor,
                } : undefined}
              />
            )
          })}

          <Tooltip
            formatter={(v, name) => {
              const val = Array.isArray(v)
                ? v.map((x) => fmt(Number(x))).join(' – ')
                : fmt(Number(v))
              return [val, name]
            }}
            labelFormatter={(l) => `t = ${l} ${timeUnit}`}
          />
          <Legend formatter={(value) => <span style={{ fontSize: 12, color: '#374151' }}>{value}</span>} />

          {/* Bands and median — single series only */}
          {single && (
            <>
              <Area
                yAxisId={axisGroups[0].axisId}
                dataKey="band_outer"
                stroke="none"
                fill={series[0].color}
                fillOpacity={0.15}
                legendType="none"
                tooltipType="none"
              />
              <Area
                yAxisId={axisGroups[0].axisId}
                dataKey="band_inner"
                stroke="none"
                fill={series[0].color}
                fillOpacity={0.25}
                legendType="none"
                tooltipType="none"
              />
              <Line
                yAxisId={axisGroups[0].axisId}
                dataKey="p50"
                stroke={series[0].color}
                dot={false}
                strokeWidth={1.5}
                strokeDasharray="4 2"
                name="median"
                legendType="none"
              />
            </>
          )}

          {/* Mean line per series */}
          {series.map((s) => (
            <Line
              key={s.id}
              yAxisId={seriesAxisId.get(s.id)}
              dataKey={`${s.id}_mean`}
              stroke={s.color}
              dot={false}
              strokeWidth={2}
              name={s.unit && s.unit !== '1' ? `${s.label} [${s.unit}]` : s.label}
              connectNulls={false}
            />
          ))}
        </ComposedChart>
      </ResponsiveContainer>
      {single && (
        <p className="mt-1 text-center text-xs text-slate-400">
          Shaded: p05–p95 (light), p25–p75 (dark) · Solid: mean · Dashed: median
        </p>
      )}
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
          <XAxis
            dataKey="x"
            tick={{ fontSize: 11 }}
            tickFormatter={(v) => fmt(Number(v))}
            label={{ value: unit, position: 'insideBottomRight', offset: -4, fontSize: 11 }}
          />
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
  const sorted = [...values].filter(isFiniteNum).sort((a, b) => a - b)
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

// ── Analysis options (results_spec, §11) ──────────────────────────────────────

const parseNums = (s: string): number[] =>
  s.split(/[,\s]+/).map((x) => parseFloat(x)).filter((n) => Number.isFinite(n))

function AnalysisOptions() {
  const spec = useStore((s) => s.resultsSpec)
  const setSpec = useStore((s) => s.setResultsSpec)
  const run = useStore((s) => s.run)
  const status = useStore((s) => s.status)
  const [pctText, setPctText] = useState(spec.percentiles.join(', '))
  const [capText, setCapText] = useState(spec.capture_times.join(', '))

  return (
    <details className="rounded-lg border border-slate-200 bg-white p-4" open>
      <summary className="cursor-pointer text-xs font-semibold uppercase tracking-wider text-slate-500">
        Analysis options
      </summary>
      <div className="mt-3 grid gap-3 sm:grid-cols-2">
        <label className="flex items-center gap-2 text-xs text-slate-600">
          <input type="checkbox" checked={spec.distribution} onChange={(e) => setSpec({ distribution: e.target.checked })} className="h-3.5 w-3.5" />
          Final-value distribution (PDF / CDF / CCDF)
        </label>
        <label className="flex items-center gap-2 text-xs text-slate-600">
          <input type="checkbox" checked={spec.final_stats} onChange={(e) => setSpec({ final_stats: e.target.checked })} className="h-3.5 w-3.5" />
          Final-value statistics (CI, skew, kurtosis, CTE)
        </label>
        <label className="block text-xs text-slate-600">
          <span className="mb-0.5 block font-medium text-slate-500">Custom percentiles (0–100)</span>
          <input value={pctText} onChange={(e) => setPctText(e.target.value)}
            onBlur={() => setSpec({ percentiles: parseNums(pctText) })}
            placeholder="e.g. 10, 50, 90" className="w-full rounded border border-slate-300 px-2 py-1 text-xs" />
        </label>
        <label className="block text-xs text-slate-600">
          <span className="mb-0.5 block font-medium text-slate-500">Capture times (timestep unit)</span>
          <input value={capText} onChange={(e) => setCapText(e.target.value)}
            onBlur={() => setSpec({ capture_times: parseNums(capText) })}
            placeholder="e.g. 10, 20, 30" className="w-full rounded border border-slate-300 px-2 py-1 text-xs" />
        </label>
      </div>
      <button
        onClick={run}
        disabled={status === 'running'}
        className="mt-3 rounded bg-blue-600 px-3 py-1.5 text-xs font-semibold text-white hover:bg-blue-500 disabled:opacity-40"
      >
        {status === 'running' ? '⟳ Running…' : 'Run with analysis'}
      </button>
    </details>
  )
}

// ── Distribution (PDF / CDF / CCDF) ────────────────────────────────────────────

function DistributionChart({ dist, unit }: { dist: NonNullable<ElementResults['analysis']>['distribution']; unit: string }) {
  const [view, setView] = useState<'pdf' | 'cdf' | 'ccdf'>('pdf')
  if (!dist) return null
  const data = view === 'pdf'
    ? dist.bin_centers.map((x, i) => ({ x: +x.toFixed(4), y: dist.pdf[i] }))
    : dist.x.map((x, i) => ({ x: +x.toFixed(4), y: (view === 'cdf' ? dist.cdf : dist.ccdf)[i] }))
  const yLabel = view === 'pdf' ? 'density' : view === 'cdf' ? 'P(X ≤ x)' : 'P(X > x)'
  return (
    <div>
      <div className="mb-2 flex items-center justify-between">
        <h4 className="text-xs font-semibold uppercase tracking-wider text-slate-500">Distribution</h4>
        <div className="flex gap-1 text-[10px]">
          {(['pdf', 'cdf', 'ccdf'] as const).map((v) => (
            <button key={v} onClick={() => setView(v)}
              className={`rounded px-2 py-0.5 uppercase ${view === v ? 'bg-slate-800 text-white' : 'bg-slate-100 text-slate-500'}`}>{v}</button>
          ))}
        </div>
      </div>
      <ResponsiveContainer width="100%" height={220}>
        <ComposedChart data={data} margin={{ top: 4, right: 8, bottom: 4, left: 8 }}>
          <CartesianGrid strokeDasharray="3 3" stroke="#e2e8f0" />
          <XAxis dataKey="x" tick={{ fontSize: 11 }} tickFormatter={(v) => fmt(Number(v))}
            label={{ value: unit, position: 'insideBottomRight', offset: -4, fontSize: 11 }} />
          <YAxis tick={{ fontSize: 11 }} tickFormatter={(v) => fmt(Number(v))}
            label={{ value: yLabel, angle: -90, position: 'insideLeft', fontSize: 11 }} />
          <Tooltip formatter={(v) => [fmt(Number(v)), yLabel]} labelFormatter={(l) => `${l} ${unit}`} />
          {view === 'pdf'
            ? <Bar dataKey="y" fill="#3b82f6" radius={[2, 2, 0, 0]} />
            : <Line dataKey="y" stroke="#2563eb" dot={false} strokeWidth={2} />}
        </ComposedChart>
      </ResponsiveContainer>
    </div>
  )
}

// ── Final-value statistics (CI / skew / kurtosis / CTE) ────────────────────────

function FinalStatsPanel({ stats, unit }: { stats: NonNullable<ElementResults['analysis']>['final_stats']; unit: string }) {
  if (!stats) return null
  const rows: [string, string][] = [
    ['Mean', fmt(stats.mean)],
    [`${Math.round(stats.confidence * 100)}% CI`, `${fmt(stats.ci_lower)} – ${fmt(stats.ci_upper)}`],
    ['Std dev', fmt(stats.std)],
    ['± CI half-width', fmt(stats.ci_half_width)],
    ['Skewness', fmt(stats.skewness)],
    ['Excess kurtosis', fmt(stats.excess_kurtosis)],
    [`CTE (p${stats.cte_percentile})`, fmt(stats.cte)],
  ]
  return (
    <div>
      <h4 className="mb-2 text-xs font-semibold uppercase tracking-wider text-slate-500">
        Final-value statistics ({unit})
      </h4>
      <div className="grid grid-cols-2 gap-px overflow-hidden rounded-lg border border-slate-200 bg-slate-200 text-sm sm:grid-cols-4">
        {rows.map(([label, val]) => (
          <div key={label} className="flex flex-col items-center bg-white px-2 py-2">
            <span className="text-center text-[10px] text-slate-400">{label}</span>
            <span className="font-mono font-medium">{val}</span>
          </div>
        ))}
      </div>
    </div>
  )
}

// ── Capture-time snapshots ─────────────────────────────────────────────────────

function CaptureTable({ captures, unit, timeUnit }: {
  captures: NonNullable<ElementResults['analysis']>['captures']; unit: string; timeUnit: string
}) {
  if (!captures || captures.length === 0) return null
  return (
    <div>
      <h4 className="mb-2 text-xs font-semibold uppercase tracking-wider text-slate-500">
        Capture-time distributions ({unit})
      </h4>
      <table className="w-full text-xs">
        <thead>
          <tr className="text-left text-slate-400">
            <th className="pb-1 font-medium">t ({timeUnit})</th>
            <th className="pb-1 text-right font-medium">mean</th>
            <th className="pb-1 text-right font-medium">p05</th>
            <th className="pb-1 text-right font-medium">p50</th>
            <th className="pb-1 text-right font-medium">p95</th>
          </tr>
        </thead>
        <tbody>
          {captures.map((c) => (
            <tr key={c.step} className="border-t border-slate-100 font-mono">
              <td className="py-1 text-slate-600">{fmt(c.time)}</td>
              <td className="py-1 text-right">{fmt(c.mean)}</td>
              <td className="py-1 text-right">{fmt(c.p05)}</td>
              <td className="py-1 text-right">{fmt(c.p50)}</td>
              <td className="py-1 text-right">{fmt(c.p95)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}

// ── Custom percentile bands (time history) ─────────────────────────────────────

function PercentileBandsChart({ bands, timeAxis, unit, timeUnit }: {
  bands: NonNullable<ElementResults['analysis']>['percentile_bands']
  timeAxis: number[]; unit: string; timeUnit: string
}) {
  if (!bands || bands.length === 0) return null
  const sorted = [...bands].sort((a, b) => a.percentile - b.percentile)
  const data = timeAxis.map((t, i) => {
    const row: Record<string, number> = { t: +t.toFixed(4) }
    for (const b of sorted) row[`p${b.percentile}`] = b.values[i]
    return row
  })
  return (
    <div>
      <h4 className="mb-2 text-xs font-semibold uppercase tracking-wider text-slate-500">
        Custom percentile bands
      </h4>
      <ResponsiveContainer width="100%" height={220}>
        <ComposedChart data={data} margin={{ top: 4, right: 8, bottom: 4, left: 8 }}>
          <CartesianGrid strokeDasharray="3 3" stroke="#e2e8f0" />
          <XAxis dataKey="t" tick={{ fontSize: 11 }}
            label={{ value: timeUnit, position: 'insideBottomRight', offset: -4, fontSize: 11 }} />
          <YAxis tick={{ fontSize: 11 }} tickFormatter={(v) => fmt(Number(v))}
            label={{ value: unit, angle: -90, position: 'insideLeft', fontSize: 11 }} />
          <Tooltip formatter={(v, n) => [fmt(Number(v)), n]} labelFormatter={(l) => `t = ${l} ${timeUnit}`} />
          <Legend formatter={(value) => <span style={{ fontSize: 12, color: '#374151' }}>{value}</span>} />
          {sorted.map((b, i) => (
            <Line key={b.percentile} dataKey={`p${b.percentile}`} name={`p${b.percentile}`}
              stroke={PALETTE[i % PALETTE.length]} dot={false} strokeWidth={1.5} />
          ))}
        </ComposedChart>
      </ResponsiveContainer>
    </div>
  )
}

// ── Results tab ───────────────────────────────────────────────────────────────

export function ResultsTab() {
  const results = useStore((s) => s.results)
  const selectedId = useStore((s) => s.selectedResultId)
  const setSelected = useStore((s) => s.setSelectedResultId)
  const modelSummary = useStore((s) => s.modelSummary)
  const status = useStore((s) => s.status)

  const [plotIds, setPlotIds] = useState<string[]>([])

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
      <div className="space-y-6">
        <AnalysisOptions />
        <p className="py-12 text-center text-sm text-slate-400">
          Run a simulation to see results.
        </p>
      </div>
    )
  }

  const outputIds = results.output_ids
  const activeId = selectedId ?? outputIds[0]

  // On first render after results arrive, default plotIds to first output
  const effectivePlotIds =
    plotIds.length > 0 && plotIds.some((id) => results.elements[id])
      ? plotIds
      : [activeId].filter(Boolean)

  const timeUnit =
    results.time_unit || (modelSummary?.simulation_settings.timestep.unit ?? '')
  const activeElem: ElementResults | undefined = activeId ? results.elements[activeId] : undefined

  const plotSeries = effectivePlotIds
    .map((id, idx) => {
      const el = results.elements[id]
      if (!el?.time_history) return null
      return {
        id,
        label: el.label,
        unit: el.unit,
        stats: el.time_history,
        color: PALETTE[idx % PALETTE.length],
      }
    })
    .filter((s): s is NonNullable<typeof s> => s !== null)

  const analysis = activeElem?.analysis ?? null

  return (
    <div className="space-y-6">
      <AnalysisOptions />

      {/* Series selector */}
      <div className="rounded-lg border border-slate-200 bg-white p-4">
        <div className="mb-2 flex items-center justify-between">
          <h3 className="text-xs font-semibold uppercase tracking-wider text-slate-500">
            Series to plot
          </h3>
          <span className="text-xs text-slate-400">
            {results.n_realizations.toLocaleString()} realizations · {results.n_steps} steps
          </span>
        </div>
        <div className="flex flex-wrap gap-2">
          {outputIds.map((id) => {
            const el = results.elements[id]
            if (!el?.time_history) return null
            const plotIdx = effectivePlotIds.indexOf(id)
            const isActive = plotIdx !== -1
            const color = isActive ? PALETTE[plotIdx % PALETTE.length] : undefined
            const unitSuffix = el.unit && el.unit !== '1' ? ` [${el.unit}]` : ''
            return (
              <button
                key={id}
                onClick={() => {
                  // prevent deselecting the last active series
                  if (isActive && effectivePlotIds.length === 1) return
                  setPlotIds(
                    isActive
                      ? effectivePlotIds.filter((x) => x !== id)
                      : [...effectivePlotIds, id],
                  )
                }}
                className={`flex items-center gap-1.5 rounded-full border px-3 py-1 text-xs font-medium transition-colors ${
                  isActive
                    ? 'border-transparent text-white'
                    : 'border-slate-300 text-slate-600 hover:border-slate-400'
                }`}
                style={isActive ? { backgroundColor: color } : undefined}
              >
                {isActive && (
                  <span className="inline-block h-2 w-2 rounded-full bg-white/60" />
                )}
                {el.label}{unitSuffix}
              </button>
            )
          })}
        </div>
      </div>

      {/* Multi-series time chart */}
      {plotSeries.length > 0 && (
        <div className="rounded-lg border border-slate-200 bg-white p-4">
          <TimeHistoryChart
            timeAxis={results.time_axis}
            series={plotSeries}
            timeUnit={timeUnit}
          />
        </div>
      )}

      {/* Per-element stats — single selector */}
      <div className="rounded-lg border border-slate-200 bg-white p-4 space-y-4">
        <div className="flex items-center gap-2">
          <span className="text-xs font-semibold uppercase tracking-wider text-slate-500">
            Statistics for:
          </span>
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
        </div>

        {activeElem && (
          <>
            {activeElem.final_values.length > 0 && (
              <>
                <StatsSummary values={activeElem.final_values} unit={activeElem.unit} />
                <FinalValuesChart values={activeElem.final_values} unit={activeElem.unit} />
              </>
            )}
            {analysis?.final_stats && <FinalStatsPanel stats={analysis.final_stats} unit={activeElem.unit} />}
            {analysis?.distribution && <DistributionChart dist={analysis.distribution} unit={activeElem.unit} />}
            {analysis?.captures && analysis.captures.length > 0 && (
              <CaptureTable captures={analysis.captures} unit={activeElem.unit} timeUnit={timeUnit} />
            )}
            {analysis?.percentile_bands && analysis.percentile_bands.length > 0 && (
              <PercentileBandsChart bands={analysis.percentile_bands} timeAxis={results.time_axis}
                unit={activeElem.unit} timeUnit={timeUnit} />
            )}
          </>
        )}
      </div>
    </div>
  )
}
