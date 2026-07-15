import { useRef, useState, useCallback, useMemo, useEffect } from 'react'
import dagre from '@dagrejs/dagre'
import { useStore } from '../../store'
import type { ContainerDef, ElementSummary } from '../../types'
import { unitLabel } from '../../display'

// ── Layout constants ───────────────────────────────────────────────────────────

const NODE_W = 172
const NODE_H = 54
const CGROUP_W = 160
const CGROUP_HEADER_H = 28
const CGROUP_MEMBER_H = 19
const CGROUP_PAD_B = 8

// Collapsed submodel box: header + a body line per interface output (capped) + a stats line.
const SUB_W = 190
const SUB_HEADER_H = 30
const SUB_LINE_H = 18
const SUB_PAD_B = 10
const SUB_MAX_OUTPUTS = 5
// Padding of the expanded submodel frame around its interior nodes.
const SUB_FRAME_PAD = 24
const SUB_FRAME_HEADER_H = 30

// ── Per-type colour palette ────────────────────────────────────────────────────

const TYPE_STROKE: Record<string, string> = {
  constant:        '#64748b',
  random_variable: '#3b82f6',
  expression:      '#7c3aed',
  accumulator:     '#d97706',
  timeseries:      '#059669',
  lookup:          '#0891b2',
  delay:           '#ea580c',
  script:          '#e11d48',
  // v2 primitives
  stock:           '#d97706',
  link:            '#0d9488',
  event:           '#e11d48',
  gate:            '#4f46e5',
  cell:            '#65a30d',
  species:         '#64748b',
  medium:          '#64748b',
  submodel:        '#0369a1',
}

const TYPE_BG: Record<string, string> = {
  constant:        '#f8fafc',
  random_variable: '#eff6ff',
  expression:      '#f5f3ff',
  accumulator:     '#fffbeb',
  timeseries:      '#f0fdf4',
  lookup:          '#ecfeff',
  delay:           '#fff7ed',
  script:          '#fff1f2',
  // v2 primitives
  stock:           '#fffbeb',
  link:            '#f0fdfa',
  event:           '#fff1f2',
  gate:            '#eef2ff',
  cell:            '#f7fee7',
  species:         '#f8fafc',
  medium:          '#f8fafc',
  submodel:        '#f0f9ff',
}

// ── Type icons (20 × 20 coordinate space) ─────────────────────────────────────

