// Live streaming view: drives the REAL Rust WASM engine (RunHandle.poll) and renders the
// p05-p95 band tightening as realizations accrue. Proof-of-life for the streaming poll boundary
// (sweep composition Phase 0c/5). Consumes only the poll payload — no synthetic data, no
// fabricated traces. Additive: does not touch the synthetic Diagram/Transport path.
import { useEffect, useRef, useState } from "react";
import demoModel from "./models/demo_model.json";
import { loadEngine, type WasmEngine } from "./engine/wasm";
import { startRun, type RunController } from "./engine/pollRun";
import type { SimulationResults } from "./engine/simResults";
import { BandChart } from "./render/BandChart";

const MODEL_JSON = JSON.stringify(demoModel);
const WATCH_ELEMENT = "balance"; // the diffusion-cone stock
const CHUNK = 100;

export default function StreamApp() {
  const engineRef = useRef<WasmEngine | null>(null);
  const controllerRef = useRef<RunController | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [results, setResults] = useState<SimulationResults | null>(null);
  const [completed, setCompleted] = useState(0);
  const [total, setTotal] = useState(0);
  const [running, setRunning] = useState(false);

  // Load the wasm engine once.
  useEffect(() => {
    let live = true;
    loadEngine(MODEL_JSON)
      .then((eng) => { if (live) engineRef.current = eng; })
      .catch((e) => { if (live) setLoadError(String(e)); });
    return () => { live = false; controllerRef.current?.cancel(); };
  }, []);

  const run = () => {
    const eng = engineRef.current;
    if (!eng || running) return;
    setResults(null);
    setCompleted(0);
    setRunning(true);
    controllerRef.current = startRun(
      eng,
      "{}", // the model carries its own n_realizations/seed/timeline
      (res, progress) => {
        setResults(res);
        setCompleted(progress.completed);
        setTotal(progress.total);
        if (progress.done) setRunning(false);
      },
      CHUNK,
    );
  };

  const cancel = () => {
    controllerRef.current?.cancel();
    controllerRef.current = null;
    setRunning(false);
  };

  // band width at the final step = a convergence proxy (tightens as realizations accrue).
  const th = results?.elements[WATCH_ELEMENT]?.time_history;
  const bandWidth = th ? th.p95[th.p95.length - 1] - th.p05[th.p05.length - 1] : null;

  return (
    <div style={{ padding: 24, fontFamily: "var(--sans, sans-serif)", color: "var(--ink-0)" }}>
      <h1 style={{ fontFamily: "var(--mono)", fontSize: 16, color: "var(--ink-0)", margin: "0 0 4px" }}>
        wasim-watch · live streaming engine
      </h1>
      <p style={{ fontFamily: "var(--mono)", fontSize: 12, color: "var(--ink-2)", margin: "0 0 16px" }}>
        Real WASM engine, polled per frame. The p05–p95 band tightens as realizations accrue.
      </p>

      {loadError && (
        <div style={{ color: "var(--alarm, #e66)", fontFamily: "var(--mono)", fontSize: 12 }}>
          engine load failed: {loadError}
        </div>
      )}

      <div style={{ display: "flex", gap: 12, alignItems: "center", marginBottom: 16, fontFamily: "var(--mono)", fontSize: 12 }}>
        <button onClick={run} disabled={!!loadError || running} style={btn(running)}>Run</button>
        <button onClick={cancel} disabled={!running} style={btn(!running)}>Cancel</button>
        <span style={{ color: "var(--ink-1)" }}>
          {completed.toLocaleString()} / {total.toLocaleString()} realizations
          {running ? " · streaming…" : completed > 0 ? " · done" : ""}
        </span>
        {bandWidth != null && (
          <span style={{ color: "var(--live)", marginLeft: "auto" }}>
            final band width (p95−p05): {bandWidth.toFixed(1)}
          </span>
        )}
      </div>

      {results ? (
        <BandChart results={results} elementId={WATCH_ELEMENT} />
      ) : (
        <div style={{ color: "var(--ink-2)", fontFamily: "var(--mono)", fontSize: 12, height: 340 }}>
          {engineRef.current || loadError ? "press Run" : "loading engine…"}
        </div>
      )}
    </div>
  );
}

function btn(disabled: boolean): React.CSSProperties {
  return {
    fontFamily: "var(--mono)",
    fontSize: 12,
    padding: "4px 12px",
    background: disabled ? "var(--hair-soft)" : "var(--live-dim)",
    color: disabled ? "var(--ink-2)" : "var(--ink-0)",
    border: "1px solid var(--hair)",
    borderRadius: 4,
    cursor: disabled ? "default" : "pointer",
  };
}
