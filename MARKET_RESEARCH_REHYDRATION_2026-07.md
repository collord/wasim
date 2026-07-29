# Market-Research Rehydration — Source Verification & Repositioning

**Date:** 2026-07-28 · **Branch:** `claude/market-research-chat-ha1zoo`
**Predecessor:** `67265379-REHYDRATION_PROMPT.md` (reasoned with NO source access; several
engine claims tagged ⚠️ VERIFY). This document is the promised verification-against-source
pass plus the strategic consequences of what the source actually shows.

Tags: ✅ GROUNDED (verified in source) · ⚠️ CAVEAT · 💡 REASONING.

---

## 0. Headline (the thing the prior conversation could not see)

Since the Jul-25 snapshot the engine did **not** advance toward the RAM beachhead the
rehydration prompt recommended. It advanced, heavily and with passing tests, in two other
directions:

1. **A quantitative-finance / derivatives-pricing suite** (branch `monte-carlo-handbook-memory`):
   Longstaff–Schwartz LSM American/Bermudan options + dual upper bounds, Asian, barrier,
   basket, digital, lookback, correlated-asset spread options (Cholesky/Margrabe), **control
   variates**, antithetics, digital Greeks (likelihood-ratio vs bump-and-revalue),
   expression-valued drift/vol, nested VaR/CVA exposure profiles.
2. **An Analytica→WASiM translation track** (branch `analytica-wasim-conversion`): the
   **dimensioned array lane landed bit-identically** (EVIU + Platform_2017 models run
   end-to-end, 0 mismatches on 234k/176k values), plus `submodel_stat2` bivariate reduction.

**The demonstrable-model corpus reflects this.** Of 16 shipped example models in
`schema_examples_manual/`, **13 are quantitative finance** (options, exposure profiles,
nested VaR, correlated assets). The non-finance ones are `retirement_planning.json` and
`two_tank_hydraulic.json`. **There is no shipped RAM/reliability example model at all** —
the failure-basis and Markov machinery exists only in unit tests.

So the strategic map the rehydration prompt carried (RAM = the beachhead; finance = not
discussed) is now **inverted by the actual investment**. That is the central finding to
resolve with the human, and §5 lays out the fork.

---

## 1. §7 verification checklist — results against real source

| # | Question (rehydration §7) | Verdict | Evidence |
|---|---|---|---|
| 1 | Outer loop realization-major or step-major? | ✅ **REALIZATION-MAJOR** | `engine_v2.rs:1444` realization loop outermost; step loop nested at `:1721`, closes `:3173`. Module invariant `:838` "realization k's streams are a pure function of (seed, k)". |
| 2 | Array/dimension executor still provenance-only (returns 0.0)? | ✅ **STALE — LANDED** | `Value::Array`/`NamedArray` fully evaluated in `eval.rs` (`VectorMap :790`, `Subscript :837`); two-tier "array lane" (`array_lane.rs`): flat MC bytecode kernel + dimensioned lane `run_dim_lane :798`. `fleet_array_spike_v2.rs` asserts real members (10/20/30, not 0.0). |
| 3 | `submodel_stat` + nested MC — exists? which reducers? | ✅ **WORKS** | Pre-pass `run_submodels` (`engine_v2.rs:1018` → `submodel_v2.rs:378` recursive engine call); reduce on demand `eval.rs:722`. **9 univariate** reducers: `mean, percentile, sd, cumulative_prob, exceedance, cte, sum, min, max`. **3 bivariate** (`submodel_stat2`): `cov, corr, beta`. `nested_stat` is a true conditional double-loop (`nested_var_smoke.rs`, per-path corr>0.995 vs closed form). |
| 4 | `event_accurate` — closed-form crossings, consumes no randomness? | ✅ **CONFIRMED** | `TimebaseMode::EventAccurate` (`engine.rs:64`); closed-form crossing `dt_cross=(bound−level)/rate` (`timebase.rs:86`); RNG-consuming rules gated behind `is_last` (`engine_v2.rs:1946`), crossing re-run snapshots stock state only, **never the RNG**. Tests `rng_stable_across_timebase`, `rng_stable_across_bound_crossing`. |
| 5 | Failure bases `condition`/`capacity_demand` status? | ✅ **condition LIVE; capacity_demand NO-OP** | Dispatch `engine_v2.rs:2314-2343`. `Condition` → `trigger_fires`/`eval_qof_value` sees stock levels → damage-stock ≥ threshold works mechanically. `CapacityDemand => false` hard-coded (`:2340`), documented no-op (`failure_bases_v2.rs:4`). `operating_time` LIVE but == exposure-time; `demand` LIVE. |
| 6 | Markov rates expression-valued/state-dependent? | ✅ **EXPRESSION-VALUED, STATE-DEPENDENT** | `TransitionRow::Expr` (`model_v2.rs:294`); evaluated every step against current ctx incl. stocks (`engine_v2.rs:2079-2103`) → rising/damage-dependent hazard on the `markov` node. |
| 7 | Seeding: splittable pure-function of (seedRoot,k)? | ✅ **PURE-FUNCTION — bit-identical under fan-out holds** | `ChaCha8Rng::seed_from_u64(seed); rng.set_stream(real_idx)` (`engine_v2.rs:1446`). Counter-based stream selector, not sequential. Sweep seeds content-derived (FNV-1a id hash, `sweep_seed.rs`), reorder-invariant. |
| 8 | `wasim-engine-semantics.md` v0.9.6 — what changed? | ✅ **DOC GONE; versioning reset** | No `*semantics*` file exists in-repo. Format is now **schema v0.1.0** (`README.md`, `model.schema.json`). The 0.9.x engine-semantics versioning the Analytica doc quoted is retired. |
| 9 | `argmin_array`/`argmax_array` + masked reductions? | ✅ **argmin/argmax present, STABLE tie-break; masked = idiom only** | `eval.rs:1257-1279`, lowest-index-wins strict `<`/`>` (determinism). No dedicated masked reducer — penalty idiom `argmin(damage + BIG·failed)` (`fleet_array_spike_v2.rs:155`). |
| 10 | Dimension size static or runtime-computed? | ✅ **STATIC-ONLY** | `DimensionDef.size: usize` fixed at parse (`model.rs:288`). Runtime-dynamic index length/membership is an explicit non-goal (`WASIM_NAMEDARRAY_DESIGN.md:32`). You can pick *which* member at runtime, not *how many*. |

