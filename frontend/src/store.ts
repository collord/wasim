import { create } from 'zustand'
import type { MainToWorker, WorkerToMain } from './worker/protocol'
import type { ModelJson, ModelSummary, SimulationResults } from './types'

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
  parsedModel: ModelJson | null
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
  simDuration: number | null
  simDurationUnit: string
  simTimestep: number | null
  simTimestepUnit: string

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
  parsedModel: null,
  modelSummary: null,
  activeTab: 'dashboard',
  status: 'idle',
  errorMessage: null,
  results: null,
  selectedResultId: null,
  nRealizations: 1000,
  seed: 42,
  simDuration: null,
  simDurationUnit: 'yr',
  simTimestep: null,
  simTimestepUnit: 'yr',

  loadModel(json, filename) {
    let parsed: ModelJson | null = null
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
      parsedModel: parsed,
      modelSummary: null,
      status: 'idle',
      results: null,
      selectedResultId: null,
      errorMessage: null,
      simDuration: ss.duration.value,
      simDurationUnit: ss.duration.unit,
      simTimestep: ss.timestep.value,
      simTimestepUnit: ss.timestep.unit,
    })
    postToWorker({ type: 'load_model', payload: json })
  },

  setActiveTab: (tab) => set({ activeTab: tab }),

  setConstant(id, value) {
    // Update local parsedModel so the form reflects the edit immediately
    const pm = get().parsedModel
    if (!pm) return
    const elements = pm.elements.map((e) => {
      if (e.id === id && e.type === 'constant') {
        const c = e as import('./types').ConstantElement
        return { ...c, value: { ...c.value, value } }
      }
      return e
    })
    set({ parsedModel: { ...pm, elements } })
    postToWorker({ type: 'set_constant', element_id: id, value })
  },

  setRvParam(id, param, value) {
    // Update local parsedModel distribution parameter
    const pm = get().parsedModel
    if (!pm) return
    const elements = pm.elements.map((e) => {
      if (e.id !== id || e.type !== 'random_variable') return e
      const rv = e as import('./types').RandomVariableElement
      const oldParam = rv.distribution.parameters[param]
      const updatedParam =
        typeof oldParam === 'object' && oldParam !== null
          ? { ...oldParam, value }
          : value
      return {
        ...rv,
        distribution: {
          ...rv.distribution,
          parameters: { ...rv.distribution.parameters, [param]: updatedParam },
        },
      }
    })
    set({ parsedModel: { ...pm, elements } })
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
  setSimDuration: (v) => set({ simDuration: v }),
  setSimTimestep: (v) => set({ simTimestep: v }),
  setSelectedResultId: (id) => set({ selectedResultId: id }),

  saveParameters() {
    const { parsedModel, modelFilename, nRealizations, seed, simDuration, simTimestep } = get()
    if (!parsedModel) return

    const constants: Record<string, number> = {}
    const rv_params: Record<string, Record<string, number>> = {}

    for (const elem of parsedModel.elements) {
      if (elem.type === 'constant' && (elem as import('./types').ConstantElement).editable) {
        const c = elem as import('./types').ConstantElement
        constants[c.id] = c.value.value
      } else if (elem.type === 'random_variable') {
        const rv = elem as import('./types').RandomVariableElement
        const params: Record<string, number> = {}
        for (const [k, v] of Object.entries(rv.distribution.parameters)) {
          params[k] = typeof v === 'number' ? v : (v as { value: number }).value
        }
        rv_params[rv.id] = params
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
        set({ modelSummary: msg.summary, activeTab: 'graph' })
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
