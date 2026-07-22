# Model Spec: Haul Fleet Overload Policy Under Grade and Price Uncertainty

*A concrete RAM/asset-decision model for surface mining, specified against WaSim's actual primitives — with an explicit accounting of what the engine supports today, what needs bolstering, and what is genuinely missing.*

**The decision.** A mine is loading 240 t nominal trucks to ~300 t. Overloading buys immediate productivity and consumes asset life at a nonlinear rate. The question is not "is overloading good" but **"what is the optimal loading policy, given ore grade, commodity price, haul profile, fleet availability, and shop capacity — and what does it cost us in the tail?"**

**Why this problem, strategically.** It is a genuine commercial decision owned by a maintenance manager or mine GM, it has nothing to do with contaminants or regulators, and it stresses exactly the modeling capability the RAM incumbents handle worst: **failure hazard as a function of an accumulating state variable, coupled through feedback to the system's own availability, under a policy that switches on exogenous stochastic inputs.**

---

## 1. The physics and economics being modeled

### 1.1 The nonlinearity that makes this a modeling problem

Overloading by 25% does not cost 25% of component life. Load-to-life relationships in haul equipment are **power laws**:

| Component class | Life relationship | Typical exponent |
|---|---|---|
| Tires | TKPH (tonne-km/h) exceedance derating | 2–4 (steep above rated TKPH) |
| Rolling-element bearings / final drives | L10 life ∝ (C/P)^p | 3 (ball), 10/3 (roller) |
| Frame / structural welds | Miner's rule over S-N curve | 3–5 on stress range |
| Suspension struts | fatigue + charge pressure | 2–3 |
| Engine / powertrain | duty-cycle derating | 1.5–2.5 |
| Brakes | energy dissipation ∝ load × grade | ~1–1.5 (near-linear) |

At exponent 3, a 25% overload gives (1.25)³ ≈ **1.95× damage rate — component life roughly halves.** At exponent 4 (tires above rated TKPH) it is 2.4×. This is the crux: intuition says "25% more load, 25% more wear," and intuition is wrong by a factor of two or more.

**The exponent is the single largest source of model uncertainty**, and it must be entered as a *distribution*, not a point value. A decision that flips between exponent 2.5 and 3.5 is not a decision; it is a request for better data.

### 1.2 Ore grade — the variable that makes this a *policy* problem

Grade changes the structure of the question, not just its parameters. Payload value is `payload_tonnes × grade × recovery × price`. A 300 t load of high-grade ore is worth substantially more than a 300 t load of low-grade ore or waste — while the **damage inflicted is identical**, because damage depends on mass, not on what the mass is worth.

That asymmetry has a sharp implication:

> **Damage is paid in tonnes; revenue is earned in metal. Therefore the marginal value of a tonne of accelerated wear is highest when hauling the highest-grade material.**

So the rational policy is not "overload" or "don't overload" — it is **grade-conditional overloading**: overload when the material being hauled is above a grade threshold, run nominal otherwise. Waste and low-grade stockpile material should essentially never be overloaded, because you consume the same asset life for a fraction of the return.

This immediately generates the second structural insight you identified:

### 1.3 Wear-levelling through dispatch rotation

If only high-grade runs are overloaded, and dispatch assigns trucks to routes, then **trucks that draw more high-grade assignments accumulate damage faster**. Left uncontrolled, the fleet develops a damage spread: a few trucks age rapidly toward failure while others stay young. That is bad for two reasons — it clusters failures in the high-damage cohort (correlated downtime), and it wastes remaining life in the low-damage cohort at fleet retirement.

**Wear-levelling** is the dispatch policy that rotates trucks between high-grade (overloaded, high-damage) and waste/low-grade (nominal, low-damage) runs to equalize accumulated damage across the fleet. This is a genuine, actionable operational lever, and it is exactly the kind of thing that is invisible to a spreadsheet and natural to a simulation.

The model should compare at least three dispatch policies:
- **Naive** — nearest-truck dispatch, no damage awareness (the common baseline).
- **Wear-levelling** — assign the *least-damaged* available truck to the high-grade/overload route.
- **Deliberate cohorting** — the opposite: concentrate damage in a designated sub-fleet, keeping the rest young. Sometimes preferable when shop capacity is the constraint, because it clusters maintenance predictably rather than spreading it.

