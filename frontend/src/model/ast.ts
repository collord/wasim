// Expression AST — the frontend mirror of the engine's `AstNode` (model.rs, serde
// `tag = "op", rename_all = "snake_case"`). The engine does NOT parse formula strings
// (a bare `Formula` evaluates to 0.0), so the authoring tool must produce a real AST.
// This module is the text ⇄ AST bridge used by the expression editor (spec §6).

export type Ast =
  | { op: 'literal'; value: number; unit?: string | null }
  | { op: 'ref'; element_id: string; output?: string }
  | { op: 'time_ref'; property: string }
  | { op: BinOp; left: Ast; right: Ast }
  | { op: 'neg' | 'not'; operand: Ast }
  | { op: 'call'; fn: string; args: Ast[] }
  | { op: 'if'; cond: Ast; then: Ast; else: Ast }
  // Preserved verbatim on round-trip; the text editor renders these opaquely.
  | { op: string; [k: string]: unknown }

type BinOp =
  | 'add' | 'subtract' | 'multiply' | 'divide' | 'power'
  | 'lt' | 'gt' | 'lte' | 'gte' | 'eq' | 'neq' | 'and' | 'or'

// ── Rosters (from BuiltinFn / TimeProperty, snake_case) ─────────────────────────

export interface FnDoc { name: string; sig: string; group: string }

export const BUILTINS: FnDoc[] = [
  { name: 'min', sig: 'min(a, b)', group: 'Math' },
  { name: 'max', sig: 'max(a, b)', group: 'Math' },
  { name: 'abs', sig: 'abs(x)', group: 'Math' },
  { name: 'sqrt', sig: 'sqrt(x)', group: 'Math' },
  { name: 'exp', sig: 'exp(x)', group: 'Math' },
  { name: 'ln', sig: 'ln(x)', group: 'Math' },
  { name: 'log', sig: 'log(x)', group: 'Math' },
  { name: 'log2', sig: 'log2(x)', group: 'Math' },
  { name: 'sin', sig: 'sin(x)', group: 'Trig' },
  { name: 'cos', sig: 'cos(x)', group: 'Trig' },
  { name: 'tan', sig: 'tan(x)', group: 'Trig' },
  { name: 'asin', sig: 'asin(x)', group: 'Trig' },
  { name: 'acos', sig: 'acos(x)', group: 'Trig' },
  { name: 'atan', sig: 'atan(x)', group: 'Trig' },
  { name: 'atan2', sig: 'atan2(y, x)', group: 'Trig' },
  { name: 'sinh', sig: 'sinh(x)', group: 'Trig' },
  { name: 'cosh', sig: 'cosh(x)', group: 'Trig' },
  { name: 'tanh', sig: 'tanh(x)', group: 'Trig' },
  { name: 'floor', sig: 'floor(x)', group: 'Math' },
  { name: 'ceil', sig: 'ceil(x)', group: 'Math' },
  { name: 'round', sig: 'round(x)', group: 'Math' },
  { name: 'mod', sig: 'mod(a, b)', group: 'Math' },
  { name: 'sign', sig: 'sign(x)', group: 'Math' },
  { name: 'int', sig: 'int(x)', group: 'Math' },
  { name: 'step', sig: 'step(x)', group: 'Math' },
  { name: 'gamma', sig: 'gamma(x)', group: 'Math' },
  { name: 'erf', sig: 'erf(x)', group: 'Math' },
  { name: 'erfc', sig: 'erfc(x)', group: 'Math' },
  { name: 'get_year', sig: 'get_year(t)', group: 'Calendar' },
  { name: 'get_month', sig: 'get_month(t)', group: 'Calendar' },
  { name: 'get_day', sig: 'get_day(t)', group: 'Calendar' },
  { name: 'get_hour', sig: 'get_hour(t)', group: 'Calendar' },
  { name: 'get_minute', sig: 'get_minute(t)', group: 'Calendar' },
  { name: 'get_second', sig: 'get_second(t)', group: 'Calendar' },
  { name: 'occurs', sig: 'occurs(event)', group: 'Events' },
  { name: 'changed', sig: 'changed(x)', group: 'Events' },
  { name: 'pv_factor', sig: 'pv_factor(rate, n)', group: 'Finance' },
  { name: 'annuity_factor', sig: 'annuity_factor(rate, n)', group: 'Finance' },
  { name: 'table_min', sig: 'table_min(table)', group: 'Array' },
  { name: 'table_max', sig: 'table_max(table)', group: 'Array' },
  { name: 'column_count', sig: 'column_count(table)', group: 'Array' },
  { name: 'sum_array', sig: 'sum_array(arr)', group: 'Array' },
  { name: 'size_array', sig: 'size_array(arr)', group: 'Array' },
  { name: 'get_element', sig: 'get_element(arr, i)', group: 'Array' },
  { name: 'interp_array', sig: 'interp_array(xs, ys, x)', group: 'Array' },
  { name: 'mean_array', sig: 'mean_array(arr)', group: 'Array' },
  { name: 'min_array', sig: 'min_array(arr)', group: 'Array' },
  { name: 'max_array', sig: 'max_array(arr)', group: 'Array' },
  { name: 'dot_product', sig: 'dot_product(a, b)', group: 'Array' },
]

