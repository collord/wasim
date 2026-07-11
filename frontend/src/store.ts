import { create } from 'zustand'
import type { MainToWorker, WorkerToMain } from './worker/protocol'
import type { ModelJson, ModelSummary, QtyDisplay, SimulationResults } from './types'

const IDENTITY_DISP: QtyDisplay = { unit: '', factor: 1, offset: 0 }

// ── Worker singleton ──────────────────────────────────────────────────────────

let _worker: Worker | null = null

function getWorker(): Worker {
  if (!_worker) {
    _worker = new Worker(new URL('./worker/sim.worker.ts', import.meta.url), {
      type: 'module',
    })
    _worker.onmessage = (e: MessageEvent<WorkerToMain>) => {
      useStore.getState()._onWorkerMessage(e.data)
    }
    _worker.onerror = (e) => {
      useStore.getState()._onWorkerMessage({
        type: 'error',
        message: e.message ?? 'worker error',
      })
    }
  }
  return _worker
}

function postToWorker(msg: MainToWorker) {
  getWorker().postMessage(msg)
}

// ── Store shape ───────────────────────────────────────────────────────────────

export type Tab = 'graph' | 'model' | 'dashboard' | 'results'
export type SimStatus = 'idle' | 'running' | 'done' | 'error'

interface State {
  // Model
  modelJson: string | null
  modelFilename: string | null
  modelSummary: ModelSummary | null

  // Active tab
  activeTab: Tab

  // Simulation
  status: SimStatus
  errorMessage: string | null

  // Results
  results: SimulationResults | null
  selectedResultId: string | null

  // Run config (user-controlled)
  nRealizations: number
  seed: number | null
  // Canonical (engine-facing) duration/timestep. The dashboard shows/edits them in the
  // declared display unit via the *Disp mappings; these values stay canonical.
  simDuration: number | null
  simTimestep: number | null
  simDurationDisp: QtyDisplay
  simTimestepDisp: QtyDisplay

  // Internal
  _onWorkerMessage: (msg: WorkerToMain) => void
}

interface Actions {
  loadModel: (json: string, filename?: string) => void
  setActiveTab: (tab: Tab) => void
  setConstant: (id: string, value: number) => void
  setRvParam: (id: string, param: string, value: number) => void
  run: () => void
  setNRealizations: (n: number) => void
  setSeed: (s: number | null) => void
  setSimDuration: (v: number) => void
  setSimTimestep: (v: number) => void
  setSelectedResultId: (id: string) => void
  saveParameters: () => void
}

// ── Store ─────────────────────────────────────────────────────────────────────

