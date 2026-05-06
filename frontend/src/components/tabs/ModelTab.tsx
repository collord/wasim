import { useStore } from '../../store'

const TYPE_COLOR: Record<string, string> = {
  constant: 'bg-slate-100 text-slate-700',
  random_variable: 'bg-blue-100 text-blue-700',
  expression: 'bg-violet-100 text-violet-700',
  accumulator: 'bg-amber-100 text-amber-700',
  timeseries: 'bg-emerald-100 text-emerald-700',
  lookup: 'bg-cyan-100 text-cyan-700',
  delay: 'bg-orange-100 text-orange-700',
  script: 'bg-rose-100 text-rose-700',
}

export function ModelTab() {
  const summary = useStore((s) => s.modelSummary)
  const parsedModel = useStore((s) => s.parsedModel)

  if (!summary || !parsedModel) {
    return (
      <p className="py-12 text-center text-sm text-slate-400">
        No model loaded.
      </p>
    )
  }

  const { simulation_settings: ss } = parsedModel
  const containerNames = Object.fromEntries(
    summary.containers.map((c) => [c.id, c.name]),
  )

  // Group elements by container
  const groups = new Map<string, typeof summary.elements>()
  groups.set('(top level)', [])
  for (const c of summary.containers) groups.set(c.id, [])
  for (const e of summary.elements) {
    const key = e.container ?? '(top level)'
    if (!groups.has(key)) groups.set(key, [])
    groups.get(key)!.push(e)
  }

  return (
    <div className="space-y-6">
      {/* Simulation settings */}
      <div className="rounded-lg border border-slate-200 bg-white p-4">
        <h3 className="mb-3 text-xs font-semibold uppercase tracking-wider text-slate-500">
          Simulation Settings
        </h3>
        <dl className="grid grid-cols-2 gap-x-8 gap-y-1 text-sm sm:grid-cols-4">
          <div>
            <dt className="text-slate-500">Duration</dt>
            <dd className="font-mono font-medium">{ss.duration.value} {ss.duration.unit}</dd>
          </div>
          <div>
            <dt className="text-slate-500">Timestep</dt>
            <dd className="font-mono font-medium">{ss.timestep.value} {ss.timestep.unit}</dd>
          </div>
          <div>
            <dt className="text-slate-500">Realizations</dt>
            <dd className="font-mono font-medium">{ss.n_realizations}</dd>
          </div>
          <div>
            <dt className="text-slate-500">Seed</dt>
            <dd className="font-mono font-medium">{ss.seed ?? 'random'}</dd>
          </div>
        </dl>
      </div>

      {/* Element list by container */}
      {Array.from(groups.entries())
        .filter(([, elems]) => elems.length > 0)
        .map(([containerId, elems]) => (
          <div key={containerId} className="rounded-lg border border-slate-200 bg-white">
            <div className="border-b border-slate-100 px-4 py-2">
              <h3 className="text-sm font-semibold text-slate-700">
                {containerId === '(top level)'
                  ? 'Top Level'
                  : (containerNames[containerId] ?? containerId)}
              </h3>
            </div>
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-slate-100 text-left text-xs text-slate-400">
                  <th className="px-4 py-2 font-normal">Name</th>
                  <th className="px-4 py-2 font-normal">ID</th>
                  <th className="px-4 py-2 font-normal">Unit</th>
                  <th className="px-4 py-2 font-normal">Type</th>
                </tr>
              </thead>
              <tbody>
                {elems.map((e) => (
                  <tr key={e.id} className="border-b border-slate-50 last:border-0 hover:bg-slate-50">
                    <td className="px-4 py-2 font-medium text-slate-800">{e.name}</td>
                    <td className="px-4 py-2 font-mono text-xs text-slate-500">{e.id}</td>
                    <td className="px-4 py-2 font-mono text-xs text-slate-400">
                      {e.unit !== '1' ? e.unit : ''}
                    </td>
                    <td className="px-4 py-2">
                      <span className={`inline-block rounded px-1.5 py-0.5 text-xs font-medium ${TYPE_COLOR[e.type] ?? 'bg-slate-100 text-slate-600'}`}>
                        {e.type}
                      </span>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ))}
    </div>
  )
}
