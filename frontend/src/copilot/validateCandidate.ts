import type { Validation, WorkerToMain } from '../worker/protocol'

/**
 * Validate a candidate model JSON through the WASM engine WITHOUT touching the app's live document
 * (WASIM_AUTHORING_ENVIRONMENT_SPEC.md §17.3 `validate()`). The engine is the copilot's ground
 * truth: the loop drafts a model, this reports structured diagnostics, and the LLM self-corrects.
 *
 * It runs in a **dedicated** worker instance, separate from the app's sim worker. The worker's
 * `validate` handler is stateless (it only calls the module-level `validate_json`, never touching
 * the live engine), but the *store* routes `validate`/`reconcile` responses into shared
 * `issues`/`topo`/`valid` fields keyed by `_reconcileToken`. A dedicated worker with its own
 * onmessage sidesteps that entirely — candidate validation can never clobber the live issues panel
 * or race the reconcile loop.
 */

/** The subset of `Worker` this helper uses. Lets a test inject a fake without a DOM. */
export interface ValidateWorker {
  postMessage(msg: { type: 'validate'; model: string; token: number }): void
  onmessage: ((e: { data: WorkerToMain }) => void) | null
  onerror: ((e: { message?: string }) => void) | null
  terminate(): void
}

/** The default factory: a real dedicated module worker. Vite resolves the `new URL(...)` at build. */
function defaultWorkerFactory(): ValidateWorker {
  return new Worker(new URL('../worker/sim.worker.ts', import.meta.url), { type: 'module' }) as unknown as ValidateWorker
}

let _factory: () => ValidateWorker = defaultWorkerFactory
let _worker: ValidateWorker | null = null
let _seq = 0
const _pending = new Map<number, (v: Validation) => void>()

function failAllPending(message: string): void {
  const err: Validation = { ok: false, issues: [{ severity: 'error', message }], topo: [] }
  for (const [, resolve] of _pending) resolve(err)
  _pending.clear()
}

function getWorker(): ValidateWorker {
  if (!_worker) {
    _worker = _factory()
    _worker.onmessage = (e: { data: WorkerToMain }) => {
      const msg = e.data
      if (msg.type === 'validated') {
        const resolve = _pending.get(msg.token)
        if (resolve) {
          _pending.delete(msg.token)
          resolve(msg.validation)
        }
      } else if (msg.type === 'error') {
        // The worker only errors on a thrown exception in the handler; fail everything in flight.
        failAllPending(msg.message)
      }
    }
    _worker.onerror = (e) => failAllPending(e.message ?? 'validation worker error')
  }
  return _worker
}

/** Validate a candidate model JSON. Resolves with the engine's diagnostics; never rejects (engine
 *  / worker failures come back as an error-shaped `Validation` so the loop can feed them to the LLM). */
export function validateCandidate(json: string): Promise<Validation> {
  const token = ++_seq
  return new Promise<Validation>((resolve) => {
    _pending.set(token, resolve)
    getWorker().postMessage({ type: 'validate', model: json, token })
  })
}

/** Override the worker factory (tests only). Disposes any existing worker first. */
export function __setValidateWorkerFactory(factory: () => ValidateWorker): void {
  disposeValidateWorker()
  _factory = factory
}

/** Tear down the dedicated worker (tests / teardown). Safe to call when none exists. */
export function disposeValidateWorker(): void {
  _worker?.terminate()
  _worker = null
  _pending.clear()
}
