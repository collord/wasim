# Workplan: Fleet-Instancing Support for the Haul-Fleet Model

*Turns HAUL_FLEET_MODEL_SPEC.md into a buildable target. Grounded in the four-probe spike
(`engine/tests/fleet_array_spike_v2.rs`) and a read of the actual engine, not the stale
§15 semantics claim.*

## 0. What the spike settled

The spec's §3.2 called fleet instancing a **"Large" hard blocker** needing an array/dimension
executor build. **That was wrong.** Measured reality:

| Capability | Believed (spec §3.2) | Actual |
|---|---|---|
| `vector_map`/`index_ref`/`index`/reductions | provenance-only, → 0.0 | **implemented + verified** (`v2_parse::array_comprehension_evaluates`) |
| Array element → results surface | (not considered) | was collapsed to member[0]; **fixed in spike** (`<id>#k` expansion) |
| Per-member state across steps (`expression`+`lag`) | (not considered) | `lag` flattened vectors; **fixed in spike** (one line) — now `[3,6,9]` |
| Array-valued **stocks** | required, substantial | **not needed** — the `expression`+`lag` recurrence carries accumulation |
| `argmin`/masked reduction (dispatch) | needed, small | confirmed absent — still needed |
| Array-valued **stateful nodes** (FSM/status/pid/milestone) | (not considered) | **scalar-state only** — the real remaining gap |

Two fixes already landed in the spike branch (results expansion + `lag` shape preservation).
The remaining engine work is smaller and more sharply targeted than the spec assumed.

## 1. The one architectural decision

Per-truck **failure** needs per-truck *state* (each truck independently working/failed, with
its own time-to-repair clock). The `failure_state_machine`, `status`, `pid`, and `milestone`
node rules are all **scalar-state-per-element**: they key their state (`fsm_state`,
`status_state`, …) by a single element id and evaluate a single `trigger_fires` bool. They
cannot today be array-valued.

So there are two ways to give 40 trucks independent failure state:

- **Option A — manual per-truck FSM elements.** 40 `failure_state_machine` elements, each
  reading `damage#i`. This is exactly the "manual replication" the spec rejected for the
  damage stocks — but note it is now confined to *only the stateful failure nodes*, because
  accumulation is handled by single array elements. Ugly but zero new engine work.

