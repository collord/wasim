import { useEffect, useMemo, useState } from 'react'
import { useStore, useElements, useContainers, useActiveLens } from '../../store'
import type { ElementSummary } from '../../types'
import type { FlatElement } from '../../model/schema'
import { kindLabel } from '../../model/schema'
import { iconTypeOf, TypeBadge } from '../../ui/typeIcons'
import { Field, NumInput, Section, Select, TextInput, Toggle } from './fields'
import { ExpressionEditor } from './ExpressionEditor'
import { DISTRIBUTIONS, distDef, paramValue } from './dists'
import { printAst, type Ast } from '../../model/ast'
import { recomputeInputs } from '../../model/edits'

export function Inspector() {
  const selectedId = useStore((s) => s.selectedId)
  const summary = useStore((s) => s.modelSummary)
  const doc = useStore((s) => s.doc)
  const lens = useActiveLens()

  const el = useMemo(() => summary?.elements.find((e) => e.id === selectedId) ?? null, [summary, selectedId])
  const flat = useMemo(() => doc?.elements.find((e) => e.id === selectedId) ?? null, [doc, selectedId])

  if (!selectedId || !el || !flat) {
    return (
      <div className="flex h-full items-center justify-center p-6 text-center text-xs text-slate-400">
        Select an element to edit its properties.
      </div>
    )
  }

  return (
    <div className="flex h-full flex-col overflow-hidden">
      {/* Header */}
      <div className="flex items-center gap-2 border-b border-slate-200 bg-slate-50 px-3 py-2.5">
        <TypeBadge type={iconTypeOf(el)} />
        <div className="min-w-0 flex-1">
          <div className="truncate text-sm font-semibold text-slate-800">{el.name}</div>
          {/* Prefer the active lens's domain label for this element's role (e.g. a flow reads
              "Flow", not "Expression"); fall back to the raw engine kind. */}
          {flat.lens_role && lens.roleLabels?.[flat.lens_role] ? (
            <div data-testid="inspector-role" className="truncate text-[10px] font-medium uppercase tracking-wide text-slate-500">
              {lens.roleLabels[flat.lens_role]}
            </div>
          ) : (
            <div data-testid="inspector-role" className="truncate text-[10px] text-slate-400">{kindLabel(flat)}</div>
          )}
        </div>
      </div>

      <div className="flex-1 overflow-auto">
        <InfoSection el={el} flat={flat} />
        <DefinitionSection el={el} flat={flat} />
        <OutputSection el={el} flat={flat} />
        <SaveSection flat={flat} />
      </div>
    </div>
  )
}

// ── Info: id / name / description / container ────────────────────────────────────

function InfoSection({ el, flat }: { el: ElementSummary; flat: FlatElement }) {
  const rename = useStore((s) => s.renameElement)
  const update = useStore((s) => s.updateElementField)
  const reparent = useStore((s) => s.reparent)
  const containers = useContainers()

  return (
    <Section title="Info">
      <Field label="Name">
        <TextInput value={el.name} onChange={(name) => update(el.id, { name })} />
      </Field>
      <Field label="ID" hint="Unique, slug-like; references update automatically.">
        <TextInput value={el.id} mono onBlur={() => {}} onChange={(v) => rename(el.id, v)} />
      </Field>
      <Field label="Description">
        <TextInput value={el.description ?? ''} onChange={(description) => update(el.id, { description })} placeholder="Shown as the canvas tooltip" />
      </Field>
      <Field label="Container">
        <Select
          value={flat.container ?? ''}
          onChange={(c) => reparent(el.id, c === '' ? null : c)}
          options={[{ value: '', label: '— (root) —' }, ...containers.map((c) => ({ value: c.id, label: c.name }))]}
        />
      </Field>
    </Section>
  )
}

// ── Definition: per value-rule / primitive editor ────────────────────────────────

function DefinitionSection({ el, flat }: { el: ElementSummary; flat: FlatElement }) {
  const rule = el.value_rule
  const prim = el.primitive

  let body = <UnsupportedEditor el={el} flat={flat} />
  if (prim === 'stock') body = <StockEditor el={el} flat={flat} />
  else if (prim === 'event') body = <EventEditor el={el} flat={flat} />
  else if (prim === 'gate') body = <GateEditor el={el} flat={flat} />
  else if (rule === 'fixed') body = <FixedEditor el={el} flat={flat} />
  else if (rule === 'sample') body = <SampleEditor el={el} flat={flat} />
  else if (rule === 'expression') body = <ExpressionRuleEditor el={el} />
  else if (rule === 'lookup') body = <LookupEditor el={el} flat={flat} />
  else if (rule === 'series') body = <SeriesEditor flat={flat} />
  else if (rule === 'lag') body = <LagEditor el={el} flat={flat} />
  else if (rule === 'filter') body = <FilterEditor el={el} flat={flat} />

  return <Section title="Definition">{body}</Section>
}

// ── Gate (boolean logic over referenced element states) ──────────────────────────

interface GateRoot { op?: string; threshold?: number; children?: { op?: string; reference?: string }[] }