function TypeIcon({ type }: { type: string }) {
  switch (type) {
    case 'constant':
      return <>
        <circle cx="10" cy="7.5" r="4.5" fill="currentColor" />
        <line x1="10" y1="12" x2="10" y2="19" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" />
        <line x1="7"  y1="19" x2="13" y2="19" stroke="currentColor" strokeWidth="2"   strokeLinecap="round" />
      </>
    case 'random_variable':
      return <path
        d="M1,19 C3,19 5,11 7,9 C9,5 11,5 13,9 C15,11 17,19 19,19"
        fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round"
      />
    case 'expression':
      return <text x="10" y="15" textAnchor="middle" fontSize="10"
        fontFamily="ui-monospace,monospace" fill="currentColor" fontWeight="700">f(x)</text>
    case 'accumulator':
      return <>
        <rect x="3" y="4" width="14" height="14" rx="2"
          fill="currentColor" fillOpacity="0.12" stroke="currentColor" strokeWidth="1.8" />
        <rect x="3" y="11" width="14" height="7" rx="2"
          fill="currentColor" fillOpacity="0.55" />
        <line x1="7" y1="2" x2="7" y2="4" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
        <line x1="13" y1="2" x2="13" y2="4" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" />
      </>
    case 'timeseries':
      return <polyline
        points="1,18 4,18 4,12 7,12 7,15 10,15 10,8 13,8 13,12 16,12 16,5 19,5"
        fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round"
      />
    case 'lookup':
      return <>
        <rect x="2" y="3" width="16" height="15" rx="2"
          fill="currentColor" fillOpacity="0.1" stroke="currentColor" strokeWidth="1.6" />
        <rect x="2" y="3" width="16" height="5" rx="2"
          fill="currentColor" fillOpacity="0.35" />
        <line x1="2"  y1="8"  x2="18" y2="8"  stroke="currentColor" strokeWidth="1.1" />
        <line x1="2"  y1="13" x2="18" y2="13" stroke="currentColor" strokeWidth="1.1" />
        <line x1="10" y1="8"  x2="10" y2="18" stroke="currentColor" strokeWidth="1.1" />
      </>
    case 'delay':
      return <>
        <circle cx="10" cy="10" r="8" fill="none" stroke="currentColor" strokeWidth="1.8" />
        <polyline points="10,4 10,10 14,13"
          fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" />
      </>
    case 'script':
      return <text x="10" y="16" textAnchor="middle" fontSize="15"
        fontFamily="ui-monospace,monospace" fill="currentColor" fontWeight="600">{'{}'}</text>
    case 'submodel':
      // A box within a box — a nested model.
      return <>
        <rect x="2" y="2" width="16" height="16" rx="2.5"
          fill="none" stroke="currentColor" strokeWidth="1.7" strokeDasharray="3 2" />
        <rect x="6" y="6" width="8" height="8" rx="1.5"
          fill="currentColor" fillOpacity="0.5" />
      </>
    default:
      return <circle cx="10" cy="10" r="7" fill="currentColor" fillOpacity="0.45"
        stroke="currentColor" strokeWidth="1.5" />
  }
}

// ── Layout types ──────────────────────────────────────────────────────────────

interface LayoutElement {
  id: string; name: string; type: string; unit: string
  description: string | null
  x: number; y: number; w: number; h: number
}

interface LayoutConstGroup {
  id: string
  members: Array<{ id: string; name: string }>
  x: number; y: number; w: number; h: number
}

// A collapsed submodel, drawn as one aggregate box.
interface LayoutSubmodel {
  id: string; name: string
  realizations: number | null
  outputs: string[]          // interface output leaf names (capped)
  extraOutputs: number       // count beyond the cap
  x: number; y: number; w: number; h: number
}

// The enclosing frame drawn behind an expanded submodel's interior nodes.
interface LayoutSubFrame {
  id: string; name: string
  realizations: number | null
  x: number; y: number; w: number; h: number
}

interface LayoutEdge {
  from: string; to: string
  points: Array<{ x: number; y: number }>
}

interface Layout {
  elements: LayoutElement[]
  constGroups: LayoutConstGroup[]
  submodels: LayoutSubmodel[]
  subFrames: LayoutSubFrame[]
  edges: LayoutEdge[]
  width: number; height: number
}

// ── Dagre layout ──────────────────────────────────────────────────────────────

const leaf = (id: string) => id.split('/').pop() ?? id

