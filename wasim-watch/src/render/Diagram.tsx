/**
 * Diagram: the "watch it run" stage. Given a RunResult, the current step,
 * and the view mode, it computes each element's frame value at the playhead
 * and renders the glyph graph. Pure view — no simulation here.
 */
import type { RunResult } from "../engine/contract";
import { CANVAS, Glyph } from "./glyphs";
import { FlowLink } from "./FlowLink";

interface Props {
  run: RunResult;
  step: number;
  /** null = aggregate/cloud view; a number = pinned single realization. */
  realizationIndex: number | null;
  /** steps within which an event still "flashes". */
  flashWindow?: number;
}

export function Diagram({ run, step, realizationIndex, flashWindow = 3 }: Props) {
  const aggregate = realizationIndex === null;
  const real = aggregate ? null : run.realizations[realizationIndex];

  // recent-event set for flashing
  const recent = new Set<string>();
  const src = real ? real.events : run.realizations[run.exemplars?.[0]?.realizationIndex ?? 0].events;
  if (real) {
    for (const e of real.events) {
      if (e.step <= step && e.step > step - flashWindow) recent.add(e.elementId);
    }
  }

  // max rates for flow scaling (from aggregate p95 so widths are stable)
  const flowMax: Record<string, number> = {};
  for (const el of run.elements) {
    if (el.kind === "flow") {
      const agg = run.aggregate[el.id];
      flowMax[el.id] = agg?.p95 ? Math.max(...agg.p95) : 1;
    }
  }

  function continuousValue(id: string): number {
    if (real) return real.traces[id]?.value?.[step] ?? 0;
    return run.aggregate[id]?.p50?.[step] ?? 0;
  }
  function band(id: string) {
    const a = run.aggregate[id];
    if (!a?.p05) return undefined;
    return { p05: a.p05[step], p50: a.p50![step], p95: a.p95![step] };
  }
  function status(id: string): string | undefined {
    if (real) return real.traces[id]?.status?.[step];
    return undefined;
  }
  function activeFraction(id: string): number | undefined {
    return run.aggregate[id]?.activeFraction?.[step];
  }

  return (
    <svg viewBox={`0 0 ${CANVAS.W} ${CANVAS.H}`} preserveAspectRatio="xMidYMid meet">
      <style>{`
        .g-label{ font-family:var(--mono); font-size:12px; fill:var(--ink-1); }
        .g-readout{ font-family:var(--mono); font-size:20px; font-weight:600; fill:var(--live); }
        .g-readout.sm{ font-size:15px; }
        .g-unit{ font-family:var(--mono); font-size:10px; fill:var(--ink-2); }
        .g-status{ font-family:var(--mono); font-size:10.5px; fill:var(--ink-1); }
        .g-flowlabel{ font-family:var(--mono); font-size:10px; fill:var(--ink-2); }
      `}</style>

      {/* flows first, so glyphs sit on top of the connectors */}
      {run.elements
        .filter((e) => e.kind === "flow")
        .map((e) => (
          <FlowLink
            key={e.id}
            meta={e}
            rate={continuousValue(e.id)}
            maxRate={flowMax[e.id]}
            recent={recent.has(e.id)}
            elements={run.elements}
          />
        ))}

      {run.elements
        .filter((e) => e.kind !== "flow")
        .map((e) => (
          <Glyph
            key={e.id}
            meta={e}
            aggregate={aggregate}
            value={continuousValue(e.id)}
            band={band(e.id)}
            status={status(e.id)}
            activeFraction={activeFraction(e.id)}
            recent={recent.has(e.id)}
          />
        ))}

      {/* suppress unused warning when aggregate */}
      {src ? null : null}
    </svg>
  );
}
