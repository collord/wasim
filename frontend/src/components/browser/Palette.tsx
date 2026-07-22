import { PALETTE } from '../../model/edits'
import { useStore } from '../../store'
import { slugify } from '../../model/edits'
import { TypeBadge } from '../../ui/typeIcons'

/** The element palette (spec §3): each entry inserts a specific primitive with defaults.
 *  Entries the engine can't run are simply absent (no Script element, etc.). */
export function Palette() {
  const addNewElement = useStore((s) => s.addNewElement)
  const format = useStore((s) => s.format)
  const groups = [...new Set(PALETTE.map((p) => p.group))]

  const insert = (key: string) => {
    const entry = PALETTE.find((p) => p.key === key)
    if (!entry) return
    const name = entry.label
    const el = entry.make(slugify(name), name, format)
    // Place near the origin; the canvas persists positions in the view block.
    addNewElement(el, { x: 120, y: 120 })
  }

  return (
    <div className="space-y-2 p-2">
      {groups.map((g) => (
        <div key={g}>
          <div className="mb-1 px-1 text-[10px] font-semibold uppercase tracking-wide text-slate-400">{g}</div>
          <div className="grid grid-cols-2 gap-1">
            {PALETTE.filter((p) => p.group === g).map((p) => (
              <button
                key={p.key}
                onClick={() => insert(p.key)}
                draggable
                onDragStart={(e) => e.dataTransfer.setData('application/wasim-palette', p.key)}
                className="flex items-center gap-1.5 rounded border border-slate-200 bg-white px-2 py-1.5 text-left text-[11px] text-slate-600 hover:border-blue-300 hover:bg-blue-50"
                title={`Insert ${p.label}`}
              >
                <TypeBadge type={p.iconType} size={16} />
                <span className="truncate">{p.label}</span>
              </button>
            ))}
          </div>
        </div>
      ))}
    </div>
  )
}
