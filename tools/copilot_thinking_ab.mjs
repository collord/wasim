#!/usr/bin/env node
/**
 * A/B the copilot's generation quality with adaptive thinking on vs off, to set an *evidence-based*
 * default for `DEFAULT_THINKING` (frontend/src/copilot/aiConfig.ts) rather than asserting one.
 *
 * It mirrors the real copilot loop (frontend/src/copilot/loop.ts): draft a model via the Anthropic
 * Messages API, validate it with the engine (`wasim-validate`), feed diagnostics back, retry to a
 * cap. It runs each prompt twice — thinking adaptive vs omitted — and reports validate-iterations
 * to convergence (and whether it converged at all).
 *
 * Requires ANTHROPIC_API_KEY and a built `wasim-validate`:
 *   cargo build --bin wasim-validate            (from engine/)
 *   ANTHROPIC_API_KEY=… node tools/copilot_thinking_ab.mjs
 */
import { execFileSync } from 'node:child_process'
import { readFileSync, writeFileSync, mkdtempSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)))
const VALIDATE = join(ROOT, 'engine', 'target', 'debug', 'wasim-validate')
const GUIDE = readFileSync(join(ROOT, 'frontend', 'src', 'copilot', 'authoring-guide.generated.md'), 'utf8')
const MODEL = process.env.AB_MODEL || 'claude-opus-4-8'
const MAX_ITERS = 5
const PROMPTS = [
  'a two-tank system where the first tank is filled at a constant rate and overflows into a second tank that drains to a creek',
  'a 30-year retirement projection: a savings balance, an uncertain annual return drawn from a normal distribution, and a fixed annual withdrawal',
  'a repairable pump that fails randomly (exponential time-to-failure) and gets repaired, tracked as available vs failed over time',
]

const SYSTEM = `${GUIDE}\n\nReturn ONE complete WASiM v2 model as a single JSON object. If given validation diagnostics, fix exactly those issues and return the corrected full model. Output only the JSON (a fenced code block is fine).`

function extractJson(text) {
  const fence = text.match(/```(?:json)?\s*([\s\S]*?)```/i)
  const h = fence ? fence[1] : text
  const start = h.indexOf('{')
  if (start === -1) throw new Error('no JSON')
  let d = 0, s = false, e = false
  for (let i = start; i < h.length; i++) {
    const c = h[i]
    if (s) { if (e) e = false; else if (c === '\\') e = true; else if (c === '"') s = false; continue }
    if (c === '"') s = true
    else if (c === '{') d++
    else if (c === '}') { if (--d === 0) return h.slice(start, i + 1) }
  }
  throw new Error('unbalanced JSON')
}

function validate(json) {
  const tmp = join(mkdtempSync(join(tmpdir(), 'ab-')), 'm.json')
  writeFileSync(tmp, json)
  try {
    execFileSync(VALIDATE, [tmp], { stdio: ['ignore', 'pipe', 'pipe'] })
    return { ok: true, issues: [] }
  } catch (err) {
    const out = err.stdout?.toString() || '{}'
    try {
      const rep = JSON.parse(out)
      return { ok: false, issues: rep.errors || ['(parse failure)'] }
    } catch {
      return { ok: false, issues: [String(err)] }
    }
  }
}

async function call(system, messages, thinking) {
  const body = { model: MODEL, max_tokens: 8000, system, messages }
  if (thinking) body.thinking = { type: 'adaptive' }
  const res = await fetch('https://api.anthropic.com/v1/messages', {
    method: 'POST',
    headers: { 'content-type': 'application/json', 'x-api-key': process.env.ANTHROPIC_API_KEY, 'anthropic-version': '2023-06-01' },
    body: JSON.stringify(body),
  })
  if (!res.ok) throw new Error(`API ${res.status}: ${(await res.text()).slice(0, 200)}`)
  const data = await res.json()
  return (data.content || []).filter((b) => b.type === 'text').map((b) => b.text).join('')
}

async function runOne(description, thinking) {
  const messages = [{ role: 'user', content: description }]
  for (let i = 1; i <= MAX_ITERS; i++) {
    const raw = await call(SYSTEM, messages, thinking)
    messages.push({ role: 'assistant', content: raw })
    let json
    try { json = extractJson(raw) } catch (e) {
      messages.push({ role: 'user', content: `No JSON found (${e.message}). Return the full model as JSON.` })
      continue
    }
    const v = validate(json)
    if (v.ok) return { iters: i, ok: true }
    messages.push({ role: 'user', content: `Validation failed:\n${v.issues.map((s) => `- ${s}`).join('\n')}\nFix and return the full model.` })
  }
  return { iters: MAX_ITERS, ok: false }
}

if (!process.env.ANTHROPIC_API_KEY) {
  console.error('Set ANTHROPIC_API_KEY to run the A/B.')
  process.exit(2)
}

const rows = []
for (const p of PROMPTS) {
  for (const thinking of [false, true]) {
    process.stderr.write(`running: thinking=${thinking} · ${p.slice(0, 40)}…\n`)
    const r = await runOne(p, thinking)
    rows.push({ thinking, ok: r.ok, iters: r.iters, prompt: p.slice(0, 40) })
  }
}

console.log('\nthinking | ok | iters | prompt')
for (const r of rows) console.log(`${r.thinking ? ' on ' : ' off'}    | ${r.ok ? '✓' : '✗'}  |   ${r.iters}   | ${r.prompt}`)
const avg = (t) => {
  const g = rows.filter((r) => r.thinking === t)
  return (g.reduce((a, r) => a + r.iters, 0) / g.length).toFixed(2)
}
console.log(`\navg iters — off: ${avg(false)}  on: ${avg(true)}`)
console.log('→ set DEFAULT_THINKING to the lower-iteration / higher-validity winner (or false if a tie: cheaper/faster).')
