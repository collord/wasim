import { useState, useMemo } from 'react'
import { useStore } from '../../store'

const TYPE_COLOR: Record<string, string> = {
  // legacy v1 types / node value_rules
  constant: 'bg-slate-100 text-slate-700',
  random_variable: 'bg-blue-100 text-blue-700',
  expression: 'bg-violet-100 text-violet-700',
  accumulator: 'bg-amber-100 text-amber-700',
  timeseries: 'bg-emerald-100 text-emerald-700',
  lookup: 'bg-cyan-100 text-cyan-700',
  delay: 'bg-orange-100 text-orange-700',
  script: 'bg-rose-100 text-rose-700',
  // v2 primitives
  stock: 'bg-amber-100 text-amber-700',
  link: 'bg-teal-100 text-teal-700',
  event: 'bg-rose-100 text-rose-700',
  gate: 'bg-indigo-100 text-indigo-700',
  cell: 'bg-lime-100 text-lime-700',
  species: 'bg-slate-100 text-slate-600',
  medium: 'bg-slate-100 text-slate-600',
}

export function ModelTab() {
  const summary = useStore((s) => s.modelSummary)
  const [expanded, setExpanded] = useState<Set<string>>(new Set())

  const { deps, rdeps } = useMemo(() => {
    const deps = new Map<string, string[]>()
    const rdeps = new Map<string, string[]>()
    for (const e of summary?.elements ?? []) {
      deps.set(e.id, e.inputs)
      for (const dep of e.inputs) {
        if (!rdeps.has(dep)) rdeps.set(dep, [])
        rdeps.get(dep)!.push(e.id)
      }
    }
    return { deps, rdeps }
  }, [summary])

  if (!summary) {
    return (
      <p className="py-12 text-center text-sm text-slate-400">
        No model loaded.
      </p>
    )
  }

  const ss = summary.simulation_settings
  const containerNames = Object.fromEntries(
    summary.containers.map((c) => [c.id, c.name]),
  )
  const nameById = Object.fromEntries(summary.elements.map((e) => [e.id, e.name]))

  function toggle(id: string) {
    setExpanded((prev) => {
      const next = new Set(prev)
      next.has(id) ? next.delete(id) : next.add(id)
      return next
    })
  }

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
                {elems.map((e) => {
                  const isOpen = expanded.has(e.id)
                  const myDeps = deps.get(e.id) ?? []
                  const myRdeps = rdeps.get(e.id) ?? []
                  const hasDeps = myDeps.length > 0 || myRdeps.length > 0
                  return (
                    <>
                      <tr
                        key={e.id}
                        title={e.description ?? undefined}
                        className={`border-b border-slate-50 ${isOpen ? '' : 'last:border-0'} ${hasDeps ? 'cursor-pointer hover:bg-slate-50' : ''}`}
                        onClick={() => hasDeps && toggle(e.id)}
                      >
                        <td className="px-4 py-2 font-medium text-slate-800">
                          <span className="flex items-center gap-1.5">
                            {hasDeps && (
                              <span className="text-slate-400 text-xs select-none">
                                {isOpen ? '▾' : '▸'}
                              </span>
                            )}
                            {e.name}
                          </span>
                        </td>
                        <td className="px-4 py-2 font-mono text-xs text-slate-500">{e.id}</td>
                        <td className="px-4 py-2 font-mono text-xs text-slate-400">
                          {e.unit !== '1' ? e.unit : ''}
                        </td>
                        <td className="px-4 py-2">
                          <span className={`inline-block rounded px-1.5 py-0.5 text-xs font-medium ${TYPE_COLOR[e.type] ?? 'bg-slate-100 text-slate-600'}`}>
                            {e.type}
                          </span>
                          {e.traits.length > 0 && (
                            <span className="ml-1.5 text-[10px] text-slate-400">{e.traits.join(' · ')}</span>
                          )}
                        </td>
                      </tr>
                      {isOpen && (
                        <tr key={`${e.id}-deps`} className="border-b border-slate-50 bg-slate-50">
                          <td colSpan={4} className="px-8 py-3">
                            <div className="flex gap-8 text-xs">
                              <div>
                                <p className="mb-1 font-semibold text-slate-500 uppercase tracking-wider text-[10px]">Depends on</p>
                                {myDeps.length === 0
                                  ? <p className="text-slate-400 italic">none</p>
                                  : <ul className="space-y-0.5">
                                      {myDeps.map((id) => (
                                        <li key={id} className="text-slate-700">{nameById[id] ?? id}</li>
                                      ))}
                                    </ul>
                                }
                              </div>
                              <div>
                                <p className="mb-1 font-semibold text-slate-500 uppercase tracking-wider text-[10px]">Used by</p>
                                {myRdeps.length === 0
                                  ? <p className="text-slate-400 italic">none</p>
                                  : <ul className="space-y-0.5">
                                      {myRdeps.map((id) => (
                                        <li key={id} className="text-slate-700">{nameById[id] ?? id}</li>
                                      ))}
                                    </ul>
                                }
                              </div>
                            </div>
                          </td>
                        </tr>
                      )}
                    </>
                  )
                })}
              </tbody>
            </table>
          </div>
        ))}
    </div>
  )
}