const GATE_OPS = [
  { value: 'and', label: 'All active (AND)' },
  { value: 'or', label: 'Any active (OR)' },
  { value: 'n_vote', label: 'At least k active (k-of-n)' },
]

/** Structured editor for the `gate` primitive: pick the logic (AND / OR / k-of-n) and which
 *  elements it references. A referenced element is "active" when its value > 0 (e.g. a failed
 *  component); the gate outputs 1 when the logic holds. Nested gate trees stay editable via the
 *  raw-JSON escape hatch. */
function GateEditor({ el, flat }: { el: ElementSummary; flat: FlatElement }) {
  const mutate = useStore((s) => s.mutateEl)
  const docEls = useStore((s) => s.doc?.elements)
  const root = (flat.root as GateRoot | undefined) ?? { op: 'n_vote', threshold: 1, children: [] }
  const op = root.op ?? 'n_vote'
  const childIds = (root.children ?? []).filter((c) => c?.op === 'reference' && c.reference).map((c) => c.reference as string)
  const candidates = (docEls ?? []).filter((e) => e.id !== el.id)

  const write = (nextOp: string, ids: string[], threshold: number) =>
    mutate(el.id, (e) => {
      const children = ids.map((id) => ({ op: 'reference', reference: id }))
      const r: GateRoot = { op: nextOp, children }
      if (nextOp === 'n_vote') r.threshold = Math.max(1, Math.min(threshold, ids.length || 1))
      e.root = r
      e.inputs = ids // so influence edges + topo see the referenced elements
    })

  const toggle = (id: string) =>
    write(op, childIds.includes(id) ? childIds.filter((x) => x !== id) : [...childIds, id], root.threshold ?? 1)

  return (
    <>
      <Field label="Logic" hint="A referenced element is active when its value > 0 (e.g. a failed component). Output is 1 when the logic holds.">
        <Select value={op} onChange={(o) => write(o, childIds, root.threshold ?? 1)} options={GATE_OPS} />
      </Field>
      {op === 'n_vote' && (
        <Field label="Minimum active (k)">
          <NumInput value={root.threshold ?? 1} onChange={(v) => write(op, childIds, Math.round(v))} />
        </Field>
      )}
      <Field label="Inputs" hint={childIds.length === 0 ? 'Select the elements this gate references.' : undefined}>
        {candidates.length === 0 ? (
          <p className="text-[11px] text-slate-400">No other elements to reference yet.</p>
        ) : (
          <div className="space-y-1">
            {candidates.map((c) => (
              <label key={c.id} className="flex items-center gap-2 rounded px-1 py-0.5 text-xs hover:bg-slate-50">
                <input type="checkbox" className="h-3.5 w-3.5" checked={childIds.includes(c.id)} onChange={() => toggle(c.id)} />
                <span className="flex-1 truncate text-slate-700">{c.name}</span>
              </label>
            ))}
          </div>
        )}
      </Field>
    </>
  )
}

// ── Fixed (constant) ──────────────────────────────────────────────────────────

function FixedEditor({ el, flat }: { el: ElementSummary; flat: FlatElement }) {
  const setConstant = useStore((s) => s.setConstant)
  const mutate = useStore((s) => s.mutateEl)
  const value = el.value ?? paramValue(flat.value)
  const editable = flat.editable ?? el.editable

  return (
    <>
      <Field label="Value" hint={editable ? undefined : 'Not marked editable — enable to expose on the dashboard.'}>
        <NumInput value={value} unit={el.unit} onChange={(v) => setConstant(el.id, v)} />
      </Field>
      <Toggle label="Editable (dashboard / optimization variable)" checked={!!editable}
        onChange={(editable) => mutate(el.id, (e) => { e.editable = editable })} />
      {editable && (
        <div className="grid grid-cols-2 gap-2">
          <Field label="Min bound">
            <NumInput value={flat.bounds?.min ?? 0} onChange={(min) => mutate(el.id, (e) => { e.bounds = { ...e.bounds, min } })} />
          </Field>
          <Field label="Max bound">
            <NumInput value={flat.bounds?.max ?? 1} onChange={(max) => mutate(el.id, (e) => { e.bounds = { ...e.bounds, max } })} />
          </Field>
        </div>
      )}
    </>
  )
}

// ── Sample (distribution) ───────────────────────────────────────────────────────

function SampleEditor({ el, flat }: { el: ElementSummary; flat: FlatElement }) {
  const setRvParam = useStore((s) => s.setRvParam)
  const mutate = useStore((s) => s.mutateEl)
  const dist = el.dist
  const family = dist?.family ?? 'normal'
  const def = distDef(family)

  const changeFamily = (fam: string) => {
    const d = distDef(fam)
    if (!d) return
    mutate(el.id, (e) => {
      e.distribution = { family: fam, parameters: d.defaults() } as FlatElement['distribution']
    })
  }

  return (
    <>
      <Field label="Distribution">
        <Select value={family} onChange={changeFamily}
          options={DISTRIBUTIONS.map((d) => ({ value: d.family, label: d.label, group: d.group }))} />
      </Field>
      {def ? (
        <div className="space-y-2">
          {def.params.map((p) => (
            <Field key={p} label={p}>
              <NumInput value={paramValue(dist?.parameters?.[p])} unit={el.unit}
                onChange={(v) => setRvParam(el.id, p, v)} />
            </Field>
          ))}
        </div>
      ) : (
        <p className="text-[11px] text-slate-400">Family “{family}” isn’t editable here yet; edit the JSON directly.</p>
      )}
      <TruncationEditor el={el} flat={flat} />
      <ResamplingEditor el={el} flat={flat} />
    </>
  )
}

