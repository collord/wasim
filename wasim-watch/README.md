# WaSim · watch it run

A **Layer-1 "watch it run"** prototype for the WaSim engine: a scrubbing,
playing diagram view where the model graph itself comes alive — stocks fill
and drain, flows pulse at their current rate, state machines flip green→red,
controllers toggle, and events flash on a timeline.

It is deliberately **a pure view over a completed run**. The simulation runs
once (here, faked; in production, the WASM engine), retains its state, and this
UI animates that retained state. Playback never re-executes the model.

## Run it

```bash
npm install
npm run dev        # opens http://localhost:5180
```

`npm run build` type-checks and bundles; `npm run preview` serves the build.

## What you're looking at

The demo model is a **pumped reservoir with a failing pump**:

- **Storm inflow** (flow) — Poisson storm events dump slugs of water in.
- **Reservoir** (stock) — integrates inflow − outflow on a fixed Euler grid,
  clamped to capacity with overflow spill.
- **Stage** (expression) — a live scalar derived from level.
- **Pump call** (controller) — an on/off hysteresis latch: pump ON above the
  upper level, OFF below the lower level.
- **Pump unit** (state machine) — a working/failed automaton with repair; when
  it fails, drawdown stops and the reservoir can climb toward spill.
- **Pumped outflow** (flow) — drains the reservoir when the pump is on+working.

Two view modes (top-right selector):

- **Uncertainty cloud** — the aggregate p05–p95 band across 60 realizations.
  Stocks show the median fill with the uncertainty band shaded behind; discrete
  elements show fractions ("38% on", "12% failed"). This is the view no other
  tool really does: **watch uncertainty itself evolve.**
- **Single realization / exemplar** — pin one story (e.g. the peak-level worst
  case) and watch its concrete states and events play through the diagram.

## Architecture — where to plug in the real engine

```
src/
  engine/
    contract.ts    ← THE INTEGRATION BOUNDARY. Typed description of what a
                     completed WaSim run hands the renderer: the fixed time
                     grid, per-element aggregate bands, per-realization traces,
                     events, exemplars. Read this first.
    synthetic.ts   ← Fakes a RunResult with real Euler integration + Monte-
                     Carlo spread. REPLACE THIS with `wasm.run(model)` returning
                     a RunResult and nothing in render/ or ui/ changes.
  render/
    glyphs.tsx     ← One SVG glyph per element kind (stock gauge, state lamp,
                     controller toggle, expression chip). Switches on `kind`.
    FlowLink.tsx   ← Flow connector: dash speed/width = rate, direction reverses
                     on sign change, boundary stubs for null endpoints.
    Diagram.tsx    ← Assembles glyphs + links at the current playhead. Pure view.
  ui/
    Transport.tsx  ← Play/pause/step, scrubber with event ticks, mode selector.
  App.tsx          ← Playback loop (rAF advancing the playhead over stored
                     history) + wiring. `generateRun()` is the swap point.
```

The contract is the important artifact. Everything the animation needs is
already state a real run retains — the view adds no simulation, only rendering.
The two engine properties this leans on:

- **`fixed` timebase is bit-identical**, so any single realization replays
  exactly and a "watch realization #37" link is shareable and reproducible.
- **`event_accurate` sub-steps land cleanly on stock-bound crossings**, so if
  you animate in that mode, gauges hit caps/thresholds exactly on-screen instead
  of overshooting and snapping back at the moments a viewer is watching closest.

## Roadmap (this is Layer 1)

- **L1 (here):** scrub/play; stock gauges; flow pulses; state/controller
  coloring; event ticks; aggregate-cloud vs single-realization.
- **L2:** richer discrete logic — Markov degradation state chips, gate/fault-tree
  true/false propagation, interrupt "freeze" markers.
- **L3:** deepen the Monte-Carlo view — per-stock ghost-band gauges already
  started; add "jump to the run behind the p95" affordance and cross-realization
  snapshot distributions at a chosen time.
- **L4:** live authoring — expose `fixed`/`interactive` inputs as on-canvas
  sliders and re-run + re-animate on drag (the "Synthesim, but probabilistic and
  client-side" move). Fast because the whole sweep runs locally in WASM.

## Notes for the Code session

- No state persistence, no backend — everything is in-memory and client-side,
  matching the WASM/"data never leaves the machine" posture.
- Reduced-motion is respected (animations disabled under the OS setting).
- The synthetic model is real arithmetic, not scripted keyframes, so if you
  change `PUMP_RATE`, `UPPER`/`LOWER`, or storm/failure rates in
  `engine/synthetic.ts`, the animation responds correctly — a good way to sanity
  check the view before the real engine is wired in.
