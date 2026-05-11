import { useStore } from '../../store'
import type { ConstantElement, RandomVariableElement } from '../../types'

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
}

// ── Components ────────────────────────────────────────────────────────────────

function ConstantInput({ elem }: { elem: ConstantElement }) {
  const setConstant = useStore((s) => s.setConstant)
  const pm = useStore((s) => s.parsedModel)
  // Read live value from parsedModel (reflects edits)
  const liveElem = pm?.elements.find((e) => e.id === elem.id) as ConstantElement | undefined
  const raw = liveElem?.value.value ?? elem.value.value
  const current = Number.isFinite(raw) ? raw : ''
  const unit = elem.value.unit !== '1' ? elem.value.unit : ''

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
            if (Number.isFinite(v)) setConstant(elem.id, v)
          }}
          min={elem.bounds?.min ?? undefined}
          max={elem.bounds?.max ?? undefined}
          step="any"
          className="w-28 rounded border border-slate-300 px-2 py-1 text-right font-mono text-sm focus:border-blue-500 focus:outline-none focus:ring-1 focus:ring-blue-500"
        />
        {unit && <span className="text-xs text-slate-400 whitespace-nowrap">{unit}</span>}
      </div>
    </div>
  )
}

function RvParamInput({ elem }: { elem: RandomVariableElement }) {
  const setRvParam = useStore((s) => s.setRvParam)
  const pm = useStore((s) => s.parsedModel)
  const liveElem = pm?.elements.find((e) => e.id === elem.id) as RandomVariableElement | undefined
  const dist = liveElem?.distribution ?? elem.distribution
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
        {Object.entries(dist.parameters).map(([pname, pval]) => {
          const num = typeof pval === 'number' ? pval : (pval as { value: number }).value
          const unit =
            typeof pval === 'object' && pval !== null && 'unit' in pval && pval.unit !== '1'
              ? (pval as { unit: string }).unit
              : ''
          return (
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
          )
        })}
      </div>
    </div>
  )
}

// ── Run controls ──────────────────────────────────────────────────────────────

function RunControls() {
  const status = useStore((s) => s.status)
  const nRealizations = useStore((s) => s.nRealizations)
  const seed = useStore((s) => s.seed)
  const simDuration = useStore((s) => s.simDuration)
  const simDurationUnit = useStore((s) => s.simDurationUnit)
  const simTimestep = useStore((s) => s.simTimestep)
  const simTimestepUnit = useStore((s) => s.simTimestepUnit)
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
          <label className="text-xs text-slate-500">Duration</label>
          <div className="flex items-center gap-1">
            <input
              type="number"
              value={simDuration ?? ''}
              onChange={(e) => setSimDuration(parseFloat(e.target.value))}
              min={0}
              step="any"
              className={`w-full ${inputCls}`}
            />
            <span className="text-xs text-slate-400 whitespace-nowrap">{simDurationUnit}</span>
          </div>
        </div>
        <div className="flex flex-col gap-0.5">
          <label className="text-xs text-slate-500">Timestep</label>
          <div className="flex items-center gap-1">
            <input
              type="number"
              value={simTimestep ?? ''}
              onChange={(e) => setSimTimestep(parseFloat(e.target.value))}
              min={0}
              step="any"
              className={`w-full ${inputCls}`}
            />
            <span className="text-xs text-slate-400 whitespace-nowrap">{simTimestepUnit}</span>
          </div>
        </div>
        <div className="flex flex-col gap-0.5">
          <label className="text-xs text-slate-500">Realizations</label>
          <input
            type="number"
            value={nRealizations}
            onChange={(e) => setNRealizations(parseInt(e.target.value, 10) || 1)}
            min={1}
            max={100000}
            className={inputCls}
          />
        </div>
        <div className="flex flex-col gap-0.5">
          <label className="text-xs text-slate-500">Seed</label>
          <input
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
  const parsedModel = useStore((s) => s.parsedModel)
  const summary = useStore((s) => s.modelSummary)

  if (!parsedModel) {
    return (
      <p className="py-12 text-center text-sm text-slate-400">
        No model loaded.
      </p>
    )
  }

  const containerNames = Object.fromEntries(
    (parsedModel.containers ?? []).map((c) => [c.id, c.name]),
  )

  // Group editable elements by container
  const editableElems = parsedModel.elements.filter(
    (e) => e.type === 'constant'
      ? (e as ConstantElement).editable === true
      : e.type === 'random_variable',
  )

  const groups = new Map<string, typeof editableElems>()
  groups.set('(top level)', [])
  for (const c of parsedModel.containers ?? []) groups.set(c.id, [])
  for (const e of editableElems) {
    const key = e.container ?? '(top level)'
    if (!groups.has(key)) groups.set(key, [])
    groups.get(key)!.push(e)
  }

  const nonEmptyGroups = Array.from(groups.entries()).filter(([, elems]) => elems.length > 0)

  const loadedLabel = summary ? `${summary.element_count} elements` : 'loading…'

  return (
    <div className="flex flex-col">
      <div className="mb-4 flex items-center justify-between">
        <p className="text-xs text-slate-400">{loadedLabel}</p>
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
                    {e.type === 'constant' ? (
                      <ConstantInput elem={e as ConstantElement} />
                    ) : (
                      <RvParamInput elem={e as RandomVariableElement} />
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
