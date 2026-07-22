import { useMemo, useRef, useState } from 'react'
import { BUILTINS, TIME_REFS, printAst, tryParseExpr, type Ast } from '../../model/ast'
import { useElements } from '../../store'

interface Props {
  ast: Ast | undefined
  /** Commit a parsed AST back to the model. */
  onCommit: (ast: Ast) => void
  placeholder?: string
}

/**
 * The expression editor (spec §6): type a formula, it parses to an AST live, references
 * autocomplete from the model's elements (and become influence arrows once committed),
 * builtins/time-refs insert from a palette, and parse errors surface inline before run time.
 */
export function ExpressionEditor({ ast, onCommit, placeholder }: Props) {
  const elements = useElements()
  const [text, setText] = useState(() => printAst(ast))
  const [pick, setPick] = useState('')
  const ref = useRef<HTMLTextAreaElement>(null)

  const parsed = useMemo(() => tryParseExpr(text.trim() === '' ? '0' : text), [text])
  const dirty = printAst(ast) !== text

  const insertAtCaret = (token: string) => {
    const el = ref.current
    const start = el?.selectionStart ?? text.length
    const end = el?.selectionEnd ?? text.length
    const next = text.slice(0, start) + token + text.slice(end)
    setText(next)
    requestAnimationFrame(() => {
      el?.focus()
      const caret = start + token.length
      el?.setSelectionRange(caret, caret)
    })
  }

  const commit = () => {
    if (parsed.ok) onCommit(parsed.ast)
  }

  const q = pick.toLowerCase()
  const matchEls = q
    ? elements.filter((e) => e.id.toLowerCase().includes(q) || e.name.toLowerCase().includes(q)).slice(0, 8)
    : elements.slice(0, 8)
  const matchFns = q ? BUILTINS.filter((b) => b.name.includes(q)).slice(0, 6) : []
  const matchTime = q ? TIME_REFS.filter((t) => t.includes(q)).slice(0, 6) : []

  return (
    <div className="space-y-1.5">
      <textarea
        ref={ref}
        value={text}
        spellCheck={false}
        onChange={(e) => setText(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) { e.preventDefault(); commit() }
        }}
        placeholder={placeholder ?? 'e.g. inflow × 0.8 + baseline'}
        rows={2}
        className={`w-full resize-y rounded border px-2 py-1.5 font-mono text-xs outline-none ${
          parsed.ok ? 'border-slate-300 focus:border-blue-400' : 'border-red-300 bg-red-50 focus:border-red-400'
        }`}
      />

      {/* Live diagnostics */}
      <div className="flex items-center justify-between text-[11px]">
        {parsed.ok ? (
          <span className="text-emerald-600">✓ parses</span>
        ) : (
          <span className="text-red-600">✗ {parsed.error}</span>
        )}
        {dirty && parsed.ok && (
          <button onClick={commit} className="rounded bg-blue-600 px-2 py-0.5 font-medium text-white hover:bg-blue-500">
            Apply
          </button>
        )}
      </div>

      {/* Reference / builtin inserter (autocomplete) */}
      <div>
        <input
          value={pick}
          onChange={(e) => setPick(e.target.value)}
          placeholder="insert reference / function…"
          className="w-full rounded border border-slate-200 bg-slate-50 px-2 py-1 text-[11px] outline-none focus:border-blue-300"
        />
        {(matchEls.length > 0 || matchFns.length > 0 || matchTime.length > 0) && (
          <div className="mt-1 max-h-40 overflow-auto rounded border border-slate-200 bg-white text-[11px] shadow-sm">
            {matchEls.map((e) => (
              <button key={e.id} onClick={() => insertAtCaret(e.id)}
                className="flex w-full items-center justify-between px-2 py-1 text-left hover:bg-blue-50">
                <span className="font-mono text-slate-700">{e.id}</span>
                <span className="ml-2 truncate text-slate-400">{e.name}</span>
              </button>
            ))}
            {matchFns.map((b) => (
              <button key={b.name} onClick={() => insertAtCaret(`${b.name}()`)}
                className="flex w-full items-center justify-between px-2 py-1 text-left hover:bg-violet-50">
                <span className="font-mono text-violet-700">{b.sig}</span>
                <span className="ml-2 text-slate-400">{b.group}</span>
              </button>
            ))}
            {matchTime.map((t) => (
              <button key={t} onClick={() => insertAtCaret(t)}
                className="flex w-full items-center px-2 py-1 text-left hover:bg-emerald-50">
                <span className="font-mono text-emerald-700">{t}</span>
                <span className="ml-2 text-slate-400">time</span>
              </button>
            ))}
          </div>
        )}
      </div>
    </div>
  )
}
