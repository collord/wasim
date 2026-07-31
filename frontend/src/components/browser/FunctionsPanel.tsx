import { useMemo, useState } from 'react'
import {
  FUNCTION_GROUPS,
  TIME_REF_CATALOG,
  insertIntoActiveExpression,
  type FnEntry,
} from '../../model/functionRef'

/**
 * The Functions reference panel (WASIM_LENS_MANIFEST_SPEC.md §9): the ~60 expression builtins and
 * the time references, browsable by category with signatures + docs — so a user who doesn't know a
 * function's name can discover it, not just type-to-filter it in the inserter. Clicking a function
 * inserts it into the focused expression editor (the last one focused); if none is focused, the
 * click is a no-op with a hint.
 */
export function FunctionsPanel() {
  const [q, setQ] = useState('')
  const query = q.trim().toLowerCase()
  const [flash, setFlash] = useState<string | null>(null)

  const groups = useMemo(() => {
    if (!query) return FUNCTION_GROUPS
    return FUNCTION_GROUPS.map((g) => ({
      group: g.group,
      entries: g.entries.filter((e) => e.name.includes(query) || e.doc?.toLowerCase().includes(query)),
    })).filter((g) => g.entries.length > 0)
  }, [query])

  const timeRefs = useMemo(
    () => (query ? TIME_REF_CATALOG.filter((t) => t.name.includes(query) || t.doc?.toLowerCase().includes(query)) : TIME_REF_CATALOG),
    [query],
  )

  // Builtins insert as `name()`; time refs insert as the bare token.
  const insert = (token: string) => {
    const ok = insertIntoActiveExpression(token)
    setFlash(ok ? null : 'Focus an expression field first, then click a function to insert it.')
  }

  const nothing = groups.length === 0 && timeRefs.length === 0

  return (
    <div className="flex h-full flex-col" data-testid="functions-panel">
      <div className="p-2">
        <input
          value={q}
          onChange={(e) => setQ(e.target.value)}
          placeholder="filter functions…"
          className="w-full rounded border border-slate-200 bg-slate-50 px-2 py-1 text-[11px] outline-none focus:border-blue-300"
        />
      </div>
      {flash && <div className="mx-2 mb-1 rounded bg-amber-50 px-2 py-1 text-[10px] text-amber-700">{flash}</div>}
      <div className="flex-1 overflow-auto px-2 pb-2">
        {nothing && <div className="px-1 py-2 text-[11px] text-slate-400">No functions match “{q}”.</div>}
        {groups.map((g) => (
          <FnGroup key={g.group} label={g.group} entries={g.entries} onInsert={(name) => insert(`${name}()`)} />
        ))}
        {timeRefs.length > 0 && (
          <div className="mb-2">
            <div className="mb-1 px-1 text-[10px] font-semibold uppercase tracking-wide text-slate-400">Time</div>
            <ul className="space-y-0.5">
              {timeRefs.map((t) => (
                <li key={t.name}>
                  <button
                    onMouseDown={(e) => e.preventDefault()}
                    onClick={() => insert(t.name)}
                    title={t.doc}
                    className="w-full rounded px-1.5 py-1 text-left hover:bg-emerald-50"
                  >
                    <span className="font-mono text-[11px] text-emerald-700">{t.name}</span>
                    {t.doc && <span className="ml-2 text-[10px] text-slate-400">{t.doc}</span>}
                  </button>
                </li>
              ))}
            </ul>
          </div>
        )}
      </div>
    </div>
  )
}

function FnGroup({ label, entries, onInsert }: { label: string; entries: FnEntry[]; onInsert: (name: string) => void }) {
  return (
    <div className="mb-2">
      <div className="mb-1 px-1 text-[10px] font-semibold uppercase tracking-wide text-slate-400">{label}</div>
      <ul className="space-y-0.5">
        {entries.map((e) => (
          <li key={e.name}>
            <button
              // preventDefault on mousedown keeps the focused expression editor focused (so it stays
              // the insert target); the click still fires.
              onMouseDown={(ev) => ev.preventDefault()}
              onClick={() => onInsert(e.name)}
              title={e.example ? `${e.doc ?? ''}  ·  e.g. ${e.example}`.trim() : e.doc}
              className="w-full rounded px-1.5 py-1 text-left hover:bg-violet-50"
            >
              <span className="font-mono text-[11px] text-violet-700">{e.sig}</span>
              {e.doc && <span className="ml-2 block truncate text-[10px] text-slate-400">{e.doc}</span>}
            </button>
          </li>
        ))}
      </ul>
    </div>
  )
}
