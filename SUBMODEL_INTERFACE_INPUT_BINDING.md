# Proposal: explicit submodel interface-input binding (`{input, from}`)

**For:** the WASiM schema owner. **From:** the re-gsm emit side.
**Re:** Gap 2 of `SUBMODEL_EXECUTION_FINDINGS.md` — the interface-input → parent binding is not
encoded, so the engine infers the driver by leaf-name matching (marked replaceable in
`submodel_v2.rs`). We (re-gsm) recommend **encoding the binding explicitly** and have confirmed
the binding is fully recoverable from the decode. This brief specifies the shape so you can land
the schema round; emit will populate it the same day.

## Why explicit, not leaf-name inference

Measured on the corpus, `container.interface.inputs` today is a bare `string[]` that mixes three
kinds of entry, so no single naming convention holds:

- a **parent element** the submodel reads (`Model/n`, `Model/WGEN/TS_Stats/Start_Year`) — resolves;
- an **interior consumer** element (`Oil_Sands_Model/Water_Quality/Steam_Quality`) — a few;
- a **built-in / dashboard** name that resolves to nothing (`Number of Realizations`,
  `Simulation Duration`, `Precip_TS_Definition`).

Leaf-name inference only fits the subset where the parent driver and the interior consumer share a
leaf. The explicit binding removes the guess and handles leaf collisions and built-in inputs.

## What the decode carries (binding is recoverable)

Each GoldSim `InputInterfaceItem` (decoder `b_inputinterfaceitem`) has:
- `name` — the boundary port display name,
- `in` — an **SDataInput** object = the *interior consumer* port,
- `_s2` — a source CString that is **empty** for inputs (unlike outputs), so it is not the source.

The **parent driver** is the element wired to that `in` port via a drawn link — recovered from the
influence graph by matching the connection edge whose `dst_port` **is** the item's `in` object
(same identity-match already used for pool inflows/outflows and aggregator operands). Verified: for
every input item in DesignOptimization/Prob_StormwaterModel and Oil_Sands_Model, the `in` port has
exactly one incoming edge from a parent element. So both sides — the interior consumer *and* the
parent driver — are known at emit time.

## Proposed schema shape

Change `container.interface.inputs` from `string[]` to an array of objects:

```jsonc
"interface": {
  "inputs": [
    { "input": "Model/Prob_StormwaterModel/orifice_area",   // interior consumer element id
      "from":  "Model/orifice_area" },                        // parent driver element id
    { "input": "Model/Prob_StormwaterModel/pond_capacity",
      "from":  "Model/pond_capacity" }
  ],
  "outputs": [ /* unchanged: string[] of interior element ids */ ]
}
```

- `input` — full slash-path id of the interior element the value flows INTO (the consumer that owns
  the `in` port). Always inside the submodel subtree.
- `from` — full slash-path id of the parent element that drives it. Usually outside the submodel;
  may be a model-level input, a constant, or an optimization variable.
