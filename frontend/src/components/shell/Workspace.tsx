import { useEffect } from 'react'
import { useStore, type Tab } from '../../store'
import { Toolbar } from './Toolbar'
import { StatusBar } from './StatusBar'
import { ModelBrowser } from '../browser/ModelBrowser'
import { Inspector } from '../inspector/Inspector'
import { EditableCanvas } from '../canvas/EditableCanvas'
import { CopilotPanel } from '../copilot/CopilotPanel'
import { GraphTab } from '../tabs/GraphTab'
import { ModelTab } from '../tabs/ModelTab'
import { DashboardTab } from '../tabs/DashboardTab'
import { ResultsTab } from '../tabs/ResultsTab'
import { SensitivityTab } from '../tabs/SensitivityTab'
import { OptimizationTab } from '../tabs/OptimizationTab'

const RESULT_TABS: { id: Tab; label: string }[] = [
  { id: 'results', label: 'Results' },
  { id: 'sensitivity', label: 'Sensitivity' },
  { id: 'optimization', label: 'Optimization' },
  { id: 'dashboard', label: 'Dashboard' },
  { id: 'graph', label: 'Graph' },
  { id: 'model', label: 'Model' },
]

/** The three-pane authoring workspace (spec §1.1): Browser | Canvas | Inspector, with a
 *  toolbar and status bar, and an Edit ⇄ Result mode switch. Result mode preserves the
 *  original tabs so nothing is lost. */
export function Workspace() {
  const mode = useStore((s) => s.mode)
  const activeTab = useStore((s) => s.activeTab)
  const setActiveTab = useStore((s) => s.setActiveTab)
  const copilotOpen = useStore((s) => s.copilotOpen)
  const undo = useStore((s) => s.undo)
  const redo = useStore((s) => s.redo)
  const saveModel = useStore((s) => s.saveModel)

  // Global keyboard shortcuts: undo/redo/save.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const meta = e.metaKey || e.ctrlKey
      if (!meta) return
      const tag = (e.target as HTMLElement)?.tagName
      if (e.key.toLowerCase() === 'z' && tag !== 'INPUT' && tag !== 'TEXTAREA') {
        e.preventDefault(); e.shiftKey ? redo() : undo()
      } else if (e.key.toLowerCase() === 's') {
        e.preventDefault(); saveModel()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [undo, redo, saveModel])

  return (
    <div className="flex h-screen flex-col">
      <Toolbar />

      <div className="flex min-h-0 flex-1">
        <div className="flex min-h-0 min-w-0 flex-1">
          {mode === 'edit' ? (
            <div className="flex min-h-0 flex-1">
              {/* Left: Model Browser + Palette */}
              <div className="w-64 shrink-0 border-r border-slate-200 bg-white">
                <ModelBrowser />
              </div>
              {/* Center: editable canvas */}
              <div className="min-w-0 flex-1">
                <EditableCanvas />
              </div>
              {/* Right: Inspector */}
              <div className="w-72 shrink-0 border-l border-slate-200 bg-white">
                <Inspector />
              </div>
            </div>
          ) : (
            <div className="flex min-h-0 flex-1 flex-col">
              <div className="flex gap-1 border-b border-slate-200 bg-white px-3">
                {RESULT_TABS.map((t) => (
                  <button key={t.id} onClick={() => setActiveTab(t.id)}
                    className={`px-3 py-2 text-sm font-medium ${activeTab === t.id ? 'border-b-2 border-blue-600 text-blue-600' : 'text-slate-500 hover:text-slate-700'}`}>
                    {t.label}
                  </button>
                ))}
              </div>
              <div className="min-h-0 flex-1 overflow-auto p-4">
                {activeTab === 'results' && <ResultsTab />}
                {activeTab === 'sensitivity' && <SensitivityTab />}
                {activeTab === 'optimization' && <OptimizationTab />}
                {activeTab === 'dashboard' && <DashboardTab />}
                {activeTab === 'graph' && <GraphTab />}
                {activeTab === 'model' && <ModelTab />}
              </div>
            </div>
          )}
        </div>
        {copilotOpen && (
          <div className="w-80 shrink-0 border-l border-slate-200 bg-white">
            <CopilotPanel />
          </div>
        )}
      </div>

      <StatusBar />
    </div>
  )
}