/** How often a sample node redraws. Absent = once per realization (a fixed uncertain
 *  parameter); `always` = a fresh draw every timestep (noise); `periodic` = every `period`. */
function ResamplingEditor({ el, flat }: { el: ElementSummary; flat: FlatElement }) {
  const mutate = useStore((s) => s.mutateEl)
  const tsUnit = useStore((s) => s.doc?.simulation_settings.timestep.unit) ?? 's'
  const resampling = flat.resampling as { mode?: string; period?: { value: number; unit: string } } | null | undefined
  const mode = resampling?.mode ?? 'once'

  const setMode = (m: string) =>
    mutate(el.id, (e) => {
      if (m === 'once') e.resampling = null
      else if (m === 'periodic') e.resampling = { mode: 'periodic', period: e.resampling?.period ?? { value: 1, unit: tsUnit } }
      else e.resampling = { mode: m }
    })
  const setPeriod = (v: number) =>
    mutate(el.id, (e) => { e.resampling = { mode: 'periodic', period: { value: v, unit: e.resampling?.period?.unit ?? tsUnit } } })

  return (
    <>
      <Field label="Resample" hint="How often a fresh value is drawn.">
        <Select value={mode} onChange={setMode}
          options={[
            { value: 'once', label: 'Once per run' },
            { value: 'always', label: 'Every timestep' },
            { value: 'periodic', label: 'Periodic…' },
          ]} />
      </Field>
      {mode === 'periodic' && (
        <Field label="Period">
          <NumInput value={resampling?.period?.value ?? 1} unit={resampling?.period?.unit ?? tsUnit}
            onChange={setPeriod} />
        </Field>
      )}
    </>
  )
}

function TruncationEditor({ el, flat }: { el: ElementSummary; flat: FlatElement }) {
  const mutate = useStore((s) => s.mutateEl)
  const trunc = (flat.distribution as { truncation?: { min?: number; max?: number } | null } | undefined)?.truncation
  const set = (patch: { min?: number; max?: number }) =>
    mutate(el.id, (e) => {
      const d = e.distribution as { truncation?: { min?: number; max?: number } | null }
      d.truncation = { ...(d.truncation ?? {}), ...patch }
    })
  return (
    <div className="grid grid-cols-2 gap-2">
      <Field label="Truncate min">
        <NumInput value={trunc?.min ?? NaN} onChange={(min) => set({ min })} />
      </Field>
      <Field label="Truncate max">
        <NumInput value={trunc?.max ?? NaN} onChange={(max) => set({ max })} />
      </Field>
    </div>
  )
}

// ── Expression ──────────────────────────────────────────────────────────────────

/* eslint-disable @typescript-eslint/no-explicit-any */
function ExpressionRuleEditor({ el }: { el: ElementSummary }) {
  const mutate = useStore((s) => s.mutateEl)
  const doc = useStore((s) => s.doc)
  const flat = doc?.elements.find((e) => e.id === el.id)
  const dims = doc?.dimensions ?? []
  const ast = (flat?.expression as { ast?: Ast } | undefined)?.ast
  const isArray = (ast as any)?.op === 'vector_map'
  const over: string = isArray ? (ast as any).over : ''
  const body: Ast | undefined = isArray ? (ast as any).body : ast

  // Commit the per-member body — re-wrapped in the vector_map when this element is an array.
  const commitBody = (b: Ast) => {
    const full: Ast = over ? { op: 'vector_map', over, body: b } : b
    mutate(el.id, (e) => { e.expression = { ast: full, display: printAst(full) }; recomputeInputs(e) })
  }

  // Make/unmake this element an array over `dim`: wrap/unwrap the vector_map and mark the
  // primary output dimensioned (what drives per-member `#k` result expansion).
  const setOver = (dim: string) => mutate(el.id, (e) => {
    const cur = (e.expression as { ast?: Ast } | undefined)?.ast
    const curBody: Ast = (cur as any)?.op === 'vector_map' ? (cur as any).body : (cur ?? { op: 'literal', value: 0 })
    const full: Ast = dim ? { op: 'vector_map', over: dim, body: curBody } : curBody
    e.expression = { ast: full, display: printAst(full) }
    const outs: any[] = Array.isArray((e as any).outputs) ? [...(e as any).outputs] : []
    const o0: any = { name: e.name ?? e.id, unit: (e.unit as string) ?? '1', ...(outs[0] ?? {}) }
    if (dim) o0.dimensions = [dim]
    else delete o0.dimensions
    outs[0] = o0
    ;(e as any).outputs = outs
    recomputeInputs(e)
  })

  return (
    <div className="space-y-2">
      {dims.length > 0 && (
        <Field label="Array over" hint="Compute one value per member of a declared dimension.">
          <Select value={over} onChange={setOver}
            options={[{ value: '', label: 'Scalar (not an array)' }, ...dims.map((d) => ({ value: d.id, label: `${d.name} (${d.size})` }))]} />
        </Field>
      )}
      <Field
        label={isArray ? 'Per-member expression' : 'Expression'}
        hint={isArray
          ? 'One value per member. `member` = this member’s index; `arr[member]` picks this member of an array.'
          : 'References draw influence arrows; ⌘/Ctrl-Enter or blur to apply.'}>
        <ExpressionEditor key={over} ast={body} onCommit={commitBody} />
      </Field>
    </div>
  )
}
/* eslint-enable @typescript-eslint/no-explicit-any */

