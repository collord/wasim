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

### On your `input`-resolution question (2026-07-14): (a) — accept the boundary-port id as-is

Confirmed: keep the synthesized `<submodel>/<name>` id; **do NOT trace the interior consumer(s)**
(option a, not b). No behavior is lost. Measured on the regenerated corpus, `input` resolves to a
real element in 5/40 bindings; for the other 35 the synthesized id is not even referenced by any
interior AST today — so requiring `input ∈ elements[]` would force consumer-tracing on your side
for zero functional gain. It also matches the old leaf-name target, so it's non-regressive.

**Engine now honors this robustly:** the submodel executor injects a fixed element for a driven
`input` id that has no interior element, so any interior reference to that boundary-port id
resolves to the parent-supplied value — whether or not the port is a distinct interior element.
So both shapes work: `input` = a real interior element (overridden) OR a synthesized boundary port
(injected). The only requirement is that `from` resolves to a fixed-value parent element — which,
as you note, is the part that actually drives, and it's solid. Verified with a fixture where an
interior expression reads a synthesized port id and receives the parent driver's value.

Practical note: driving currently reads the `from` element's value only when it's a **fixed
scalar** (the case for all optimization variables and the corpus drivers). A `from` pointing at a
computed/expression parent element isn't evaluated into the submodel yet — flag any such case if
it arises; none in the corpus today.
