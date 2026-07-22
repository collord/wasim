/**
 * Synthetic run generator
 * ======================================================================
 * Produces a `RunResult` shaped exactly like the engine contract, so the
 * view layer can be built and demoed without the WASM engine present.
 *
 * The model it fakes is a small but honest WaSim-style system:
 *
 *     Inflow (Poisson storm events) ──▶ [ Reservoir stock ] ──▶ Outflow
 *                                              │                   ▲
 *                                              ▼                   │
 *                                        stage level ── controls ──┘
 *                                              │
 *                          [ Pump ] (failure/repair state machine)
 *                          gated by an on/off level controller,
 *                          draining the reservoir when working+on.
 *
 * Every realization integrates the same structure with different random
 * draws (storm timing, pump failure/repair times), exactly as a real
 * Monte-Carlo sweep would. We retain per-realization traces AND compute
 * the p05..p95 aggregate bands, matching the default results surface.
 *
 * This is deliberately real arithmetic (Euler integration on a fixed grid)
 * so the animation shows plausible physics, not scripted keyframes.
 */

import type {
  AggregateTrace,
  ElementMeta,
  EventMark,
  Realization,
  RealizationTrace,
  RunResult,
} from "./contract";

/** Small deterministic PRNG (mulberry32) so runs are reproducible per seed. */
function rng(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** Draw an exponential interval with the given rate. */
function expDraw(r: () => number, rate: number): number {
  return -Math.log(1 - r()) / rate;
}

export const MODEL_ELEMENTS: ElementMeta[] = [
  {
    id: "inflow",
    label: "Storm inflow",
    kind: "flow",
    x: 0.1,
    y: 0.28,
    units: "m³/s",
    from: null,
    to: "reservoir",
  },
  {
    id: "reservoir",
    label: "Reservoir",
    kind: "stock",
    x: 0.42,
    y: 0.4,
    units: "Mm³",
    capacity: 100,
  },
  {
    id: "stage",
    label: "Stage",
    kind: "expression",
    x: 0.42,
    y: 0.72,
    units: "m",
  },
  {
    id: "controller",
    label: "Pump call",
    kind: "controller",
    x: 0.72,
    y: 0.72,
  },
  {
    id: "pump",
    label: "Pump unit",
    kind: "state_machine",
    x: 0.72,
    y: 0.4,
    states: ["working", "failed"],
  },
  {
    id: "outflow",
    label: "Pumped outflow",
    kind: "flow",
    x: 0.9,
    y: 0.4,
    units: "m³/s",
    from: "reservoir",
    to: null,
  },
];

export const N_STEPS = 180; // 180-day run
export const DT = 1; // 1-day steps
const CAPACITY = 100;
const UPPER = 70; // controller turns pump ON above this level
const LOWER = 45; // controller turns pump OFF below this level
const PUMP_RATE = 1.6; // drawdown per day when pumping (Mm³/day)
const BASE_INFLOW = 0.35; // steady baseflow (Mm³/day)

export interface SimOut {
  reservoir: number[];
  stage: number[];
  inflow: number[];
  outflow: number[];
  pumpStatus: ("working" | "failed")[];
  controllerStatus: ("on" | "off")[];
  events: EventMark[];
  terminatedAt: number | null;
}

/** Integrate one realization on the fixed grid. Real Euler, real logic. */
export function simulateOne(seed: number): SimOut {
  const r = rng(seed);
  const reservoir: number[] = [];
  const stage: number[] = [];
  const inflow: number[] = [];
  const outflow: number[] = [];
  const pumpStatus: ("working" | "failed")[] = [];
  const controllerStatus: ("on" | "off")[] = [];
  const events: EventMark[] = [];

  let level = 30 + r() * 15; // random initial fill
  let pumpWorking = true;
  let pumpOn = false;
  let nextStorm = expDraw(r, 1 / 14); // storms ~ every 14 days
  let nextFail = expDraw(r, 1 / 90); // MTBF ~ 90 days
  let repairDone = Infinity;

  for (let i = 0; i < N_STEPS; i++) {
    const t = i * DT;

    // --- discrete events resolved at the step (single declaration-order pass)
    let stormPulse = 0;
    if (t >= nextStorm) {
      const volume = 12 + r() * 30; // storm delivers a slug of water
      stormPulse = volume;
      events.push({ step: i, elementId: "inflow", type: "occurrence", label: "storm" });
      nextStorm = t + expDraw(r, 1 / 14);
    }

    if (pumpWorking && t >= nextFail) {
      pumpWorking = false;
      events.push({ step: i, elementId: "pump", type: "failure", label: "pump failed" });
      repairDone = t + 4 + r() * 8; // repair takes 4–12 days
    }
    if (!pumpWorking && t >= repairDone) {
      pumpWorking = true;
      events.push({ step: i, elementId: "pump", type: "repair", label: "repaired" });
      nextFail = t + expDraw(r, 1 / 90);
      repairDone = Infinity;
    }

    // --- on/off controller with hysteresis (bang-bang latch)
    if (!pumpOn && level > UPPER) {
      pumpOn = true;
      events.push({ step: i, elementId: "controller", type: "threshold", label: "call ON" });
    } else if (pumpOn && level < LOWER) {
      pumpOn = false;
      events.push({ step: i, elementId: "controller", type: "threshold", label: "call OFF" });
    }

    // --- flows this step
    const inRate = BASE_INFLOW + stormPulse; // stormPulse is a one-step slug
    const pumping = pumpOn && pumpWorking;
    const outRate = pumping ? PUMP_RATE : 0;

    // --- integrate stock (Euler), clamp to [0, capacity] with overflow spill
    level = level + (inRate - outRate) * DT;
    if (level > CAPACITY) level = CAPACITY; // overflow routing (spill)
    if (level < 0) level = 0;

    reservoir.push(level);
    stage.push((level / CAPACITY) * 12); // 0..12 m stage mapping
    inflow.push(inRate);
    outflow.push(outRate);
    pumpStatus.push(pumpWorking ? "working" : "failed");
    controllerStatus.push(pumpOn ? "on" : "off");
  }

  return {
    reservoir,
    stage,
    inflow,
    outflow,
    pumpStatus,
    controllerStatus,
    events,
    terminatedAt: null,
  };
}

function percentile(sorted: number[], p: number): number {
  const idx = (p / 100) * (sorted.length - 1);
  const lo = Math.floor(idx);
  const hi = Math.ceil(idx);
  if (lo === hi) return sorted[lo];
  return sorted[lo] + (sorted[hi] - sorted[lo]) * (idx - lo);
}

export function generateRun(nRealizations = 60, seedRoot = 4207): RunResult {
  const sims: SimOut[] = [];
  for (let k = 0; k < nRealizations; k++) {
    sims.push(simulateOne(seedRoot + k * 977));
  }

  const grid = Array.from({ length: N_STEPS }, (_, i) => i * DT);

  // --- per-realization traces
  const realizations: Realization[] = sims.map((s, k) => {
    const traces: Record<string, RealizationTrace> = {
      reservoir: { elementId: "reservoir", value: s.reservoir },
      stage: { elementId: "stage", value: s.stage },
      inflow: { elementId: "inflow", value: s.inflow },
      outflow: { elementId: "outflow", value: s.outflow },
      pump: { elementId: "pump", status: s.pumpStatus },
      controller: { elementId: "controller", status: s.controllerStatus },
    };
    return {
      index: k,
      seed: seedRoot + k * 977,
      traces,
      events: s.events,
      terminatedAt: s.terminatedAt,
    };
  });

  // --- aggregate bands for continuous elements
  const continuous: (keyof SimOut)[] = ["reservoir", "stage", "inflow", "outflow"];
  const aggregate: Record<string, AggregateTrace> = {};
  for (const key of continuous) {
    const p05: number[] = [];
    const p25: number[] = [];
    const p50: number[] = [];
    const p75: number[] = [];
    const p95: number[] = [];
    for (let i = 0; i < N_STEPS; i++) {
      const col = sims.map((s) => (s[key] as number[])[i]).sort((a, b) => a - b);
      p05.push(percentile(col, 5));
      p25.push(percentile(col, 25));
      p50.push(percentile(col, 50));
      p75.push(percentile(col, 75));
      p95.push(percentile(col, 95));
    }
    aggregate[key] = { elementId: key, p05, p25, p50, p75, p95 };
  }

  // discrete aggregate: failed-fraction for the pump, on-fraction for controller
  const pumpFailFrac: number[] = [];
  const ctrlOnFrac: number[] = [];
  for (let i = 0; i < N_STEPS; i++) {
    pumpFailFrac.push(sims.filter((s) => s.pumpStatus[i] === "failed").length / sims.length);
    ctrlOnFrac.push(sims.filter((s) => s.controllerStatus[i] === "on").length / sims.length);
  }
  aggregate.pump = { elementId: "pump", activeFraction: pumpFailFrac };
  aggregate.controller = { elementId: "controller", activeFraction: ctrlOnFrac };

  // --- exemplar: the realization with the highest peak reservoir level
  let worst = 0;
  let worstPeak = -Infinity;
  sims.forEach((s, k) => {
    const peak = Math.max(...s.reservoir);
    if (peak > worstPeak) {
      worstPeak = peak;
      worst = k;
    }
  });

  return {
    modelName: "Pumped reservoir · failing pump",
    grid,
    timeUnit: "days",
    timebase: "fixed",
    seedRoot,
    elements: MODEL_ELEMENTS,
    aggregate,
    realizations,
    exemplars: [{ label: "peak level (worst case)", realizationIndex: worst }],
  };
}
