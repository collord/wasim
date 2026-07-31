import { describe, it, expect } from 'vitest'
import { BUILTINS, TIME_REFS } from './ast'
import { FUNCTION_CATALOG, TIME_REF_CATALOG, FUNCTION_GROUPS } from './functionRef'
import functionsDoc from './functions.json'

/**
 * The Functions reference (§9) enriches the parser rosters with docs. This guards the sync
 * contract: every builtin/time-ref is documented, and functions.json documents nothing that
 * doesn't exist — so the panel can never drift from what the engine actually accepts.
 */

describe('functions.json ↔ roster sync', () => {
  it('documents every builtin, and no phantom builtins', () => {
    const rosterNames = BUILTINS.map((b) => b.name).sort()
    const docNames = functionsDoc.functions.map((f) => f.name).sort()
    expect(docNames).toEqual(rosterNames)
  })

  it('documents every time ref, and no phantom time refs', () => {
    const rosterNames = [...TIME_REFS].sort()
    const docNames = functionsDoc.timeRefs.map((t) => t.name).sort()
    expect(docNames).toEqual(rosterNames)
  })

  it('every builtin doc is non-empty and has an example', () => {
    for (const f of functionsDoc.functions) {
      expect(f.doc, `doc for ${f.name}`).toBeTruthy()
      expect(f.example, `example for ${f.name}`).toBeTruthy()
    }
  })
})

describe('enriched catalog', () => {
  it('preserves roster order and count, with docs attached', () => {
    expect(FUNCTION_CATALOG.map((e) => e.name)).toEqual(BUILTINS.map((b) => b.name))
    for (const e of FUNCTION_CATALOG) {
      expect(e.doc).toBeTruthy()
      expect(e.sig).toBeTruthy() // carried from the roster
    }
  })

  it('groups cover every function exactly once', () => {
    const flat = FUNCTION_GROUPS.flatMap((g) => g.entries.map((e) => e.name)).sort()
    expect(flat).toEqual(BUILTINS.map((b) => b.name).sort())
  })

  it('time-ref catalog matches the roster in order', () => {
    expect(TIME_REF_CATALOG.map((t) => t.name)).toEqual([...TIME_REFS])
    for (const t of TIME_REF_CATALOG) expect(t.doc).toBeTruthy()
  })
})
