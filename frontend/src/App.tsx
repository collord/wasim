import { useStore } from './store'
import { FileDropZone } from './components/FileDropZone'
import { Workspace } from './components/shell/Workspace'

export function App() {
  const modelLoaded = useStore((s) => s.doc !== null)
  const newModel = useStore((s) => s.newModel)

  if (!modelLoaded) {
    return (
      <div className="flex min-h-screen flex-col bg-slate-50">
        <header className="border-b border-slate-200 bg-white px-4 py-3">
          <div className="mx-auto flex max-w-5xl items-center gap-4">
            <span className="font-mono text-lg font-bold tracking-tight text-slate-900">WASiM</span>
            <span className="text-sm text-slate-400">authoring environment</span>
          </div>
        </header>
        <main className="mx-auto w-full max-w-lg flex-1 px-4 pt-16">
          <FileDropZone />
          <div className="mt-6 text-center">
            <button
              onClick={newModel}
              className="rounded border border-slate-300 bg-white px-4 py-2 text-sm font-medium text-slate-700 hover:bg-slate-50"
            >
              + New blank model
            </button>
          </div>
        </main>
      </div>
    )
  }

  return <Workspace />
}