function buildLayout(
  elements: ElementSummary[],
  containers: ContainerDef[],
  expanded: Set<string>,
  unitMap: Record<string, string>,
  descMap: Record<string, string | null>,
): Layout {
  const knownIds = new Set(elements.map((e) => e.id))

  // ── Submodel membership (transitive: walk each element's container up parents) ──
  const parentOf = new Map<string, string | null>(containers.map((c) => [c.id, c.parent]))
  const submodelIds = new Set(containers.filter((c) => c.kind === 'submodel').map((c) => c.id))
  const subById = new Map(containers.map((c) => [c.id, c]))

  // The submodel (if any) an element belongs to, walking up the container chain.
  const submodelOf = (container: string | null | undefined): string | null => {
    let cur = container ?? null
    const seen = new Set<string>()
    while (cur && !seen.has(cur)) {
      seen.add(cur)
      if (submodelIds.has(cur)) return cur
      cur = parentOf.get(cur) ?? null
    }
    return null
  }

  // ── Partition elements ──
  const ungroupedConstants: ElementSummary[] = []
  const individualElems: ElementSummary[] = []      // rendered as their own node
  const collapsedMembers = new Map<string, ElementSummary[]>() // subId → interior elems

  for (const e of elements) {
    const sub = submodelOf(e.container)
    if (sub && !expanded.has(sub)) {
      // Interior of a collapsed submodel → folded into the submodel box.
      const members = collapsedMembers.get(sub) ?? []
      members.push(e)
      collapsedMembers.set(sub, members)
    } else if (e.type === 'constant' && !e.container) {
      ungroupedConstants.push(e)
    } else {
      individualElems.push(e)
    }
  }

  // ── id → graph node id (constants fold to CGROUP; collapsed-submodel interior folds to subId) ──
  const nodeOf = new Map<string, string>()
  const CGROUP_ID = '~constants'
  for (const e of ungroupedConstants) nodeOf.set(e.id, CGROUP_ID)
  for (const e of individualElems)    nodeOf.set(e.id, e.id)
  for (const [subId, members] of collapsedMembers) {
    for (const e of members) nodeOf.set(e.id, subId)
  }

  // ── Dagre graph (compound so expanded-submodel interiors cluster together) ──
  const g = new dagre.graphlib.Graph({ multigraph: false, compound: true })
  g.setGraph({ rankdir: 'LR', nodesep: 32, ranksep: 80, marginx: 48, marginy: 48 })
  g.setDefaultEdgeLabel(() => ({}))

  if (ungroupedConstants.length > 0) {
    const h = CGROUP_HEADER_H + ungroupedConstants.length * CGROUP_MEMBER_H + CGROUP_PAD_B
    g.setNode(CGROUP_ID, { width: CGROUP_W, height: h })
  }

  // Collapsed submodel boxes (one node each). Height scales with capped output list.
  const subHeight = (nOut: number) =>
    SUB_HEADER_H + SUB_LINE_H /* realizations line */ +
    Math.min(nOut, SUB_MAX_OUTPUTS) * SUB_LINE_H + (nOut > SUB_MAX_OUTPUTS ? SUB_LINE_H : 0) + SUB_PAD_B
  for (const [subId, members] of collapsedMembers) {
    void members
    const nOut = subById.get(subId)?.interface?.outputs.length ?? 0
    g.setNode(subId, { width: SUB_W, height: subHeight(nOut) })
  }

  // Individual element nodes; expanded-submodel interiors get clustered under their sub.
  for (const e of individualElems) {
    g.setNode(e.id, { width: NODE_W, height: NODE_H })
  }
  const expandedFrameIds: string[] = []
  for (const subId of expanded) {
    if (!submodelIds.has(subId)) continue
    const interior = individualElems.filter((e) => submodelOf(e.container) === subId)
    if (interior.length === 0) continue
    g.setNode(subId, {}) // cluster parent
    for (const e of interior) g.setParent(e.id, subId)
    expandedFrameIds.push(subId)
  }

  // Edges must never touch an *expanded* submodel id — that id is a dagre cluster parent,
  // and dagre rejects edges to/from cluster nodes. So when a submodel is expanded, route its
  // boundary edges to/from its interior element nodes (a real leaf node), else skip.
  const isCluster = (id: string) => submodelIds.has(id) && expanded.has(id)
  // A submodel's first interface output element (used as the "out" endpoint when expanded).
  const outEndpoint = (subId: string): string | undefined => {
    const out = subById.get(subId)?.interface?.outputs.find((o) => knownIds.has(o))
    return out ? nodeOf.get(out) : undefined
  }

  const addedEdges = new Set<string>()
  const addEdge = (from: string | undefined, to: string | undefined) => {
    if (!from || !to || from === to || isCluster(from) || isCluster(to)) return
    const key = `${from}→${to}`
    if (addedEdges.has(key)) return
    addedEdges.add(key)
    g.setEdge(from, to)
  }
  // ── Edges from element.inputs ──
  for (const e of elements) {
    const toNode = nodeOf.get(e.id)
    if (!toNode) continue
    for (const src of e.inputs) {
      let fromNode: string | undefined
      if (submodelIds.has(src)) {
        // A submodel_stat consumer lists the submodel *container* id in its inputs. Collapsed:
        // the box id is a real node. Expanded: use the submodel's output element instead.
        fromNode = expanded.has(src) ? outEndpoint(src) : src
      } else if (knownIds.has(src)) {
        fromNode = nodeOf.get(src)
      }
      addEdge(fromNode, toNode)
    }
  }
  // ── Synthesized IN edges: interface.inputs[].from (parent driver) → submodel/consumer ──
  for (const subId of submodelIds) {
    const iface = subById.get(subId)?.interface
    if (!iface) continue
    for (const b of iface.inputs) {
      if (!b.from || !knownIds.has(b.from)) continue
      const fromNode = nodeOf.get(b.from) ?? b.from
      // collapsed → into the box; expanded → into the interior consumer node (skip if the
      // boundary port has no distinct interior element, to avoid an edge to the cluster).
      const toNode = expanded.has(subId) ? nodeOf.get(b.input) : subId
      addEdge(fromNode, toNode)
    }
  }

  dagre.layout(g)
  const gl = g.graph()

  // ── Read back: const groups ──
  const constGroups: LayoutConstGroup[] = []
  if (ungroupedConstants.length > 0) {
    const n = g.node(CGROUP_ID)
    const h = CGROUP_HEADER_H + ungroupedConstants.length * CGROUP_MEMBER_H + CGROUP_PAD_B
    constGroups.push({
      id: CGROUP_ID,
      members: ungroupedConstants.map((e) => ({ id: e.id, name: e.name })),
      x: n?.x ?? 0, y: n?.y ?? 0, w: CGROUP_W, h,
    })
  }

  // ── Read back: collapsed submodel boxes ──
  const submodels: LayoutSubmodel[] = []
  for (const [subId] of collapsedMembers) {
    const n = g.node(subId)
    const c = subById.get(subId)
    const outs = c?.interface?.outputs ?? []
    submodels.push({
      id: subId,
      name: c?.name ?? leaf(subId),
      realizations: c?.simulation_settings?.n_realizations ?? null,
      outputs: outs.slice(0, SUB_MAX_OUTPUTS).map(leaf),
      extraOutputs: Math.max(0, outs.length - SUB_MAX_OUTPUTS),
      x: n?.x ?? 0, y: n?.y ?? 0, w: SUB_W, h: subHeight(outs.length),
    })
  }

  // ── Read back: element nodes ──
  const layoutElements: LayoutElement[] = individualElems.map((e) => {
    const n = g.node(e.id)
    return {
      id: e.id, name: e.name, type: e.type,
      unit: unitMap[e.id] ?? '1',
      description: descMap[e.id] ?? null,
      x: n?.x ?? 0, y: n?.y ?? 0, w: NODE_W, h: NODE_H,
    }
  })

  // ── Read back: expanded submodel frames (bbox of interior nodes) ──
  const subFrames: LayoutSubFrame[] = []
  for (const subId of expandedFrameIds) {
    const interior = layoutElements.filter((e) => submodelOf(elements.find((x) => x.id === e.id)?.container) === subId)
    if (interior.length === 0) continue
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity
    for (const e of interior) {
      minX = Math.min(minX, e.x - e.w / 2); maxX = Math.max(maxX, e.x + e.w / 2)
      minY = Math.min(minY, e.y - e.h / 2); maxY = Math.max(maxY, e.y + e.h / 2)
    }
    const c = subById.get(subId)
    subFrames.push({
      id: subId, name: c?.name ?? leaf(subId),
      realizations: c?.simulation_settings?.n_realizations ?? null,
      x: minX - SUB_FRAME_PAD,
      y: minY - SUB_FRAME_PAD - SUB_FRAME_HEADER_H,
      w: (maxX - minX) + SUB_FRAME_PAD * 2,
      h: (maxY - minY) + SUB_FRAME_PAD * 2 + SUB_FRAME_HEADER_H,
    })
  }

  const layoutEdges: LayoutEdge[] = g.edges().map((ev) => ({
    from: ev.v, to: ev.w,
    points: (g.edge(ev)?.points ?? []) as Array<{ x: number; y: number }>,
  }))

  return {
    elements: layoutElements,
    constGroups,
    submodels,
    subFrames,
    edges: layoutEdges,
    width: gl.width ?? 800,
    height: gl.height ?? 600,
  }
}