export const useStore = create<State & Actions>((set, get) => ({
  modelJson: null,
  modelFilename: null,
  modelSummary: null,
  activeTab: 'dashboard',
  status: 'idle',
  errorMessage: null,
  results: null,
  selectedResultId: null,
  nRealizations: 1000,
  seed: 42,
  simDuration: null,
  simTimestep: null,
  simDurationDisp: IDENTITY_DISP,
  simTimestepDisp: IDENTITY_DISP,

  loadModel(json, filename) {
    // Lightweight parse for sim-settings only (top level is format-agnostic, v1 or v2).
    // Element rendering/editing is driven entirely by the engine's model_summary.
    let parsed: ModelJson
    try {
      parsed = JSON.parse(json) as ModelJson
    } catch {
      set({ status: 'error', errorMessage: 'Invalid JSON' })
      return
    }
    const ss = parsed.simulation_settings
    set({
      modelJson: json,
      modelFilename: filename ?? null,
      modelSummary: null,
      status: 'idle',
      results: null,
      selectedResultId: null,
      errorMessage: null,
      simDuration: ss.duration.value,
      simTimestep: ss.timestep.value,
      // Disp mappings reset to identity until the engine summary arrives (model_loaded).
      simDurationDisp: IDENTITY_DISP,
      simTimestepDisp: IDENTITY_DISP,
    })
    postToWorker({ type: 'load_model', payload: json })
  },

  setActiveTab: (tab) => set({ activeTab: tab }),

  setConstant(id, value) {
    // Echo the edit into the summary so the form reflects it immediately.
    const sm = get().modelSummary
    if (sm) {
      set({
        modelSummary: {
          ...sm,
          elements: sm.elements.map((e) => (e.id === id ? { ...e, value } : e)),
        },
      })
    }
    postToWorker({ type: 'set_constant', element_id: id, value })
  },

  setRvParam(id, param, value) {
    const sm = get().modelSummary
    if (sm) {
      set({
        modelSummary: {
          ...sm,
          elements: sm.elements.map((e) => {
            if (e.id !== id || !e.dist) return e
            const old = e.dist.parameters[param]
            const updated =
              typeof old === 'object' && old !== null ? { ...old, value } : value
            return {
              ...e,
              dist: { ...e.dist, parameters: { ...e.dist.parameters, [param]: updated } },
            }
          }),
        },
      })
    }
    postToWorker({ type: 'set_rv_param', element_id: id, param_name: param, value })
  },

  run() {
    const { nRealizations, seed, simDuration, simTimestep } = get()
    set({ status: 'running', errorMessage: null })
    postToWorker({
      type: 'run',
      config: {
        n_realizations: nRealizations,
        seed: seed ?? undefined,
        duration_override: simDuration ?? undefined,
        timestep_override: simTimestep ?? undefined,
      },
    })
  },

  setNRealizations: (n) => set({ nRealizations: n }),
  setSeed: (s) => set({ seed: s }),
  // Inputs are in display units; store canonical (canonical = (display - offset) / factor).
  setSimDuration: (v) => {
    const d = get().simDurationDisp
    set({ simDuration: (v - d.offset) / d.factor })
  },
  setSimTimestep: (v) => {
    const d = get().simTimestepDisp
    set({ simTimestep: (v - d.offset) / d.factor })
  },
  setSelectedResultId: (id) => set({ selectedResultId: id }),

  saveParameters() {
    const { modelSummary, modelFilename, nRealizations, seed, simDuration, simTimestep } = get()
    if (!modelSummary) return

    const constants: Record<string, number> = {}
    const rv_params: Record<string, Record<string, number>> = {}

    for (const e of modelSummary.elements) {
      if (e.value_rule === 'fixed' && e.editable && e.value !== null) {
        constants[e.id] = e.value
      } else if (e.value_rule === 'sample' && e.dist) {
        const params: Record<string, number> = {}
        for (const [k, v] of Object.entries(e.dist.parameters)) {
          params[k] = typeof v === 'number' ? v : (v as { value: number }).value
        }
        rv_params[e.id] = params
      }
    }

    const paramsJson = JSON.stringify(
      {
        constants,
        rv_params,
        run_config: {
          n_realizations: nRealizations,
          ...(seed !== null ? { seed } : {}),
          ...(simDuration !== null ? { duration_override: simDuration } : {}),
          ...(simTimestep !== null ? { timestep_override: simTimestep } : {}),
        },
      },
      null,
      2,
    )

    const stem = modelFilename
      ? modelFilename.replace(/\.json$/i, '')
      : 'model'
    const filename = `${stem}.params.json`

    const blob = new Blob([paramsJson], { type: 'application/json' })
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = filename
    a.click()
    URL.revokeObjectURL(url)
  },

  _onWorkerMessage(msg) {
    switch (msg.type) {
      case 'model_loaded':
        set({
          modelSummary: msg.summary,
          activeTab: 'graph',
          // Display mappings arrive with the summary (engine-computed); the dashboard
          // shows/edits duration & timestep in these units while the store stays canonical.
          simDurationDisp: msg.summary.time_display.duration,
          simTimestepDisp: msg.summary.time_display.timestep,
        })
        break

      case 'complete': {
        set({
          status: 'done',
          results: msg.results,
          selectedResultId: msg.results.output_ids[0] ?? null,
          activeTab: 'results',
        })
        break
      }

      case 'error':
        set({ status: 'error', errorMessage: msg.message })
        break
    }
  },
}))