**Net:** every ⚠️ load-bearing claim in the rehydration prompt verified **TRUE or better**.
Nothing the strategy leaned on turned out to be vapor. The two "must-build, CI-must-assert"
items (reorder-invariant sweep seeding; bit-identical determinism) are **already built and
already asserted in CI** (`sweep_seed_v2.rs`, `timebase_bit_identity.rs`).

### Caveats worth carrying (capability ≠ test coverage)
- ⚠️ `condition`-basis failure **can** read a damage stock, but the shipped failure-basis
  test uses a clock (`elapsed>=2`), not a stock. The stock-in-condition eval path is covered
  elsewhere (`discrete_nodes_v2.rs`); a dedicated *stock-threshold failure* test is absent.
- ⚠️ Markov `Expr` transitions are wired end-to-end but the only Markov test uses a Fixed
  matrix. Dynamic-hazard RAM would be building on an untested (though present) path.
- ⚠️ `operating_time` is not truly operating-hours-gated (decremented by `dt` every step
  unconditionally) — same as exposure-time today.
- ⚠️ Nested submodel recursion is depth-general by construction but only depth-1 is tested,
  and there is no run-recursion cycle guard (only the container parent chain is guarded).

---

## 2. Sweep composition is past "embryo"

The rehydration prompt's §3.3/§5 hoped `submodel_stat` was "sweep composition in embryo."
Reality is stronger: the **marginal** case (one nested MC sub-loop, reduced by any of 9
reducers, fed to the parent as a constant) and the **conditional double-loop** case
(`nested_stat`: inner submodel re-run per outer realization, bound to that path's state)
**both ship and pass tests**. Content-derived `sweepId` seeding with reorder-invariance and
composition-isolation CI is in place. What remains from `WORKPLAN_SWEEP_COMPOSITION.md` is
mostly the *vector* reductions (gated on the labeled n-d value type, §3) and SIPmath-style
whole-distribution bindings — not the scalar core, which exists.

💡 Consequence: the "should WASiM build mid-graph reductions or sweep composition?" debate is
settled by the realization-major loop (item 1) **and** by the fact that sweep composition is
already the shipped mechanism. Mid-graph across-realization reduction would fight the
architecture; sweep composition rides it.

---

## 3. The one real engine bottleneck, named honestly

`ANALYTICA_TRANSLATION_STATUS.md` §9.3 converges three independent gaps — label subscript,
multi-dimensional broadcast, and mid-graph *vector* sample reductions — onto **one** engine
change: a **labeled n-d value type** (the "NamedArray" design). The *dimensioned array lane*
that landed is the correctness-first slice; the labeled n-d type is the general form. This is
the same "#1 leverage" conclusion the rehydration prompt reached (§5), now sharpened: it's not
"an array executor" (that shipped) — it's **labels on the axes** plus **axis-selective
reductions** (current reducers collapse *all* axes to scalar, `eval.rs:1234`) plus the
still-static dimension size (item 10). Those three are the honest frontier.

> **Update (2026-07-29):** the axis-selective-reduction gap is **closed** — the array reducers
> now take an optional 1-based axis-position argument and collapse one axis, keeping the rest
> (`STAGE3F_AXIS_SELECTIVE_REDUCTION.md`, proven by `axis_reduction_v2.rs`/`nd_recurrence_v2.rs`).
> Of the three frontier items, only the **static dimension size** remains.

---

## 4. Open Thread #1 (geospatial transfer-function) — now answerable

The question the chat ended on: *does Stage-2 need spatial fields as first-class cell arrays
(⇒ needs the array executor), or can it consume geostatistical realizations one at a time
through the sweep boundary?*

**Answer: both paths now exist, and the sweep-boundary path is the unconstrained one — take it.**

- The **one-at-a-time sweep path** (feed each geostatistical realization through the
  `nested_stat`/submodel boundary, run the deterministic Stage-2 model, reduce the response)
  is *already shipped* and rides realization-major execution + content-derived seeding. It
  never needs WASiM to hold a full field as an array. **No new engine work required.**
- The **cell-array path** (fields as first-class dimensioned arrays) is *also* now possible
  via the dimensioned lane — **but** it inherits the static-dimension-size constraint (item
  10): grid cardinality is fixed at parse time, not runtime. Fine for a fixed grid, blocking
  for runtime-varying field size.

💡 Recommendation: the geospatial play **rides on already-shipped work** (sweep boundary), so
it does *not* gate on new engine investment — consistent with "WASiM = Stage-2 transfer
function + composition; don't rebuild Stage-1 geostatistics (GSLIB/GeostatsPy own it)."

---

## 5. The fork to put to the human

The verification removes the technical uncertainty. What's left is a **positioning decision**
the source has quietly pre-loaded toward finance:

**Option A — Quantitative finance / model risk (what's actually built).**
13 polished demonstrable models; a genuine efficiency story (variance reduction + control
variates + antithetics + standard-error/CI + live Black-Scholes benchmark via `erf`); LSM
American + dual bounds; nested VaR/CVA/exposure profiles. Buyer is regulated and well-funded
(SR 11-7 model-risk management: reproducibility + open diffable format + "why do two desks
price differently" is a sharp pitch). *Cost:* crowded, sophisticated incumbents; requires
finance-domain validation the founder doesn't obviously have.

**Option B — RAM beachhead (rehydration prompt's recommendation).**
Empty commercial floor (Windows-desktop/quote-gated incumbents; RAPTOR a fossil), same
AC+SDS×MC coordinate, founder has adjacent contacts at surface mines. *Cost:* **zero shipped
demonstrable models** and the two most RAM-specific paths (stock-threshold failure, dynamic
Markov hazard) are *present but untested*. Everything in `HAUL_FLEET_MODEL_SPEC.md` is still
buildable on today's engine (per §1 items 5–6) but nobody has built the Stage-1 demo yet.

**Option C — Neither is the identity; the coordinate is (openness is the strategy).**
Keep the engine domain-neutral, ship finance *and* RAM as paid libraries on one platform.
The finance suite already proves the domain-neutral MC-efficiency core; RAM would be a second
library, environmental a third.

💡 My read: the finance work wasn't a detour — it built and *proved* the domain-neutral
efficiency machinery (variance reduction, control variates, nested MC) that strengthens
**every** vertical. But it also produced the only polished demo corpus that exists. The
cheapest high-signal next move is not to pick A-vs-B in the abstract but to **build the
Stage-1 RAM demo** (single truck, damage stock, `condition` failure, MC over load-exponent +
price) — it's a few days on today's verified engine, it closes the untested-path caveats, and
it's the artifact that lets the founder's mine contacts answer the three validation questions
in rehydration §8.4. If it lands and resonates, B/C; if it stalls, the finance corpus is the
fallback that's already real.

---

## 6. Not-goals reaffirmed (unchanged by source)
- Don't build `.ana`/ModelJSON importers as homes (interchange = bridge). Assisted Analytica
  pipeline only (mechanistic skeleton + flagged stubs), as `ANALYTICA_TRANSLATION_STATUS.md`
  concludes.
- Don't rebuild Stage-1 geostatistics.
- Don't chase Analytica's static-model market (different ontology/buyer).
- Don't vendor/port RAPTOR (license contamination).
