# Stage-1 RAM Demo — Haul-Truck Overload Decision

**Model:** [`schema_examples_manual/haul_truck_overload_reliability.json`](schema_examples_manual/haul_truck_overload_reliability.json)
**Regression test:** [`engine/tests/haul_truck_overload_smoke.rs`](engine/tests/haul_truck_overload_smoke.rs)
**Spec:** [`HAUL_FLEET_MODEL_SPEC.md`](HAUL_FLEET_MODEL_SPEC.md) §4 (Stage 1) · **Verification context:** [`MARKET_RESEARCH_REHYDRATION_2026-07.md`](MARKET_RESEARCH_REHYDRATION_2026-07.md)

The first buildable, runnable artifact for the RAM beachhead — a single-unit
reliability model that a maintenance manager can read in five minutes. It runs on
today's engine with **zero new features**, and it exercises the exact capability the
RAM incumbents handle worst: **failure hazard as a function of an accumulating state
variable, under uncertainty**.

---

## 1. The decision

A mine loads 240 t-rated trucks to 300 t (+25%) for immediate productivity. Overloading
consumes component life at a *nonlinear* rate. Question: what does a 25% overload actually
cost in asset life, and how certain is that number?

## 2. What the model does

Two trucks — **Truck A (nominal 240 t)** and **Truck B (overloaded 300 t)** — run side by
side sharing **one** per-realization draw of the load exponent, so the comparison is
apples-to-apples on identical physics:

- **Power-law damage.** Each truck's damage stock integrates
  `base_rate · (payload / 240)^β`. `base_rate = 0.2/yr` calibrates nominal design life to
  5 years (damage reaches 1.0 at t = 5).
- **`condition`-basis failure.** A failure state machine fails the truck when its damage
  stock crosses **1.0** — the RAM idiom "fail when accumulated damage ≥ threshold,"
  expressed directly.
- **Uncertainty where it belongs.** The load exponent is a *distribution*,
  `β ~ Triangular(2.5, 3.0, 4.0)`, not a point value — because a decision that flips between
  β = 2.5 and β = 3.5 is a request for better data, not a decision. Metal price is a
  lognormal draw for the NPV side.
- **Readouts.** `uptime_*` (time-to-first-failure), `net_npv_*` (discounted revenue earned
  before failure, minus a one-time replacement cost charged by the FSM on failure).

## 3. Result (3000 realizations, seed 20260728)

| Policy | Mean life | Std | P10 | P50 | P90 | Mean net NPV |
|---|---|---|---|---|---|---|
| Nominal 240 t | **5.05 yr** | 0.000 | 5.05 | 5.05 | 5.05 | 23,829 |
| Overload 300 t | **2.55 yr** | 0.170 | 2.30 | 2.55 | 2.75 | 16,388 |

**Overload / nominal life ratio = 0.505.**

Two findings, both the kind that justify a study:

1. **A 25% overload almost exactly halves component life** (ratio 0.505) at β ≈ 3. Intuition
   says "25% more load, 25% more wear." Intuition is wrong by a factor of two — that is the
   whole point, and it is invisible without the power law.
2. **Overloading doesn't just shorten life, it makes life less predictable.** Nominal life is
   *perfectly deterministic* (std 0.000 — when payload = rated, β cancels), while overload
   life carries a real 2.30→2.75 yr P10–P90 band. The uncertainty is *created by the policy*.
   This reframes the pitch: the decision isn't "5 years vs 2.5 years," it's "a known 5 years
   vs a 2.3–2.8-year range you now have to plan maintenance around."

(The NPV columns show the tradeoff is live — the overloaded truck earns faster but dies
sooner and takes its replacement hit earlier; whether overload *wins* depends on price and
discount rate, which is exactly the policy question. NPV here is revenue-to-first-failure and
is illustrative, not the headline — see §5.)

## 4. Why this is the right first artifact

- **Runs on the verified engine today.** No array executor, no new primitives. Every
  mechanism used (power-law `expression`, damage `stock`, `condition`-basis failure `event`,
  MC over a `Triangular` draw, discounted NPV) was confirmed live in the source-verification
  pass ([`MARKET_RESEARCH_REHYDRATION_2026-07.md`](MARKET_RESEARCH_REHYDRATION_2026-07.md) §1).
- **Closes a verification caveat.** The `condition` failure basis is now exercised by a *real
  accumulating damage stock*, not just a clock trigger — the one gap flagged in the RAM
  primitive check.
- **Bit-identical.** Same seed → byte-identical results (asserted in the test), the
  reproducibility guarantee no RAM incumbent publishes.

## 5. What Stage 1 deliberately omits (the honest edges)

These are *sharpening*, not apology — each is a named next step, not a hidden gap:

- **No fleet.** One truck per policy. Damage-spread across trucks, wear-levelling dispatch,
  and the availability feedback loop are **Stage 3** (needs the array/dimension executor,
  which the verification pass found *already landed* — so Stage 3 is now unblocked).
- **No grade-conditional policy.** The core "damage is paid in tonnes, revenue is earned in
  metal" insight (spec §1.2) needs a grade signal and a policy switch — **Stage 1b**, a small
  addition (an `if` on a sampled grade).
- **Run-to-failure economics.** NPV is revenue-to-first-failure with a single replacement
  charge; repair/replace cycles over the full horizon are Stage 1b. The life numbers (the
  headline) are unaffected by this.
- **No availability feedback.** Damage accrual is not yet gated on fleet availability (spec
  §1.4) — that loop needs multiple trucks, so it arrives with Stage 3.

## 6. Reproduce

```bash
cd engine
cargo test --test haul_truck_overload_smoke -- --nocapture
```

## 7. The three questions to put to a mine contact (spec §5)

Before Stage 2/3 build effort, validate the *framing* — the model is only as good as these
answers:

1. How is the overload decision made today — spreadsheet, OEM study, or judgment?
2. **What load exponent do you believe?** If nobody knows, the value-of-information output
   (how much the life spread narrows once β is pinned) becomes the headline deliverable.
3. **Is the binding constraint trucks, shovel, crusher, or tire supply?** If the mill is the
   bottleneck, overloading buys nothing and the whole model reframes. *Ask this first — it is
   the one most likely to invalidate the framing.*