That third option is counterintuitive and is the sort of finding that justifies a study.

### 1.4 The reinforcing feedback loop

```
overload ──▶ damage rate ↑ ──▶ failures ↑ ──▶ trucks available ↓
                 ▲                                      │
                 │                                      ▼
                 └────── utilization per truck ↑ ◀── same tonnage target
```

Fewer available trucks means the survivors must work harder to hit the production target, which accelerates *their* damage, which reduces availability further. This is a **reinforcing (positive) feedback loop** — a system-dynamics structure, not a reliability block diagram. It is why the fleet can degrade faster than component-level arithmetic predicts, and it is the single strongest argument for simulating rather than calculating.

The loop is bounded by shop capacity (a queue) and by tire supply (a stock with lead time), both of which can become the binding constraint before component life does.

---

## 2. Model structure in WaSim primitives

### 2.1 Element inventory

**Per-truck state (fleet of N, typically 20–60):**

| Element | Type | Role |
|---|---|---|
| `damage_frame[i]` | `stock` | Cumulative Miner's-rule damage, structural. Fails at 1.0. |
| `damage_drive[i]` | `stock` | Cumulative drivetrain/bearing damage. |
| `tire_life_used[i]` | `stock` | TKPH-weighted tire consumption. |
| `damage_rate[i]` | `expression` | Power-law: `(payload[i]/rated)^exponent × cycles_per_step` |
| `truck_state[i]` | `failure_state_machine` | operating / failed, `condition` basis on damage ≥ threshold |
| `wear_state[i]` | `markov` | Optional multi-state degradation (good → worn → critical) |
| `assigned_route[i]` | `expression` | Dispatch decision — which route, hence payload and grade |
| `payload[i]` | `expression` | Policy output: 240 or 300 t, gated on grade and price |

**Shared / fleet-level:**

| Element | Type | Role |
|---|---|---|
| `shop_bays` | `resource` | Finite repair capacity; `spend`/`deposit` on repair start/finish |
| `repair_queue` | `queue` | Trucks awaiting a bay, with capacity blocking |
| `tire_inventory` | `stock` | Consumed on replacement, replenished with lead-time delay |
| `tire_on_order` | `stock` + `delay` | Purchase-order pipeline |
| `ore_grade` | `series` or `markov` | Grade of material at the active face — time-varying, uncertain |
| `metal_price` | `series` | Stochastic price path (GBM or historical bootstrap) |
| `overload_policy` | `expression` / controller | Boolean: overload iff grade > g\* AND price > p\* |
| `production` | `stock` | Cumulative tonnes / metal produced |
| `maintenance_cost` | `stock` | Cumulative cost accumulator |
| `crusher_capacity` | `expression` | Downstream cap — overloading is worthless if the mill is the bottleneck |

### 2.2 The core coupling (the part incumbents handle badly)

```
payload[i] ──▶ damage_rate[i] = (payload[i]/240)^β × cycles
                     │
                     ▼
              damage_frame[i]  (stock, integrates damage_rate)
                     │
                     ▼
        truck_state[i] hazard  ← failure_state_machine, `condition` basis
                     │
                     ▼
              available_trucks (sum over fleet)
                     │
                     ▼
        cycles_per_truck = target_tonnage / (available × payload)
                     │
                     └──────────▶ back into damage_rate[i]   ← THE LOOP
```

**This is the model's spine.** Failure hazard is a function of an accumulating stock, and that stock's accumulation rate depends on the system's own availability. Aggregate-continuous coupled bidirectionally to stochastic-discrete-state.

### 2.3 Scenario matrix

| Dimension | Values |
|---|---|
| Loading policy | nominal 240 / always-overload 300 / grade-conditional / grade+price-conditional |
| Dispatch policy | naive / wear-levelling / deliberate cohorting |
| Load exponent β | distribution (e.g. triangular 2.5–4.0, mode 3.0) — **sampled per realization** |
| Price path | stochastic, correlated across time |
| Grade sequence | from mine plan + geological uncertainty |
| Shop capacity | 2 / 3 / 4 bays (sensitivity) |
| Tire lead time | nominal / constrained supply |

