import { useMemo, useState } from 'react'
import { useStore } from '../../store'
import { DEFAULT_MODELS, type LlmProvider } from '../../llm/config'

/** The copilot panel (spec §17.4): provider config, conversation, and a diff-previewed proposal
 *  that the user explicitly Accepts (nothing auto-applies). The engine is the validation gate. */
export function CopilotPanel() {
  const cfg = useStore((s) => s.llmConfig)
  const [showSettings, setShowSettings] = useState(!cfg)
  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center justify-between border-b border-slate-200 bg-slate-50 px-3 py-2">
        <span className="text-sm font-semibold text-slate-700">AI Copilot</span>
        <div className="flex items-center gap-2">
          <button onClick={() => setShowSettings((v) => !v)} className="text-xs text-slate-500 hover:text-slate-700">
            {showSettings ? 'Chat' : 'Settings'}
          </button>
          <button onClick={() => useStore.getState().toggleCopilot(false)} className="text-slate-400 hover:text-slate-600">✕</button>
        </div>
      </div>
      {showSettings || !cfg ? <Settings onSaved={() => setShowSettings(false)} /> : <Chat />}
    </div>
  )
}

// ── Settings ────────────────────────────────────────────────────────────────────

function Settings({ onSaved }: { onSaved: () => void }) {
  const cfg = useStore((s) => s.llmConfig)
  const setLlmConfig = useStore((s) => s.setLlmConfig)
  const [provider, setProvider] = useState<LlmProvider>(cfg?.provider ?? 'anthropic')
  const [model, setModel] = useState(cfg?.model ?? DEFAULT_MODELS.anthropic[0])
  const [apiKey, setApiKey] = useState(cfg?.apiKey ?? '')
  const [baseUrl, setBaseUrl] = useState(cfg?.baseUrl ?? '')

  const save = () => {
    setLlmConfig({ provider, model, apiKey, ...(provider === 'openai' && baseUrl ? { baseUrl } : {}) })
    onSaved()
  }

  return (
    <div className="space-y-3 overflow-auto p-3 text-xs">
      <p className="rounded bg-amber-50 p-2 text-[11px] text-amber-700">
        Bring-your-own-key. The key is stored only in this browser and sent only to the provider
        you choose — never to any WASiM server and never into model.json. Calling a provider
        directly from the browser exposes the key to page scripts (acceptable for a local, single-user tool).
      </p>
      <label className="block">
        <span className="mb-0.5 block font-medium text-slate-500">Provider</span>
        <select value={provider} onChange={(e) => { const p = e.target.value as LlmProvider; setProvider(p); setModel(DEFAULT_MODELS[p][0]) }}
          className="w-full rounded border border-slate-300 px-2 py-1">
          <option value="anthropic">Anthropic</option>
          <option value="openai">OpenAI (or compatible)</option>
        </select>
      </label>
      <label className="block">
        <span className="mb-0.5 block font-medium text-slate-500">Model</span>
        <input list="llm-models" value={model} onChange={(e) => setModel(e.target.value)} className="w-full rounded border border-slate-300 px-2 py-1 font-mono" />
        <datalist id="llm-models">{DEFAULT_MODELS[provider].map((m) => <option key={m} value={m} />)}</datalist>
      </label>
      <label className="block">
        <span className="mb-0.5 block font-medium text-slate-500">API key</span>
        <input type="password" value={apiKey} onChange={(e) => setApiKey(e.target.value)} placeholder="sk-…" className="w-full rounded border border-slate-300 px-2 py-1 font-mono" />
      </label>
      {provider === 'openai' && (
        <label className="block">
          <span className="mb-0.5 block font-medium text-slate-500">Base URL (optional gateway)</span>
          <input value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} placeholder="https://api.openai.com/v1" className="w-full rounded border border-slate-300 px-2 py-1 font-mono" />
        </label>
      )}
      <div className="flex gap-2">
        <button onClick={save} disabled={!apiKey} className="rounded bg-blue-600 px-3 py-1.5 font-semibold text-white hover:bg-blue-500 disabled:opacity-40">Save</button>
        {cfg && <button onClick={() => useStore.getState().setLlmConfig(null)} className="rounded border border-slate-300 px-3 py-1.5 text-slate-600 hover:bg-slate-50">Clear</button>}
      </div>
    </div>
  )
}

// ── Chat + proposal ──────────────────────────────────────────────────────────────

