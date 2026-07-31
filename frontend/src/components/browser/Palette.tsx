import { useState } from 'react'
import { PALETTE } from '../../model/edits'
import { useStore, useActiveLens } from '../../store'
import { slugify } from '../../model/edits'
import { TypeBadge } from '../../ui/typeIcons'

/** The element palette (spec §3): each entry inserts a specific primitive with defaults.
 *  Entries the engine can't run are simply absent (no Script element, etc.).
 *
 *  The palette is projected through the active lens (`WASIM_LENS_MANIFEST_SPEC.md` §6): the lens
 *  chooses, groups, and orders which `PALETTE` entries show. The `general` lens returns the full
 *  registry catalog (§5 taxonomy). Sections flagged `advanced` collapse by default; items whose
 *  construct has no structured inspector yet (`editor: 'raw'`) are hinted so the user knows they
 *  edit via the JSON escape hatch. */
export function Palette() {
  const addNewElement = useStore((s) => s.addNewElement)
  const format = useStore((s) => s.format)
  const lens = useActiveLens()
  const groups = lens.palette(PALETTE)

  // Which advanced sections the user has expanded this session (non-advanced are always open).
  const [expanded, setExpanded] = useState<Record<string, boolean>>({})

  const insert = (key: string, label: string, lensRole?: string) => {
    const entry = PALETTE.find((p) => p.key === key)
    if (!entry) return
    const el = entry.make(slugify(label), label, format)
    // Tag the element with its lens role so the lens round-trips (re-opening reconstructs the
    // vocabulary rather than a raw graph). Engine-ignored; see WASIM_LENS_MANIFEST_SPEC.md.
    if (lensRole) el.lens_role = lensRole
    // Place near the origin; the canvas persists positions in the view block.
    addNewElement(el, { x: 120, y: 120 })
  }

  return (
    <div className="space-y-2 p-2">
      {groups.map((g) => {
        const isOpen = !g.advanced || expanded[g.label]
        return (
          <div key={g.label}>
            {g.advanced ? (
              <button
                onClick={() => setExpanded((s) => ({ ...s, [g.label]: !s[g.label] }))}
                aria-expanded={isOpen}
                className="mb-1 flex w-full items-center gap-1 px-1 text-[10px] font-semibold uppercase tracking-wide text-slate-400 hover:text-slate-600"
              >
                <span aria-hidden className="inline-block w-2 text-slate-400">{isOpen ? '▾' : '▸'}</span>
                <span>{g.label}</span>
              </button>
            ) : (
              <div className="mb-1 px-1 text-[10px] font-semibold uppercase tracking-wide text-slate-400">{g.label}</div>
            )}
            {isOpen && (
              <div className="grid grid-cols-2 gap-1">
                {g.items.map((p) => (
                  <button
                    key={`${p.key}:${p.label}`}
                    onClick={() => insert(p.key, p.label, p.lensRole)}
                    draggable
                    onDragStart={(e) => e.dataTransfer.setData('application/wasim-palette', p.key)}
                    className="flex items-center gap-1.5 rounded border border-slate-200 bg-white px-2 py-1.5 text-left text-[11px] text-slate-600 hover:border-blue-300 hover:bg-blue-50"
                    title={p.editor === 'raw' ? `Insert ${p.label} — edits as raw JSON (no form yet)` : `Insert ${p.label}`}
                  >
                    <TypeBadge type={p.iconType} size={16} />
                    <span className="truncate">{p.label}</span>
                    {p.editor === 'raw' && (
                      <span
                        aria-hidden
                        className="ml-auto shrink-0 text-[8px] font-semibold uppercase tracking-wide text-slate-300"
                        title="Edited as raw JSON"
                      >
                        {'{}'}
                      </span>
                    )}
                  </button>
                ))}
              </div>
            )}
          </div>
        )
      })}
    </div>
  )
}
