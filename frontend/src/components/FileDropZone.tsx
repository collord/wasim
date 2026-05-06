import { useCallback, useState } from 'react'
import { useStore } from '../store'

export function FileDropZone() {
  const loadModel = useStore((s) => s.loadModel)
  const [dragging, setDragging] = useState(false)

  const handleFile = useCallback(
    (file: File) => {
      const reader = new FileReader()
      reader.onload = (e) => loadModel(e.target?.result as string)
      reader.readAsText(file)
    },
    [loadModel],
  )

  const onDrop = useCallback(
    (e: React.DragEvent) => {
      e.preventDefault()
      setDragging(false)
      const file = e.dataTransfer.files[0]
      if (file) handleFile(file)
    },
    [handleFile],
  )

  const onFileInput = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      const file = e.target.files?.[0]
      if (file) handleFile(file)
    },
    [handleFile],
  )

  return (
    <div
      onDragOver={(e) => { e.preventDefault(); setDragging(true) }}
      onDragLeave={() => setDragging(false)}
      onDrop={onDrop}
      className={`
        flex flex-col items-center justify-center gap-3 rounded-xl border-2 border-dashed
        px-8 py-16 text-center transition-colors
        ${dragging ? 'border-blue-400 bg-blue-50' : 'border-slate-300 bg-white hover:border-slate-400'}
      `}
    >
      <svg className="h-10 w-10 text-slate-400" fill="none" viewBox="0 0 24 24" stroke="currentColor">
        <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.5}
          d="M9 13h6m-3-3v6m5 5H7a2 2 0 01-2-2V5a2 2 0 012-2h5.586a1 1 0 01.707.293l5.414 5.414a1 1 0 01.293.707V19a2 2 0 01-2 2z" />
      </svg>
      <div>
        <p className="font-medium text-slate-700">Drop a model.json file here</p>
        <p className="mt-1 text-sm text-slate-500">or click to browse</p>
      </div>
      <label className="cursor-pointer rounded-md bg-slate-900 px-4 py-2 text-sm font-medium text-white hover:bg-slate-700">
        Browse
        <input type="file" accept=".json" className="sr-only" onChange={onFileInput} />
      </label>
    </div>
  )
}
