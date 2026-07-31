import { describe, it, expect, vi } from 'vitest'
import type { ModelDoc } from '../model/schema'
import type { Validation } from '../worker/protocol'
import type { ChatResult } from './providers/tools'
import { proposeWithTools, type ToolProposeDeps } from './toolLoop'

const OK: Validation = { ok: true, issues: [], topo: [] }
const BAD: Validation = { ok: false, issues: [{ severity: 'error', message: 'dangling ref' }], topo: [] }
const CFG = { provider: 'anthropic', model: 'claude-opus-4-8', apiKey: 'k' } as const
const START = { wasim_version: '0.1.0', simulation_settings: {}, elements: [] } as unknown as ModelDoc

function toolUse(id: string, input: Record<string, unknown>): ChatResult {
  return { text: '', toolCalls: [{ id, name: 'apply_edit', input }], stopReason: 'tool_use' }
}
const END: ChatResult = { text: 'done', toolCalls: [], stopReason: 'end' }

describe('proposeWithTools', () => {
  it('applies one edit then terminates on stopReason end', async () => {
    const call = vi.fn().mockResolvedValueOnce(toolUse('t1', { op: 'add' })).mockResolvedValueOnce(END)
    const execute = vi.fn().mockReturnValue({ doc: { ...START, tagged: true } as unknown as ModelDoc })
    const deps: ToolProposeDeps = { call, validate: vi.fn().mockResolvedValue(OK), execute }
    const r = await proposeWithTools('add rain', START, CFG, 'SYS', [], deps)

    expect(r.iterations).toBe(2)
    expect(r.validation.ok).toBe(true)
    expect(r.edits).toEqual([{ op: 'add', id: undefined, error: undefined, validation: OK }])
    expect(execute).toHaveBeenCalledTimes(1)
  })

  it('feeds the assistant tool_use turn + a tool_result turn into the next call', async () => {
    const call = vi.fn().mockResolvedValueOnce(toolUse('t1', { op: 'add', id: 'rain' })).mockResolvedValueOnce(END)
    const deps: ToolProposeDeps = {
      call,
      validate: vi.fn().mockResolvedValue(OK),
      execute: vi.fn().mockReturnValue({ doc: START }),
    }
    await proposeWithTools('x', START, CFG, 'SYS', [], deps)

    const secondMessages = call.mock.calls[1][2] as { role: string; content: unknown }[]
    expect(secondMessages[1]).toEqual({ role: 'assistant', content: [{ type: 'tool_use', id: 't1', name: 'apply_edit', input: { op: 'add', id: 'rain' } }] })
    expect(secondMessages[2]).toMatchObject({ role: 'user', content: [{ type: 'tool_result', toolUseId: 't1' }] })
  })

  it('applies multiple tool calls in one turn sequentially', async () => {
    const call = vi
      .fn()
      .mockResolvedValueOnce({ text: '', toolCalls: [{ id: 'a', name: 'apply_edit', input: { op: 'add' } }, { id: 'b', name: 'apply_edit', input: { op: 'modify' } }], stopReason: 'tool_use' } satisfies ChatResult)
      .mockResolvedValueOnce(END)
    const execute = vi.fn().mockReturnValue({ doc: START })
    const deps: ToolProposeDeps = { call, validate: vi.fn().mockResolvedValue(OK), execute }
    const r = await proposeWithTools('x', START, CFG, 'SYS', [], deps)
    expect(execute).toHaveBeenCalledTimes(2)
    expect(r.edits).toHaveLength(2)
  })

  it('feeds an executor error back and keeps looping (model can fix)', async () => {
    const call = vi.fn().mockResolvedValueOnce(toolUse('t1', { op: 'delete', id: 'ghost' })).mockResolvedValueOnce(END)
    const execute = vi.fn().mockReturnValue({ doc: START, error: 'no element "ghost"' })
    const validate = vi.fn().mockResolvedValue(OK)
    const deps: ToolProposeDeps = { call, validate, execute }
    const r = await proposeWithTools('x', START, CFG, 'SYS', [], deps)

    expect(r.edits[0].error).toMatch(/ghost/)
    expect(validate).not.toHaveBeenCalled() // errored edit isn't validated (doc unchanged)
    // The tool_result fed back is flagged isError.
    const secondMessages = call.mock.calls[1][2] as { role: string; content: { isError?: boolean }[] }[]
    expect((secondMessages[2].content[0]).isError).toBe(true)
  })

  it('reports invalid edits (validation fails) but keeps going', async () => {
    const call = vi.fn().mockResolvedValueOnce(toolUse('t1', { op: 'add' })).mockResolvedValueOnce(END)
    const deps: ToolProposeDeps = { call, validate: vi.fn().mockResolvedValue(BAD), execute: vi.fn().mockReturnValue({ doc: START }) }
    const r = await proposeWithTools('x', START, CFG, 'SYS', [], deps)
    expect(r.validation.ok).toBe(false)
    expect(r.edits[0].validation.ok).toBe(false)
  })

  it('caps at maxIterations', async () => {
    const call = vi.fn().mockResolvedValue(toolUse('t', { op: 'add' })) // always wants more tools
    const deps: ToolProposeDeps = { call, validate: vi.fn().mockResolvedValue(OK), execute: vi.fn().mockReturnValue({ doc: START }) }
    const r = await proposeWithTools('x', START, CFG, 'SYS', [], deps, { maxIterations: 3 })
    expect(r.iterations).toBe(3)
    expect(call).toHaveBeenCalledTimes(3)
  })

  it('honors an abort signal', async () => {
    const ctrl = new AbortController()
    ctrl.abort()
    const deps: ToolProposeDeps = { call: vi.fn(), validate: vi.fn(), execute: vi.fn() }
    await expect(proposeWithTools('x', START, CFG, 'SYS', [], deps, { signal: ctrl.signal })).rejects.toThrow(/abort/i)
    expect(deps.call).not.toHaveBeenCalled()
  })
})
