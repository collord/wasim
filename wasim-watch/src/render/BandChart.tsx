// A self-contained band chart for one streamed element: the p05-p95 spread as a shaded cone
// with the p50 median line, over the run's time steps. Redrawn on every poll, so the band
// visibly tightens as realizations accrue. Consumes only what the poll payload carries
// (mean + percentiles) — no per-realization traces, no layout.
import type { SimulationResults } from "../engine/simResults";

interface Props {
  results: SimulationResults;
  elementId: string;
  width?: number;
  height?: number;
}

const PAD = { top: 16, right: 16, bottom: 28, left: 48 };

export function BandChart({ results, elementId, width = 720, height = 340 }: Props) {
  const el = results.elements[elementId];
  const th = el?.time_history;
  if (!th || th.p05.length === 0) {
    return <div style={{ color: "var(--ink-2)", fontFamily: "var(--mono)" }}>no time history for {elementId}</div>;
  }

  const steps = th.p05.length;
  const xs = results.time_axis.length === steps ? results.time_axis : th.p05.map((_, i) => i);

  // y-domain from the widest band (p05..p95) with a little headroom.
  let lo = Infinity, hi = -Infinity;
  for (let i = 0; i < steps; i++) {
    lo = Math.min(lo, th.p05[i]);
    hi = Math.max(hi, th.p95[i]);
  }
  if (!isFinite(lo) || !isFinite(hi) || lo === hi) { lo = 0; hi = 1; }
  const padY = (hi - lo) * 0.06;
  lo -= padY; hi += padY;

  const innerW = width - PAD.left - PAD.right;
  const innerH = height - PAD.top - PAD.bottom;
  const sx = (i: number) => PAD.left + (innerW * (xs[i] - xs[0])) / (xs[steps - 1] - xs[0] || 1);
  const sy = (v: number) => PAD.top + innerH * (1 - (v - lo) / (hi - lo));

  // shaded p05-p95 area: forward along p95, back along p05.
  const bandPath =
    th.p95.map((v, i) => `${i === 0 ? "M" : "L"}${sx(i).toFixed(1)},${sy(v).toFixed(1)}`).join("") +
    th.p05.map((_, i) => `L${sx(steps - 1 - i).toFixed(1)},${sy(th.p05[steps - 1 - i]).toFixed(1)}`).join("") +
    "Z";
  const medianPath = th.p50.map((v, i) => `${i === 0 ? "M" : "L"}${sx(i).toFixed(1)},${sy(v).toFixed(1)}`).join("");

  const yTicks = [lo, (lo + hi) / 2, hi];

  return (
    <svg width={width} height={height} style={{ display: "block", fontFamily: "var(--mono)" }}>
      {/* y grid + labels */}
      {yTicks.map((v, i) => (
        <g key={i}>
          <line x1={PAD.left} x2={width - PAD.right} y1={sy(v)} y2={sy(v)} stroke="var(--hair-soft)" strokeWidth={1} />
          <text x={PAD.left - 8} y={sy(v) + 3} textAnchor="end" fontSize={10} fill="var(--ink-2)">
            {v.toFixed(0)}
          </text>
        </g>
      ))}
      {/* x endpoints */}
      <text x={PAD.left} y={height - 10} fontSize={10} fill="var(--ink-2)">
        {xs[0].toFixed(0)}
      </text>
      <text x={width - PAD.right} y={height - 10} textAnchor="end" fontSize={10} fill="var(--ink-2)">
        {xs[steps - 1].toFixed(0)} {results.time_unit}
      </text>

      {/* p05-p95 cone */}
      <path d={bandPath} fill="var(--cloud)" fillOpacity={0.22} stroke="none" />
      {/* p50 median */}
      <path d={medianPath} fill="none" stroke="var(--live)" strokeWidth={2} />

      <text x={width - PAD.right} y={PAD.top} textAnchor="end" fontSize={11} fill="var(--ink-1)">
        {el.label} — p05–p95 band, p50 median
      </text>
    </svg>
  );
}
