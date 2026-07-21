// Small labelled form controls shared by the inspector editors.
import type { ReactNode } from 'react'

export function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="border-t border-slate-100 px-3 py-3 first:border-t-0">
      <h4 className="mb-2 text-[10px] font-semibold uppercase tracking-wide text-slate-400">{title}</h4>
      <div className="space-y-2">{children}</div>
    </div>
  )
}

export function Field({ label, children, hint }: { label: string; children: ReactNode; hint?: string }) {
  return (
    <label className="block">
      <span className="mb-0.5 block text-[11px] font-medium text-slate-500">{label}</span>
      {children}
      {hint && <span className="mt-0.5 block text-[10px] text-slate-400">{hint}</span>}
    </label>
  )
}

export function TextInput({ value, onChange, mono, placeholder, onBlur }: {
  value: string; onChange: (v: string) => void; mono?: boolean; placeholder?: string; onBlur?: () => void
}) {
  return (
    <input
      value={value}
      placeholder={placeholder}
      onChange={(e) => onChange(e.target.value)}
      onBlur={onBlur}
      className={`w-full rounded border border-slate-300 px-2 py-1 text-xs outline-none focus:border-blue-400 ${mono ? 'font-mono' : ''}`}
    />
  )
}

export function NumInput({ value, onChange, step, unit }: {
  value: number; onChange: (v: number) => void; step?: number; unit?: string | null
}) {
  return (
    <div className="flex items-center gap-1">
      <input
        type="number"
        value={Number.isFinite(value) ? value : ''}
        step={step ?? 'any'}
        onChange={(e) => { const v = parseFloat(e.target.value); if (!isNaN(v)) onChange(v) }}
        className="w-full rounded border border-slate-300 px-2 py-1 text-xs outline-none focus:border-blue-400"
      />
      {unit && unit !== '1' && <span className="shrink-0 text-[10px] text-slate-400">{unit}</span>}
    </div>
  )
}

export function Toggle({ label, checked, onChange }: { label: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <label className="flex cursor-pointer items-center gap-2 text-[11px] text-slate-600">
      <input type="checkbox" checked={checked} onChange={(e) => onChange(e.target.checked)} className="h-3.5 w-3.5" />
      {label}
    </label>
  )
}

export function Select<T extends string>({ value, options, onChange }: {
  value: T; options: { value: T; label: string; group?: string }[]; onChange: (v: T) => void
}) {
  const groups = [...new Set(options.map((o) => o.group))]
  return (
    <select
      value={value}
      onChange={(e) => onChange(e.target.value as T)}
      className="w-full rounded border border-slate-300 px-2 py-1 text-xs outline-none focus:border-blue-400"
    >
      {groups[0] === undefined
        ? options.map((o) => <option key={o.value} value={o.value}>{o.label}</option>)
        : groups.map((g) => (
            <optgroup key={g} label={g}>
              {options.filter((o) => o.group === g).map((o) => <option key={o.value} value={o.value}>{o.label}</option>)}
            </optgroup>
          ))}
    </select>
  )
}