export const TIME_REFS: string[] = [
  'elapsed', 'timestep', 'year', 'month', 'day_of_year', 'day_of_month',
  'days_in_month', 'hour', 'minute', 'second', 'start',
  'elapsed_months', 'elapsed_years',
]

const BUILTIN_NAMES = new Set(BUILTINS.map((b) => b.name))
const TIME_REF_NAMES = new Set(TIME_REFS)

// ── Printer: AST → readable infix display string ────────────────────────────────

const BIN_SYM: Record<string, string> = {
  add: '+', subtract: '−', multiply: '×', divide: '/', power: '^',
  lt: '<', gt: '>', lte: '≤', gte: '≥', eq: '==', neq: '≠', and: '&&', or: '||',
}
// Precedence for parenthesization when printing.
const PREC: Record<string, number> = {
  or: 1, and: 2,
  eq: 3, neq: 3, lt: 3, gt: 3, lte: 3, gte: 3,
  add: 4, subtract: 4,
  multiply: 5, divide: 5,
  power: 6,
}

export function printAst(node: Ast | undefined | null, parentPrec = 0): string {
  if (!node) return ''
  const n = node as Record<string, unknown>
  switch (node.op) {
    case 'literal': {
      const v = (node as { value: number }).value
      const u = (node as { unit?: string | null }).unit
      return u && u !== '1' ? `${fmtNum(v)} ${u}` : fmtNum(v)
    }
    case 'ref':
      return (node as { element_id: string }).element_id
    case 'time_ref':
      return (node as { property: string }).property
    case 'neg':
      return `−${printAst((node as { operand: Ast }).operand, 7)}`
    case 'not':
      return `!${printAst((node as { operand: Ast }).operand, 7)}`
    case 'if':
      return `if(${printAst((node as any).cond)}, ${printAst((node as any).then)}, ${printAst((node as any).else)})`
    case 'call':
      return `${(node as any).fn}(${((node as any).args as Ast[]).map((a) => printAst(a)).join(', ')})`
    default: {
      if (node.op in BIN_SYM) {
        const prec = PREC[node.op] ?? 0
        const s = `${printAst(n.left as Ast, prec)} ${BIN_SYM[node.op]} ${printAst(n.right as Ast, prec + 1)}`
        return prec < parentPrec ? `(${s})` : s
      }
      // Opaque node (lookup_call, submodel_stat, array, …) — a stable placeholder.
      return `⟨${node.op}⟩`
    }
  }
}

function fmtNum(v: number): string {
  if (!isFinite(v)) return String(v)
  return Number.isInteger(v) ? String(v) : String(+v.toPrecision(10))
}

/** All element ids referenced by an AST (drives `inputs` + influence arrows, spec §2.2). */
export function refsOf(node: Ast | undefined | null, acc: Set<string> = new Set()): Set<string> {
  if (!node || typeof node !== 'object') return acc
  const n = node as Record<string, unknown>
  if (node.op === 'ref' && typeof n.element_id === 'string') acc.add(n.element_id)
  for (const v of Object.values(n)) {
    if (Array.isArray(v)) v.forEach((c) => refsOf(c as Ast, acc))
    else if (v && typeof v === 'object' && 'op' in (v as object)) refsOf(v as Ast, acc)
  }
  return acc
}

// ── Parser: text → AST (Pratt / precedence-climbing) ────────────────────────────

export class ParseError extends Error {
  constructor(msg: string, public pos: number) { super(msg) }
}

type Tok =
  | { t: 'num'; v: number; p: number }
  | { t: 'id'; v: string; p: number }
  | { t: 'op'; v: string; p: number }
  | { t: 'lp' | 'rp' | 'comma'; p: number }

function lex(src: string): Tok[] {
  const toks: Tok[] = []
  let i = 0
  const two = ['<=', '>=', '==', '!=', '&&', '||']
  while (i < src.length) {
    const c = src[i]
    if (/\s/.test(c)) { i++; continue }
    if (c === '(') { toks.push({ t: 'lp', p: i }); i++; continue }
    if (c === ')') { toks.push({ t: 'rp', p: i }); i++; continue }
    if (c === ',') { toks.push({ t: 'comma', p: i }); i++; continue }
    const pair = src.slice(i, i + 2)
    if (two.includes(pair)) { toks.push({ t: 'op', v: pair, p: i }); i += 2; continue }
    if ('+-*/^<>!'.includes(c)) { toks.push({ t: 'op', v: c, p: i }); i++; continue }
    // Unicode operators a user may paste from the display string.
    if (c === '×') { toks.push({ t: 'op', v: '*', p: i }); i++; continue }
    if (c === '−' || c === '–') { toks.push({ t: 'op', v: '-', p: i }); i++; continue }
    if (c === '≤') { toks.push({ t: 'op', v: '<=', p: i }); i++; continue }
    if (c === '≥') { toks.push({ t: 'op', v: '>=', p: i }); i++; continue }
    if (c === '≠') { toks.push({ t: 'op', v: '!=', p: i }); i++; continue }
    if (/[0-9.]/.test(c)) {
      let j = i + 1
      while (j < src.length && /[0-9.eE+\-]/.test(src[j])) {
        // Stop unless the +/- is part of an exponent.
        if ((src[j] === '+' || src[j] === '-') && !/[eE]/.test(src[j - 1])) break
        j++
      }
      const num = Number(src.slice(i, j))
      if (isNaN(num)) throw new ParseError(`invalid number '${src.slice(i, j)}'`, i)
      toks.push({ t: 'num', v: num, p: i }); i = j; continue
    }
    if (/[A-Za-z_]/.test(c)) {
      let j = i + 1
      while (j < src.length && /[A-Za-z0-9_./]/.test(src[j])) j++
      toks.push({ t: 'id', v: src.slice(i, j), p: i }); i = j; continue
    }
    throw new ParseError(`unexpected character '${c}'`, i)
  }
  return toks
}

