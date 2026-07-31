/**
 * Frozen snapshot of the four hand-written lens specs as they existed BEFORE the manifest
 * refactor (the old `registry.ts`). The parity test (`loader.test.ts`) compiles the same four
 * lenses from JSON manifests and asserts the compiled projections match this snapshot exactly —
 * proving the refactor is behavior-preserving. This file is test-only; do not import from app code.
 */
import type { LensSpec } from '../types'
import { groupInOrder } from '../types'
import { STOCK_FLOW_TEMPLATES } from '../stockFlowTemplates'
import { RELIABILITY_TEMPLATES } from '../reliabilityTemplates'
import { DECISION_TEMPLATES } from '../decisionTemplates'

const legacyGeneral: LensSpec = {
  id: 'general',
  label: 'General',
  tagline: 'The full engine, unfiltered — every element type, no domain vocabulary.',
  palette: (all) => groupInOrder(all),
}

const legacyStockFlow: LensSpec = {
  id: 'stock-flow',
  label: 'Stock & Flow',
  tagline: 'Forrester stock-and-flow — stocks accumulate, flows are rates, guaranteed consistent.',
  palette: () => [
    { label: 'Stocks', items: [{ key: 'stock', label: 'Stock', iconType: 'accumulator', lensRole: 'stock' }] },
    { label: 'Flows', items: [{ key: 'expression', label: 'Flow', iconType: 'expression', lensRole: 'flow' }] },
    {
      label: 'Auxiliaries',
      items: [
        { key: 'expression', label: 'Auxiliary', iconType: 'expression', lensRole: 'auxiliary' },
        { key: 'constant', label: 'Constant', iconType: 'constant', lensRole: 'auxiliary' },
        { key: 'stochastic', label: 'Uncertain input', iconType: 'random_variable', lensRole: 'auxiliary' },
      ],
    },
  ],
  roleLabels: { stock: 'Stock', flow: 'Flow', auxiliary: 'Auxiliary' },
  glyphOf: (role) =>
    role === 'stock' ? 'box' : role === 'auxiliary' ? 'circle' : role === 'flow' ? 'valve' : 'default',
  templates: STOCK_FLOW_TEMPLATES,
  preferredResultId: (doc, outputIds) =>
    doc.elements.find((e) => e.lens_role === 'stock' && outputIds.includes(e.id))?.id ?? null,
}

const legacyReliability: LensSpec = {
  id: 'reliability',
  label: 'Reliability',
  tagline: 'Repairable components and failure FSMs — simulate-first RAM, not static block arithmetic.',
  palette: () => [
    { label: 'Components', items: [{ key: 'event', label: 'Component', iconType: 'event', lensRole: 'component' }] },
    { label: 'Redundancy', items: [{ key: 'gate', label: 'Redundancy gate', iconType: 'event', lensRole: 'redundancy' }] },
    { label: 'State', items: [{ key: 'stock', label: 'Damage state', iconType: 'accumulator', lensRole: 'state' }] },
    {
      label: 'Inputs',
      items: [
        { key: 'constant', label: 'Parameter', iconType: 'constant', lensRole: 'parameter' },
        { key: 'stochastic', label: 'Uncertain input', iconType: 'random_variable', lensRole: 'parameter' },
      ],
    },
  ],
  roleLabels: { component: 'Component', redundancy: 'Redundancy', state: 'Damage state', parameter: 'Parameter' },
  glyphOf: (role) => (role === 'component' || role === 'state' || role === 'redundancy' ? 'box' : 'default'),
  templates: RELIABILITY_TEMPLATES,
  preferredResultId: (doc, outputIds) => {
    const byRole = (r: string) => doc.elements.find((e) => e.lens_role === r && outputIds.includes(e.id))?.id
    return byRole('redundancy') ?? byRole('component') ?? null
  },
}

const legacyDecision: LensSpec = {
  id: 'decision',
  label: 'Decision',
  tagline: 'Decisions, chance inputs, and an objective — optimize under uncertainty and price information (VOI).',
  palette: () => [
    { label: 'Decisions', items: [{ key: 'constant', label: 'Decision', iconType: 'constant', lensRole: 'decision' }] },
    { label: 'Uncertainty', items: [{ key: 'stochastic', label: 'Chance input', iconType: 'random_variable', lensRole: 'chance' }] },
    { label: 'Objective', items: [{ key: 'expression', label: 'Objective', iconType: 'expression', lensRole: 'objective' }] },
  ],
  roleLabels: { decision: 'Decision', chance: 'Chance input', objective: 'Objective' },
  glyphOf: (role) => (role === 'decision' ? 'box' : role === 'chance' ? 'circle' : role === 'objective' ? 'hex' : 'default'),
  templates: DECISION_TEMPLATES,
  preferredResultId: (doc, outputIds) =>
    doc.elements.find((e) => e.lens_role === 'objective' && outputIds.includes(e.id))?.id ?? null,
}

export const LEGACY_LENSES: Record<string, LensSpec> = {
  general: legacyGeneral,
  'stock-flow': legacyStockFlow,
  reliability: legacyReliability,
  decision: legacyDecision,
}