// ── Stock ───────────────────────────────────────────────────────────────────────

function StockEditor({ el, flat }: { el: ElementSummary; flat: FlatElement }) {
  const mutate = useStore((s) => s.mutateEl)
  const initial = paramValue(flat.initial_value)
  const rateAst = (flat.rate as { ast?: Ast } | undefined)?.ast
  // The engine treats a direct `rate` and inflows/outflows as either-or (a present rate
  // shadows flows). Surface that as an explicit mode: 'rate' shows the net-rate expression;
  // 'flows' shows inflows/outflows + growth rate (return_rate composes with flows).
  const mode: 'flows' | 'rate' = flat.rate != null ? 'rate' : 'flows'
  const growth = flat.return_rate?.value
  // return_rate is applied as `rr · dt` in the timestep's time unit (not per-step), so label
  // it per that unit to keep the rate unambiguous when dt ≠ 1.
  const timeUnit = useStore((s) => s.doc?.simulation_settings.timestep.unit) ?? 's'

  const setMode = (m: string) =>
    mutate(el.id, (e) => {
      if (m === 'rate') {
        e.rate = e.rate ?? { ast: { op: 'literal', value: 0 }, display: '0' }
        delete e.inflows; delete e.outflows; delete e.return_rate
      } else {
        delete e.rate
        e.inflows = e.inflows ?? []; e.outflows = e.outflows ?? []
      }
      recomputeInputs(e)
    })

  return (
    <>
      <Field label="Initial value">
        <NumInput value={initial} unit={el.unit}
          onChange={(v) => mutate(el.id, (e) => { e.initial_value = { value: v, unit: e.initial_value?.unit ?? el.unit } })} />
      </Field>
      <Field label="Change driven by" hint="A stock is driven by wired flows or a single net-rate expression — not both.">
        <Select value={mode} onChange={setMode}
          options={[
            { value: 'flows', label: 'Inflows / outflows' },
            { value: 'rate', label: 'Net rate expression' },
          ]} />
      </Field>
      {mode === 'rate' ? (
        <Field label="Net rate (d/dt)" hint="Direct rate expression for the whole stock.">
          <ExpressionEditor ast={rateAst}
            onCommit={(a) => mutate(el.id, (e) => { e.rate = { ast: a, display: printAst(a) }; recomputeInputs(e) })} />
        </Field>
      ) : (
        <>
          <RefListEditor el={el} flat={flat} field="inflows" label="Inflows" />
          <RefListEditor el={el} flat={flat} field="outflows" label="Outflows" />
          <Field label="Growth rate" hint={`Compounds on the current level (e.g. interest), per ${timeUnit}; adds to flows.`}>
            <NumInput value={growth ?? NaN} unit={`/${timeUnit}`}
              onChange={(v) => mutate(el.id, (e) => {
                if (isNaN(v) || v === 0) delete e.return_rate
                else e.return_rate = { value: v, unit: '1' }
              })} />
          </Field>
        </>
      )}
      <div className="grid grid-cols-2 gap-2">
        <Field label="Floor (min)">
          <NumInput value={flat.floor?.value ?? flat.min_value ?? NaN}
            onChange={(v) => mutate(el.id, (e) => { e.floor = { value: v, unit: el.unit } })} />
        </Field>
        <Field label="Capacity (max)">
          <NumInput value={flat.capacity?.value ?? NaN}
            onChange={(v) => mutate(el.id, (e) => { e.capacity = { value: v, unit: el.unit } })} />
        </Field>
      </div>
    </>
  )
}

