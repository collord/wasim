/// <reference lib="webworker" />
import type { MainToWorker, WorkerToMain } from './protocol'
import type { WasmEngine } from '@engine/wasim_engine'

let engine: WasmEngine | null = null

async function getWasmEngine(modelJson: string): Promise<WasmEngine> {
  const mod = await import('@engine/wasim_engine').catch(() => {
    throw new Error('Failed to load WASM engine. Run ./engine/build-wasm.sh first.')
  })
  await mod.default() // init()
  return new mod.WasmEngine(modelJson)
}

function post(msg: WorkerToMain) {
  self.postMessage(msg)
}

self.onmessage = async (e: MessageEvent<MainToWorker>) => {
  const msg = e.data
  try {
    switch (msg.type) {
      case 'load_model': {
        engine?.free()
        engine = null
        engine = await getWasmEngine(msg.payload)
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
    }
  } catch (err) {
    post({ type: 'error', message: String(err) })
  }
}