// ── Edge path (smooth quadratic bezier through waypoints) ─────────────────────

function edgePath(pts: Array<{ x: number; y: number }>): string {
  if (pts.length < 2) return ''
  const [first, ...rest] = pts
  const last = rest[rest.length - 1]

  if (pts.length === 2) {
    return `M${first.x.toFixed(1)},${first.y.toFixed(1)} L${last.x.toFixed(1)},${last.y.toFixed(1)}`
  }

  let d = `M${first.x.toFixed(1)},${first.y.toFixed(1)}`
  // Quadratic bezier through each interior waypoint, landing on midpoints
  for (let i = 0; i < rest.length - 1; i++) {
    const cp = rest[i]
    const end = { x: (rest[i].x + rest[i + 1].x) / 2, y: (rest[i].y + rest[i + 1].y) / 2 }
    d += ` Q${cp.x.toFixed(1)},${cp.y.toFixed(1)} ${end.x.toFixed(1)},${end.y.toFixed(1)}`
  }
  d += ` L${last.x.toFixed(1)},${last.y.toFixed(1)}`
  return d
}

function trunc(s: string, n: number) { return s.length > n ? s.slice(0, n - 1) + '…' : s }

// ── Graph tab ─────────────────────────────────────────────────────────────────

export function GraphTab() {
  const modelSummary = useStore((s) => s.modelSummary)

  const unitMap = useMemo(() =>
    Object.fromEntries((modelSummary?.elements ?? []).map((e) => [e.id, unitLabel(e)])),
  [modelSummary])

  const descMap = useMemo(() =>
    Object.fromEntries((modelSummary?.elements ?? []).map((e) => [e.id, e.description])),
  [modelSummary])

  // Expanded submodel ids (click a submodel box to drill in / collapse).
  const [expanded, setExpanded] = useState<Set<string>>(new Set())
  const toggleSub = useCallback((id: string) => {
    setExpanded((prev) => {
      const next = new Set(prev)
      next.has(id) ? next.delete(id) : next.add(id)
      return next
    })
  }, [])

  const layout = useMemo(() =>
    modelSummary
      ? buildLayout(modelSummary.elements, modelSummary.containers ?? [], expanded, unitMap, descMap)
      : null,
  [modelSummary, expanded, unitMap, descMap])

  // ── Pan / zoom ──────────────────────────────────────────────────────────────
  const [tx, setTx] = useState(0)
  const [ty, setTy] = useState(0)
  const [scale, setScale] = useState(1)
  const svgRef = useRef<SVGSVGElement>(null)
  const drag   = useRef<{ sx: number; sy: number; ox: number; oy: number } | null>(null)

  // ── Tooltip ─────────────────────────────────────────────────────────────────
  const [tooltip, setTooltip] = useState<{ text: string; x: number; y: number } | null>(null)

  useEffect(() => {
    if (!layout || !svgRef.current) return
    const { width: lw, height: lh } = layout
    const { clientWidth: sw, clientHeight: sh } = svgRef.current
    const s = Math.min(0.9 * sw / lw, 0.9 * sh / lh, 1.2)
    setScale(s)
    setTx((sw - lw * s) / 2)
    setTy((sh - lh * s) / 2)
  }, [layout])

  const onWheel = useCallback((e: React.WheelEvent) => {
    e.preventDefault()
    setScale((s) => Math.max(0.12, Math.min(4, s * Math.exp(-e.deltaY * 0.001))))
  }, [])

  const onMouseDown = useCallback((e: React.MouseEvent) => {
    if (e.button !== 0) return
    drag.current = { sx: e.clientX, sy: e.clientY, ox: tx, oy: ty }
    setTooltip(null)
  }, [tx, ty])

  const onMouseMove = useCallback((e: React.MouseEvent) => {
    if (!drag.current) return
    setTx(drag.current.ox + e.clientX - drag.current.sx)
    setTy(drag.current.oy + e.clientY - drag.current.sy)
  }, [])

  const onMouseUp = useCallback(() => { drag.current = null }, [])

  const fitView = () => {
    if (!layout || !svgRef.current) return
    const { width: lw, height: lh } = layout
    const { clientWidth: sw, clientHeight: sh } = svgRef.current
    const s = Math.min(0.9 * sw / lw, 0.9 * sh / lh, 1.2)
    setScale(s); setTx((sw - lw * s) / 2); setTy((sh - lh * s) / 2)
  }

  if (!modelSummary || !layout) {
    return <p className="py-12 text-center text-sm text-slate-400">No model loaded.</p>
  }

  return (
    <div className="relative overflow-hidden rounded-lg border border-slate-200 bg-slate-50"
      style={{ height: 'calc(100vh - 192px)' }}>

      <button onClick={fitView}
        className="absolute right-3 top-3 z-10 rounded border border-slate-200 bg-white px-2 py-1 text-xs text-slate-500 shadow-sm hover:bg-slate-50">
        Fit
      </button>

      {/* Node description tooltip */}
      {tooltip && (
        <div
          className="pointer-events-none absolute z-20 max-w-xs rounded-lg border border-slate-200 bg-white px-3 py-2 text-xs text-slate-600 shadow-lg"
          style={{ left: tooltip.x + 12, top: tooltip.y + 12 }}
        >
          {tooltip.text}
        </div>
      )}

      <svg
        ref={svgRef}
        className="h-full w-full cursor-grab select-none active:cursor-grabbing"
        onWheel={onWheel}
        onMouseDown={onMouseDown}
        onMouseMove={onMouseMove}
        onMouseUp={onMouseUp}
        onMouseLeave={onMouseUp}
      >
        <defs>
          <marker id="wm-arrow" markerWidth="7" markerHeight="5"
            refX="6" refY="2.5" orient="auto">
            <path d="M0,0 L7,2.5 L0,5 Z" fill="#cbd5e1" />
          </marker>
        </defs>

        <g transform={`translate(${tx},${ty}) scale(${scale})`}>

          {/* ── Expanded submodel frames (behind everything) ────────────── */}
          {layout.subFrames.map((f) => {
            const color = TYPE_STROKE.submodel
            const bg    = TYPE_BG.submodel
            return (
              <g key={`frame-${f.id}`} transform={`translate(${f.x},${f.y})`}
                onClick={(e) => { e.stopPropagation(); toggleSub(f.id) }}
                style={{ cursor: 'pointer' }}>
                <rect width={f.w} height={f.h} rx="10" fill={bg} fillOpacity="0.5"
                  stroke={color} strokeWidth="1.5" strokeDasharray="6 4" />
                <g transform={`translate(10,${(SUB_FRAME_HEADER_H - 16) / 2}) scale(0.8)`} style={{ color }}>
                  <TypeIcon type="submodel" />
                </g>
                <text x="32" y={SUB_FRAME_HEADER_H / 2 + 4}
                  fontSize="12" fontWeight="700" fill={color}
                  fontFamily="ui-sans-serif,system-ui,sans-serif">
                  {trunc(f.name, 28)}{f.realizations != null ? `  ·  ${f.realizations} realizations` : ''}
                </text>
                <text x={f.w - 14} y={SUB_FRAME_HEADER_H / 2 + 5} textAnchor="end"
                  fontSize="13" fill={color}>▾</text>
              </g>
            )
          })}

          {/* ── Edges ──────────────────────────────────────────────────── */}
          {layout.edges.map((edge) => (
            <path
              key={`${edge.from}→${edge.to}`}
              d={edgePath(edge.points)}
              fill="none"
              stroke="#cbd5e1"
              strokeWidth="1.5"
              markerEnd="url(#wm-arrow)"
            />
          ))}

          {/* ── Constant group nodes ────────────────────────────────────── */}
          {layout.constGroups.map((cg) => {
            const x = cg.x - cg.w / 2
            const y = cg.y - cg.h / 2
            const color = TYPE_STROKE.constant
            const bg    = TYPE_BG.constant
            return (
              <g key={cg.id} transform={`translate(${x},${y})`}>
                {/* Drop shadow */}
                <rect x="1" y="2" width={cg.w} height={cg.h} rx="7" fill="rgba(0,0,0,0.06)" />
                {/* Card */}
                <rect width={cg.w} height={cg.h} rx="7" fill="white"
                  stroke={color} strokeWidth="1.5" strokeDasharray="5 3" />
                {/* Header band */}
                <rect width={cg.w} height={CGROUP_HEADER_H} rx="7" fill={bg} />
                <rect y={CGROUP_HEADER_H - 6} width={cg.w} height="6" fill={bg} />
                <line x1="0" y1={CGROUP_HEADER_H} x2={cg.w} y2={CGROUP_HEADER_H}
                  stroke={color} strokeWidth="0.75" strokeOpacity="0.5" />
                {/* Header icon + label */}
                <g transform={`translate(8,${(CGROUP_HEADER_H - 16) / 2}) scale(0.8)`} style={{ color }}>
                  <TypeIcon type="constant" />
                </g>
                <text x="30" y={CGROUP_HEADER_H / 2 + 4}
                  fontSize="11" fontWeight="700" fill={color}
                  fontFamily="ui-sans-serif,system-ui,sans-serif">
                  Constants
                </text>
                {/* Member list */}
                {cg.members.map((m, i) => (
                  <text key={m.id}
                    x="12"
                    y={CGROUP_HEADER_H + CGROUP_MEMBER_H * i + CGROUP_MEMBER_H - 5}
                    fontSize="10" fill="#475569"
                    fontFamily="ui-sans-serif,system-ui,sans-serif">
                    {trunc(m.name, 20)}
                  </text>
                ))}
              </g>
            )
          })}

          {/* ── Collapsed submodel boxes ────────────────────────────────── */}
          {layout.submodels.map((sm) => {
            const x = sm.x - sm.w / 2
            const y = sm.y - sm.h / 2
            const color = TYPE_STROKE.submodel
            const bg    = TYPE_BG.submodel
            let line = 0
            return (
              <g key={sm.id} transform={`translate(${x},${y})`}
                onClick={(e) => { e.stopPropagation(); toggleSub(sm.id) }}
                style={{ cursor: 'pointer' }}>
                {/* Drop shadow */}
                <rect x="1" y="2" width={sm.w} height={sm.h} rx="8" fill="rgba(0,0,0,0.07)" />
                {/* Card */}
                <rect width={sm.w} height={sm.h} rx="8" fill="white"
                  stroke={color} strokeWidth="1.75" strokeDasharray="6 4" />
                {/* Header band */}
                <rect width={sm.w} height={SUB_HEADER_H} rx="8" fill={bg} />
                <rect y={SUB_HEADER_H - 8} width={sm.w} height="8" fill={bg} />
                <line x1="0" y1={SUB_HEADER_H} x2={sm.w} y2={SUB_HEADER_H}
                  stroke={color} strokeWidth="0.75" strokeOpacity="0.5" />
                <g transform={`translate(9,${(SUB_HEADER_H - 16) / 2}) scale(0.8)`} style={{ color }}>
                  <TypeIcon type="submodel" />
                </g>
                <text x="32" y={SUB_HEADER_H / 2 + 4}
                  fontSize="11.5" fontWeight="700" fill={color}
                  fontFamily="ui-sans-serif,system-ui,sans-serif">
                  {trunc(sm.name, 20)}
                </text>
                <text x={sm.w - 12} y={SUB_HEADER_H / 2 + 5} textAnchor="end"
                  fontSize="13" fill={color}>▸</text>
                {/* Body: realizations + interface outputs */}
                <text x="12" y={SUB_HEADER_H + SUB_LINE_H * (++line) - 5}
                  fontSize="10" fill="#0369a1" fontWeight="600"
                  fontFamily="ui-sans-serif,system-ui,sans-serif">
                  ⟳ {sm.realizations != null ? `${sm.realizations} realizations` : 'nested run'}
                </text>
                {sm.outputs.map((o) => (
                  <text key={o} x="14" y={SUB_HEADER_H + SUB_LINE_H * (++line) - 5}
                    fontSize="10" fill="#475569"
                    fontFamily="ui-sans-serif,system-ui,sans-serif">
                    → {trunc(o, 22)}
                  </text>
                ))}
                {sm.extraOutputs > 0 && (
                  <text x="14" y={SUB_HEADER_H + SUB_LINE_H * (++line) - 5}
                    fontSize="10" fill="#94a3b8" fontStyle="italic"
                    fontFamily="ui-sans-serif,system-ui,sans-serif">
                    +{sm.extraOutputs} more
                  </text>
                )}
              </g>
            )
          })}

          {/* ── Element nodes ───────────────────────────────────────────── */}
          {layout.elements.map((node) => {
            const color = TYPE_STROKE[node.type] ?? '#94a3b8'
            const bg    = TYPE_BG[node.type]     ?? '#f8fafc'
            const unit  = node.unit !== '1' ? node.unit : null
            const x = node.x - NODE_W / 2
            const y = node.y - NODE_H / 2

            return (
              <g key={node.id} transform={`translate(${x},${y})`}
                onMouseEnter={node.description ? (e) => setTooltip({ text: node.description!, x: e.clientX, y: e.clientY }) : undefined}
                onMouseMove={node.description ? (e) => setTooltip((t) => t ? { ...t, x: e.clientX, y: e.clientY } : null) : undefined}
                onMouseLeave={node.description ? () => setTooltip(null) : undefined}
              >

                {/* Drop shadow */}
                <rect x="1" y="2" width={NODE_W} height={NODE_H} rx="7" fill="rgba(0,0,0,0.07)" />

                {/* Card background */}
                <rect width={NODE_W} height={NODE_H} rx="7" fill="white"
                  stroke={color} strokeWidth="1.5" />

                {/* Left stripe */}
                <rect width="38" height={NODE_H} rx="7" fill={bg} />
                <rect x="31" width="7" height={NODE_H} fill={bg} />
                <line x1="38" y1="2" x2="38" y2={NODE_H - 2}
                  stroke={color} strokeWidth="0.75" strokeOpacity="0.4" />

                {/* Icon */}
                <g transform={`translate(9,${(NODE_H - 20) / 2})`} style={{ color }}>
                  <TypeIcon type={node.type} />
                </g>

                {/* Name */}
                <text x="46" y={unit ? 19 : 30}
                  fontSize="12" fontWeight="600" fill="#1e293b"
                  fontFamily="ui-sans-serif,system-ui,sans-serif">
                  {trunc(node.name, 16)}
                </text>

                {/* Type · unit */}
                <text x="46" y={unit ? 35 : 46}
                  fontSize="10" fill="#94a3b8"
                  fontFamily="ui-sans-serif,system-ui,sans-serif">
                  {node.type}{unit ? ` · ${unit}` : ''}
                </text>
              </g>
            )
          })}
        </g>
      </svg>
    </div>
  )
}
