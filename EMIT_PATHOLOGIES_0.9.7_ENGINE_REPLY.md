# Emit pathologies 0.9.7 — engine reply

**Scope.** Engine-side response to `EMIT_PATHOLOGIES_0.9.7_RESOLUTION.md` (the re-gsm team's fixes +
5 feedback items). Covers: the final parse confirmation the resolution requested, **one remaining
pathology the resolution missed (now fixed engine-side)**, and a point-by-point answer to the 5
feedback items.

**Headline.** Re-running `parse_v2` → `ModelGraphV2::build` over the regenerated corpus:
**218/220 clean on the first pass** (was 190/220) — the pid (15) and status (13) fixes fully
resolved. The last 2 failures were a **fourth pathology the resolution did not catch**; fixed
engine-side. **The corpus is now 220/220 parse+build clean.**

---

## Pathology 4 (new) — `PartitionEntry.species: null` (2 files) — FIXED ENGINE-SIDE

The resolution fixed the top-level `species` *definition* element (set `species` = element name).
But `demo.json` / `demonstration_llw_sa_model_v1_15.json` still failed with the identical serde
error `missing field 'species'` — this time on a **different site**: a cell's
`partitioning[].species` entry, emitted as `null`:

```jsonc
"partitioning": [{
  "species": null,                         // ← the Kd's species binding is null
  "from_medium": "Water", "to_medium": "CoverMaterial",
  "coefficient": { "ast": { "op": "ref", "element_id": ".../Kd_Cover" } }
}]
```

**This is correct emit, not a bug** — and it is the *same* set-vs-per-species tension as Pathology 3,
which the resolution argued (rightly) should be modeled as a **set/dimension**. A GoldSim Kd is one
coefficient per `(from_medium, to_medium)` pair applied to the **whole species set**, so there is no
single species to name. The old engine `PartitionEntry.species: String` (required) couldn't express
"applies to all species."

**Engine fix.** `PartitionEntry.species` is now `Option<String>`: `None` (null/absent) = the Kd
applies to **every species in the cell's set**.
- [model_v2.rs](engine/src/model_v2.rs) / [v2_parse.rs](engine/src/v2_parse.rs): field is optional.
- [engine_v2.rs](engine/src/engine_v2.rs) `partition_ratios`: the entry matches a species if it
  names it *or is set-wide* (`e.species.as_deref().map_or(true, |s| s == species)`).
- The equilibrium pass no longer adds a `None` entry's (nonexistent) species to the species set.

Tested: `cells_v2::partitioning_set_wide_species_null` (two species share one `species: null` Kd=4;
both partition solid:fluid = 4:1). Both previously-failing corpus files now parse + build.

This directly implements the "set-as-dimension" model the resolution advocated in Pathology 3 —
consistent treatment on both the species def *and* the partition Kd.

---

## Answers to the 5 feedback items

### #1 — PID gains `kp`/`ki`/`kd` should accept `quantity_or_formula`, not just `number`

**Agreed in principle; not yet changed. Low urgency.** The engine's `PidController` stores
`kp/ki/kd` as `f64` and the parser reads them as optional numbers (defaulting to 0.0). Your point is
correct: GoldSim wires gains to elements, so they may be refs/expressions unknown at emit time.

However, converting these to `quantity_or_formula` is a **non-trivial engine change** (the PID
handler evaluates `kp*error + ki*integral + kd*derivative` once per step with `f64` gains; making
them formulas means resolving three `quantity_or_formula`s per PID per step, threading `ctx`). Given
only **3 true-PID models** are affected and gains-absent → 0.0 → feed-through is a *visible* wrong
behavior (not silent), I'd rather do this deliberately than rush it. **Proposed:** track as a small
follow-up; when done, `kp/ki/kd` will accept a `quantity_or_formula` (ref or literal) exactly like
`setpoint` already does. Until then, emitting the gains as literal numbers where re-gsm *can* resolve
the constant (many gains are plain constants) would fix most cases without any engine change — worth
checking how many of the 3 have constant vs. wired gains.

### #2 — Controller mode collapsed onto `value_rule: "pid"` — does the engine degrade?

**Partially. `on_off` does NOT degrade correctly; `proportional` does.** The engine's handler is a
pure PID:

```
error = setpoint − measured   (zeroed within deadband)
out   = kp·error + ki·integral + kd·derivative   (clamped to output_min/max)
```

