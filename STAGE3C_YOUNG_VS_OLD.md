# Stage-3c — Young vs Old: Overhaul + Heterogeneous Ages

**Model:** [`parameters_examples/haul_fleet_dispatch_repair.json`](parameters_examples/haul_fleet_dispatch_repair.json)
**Regression test:** [`engine/tests/haul_fleet_dispatch_repair_v2.rs`](engine/tests/haul_fleet_dispatch_repair_v2.rs)
**Resolves the caveat in:** [`STAGE3B_DISPATCH_POLICY.md`](STAGE3B_DISPATCH_POLICY.md) §3

Stage-3b turned up an honest null result: in a *symmetric, run-to-failure* fleet, naive
round-robin and wear-levelling are **identical** (both 0.006 peak spread) — `argmin` has no
asymmetry to correct. This stage adds the asymmetry the scope doc named — **young vs old** — and
shows wear-levelling separating.

## What changed from Stage-3b

1. **Heterogeneous initial ages.** Trucks start on a damage ramp `[0, 0.15, 0.30, 0.45, 0.60,
   0.75]` — T01 brand new, T06 three-quarters worn.
2. **Instant overhaul.** A truck that reaches the failure threshold is down for one step, then
   re-enters with damage reset to 0 (young again). The fleet therefore keeps a **continuous mix
   of ages** for the whole horizon, instead of everyone marching to failure together.

Everything else is Stage-3b: six trucks, one overloaded per step, `dispatch_policy` selects
which (naive round-robin / wear-levelling `argmin_where(damage_prev, available)` / cohorting),
run three times, MC over the load exponent.

## Result (400 realizations, seed fixed)

| Metric | Naive | Wear-levelling | Cohorting |
|---|---:|---:|---:|
| **Steady-state damage spread** (mean over 2nd half) | 0.828 | **0.684** | 0.778 |
| Overhauls over the horizon | 13.12 | 12.67 | 13.47 |

### 1. Wear-levelling now separates from naive — and is the most balanced

In Stage-3b, `naive == wear-levelling` to three digits. Here they differ by **0.14** (0.684 vs
0.828), and wear-levelling is the **most balanced of all three** policies. The mechanism is
exactly the young-vs-old story: a freshly-overhauled truck is the least-damaged available, so
`argmin_where(damage_prev, available)` steers the overload straight onto it — **loading the
young, sparing the old** — which pulls the fleet's ages together. Round-robin, blind to age,
lets trucks drift across the full sawtooth, so it carries the largest spread. **This is the
result the Stage-3b caveat predicted: wear-levelling's advantage is real, but only once the
fleet is not symmetric.**

### 2. No free lunch — total overhaul work is ~policy-independent

All three policies do **12.7–13.5 overhauls** over the horizon (within ~6% of each other). The
total damage inflicted per step is fixed (one overload, whoever gets it), so the policies differ
in *how wear is distributed, not how much*. Wear-levelling's value is **evenness** — no single
truck bears disproportionate wear, and less remaining life is stranded in young trucks at
fleet retirement — **not** a lower failure count. That is the honest pitch to a maintenance
manager: wear-levelling buys you a *uniform, predictable* fleet, not *fewer* overhauls.

### 3. Overhaul flips the cohorting intuition

In Stage-3b (run-to-failure) cohorting produced the largest imbalance. With overhaul it is the
**middle** policy (0.778): cohorting keeps hammering its lowest-index truck, which therefore
**fails and resets repeatedly** (fail → overhaul → fail), so a freshly-young member is almost
always present, capping the max−min. The extreme-imbalance reading of cohorting only holds when
trucks are *not* replaced. A good reminder that a policy's ranking depends on the maintenance
regime it runs under, not just the dispatch rule.

## What this exercised

- The full authored-array vocabulary end to end: a per-member `fixed` age ramp
  (`initial_damage`), a nested-`if` overhaul-reset recurrence in a `vector_map` body, availability
  gating, `argmin_where` dispatch, and a scalar `lag`-based `cum_failures` counter — all on
  today's engine, no new features.
- Determinism: the wear-levelling run is bit-identical across repeats.

## Next

- **Repair *delay* + a shop-capacity queue** (finite bays): turn the overhaul from instant to a
  multi-step downtime through a queue, so the clustered-vs-staggered failure timing becomes a
  *lost-production* difference. That is the setting where the Stage-3b tradeoff (wear-levelling
  synchronizes, cohorting staggers) shows up as dollars, and where cohorting can win outright.
- Scale to N = 40 and fold in the grade/price-conditional overload gate from the base fleet model.
