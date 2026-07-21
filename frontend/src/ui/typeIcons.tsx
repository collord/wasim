// Shared per-type icon + colour system (extracted from GraphTab so the canvas, browser,
// palette, and inspector all render elements identically).

import type { ElementSummary } from '../types'

export const TYPE_STROKE: Record<string, string> = {
  constant: '#64748b',
  random_variable: '#3b82f6',
  expression: '#7c3aed',
  accumulator: '#d97706',
  timeseries: '#059669',
  lookup: '#0891b2',
  delay: '#ea580c',
  script: '#e11d48',
  stock: '#d97706',
  link: '#0d9488',
  event: '#e11d48',
  gate: '#4f46e5',
  cell: '#65a30d',
  species: '#64748b',
  medium: '#64748b',
  submodel: '#0369a1',
  container: '#0369a1',
}

export const TYPE_BG: Record<string, string> = {
  constant: '#f8fafc',
  random_variable: '#eff6ff',
  expression: '#f5f3ff',
  accumulator: '#fffbeb',
  timeseries: '#f0fdf4',
  lookup: '#ecfeff',
  delay: '#fff7ed',
  script: '#fff1f2',
  stock: '#fffbeb',
  link: '#f0fdfa',
  event: '#fff1f2',
  gate: '#eef2ff',
  cell: '#f7fee7',
  species: '#f8fafc',
  medium: '#f8fafc',
  submodel: '#f0f9ff',
  container: '#f0f9ff',
}

/** The icon-type key for a summary element (prefers the legacy `type`, falls back to the
 *  v2 primitive so v2-native models still get icons). */
export function iconTypeOf(e: Pick<ElementSummary, 'type' | 'primitive' | 'value_rule'>): string {
  if (e.type && TYPE_STROKE[e.type]) return e.type
  if (e.primitive === 'node') {
    switch (e.value_rule) {
      case 'fixed': return 'constant'
      case 'sample': return 'random_variable'
      case 'expression': return 'expression'
      case 'lookup': return 'lookup'
      case 'series': return 'timeseries'
      case 'lag': case 'filter': case 'queue': return 'delay'
      default: return 'expression'
    }
  }
  return e.primitive ?? 'constant'
}

export function TypeIcon({ type }: { type: string }) {
  switch (type) {
    case 'constant':
      return <>
        <circle cx="10" cy="7.5" r="4.5" fill="currentColor" />
        <line x1="10" y1="12" x2="10" y2="19" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" />
        <line x1="7" y1="19" x2="13" y2="19" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
      </>
    case 'random_variable':
      return <path d="M1,19 C3,19 5,11 7,9 C9,5 11,5 13,9 C15,11 17,19 19,19"
        fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
    case 'expression':
      return <text x="10" y="15" textAnchor="middle" fontSize="10"
        fontFamily="ui-monospace,monospace" fill="currentColor" fontWeight="700">f(x)</text>
    case 'accumulator':
    case 'stock':
      return <>
        <rect x="3" y="4" width="14" height="14" rx="2" fill="currentColor" fillOpacity="0.12" stroke="currentColor" strokeWidth="1.8" />
        <rect x="3" y="11" width="14" height="7" rx="2" fill="currentColor" fillOpacity="0.55" />
        <line x1="7" y1="2" x2="7" y2="4" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
        <line x1="13" y1="2" x2="13" y2="4" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
      </>
    case 'timeseries':
      return <polyline points="1,18 4,18 4,12 7,12 7,15 10,15 10,8 13,8 13,12 16,12 16,5 19,5"
        fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" />
    case 'lookup':
      return <>
        <rect x="2" y="3" width="16" height="15" rx="2" fill="currentColor" fillOpacity="0.1" stroke="currentColor" strokeWidth="1.6" />
        <rect x="2" y="3" width="16" height="5" rx="2" fill="currentColor" fillOpacity="0.35" />
        <line x1="2" y1="8" x2="18" y2="8" stroke="currentColor" strokeWidth="1.1" />
        <line x1="2" y1="13" x2="18" y2="13" stroke="currentColor" strokeWidth="1.1" />
        <line x1="10" y1="8" x2="10" y2="18" stroke="currentColor" strokeWidth="1.1" />
      </>
    case 'delay':
      return <>
        <circle cx="10" cy="10" r="8" fill="none" stroke="currentColor" strokeWidth="1.8" />
        <polyline points="10,4 10,10 14,13" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" />
      </>
    case 'link':
      return <>
        <circle cx="4" cy="10" r="2.6" fill="currentColor" />
        <circle cx="16" cy="10" r="2.6" fill="currentColor" />
        <line x1="6.4" y1="10" x2="13.4" y2="10" stroke="currentColor" strokeWidth="2" />
      </>
    case 'event':
      return <path d="M11,2 L4,11 L9,11 L8,18 L16,8 L11,8 Z" fill="currentColor" />
    case 'gate':
      return <path d="M4,4 L11,4 A6,6 0 0 1 11,16 L4,16 Z" fill="none" stroke="currentColor" strokeWidth="1.8" />
    case 'cell':
      return <>
        <circle cx="10" cy="10" r="7" fill="currentColor" fillOpacity="0.15" stroke="currentColor" strokeWidth="1.6" />
        <circle cx="10" cy="10" r="2.4" fill="currentColor" />
      </>
    case 'submodel':
    case 'container':
      return <>
        <rect x="2" y="2" width="16" height="16" rx="2.5" fill="none" stroke="currentColor" strokeWidth="1.7" strokeDasharray="3 2" />
        <rect x="6" y="6" width="8" height="8" rx="1.5" fill="currentColor" fillOpacity="0.5" />
      </>
    default:
      return <circle cx="10" cy="10" r="7" fill="currentColor" fillOpacity="0.45" stroke="currentColor" strokeWidth="1.5" />
  }
}

/** A small inline badge (used in the browser/palette/inspector lists). */
export function TypeBadge({ type, size = 20 }: { type: string; size?: number }) {
  const color = TYPE_STROKE[type] ?? '#94a3b8'
  return (
    <svg width={size} height={size} viewBox="0 0 20 20" style={{ color, flexShrink: 0 }}>
      <TypeIcon type={type} />
    </svg>
  )
}
