# Stage-3b — Dispatch Policy Comparison

**Model:** [`parameters_examples/haul_fleet_dispatch.json`](parameters_examples/haul_fleet_dispatch.json)
**Regression test:** [`engine/tests/haul_fleet_dispatch_v2.rs`](engine/tests/haul_fleet_dispatch_v2.rs)
**Spec:** [`HAUL_FLEET_MODEL_SPEC.md`](HAUL_FLEET_MODEL_SPEC.md) §1.3 · **Builds on:** [`STAGE3_FLEET_SCOPE.md`](STAGE3_FLEET_SCOPE.md)

Proves the value of *wear-levelling dispatch* by running the fleet against two alternatives —
and turns up a sharper, more honest picture than "wear-levelling wins."

## The setup

Six trucks, run-to-failure, over a 3-year (156-week) horizon. **Every step, exactly one
available truck is assigned the overload** (300 t, ~1.95× the damage rate at β = 3); the rest
haul nominal (240 t). A single `dispatch_policy` switch chooses *which* truck, and the model is
run three times (one value each — no engine work, just a policy switch on `overload_target`,
per the scope doc):

| Policy | `overload_target` | Idea |
|---|---|---|
| **Naive** (0) | `mod(floor(elapsed/timestep), 6) + 1` | Damage-blind round-robin rotation. |
| **Wear-levelling** (1) | `argmin_where(damage_prev, available)` | The least-damaged available truck. |
| **Cohorting** (2) | `argmin_where(truck_index, available)` | Lowest-index available truck — sacrifice T01, then T02, … |

Damage is calibrated so a truck overloaded *every* step (cohorting's T01) fails ~mid-horizon
while nominal-only trucks survive.

## Result (400 realizations, MC over β ~ Triangular(2.5, 3, 4), seed fixed)

| Metric | Naive | Wear-levelling | Cohorting |
|---|---:|---:|---:|
| **Peak damage spread** (max over time of fleet max−min) | 0.006 | 0.006 | **0.488** |
| Available trucks @ horizon (of 6) | 3.38 | 3.38 | 3.81 |
| Trucks failed @ horizon | 2.62 | 2.62 | 2.20 |

### 1. Cohorting drives an ~80× fleet imbalance — the wear-levelling diagnostic works

Concentrating overload on one truck sends its damage racing ahead: the fleet reaches a **peak
spread of 0.49** (one truck near failure while others are barely worn), versus **0.006** under
either balancing policy — an ~80× difference. This is exactly the `damage_spread` diagnostic
the fleet model exposes (spec §2.4): *0 = balanced, large = a few trucks aging fast toward
clustered failure.* Balanced dispatch keeps it near zero; cohorting blows it up.

### 2. Balanced ≠ safer — wear-levelling *clusters* failures (the counterintuitive part)

Equalizing damage **synchronizes lifetimes**: the whole fleet ages together and reaches
end-of-life at nearly the same time, so **more trucks have failed by the horizon** under the
balancing policies (2.62) than under cohorting (2.20). Cohorting deliberately **staggers**
failures — T01 fails early and alone, then T02 — trading a lop-sided fleet for *predictable,
one-at-a-time* maintenance. This is spec §1.3's counterintuitive claim made concrete: when shop
capacity or spares are the binding constraint, the *staggered* failures of a cohorting policy
can beat the *correlated-downtime wave* that perfect wear-levelling sets up. **A balanced fleet
is not automatically a safer one.**

### 3. Honest finding — here, naive round-robin ≈ wear-levelling

Naive and wear-levelling are **indistinguishable** in this run (0.006 spread, 2.62 failed, 3.38
available — identical to three digits). Both spread the overload evenly, and in a *symmetric,
run-to-failure* fleet that is all that matters — `argmin`'s cleverness has no asymmetry to
correct. Wear-levelling only earns its keep when the fleet is **not** symmetric:

- **Replacement / imperfect repair** — a freshly-overhauled truck is young; wear-levelling
  steers overload onto it, round-robin doesn't. (Needs the `status` reset wired to a repair
  condition — currently run-to-failure.)
- **Heterogeneous trucks** — different ages, models, or starting damage.
- **Uneven grade/route exposure** feeding back into damage.

So the defensible claim is **not** "wear-levelling beats round-robin" (it doesn't, here) but
"**cohorting vs balancing is a real, quantified tradeoff, and wear-levelling's advantage over
naive is contingent on fleet asymmetry.**" That is a more useful thing to tell a maintenance
manager than a rigged win — and it names the exact model refinement (replacement) that would
make wear-levelling pull ahead.

## What this exercised

- The masked-reduction builtins (`argmin_where`) and the array lane — all three policies are a
  one-line change to a scalar `overload_target`, no structural edit.
- Determinism: the wear-levelling run is asserted bit-identical across repeats.
- A modelling gotcha worth recording: **`elapsed / timestep`** (both `time_ref`, same unit) is
  the step index; **`TimestepLength`** is `dt` in *seconds*, so `elapsed / TimestepLength` is a
  unit mismatch that silently collapses a round-robin to "always truck 1."

## Next

1. **Add replacement** (`status` reset on a repair-completion condition + a repair delay) so the
   young-vs-old asymmetry appears — then re-run and show wear-levelling separating from naive.
2. **Shop-capacity queue** (finite bays) so the clustered-vs-staggered failure timing turns into
   a *downtime / lost-production* difference, not just a failure count — that is where cohorting
   can win outright.
3. Scale to N = 40 and add the grade/price-conditional overload gate from the base fleet model.
