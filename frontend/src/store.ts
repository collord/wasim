import { create } from 'zustand'
import type { Issue, MainToWorker, Validation, WorkerToMain } from './worker/protocol'
import type {
  ModelJson,
  ModelSummary,
  OptimizationSpec,
  QtyDisplay,
  ResultsSpec,
  SensitivityResults,
  SensitivitySpec,
  SimulationResults,
  StudyResults,
} from './types'
import type { FlatElement, ModelDoc, ModelFormat } from './model/schema'
import { detectFormat } from './model/schema'
import type { LlmConfig } from './llm/config'
import { loadLlmConfig, saveLlmConfig } from './llm/config'
import { getProvider } from './llm/provider'
import { runCopilot } from './llm/copilot'
import {
  addElement, blankModel, deleteElement, duplicateElement, mutateElement, renameId,
  serializeModel, setContainer, setPosition, setPositions, toggleDashboard, updateElement,
  updateSettings, uniqueId,
} from './model/edits'
import type { NodeView } from './model/schema'

const IDENTITY_DISP: QtyDisplay = { unit: '', factor: 1, offset: 0 }
const RECONCILE_DEBOUNCE_MS = 250
const HISTORY_LIMIT = 100

// ── Worker singleton ──────────────────────────────────────────────────────────

let _worker: Worker | null = null

function getWorker(): Worker {
  if (!_worker) {
    _worker = new Worker(new URL('./worker/sim.worker.ts', import.meta.url), { type: 'module' })
    _worker.onmessage = (e: MessageEvent<WorkerToMain>) => {
      useStore.getState()._onWorkerMessage(e.data)
    }
    _worker.onerror = (e) => {
      useStore.getState()._onWorkerMessage({ type: 'error', message: e.message ?? 'worker error' })
    }
  }
  return _worker
}

function postToWorker(msg: MainToWorker) {
  getWorker().postMessage(msg)
}

// ── Store shape ───────────────────────────────────────────────────────────────

export type Tab = 'graph' | 'model' | 'dashboard' | 'results' | 'sensitivity' | 'optimization'
export type SimStatus = 'idle' | 'running' | 'done' | 'error'
export type Mode = 'edit' | 'result'

interface State {
  // Canonical editable document (spec §13.1) — the source of truth.
  doc: ModelDoc | null
  format: ModelFormat
  dirty: boolean
  // Engine-derived render/edit source (model of record for rendering).
  modelSummary: ModelSummary | null
  modelJson: string | null // last serialized doc (kept for viewer compatibility)
  modelFilename: string | null

  // Undo/redo command stack over `doc` (snapshots; docs are plain JSON, §13.4).
  past: ModelDoc[]
  future: ModelDoc[]

  // Workspace
  mode: Mode
  activeTab: Tab // active Result-mode view
  selectedId: string | null
  selectedIds: string[]

  // Validation (spec §8)
  issues: Issue[]
  topo: string[]
  valid: boolean
  reconciling: boolean

  // Simulation
  status: SimStatus
  errorMessage: string | null
  results: SimulationResults | null
  selectedResultId: string | null
  // Analysis config (spec §11); when enabled, run() sends it as results_spec.
  resultsSpec: ResultsSpec

  // Sensitivity
  sensStatus: SimStatus
  sensResults: SensitivityResults | null
  sensError: string | null

  // Optimization (runtime; independent of the sim run status)
  optStatus: SimStatus
  optResults: StudyResults | null
  optError: string | null

  // Run config (user-controlled; mirrors doc.simulation_settings)
  nRealizations: number
  seed: number | null
  simDuration: number | null
  simTimestep: number | null
  simDurationDisp: QtyDisplay
  simTimestepDisp: QtyDisplay

  // Copilot (spec §17)
  llmConfig: LlmConfig | null
  copilotOpen: boolean
  copilotMessages: { role: 'user' | 'assistant'; text: string }[]
  copilotRunning: boolean
  copilotProposal: { modelJson: string; rationale: string; ok: boolean; issueCount: number } | null
  copilotError: string | null

  // Internal
  _reconcileToken: number
  _reconcileTimer: ReturnType<typeof setTimeout> | null
  _onWorkerMessage: (msg: WorkerToMain) => void
}