function Chat() {
  const messages = useStore((s) => s.copilotMessages)
  const running = useStore((s) => s.copilotRunning)
  const proposal = useStore((s) => s.copilotProposal)
  const error = useStore((s) => s.copilotError)
  const send = useStore((s) => s.sendCopilot)
  const [text, setText] = useState('')

  const submit = () => {
    const t = text.trim()
    if (!t || running) return
    setText('')
    void send(t)
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="min-h-0 flex-1 space-y-2 overflow-auto p-3 text-xs">
        {messages.length === 0 && (
          <p className="text-slate-400">
            Describe a model in words — e.g. “a two-tank system that overflows into a creek”, or
            “a 30-year retirement projection with lognormal returns”. The engine validates every
            proposal before you accept it.
          </p>
        )}
        {messages.map((m, i) => (
          <div key={i} className={m.role === 'user' ? 'text-right' : ''}>
            <span className={`inline-block max-w-[90%] whitespace-pre-wrap rounded-lg px-2.5 py-1.5 text-left ${
              m.role === 'user' ? 'bg-blue-600 text-white' : 'bg-slate-100 text-slate-700'}`}>{m.text}</span>
          </div>
        ))}
        {running && <div className="text-slate-400">⟳ thinking… (proposing → validating → fixing)</div>}
        {error && <div className="rounded bg-red-50 px-2 py-1 text-red-600">{error}</div>}
        {proposal && <ProposalCard />}
      </div>
      <div className="border-t border-slate-200 p-2">
        <textarea value={text} onChange={(e) => setText(e.target.value)} rows={2}
          onKeyDown={(e) => { if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) { e.preventDefault(); submit() } }}
          placeholder="Describe or refine the model… (⌘/Ctrl-Enter to send)"
          className="w-full resize-none rounded border border-slate-300 px-2 py-1.5 text-xs outline-none focus:border-blue-400" />
        <div className="mt-1 flex justify-end">
          <button onClick={submit} disabled={running || !text.trim()} className="rounded bg-blue-600 px-3 py-1 text-xs font-semibold text-white hover:bg-blue-500 disabled:opacity-40">Send</button>
        </div>
      </div>
    </div>
  )
}

function ProposalCard() {
  const proposal = useStore((s) => s.copilotProposal)!
  const doc = useStore((s) => s.doc)
  const accept = useStore((s) => s.acceptProposal)
  const reject = useStore((s) => s.rejectProposal)

  const diff = useMemo(() => {
    let next: { elements?: { id: string }[] } = {}
    try { next = JSON.parse(proposal.modelJson) } catch { /* ignore */ }
    const nextIds = new Set((next.elements ?? []).map((e) => e.id))
    const curIds = new Set((doc?.elements ?? []).map((e) => e.id))
    const added = [...nextIds].filter((id) => !curIds.has(id))
    const removed = [...curIds].filter((id) => !nextIds.has(id))
    const kept = [...nextIds].filter((id) => curIds.has(id))
    return { added, removed, kept, total: nextIds.size }
  }, [proposal.modelJson, doc])

  return (
    <div className="rounded-lg border border-blue-200 bg-blue-50/50 p-2.5">
      <div className="mb-1.5 flex items-center justify-between">
        <span className="text-[11px] font-semibold text-slate-700">Proposed model</span>
        <span className={`rounded px-1.5 py-0.5 text-[10px] font-medium ${proposal.ok ? 'bg-emerald-100 text-emerald-700' : 'bg-amber-100 text-amber-700'}`}>
          {proposal.ok ? '● engine-valid' : `⚠ ${proposal.issueCount} issue(s)`}
        </span>
      </div>
      <div className="mb-2 text-[11px] text-slate-500">
        {diff.total} elements · <span className="text-emerald-600">+{diff.added.length}</span> ·{' '}
        <span className="text-red-600">−{diff.removed.length}</span> · {diff.kept.length} kept
        {diff.added.length > 0 && <div className="mt-0.5 truncate font-mono text-[10px] text-emerald-700">+ {diff.added.slice(0, 8).join(', ')}</div>}
      </div>
      <div className="flex gap-2">
        <button onClick={accept} className="rounded bg-emerald-600 px-3 py-1 text-[11px] font-semibold text-white hover:bg-emerald-500">Accept</button>
        <button onClick={reject} className="rounded border border-slate-300 bg-white px-3 py-1 text-[11px] font-medium text-slate-600 hover:bg-slate-50">Reject</button>
      </div>
    </div>
  )
}
