import { describe, it, expect, afterEach } from 'vitest'
import type { WorkerToMain, Validation } from '../worker/protocol'
import {
  validateCandidate,
  disposeValidateWorker,
  __setValidateWorkerFactory,
  type ValidateWorker,
} from './validateCandidate'

/**
 * The real dedicated worker + WASM engine (and the isolation guarantee that the live store's
 * `issues` are untouched) are exercised end-to-end in `e2e/copilot.spec.ts` — Web Workers and WASM
 * don't run in vitest's node environment. Here we unit-test the correlation logic with a fake
 * worker: request id → matching `validated` response → resolved promise, and worker errors →
 * error-shaped Validation.
 */

/** A fake worker that echoes each `validate` back as a `validated` with a caller-controlled result. */
class FakeWorker implements ValidateWorker {
  onmessage: ((e: { data: WorkerToMain }) => void) | null = null
  onerror: ((e: { message?: string }) => void) | null = null
  sent: { model: string; token: number }[] = []
  respond: (model: string) => Validation = () => ({ ok: true, issues: [], topo: [] })

  postMessage(msg: { type: 'validate'; model: string; token: number }): void {
    this.sent.push({ model: msg.model, token: msg.token })
    // Reply asynchronously, like a real worker, echoing the token back.
    queueMicrotask(() =>
      this.onmessage?.({ data: { type: 'validated', validation: this.respond(msg.model), token: msg.token } }),
    )
  }
  terminate(): void {}
}

afterEach(() => disposeValidateWorker())

describe('validateCandidate — correlation', () => {
  it('resolves with the validation matching its request', async () => {
    const fake = new FakeWorker()
    fake.respond = (model) =>
      model.includes('bad')
        ? { ok: false, issues: [{ severity: 'error', message: 'boom' }], topo: [] }
        : { ok: true, issues: [], topo: ['a'] }
    __setValidateWorkerFactory(() => fake)

    expect(await validateCandidate('{"good":1}')).toEqual({ ok: true, issues: [], topo: ['a'] })
    const bad = await validateCandidate('{"bad":1}')
    expect(bad.ok).toBe(false)
    expect(bad.issues[0].message).toBe('boom')
  })

  it('correlates overlapping requests by token (out-of-order replies)', async () => {
    const fake = new FakeWorker()
    // Hold replies and flush them in reverse, to prove token-keyed correlation (not FIFO).
    const queued: (() => void)[] = []
    fake.postMessage = (msg) => {
      fake.sent.push({ model: msg.model, token: msg.token })
      queued.push(() =>
        fake.onmessage?.({
          data: { type: 'validated', validation: { ok: true, issues: [], topo: [msg.model] }, token: msg.token },
        }),
      )
    }
    __setValidateWorkerFactory(() => fake)

    const p1 = validateCandidate('one')
    const p2 = validateCandidate('two')
    queued.reverse().forEach((fn) => fn()) // reply to two, then one
    expect((await p1).topo).toEqual(['one'])
    expect((await p2).topo).toEqual(['two'])
  })

  it('turns a worker error into an error-shaped Validation (never rejects)', async () => {
    const fake = new FakeWorker()
    fake.postMessage = () => queueMicrotask(() => fake.onmessage?.({ data: { type: 'error', message: 'wasm blew up' } }))
    __setValidateWorkerFactory(() => fake)

    const v = await validateCandidate('{"x":1}')
    expect(v.ok).toBe(false)
    expect(v.issues[0].message).toBe('wasm blew up')
  })
})
