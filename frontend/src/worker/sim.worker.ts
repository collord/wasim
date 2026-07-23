/// <reference lib="webworker" />
import type { Issue, MainToWorker, Validation, WorkerToMain } from './protocol'
import type { WasmEngine } from '@engine/wasim_engine'

let engine: WasmEngine | null = null

type EngineModule = {
  default: () => Promise<void>
  WasmEngine: new (json: string) => WasmEngine
  validate_json: (json: string) => string
}

let modPromise: Promise<EngineModule> | null = null
async function getMod(): Promise<EngineModule> {
  if (!modPromise) {
    modPromise = import('@engine/wasim_engine')
      .catch(() => {
        throw new Error('Failed to load WASM engine. Run ./engine/build-wasm.sh first.')
      })
      .then(async (mod) => {
        await (mod as unknown as EngineModule).default()
        return mod as unknown as EngineModule
      })
  }
  return modPromise
}

function post(msg: WorkerToMain) {
  self.postMessage(msg)
}

/** Run validate_json → structured Validation. Engine-truth for the issues panel (§8). */
function validate(mod: EngineModule, json: string): Validation {
  try {
    const raw = JSON.parse(mod.validate_json(json)) as {
      ok: boolean; errors: string[]; warnings: string[]; topo: string[]
    }
    const issues: Issue[] = [
      ...raw.errors.map((message) => ({ severity: 'error' as const, message, element_id: guessElement(message) })),
      ...raw.warnings.map((message) => ({ severity: 'warning' as const, message, element_id: guessElement(message) })),
    ]
    return { ok: raw.ok, issues, topo: raw.topo }
  } catch (e) {
    return { ok: false, issues: [{ severity: 'error', message: String(e) }], topo: [] }
  }
}

// Engine messages often quote the offending element id in single quotes ('id'); surface it
// so the issues panel can jump-to-element.
function guessElement(message: string): string | null {
  const m = message.match(/'([^']+)'/)
  return m ? m[1] : null
}

self.onmessage = async (e: MessageEvent<MainToWorker>) => {
  const msg = e.data
  try {
    switch (msg.type) {
      case 'load_model': {
        const mod = await getMod()
        engine?.free()
        engine = null
        engine = new mod.WasmEngine(msg.payload)
        const summary = JSON.parse(engine.model_summary())
        post({ type: 'model_loaded', summary })
        break
      }
      case 'set_constant':
        engine?.set_constant(msg.element_id, msg.value)
        break
      case 'set_rv_param':
        engine?.set_rv_param(msg.element_id, msg.param_name, msg.value)
        break
      case 'run': {
        if (!engine) throw new Error('no model loaded')
        const results = JSON.parse(engine.run_json(JSON.stringify(msg.config)))
        post({ type: 'complete', results })
        break
      }
      case 'run_sensitivity': {
        if (!engine) throw new Error('no model loaded')
        const results = JSON.parse(engine.sensitivity_json(JSON.stringify(msg.spec)))
        post({ type: 'sensitivity_complete', results })
        break
      }
      case 'run_optimization': {
        if (!engine) throw new Error('no model loaded')
        const results = JSON.parse(engine.optimize_json(JSON.stringify(msg.spec)))
        post({ type: 'optimization_complete', results })
        break
      }
      case 'reconcile': {
        // A structural edit: rebuild the run engine from the whole model so a single source
        // of schema truth stays in Rust (§13.2). Validation comes first (non-throwing), then
        // — if the graph is sound — a fresh engine + summary. Bad models still return issues.
        const mod = await getMod()
        const validation = validate(mod, msg.model)
        let summary = null
        if (validation.ok) {
          try {
            engine?.free()
            engine = null
            engine = new mod.WasmEngine(msg.model)
            summary = JSON.parse(engine.model_summary())
          } catch (err) {
            validation.ok = false
            validation.issues.unshift({ severity: 'error', message: String(err) })
          }
        }
        post({ type: 'reconciled', summary, validation, token: msg.token })
        break
      }
      case 'validate': {
        const mod = await getMod()
        post({ type: 'validated', validation: validate(mod, msg.model), token: msg.token })
        break
      }
      case 'llm_validate': {
        const mod = await getMod()
        post({ type: 'llm_validated', validation: validate(mod, msg.model), token: msg.token })
        break
      }
    }
  } catch (err) {
    post({ type: 'error', message: String(err) })
  }
}