/** Edit a list of element-id references (inflows / outflows), each an existing element. */
function RefListEditor({ el, flat, field, label }: { el: ElementSummary; flat: FlatElement; field: 'inflows' | 'outflows'; label: string }) {
  const mutate = useStore((s) => s.mutateEl)
  const elements = useElements()
  const list = (flat[field] as string[] | undefined) ?? []
  const candidates = elements.filter((e) => e.id !== el.id && !list.includes(e.id))

  return (
    <Field label={label}>
      <div className="space-y-1">
        {list.map((ref) => (
          <div key={ref} className="flex items-center gap-1">
            <span className="flex-1 truncate rounded bg-slate-100 px-2 py-0.5 font-mono text-[11px]">{ref}</span>
            <button className="text-slate-400 hover:text-red-500"
              onClick={() => mutate(el.id, (e) => { e[field] = (e[field] as string[]).filter((x) => x !== ref); recomputeInputs(e) })}>×</button>
          </div>
        ))}
        {candidates.length > 0 && (
          <select value="" onChange={(ev) => { const v = ev.target.value; if (v) mutate(el.id, (e) => { e[field] = [...((e[field] as string[]) ?? []), v]; recomputeInputs(e) }) }}
            className="w-full rounded border border-dashed border-slate-300 px-2 py-1 text-[11px] text-slate-500">
            <option value="">+ add {label.toLowerCase().replace(/s$/, '')}…</option>
            {candidates.map((c) => <option key={c.id} value={c.id}>{c.name}</option>)}
          </select>
        )}
      </div>
    </Field>
  )
}

// ── Lookup / Series ──────────────────────────────────────────────────────────────

function LookupEditor({ el, flat }: { el: ElementSummary; flat: FlatElement }) {
  const mutate = useStore((s) => s.mutateEl)
  const table = flat.table ?? { x: [], y: [] }
  const rows = table.x.map((x, i) => ({ x, y: table.y[i] ?? 0 }))

  const setCell = (i: number, key: 'x' | 'y', v: number) =>
    mutate(el.id, (e) => { if (e.table) e.table[key][i] = v })
  const addRow = () => mutate(el.id, (e) => { if (e.table) { e.table.x.push(0); e.table.y.push(0) } })
  const delRow = (i: number) => mutate(el.id, (e) => { if (e.table) { e.table.x.splice(i, 1); e.table.y.splice(i, 1) } })

  return (
    <>
      <Field label="Interpolation">
        <Select value={(table.interpolation as string) ?? 'linear'}
          onChange={(v) => mutate(el.id, (e) => { if (e.table) e.table.interpolation = v })}
          options={[{ value: 'linear', label: 'Linear' }, { value: 'step', label: 'Step' }, { value: 'cubic', label: 'Cubic' }]} />
      </Field>
      <TableGrid rows={rows} onSet={setCell} onAdd={addRow} onDel={delRow} xLabel="x" yLabel="y" />
    </>
  )
}

function SeriesEditor({ flat }: { flat: FlatElement }) {
  const mutate = useStore((s) => s.mutateEl)
  const ts = flat.timestamps ?? []
  const vs = flat.values ?? []
  const rows = ts.map((t, i) => ({ x: t, y: vs[i] ?? 0 }))
  const id = flat.id

  const setCell = (i: number, key: 'x' | 'y', v: number) =>
    mutate(id, (e) => { if (key === 'x' && e.timestamps) e.timestamps[i] = v; if (key === 'y' && e.values) e.values[i] = v })
  const addRow = () => mutate(id, (e) => { e.timestamps = [...(e.timestamps ?? []), 0]; e.values = [...(e.values ?? []), 0] })
  const delRow = (i: number) => mutate(id, (e) => { e.timestamps?.splice(i, 1); e.values?.splice(i, 1) })

  return (
    <>
      <Field label="Time unit">
        <TextInput value={flat.time_unit ?? 's'} onChange={(v) => mutate(id, (e) => { e.time_unit = v })} mono />
      </Field>
      <TableGrid rows={rows} onSet={setCell} onAdd={addRow} onDel={delRow} xLabel="time" yLabel="value" />
    </>
  )
}

function TableGrid({ rows, onSet, onAdd, onDel, xLabel, yLabel }: {
  rows: { x: number; y: number }[]; onSet: (i: number, k: 'x' | 'y', v: number) => void
  onAdd: () => void; onDel: (i: number) => void; xLabel: string; yLabel: string
}) {
  return (
    <div>
      <div className="mb-1 grid grid-cols-[1fr_1fr_auto] gap-1 text-[10px] font-medium text-slate-400">
        <span>{xLabel}</span><span>{yLabel}</span><span />
      </div>
      <div className="max-h-48 space-y-1 overflow-auto">
        {rows.map((r, i) => (
          <div key={i} className="grid grid-cols-[1fr_1fr_auto] items-center gap-1">
            <input type="number" value={r.x} onChange={(e) => onSet(i, 'x', parseFloat(e.target.value))}
              className="rounded border border-slate-300 px-1.5 py-0.5 text-[11px]" />
            <input type="number" value={r.y} onChange={(e) => onSet(i, 'y', parseFloat(e.target.value))}
              className="rounded border border-slate-300 px-1.5 py-0.5 text-[11px]" />
            <button className="px-1 text-slate-400 hover:text-red-500" onClick={() => onDel(i)}>×</button>
          </div>
        ))}
      </div>
      <button onClick={onAdd} className="mt-1 text-[11px] font-medium text-blue-600 hover:text-blue-500">+ add row</button>
    </div>
  )
}

// ── Lag / Filter ──────────────────────────────────────────────────────────────

