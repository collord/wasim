import { useRef, useState, useCallback, useMemo, useEffect } from 'react'
import dagre from '@dagrejs/dagre'
import { useStore } from '../../store'
import type { ModelElement } from '../../types'

// ── Layout constants ───────────────────────────────────────────────────────────

const NODE_W = 172
const NODE_H = 54
const CGROUP_W = 160
const CGROUP_HEADER_H = 28
const CGROUP_MEMBER_H = 19
const CGROUP_PAD_B = 8

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

interface LayoutEdge {
  from: string; to: string
  points: Array<{ x: number; y: number }>
}

interface Layout {
  elements: LayoutElement[]
  constGroups: LayoutConstGroup[]
  edges: LayoutEdge[]
  width: number; height: number
}

// ── Dagre layout ──────────────────────────────────────────────────────────────

function buildLayout(elements: ModelElement[], unitMap: Record<string, string>, descMap: Record<string, string | null>): Layout {
  const knownIds = new Set(elements.map((e) => e.id))

  // Partition: constants with no explicit container → const_group
  // Everything else → individual element node
  const ungroupedConstants: ModelElement[] = []
  const individualElems: ModelElement[] = []

  for (const e of elements) {
    const container = (((e as unknown) as Record<string, unknown>).container as string | null) ?? null
    if (e.type === 'constant' && !container) {
      ungroupedConstants.push(e)
    } else {
      individualElems.push(e)
    }
  }

  // id → graph node id (for edge mapping)
  const nodeOf = new Map<string, string>()
  const CGROUP_ID = '~constants'

  for (const e of ungroupedConstants) nodeOf.set(e.id, CGROUP_ID)
  for (const e of individualElems)    nodeOf.set(e.id, e.id)

  // Build dagre graph
  const g = new dagre.graphlib.Graph({ multigraph: false })
  g.setGraph({ rankdir: 'LR', nodesep: 32, ranksep: 80, marginx: 48, marginy: 48 })
  g.setDefaultEdgeLabel(() => ({}))

  // Add constant group node
  if (ungroupedConstants.length > 0) {
    const h = CGROUP_HEADER_H + ungroupedConstants.length * CGROUP_MEMBER_H + CGROUP_PAD_B
    g.setNode(CGROUP_ID, { width: CGROUP_W, height: h })
  }

  // Add individual element nodes
  for (const e of individualElems) {
    g.setNode(e.id, { width: NODE_W, height: NODE_H })
  }

  // Add edges (mapped to graph node IDs, deduped, no self-edges)
  const addedEdges = new Set<string>()
  for (const e of elements) {
    const toNode = nodeOf.get(e.id)
    if (!toNode) continue
    const inputs = (((e as unknown) as Record<string, unknown>).inputs as string[]) ?? []
    for (const src of inputs) {
      if (!knownIds.has(src)) continue
      const fromNode = nodeOf.get(src)
      if (!fromNode || fromNode === toNode) continue
      const key = `${fromNode}→${toNode}`
      if (addedEdges.has(key)) continue
      addedEdges.add(key)
      g.setEdge(fromNode, toNode)
    }
  }

  dagre.layout(g)

  const gl = g.graph()

  // Collect const groups
  const constGroups: LayoutConstGroup[] = []
  if (ungroupedConstants.length > 0) {
    const n = g.node(CGROUP_ID)
    const h = CGROUP_HEADER_H + ungroupedConstants.length * CGROUP_MEMBER_H + CGROUP_PAD_B
    constGroups.push({
      id: CGROUP_ID,
      members: ungroupedConstants.map((e) => ({ id: e.id, name: e.name })),
      x: n?.x ?? 0, y: n?.y ?? 0,
      w: CGROUP_W, h,
    })
  }

  // Collect element nodes
  const layoutElements: LayoutElement[] = individualElems.map((e) => {
    const n = g.node(e.id)
    return {
      id: e.id, name: e.name, type: e.type,
      unit: unitMap[e.id] ?? '1',
      description: descMap[e.id] ?? null,
      x: n?.x ?? 0, y: n?.y ?? 0,
      w: NODE_W, h: NODE_H,
    }
  })

  // Collect edges
  const layoutEdges: LayoutEdge[] = g.edges().map((ev) => ({
    from: ev.v, to: ev.w,
    points: (g.edge(ev)?.points ?? []) as Array<{ x: number; y: number }>,
  }))

  return {
    elements: layoutElements,
    constGroups,
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
  const parsedModel  = useStore((s) => s.parsedModel)
  const modelSummary = useStore((s) => s.modelSummary)

  const unitMap = useMemo(() =>
    Object.fromEntries((modelSummary?.elements ?? []).map((e) => [e.id, e.unit])),
  [modelSummary])

  const descMap = useMemo(() =>
    Object.fromEntries((modelSummary?.elements ?? []).map((e) => [e.id, e.description])),
  [modelSummary])

  const layout = useMemo(() =>
    parsedModel ? buildLayout(parsedModel.elements, unitMap, descMap) : null,
  [parsedModel, unitMap, descMap])

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

  if (!parsedModel || !layout) {
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