const INFIX: Record<string, { op: BinOp; lp: number; rp: number }> = {
  '||': { op: 'or', lp: 1, rp: 2 },
  '&&': { op: 'and', lp: 2, rp: 3 },
  '==': { op: 'eq', lp: 3, rp: 4 },
  '!=': { op: 'neq', lp: 3, rp: 4 },
  '<': { op: 'lt', lp: 3, rp: 4 },
  '>': { op: 'gt', lp: 3, rp: 4 },
  '<=': { op: 'lte', lp: 3, rp: 4 },
  '>=': { op: 'gte', lp: 3, rp: 4 },
  '+': { op: 'add', lp: 4, rp: 5 },
  '-': { op: 'subtract', lp: 4, rp: 5 },
  '*': { op: 'multiply', lp: 5, rp: 6 },
  '/': { op: 'divide', lp: 5, rp: 6 },
  '^': { op: 'power', lp: 7, rp: 7 }, // right-assoc
}

/**
 * Parse an expression to an AST. Identifiers resolve to `time_ref` (known time property),
 * a builtin `call` (when followed by `(`), or an element `ref` otherwise. Throws ParseError
 * with a position on malformed input.
 */
export function parseExpr(src: string): Ast {
  const toks = lex(src)
  let pos = 0
  const peek = () => toks[pos]
  const eof = () => pos >= toks.length

  function parse(minbp: number): Ast {
    let left = nud()
    while (!eof()) {
      const tk = peek()
      if (tk.t !== 'op') break
      const info = INFIX[tk.v]
      if (!info || info.lp < minbp) break
      pos++
      const right = parse(info.rp)
      left = { op: info.op, left, right }
    }
    return left
  }

  function nud(): Ast {
    if (eof()) throw new ParseError('unexpected end of expression', src.length)
    const tk = peek()
    if (tk.t === 'num') { pos++; return { op: 'literal', value: tk.v } }
    if (tk.t === 'op' && (tk.v === '-' || tk.v === '+')) {
      pos++
      const operand = parse(7)
      return tk.v === '-' ? { op: 'neg', operand } : operand
    }
    if (tk.t === 'op' && tk.v === '!') { pos++; return { op: 'not', operand: parse(7) } }
    if (tk.t === 'lp') {
      pos++
      const e = parse(0)
      expect('rp')
      return e
    }
    if (tk.t === 'id') {
      pos++
      const name = tk.v
      if (peek()?.t === 'lp') {
        // Function-style call.
        pos++
        const args: Ast[] = []
        if (peek()?.t !== 'rp') {
          args.push(parse(0))
          while (peek()?.t === 'comma') { pos++; args.push(parse(0)) }
        }
        expect('rp')
        if (name === 'if') {
          if (args.length !== 3) throw new ParseError('if(cond, then, else) needs 3 arguments', tk.p)
          return { op: 'if', cond: args[0], then: args[1], else: args[2] }
        }
        if (!BUILTIN_NAMES.has(name)) throw new ParseError(`unknown function '${name}'`, tk.p)
        return { op: 'call', fn: name, args }
      }
      if (TIME_REF_NAMES.has(name)) return { op: 'time_ref', property: name }
      return { op: 'ref', element_id: name }
    }
    throw new ParseError('unexpected token', tk.p)
  }

  function expect(t: Tok['t']) {
    const tk = peek()
    if (!tk || tk.t !== t) throw new ParseError(`expected ${t === 'rp' ? ')' : t}`, tk?.p ?? src.length)
    pos++
  }

  const ast = parse(0)
  if (!eof()) throw new ParseError('unexpected trailing input', peek().p)
  return ast
}

/** Parse to `{ ok, ast }` or `{ ok:false, error, pos }` — the editor's non-throwing entry. */
export function tryParseExpr(src: string): { ok: true; ast: Ast } | { ok: false; error: string; pos: number } {
  try {
    return { ok: true, ast: parseExpr(src) }
  } catch (e) {
    if (e instanceof ParseError) return { ok: false, error: e.message, pos: e.pos }
    return { ok: false, error: String(e), pos: 0 }
  }
}
