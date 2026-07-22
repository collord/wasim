/**
 * Glyphs: one SVG renderer per element kind.
 * Each receives the value/status it holds AT THE PLAYHEAD plus a `recent`
 * flag (an event fired within the last few steps) so it can flash.
 * These are pure functions of the frame — no internal simulation.
 */
import type { ElementMeta } from "../engine/contract";

const W = 1000;
const H = 560;

/** Map normalized model coords (0..1) to canvas px. */
export function toPx(x: number, y: number): [number, number] {
  return [40 + x * (W - 80), 30 + y * (H - 60)];
}
export const CANVAS = { W, H };

interface GlyphProps {
  meta: ElementMeta;
  /** Continuous value at playhead (median or single realization). */
  value?: number;
  /** Aggregate band at playhead, for the "cloud" fill. */
  band?: { p05: number; p50: number; p95: number };
  /** Discrete status at playhead. */
  status?: string;
  /** Fraction in active/failed state (aggregate discrete). */
  activeFraction?: number;
  /** An event fired on this element within the flash window. */
  recent?: boolean;
  /** Show aggregate (cloud) vs single-realization styling. */
  aggregate: boolean;
}

/* ---------------------------------------------------------------- stock */
function Stock({ meta, value, band, recent, aggregate }: GlyphProps) {
  const [cx, cy] = toPx(meta.x, meta.y);
  const w = 96;
  const h = 108;
  const x = cx - w / 2;
  const y = cy - h / 2;
  const cap = meta.capacity ?? 100;

  const fillFrac = Math.max(0, Math.min(1, (value ?? 0) / cap));
  const fillH = fillFrac * (h - 8);

  // uncertainty cloud: shade between p05 and p95 fill levels
  const p05f = band ? Math.max(0, Math.min(1, band.p05 / cap)) : fillFrac;
  const p95f = band ? Math.max(0, Math.min(1, band.p95 / cap)) : fillFrac;
  const cloudTop = y + (h - 4) - p95f * (h - 8);
  const cloudBot = y + (h - 4) - p05f * (h - 8);

  return (
    <g>
      <rect
        x={x}
        y={y}
        width={w}
        height={h}
        rx={5}
        fill="#10141a"
        stroke={recent ? "var(--amber)" : "var(--hair)"}
        strokeWidth={recent ? 2 : 1}
      />
      {aggregate && band && (
        <rect
          x={x + 3}
          y={cloudTop}
          width={w - 6}
          height={Math.max(0, cloudBot - cloudTop)}
          fill="var(--cloud)"
          opacity={0.28}
        />
      )}
      {/* the live fill (median / single realization) */}
      <rect
        x={x + 3}
        y={y + (h - 4) - fillH}
        width={w - 6}
        height={fillH}
        fill="var(--live-dim)"
        opacity={0.55}
      />
      <rect
        x={x + 3}
        y={y + (h - 4) - fillH}
        width={w - 6}
        height={2}
        fill="var(--live)"
      />
      <text x={cx} y={y - 8} textAnchor="middle" className="g-label">
        {meta.label}
      </text>
      <text x={cx} y={cy + 4} textAnchor="middle" className="g-readout">
        {(value ?? 0).toFixed(1)}
      </text>
      <text x={cx} y={cy + 20} textAnchor="middle" className="g-unit">
        {meta.units}
      </text>
    </g>
  );
}

/* ------------------------------------------------------------- expression */
function Expression({ meta, value, recent }: GlyphProps) {
  const [cx, cy] = toPx(meta.x, meta.y);
  const w = 92;
  const h = 40;
  return (
    <g>
      <rect
        x={cx - w / 2}
        y={cy - h / 2}
        width={w}
        height={h}
        rx={4}
        fill="var(--bench-2)"
        stroke={recent ? "var(--amber)" : "var(--hair)"}
      />
      <text x={cx} y={cy - h / 2 - 6} textAnchor="middle" className="g-label">
        {meta.label}
      </text>
      <text x={cx} y={cy + 2} textAnchor="middle" className="g-readout sm">
        {(value ?? 0).toFixed(2)}
      </text>
      <text x={cx} y={cy + 15} textAnchor="middle" className="g-unit">
        {meta.units}
      </text>
    </g>
  );
}

/* ---------------------------------------------------------- state machine */
function StateMachine({ meta, status, activeFraction, recent, aggregate }: GlyphProps) {
  const [cx, cy] = toPx(meta.x, meta.y);
  const r = 26;
  const failed = status === "failed";
  // aggregate: color by failed-fraction (green→red)
  const frac = activeFraction ?? (failed ? 1 : 0);
  const lamp = aggregate
    ? `color-mix(in srgb, var(--alarm) ${Math.round(frac * 100)}%, var(--live))`
    : failed
    ? "var(--alarm)"
    : "var(--live)";
  return (
    <g>
      <circle
        cx={cx}
        cy={cy}
        r={r}
        fill="#10141a"
        stroke={recent ? "var(--amber)" : "var(--hair)"}
        strokeWidth={recent ? 2 : 1}
      />
      <circle cx={cx} cy={cy} r={r - 8} fill={lamp} opacity={0.9}>
        {recent && (
          <animate
            attributeName="opacity"
            values="0.4;1;0.4"
            dur="0.7s"
            repeatCount="2"
          />
        )}
      </circle>
      <text x={cx} y={cy - r - 8} textAnchor="middle" className="g-label">
        {meta.label}
      </text>
      <text x={cx} y={cy + r + 16} textAnchor="middle" className="g-status">
        {aggregate ? `${Math.round(frac * 100)}% failed` : status}
      </text>
    </g>
  );
}

/* -------------------------------------------------------------- controller */
function Controller({ meta, status, activeFraction, recent, aggregate }: GlyphProps) {
  const [cx, cy] = toPx(meta.x, meta.y);
  const w = 58;
  const h = 26;
  const on = status === "on";
  const frac = activeFraction ?? (on ? 1 : 0);
  const knobX = aggregate
    ? cx - w / 2 + 6 + frac * (w - 24)
    : on
    ? cx + w / 2 - 15
    : cx - w / 2 + 6;
  return (
    <g>
      <rect
        x={cx - w / 2}
        y={cy - h / 2}
        width={w}
        height={h}
        rx={h / 2}
        fill={on || aggregate ? "var(--live-dim)" : "var(--bench-3)"}
        stroke={recent ? "var(--amber)" : "var(--hair)"}
        strokeWidth={recent ? 2 : 1}
      />
      <circle cx={knobX} cy={cy} r={8} fill={on ? "var(--live)" : "var(--ink-2)"} />
      <text x={cx} y={cy - h / 2 - 6} textAnchor="middle" className="g-label">
        {meta.label}
      </text>
      <text x={cx} y={cy + h / 2 + 14} textAnchor="middle" className="g-status">
        {aggregate ? `${Math.round(frac * 100)}% on` : on ? "ON" : "OFF"}
      </text>
    </g>
  );
}

export function Glyph(props: GlyphProps) {
  switch (props.meta.kind) {
    case "stock":
      return <Stock {...props} />;
    case "expression":
      return <Expression {...props} />;
    case "state_machine":
    case "markov":
      return <StateMachine {...props} />;
    case "controller":
    case "gate":
      return <Controller {...props} />;
    default:
      return null;
  }
}