function LagEditor({ el, flat }: { el: ElementSummary; flat: FlatElement }) {
  const mutate = useStore((s) => s.mutateEl)
  const elements = useElements()
  return (
    <>
      <Field label="Input">
        <Select value={flat.input ?? ''} onChange={(v) => mutate(el.id, (e) => { e.input = v || null; e.inputs = v ? [v] : [] })}
          options={[{ value: '', label: '— none —' }, ...elements.filter((e) => e.id !== el.id).map((e) => ({ value: e.id, label: e.name }))]} />
      </Field>
      <Field label="Initial value" hint="Value emitted on the first step.">
        <NumInput value={flat.initial?.value ?? 0} unit={el.unit}
          onChange={(v) => mutate(el.id, (e) => { e.initial = { value: v, unit: el.unit } })} />
      </Field>
    </>
  )
}

function FilterEditor({ el, flat }: { el: ElementSummary; flat: FlatElement }) {
  const mutate = useStore((s) => s.mutateEl)
  const elements = useElements()
  return (
    <>
      <Field label="Input">
        <Select value={flat.input ?? ''} onChange={(v) => mutate(el.id, (e) => { e.input = v || null; e.inputs = v ? [v] : [] })}
          options={[{ value: '', label: '— none —' }, ...elements.filter((e) => e.id !== el.id).map((e) => ({ value: e.id, label: e.name }))]} />
      </Field>
      <div className="grid grid-cols-2 gap-2">
        <Field label="Window (steps)">
          <NumInput value={flat.window ?? 1} step={1} onChange={(v) => mutate(el.id, (e) => { e.window = Math.max(1, Math.round(v)) })} />
        </Field>
        <Field label="Statistic">
          <Select value={flat.statistic ?? 'mean'} onChange={(v) => mutate(el.id, (e) => { e.statistic = v })}
            options={[
              { value: 'mean', label: 'Mean' }, { value: 'min', label: 'Min' }, { value: 'max', label: 'Max' },
              { value: 'sum', label: 'Sum' }, { value: 'ema', label: 'EMA' },
            ]} />
        </Field>
      </div>
    </>
  )
}

// ── Event / failure state machine (spec §5.5) ─────────────────────────────────────

const TRIGGER_MODES = [
  { value: 'on_condition', label: 'When a condition is true' },
  { value: 'on_schedule', label: 'At scheduled times' },
  { value: 'on_event', label: 'When another event fires' },
  { value: 'periodic', label: 'Periodically' },
  { value: 'always', label: 'Every timestep' },
]
const FAILURE_BASES = [
  { value: 'condition', label: 'Condition (state ≥ threshold)' },
  { value: 'operating_time', label: 'Operating time (TTF)' },
  { value: 'exposure_time', label: 'Exposure time (TTF)' },
  { value: 'demand', label: 'On demand (per-demand p)' },
]
const REPAIR_POLICIES = [
  { value: 'none', label: 'None (run to failure)' },
  { value: 'repair', label: 'Repair (as-was)' },
  { value: 'replace', label: 'Replace (as-new)' },
  { value: 'preventive_maintenance', label: 'Preventive maintenance' },
]
const EFFECT_MODES = ['additive', 'multiplicative', 'replace', 'interrupt', 'spend', 'deposit', 'borrow']
const NEEDS_TTF = new Set(['operating_time', 'exposure_time', 'demand'])
const NEEDS_TTR = new Set(['repair', 'replace', 'preventive_maintenance'])

