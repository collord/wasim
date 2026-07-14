# Finding: `duration: 0` (handoff P0-a) — needs an owner decision

**For:** the WASiM schema/engine owner. **From:** the emit side.
**Re:** EMITTER_HANDOFF.md §3 P0-a ("`duration: 0` on 18 top-level models … looks like
the master clock being dropped/zeroed").

## What I found

It is **not** a dropped/zeroed clock — the source `.gsm` genuinely stores zero. For the
affected models the decoded master clock is `_t = [start, end, duration]` with
**start == end and duration == 0**, and the duration *display string* is literally `"0 s"` /
`"0 day"`:

| model | `_t` (start, end, dur) | `_s58` (dur display) | step |
|---|---|---|---|
| `randomsequencegenerator` (must_pass) | `[3.451e9, 3.451e9, 0]` | `"0 s"` | `"0 s"` |
| `oil_sands_production` | `[3.770e9, 3.770e9, 0]` | `"0 day"` | `"1 day"` |
| `distributions` | `[3.972e9, 3.972e9, 0]` | `"0 s"` | `"0 s"` |

So "emit the actual duration — GoldSim gives it" doesn't hold here: **there is no non-zero
duration in the clock to emit.** (My root-clock selection is correct — it matches
`MasterClockInfo._model is root`, so this isn't a submodel clock bleeding into the parent.)

These 19 are **driver / instant models**: optimization drivers (`designoptimization`,
`probabilisticoptimization`), statistics drivers (`montecarlostatistics`, `sensitivity`,
`customimportance`, `uncertaintyvariability`), sequence/param generators
(`randomsequencegenerator`, `precipgen_par`, `wind_model_parameters`), single-period
calcs (`coffeemachinepurchasedecision`, `distributions`, `rectangular_weir`), and
`unscheduledtimesteps` (event-driven). Their real work is a nested submodel run or a static
evaluation — the top-level timeline is genuinely a point.

Same for the 5 submodels flagged `duration: 0` — their own nested clock also decodes to 0.

## Where I earlier went wrong

I had changed the emit guard to accept `t[2] >= 0` (emit the faithful `0`) precisely because
these clocks store 0. Given the engine rejects `duration <= 0` (`InvalidModel`), **faithful-but-
unrunnable is the wrong trade** — you're right that emit must not produce 0.

## The decision I need from you

The data is 0; the engine needs > 0. Options, roughly in order of my preference:

1. **Synthesize a minimal top-level duration** for a genuine 0-duration clock — e.g. one
   timestep (`duration = step`), or the model's largest submodel duration. Emit-side only,
   unblocks loading, but it's a fabricated number.
2. **Engine/schema allows a driver model** (`duration == 0`, or an explicit `driver: true` /
   "single evaluation" mode) — semantically honest for optimization/statistics drivers whose
   real timeline is the submodel. Needs a schema+engine change, not emit.
3. **Alternate clock-mode RE** — for models like `unscheduledtimesteps` the duration may live
   in a different clock field (event-driven / run-to-date), which I could dig out. But
   `randomsequencegenerator` (must_pass) genuinely stores `0 s`, so this won't cover all 19.

I lean **2** for correctness (these really are 0-duration drivers) with **1** as the emit-side
stopgap if you want them runnable now. Tell me which, and for the submodel case whether you
want `null` (inherit — but the parent is also 0 for drivers) or a synthesized value.

Until then I've **left `duration` at the decoded value** rather than re-fabricate a 365-day
placeholder (which was worse — it invented a wrong number *and* dropped the real
`n_realizations`).

---

## Owner decision (2026-07-13): Option 2 — the engine accepts `duration: 0`

Your read is right, and thank you for the RE — this is not an emit bug, and you were
right to stop fabricating a duration. **Keep emitting the faithful `0`.** The fix is on
the engine/schema side.

The engine now treats a 0-duration model as a **single evaluation**:
`n_steps = max(1, round(duration / timestep))`, so `duration == 0` → one step. The engine
evaluates every element once at `t = start` (stocks return their `initial_value`, since
there is no interval to integrate), captures final values, and stops. The load-time guard
was relaxed from `duration > 0` to `duration >= 0` (negatives are still rejected). This is
semantically honest: these really are 0-duration drivers, so `0` carries the meaning — no
fabricated number, and no new `driver: true` flag needed (that would be redundant with the
duration already being 0).

Documented in `wasim-engine-semantics.md` §9 ("Step count and single-evaluation models")
and the `simulation_settings` description in `wasim-schema-v2.json`; CHANGELOG note under
0.8.2 (no `$id` bump — behavior/description only, no shape change).

**What this means for emit:**

- **Top-level `duration: 0`** — emit it faithfully. The engine runs these now. No change
  needed on your side for the 18 models; drop any `t[2] >= 0` guard hesitation.
- **Submodel `duration: 0`** — same: emit the faithful decoded value. A submodel with
  `duration: 0` runs one evaluation per realization, which is exactly right for a
  Monte-Carlo statistics driver (`n_realizations` samples at a single time point). Do
  **not** substitute `null`-to-inherit for these, because the parent is also 0 for driver
  models — inheriting would just propagate 0 anyway, and the explicit 0 is clearer.
- **Option 3 (alternate clock-mode RE for `unscheduledtimesteps` et al.)** — not needed for
  loadability now. If some of these models have a *different* meaningful duration hiding in
  an event-driven / run-to-date clock field, that's a separate faithfulness question worth
  a look eventually, but it no longer blocks anything. `randomsequencegenerator` genuinely
  being `0 s` is fine as-is.

Net: this item (handoff P0-a) is **resolved on the engine side**; emit keeps the faithful
`0`. The remaining P0 item (P0-b, `submodel_stat` still stubbed) is unaffected and still
open per `SUBMODEL_STAT_ENCODING.md`.
