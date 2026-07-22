import { useMemo, useState } from 'react'
import { useStore, useElements, useContainers } from '../../store'
import type { ElementSummary } from '../../types'
import { iconTypeOf, TypeBadge } from '../../ui/typeIcons'
import { kindLabel } from '../../model/schema'
import { Palette } from './Palette'

type Lens = 'containment' | 'type'
type Panel = 'browse' | 'palette'

/** The model browser (spec §4): a searchable tree with two lenses (by containment / by
 *  type), plus the insert palette. Every element comes straight from ModelSummary.elements
 *  so the browser and canvas never disagree. */
export function ModelBrowser() {
  const elements = useElements()
  const containers = useContainers()
  const selectedId = useStore((s) => s.selectedId)
  const select = useStore((s) => s.select)
  const remove = useStore((s) => s.removeElement)
  const duplicate = useStore((s) => s.duplicateElement)

  const [panel, setPanel] = useState<Panel>('browse')
  const [lens, setLens] = useState<Lens>('containment')
  const [query, setQuery] = useState('')

  const q = query.trim().toLowerCase()
  const filtered = useMemo(
    () => (q ? elements.filter((e) =>
      e.id.toLowerCase().includes(q) || e.name.toLowerCase().includes(q) ||
      kindLabel(e).toLowerCase().includes(q) || (e.unit ?? '').toLowerCase().includes(q)) : elements),
    [elements, q],
  )

  const byType = useMemo(() => {
    const groups = new Map<string, ElementSummary[]>()
    for (const e of filtered) {
      const k = kindLabel(e)
      const arr = groups.get(k) ?? []
      arr.push(e)
      groups.set(k, arr)
    }
    return [...groups.entries()].sort((a, b) => a[0].localeCompare(b[0]))
  }, [filtered])

  const byContainer = useMemo(() => {
    const root: ElementSummary[] = []
    const inC = new Map<string, ElementSummary[]>()
    for (const e of filtered) {
      if (e.container) { const a = inC.get(e.container) ?? []; a.push(e); inC.set(e.container, a) }
      else root.push(e)
    }
    return { root, inC }
  }, [filtered])

  const Row = ({ e }: { e: ElementSummary }) => (
    <div
      onClick={(ev) => select(e.id, ev.metaKey || ev.ctrlKey)}
      className={`group flex cursor-pointer items-center gap-1.5 rounded px-1.5 py-1 text-[12px] ${
        selectedId === e.id ? 'bg-blue-100 text-blue-900' : 'text-slate-600 hover:bg-slate-100'
      }`}
    >
      <TypeBadge type={iconTypeOf(e)} size={15} />
      <span className="min-w-0 flex-1 truncate">{e.name}</span>
      {e.unit && e.unit !== '1' && <span className="text-[9px] text-slate-400">{e.unit}</span>}
      <button
        onClick={(ev) => { ev.stopPropagation(); duplicate(e.id) }}
        className="hidden text-slate-300 hover:text-blue-500 group-hover:block"
        title="Duplicate"
      >⧉</button>
      <button
        onClick={(ev) => { ev.stopPropagation(); remove(e.id) }}
        className="hidden text-slate-300 hover:text-red-500 group-hover:block"
        title="Delete"
      >×</button>
    </div>
  )

  return (
    <div className="flex h-full flex-col">
      {/* Panel switch */}
      <div className="flex border-b border-slate-200 text-[11px] font-medium">
        {(['browse', 'palette'] as Panel[]).map((p) => (
          <button key={p} onClick={() => setPanel(p)}
            className={`flex-1 py-2 ${panel === p ? 'border-b-2 border-blue-600 text-blue-600' : 'text-slate-500 hover:text-slate-700'}`}>
            {p === 'browse' ? 'Browser' : 'Palette'}
          </button>
        ))}
      </div>

      {panel === 'palette' ? (
        <div className="flex-1 overflow-auto"><Palette /></div>
      ) : (
        <>
          <div className="space-y-2 border-b border-slate-100 p-2">
            <input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Search name, id, type, unit…"
              className="w-full rounded border border-slate-300 px-2 py-1 text-xs outline-none focus:border-blue-400"
            />
            <div className="flex gap-1 text-[10px]">
              {(['containment', 'type'] as Lens[]).map((l) => (
                <button key={l} onClick={() => setLens(l)}
                  className={`rounded px-2 py-0.5 ${lens === l ? 'bg-slate-800 text-white' : 'bg-slate-100 text-slate-500'}`}>
                  {l === 'containment' ? 'By container' : 'By type'}
                </button>
              ))}
              <span className="ml-auto self-center text-slate-400">{elements.length} elems</span>
            </div>
          </div>

          <div className="flex-1 overflow-auto p-1.5">
            {elements.length === 0 && (
              <p className="px-2 py-6 text-center text-[11px] text-slate-400">
                Empty model. Add elements from the Palette.
              </p>
            )}
            {lens === 'type'
              ? byType.map(([k, els]) => (
                  <div key={k} className="mb-2">
                    <div className="px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wide text-slate-400">{k} · {els.length}</div>
                    {els.map((e) => <Row key={e.id} e={e} />)}
                  </div>
                ))
              : (
                <>
                  {containers.map((c) => {
                    const els = byContainer.inC.get(c.id) ?? []
                    if (els.length === 0 && q) return null
                    return (
                      <div key={c.id} className="mb-2">
                        <div className="flex items-center gap-1 px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wide text-sky-600">
                          <TypeBadge type="container" size={13} /> {c.name}
                        </div>
                        <div className="pl-2">{els.map((e) => <Row key={e.id} e={e} />)}</div>
                      </div>
                    )
                  })}
                  {byContainer.root.map((e) => <Row key={e.id} e={e} />)}
                </>
              )}
          </div>
        </>
      )}
    </div>
  )
}
