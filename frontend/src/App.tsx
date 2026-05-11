import { useStore, type Tab } from './store'
import { FileDropZone } from './components/FileDropZone'
import { GraphTab } from './components/tabs/GraphTab'
import { ModelTab } from './components/tabs/ModelTab'
import { DashboardTab } from './components/tabs/DashboardTab'
import { ResultsTab } from './components/tabs/ResultsTab'

const TABS: { id: Tab; label: string }[] = [
  { id: 'graph', label: 'Graph' },
  { id: 'model', label: 'Model' },
  { id: 'dashboard', label: 'Dashboard' },
  { id: 'results', label: 'Results' },
]

export function App() {
  const modelLoaded = useStore((s) => s.parsedModel !== null)
  const activeTab = useStore((s) => s.activeTab)
  const setActiveTab = useStore((s) => s.setActiveTab)
  const status = useStore((s) => s.status)
  const loadModel = useStore((s) => s.loadModel)
  const modelName = useStore((s) => s.parsedModel?.source?.notes ?? null)

  const onFileInput = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0]
    if (!file) return
    const reader = new FileReader()
    reader.onload = (ev) => loadModel(ev.target?.result as string)
    reader.readAsText(file)
  }

  return (
    <div className="flex min-h-screen flex-col">
      {/* Header */}
      <header className="border-b border-slate-200 bg-white px-4 py-3 shadow-sm">
        <div className="mx-auto flex max-w-5xl items-center gap-4">
          <span className="font-mono text-lg font-bold tracking-tight text-slate-900">
            WASiM
          </span>
          {modelLoaded && (
            <span className="text-sm text-slate-400">
              {modelName ?? 'model loaded'}
            </span>
          )}
          <label className="ml-auto cursor-pointer rounded border border-slate-300 bg-white px-3 py-1.5 text-xs font-medium text-slate-700 hover:bg-slate-50">
            {modelLoaded ? 'Load different model' : 'Load model.json'}
            <input type="file" accept=".json" className="sr-only" onChange={onFileInput} />
          </label>
          {status === 'error' && (
            <span className="text-xs font-medium text-red-600">Error — see dashboard</span>
          )}
        </div>
      </header>

      {/* Main */}
      <main className="mx-auto w-full max-w-5xl flex-1 px-4 py-6">
        {!modelLoaded ? (
          <div className="mx-auto max-w-lg pt-16">
            <FileDropZone />
          </div>
        ) : (
          <>
            {/* Tab bar */}
            <div className="mb-6 flex gap-1 border-b border-slate-200">
              {TABS.map((tab) => (
                <button
                  key={tab.id}
                  onClick={() => setActiveTab(tab.id)}
                  className={`px-4 py-2 text-sm font-medium transition-colors ${
                    activeTab === tab.id
                      ? 'border-b-2 border-blue-600 text-blue-600'
                      : 'text-slate-500 hover:text-slate-700'
                  }`}
                >
                  {tab.label}
                </button>
              ))}
            </div>

            {/* Tab content */}
            {activeTab === 'graph'     && <GraphTab />}
            {activeTab === 'model'     && <ModelTab />}
            {activeTab === 'dashboard' && <DashboardTab />}
            {activeTab === 'results'   && <ResultsTab />}
          </>
        )}
      </main>
    </div>
  )
}