### 2.4 Outputs that answer the question

- **Distribution** (P10/P50/P90) of: metal produced, NPV, fleet availability, maintenance cost, tire consumption
- **Policy frontier** — NPV vs. price threshold p\* and grade threshold g\*, identifying where the decision flips
- **Damage spread across fleet** at horizon — the wear-levelling diagnostic (variance of `damage_*` across trucks)
- **Criticality ranking** — which component class drives lost production
- **Tail risk** — P(catastrophic structural failure), P(fleet availability < X% for > Y days), P(tire stockout)
- **Value of information** — how much the NPV spread narrows if β were known precisely. *This is often the most valuable output: it tells the mine whether to fund instrumentation and a strain-gauge campaign rather than guess.*

---

## 3. Engine gap analysis — what WaSim supports today

This is the section that matters for build planning. Assessed against the capabilities document.

### 3.1 Already supported — no work needed ✅

**The damage-accumulation mechanism exists.** This was the thing I expected to be the blocker, and it is not:

- `failure_state_machine` already supports a **`condition` failure basis** — so "fail when cumulative damage ≥ threshold" is directly expressible today. No engine change.
- `markov` transition rates are already **expression-valued and explicitly state/time-dependent** — so degradation rates that depend on accumulated damage are supported today.
- `operating_time` and `demand` failure bases cover the conventional RAM cases alongside.

**Everything else structural is present:**

| Need | WaSim primitive | Status |
|---|---|---|
| Cumulative damage | `stock` integrating a rate | ✅ |
| Power-law damage rate | `expression` (pow) | ✅ |
| Shop capacity contention | `resource` + `priority_withdrawal` | ✅ |
| Repair queue | `queue` with capacity blocking | ✅ |
| Tire inventory + lead time | `stock` + `delay` | ✅ |
| Grade / price time series | `series`, `markov`, distributions | ✅ |
| Policy switching | `expression` / bang-bang controller with hysteresis | ✅ |
| Threshold optimization (g\*, p\*) | static optimization over editable `fixed` inputs | ✅ |
| Uncertainty in β | distribution sampled per realization | ✅ |
| Correlated inputs | Iman–Conover rank correlation | ✅ |
| Tail probability estimation | importance sampling + weighted reductions | ✅ |
| Value-of-information | nested Monte-Carlo via submodels + `submodel_stat` | ✅ |
| Scheduled PM | `failure_state_machine` scheduled-preventive policy | ✅ |
| Repair-as-good-as-new vs. replace | repair policies: none/repair/replace | ✅ |

**Assessment: the physics, economics, and reliability semantics of this model are all expressible in the current engine.**

### 3.2 The blocker — fleet instancing ⛔

**The array/dimension executor is provenance-only.** `vector_map`, `index_ref`, `index`, and `extern_call` are parsed and graph-connected, but the dimension-aware executor is not implemented and array-valued results return `0.0`.

This is the one hard blocker, and it is squarely in the way: **a 40-truck fleet with per-truck damage state is an array problem.** Without the array executor there are only bad options:

- **Manual replication** — copy-paste 40 sets of elements with `truck_01_damage_frame`, `truck_02_…`. Works, but the model becomes unreadable and unmaintainable, and changing fleet size means rebuilding. Viable for a *demo* at N=5, not for a deliverable at N=40.
- **Cohort approximation** — model 3–5 truck *cohorts* as aggregate stocks rather than individuals. Loses the damage-spread output entirely, which is the whole wear-levelling analysis. Defeats the purpose.
- **Submodel-per-truck** — nested submodels are for nested *Monte-Carlo*, not for parallel instances within one timeline. Wrong tool.

**Verdict: the array/dimension executor is a prerequisite for this vertical, not an optional enhancement.** It was already on the deferred list; this use case makes it the top engine priority. Notably it is *general* infrastructure — it unblocks any multi-unit RAM model (compressor trains, pump fleets, conveyor networks, wind farms), not just haul trucks. That makes it the highest-leverage engine investment for the RAM strategy overall.

