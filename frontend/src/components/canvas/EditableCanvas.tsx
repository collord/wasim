import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import dagre from '@dagrejs/dagre'
import { useStore, useElements, usePositions, useActiveLens } from '../../store'
import type { ElementSummary } from '../../types'
import type { NodeShape } from '../../lenses/types'
import { iconTypeOf, TYPE_BG, TYPE_STROKE, TypeIcon } from '../../ui/typeIcons'
import { PALETTE, slugify } from '../../model/edits'
import { unitLabel } from '../../display'

const NODE_W = 168
const NODE_H = 52

interface Pos { x: number; y: number }

/** Compute a Dagre layout for elements lacking a stored position (seeds free placement). */
function autoLayout(elements: ElementSummary[]): Record<string, Pos> {
  const g = new dagre.graphlib.Graph()
  g.setGraph({ rankdir: 'LR', nodesep: 30, ranksep: 90, marginx: 40, marginy: 40 })
  g.setDefaultEdgeLabel(() => ({}))
  const ids = new Set(elements.map((e) => e.id))
  for (const e of elements) g.setNode(e.id, { width: NODE_W, height: NODE_H })
  for (const e of elements) for (const src of e.inputs) if (ids.has(src) && src !== e.id) g.setEdge(src, e.id)
  dagre.layout(g)
  const out: Record<string, Pos> = {}
  for (const e of elements) { const n = g.node(e.id); if (n) out[e.id] = { x: n.x, y: n.y } }
  return out
}