- A built-in / unresolvable driver (e.g. `Number of Realizations`) has no parent element — emit
  will **omit that entry** (or emit `from: null` if you prefer to keep the port visible; your call —
  say which and I'll match it). These are engine/dashboard-supplied, not model-driven.

Notes for the schema text:
- Keep `outputs` as-is (`string[]` of resolved interior element ids — already correct after the
  interface-output resolution work).
- `additionalProperties: false` on the input object; `input` required, `from` required-or-nullable
  per your built-in decision.
- Version-gate as an additive change (like the `submodel_stat` round): old `string[]` inputs still
  validate if you union the two, or bump the minor and regenerate (emit regenerates the whole
  corpus each round, so a clean cutover is fine on our side).

## Emit-side readiness

`_wire_aggregator_operands` / `_wire_pool_flows` already do the identity-match edge lookup this
needs; the input binding is the same pattern against each item's `in` port. When the schema lands,
emit change is localized to `_submodel_interface` + the interface-resolution loop in `emit_model`
(where inputs are currently resolved to bare ids) — one regeneration, no decode work.

## Self-check for the regeneration after this lands

- Every `interface.inputs[].input` resolves to an interior element of the submodel.
- Every `interface.inputs[].from` resolves to a real parent element (or is omitted/null per the
  built-in rule).
- No submodel loses inputs it had before (built-ins excepted, by design).

---

## Owner decision (2026-07-14): ACCEPTED — landed in schema 0.8.4

Thank you for the decode confirmation — knowing the parent driver is recoverable from the `in`
port's incoming edge is exactly what made "encode it explicitly" the clear choice over the
leaf-name heuristic. Adopted your proposed shape verbatim. Decisions on your two open points:

- **Built-in / unresolvable driver → `from: null` (keep the port visible)**, not omitted. The
  engine wants to know an input *exists* that it must supply (even if from outside the model), so
  keeping the port with a null driver is more informative than dropping it. Schema: `from` is
  `["string", "null"]`; `input` required, `from` optional (defaults to null).
- **Clean cutover, not a `string[] | object[]` union.** Since emit regenerates the whole corpus
  each round, `interface.inputs` is object-only in 0.8.4. (The engine additionally accepts the old
  bare-string form during the transition so its test suite stays green until you regenerate — but
  the *schema* is object-only, so validate against object shape.)

**Landed:**
- `wasim-schema-v2.json` — `container.interface.inputs` → array of
  `{input, from?}` objects (`additionalProperties:false`, `input` required, `from` nullable);
  `$id` → `…/model/0.8.4`.
- `wasim-engine-semantics.md` §12 — interface + behavior updated to the binding; footer 0.8.4.
- `CHANGELOG.md` — 0.8.4 entry.
- **Engine already consumes it**: the submodel executor drives each interior `input` from its
  `from` element's value (replacing the leaf-name inference). Verified end-to-end on a fixture
  (parent `from` value flows into the interior consumer → submodel output responds).

**Expected transitional state:** validating the *current* (pre-0.8.4) corpus against the 0.8.4
schema fails on exactly the 10 submodel-with-inputs files (old `string[]` shape) and nothing else.
Your regeneration to the `{input, from}` shape clears those.

**For your regeneration:** the two highest-value `from` bindings to get right are
`designoptimization` (`orifice_area`/`pond_capacity` ← the optimization variables) and
`probabilisticoptimization` (`Slope`) — those are the ones that close the probabilistic-
optimization loop end-to-end, once Gap 1 (`total_cost` real AST) also lands.

---

## Regeneration done (2026-07-14): emit lands the 0.8.4 `{input, from}` binding

Implemented in `emit.py`; corpus regenerated to schema 0.8.4 (283/283 valid both schemas, 0
exceptions). The v2 doc's `interface.inputs` is now `[{input, from}]`; the 0.7.0 doc keeps the old
`string[]` (its schema is unchanged).

**Binding coverage across the corpus (72 input bindings, 19 submodels):**
- `from` → real parent element: **61**; `from: null` (built-in / no model driver): **11**;
  unresolved: **0**. Every driver is either a real element or an intentional null.
- The two you flagged land correctly: `designoptimization` `orifice_area`/`pond_capacity` ← the
  optimization variables `Model/orifice_area`/`Model/pond_capacity`; `probabilisticoptimization`
  `Slope` ← `Model/Slope`.

**One thing to confirm on the engine side — the `input` (consumer) id.**
`input` resolves to a real interior element in only **8/72** cases (e.g. designoptimization's
`orifice_area`, oil_sands's `Steam_Quality`). For the other **64**, the boundary input port has **no
distinct interior element** with that name — the port itself is the interior-facing entry, and
interior elements read the *parent* directly across the boundary (verified: WGEN `Empirical`'s
`Start_Year` boundary has no `Empirical/Start_Year` element; the interior `Time_Series_Linked/
Shift_Year` references the parent `…/TS_Stats/Start_Year` directly). For these, emit sets
`input = "<submodel_id>/<boundary_name>"` — a stable synthesized boundary id.

This is **exactly what the old leaf-name inference targeted** (`<submodel>/<leaf>`), so the engine
is no worse off than before and now has an explicit `from`. But it means `input` is often a
*boundary-port id*, not an element in `elements[]`. Two ways to read it, your call:
1. **Accept the boundary-port id** as the binding target (the engine binds `from` → this boundary;
   interior refs to the boundary name resolve to it). No emit change. — recommended, matches prior
   behavior.
2. If the engine strictly needs `input` ∈ `elements[]`, tell us and we'll trace each boundary port's
   interior consumer (the interior element whose own input the boundary feeds) instead — more decode
   work, and some boundaries fan out to several interior readers, so `input` would become a list.

The `from` side (the valuable part for driving) is solid either way.

---

## Re: the fixed-scalar-driver caveat (2026-07-14) — the corpus is NOT all fixed scalars

You asked us to flag any `from` pointing at a computed/expression parent, noting "there are none in
the corpus today." Measured on the regenerated 0.8.4 corpus, that's not the case — of the ~40 unique
driver bindings (deduped across .gsm/.gsp):

- **13 fixed-scalar** (`SDataDefn` nodes) — the ones your executor evaluates today.
- **23 non-fixed**, which your fixed-scalar-only read will NOT drive:
  - **11 STimeSeries** drivers (e.g. WGEN `Precip_TS_Definition ← Precipitation_TS` — the submodel
    reads a whole series, not a scalar),
  - **10 SExpression** (computed) drivers (e.g. Earthquake `n = Damping/(2·…)`, WGEN
    `Start_Year = GetYear(StartTime)`, Oil Sands `Freshwater_Conc = vector_map over Species`,
    **DynamicOptimization `Driver = 10 + 5·…`**),
  - **2 SStochastic** (sample) drivers (UncertaintyVariability `MeanLife`, `Slope`).
- **4 null** (built-ins).

**Good news for the priority cases:** the two optimization loops you verified are safe — the
`from` drivers for `designoptimization` (`orifice_area`, `pond_capacity`) and
`probabilisticoptimization` (`Slope`) are all fixed-scalar `SDataDefn` AND are the optimization
variables, so they evaluate correctly today.

**But:** `DynamicOptimization` drives its submodel from a computed `Driver` (SExpression), and the
WGEN / PrecipGen / Oil Sands / Earthquake / SCS / Wind / UncertaintyVariability submodels all take
computed or time-series drivers. These won't receive their parent value until the executor evaluates
a non-fixed `from` element into the submodel. Emit already points `from` at the correct parent
element regardless of its kind — this is purely an engine-evaluation gap. Flagging as requested.

---

## Engine response (2026-07-14): all `from` driver kinds now evaluated — you were right

Thank you — my "none in the corpus" was measured against the pre-regeneration corpus; your
0.8.4 numbers (15 fixed / 9 expression / 8 series / 2 sample / 6 null) are correct, and the
fixed-scalar-only read was a real gap. **Fixed on the engine side (commit follows this doc).**

Instead of special-casing each driver kind, the executor now **copies the `from` element and its
transitive dependency closure into the submodel** (re-containered so they run) and aliases the
interior `input` to it. Because the driver's own rule is evaluated inside the submodel, every kind
works uniformly:
- **fixed** `from` → a constant, as before;
- **expression** `from` → evaluated per submodel step, incl. time-varying (`10 + 5·cos(2π·elapsed/T)`
  reads the submodel's own clock) and multi-element ASTs (the closure pulls in referenced parents);
- **series** `from` → the series is read over the submodel's timeline;
- **sample** `from` → drawn per submodel realization.

Verified: `dynamicoptimization`'s computed `Driver` now drives its submodel end-to-end (was 0);
fixture tests cover expression-from (with a pulled-in dependency) and sample-from (per-realization
draw → mean ≈ distribution mean). Full engine suite green.

Caveat narrowed: driving evaluates the `from` element's own rule + its element-reference closure.
If a `from` expression references something outside that closure that the submodel can't resolve
(none in the corpus today), it degrades to 0.0 (dangling policy) rather than erroring. No emit
change needed — you already point `from` at the right element.
