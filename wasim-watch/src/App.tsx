import { useEffect, useMemo, useRef, useState } from "react";
import { generateRun } from "./engine/synthetic";
import { Diagram } from "./render/Diagram";
import { Transport } from "./ui/Transport";

/**
 * Top-level app. In production, `generateRun()` is replaced by a call into
 * the WaSim WASM engine returning the same `RunResult` shape — the run
 * completes, its state is retained, and THIS view animates the retained
 * state. Playback never re-executes the simulation.
 */
export default function App() {
  // one completed run, memoized (the "run once, then view" contract)
  const run = useMemo(() => generateRun(60), []);
  const last = run.grid.length - 1;

  const [step, setStep] = useState(0);
  const [playing, setPlaying] = useState(true);
  const [realizationIndex, setRealizationIndex] = useState<number | null>(null);

  // playback loop — advances the playhead over stored history at ~24 steps/s
  const raf = useRef<number | null>(null);
  const accum = useRef(0);
  const lastT = useRef<number | null>(null);
  useEffect(() => {
    if (!playing) {
      if (raf.current) cancelAnimationFrame(raf.current);
      lastT.current = null;
      return;
    }
    const tick = (now: number) => {
      if (lastT.current !== null) {
        accum.current += now - lastT.current;
        const stepsPerSec = 24;
        while (accum.current > 1000 / stepsPerSec) {
          accum.current -= 1000 / stepsPerSec;
          setStep((s) => (s >= last ? 0 : s + 1));
        }
      }
      lastT.current = now;
      raf.current = requestAnimationFrame(tick);
    };
    raf.current = requestAnimationFrame(tick);
    return () => {
      if (raf.current) cancelAnimationFrame(raf.current);
    };
  }, [playing, last]);

  return (
    <div className="app">
      <header className="masthead">
        <h1>
          WaSim <span className="dim">· watch it run</span>
        </h1>
        <span className="timebase">timebase: {run.timebase}</span>
        <span className="model">{run.modelName}</span>
      </header>

      <div className="stage">
        <Diagram run={run} step={step} realizationIndex={realizationIndex} />
      </div>

      <div>
        <Transport
          run={run}
          step={step}
          playing={playing}
          realizationIndex={realizationIndex}
          onStep={(s) => {
            setStep(s);
          }}
          onTogglePlay={() => setPlaying((p) => !p)}
          onPickRealization={setRealizationIndex}
        />
      </div>
    </div>
  );
}
