import type { FlatElement, ModelDoc } from '../../model/schema'
import type { LensId } from '../types'
import {
  isStock, isGate, isEvent, isStatus, isPid,
  flowIds, eventsFeedingGates, isCouplingMatrix, reaches, optimizationOf,
} from './structure'

/**
 * Computed lens roles (`EMITTER_LENS_TARGETING.md` §2): derive each element's domain role for a
 * given lens **from structure**, so the existing behaviors (which read `lens_role`) work on
 * imported/untagged models, not only on in-app-authored ones. Roles are *lens-relative* — a stock
 * is `stock` under stock-flow but `compartment` under metapop — so this is keyed by `LensId`.
 *
 * Migration safety: the wiring (`withComputedRoles`, `lensHint.ts`) resolves a role as
 * `computed ?? stored`, so where structure is silent (e.g. metapop's scalar `mix` tagged
 * `coupling`, or a force-of-infection tagged `mixing` that can't be re-derived) the stamped role
 * still wins. On template models this reduces to today's behavior; the computed layer only *adds*
 * roles where none were stamped.
 */

export type RoleMap = Map<string, string>

/** stock-flow: stocks are `stock`; nodes referenced by a stock's in/outflows are `flow`. */
function stockFlowRoles(els: FlatElement[]): RoleMap {
  const flows = flowIds(els)
  const m: RoleMap = new Map()
  for (const e of els) {
    if (isStock(e)) m.set(e.id, 'stock')
    else if (flows.has(e.id)) m.set(e.id, 'flow')
  }
  return m
}

/** metapop: the stock-flow substrate re-read as compartments/transitions, plus the two nouns that
 *  make it a network — a square coupling matrix (`coupling`) and a term reaching both a coupling
 *  and a compartment (`mixing`). Scalar couplings / mixing weights are intentionally left unroled
 *  (structurally indistinguishable from parameters) and rely on a stored role. */
function metapopRoles(doc: ModelDoc, els: FlatElement[]): RoleMap {
  const flows = flowIds(els)
  const m: RoleMap = new Map()
  for (const e of els) {
    if (isStock(e)) m.set(e.id, 'compartment')
    else if (isCouplingMatrix(e, doc)) m.set(e.id, 'coupling')
    else if (flows.has(e.id)) m.set(e.id, 'transition')
  }
  // mixing: a (non-flow) term whose closure reaches a coupling matrix and a compartment.
  const isCoupling = (x: FlatElement) => m.get(x.id) === 'coupling'
  const isCompartment = (x: FlatElement) => m.get(x.id) === 'compartment'
  for (const e of els) {
    if (m.has(e.id)) continue
    if (reaches(doc, e.id, isCoupling) && reaches(doc, e.id, isCompartment)) m.set(e.id, 'mixing')
  }
  // NB: a *scalar* coupling weight is deliberately left unroled here — it is structurally identical
  // to a scalar `parameter` (both are `fixed` leaves with `editable`/`bounds`; see the two-patch-sir
  // template's `mix` vs `beta`), so guessing either would mis-tag the other. That subtle case relies
  // on a stored role; `withComputedRoles` fills only where structure is *unambiguous* (see below).
  return m
}

/** reliability: fault-tree `gate`s are `redundancy`; the `event` components feeding them are
 *  `component`; damage `stock`s are `state`. (Standalone events with no gate stay unroled here —
 *  a bare failure model still has its `component` events, see below.) */
function reliabilityRoles(els: FlatElement[]): RoleMap {
  const m: RoleMap = new Map()
  for (const e of els) {
    if (isGate(e)) m.set(e.id, 'redundancy')
    else if (isEvent(e)) m.set(e.id, 'component') // a reliability event is a failure component
    else if (isStock(e)) m.set(e.id, 'state')
  }
  return m
}

/** decision: elements named by the optimization study — its variables are `decision`, its
 *  objective element is `objective`. Read from the top-level `optimization` block. */
function decisionRoles(doc: ModelDoc): RoleMap {
  const m: RoleMap = new Map()
  const opt = optimizationOf(doc)
  if (!opt) return m
  for (const v of opt.variables ?? []) if (v.element_id) m.set(v.element_id, 'decision')
  const obj = opt.objective as { element_id?: string } | undefined
  if (obj?.element_id) m.set(obj.element_id, 'objective')
  return m
}

/** discrete-event: `event`s NOT feeding a fault-tree gate are the paradigm's own; `status` nodes
 *  are latches. Roles are coarse (generator/effect/branch/milestone need `source_type`, which the
 *  frontend doesn't read); the lens can refine on render once the emitter fills the fidelity gaps. */
function discreteEventRoles(els: FlatElement[]): RoleMap {
  const feeders = eventsFeedingGates(els)
  const m: RoleMap = new Map()
  for (const e of els) {
    if (isStatus(e)) m.set(e.id, 'latch')
    else if (isEvent(e) && !feeders.has(e.id)) m.set(e.id, 'event')
  }
  return m
}

/** control-systems: the `pid` node is the `controller`. Plant refs (sensed input / actuated
 *  output) are an overlay highlight, not a role — they never relabel the plant element. */
function controlRoles(els: FlatElement[]): RoleMap {
  const m: RoleMap = new Map()
  for (const e of els) if (isPid(e)) m.set(e.id, 'controller')
  return m
}

/** Structural roles for `lensId` over the whole doc (elements resolved globally; the hint scopes
 *  by container separately). Returns an empty map for lenses without a role vocabulary (general). */
export function computeRolesForLens(doc: ModelDoc, lensId: LensId): RoleMap {
  const els = doc.elements
  switch (lensId) {
    case 'stock-flow': return stockFlowRoles(els)
    case 'metapop': return metapopRoles(doc, els)
    case 'reliability': return reliabilityRoles(els)
    case 'decision': return decisionRoles(doc)
    case 'discrete-event': return discreteEventRoles(els)
    case 'control-systems': return controlRoles(els)
    default: return new Map()
  }
}

/** A doc clone whose elements carry computed `lens_role`s for `lensId` — the safe migration
 *  bridge that lets the `lens_role`-reading behaviors work on imported/untagged models.
 *
 *  **Stored roles are authoritative when present.** If the doc already carries *any* `lens_role`
 *  (an in-app-authored model, e.g. every shipped template), it is returned unchanged — so the
 *  migration is provably non-regressive on authored docs, and the subtler stamped roles the
 *  structure can't re-derive (metapop's scalar `coupling`/`mixing`) are preserved exactly. Only a
 *  fully-untagged doc (a fresh import) gets computed roles filled in. Clones only the elements
 *  whose role changes, keeping references stable elsewhere. */
export function withComputedRoles(doc: ModelDoc, lensId: LensId): ModelDoc {
  const hasStored = doc.elements.some((e) => e.lens_role != null)
  if (hasStored) return doc
  const roles = computeRolesForLens(doc, lensId)
  if (roles.size === 0) return doc
  let changed = false
  const elements = doc.elements.map((e) => {
    const role = roles.get(e.id)
    if (role == null || role === e.lens_role) return e
    changed = true
    return { ...e, lens_role: role }
  })
  return changed ? { ...doc, elements } : doc
}
