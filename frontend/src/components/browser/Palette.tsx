import { PALETTE } from '../../model/edits'
import { useStore, useActiveLens } from '../../store'
import { slugify } from '../../model/edits'
import { TypeBadge } from '../../ui/typeIcons'

/** The element palette (spec §3): each entry inserts a specific primitive with defaults.
 *  Entries the engine can't run are simply absent (no Script element, etc.).
 *
 *  The palette is projected through the active lens (`WASIM_LENS_IMPLEMENTATION_PLAN.md` Part A):
 *  the lens chooses, groups, and orders which `PALETTE` entries show. The `general` lens returns
 *  the full union in file order, so behavior is unchanged when no lens is selected. */
export function Palette() {
  const addNewElement = useStore((s) => s.addNewElement)
  const format = useStore((s) => s.format)
  const lens = useActiveLens()
  const groups = lens.palette(PALETTE)

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
        <div key={g.label}>
          <div className="mb-1 px-1 text-[10px] font-semibold uppercase tracking-wide text-slate-400">{g.label}</div>
          <div className="grid grid-cols-2 gap-1">
            {g.entries.map((p) => (
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