export function EditableCanvas() {
  const elements = useElements()
  const positions = usePositions()
  const selectedIds = useStore((s) => s.selectedIds)
  const select = useStore((s) => s.select)
  const moveNode = useStore((s) => s.moveNode)
  const tidyPositions = useStore((s) => s.tidyPositions)
  const removeElement = useStore((s) => s.removeElement)
  const addNewElement = useStore((s) => s.addNewElement)
  const duplicate = useStore((s) => s.duplicateElement)
  const format = useStore((s) => s.format)
  const lens = useActiveLens()
  const loadTemplate = useStore((s) => s.loadTemplate)
  const docEls = useStore((s) => s.doc?.elements)

  // Map each element to its lens glyph shape (via lens_role on the canonical doc; the engine
  // summary doesn't carry the engine-ignored tag). Stock → box, auxiliary → circle, etc.
  const shapeOf = useMemo(() => {
    const roles = new Map((docEls ?? []).map((e) => [e.id, e.lens_role]))
    return (id: string): NodeShape => lens.glyphOf?.(roles.get(id)) ?? 'default'
  }, [docEls, lens])

  const svgRef = useRef<SVGSVGElement>(null)
  const [view, setView] = useState({ tx: 0, ty: 0, scale: 1 })
  const pan = useRef<{ sx: number; sy: number; ox: number; oy: number } | null>(null)
  const nodeDrag = useRef<{ id: string; sx: number; sy: number; ox: number; oy: number; moved: boolean } | null>(null)

  // Auto-layout seeds any element without a stored position.
  const auto = useMemo(() => autoLayout(elements), [elements])
  const posOf = useCallback((id: string): Pos => positions[id] ?? auto[id] ?? { x: 100, y: 100 }, [positions, auto])

  const bounds = useMemo(() => {
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity
    for (const e of elements) {
      const p = posOf(e.id)
      minX = Math.min(minX, p.x - NODE_W / 2); maxX = Math.max(maxX, p.x + NODE_W / 2)
      minY = Math.min(minY, p.y - NODE_H / 2); maxY = Math.max(maxY, p.y + NODE_H / 2)
    }
    if (!isFinite(minX)) return { minX: 0, minY: 0, w: 800, h: 600 }
    return { minX, minY, w: maxX - minX, h: maxY - minY }
  }, [elements, posOf])

  const fit = useCallback(() => {
    if (!svgRef.current) return
    const { clientWidth: sw, clientHeight: sh } = svgRef.current
    const s = Math.min(0.9 * sw / (bounds.w || 1), 0.9 * sh / (bounds.h || 1), 1.4)
    setView({ scale: s, tx: sw / 2 - (bounds.minX + bounds.w / 2) * s, ty: sh / 2 - (bounds.minY + bounds.h / 2) * s })
  }, [bounds])

  useEffect(() => { fit() /* on first layout */ }, [elements.length]) // eslint-disable-line react-hooks/exhaustive-deps

  // Screen → world coordinate.
  const toWorld = (clientX: number, clientY: number): Pos => {
    const rect = svgRef.current!.getBoundingClientRect()
    return { x: (clientX - rect.left - view.tx) / view.scale, y: (clientY - rect.top - view.ty) / view.scale }
  }

  const onWheel = (e: React.WheelEvent) => {
    e.preventDefault()
    const factor = Math.exp(-e.deltaY * 0.0012)
    setView((v) => {
      const rect = svgRef.current!.getBoundingClientRect()
      const cx = e.clientX - rect.left, cy = e.clientY - rect.top
      const ns = Math.max(0.12, Math.min(4, v.scale * factor))
      return { scale: ns, tx: cx - (cx - v.tx) * (ns / v.scale), ty: cy - (cy - v.ty) * (ns / v.scale) }
    })
  }

  const onBgMouseDown = (e: React.MouseEvent) => {
    if (e.button !== 0) return
    pan.current = { sx: e.clientX, sy: e.clientY, ox: view.tx, oy: view.ty }
    select(null)
  }

  const onMouseMove = (e: React.MouseEvent) => {
    if (nodeDrag.current) {
      const w = toWorld(e.clientX, e.clientY)
      const nd = nodeDrag.current
      nd.moved = true
      const nx = nd.ox + (w.x - nd.sx), ny = nd.oy + (w.y - nd.sy)
      moveNode(nd.id, { x: nx, y: ny })
      return
    }
    if (pan.current) setView((v) => ({ ...v, tx: pan.current!.ox + e.clientX - pan.current!.sx, ty: pan.current!.oy + e.clientY - pan.current!.sy }))
  }

  const endDrag = () => { pan.current = null; nodeDrag.current = null }

  const onNodeMouseDown = (e: React.MouseEvent, id: string) => {
    e.stopPropagation()
    if (e.button !== 0) return
    const w = toWorld(e.clientX, e.clientY)
    const p = posOf(id)
    nodeDrag.current = { id, sx: w.x, sy: w.y, ox: p.x, oy: p.y, moved: false }
    if (!selectedIds.includes(id)) select(id, e.metaKey || e.ctrlKey)
  }

  const onNodeClick = (e: React.MouseEvent, id: string) => {
    e.stopPropagation()
    if (!nodeDrag.current?.moved) select(id, e.metaKey || e.ctrlKey)
  }

  const onKeyDown = (e: React.KeyboardEvent) => {
    if ((e.key === 'Delete' || e.key === 'Backspace') && selectedIds.length) {
      e.preventDefault()
      selectedIds.forEach((id) => removeElement(id))
    } else if (e.key.toLowerCase() === 'd' && (e.metaKey || e.ctrlKey) && selectedIds.length === 1) {
      e.preventDefault()
      duplicate(selectedIds[0])
    }
  }

  // Palette drag-and-drop insert.
  const onDrop = (e: React.DragEvent) => {
    e.preventDefault()
    const key = e.dataTransfer.getData('application/wasim-palette')
    const entry = PALETTE.find((p) => p.key === key)
    if (!entry) return
    const w = toWorld(e.clientX, e.clientY)
    const el = entry.make(slugify(entry.label), entry.label, format)
    addNewElement(el, w)
  }

  const edges = useMemo(() => {
    const ids = new Set(elements.map((e) => e.id))
    const out: { from: string; to: string }[] = []
    for (const e of elements) for (const src of e.inputs) if (ids.has(src) && src !== e.id) out.push({ from: src, to: e.id })
    return out
  }, [elements])

  return (
    <div className="relative h-full w-full overflow-hidden bg-slate-50" onDragOver={(e) => e.preventDefault()} onDrop={onDrop}>
      <div className="absolute right-3 top-3 z-10 flex gap-1">
        <button onClick={() => tidyPositions(autoLayout(elements))}
          className="rounded border border-slate-200 bg-white px-2 py-1 text-[11px] text-slate-500 shadow-sm hover:bg-slate-50">Tidy layout</button>
        <button onClick={fit} className="rounded border border-slate-200 bg-white px-2 py-1 text-[11px] text-slate-500 shadow-sm hover:bg-slate-50">Fit</button>
      </div>

      <svg
        ref={svgRef}
        tabIndex={0}
        className="h-full w-full cursor-grab select-none outline-none active:cursor-grabbing"
        onWheel={onWheel}
        onMouseDown={onBgMouseDown}
        onMouseMove={onMouseMove}
        onMouseUp={endDrag}
        onMouseLeave={endDrag}
        onKeyDown={onKeyDown}
      >
        <defs>
          <marker id="ec-arrow" markerWidth="7" markerHeight="5" refX="6" refY="2.5" orient="auto">
            <path d="M0,0 L7,2.5 L0,5 Z" fill="#cbd5e1" />
          </marker>
        </defs>
        <g transform={`translate(${view.tx},${view.ty}) scale(${view.scale})`}>
          {/* Influence edges (thin dashed grey — a projection of the dependency graph, §2.2) */}
          {edges.map((ed, i) => {
            const a = posOf(ed.from), b = posOf(ed.to)
            return <line key={i} x1={a.x} y1={a.y} x2={b.x} y2={b.y} stroke="#cbd5e1" strokeWidth="1.4"
              strokeDasharray="4 3" markerEnd="url(#ec-arrow)" />
          })}

          {elements.map((e) => {
            const p = posOf(e.id)
            const t = iconTypeOf(e)
            const color = TYPE_STROKE[t] ?? '#94a3b8'
            const bg = TYPE_BG[t] ?? '#f8fafc'
            const sel = selectedIds.includes(e.id)
            const unit = unitLabel(e)
            const x = p.x - NODE_W / 2, y = p.y - NODE_H / 2
            // Forrester glyph per lens role: a stock is a squared, filled box (an accumulator); an
            // auxiliary is a pill (circle-like). Others render as the default rounded node.
            const shape = shapeOf(e.id)
            const rx = shape === 'box' ? 3 : shape === 'circle' ? NODE_H / 2 : 7
            const nodeStroke = sel ? 2.5 : shape === 'box' ? 2.25 : 1.5
            const nodeFill = shape === 'box' ? '#eff6ff' : 'white'
            return (
              <g key={e.id} data-shape={shape} transform={`translate(${x},${y})`}
                onMouseDown={(ev) => onNodeMouseDown(ev, e.id)} onClick={(ev) => onNodeClick(ev, e.id)}
                style={{ cursor: 'pointer' }}>
                <rect x="1" y="2" width={NODE_W} height={NODE_H} rx={rx} fill="rgba(0,0,0,0.06)" />
                <rect width={NODE_W} height={NODE_H} rx={rx} fill={nodeFill}
                  stroke={sel ? '#2563eb' : color} strokeWidth={nodeStroke} />
                <rect width="36" height={NODE_H} rx={rx} fill={bg} />
                <rect x="29" width="7" height={NODE_H} fill={bg} />
                <line x1="36" y1="2" x2="36" y2={NODE_H - 2} stroke={color} strokeWidth="0.75" strokeOpacity="0.4" />
                <g transform={`translate(8,${(NODE_H - 20) / 2})`} style={{ color }}><TypeIcon type={t} /></g>
                <text x="44" y={unit && unit !== '1' ? 22 : 30} fontSize="12" fontWeight="600" fill="#1e293b"
                  fontFamily="ui-sans-serif,system-ui,sans-serif">{trunc(e.name, 16)}</text>
                <text x="44" y={unit && unit !== '1' ? 36 : 44} fontSize="10" fill="#94a3b8"
                  fontFamily="ui-sans-serif,system-ui,sans-serif">
                  {(e.value_rule ?? e.primitive ?? e.type)}{unit && unit !== '1' ? ` · ${unit}` : ''}
                </text>
              </g>
            )
          })}
        </g>
      </svg>

      {elements.length === 0 && (
        <div className="absolute inset-0 flex items-center justify-center p-6">
          {lens.templates && lens.templates.length > 0 ? (
            <div className="w-full max-w-sm text-center">
              <p className="mb-3 text-sm text-slate-500">
                Start from a {lens.label} template, or drag from the Palette.
              </p>
              <div className="flex flex-col gap-2">
                {lens.templates.map((t) => (
                  <button
                    key={t.id}
                    onClick={() => loadTemplate(t.build(), `${t.id}.json`)}
                    className="rounded-lg border border-slate-200 bg-white px-4 py-2.5 text-left shadow-sm hover:border-blue-300 hover:bg-blue-50"
                  >
                    <div className="text-sm font-semibold text-slate-700">{t.label}</div>
                    <div className="text-xs text-slate-400">{t.description}</div>
                  </button>
                ))}
              </div>
            </div>
          ) : (
            <div className="pointer-events-none text-center text-sm text-slate-400">
              Drag elements from the Palette, or double-click a palette entry, to start building.
            </div>
          )}
        </div>
      )}
    </div>
  )
}

function trunc(s: string, n: number) { return s.length > n ? s.slice(0, n - 1) + '…' : s }
