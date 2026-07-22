import type { RunResult } from "../engine/contract";

interface Props {
  run: RunResult;
  step: number;
  playing: boolean;
  realizationIndex: number | null;
  onStep: (s: number) => void;
  onTogglePlay: () => void;
  onPickRealization: (i: number | null) => void;
}

export function Transport({
  run,
  step,
  playing,
  realizationIndex,
  onStep,
  onTogglePlay,
  onPickRealization,
}: Props) {
  const last = run.grid.length - 1;
  const t = run.grid[step];

  // event ticks for the currently-viewed realization (or the exemplar in agg)
  const evReal =
    realizationIndex === null
      ? run.realizations[run.exemplars?.[0]?.realizationIndex ?? 0]
      : run.realizations[realizationIndex];

  return (
    <>
      <div className="transport">
        <button className="btn primary" onClick={onTogglePlay}>
          {playing ? "❚❚ pause" : "▶ play"}
        </button>
        <button className="btn" onClick={() => onStep(Math.max(0, step - 1))}>
          ◀ step
        </button>
        <button className="btn" onClick={() => onStep(Math.min(last, step + 1))}>
          step ▶
        </button>

        <div className="scrub">
          <div style={{ position: "relative", flex: 1 }}>
            <input
              type="range"
              min={0}
              max={last}
              value={step}
              onChange={(e) => onStep(Number(e.target.value))}
              aria-label="timeline"
            />
            <svg
              width="100%"
              height="10"
              style={{ position: "absolute", left: 0, top: -12, pointerEvents: "none" }}
            >
              {evReal.events.map((ev, i) => {
                const col =
                  ev.type === "failure"
                    ? "var(--alarm)"
                    : ev.type === "repair"
                    ? "var(--live)"
                    : "var(--amber)";
                return (
                  <rect
                    key={i}
                    x={`${(ev.step / last) * 100}%`}
                    y={2}
                    width={2}
                    height={7}
                    fill={col}
                  />
                );
              })}
            </svg>
          </div>
        </div>

        <div className="clock">
          t = <b>{t.toFixed(0)}</b> {run.timeUnit} · {step}/{last}
        </div>

        <div className="mode">
          view
          <select
            value={realizationIndex === null ? "agg" : String(realizationIndex)}
            onChange={(e) =>
              onPickRealization(e.target.value === "agg" ? null : Number(e.target.value))
            }
          >
            <option value="agg">uncertainty cloud (p05–p95)</option>
            {run.exemplars?.map((ex) => (
              <option key={ex.realizationIndex} value={ex.realizationIndex}>
                exemplar · {ex.label}
              </option>
            ))}
            <option value="0">realization #0</option>
            <option value="7">realization #7</option>
            <option value="15">realization #15</option>
          </select>
        </div>
      </div>

      <div className="legend">
        <span className="k">
          <span className="sw" style={{ background: "var(--live)" }} /> live value / flow
        </span>
        <span className="k">
          <span className="sw" style={{ background: "var(--cloud)", opacity: 0.6 }} /> uncertainty band
        </span>
        <span className="k">
          <span className="sw" style={{ background: "var(--amber)" }} /> event fired
        </span>
        <span className="k">
          <span className="sw" style={{ background: "var(--alarm)" }} /> failed / interrupt
        </span>
      </div>
    </>
  );
}