- **Option B — array-valued stateful nodes.** Make the state carriers hold `Vec` state and
  evaluate `trigger_fires` per member. This is the general fix; it makes the fleet a handful
  of array elements end-to-end. More engine work, but it is the reusable infrastructure the
  whole RAM vertical needs (§3.2's original instinct, relocated to the correct primitive).

**Recommendation: Option B, scoped to `status` first.** Per-truck failure in this model does
not need the full FSM — it needs "latch failed when `damage[i] ≥ 1`, latch working when a
repair completes." That is exactly the `status` node (trigger-set/trigger-reset latch).
Make **`status` array-valued** and per-truck failure becomes one array element. The full FSM
(time-to-failure/repair distributions, replace-vs-repair) can stay scalar/manual until a
model genuinely needs distributional repair per unit. This keeps the critical path small
while buying the general capability where it is cheapest.

## 2. Engine changes

### 2.1 Done in the spike (verify, then keep)
1. **Array-member result expansion.** An element whose primary `output` declares
   `dimensions` expands into `<id>#1..#N` result series (label `Name[k]`, unit inherited).
   Recorded in both the normal and interrupt recording passes; assembled into `ElementResults`.
   *(engine_v2.rs — `array_members` precompute + two recording sites + assembly loop.)*
2. **`lag` preserves `Value` shape.** `NodeRule::Lag` returns the full previous-step `Value`
   (was `as_scalar()`), so an array input lags per-member. Scalar inputs unchanged.

### 2.2 New — required for the fleet model
3. **`argmin_array` / `argmax_array` builtins.** Return the 1-based index of the min/max
   member. **Lowest-index tie-break** (mandatory for the bit-identity guarantee). Add to the
   `BuiltinFn` enum, the dispatch in `eval.rs`, and the array-consuming arg handling.
4. **Masked reduction.** Dispatch is "least-damaged *available* truck" — an argmin over a
   filtered subset. Two viable shapes, pick one:
   - a masked builtin `argmin_masked(values, mask)` — reduce only where `mask[i] > 0`; or
   - compose from existing nodes: `damage + (1 − available) × BIG` then `argmin_array`
     (a penalty makes unavailable trucks lose). **Prefer the composition** — zero new
     builtins beyond argmin, and it is expressible in the model today. Document the idiom.
5. **Array-valued `status` node.** When `status`'s input/triggers reference array-valued
   elements, hold `Vec<bool>` state and evaluate the set/reset condition per member. This
   requires `trigger_fires` (or a per-member variant) to evaluate a comparison against member
   `k`. Scope: the trigger's `condition` AST is evaluated in a context where `index_ref`
   is bound — i.e. wrap the per-member evaluation the same way `vector_map` does. State map
   becomes `HashMap<String, Vec<bool>>` for array status elements.

### 2.3 Optional — nicer, not blocking
6. **Array-aware `time_history_displays` / dashboard binding.** So the FE can plot the N
   member series (damage spread) without hand-listing `damage#1..#40`. Small results-layer +
   FE work; the data already exists post-2.1.
7. **`capacity_demand` failure basis** (spec §3.3a). The natural "demand exceeds capacity"
   semantic. Workaround: a `status`/`condition` comparing a demand array to a capacity array.
   Only implement if a model needs the idiom.
8. **Imperfect (Kijima) repair.** A repair event applying a multiplicative effect
   (`damage ← damage × (1 − restoration)`) to the damage element. **Verify the existing
   multiplicative-effect path already does this before scheduling work** — it may be free.

## 3. Schema changes

The schema is largely ready — `dimensions[]`, `output_spec.dimensions`, and the array AST
nodes all exist. Minimal additions:

1. **`argmin_array`/`argmax_array`** added to the builtin-`fn` enum in the schema (the parser
   validates `fn` against this enum — the spike's Probe 4 failed precisely at this
   validation). Mirror `min_array`/`max_array`.
2. **No new element types.** Per-truck state is expressed with existing primitives made
   array-aware (`status`), not new schema.
3. **Documentation, not schema:** the "penalty-masked argmin" dispatch idiom and the
   "`expression` + `lag` accumulator" per-member-state idiom belong in
   `wasim-engine-semantics.md` §15, which must also be corrected — it currently claims the
   array executor degrades to 0.0, which is false and cost this spec its central conclusion.

### Schema/doc reconciliation (do this regardless)
`wasim-engine-semantics.md` §15 says `vector_map`/`index_ref`/`extern_call` evaluate to 0.0
(placeholder). The code implements them. Update §15 to describe the real executor, the
`<id>#k` member-result expansion, the vector-preserving `lag`, and the array-aware `status`.

## 4. Build order (revised, with effort)

| # | Item | Effort | Blocking? | Status |
|---|---|---|---|---|
| 1 | Array-member result expansion (`#k`) | S | yes (the deliverable output) | **DONE** |
| 2 | `lag` preserves vector shape | XS | yes (per-member accumulation) | **DONE** |
| 3 | `argmin_array`/`argmax_array` + schema enum + tie-break | S | yes (dispatch) | **DONE** |
| 4 | Penalty-masked argmin idiom (doc + test) | XS | yes (wear-levelling) | **DONE** (in scenario + §15) |
| 5 | Array-valued `status` (per-truck failure latch) | M | yes (per-truck failure) | **DONE** (Option B) |
| 6 | §15 doc reconciliation | XS | no (but prevents re-mis-scoping) | **DONE** |
| 7 | Array-aware TH displays / FE binding | S–M | no (usability) | todo |
| 8 | `capacity_demand` basis | S–M | no (workaround exists) | todo |
| 9 | Imperfect repair | XS–S | no (verify-first) | todo |

**Critical path to a runnable N=40 fleet model: items 1–6 — all DONE.** The committed scenario
`parameters_examples/haul_fleet_overload.json` parses and runs as-is (no workarounds): array
accumulation via `expression`+`lag`, per-truck failure via array `status`, and wear-levelling
dispatch via `argmin_array` that **rotates the overload target across all trucks** (verified in
`fleet_array_spike_v2::probe6`). Remaining items 7–9 are usability/refinement, not blockers.

**Option A vs B (§1): chose B.** Per-truck failure is a real array-valued `status` node
(per-member `Vec<bool>` state, per-member trigger evaluation), not N replicated FSM elements.
The fleet is now a handful of array elements end-to-end — the reusable infrastructure the RAM
vertical needs. Extending Option B to the other stateful rules (`pid`/`fsm`/…) is the same
pattern when a model demands it.

## 5. Staged delivery (revised from §4 of the spec)

- **Stage 1 — single truck, today.** Scalar model, one FSM, Monte-Carlo over β and price.
  Runs on the current engine; already demonstrates the core "25% overload ≈ halves life" and
  P10-vs-P50 insight. *(No new features.)*
- **Stage 2 — small array fleet (N=5), after items 3–5.** Real array instancing, not
  manual replication: `payload`, `damage`, `failed` as array elements; damage spread visible
  via `#k` series. Proves the feedback loop and the wear-levelling diagnostic.
- **Stage 3 — full fleet (N=40).** Same model, larger dimension. Dispatch policies, policy
  frontier, criticality, tail risk.
- **Stage 4 — generalize.** Compressor trains, pump fleets, conveyor networks — the same
  array structure with the nouns changed.

The scenario JSON (`parameters_examples/haul_fleet_overload.json`) targets **Stage 2/3
shape** — the real array-instanced model, using the penalty-masked-argmin dispatch idiom and
a manual array-`status` failure latch, flagging inline where item 3/5 must land.

**Validated** (`fleet_array_spike_v2::probe5`): the full model parses except the single
`argmin_array` call (item 3, as expected); a runnable variant (argmin swapped for a fixed
dispatch target) executes end-to-end on today's post-spike engine and reproduces the core
result — the overloaded truck accrues ~1.95× the damage of nominal trucks (the `(300/240)^3`
power-law penalty, i.e. the spec's "25% overload ≈ halves life"), and `damage_spread` > 0
shows the wear concentration the wear-levelling policy exists to correct. Per-truck series
`damage#1..#5` are the damage-spread output, produced by the item-1 result expansion.

Two modeling notes captured while validating (both in the JSON descriptions): (1) damage is
scaled per-STEP (no `dt` multiply) — nominal life ≈ 1.0 over ~156 weeks; (2) `metal_price` is
a per-realization `sample` (cross-realization price uncertainty, which is what the policy
frontier needs) rather than an intra-run GBM path — a within-run stochastic price path is a
documented refinement, not core to the fleet mechanics.
