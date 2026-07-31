import { BUILTINS, TIME_REFS, type FnDoc } from './ast'
import functionsDoc from './functions.json'

/**
 * The Functions reference catalog (WASIM_LENS_MANIFEST_SPEC.md §9): the parser's `BUILTINS` /
 * `TIME_REFS` rosters (name + signature + group) enriched with the prose docs + examples from
 * `functions.json`. The roster stays the source of truth for *what exists* (it mirrors the engine's
 * `BuiltinFn` / `TimeProperty` enums); `functions.json` only adds documentation, keyed by name — so
 * a function can never be documented without existing, nor exist undocumented (guarded by a test).
 */

export interface FnEntry extends FnDoc {
  doc?: string
  example?: string
}

export interface TimeRefEntry {
  name: string
  doc?: string
}

const DOC_BY_NAME = new Map(functionsDoc.functions.map((f) => [f.name, f]))
const TIME_DOC_BY_NAME = new Map(functionsDoc.timeRefs.map((t) => [t.name, t]))

/** Builtins with docs attached, in roster order. */
export const FUNCTION_CATALOG: FnEntry[] = BUILTINS.map((b) => {
  const d = DOC_BY_NAME.get(b.name)
  return { ...b, doc: d?.doc, example: d?.example }
})

/** Time references with docs attached, in roster order. All share the group "Time". */
export const TIME_REF_CATALOG: TimeRefEntry[] = TIME_REFS.map((name) => ({
  name,
  doc: TIME_DOC_BY_NAME.get(name)?.doc,
}))

/** Builtins grouped by category (roster order preserved within each group), for the panel. */
export const FUNCTION_GROUPS: { group: string; entries: FnEntry[] }[] = (() => {
  const order: string[] = []
  const byGroup = new Map<string, FnEntry[]>()
  for (const e of FUNCTION_CATALOG) {
    let bucket = byGroup.get(e.group)
    if (!bucket) {
      bucket = []
      byGroup.set(e.group, bucket)
      order.push(e.group)
    }
    bucket.push(e)
  }
  return order.map((group) => ({ group, entries: byGroup.get(group)! }))
})()

// ── Active expression-insert target ──────────────────────────────────────────────
//
// The Functions panel lives in the left pane, outside any inspector. To let a click there insert
// into whichever expression is being edited, an `ExpressionEditor` registers its caret-insert
// callback while focused; the panel calls the active one. A tiny module-level registry (not store
// state — this is transient UI focus, not model data) keeps it out of the render path.

type Inserter = (token: string) => void
let activeInserter: Inserter | null = null

/** An `ExpressionEditor` claims the insert target on focus; releases it on blur (if still owner). */
export function setActiveExpressionTarget(fn: Inserter): void {
  activeInserter = fn
}
export function clearActiveExpressionTarget(fn: Inserter): void {
  if (activeInserter === fn) activeInserter = null
}

/** Insert into the focused expression, if any. Returns false when no editor is focused. */
export function insertIntoActiveExpression(token: string): boolean {
  if (!activeInserter) return false
  activeInserter(token)
  return true
}
export function hasActiveExpressionTarget(): boolean {
  return activeInserter !== null
}
