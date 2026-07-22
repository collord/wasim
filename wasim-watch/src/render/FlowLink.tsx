/**
 * FlowLink: draws a flow element as a connector between its from/to glyphs,
 * with dash animation whose speed & thickness encode the current rate and
 * whose direction reverses if the rate goes negative. A null endpoint is a
 * system boundary (drawn as a short stub with an open chevron).
 */
import type { ElementMeta } from "../engine/contract";
import { toPx } from "./glyphs";

interface Props {
  meta: ElementMeta;
  rate: number;
  maxRate: number;
  recent?: boolean;
}

function anchor(id: string | null | undefined, elements: ElementMeta[]): [number, number] | null {
  if (!id) return null;
  const e = elements.find((m) => m.id === id);
  if (!e) return null;
  return toPx(e.x, e.y);
}

export function FlowLink({
  meta,
  rate,
  maxRate,
  recent,
  elements,
}: Props & { elements: ElementMeta[] }) {
  const [selfx, selfy] = toPx(meta.x, meta.y);
  const fromPt = anchor(meta.from, elements) ?? [selfx - 70, selfy];
  const toPt = anchor(meta.to, elements) ?? [selfx + 70, selfy];

  const mag = Math.abs(rate);
  const frac = maxRate > 0 ? Math.min(1, mag / maxRate) : 0;
  const active = frac > 0.001;
  const width = 1.5 + frac * 5;
  // dash speed: faster when flowing faster
  const dur = active ? Math.max(0.4, 1.8 - frac * 1.4) : 0;
  const reversed = rate < 0;

  const [x1, y1] = reversed ? toPt : fromPt;
  const [x2, y2] = reversed ? fromPt : toPt;

  return (
    <g>
      <line
        x1={fromPt[0]}
        y1={fromPt[1]}
        x2={toPt[0]}
        y2={toPt[1]}
        stroke="var(--hair-soft)"
        strokeWidth={1}
      />
      {active && (
        <line
          x1={x1}
          y1={y1}
          x2={x2}
          y2={y2}
          stroke={recent ? "var(--amber)" : "var(--live)"}
          strokeWidth={width}
          strokeLinecap="round"
          strokeDasharray="2 10"
          opacity={0.9}
        >
          <animate
            attributeName="stroke-dashoffset"
            from="12"
            to="0"
            dur={`${dur}s`}
            repeatCount="indefinite"
          />
        </line>
      )}
      <text
        x={(fromPt[0] + toPt[0]) / 2}
        y={(fromPt[1] + toPt[1]) / 2 - 8}
        textAnchor="middle"
        className="g-flowlabel"
      >
        {meta.label} · {rate.toFixed(2)}
      </text>
    </g>
  );
}