- **`proportional`** (kp only, ki=kd=0): degrades **correctly** — it *is* a P-controller.
- **`on_off`** (bang-bang): does **NOT** degrade correctly. With all gains 0 the output is a
  constant 0 (feed-through of nothing), not a switching on/off signal. A bang-bang controller needs
  `out = if error > 0 { output_max } else { output_min }` (a sign/threshold rule), which the PID
  math cannot produce with any gain values.

**Recommendation:** the engine does **not** currently read `controller.mode`. Two options: (a) the
engine adds a `mode` field to `PidController` and branches (on_off → threshold rule); or (b) re-gsm
emits `on_off` controllers as a `gate`/expression node (`if measured < setpoint then hi else lo`)
rather than `pid`. **(b) is the smaller change** and keeps the `pid` rule honest (a genuine PID). I'd
lean (b) unless there are many on_off controllers. How many of the 15 controller models are
`on_off`? (You noted `control_systems.json` is one.)

### #3 — 6 status nodes with no decodable trigger (never-firing `on_condition`)

**Acknowledged; no engine action needed. Correctly a re-gsm gap.** The engine parses a
`{"mode":"on_condition"}` with no `condition` as a never-firing trigger (the latch stays in its
initial state), which is the safe degradation. These 6 are inert-but-valid — they won't crash, they
just never latch. Binding them needs the event-link → `on_event` trigger work you flagged (tracks
with `CONNECTION_WIRING`). The engine **already supports `on_event` triggers** (`trigger.mode:
"on_event"` + `source`, wired in the S1 round — see semantics §6), so when re-gsm can resolve the
event-link sources, no further engine work is required to consume them.

### #4 — Species-set as dimension with `members[]`, not N species defs

**Confirmed: the engine accepts this shape.** A `species` element with a non-null name and an extra
`members: [...]` field parses fine — the engine's `RawElement` does not `deny_unknown_fields`, so
`members` is silently ignored (harmless). Dimension `ref`s to the species element resolve normally.
So your divergence from the proposed per-member expansion is **fully compatible** with the engine as
it stands, and — per Pathology 4 above — it's the model the engine now supports end-to-end
(set-wide Kd included).

**One caveat (not a blocker):** the engine's cell **decay** pass keys on *per-species* `half_life` /
`decay_products` (`species_info.get(sp)`). A single set element with `half_life: null` means **no
radioactive decay** is applied to any nuclide in the set. For the LLW/demo models (which are decay-
chain models: U238→…, the whole point), decay won't run until either (a) re-gsm *also* emits
per-member species defs carrying each nuclide's half-life/products (coexisting with the dimension
element), or (b) the schema grows a way to attach per-member half-lives to the set. This is the
"genuine schema/model question" you raised — **it is real for decay-chain models**, but it's the
next layer (cell physics), not a parse blocker. Recommend tracking it with the cell mass-delivery
decode gap (cells still emit no `inflows`/`initial_inventory`, so cell mass is zero regardless — see
the S2 validation notes).

### #5 — Encode discriminated body requirements in the JSON schema (if/then/required)

**Strongly agree — this is the right structural fix.** All these pathologies passed schema
validation but failed the engine parse precisely because the per-`value_rule` required fields live
only in `v2_parse.rs`, not the schema. A JSON-schema `if`/`then`/`required` block per discriminator
(pid → require input+setpoint; status → require set+reset; etc.) would let *any* producer or
validator (`emitcheck.py`, CI, future tools) catch a partial emit without running the engine. Your
manual `emitcheck.py` gates are a good stopgap; the schema-level constraint is the durable fix. This
is a **schema change** (owned in openvsim), and the engine would benefit from it too — worth doing.

---

## Net

- **Corpus: 220/220 parse+build clean** after the engine-side `PartitionEntry.species` fix.
- **4th pathology** (partition Kd species-null) found and fixed engine-side — it's the correct
  set-wide-Kd shape, consistent with the resolution's set-as-dimension model.
- Feedback: **#3, #4 need no engine action** (already supported / correctly degrading); **#1, #2, #5
  are agreed follow-ups** — #1 (gain formulas) and #2 (on_off mode) are small engine/emit changes we
  should schedule; #5 (schema if/then) is an openvsim schema improvement that helps everyone.
- **Still gated on emit** (not parse blockers): cell mass-delivery decode (inflows/inventory →
  cell mass is zero) and per-nuclide half-life for decay chains.

---

*Generated 2026-07-21, engine reply to `EMIT_PATHOLOGIES_0.9.7_RESOLUTION.md`. Corpus re-verified at
`~/openvsim/wasim/schema_examples` (220 files).*
