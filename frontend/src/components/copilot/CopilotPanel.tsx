import { useRef, useState } from 'react'
import { useStore } from '../../store'
import { effectiveThinking } from '../../copilot/aiConfig'
import { callAnthropic } from '../../copilot/anthropic'
import { validateCandidate } from '../../copilot/validateCandidate'
import { propose, type ProposeResult } from '../../copilot/loop'
import { BATHTUB_EXEMPLAR } from '../../copilot/exemplar'
import authoringGuide from '../../copilot/authoring-guide.generated.md?raw'
import type { Validation } from '../../worker/protocol'

/**
 * The LLM-authoring copilot panel (WASIM_AUTHORING_ENVIRONMENT_SPEC.md §17): describe a model in
 * words → the copilot drafts it, validates it through the engine, and self-corrects until valid →
 * you Accept (loads it) or Reject. Crudest useful surface — the feasibility slice, not the polished
 * Copilot panel. Nothing auto-applies; Accept is gated on the engine calling the model valid.
 */

const SYSTEM_PROMPT =
  authoringGuide +
  '\n\nReturn ONE complete WASiM v2 model as a single JSON object. If you are given validation ' +
  'diagnostics, fix exactly those issues and return the corrected full model. A fenced ```json block ' +
  'is fine. Do not include commentary that is not part of the JSON.\n\n' +
  'A complete, engine-valid example to follow (note the exact shapes — `ref` uses `element_id`, ' +
  'stocks list flow ids in `inflows`/`outflows`):\n```json\n' +
  BATHTUB_EXEMPLAR +
  '\n```'

interface Status {
  attempt: number
  validation: Validation | null
}

export function CopilotPanel() {
  const aiConfig = useStore((s) => s.aiConfig)
  const loadModel = useStore((s) => s.loadModel)

  const [description, setDescription] = useState('')
  const [running, setRunning] = useState(false)
  const [status, setStatus] = useState<Status | null>(null)
  const [result, setResult] = useState<ProposeResult | null>(null)
  const [error, setError] = useState<string | null>(null)
  const abortRef = useRef<AbortController | null>(null)

  const hasKey = aiConfig.apiKey.trim().length > 0

  const generate = async () => {
    setRunning(true)
    setError(null)
    setResult(null)
    setStatus({ attempt: 1, validation: null })
    const ctrl = new AbortController()
    abortRef.current = ctrl
    try {
      const r = await propose(
        description.trim(),
        { apiKey: aiConfig.apiKey, model: aiConfig.model, thinking: effectiveThinking(aiConfig) },
        SYSTEM_PROMPT,
        { call: callAnthropic, validate: validateCandidate },
        { signal: ctrl.signal, onIteration: (attempt, validation) => setStatus({ attempt, validation }) },
      )
      setResult(r)
    } catch (e) {
      if ((e as Error).name !== 'AbortError') setError((e as Error).message)
    } finally {
      setRunning(false)
      abortRef.current = null
    }
  }

  const accept = () => {
    if (!result?.validation.ok) return
    loadModel(result.model, 'copilot-model.json')
    setResult(null)
    setStatus(null)
    setDescription('')
  }

  return (
    <div className="flex h-full flex-col p-2" data-testid="copilot-panel">
      {!hasKey && (
        <div className="mb-2 rounded bg-amber-50 px-2 py-1.5 text-[11px] text-amber-700">
          Set an Anthropic API key in <span className="font-medium">Settings…</span> to enable the copilot.
        </div>
      )}

      <textarea
        value={description}
        onChange={(e) => setDescription(e.target.value)}
        placeholder="Describe a model — e.g. “a bathtub: one stock filled by a tap and drained in proportion to its level”"
        rows={4}
        disabled={running}
        className="w-full resize-y rounded border border-slate-300 px-2 py-1.5 text-xs outline-none focus:border-blue-400 disabled:bg-slate-50"
      />

      <div className="mt-2 flex items-center gap-2">
        <button
          onClick={generate}
          disabled={running || !hasKey || description.trim().length === 0}
          className="rounded bg-blue-600 px-3 py-1 text-xs font-semibold text-white hover:bg-blue-500 disabled:opacity-40"
        >
          {running ? 'Generating…' : 'Generate'}
        </button>
        {running && (
          <button
            onClick={() => abortRef.current?.abort()}
            className="rounded border border-slate-300 px-2 py-1 text-xs text-slate-600 hover:bg-slate-50"
          >
            Cancel
          </button>
        )}
      </div>

      {status && (
        <div className="mt-2 text-[11px] text-slate-500">
          Attempt {status.attempt}/5
          {status.validation && (status.validation.ok
            ? <span className="ml-1 text-emerald-600">· valid ✓</span>
            : <span className="ml-1 text-rose-600">· {status.validation.issues.length} issue(s)</span>)}
        </div>
      )}

      {error && <div className="mt-2 rounded bg-rose-50 px-2 py-1.5 text-[11px] text-rose-700">{error}</div>}

      {result && (
        <div className="mt-2 flex min-h-0 flex-1 flex-col">
          <div className="mb-1 text-[11px]">
            {result.validation.ok ? (
              <span className="text-emerald-600">Valid model in {result.iterations} attempt(s).</span>
            ) : (
              <span className="text-rose-600">
                Not valid after {result.iterations} attempt(s) — {result.validation.issues.length} issue(s).
              </span>
            )}
          </div>
          {!result.validation.ok && result.validation.issues.length > 0 && (
            <ul className="mb-1 max-h-24 overflow-auto text-[10px]">
              {result.validation.issues.map((i, k) => (
                <li key={k} className={i.severity === 'error' ? 'text-rose-600' : 'text-amber-600'}>
                  {i.severity}: {i.message}
                </li>
              ))}
            </ul>
          )}
          <pre className="min-h-0 flex-1 overflow-auto rounded border border-slate-200 bg-slate-50 p-2 text-[10px] leading-tight">
            {result.model}
          </pre>
          <div className="mt-2 flex gap-2">
            <button
              onClick={accept}
              disabled={!result.validation.ok}
              className="rounded bg-emerald-600 px-3 py-1 text-xs font-semibold text-white hover:bg-emerald-500 disabled:opacity-40"
              title={result.validation.ok ? 'Load this model' : 'Only a valid model can be accepted'}
            >
              Accept
            </button>
            <button
              onClick={() => { setResult(null); setStatus(null) }}
              className="rounded border border-slate-300 px-3 py-1 text-xs text-slate-600 hover:bg-slate-50"
            >
              Reject
            </button>
          </div>
        </div>
      )}
    </div>
  )
}