### 3.3 Needs bolstering — moderate work 🔧

**(a) `capacity_demand` failure basis** — currently a documented no-op pending schema fields. This is the natural basis for "fails when demanded load exceeds capacity," which is *exactly* the overload semantic. Workable around today via the `condition` basis, but implementing it properly makes overload modeling idiomatic rather than a workaround.

**(b) Damage-indexed dispatch (the wear-levelling policy).** The policy "assign the least-damaged available truck" requires an **argmin over an array with a filter** — select the minimum-damage truck among those currently in the operating state. The array builtins list `min_array` but there is no `argmin`, and no filtered/masked reduction. Needed additions:
- `argmin_array` / `argmax_array` (return index, not value)
- masked reductions — reduce over elements satisfying a predicate (e.g. only available trucks)
- a stable tie-break rule (lowest index) so dispatch stays deterministic — **required for the bit-identical guarantee**

These are small additions to the array builtin set, but they must land *with* the array executor, since dispatch is the reason fleet instancing matters.

**(c) Multi-mode failure per unit.** A truck fails independently by frame, drivetrain, tires, engine, brakes — each with its own damage stock and threshold, any of which downs the truck. Expressible today as parallel `failure_state_machine`s OR'd through a `gate`, but the RAM idiom expects a single equipment item carrying *multiple failure modes* with per-mode criticality reporting. **Recommend a `failure_modes` grouping construct** — largely schema and reporting, minor engine work — because per-mode criticality ranking is a standard RAM deliverable.

**(d) Repair restoring partial life.** Current repair policies are none / repair / replace(as-good-as-new). Real overhauls restore *some* fraction of life — the standard "Kijima virtual age" / imperfect-repair models. Needed: a repair effect that sets `damage_stock ← damage_stock × (1 − restoration_fraction)`. This is arguably expressible today via an event applying a multiplicative effect to the damage stock on repair completion, which should be **verified before scheduling work** — it may already be free.

### 3.4 Out of scope — do not build 🚫

- **Per-cycle discrete-event haulage simulation** (individual truck trips through shovel queues at a loading face). That is discrete-entity/next-event ontology — the ceded territory. Model haulage as **aggregate cycle rates**, not as individual trips. Anyone needing true cycle-level dispatch simulation should be sent to a DES tool.
- **CMMS/database integration.** Distributions come in via a failure-mode import table (CSV/JSON), not a live connector. Keep the data half out.
- **Geological block modeling.** Grade sequence is an *input* (from the mine plan, with uncertainty), not something WaSim computes.

### 3.5 Build order

| Priority | Item | Effort | Unblocks |
|---|---|---|---|
| **1** | Array/dimension executor | Large | All multi-unit RAM. Hard prerequisite. |
| **2** | `argmin`/`argmax` + masked reductions | Small | Wear-levelling dispatch policies |
| **3** | `capacity_demand` failure basis | Small–moderate | Idiomatic overload/demand-vs-capacity |
| **4** | `failure_modes` grouping + per-mode criticality | Moderate | Standard RAM reporting |
| **5** | Imperfect repair (verify first — may exist) | Small | Realistic overhaul modeling |
| **6** | Failure-mode import format (CSV/JSON) | Small | Workflow gap with data half |

**Items 2–6 are all small-to-moderate. Item 1 is the real work, and it is general-purpose infrastructure that every multi-unit RAM model needs.**

---

### 3.6 Spike findings (supersede §3.2 and the §3.5 effort ratings)

A four-probe spike (`engine/tests/fleet_array_spike_v2.rs`) measured the array executor
against this model's actual needs. **§3.2's premise was wrong**: the array/dimension
executor is *not* provenance-only. The comprehension core (`vector_map`/`index_ref`/
`index`/reductions) is implemented and verified. The real gaps were two narrow boundaries,
both now closed by small changes:

| Probe | Question | Finding | Fix |
|---|---|---|---|
| 1 | Do per-member results reach the surface? | ⛔ was collapsed to member[0] | **Fixed** — array elements (primary output declaring `dimensions`) now expand into `<id>#1..#N` member result series, reusing the `#k` port convention. Damage-spread falls out of A3 per member. |
| 2 | Can a `stock` carry per-member state? | Array-fed stock takes member[0] only | Not needed — see Probe 3 |
| 3 | Does per-member state survive across steps via `expression` + `lag`? | ⛔ `lag` flattened the vector → then ✅ | **Fixed** — `lag` now preserves the `Value` shape (`[3,6,9]` verified). This makes the **stateless path viable**: per-truck accumulation via an array `expression` reading its own lagged array value. |
| 4 | `argmin`/masked reduction for dispatch? | ⛔ `argmin_array` absent | Still needed (small builtin additions) |

**Re-scoped build order:**

| Priority | Item | Effort | Status |
|---|---|---|---|
| ~~1~~ | ~~Array/dimension executor (build)~~ | ~~Large~~ | **Already existed + verified** |
| **1** | Array-member result expansion (`#k`) | Small | **Done in spike** |
| **2** | `lag` preserves vector shape (per-member state) | Trivial | **Done in spike** |
| **3** | `argmin_array`/`argmax_array` + masked reduction + lowest-index tie-break | Small | Needed for dispatch |
| **4** | Array-aware `time_history` displays / dashboard binding | Small–moderate | For per-member plots in the FE |
| **5** | `capacity_demand` failure basis | Small–moderate | Idiomatic overload (workaround exists) |
| **6** | `failure_modes` grouping + per-mode criticality | Moderate | RAM reporting |
| **7** | Imperfect (Kijima) repair | Small | Verify multiplicative-effect path first |

**Net:** fleet instancing dropped from "Large hard prerequisite" to "two small boundary
fixes (done) + one small dispatch-builtin addition." Array-valued stocks are **off** the
critical path — the `expression`+`lag` recurrence carries per-member accumulation.

---

## 4. A staged demonstration path

Because the array executor gates the full model, build the credibility story in stages that each stand alone:

**Stage 1 — Single truck, no fleet (buildable today).** One truck, damage stocks, power-law damage rate, `condition`-basis failure, repair, Monte-Carlo over β and price. Output: distribution of time-to-first-failure and NPV, nominal vs. overloaded. **This runs on the current engine with zero new features** and already demonstrates the core insight — that a 25% overload roughly halves component life, and that the P10 case looks very different from the P50. Good enough for a first conversation with a maintenance manager.

**Stage 2 — Small fleet by manual replication (N=5).** Ugly under the hood, but proves the feedback loop and shows damage spread across a handful of trucks. Sufficient for a demo video.

**Stage 3 — Full fleet after the array executor lands (N=40).** The real deliverable: dispatch policies, wear-levelling, damage-spread diagnostics, criticality ranking, policy frontier.

**Stage 4 — Generalize.** The same structure with different vocabulary is a compressor train, a conveyor network, a pump station, or a wind farm. The RAM domain library is largely this model with the nouns changed.

---

## 5. What to verify with practitioners before building

The engine analysis is grounded in the capabilities document. The *domain* analysis is inferred from engineering first principles and should be checked with the mine maintenance contacts before committing build effort:

1. **Is the overload decision currently made with any model at all** — spreadsheet, OEM vendor study, or professional judgment? (This sizes the pain.)
2. **Do they track per-truck cumulative payload / TKPH exposure?** Fleet-management systems weigh every load, so the data likely exists — but is it retained and usable? Damage accumulation is only credible if payload history is real.
3. **Is wear-levelling already practiced informally?** If dispatchers already rotate trucks by feel, the model formalizes existing intuition (easy sell). If not, it is a new idea (harder sell, bigger prize).
4. **What load exponent do they believe?** If OEMs supply derating curves, β is data. If nobody knows, the value-of-information output becomes the headline deliverable.
5. **Is the binding constraint trucks, shovels, crusher, or tire supply?** If the mill is the bottleneck, overloading buys nothing and the whole model reframes around a different question.
6. **Would they pay for this answer, and who signs?** Maintenance manager, mine GM, or corporate asset management — different buyers, different price points.

Question 5 is the one most likely to invalidate the framing, and it is a single conversation. Ask it first.
