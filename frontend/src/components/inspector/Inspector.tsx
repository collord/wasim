import { useMemo } from 'react'
import { useStore, useElements, useContainers } from '../../store'
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
          <div className="truncate text-[10px] text-slate-400">{kindLabel(flat)}</div>
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

  let body = <UnsupportedEditor el={el} />
  if (prim === 'stock') body = <StockEditor el={el} flat={flat} />
  else if (rule === 'fixed') body = <FixedEditor el={el} flat={flat} />
  else if (rule === 'sample') body = <SampleEditor el={el} flat={flat} />
  else if (rule === 'expression') body = <ExpressionRuleEditor el={el} />
  else if (rule === 'lookup') body = <LookupEditor el={el} flat={flat} />
  else if (rule === 'series') body = <SeriesEditor flat={flat} />
  else if (rule === 'lag') body = <LagEditor el={el} flat={flat} />
  else if (rule === 'filter') body = <FilterEditor el={el} flat={flat} />

  return <Section title="Definition">{body}</Section>
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

function ExpressionRuleEditor({ el }: { el: ElementSummary }) {
  const setExpr = useStore((s) => s.mutateEl)
  const doc = useStore((s) => s.doc)
  const flat = doc?.elements.find((e) => e.id === el.id)
  const ast = (flat?.expression as { ast?: Ast } | undefined)?.ast

  const commit = (a: Ast) =>
    setExpr(el.id, (e) => {
      e.expression = { ast: a, display: printAst(a) }
      // keep inputs in sync
      const refs = new Set<string>()
      const walk = (n: unknown) => {
        if (!n || typeof n !== 'object') return
        const o = n as Record<string, unknown>
        if (o.op === 'ref' && typeof o.element_id === 'string') refs.add(o.element_id)
        for (const v of Object.values(o)) {
          if (Array.isArray(v)) v.forEach(walk)
          else if (v && typeof v === 'object') walk(v)
        }
      }
      walk(a)
      e.inputs = [...refs]
    })

  return (
    <Field label="Expression" hint="References draw influence arrows; ⌘/Ctrl-Enter or blur to apply.">
      <ExpressionEditor ast={ast} onCommit={commit} />
    </Field>
  )
}

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

// ── Unsupported-rule fallback (engine-truthful; no faked UI, §14) ─────────────────

function UnsupportedEditor({ el }: { el: ElementSummary }) {
  return (
    <div className="space-y-2 text-[11px] text-slate-500">
      <p>Rich editing for <span className="font-mono">{kindLabel(el)}</span> isn’t built yet.</p>
      {el.formula && <p className="rounded bg-slate-50 p-2 font-mono text-slate-600">{el.formula}</p>}
      <p className="text-slate-400">Edit this element’s fields directly in the model JSON; the reconcile loop validates it live.</p>
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
