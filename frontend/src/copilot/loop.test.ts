import { describe, it, expect, vi } from 'vitest'
import type { Validation } from '../worker/protocol'
import { propose, type ProposeDeps } from './loop'

const CFG = { apiKey: 'k', model: 'claude-opus-4-8' }
const OK: Validation = { ok: true, issues: [], topo: [] }
const BAD: Validation = { ok: false, issues: [{ severity: 'error', message: 'unknown field foo' }], topo: [] }

describe('propose — the agentic loop', () => {
  it('converges on the first valid model', async () => {
    const deps: ProposeDeps = {
      call: vi.fn().mockResolvedValue('{"wasim_version":"0.1.0"}'),
      validate: vi.fn().mockResolvedValue(OK),
    }
    const r = await propose('a bathtub', CFG, 'GUIDE', deps)
    expect(r.iterations).toBe(1)
    expect(r.validation.ok).toBe(true)
    expect(deps.call).toHaveBeenCalledTimes(1)
  })

  it('feeds diagnostics back and retries until valid', async () => {
    const call = vi
      .fn()
      .mockResolvedValueOnce('{"bad":1}')
      .mockResolvedValueOnce('{"good":1}')
    const validate = vi.fn().mockResolvedValueOnce(BAD).mockResolvedValueOnce(OK)
    const r = await propose('x', CFG, 'GUIDE', { call, validate })

    expect(r.iterations).toBe(2)
    expect(r.validation.ok).toBe(true)
    // By the 2nd call the conversation carries the assistant's bad draft + a diagnostics user turn,
    // so the model corrects its own prior JSON. (The array is passed by reference and keeps growing,
    // so assert on the first three turns rather than exact length.)
    const secondCallMessages = call.mock.calls[1][2] as { role: string; content: string }[]
    expect(secondCallMessages.slice(0, 3).map((m) => m.role)).toEqual(['user', 'assistant', 'user'])
    expect(secondCallMessages[2].content).toMatch(/unknown field foo/)
  })

  it('caps at maxIterations and returns the best-effort attempt', async () => {
    const deps: ProposeDeps = {
      call: vi.fn().mockResolvedValue('{"still":"bad"}'),
      validate: vi.fn().mockResolvedValue(BAD),
    }
    const r = await propose('x', CFG, 'GUIDE', deps, { maxIterations: 3 })
    expect(r.iterations).toBe(3)
    expect(r.validation.ok).toBe(false)
    expect(deps.call).toHaveBeenCalledTimes(3)
    expect(r.model).toBe('{"still":"bad"}') // best-effort JSON is still returned for display
  })

  it('treats an unparseable response as a validation failure and retries', async () => {
    const call = vi
      .fn()
      .mockResolvedValueOnce('sorry, I cannot help with that')
      .mockResolvedValueOnce('{"good":1}')
    const validate = vi.fn().mockResolvedValue(OK)
    const r = await propose('x', CFG, 'GUIDE', { call, validate })

    expect(r.iterations).toBe(2)
    expect(r.validation.ok).toBe(true)
    // validate() only ran once — the first response had no JSON to validate.
    expect(validate).toHaveBeenCalledTimes(1)
  })

  it('reports progress via onIteration', async () => {
    const deps: ProposeDeps = {
      call: vi.fn().mockResolvedValue('{"a":1}'),
      validate: vi.fn().mockResolvedValue(OK),
    }
    const seen: [number, boolean | null][] = []
    await propose('x', CFG, 'GUIDE', deps, { onIteration: (n, v) => seen.push([n, v ? v.ok : null]) })
    expect(seen).toEqual([
      [1, null], // before the call
      [1, true], // after validation
    ])
  })

  it('honors an abort signal', async () => {
    const ctrl = new AbortController()
    ctrl.abort()
    const deps: ProposeDeps = { call: vi.fn(), validate: vi.fn() }
    await expect(propose('x', CFG, 'GUIDE', deps, { signal: ctrl.signal })).rejects.toThrow(/abort/i)
    expect(deps.call).not.toHaveBeenCalled()
  })
})