interface Actions {
  // Files
  loadModel: (json: string, filename?: string) => void
  newModel: () => void
  saveModel: () => Promise<void>
  saveParameters: () => void

  // Workspace
  setMode: (m: Mode) => void
  setActiveTab: (tab: Tab) => void
  select: (id: string | null, additive?: boolean) => void

  // Editing (structural → reconcile; value → fast path)
  applyEdit: (next: ModelDoc, opts?: { reconcile?: boolean }) => void
  updateElementField: (id: string, patch: Partial<FlatElement>) => void
  mutateEl: (id: string, fn: (el: FlatElement) => void) => void
  addNewElement: (el: FlatElement, pos?: NodeView) => void
  duplicateElement: (id: string) => void
  removeElement: (id: string) => void
  renameElement: (oldId: string, newId: string) => void
  reparent: (id: string, container: string | null) => void
  moveNode: (id: string, pos: NodeView) => void
  tidyPositions: (positions: Record<string, NodeView>) => void
  editSettings: (patch: Partial<ModelDoc['simulation_settings']>) => void
  toggleDashboardItem: (which: 'inputs' | 'outputs', id: string) => void
  undo: () => void
  redo: () => void

  // Value fast-paths (no rebuild)
  setConstant: (id: string, value: number) => void
  setRvParam: (id: string, param: string, value: number) => void

  // Run
  run: () => void
  runSensitivity: (spec: SensitivitySpec) => void
  runOptimization: (spec: OptimizationSpec) => void
  setNRealizations: (n: number) => void
  setSeed: (s: number | null) => void
  setSimDuration: (v: number) => void
  setSimTimestep: (v: number) => void
  setSelectedResultId: (id: string) => void
  setResultsSpec: (patch: Partial<ResultsSpec>) => void

  // Copilot
  setLlmConfig: (cfg: LlmConfig | null) => void
  toggleCopilot: (open?: boolean) => void
  sendCopilot: (message: string) => Promise<void>
  acceptProposal: () => void
  rejectProposal: () => void
  validateModel: (json: string) => Promise<Validation>

  // Internal
  _scheduleReconcile: () => void
  _pushHistory: (prev: ModelDoc) => void
}

// Copilot silent-validation request/response bridge (module-level so it doesn't churn state).
const llmResolvers = new Map<number, (v: Validation) => void>()
let llmToken = 0

// ── Helpers ─────────────────────────────────────────────────────────────────────

function runtimeFromDoc(doc: ModelDoc) {
  const ss = doc.simulation_settings
  return {
    nRealizations: ss.n_realizations ?? 1,
    seed: ss.seed ?? null,
    simDuration: ss.duration.value,
    simTimestep: ss.timestep.value,
  }
}

// ── Store ─────────────────────────────────────────────────────────────────────