/* eslint-disable @typescript-eslint/no-explicit-any */
function EventEditor({ el, flat }: { el: ElementSummary; flat: FlatElement }) {
  const mutate = useStore((s) => s.mutateEl)
  const elements = useElements()
  const tsUnit = useStore((s) => s.doc?.simulation_settings.timestep.unit) ?? 's'
  const trigger = (flat.trigger as any) ?? { mode: 'on_condition' }
  const fp = flat.failure_process as any
  const effects = (flat.effects as any[]) ?? []
  const mode: string = trigger.mode ?? 'on_condition'
  const others = elements.filter((e) => e.id !== el.id)

  const patchTrigger = (fn: (t: any) => void) =>
    mutate(el.id, (e) => { const t = { ...((e.trigger as any) ?? {}) }; fn(t); e.trigger = t; recomputeInputs(e) })
  const patchFp = (fn: (f: any) => void) =>
    mutate(el.id, (e) => { const f = { ...((e.failure_process as any) ?? {}) }; fn(f); e.failure_process = f })
  const setEffects = (next: any[]) => mutate(el.id, (e) => { e.effects = next; recomputeInputs(e) })

  return (
    <div className="space-y-3">
      <Field label="Fires" hint="What makes this event / failure fire.">
        <Select value={mode} onChange={(m) => patchTrigger((t) => {
          t.mode = m
          if (m === 'on_condition' && !t.condition) t.condition = { ast: { op: 'literal', value: 0 }, display: '0' }
          if (m === 'on_schedule' && !t.schedule) t.schedule = []
        })} options={TRIGGER_MODES} />
      </Field>

      {mode === 'on_condition' && (
        <Field label="Condition" hint="Fires the step this becomes true — e.g. damage ≥ 1.">
          <ExpressionEditor ast={(trigger.condition as any)?.ast}
            onCommit={(a) => patchTrigger((t) => { t.condition = { ast: a, display: printAst(a) } })}
            placeholder="e.g. damage ≥ 1" />
        </Field>
      )}
      {mode === 'on_schedule' && (
        <Field label={`Times (${tsUnit})`} hint="Comma-separated instants.">
          <TextInput mono value={((trigger.schedule as any[]) ?? []).map((s) => s.value).join(', ')}
            onChange={(v) => patchTrigger((t) => {
              t.schedule = v.split(',').map((s) => Number(s.trim())).filter((n) => !isNaN(n)).map((n) => ({ value: n, unit: tsUnit }))
            })} />
        </Field>
      )}
      {mode === 'on_event' && (
        <Field label="Source event">
          <Select value={(trigger.source as string) ?? ''} onChange={(v) => patchTrigger((t) => { t.source = v || null })}
            options={[{ value: '', label: '— none —' }, ...others.map((e) => ({ value: e.id, label: e.name }))]} />
        </Field>
      )}
      {mode === 'periodic' && (
        <Field label={`Period (${tsUnit})`}>
          <NumInput value={(trigger.period as any)?.value ?? 1} onChange={(v) => patchTrigger((t) => { t.period = { value: v, unit: tsUnit } })} />
        </Field>
      )}

      <Toggle label="Acts as a failure state machine" checked={!!fp}
        onChange={(on) => mutate(el.id, (e) => {
          if (on) e.failure_process = { basis: 'condition', repair: { policy: 'none' } }
          else delete (e as any).failure_process
        })} />

      {fp && (
        <div className="space-y-2 rounded border border-slate-200 bg-slate-50/50 p-2">
          <Field label="Failure basis" hint="How failure is decided.">
            <Select value={fp.basis ?? 'condition'} onChange={(b) => patchFp((f) => { f.basis = b })} options={FAILURE_BASES} />
          </Field>
          {NEEDS_TTF.has(fp.basis ?? '') && (
            <DistPicker label="Time to failure" dist={fp.time_to_failure} onChange={(d) => patchFp((f) => { f.time_to_failure = d })} />
          )}
          <Field label="Repair policy">
            <Select value={fp.repair?.policy ?? 'none'} onChange={(p) => patchFp((f) => { f.repair = { ...(f.repair ?? {}), policy: p } })} options={REPAIR_POLICIES} />
          </Field>
          {NEEDS_TTR.has(fp.repair?.policy ?? 'none') && (
            <DistPicker label="Time to repair" dist={fp.repair?.time_to_repair} onChange={(d) => patchFp((f) => { f.repair = { ...(f.repair ?? {}), time_to_repair: d } })} />
          )}
        </div>
      )}

      <EffectsEditor targets={others} effects={effects} unit={el.unit} onChange={setEffects} />
    </div>
  )
}

/** Compact distribution editor (family + params) for TTF / TTR fields. */
function DistPicker({ label, dist, onChange }: { label: string; dist: any; onChange: (d: any) => void }) {
  const family: string = dist?.family ?? 'lognormal'
  const def = distDef(family)
  return (
    <Field label={label}>
      <Select value={family} onChange={(fam) => { const d = distDef(fam); onChange({ family: fam, parameters: d ? d.defaults() : {} }) }}
        options={DISTRIBUTIONS.map((d) => ({ value: d.family, label: d.label, group: d.group }))} />
      {def && (
        <div className="mt-1 grid grid-cols-2 gap-2">
          {def.params.map((p) => (
            <Field key={p} label={p}>
              <NumInput value={paramValue(dist?.parameters?.[p])}
                onChange={(v) => onChange({ family, parameters: { ...(dist?.parameters ?? {}), [p]: { value: v, unit: '1' } } })} />
            </Field>
          ))}
        </div>
      )}
    </Field>
  )
}

/** Effects applied when the event fires (spec §5.5): target element + how it changes. */
function EffectsEditor({ targets, effects, unit, onChange }: {
  targets: ElementSummary[]; effects: any[]; unit: string; onChange: (e: any[]) => void
}) {
  const add = () => onChange([...effects, { target: targets[0]?.id ?? '', mode: 'additive', change: { value: 0, unit } }])
  const update = (i: number, patch: any) => onChange(effects.map((e, j) => (j === i ? { ...e, ...patch } : e)))
  const remove = (i: number) => onChange(effects.filter((_, j) => j !== i))
  return (
    <div className="space-y-1.5">
      <div className="flex items-center justify-between">
        <span className="text-[11px] font-medium text-slate-500">Effects on fire</span>
        <button type="button" onClick={add} className="rounded border border-slate-200 px-1.5 py-0.5 text-[11px] text-slate-500 hover:bg-slate-50">+ add</button>
      </div>
      {effects.length === 0 && <p className="text-[11px] text-slate-400">No effects — a failure FSM still exposes its 0 / 1 failed state.</p>}
      {effects.map((eff, i) => (
        <div key={i} className="space-y-1 rounded border border-slate-200 p-1.5">
          <div className="flex items-center gap-1">
            <div className="flex-1">
              <Select value={eff.target ?? ''} onChange={(v) => update(i, { target: v })}
                options={[{ value: '', label: '— target —' }, ...targets.map((t) => ({ value: t.id, label: t.name }))]} />
            </div>
            <button type="button" onClick={() => remove(i)} className="px-1 text-slate-400 hover:text-rose-500" title="Remove effect">×</button>
          </div>
          <div className="flex items-center gap-1">
            <div className="flex-1">
              <Select value={eff.mode ?? 'additive'} onChange={(v) => update(i, { mode: v })}
                options={EFFECT_MODES.map((m) => ({ value: m, label: m }))} />
            </div>
            {eff.mode !== 'interrupt' && (
              <div className="w-24">
                <NumInput value={(eff.change as any)?.value ?? 0}
                  onChange={(v) => update(i, { change: { value: v, unit: (eff.change as any)?.unit ?? '1' } })} />
              </div>
            )}
          </div>
        </div>
      ))}
    </div>
  )
}
/* eslint-enable @typescript-eslint/no-explicit-any */

