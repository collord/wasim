# Stage-3d — The Shop is the Constraint

**Model:** [`parameters_examples/haul_fleet_dispatch_shop.json`](parameters_examples/haul_fleet_dispatch_shop.json)
**Regression test:** [`engine/tests/haul_fleet_dispatch_shop_v2.rs`](engine/tests/haul_fleet_dispatch_shop_v2.rs)
**Completes the arc from:** [`STAGE3B_DISPATCH_POLICY.md`](STAGE3B_DISPATCH_POLICY.md) → [`STAGE3C_YOUNG_VS_OLD.md`](STAGE3C_YOUNG_VS_OLD.md)

Stage-3c showed wear-levelling produces the most *balanced* fleet. This stage adds the thing
that makes balance a liability: a **repair shop with finite capacity** — a bay, or a mechanic.
When repairs are the bottleneck, wear-levelling's *synchronized* failures overwhelm it, and the
"balanced" policy ends up with the **most** downtime.

## The model

Two layers:

1. **Dispatch / overhaul** (Stage-3c): six trucks, heterogeneous ages, one overloaded per step
   by `dispatch_policy`, trucks overhaul on failure. This layer sets the **failure timing** —
   clustered under wear-levelling (it synchronises ages), staggered under naive round-robin and
   cohorting.
2. **Repair shop** — a fluid queue priced *per realization*:
   `backlog[t] = max(0, backlog_prev + failures_this_step − shop_throughput)`, with
   `shop_throughput = bays / repair_time` (0.12 trucks/step, ~70% utilised against the ~0.083
   mean failure rate). `trucks_down = min(backlog, 6)`; `cum_downtime` integrates it.

The array primitives can't express a top-k bay assignment, so the shop is a fluid/aggregate
approximation of finite capacity — but backlog is computed **inside each realization**, so each
realization's failure **burstiness** drives its own downtime, which is exactly the effect being
measured. (A faithful per-bay queue with a repair *delay* is the noted next step.)

## Result (300 realizations, seed fixed)

| Metric | Naive | Wear-levelling | Cohorting |
|---|---:|---:|---:|
| **Cumulative truck-downtime** (truck-steps) | 47.2 | **78.7** | 74.3 |
| Peak repair backlog (trucks) | 0.85 | 2.16 | 1.51 |

### Wear-levelling incurs ~67% more downtime than naive

When the shop is the constraint, **the most-balanced policy is the worst one.** Wear-levelling
equalizes ages, so trucks reach end-of-life together and fail in **bursts** — the backlog spikes
to **2.2 trucks** waiting, and the single shop takes many steps to clear it, piling up **78.7**
truck-steps of downtime. Naive round-robin phase-distributes the failures into a **steady
trickle** the shop keeps up with (peak backlog 0.85, downtime 47.2) — **35% less lost uptime.**
Cohorting sits between (74.3): it hammers one truck, which fails-and-resets repeatedly, so its
arrivals are semi-bursty.

### The whole arc, in one line each

| Stage | Setting | Who wins | Why |
|---|---|---|---|
| **3b** | symmetric, run-to-failure | cohort worst on *spread*; naive = wear-levelling | no asymmetry for `argmin` to correct |
| **3c** | young vs old + overhaul | **wear-levelling** (most balanced fleet) | it loads the young, spares the old |
| **3d** | + finite repair shop | **naive / staggering** (least downtime) | balanced ⇒ synchronized ⇒ bursty ⇒ queue backs up |

**There is no dominant dispatch policy.** Wear-levelling maximises fleet *evenness* (retirement
value, no stranded remaining life); staggering maximises *availability* when repair capacity is
scarce. Which one a mine should run is an **empirical question about where its binding constraint
is** — trucks, or the shop — and this model is exactly the instrument to answer it for a given
fleet, failure law, and bay/mechanic count. That is a far more useful thing to hand a maintenance
manager than "wear-levelling is best practice."

## What this exercised

- A two-layer model — per-truck array dynamics feeding a scalar fluid queue — entirely in
  authored `vector_map`/`lag`/reduction elements plus `max`/`min` builtins. No new engine work.
- Determinism: the wear-levelling run is bit-identical across repeats.

> **Follow-up ([`STAGE3E_SUBSYSTEMS.md`](STAGE3E_SUBSYSTEMS.md)):** this 67% penalty is inflated
> by treating each truck as a *single composite failure*. Splitting the truck into independent
> subsystems (tires β4 / drivetrain β3 / structure β1.5), whose failures desynchronize, drops the
> wear-levelling penalty to **~30% (1.30× vs 1.67×)** — most of it was a flattening artifact. The
> policy *ranking* survives; the *magnitude* was overstated ~1.8×.

## Next (to make it decision-grade)

1. **Faithful per-bay queue with a repair *delay***: replace the fluid backlog with a real
   G/D/R queue (arrivals held for `repair_time` steps, at most `bays` in service) so downtime is
   in real weeks, and couple it back so a queued truck is genuinely unavailable to haul.
2. **Put dollars on it**: production = ∫ available·haul_rate·grade·price − repair_cost, so the
   spread-vs-downtime tradeoff resolves to NPV per policy at a given shop size.
3. **Sweep shop capacity** (1–4 bays) to find the crossover where wear-levelling overtakes
   staggering — the actual decision boundary.