export const useStore = create<State & Actions>((set, get) => ({
  doc: null,
  format: 'v2',
  dirty: false,
  modelSummary: null,
  modelJson: null,
  modelFilename: null,
  past: [],
  future: [],
  mode: 'edit',
  activeTab: 'results',
  selectedId: null,
  selectedIds: [],
  issues: [],
  topo: [],
  valid: true,
  reconciling: false,
  status: 'idle',
  errorMessage: null,
  results: null,
  selectedResultId: null,
  resultsSpec: {
    elements: [],
    percentiles: [],
    distribution: false,
    bins: 30,
    capture_times: [],
    final_stats: false,
    confidence: 0.95,
    cte_percentile: 95,
  },
  sensStatus: 'idle',
  sensResults: null,
  sensError: null,
  optStatus: 'idle',
  optResults: null,
  optError: null,
  nRealizations: 1000,
  seed: 42,
  simDuration: null,
  simTimestep: null,
  simDurationDisp: IDENTITY_DISP,
  simTimestepDisp: IDENTITY_DISP,
  llmConfig: loadLlmConfig(),
  copilotOpen: false,
  copilotMessages: [],
  copilotRunning: false,
  copilotProposal: null,
  copilotError: null,
  _reconcileToken: 0,
  _reconcileTimer: null,

  // ── Files ──────────────────────────────────────────────────────────────────
  loadModel(json, filename) {
    let doc: ModelDoc
    try {
      doc = JSON.parse(json) as ModelDoc
    } catch {
      set({ status: 'error', errorMessage: 'Invalid JSON' })
      return
    }
    if (!doc.view) doc.view = { positions: {} }
    if (!doc.elements) doc.elements = []
    const format = detectFormat(doc)
    set({
      doc,
      format,
      modelJson: json,
      modelFilename: filename ?? null,
      modelSummary: null,
      dirty: false,
      past: [],
      future: [],
      mode: 'edit',
      selectedId: null,
      selectedIds: [],
      status: 'idle',
      results: null,
      selectedResultId: null,
      errorMessage: null,
      sensStatus: 'idle',
      sensResults: null,
      sensError: null,
      issues: [],
      topo: [],
      valid: true,
      ...runtimeFromDoc(doc),
      simDurationDisp: IDENTITY_DISP,
      simTimestepDisp: IDENTITY_DISP,
    })
    const token = get()._reconcileToken + 1
    set({ _reconcileToken: token, reconciling: true })
    postToWorker({ type: 'reconcile', model: json, token })
  },

  newModel() {
    const doc = blankModel()
    get().loadModel(serializeModel(doc), 'untitled.json')
  },

  async saveModel() {
    const { doc, modelFilename } = get()
    if (!doc) return
    const text = serializeModel(doc)
    const name = modelFilename ?? 'model.json'
    // File System Access API where available (§13.4), else download fallback.
    const w = window as unknown as { showSaveFilePicker?: (o: unknown) => Promise<FileSystemFileHandle> }
    if (typeof w.showSaveFilePicker === 'function') {
      try {
        const handle = await w.showSaveFilePicker({
          suggestedName: name,
          types: [{ description: 'WASiM model', accept: { 'application/json': ['.json'] } }],
        })
        const writable = await (handle as unknown as { createWritable: () => Promise<{ write: (s: string) => Promise<void>; close: () => Promise<void> }> }).createWritable()
        await writable.write(text)
        await writable.close()
        set({ dirty: false })
        return
      } catch (e) {
        if ((e as Error).name === 'AbortError') return
        // fall through to download
      }
    }
    const blob = new Blob([text], { type: 'application/json' })
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = name
    a.click()
    URL.revokeObjectURL(url)
    set({ dirty: false })
  },

  // ── Workspace ────────────────────────────────────────────────────────────────
  setMode: (mode) => set({ mode }),
  setActiveTab: (activeTab) => set({ activeTab, mode: 'result' }),
  select: (id, additive) =>
    set((s) => {
      if (id === null) return { selectedId: null, selectedIds: [] }
      if (additive) {
        const has = s.selectedIds.includes(id)
        const ids = has ? s.selectedIds.filter((x) => x !== id) : [...s.selectedIds, id]
        return { selectedId: ids[ids.length - 1] ?? null, selectedIds: ids }
      }
      return { selectedId: id, selectedIds: [id] }
    }),

  // ── Editing ──────────────────────────────────────────────────────────────────
  _pushHistory(prev) {
    set((s) => ({ past: [...s.past.slice(-HISTORY_LIMIT + 1), prev], future: [] }))
  },

  applyEdit(next, opts) {
    const prev = get().doc
    if (prev) get()._pushHistory(prev)
    set({ doc: next, dirty: true, modelJson: serializeModel(next) })
    if (opts?.reconcile !== false) get()._scheduleReconcile()
  },

  updateElementField(id, patch) {
    const doc = get().doc
    if (!doc) return
    get().applyEdit(updateElement(doc, id, patch))
  },

  mutateEl(id, fn) {
    const doc = get().doc
    if (!doc) return
    get().applyEdit(mutateElement(doc, id, fn))
  },

  addNewElement(el, pos) {
    const doc = get().doc
    if (!doc) return
    const id = uniqueId(doc, el.id)
    const withId = { ...el, id }
    get().applyEdit(addElement(doc, withId, pos))
    set({ selectedId: id, selectedIds: [id] })
  },

  duplicateElement(id) {
    const doc = get().doc
    if (!doc) return
    const [next, newId] = duplicateElement(doc, id)
    if (newId === id) return
    get().applyEdit(next)
    set({ selectedId: newId, selectedIds: [newId] })
  },

  removeElement(id) {
    const doc = get().doc
    if (!doc) return
    get().applyEdit(deleteElement(doc, id))
    set((s) => ({
      selectedId: s.selectedId === id ? null : s.selectedId,
      selectedIds: s.selectedIds.filter((x) => x !== id),
    }))
  },

  renameElement(oldId, newId) {
    const doc = get().doc
    if (!doc || oldId === newId) return
    const uid = uniqueId(doc, newId)
    get().applyEdit(renameId(doc, oldId, uid))
    set((s) => ({
      selectedId: s.selectedId === oldId ? uid : s.selectedId,
      selectedIds: s.selectedIds.map((x) => (x === oldId ? uid : x)),
    }))
  },

  reparent(id, container) {
    const doc = get().doc
    if (!doc) return
    get().applyEdit(setContainer(doc, id, container))
  },

  moveNode(id, pos) {
    const doc = get().doc
    if (!doc) return
    // Layout-only: mutate the view block without a reconcile (engine ignores view, §13.3).
    get().applyEdit(setPosition(doc, id, pos), { reconcile: false })
  },

  tidyPositions(positions) {
    const doc = get().doc
    if (!doc) return
    get().applyEdit(setPositions(doc, positions), { reconcile: false })
  },

  toggleDashboardItem(which, id) {
    const doc = get().doc
    if (!doc) return
    // Dashboard config lives in the view block — a layout edit, no reconcile needed.
    get().applyEdit(toggleDashboard(doc, which, id), { reconcile: false })
  },

  editSettings(patch) {
    const doc = get().doc
    if (!doc) return
    const next = updateSettings(doc, patch)
    get().applyEdit(next)
    set(runtimeFromDoc(next))
  },

  undo() {
    const { past, doc } = get()
    if (past.length === 0 || !doc) return
    const prev = past[past.length - 1]
    set((s) => ({ past: s.past.slice(0, -1), future: [doc, ...s.future], doc: prev, dirty: true, modelJson: serializeModel(prev) }))
    get()._scheduleReconcile()
  },

  redo() {
    const { future, doc } = get()
    if (future.length === 0 || !doc) return
    const next = future[0]
    set((s) => ({ future: s.future.slice(1), past: [...s.past, doc!], doc: next, dirty: true, modelJson: serializeModel(next) }))
    get()._scheduleReconcile()
  },

  // ── Copilot (spec §17) ─────────────────────────────────────────────────────────
  setLlmConfig(cfg) {
    saveLlmConfig(cfg)
    set({ llmConfig: cfg })
  },

  toggleCopilot(open) {
    set((s) => ({ copilotOpen: open ?? !s.copilotOpen }))
  },

  validateModel(json) {
    // Silent validation for the copilot — resolves a promise, never touches the issues panel.
    return new Promise<Validation>((resolve) => {
      const token = ++llmToken
      llmResolvers.set(token, resolve)
      postToWorker({ type: 'llm_validate', model: json, token })
    })
  },

  async sendCopilot(message) {
    const cfg = get().llmConfig
    if (!cfg) { set({ copilotError: 'Configure an AI provider first (Settings).' }); return }
    const doc = get().doc
    set((s) => ({
      copilotRunning: true,
      copilotError: null,
      copilotProposal: null,
      copilotMessages: [...s.copilotMessages, { role: 'user', text: message }],
    }))
    try {
      const result = await runCopilot({
        provider: getProvider(cfg),
        userMessage: message,
        currentModel: doc ? serializeModel(doc) : null,
        validate: (j) => get().validateModel(j),
      })
      const val = result.finalValidation
      set((s) => ({
        copilotRunning: false,
        copilotMessages: [...s.copilotMessages, { role: 'assistant', text: result.rationale || '(no rationale)' }],
        copilotProposal: result.modelJson
          ? {
              modelJson: result.modelJson,
              rationale: result.rationale,
              ok: val?.ok ?? false,
              issueCount: val?.issues.length ?? 0,
            }
          : null,
      }))
    } catch (e) {
      set({ copilotRunning: false, copilotError: String(e) })
    }
  },

  acceptProposal() {
    const p = get().copilotProposal
    if (!p) return
    let next: ModelDoc
    try {
      next = JSON.parse(p.modelJson) as ModelDoc
    } catch {
      set({ copilotError: 'Proposed model is not valid JSON.' })
      return
    }
    if (!next.view) next.view = { positions: {} }
    if (!next.elements) next.elements = []
    // Enter via the normal reconcile path so it's undoable and re-validated (§17.4).
    set({ format: detectFormat(next) })
    get().applyEdit(next)
    set((s) => ({
      copilotProposal: null,
      copilotMessages: [...s.copilotMessages, { role: 'assistant', text: '✓ Proposal accepted and applied.' }],
      ...runtimeFromDoc(next),
    }))
  },

  rejectProposal() {
    set({ copilotProposal: null })
  },

  _scheduleReconcile() {
    const existing = get()._reconcileTimer
    if (existing) clearTimeout(existing)
    const timer = setTimeout(() => {
      const doc = get().doc
      if (!doc) return
      const token = get()._reconcileToken + 1
      set({ _reconcileToken: token, reconciling: true, _reconcileTimer: null })
      postToWorker({ type: 'reconcile', model: serializeModel(doc), token })
    }, RECONCILE_DEBOUNCE_MS)
    set({ _reconcileTimer: timer })
  },

  // ── Value fast-paths ──────────────────────────────────────────────────────────
  setConstant(id, value) {
    const sm = get().modelSummary
    if (sm) {
      set({ modelSummary: { ...sm, elements: sm.elements.map((e) => (e.id === id ? { ...e, value } : e)) } })
    }
    // Keep the canonical doc in sync so Save reflects the edit (no reconcile needed).
    const doc = get().doc
    if (doc) {
      const next = mutateElement(doc, id, (el) => { if (el.value) el.value = { ...el.value, value } })
      set({ doc: next, dirty: true, modelJson: serializeModel(next) })
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
            const updated = typeof old === 'object' && old !== null ? { ...old, value } : value
            return { ...e, dist: { ...e.dist, parameters: { ...e.dist.parameters, [param]: updated } } }
          }),
        },
      })
    }
    const doc = get().doc
    if (doc) {
      const next = mutateElement(doc, id, (el) => {
        const d = el.distribution as { parameters?: Record<string, unknown> } | undefined
        if (d?.parameters && param in d.parameters) {
          const old = d.parameters[param]
          d.parameters[param] = typeof old === 'object' && old !== null ? { ...(old as object), value } : value
        }
      })
      set({ doc: next, dirty: true, modelJson: serializeModel(next) })
    }
    postToWorker({ type: 'set_rv_param', element_id: id, param_name: param, value })
  },

  // ── Run ───────────────────────────────────────────────────────────────────────
  run() {
    const { nRealizations, seed, simDuration, simTimestep, resultsSpec } = get()
    set({ status: 'running', errorMessage: null, mode: 'result' })
    // Only attach a results_spec when the user has enabled some analysis; otherwise the
    // engine emits the default fixed summary (byte-identical to the pre-analysis path).
    const rs = resultsSpec
    const analysisOn =
      rs.distribution || rs.final_stats || rs.percentiles.length > 0 || rs.capture_times.length > 0
    postToWorker({
      type: 'run',
      config: {
        n_realizations: nRealizations,
        seed: seed ?? undefined,
        duration_override: simDuration ?? undefined,
        timestep_override: simTimestep ?? undefined,
        ...(analysisOn ? { results_spec: rs } : {}),
      },
    })
  },

  runSensitivity(spec) {
    set({ sensStatus: 'running', sensError: null, sensResults: null })
    postToWorker({ type: 'run_sensitivity', spec })
  },

  runOptimization(spec) {
    set({ optStatus: 'running', optError: null, optResults: null })
    postToWorker({ type: 'run_optimization', spec })
  },

  setNRealizations: (n) => { set({ nRealizations: n }); get().editSettings({ n_realizations: n }) },
  setSeed: (s) => { set({ seed: s }); get().editSettings({ seed: s }) },
  setSimDuration: (v) => {
    const d = get().simDurationDisp
    const canonical = (v - d.offset) / d.factor
    set({ simDuration: canonical })
    const doc = get().doc
    if (doc) get().editSettings({ duration: { ...doc.simulation_settings.duration, value: canonical } })
  },
  setSimTimestep: (v) => {
    const d = get().simTimestepDisp
    const canonical = (v - d.offset) / d.factor
    set({ simTimestep: canonical })
    const doc = get().doc
    if (doc) get().editSettings({ timestep: { ...doc.simulation_settings.timestep, value: canonical } })
  },
  setSelectedResultId: (id) => set({ selectedResultId: id }),
  setResultsSpec: (patch) => set((s) => ({ resultsSpec: { ...s.resultsSpec, ...patch } })),

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
    const stem = modelFilename ? modelFilename.replace(/\.json$/i, '') : 'model'
    const blob = new Blob([paramsJson], { type: 'application/json' })
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = `${stem}.params.json`
    a.click()
    URL.revokeObjectURL(url)
  },

  // ── Worker messages ─────────────────────────────────────────────────────────
  _onWorkerMessage(msg) {
    switch (msg.type) {
      case 'model_loaded':
        set({
          modelSummary: msg.summary,
          simDurationDisp: msg.summary.time_display.duration,
          simTimestepDisp: msg.summary.time_display.timestep,
        })
        break

      case 'reconciled': {
        if (msg.token < get()._reconcileToken) break // stale
        const applyValidation = (v: Validation) =>
          set({ issues: v.issues, topo: v.topo, valid: v.ok, reconciling: false })
        applyValidation(msg.validation)
        if (msg.summary) {
          set({
            modelSummary: msg.summary,
            simDurationDisp: msg.summary.time_display.duration,
            simTimestepDisp: msg.summary.time_display.timestep,
          })
        }
        break
      }

      case 'validated': {
        if (msg.token < get()._reconcileToken) break
        set({ issues: msg.validation.issues, topo: msg.validation.topo, valid: msg.validation.ok, reconciling: false })
        break
      }

      case 'llm_validated': {
        // Resolve the copilot's silent-validation promise; never touches the issues panel.
        const resolve = llmResolvers.get(msg.token)
        if (resolve) { llmResolvers.delete(msg.token); resolve(msg.validation) }
        break
      }

      case 'complete':
        set({
          status: 'done',
          results: msg.results,
          selectedResultId: msg.results.output_ids[0] ?? null,
          activeTab: 'results',
          mode: 'result',
        })
        break

      case 'sensitivity_complete':
        set({ sensStatus: 'done', sensResults: msg.results })
        break

      case 'optimization_complete':
        set({ optStatus: 'done', optResults: msg.results })
        break

      case 'error':
        // A worker error can arrive for any in-flight job; surface it on whichever is running.
        if (get().optStatus === 'running') set({ optStatus: 'error', optError: msg.message })
        else if (get().sensStatus === 'running') set({ sensStatus: 'error', sensError: msg.message })
        else set({ status: 'error', errorMessage: msg.message, reconciling: false })
        break
    }
  },
}))

// ── Stable-reference selector hooks ──────────────────────────────────────────
// Selectors must return a stable reference for empty state; a fresh `?? []` / `?? {}`
// makes useSyncExternalStore see the snapshot change every render (infinite loop / React
// #185). These share one frozen empty per shape.
const EMPTY_ELEMENTS: ModelSummary['elements'] = []
const EMPTY_CONTAINERS: NonNullable<ModelDoc['containers']> = []
const EMPTY_POSITIONS: Record<string, NodeView> = {}

export const useElements = () => useStore((s) => s.modelSummary?.elements ?? EMPTY_ELEMENTS)
export const useContainers = () => useStore((s) => s.doc?.containers ?? EMPTY_CONTAINERS)
export const usePositions = () => useStore((s) => s.doc?.view?.positions ?? EMPTY_POSITIONS)

// Re-export so components can import the doc type location conveniently.
export type { ModelDoc, FlatElement } from './model/schema'
export type { ModelJson }