// ── Unsupported-rule fallback (engine-truthful; no faked UI, §14) ─────────────────

function UnsupportedEditor({ el, flat }: { el: ElementSummary; flat: FlatElement }) {
  const replaceEl = useStore((s) => s.replaceEl)
  const [text, setText] = useState(() => JSON.stringify(flat, null, 2))
  const [err, setErr] = useState<string | null>(null)

  // Re-seed the editor only when a *different* element is selected — not on every reconcile,
  // so an in-progress edit isn't clobbered by the doc snapshot changing identity.
  useEffect(() => { setText(JSON.stringify(flat, null, 2)); setErr(null) }, [flat.id]) // eslint-disable-line react-hooks/exhaustive-deps

  const apply = () => {
    let parsed: FlatElement
    try {
      parsed = JSON.parse(text)
    } catch (e) {
      setErr(`Invalid JSON: ${(e as Error).message}`)
      return
    }
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
      setErr('The element must be a JSON object.')
      return
    }
    if (parsed.id !== flat.id) {
      setErr(`Keep "id": "${flat.id}" — use the ID field above to rename (that also updates references).`)
      return
    }
    setErr(null)
    replaceEl(flat.id, parsed) // reconciles + validates live
  }

  return (
    <div className="space-y-2 text-[11px] text-slate-500">
      <p>No structured editor for <span className="font-mono">{kindLabel(el)}</span> yet — edit its raw JSON below; the reconcile loop validates it live.</p>
      <textarea
        className="h-56 w-full resize-y rounded border border-slate-200 bg-slate-50 p-2 font-mono text-[11px] leading-snug text-slate-700 focus:border-slate-400 focus:outline-none"
        spellCheck={false}
        value={text}
        onChange={(e) => setText(e.target.value)}
      />
      {err && <p className="rounded bg-rose-50 p-1.5 text-rose-600">{err}</p>}
      <div className="flex gap-2">
        <button type="button" onClick={apply}
          className="rounded bg-slate-800 px-2 py-1 text-[11px] font-medium text-white hover:bg-slate-700">
          Apply JSON
        </button>
        <button type="button" onClick={() => { setText(JSON.stringify(flat, null, 2)); setErr(null) }}
          className="rounded border border-slate-200 px-2 py-1 text-[11px] text-slate-500 hover:bg-slate-50">
          Revert
        </button>
      </div>
    </div>
  )
}

// ── Output & units / Save results ────────────────────────────────────────────────

function OutputSection({ el, flat }: { el: ElementSummary; flat: FlatElement }) {
  const mutate = useStore((s) => s.mutateEl)
  return (
    <Section title="Output & units">
      <Field label="Canonical unit" hint={el.display_unit ? `Displayed as ${el.display_unit}` : undefined}>
        <TextInput value={el.unit} mono onChange={(unit) => mutate(el.id, (e) => {
          // Write the unit where the primitive keeps it.
          if (e.value) e.value = { ...e.value, unit }
          else if (e.initial_value) e.initial_value = { ...e.initial_value, unit }
          else e.unit = unit
        })} />
      </Field>
      {el.traits.length > 0 && (
        <div className="flex flex-wrap gap-1">
          {el.traits.map((t) => <span key={t} className="rounded bg-slate-100 px-1.5 py-0.5 text-[10px] text-slate-500">{t}</span>)}
        </div>
      )}
      <div className="text-[10px] text-slate-400">Referenced by {el.inputs.length ? '' : 'none · '}{el.inputs.length} input(s)</div>
      {void flat}
    </Section>
  )
}

function SaveSection({ flat }: { flat: FlatElement }) {
  const mutate = useStore((s) => s.mutateEl)
  const save = flat.save_results ?? {}
  return (
    <Section title="Save results">
      <Toggle label="Time history" checked={save.time_history ?? true}
        onChange={(v) => mutate(flat.id, (e) => { e.save_results = { ...e.save_results, time_history: v } })} />
      <Toggle label="Final value" checked={save.final_value ?? true}
        onChange={(v) => mutate(flat.id, (e) => { e.save_results = { ...e.save_results, final_value: v } })} />
    </Section>
  )
}
