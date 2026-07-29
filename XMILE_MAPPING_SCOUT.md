# XMILE → WaSim v2 Mapping — Scouting Report

**Purpose.** De-risk an eventual XMILE **import** code path in the WaSim Rust engine by
mapping every XMILE v1.0 construct onto the WaSim v2 model schema (0.9.8), flagging gaps
and semantic mismatches. **No import code is written here** — this is a logical-mapping
reconnaissance whose deliverable is the mapping table plus a recommended first slice.

Grounded against:
- WaSim schema `/Users/collord/wasim/schema/wasim-schema-v2.json` (v0.9.8)
- WaSim engine contract `/Users/collord/wasim/schema/wasim-engine-semantics.md` (v0.9.6 header)
- Value-prop thesis `/Users/collord/wasim/WASIM_VALUE_PROP_THESIS.md` §4.2–4.3
- The OASIS XMILE v1.0 Standard and its example corpus (URLs in §0).

---

## 0. XMILE artifacts (authoritative sources)

**The standard is a single document** — XMILE became an OASIS Standard on **14 Dec 2015**;
the TC **closed 11 Sep 2018**. There is **no separate "Advanced functions" spec**; the
pre-OASIS isee/Ventana "SMILE / display / advanced" white-papers were folded into the one
ratified document. (SMILE = the equation semantics; XMILE = the XML serialization.)

| Artifact | URL |
|----------|-----|
| OASIS Standard (HTML) | http://docs.oasis-open.org/xmile/xmile/v1.0/os/xmile-v1.0-os.html |
| OASIS Standard (PDF) | http://docs.oasis-open.org/xmile/xmile/v1.0/os/xmile-v1.0-os.pdf |
| XSD schema | http://docs.oasis-open.org/xmile/xmile/v1.0/os/schemas/xmile.xsd |
| Canonical examples dir | http://docs.oasis-open.org/xmile/xmile/v1.0/os/examples/ |
| Errata 01 (Jan 2016) | https://docs.oasis-open.org/xmile/xmile/v1.0/errata01/xmile-v1.0-errata01-complete.html |
| Standard landing / citation | https://www.oasis-open.org/standard/xmile1-0/ |
| TC page (closed 2018) | https://www.oasis-open.org/committees/tc_home.php?wg_abbrev=xmile |

**Namespace:** `http://docs.oasis-open.org/xmile/ns/XMILE/v1.0`.

**Example corpus (concrete files):**
- OASIS canonical: `variables.xml`, `arrays.xml`, `lynx-hares.xml`, `corporate-growth.xml`,
  `single_file_submodel.xml`, `master.xml`+`included_model.xml`+`included_macros.xml`
  (submodel/include demo) — all under the examples dir above.
- SDXorg test-models (community corpus **with canonical numeric output**, multiple formats):
  https://github.com/SDXorg/test-models — `samples/` has `teacup`, `Lotka_Volterra`,
  `Population`, `SIR`, `Workforce`, `arrays/{a2a,non-a2a}`,
  `bpowers-hares_and_lynxes_modules` (module demo), `pendulum`, `SIR`, …
  - Teacup (hand-coded XMILE): https://raw.githubusercontent.com/SDXorg/test-models/master/samples/teacup/teacup.xmile
- System Dynamics Society XMILE page: https://systemdynamics.org/resources-old/xmile/

**Representative XML** (verbatim from OASIS `variables.xml` / SDXorg `teacup.xmile`):

```xml
<!-- Stock: <eqn> is the INITIAL value, not a rate. Flows couple by name. -->
<stock name="Backlog">
  <eqn>8000</eqn>
  <inflow>orders_entered</inflow>
  <outflow>orders_completed</outflow>
  <non_negative />
  <units>SKU</units>
</stock>

<!-- Flow: <eqn> IS the rate expression -->
<flow name="Heat Loss to Room">
  <eqn>("Teacup Temperature"-"Room Temperature")/"Characteristic Time"</eqn>
</flow>

<!-- Aux with an EMBEDDED graphical function: eqn result → x → interpolated y -->
<aux name="effect_of_backlog_on_delivery_rate">
  <eqn>Backlog / normal_backlog</eqn>
  <gf>
    <yscale min="0" max="10" />
    <xpts>0.9,1,1.7,2.3,3.5,6.3,10,20</xpts>
    <ypts>0,1,3.5,4.3,5,5.6,6,6.5</ypts>
  </gf>
</aux>
```

**Uncertainty — confirmed deterministic.** `<sim_specs>` carries only `method / start /
stop / dt / time_units / pause` — **no run-count, no Monte-Carlo mode, no
sensitivity/sweep metadata**. Random builtins (`NORMAL`, `RANDOM`, `POISSON`, `EXPRND`,
`LOGNORMAL`) are **per-step draws** with an optional reproducibility `seed`, not
distributions over an ensemble. Sensitivity/MC run configuration lives in the **tools**
(Stella Sensitivity Specs, Vensim `.vsc`), never in the XMILE file. This is the central
asymmetry the WaSim thesis (§4.3) is built on and it dominates the mapping below.

---

## 1. Construct-by-construct mapping table

Confidence legend: **direct** = 1:1 field mapping; **transform** = mechanical rewrite
needed (rename, restructure, or an equation lowering); **gap** = no WaSim equivalent, or
a semantic difference that must be resolved by policy.

| XMILE construct | WaSim target | Confidence | Notes |
|---|---|---|---|
| `<xmile>` root, `<header>` | model root object; header → `source.*` provenance / `name` | direct | Header metadata is lossy-safe to park in `source` or drop. |
| `<model>` (single) | the `elements[]` flat list | direct | XMILE names are container-scoped; WaSim ids are **globally unique** — must qualify (see §1a). |
| `<model>` (multiple) + `<module>` | `container_def` with `kind:"submodel"` (§12) + `interface` | implemented | See "module" row; runs at n_realizations:1 with parent reads via `submodel_stat(mean)` → deterministic inline sub-call. |
| `<sim_specs> start/stop/dt` | `simulation_settings.{duration,timestep}` (+ `calendar_start` for `start`) | transform | `duration = stop − start`; `timestep = dt`. XMILE `start`≠0 has no direct WaSim "start offset" except via the calendar anchor — see §1b. |
| `<sim_specs method="euler">` | (implicit; WaSim is explicit-Euler only) | direct | Euler is WaSim's only integrator — matches. |
| `<sim_specs method="rk4"/"rk2"/…>` | — | **gap** | WaSim integrates explicit-Euler only. RK2/RK4/RK45/Gear have **no** engine equivalent. Import must down-convert (accept + warn, results will diverge) — see §3. |
| `<stock>` | `primitive:"stock"`, `initial_value` from `<eqn>`, `inflows[]`/`outflows[]` from `<inflow>`/`<outflow>` | direct\* | \*The `<eqn>` is the **initial value**, NOT a rate — do not map it to `rate`. Clean for the plain case; conveyors/queues break it (below). |
| stock `<non_negative/>` | `floor: 0` on the stock | **direct** | VERIFIED against engine source. `floor` is a first-class field on every stock (`model_v2.rs:326`), applied two ways each step: outflows via `priority_withdrawal` are curtailed to `(level − floor)` (`engine_v2.rs:2545`), AND the level is unconditionally clamped post-Euler-step `next = next.max(floor)` (`engine_v2.rs:2628-2630`). So `<non_negative/>` ⇒ `floor: 0` is a plain field set. Fidelity note: WaSim's clamp is a *post-step level clamp*; XMILE non-negativity *curtails the breaching outflow*. For a plain (non-withdrawal) outflow that overshoots zero within one Euler step, the clamped level matches but the realized outflow that step differs slightly — a numerical-fidelity footnote, not a representational gap. |
| stock `<conveyor len=… capacity=…>` (`<leak>`,`<arrest>`) | — (closest: `link` `transit_buffer` with `transit_time`) | **gap** | A conveyor is a *transit delay stock*. WaSim has transit delay on **links** (`transit_time`, FIFO plug-flow), not on stocks. Import would have to re-express a conveyor as a link+stock pair; leak/arrest have no clean home. Non-trivial. |
| stock `<queue>` | — | **gap** | No WaSim queue primitive. Would require decomposition; out of first-slice scope. |
| `<flow>` | `primitive:"node"`, `value_rule:"expression"`, `<eqn>`→AST | direct | A flow is a rate expression node; the stock's `inflows[]`/`outflows[]` reference it by id. Matches thesis §4.2 ("flow is a rate expression"). |
| flow `<non_negative/>` | wrap AST in `max(expr, 0)` (`call fn:"max"`) | transform | Mechanical: `non_negative` flow ⇒ `max(rate,0)`. |
| `<aux>` | `primitive:"node"`, `value_rule:"expression"` (or `"fixed"` if `<eqn>` is a constant) | direct | "Auxiliaries are ordinary derived nodes" (thesis §4.2). Constant-eqn aux → `fixed`; formula aux → `expression`. |
| `<gf>` (named, standalone) | `primitive:"node"`, `value_rule:"lookup"`, `table:{x[],y[]}` | direct | Called elsewhere via `lookup_call` op. |
| `<gf>` embedded in a variable | a companion `lookup` node + a `lookup_call` wrapping the host `<eqn>` | transform | XMILE embeds the GF *inside* the aux/flow: the `<eqn>` result is the x. Lowering: emit the eqn as one node, a `lookup` node for the table, and make the host output `lookup_call(table, eqn_node)`. Two WaSim elements per one XMILE variable. |
| `<gf type="continuous">` | `interpolation:"linear"` | direct | Continuous = linear interp, flat beyond ends. WaSim `linear` clamps at ends — matches. |
| `<gf type="extrapolate">` | — (nearest: `linear`) | **gap (minor)** | WaSim `linear` clamps at the ends; it does **not** extend the end slope. Extrapolate has no exact WaSim mode → down-convert to `linear` + warn (values differ only outside the x-range). |
| `<gf type="discrete">` | `interpolation:"step"` | direct | Step/hold ≙ WaSim `step`. |
| `<gf>` with `<xscale>` (even x) | expand to explicit `x[]` | transform | WaSim tables are explicit knot lists; evenly-spaced `xscale min/max` must be expanded to `x[]`. Trivial. |
| `<module>` / `<connect from= to=>` | `container_def kind:"submodel"` (n_realizations:1) + `interface.inputs[]{input,from}`; parent reads of `mod.var` → `submodel_stat(mean, …)` | **implemented** | `<connect>` edges → interface bindings; the submodel runs deterministically (n_realizations:1) and each parent reference to an interior variable is rewritten to `submodel_stat(mean, submodel_id, output)`, which at one realization returns the exact interior value. This reproduces XMILE's inline deterministic sub-call — values flow back across the boundary (verified `result = 11` end-to-end). Only interior vars the parent never references stay unsurfaced. |
| `<dimensions>`/`<dim>` (numeric size) | top-level `dimensions[]` → `dimension_def{id,name,size}` | direct | WaSim ordinal sets are exactly named ordered dimensions with a `size`. |
| `<dim>` with named `<elem>` | `dimension_def` with `labels[]` | direct | Named members → `labels[]`. |
| array var, **apply-to-all** `<eqn>` | one node whose expression is a `vector_map` over the dim | transform | `vector_map{over:dim, body:eqn}` produces the dimensioned array from one formula. Clean when the eqn is uniform. |
| array var, **element-specific** `<element subscript=k>` | either N scalar nodes, or an `array` constructor / per-index `if` | transform | Non-apply-to-all has no single WaSim comprehension; lower to an inline `array` constructor of the per-element ASTs, or explode to N elements. More work than a2a. |
| subscript ref `v[Boston,dresses]` | `index{array, indices[]}` (1–2 axes) with `index_ref` inside `vector_map` | transform | WaSim supports ≤2 axes (`row`/`col`). XMILE allows more (`uses_arrays max_dimensions=N`). ≥3-D arrays are a **gap**. |
| `<units>` / `<model_units>` | `quantity.unit` on values; `display_unit` for labels | transform | WaSim normalizes to SI at load; unit *algebra* in `<unit><eqn>` (e.g. `J = kg*m^2/s^2`) must be resolved to a base unit string. WaSim does static dim-checking (§14) but not arbitrary user-defined unit equations → resolve to SI or drop with warn. |
| `<eqn>` infix expression | `$defs/ast_node` (op-discriminated AST) | transform | The parser is the hardest sub-component — see §2. |
| random builtins (NORMAL/RANDOM/…) as **per-step draws** | `value_rule:"process"` or a per-step `sample` with `resampling` | transform | XMILE randoms are per-step; WaSim's `sample` is once-per-realization by default → need `resampling` each step, or map to a per-step stochastic node. Semantics preserved but not 1:1. |
| **Monte-Carlo / ensemble** | native (`n_realizations`, realization-major engine) | **gap (in XMILE's favor is reversed)** | XMILE has **no** ensemble concept to import. WaSim's per-realization uncertainty is *added value on import*, not something the file carries. Import defaults `n_realizations:1`. |
| `<macro>` (user function) | inline-expand into AST, or `extern_call` | transform / gap | Simple single-`<eqn>` macros can be **inlined** (substitute params). Complex macros (embedded `<variables>`, own `<sim_specs>`, recursion) have **no** WaSim analog → `extern_call` stub or a submodel, both lossy. |
| `<behavior>`, `<style>`, `<views>`, `<display>` | dropped / `connections[]` for graph geometry | direct (drop) | Presentation; not simulation-bearing. WaSim keeps a visual `connections[]` list but the rest is discardable. |

### 1a. The id-uniqueness transform (pervasive, easy to underestimate)

XMILE variable **names** are scoped per `<model>`/module and may repeat across modules
(the same bare `nCells` in two modules). WaSim `id` MUST be **globally unique** across the
whole `elements[]` list (semantics §1). Every import must **qualify** ids by module path
(e.g. `Main/Backlog`) while keeping the human name in `name`. All references — `<inflow>`,
`<outflow>`, eqn variable refs, `<connect>` — must be **rewritten to the qualified id**.
This is not hard but it is *everywhere* and easy to get wrong; it should be the importer's
first pass (build a name→id table per scope, then resolve).

### 1b. The `start`-time asymmetry

XMILE `<start>` may be nonzero (e.g. simulate 1990→2020). WaSim `simulation_settings`
has `duration` + `timestep` and a t=0-relative clock; the only place a real start date
lives is `calendar_start` (epoch seconds), which drives calendar `time_ref` properties but
**does not shift the internal clock origin**. Import policy: set `duration = stop − start`,
`timestep = dt`, and if any equation reads `TIME`/`STARTTIME` absolutely, offset those refs
by `start`. For most SD models `start` is 0 or cosmetic, so this is usually benign — but
flag it so a 1990-based model doesn't silently run 0-based.

---

## 2. Equation translation (`<eqn>` → `ast_node`)

The `<eqn>` body is a SMILE infix expression language. Translation is **parse → lower**.
The parser (Pratt/precedence-climbing over the §3.3.1 operator table) is *the single
hardest sub-component of the whole importer* — more than the XML, more than the structural
mapping. It is a real programming-language front end: tokenizer, precedence, right-assoc
`^`, `IF/THEN/ELSE`, function calls, quoted identifiers (`"Room Temperature"`), subscript
brackets. Budget accordingly.

**Operator mapping (direct):**

| XMILE | WaSim AST |
|---|---|
| `+ − * / ^` | `add subtract multiply divide power` |
| unary `−`, `NOT` | `neg`, `not` |
| `< <= > >= = <>` | `lt lte gt gte eq neq` |
| `AND OR` | `and or` |
| `IF c THEN a ELSE b` | `if{cond,then,else}` |
| `MOD` | `mod` (binary op → `call fn:"mod"`) |
| quoted/bare identifier | `ref{element_id}` (after id-qualification, §1a) |
| numeric literal | `literal{value,unit?}` |
| `v[i,j]` subscript | `index{array, indices[≤2]}` |
| `gfname(x)` | `lookup_call{element_id, input}` |

**Builtin function mapping.** WaSim's `call` `fn` enum (schema lines 837–887) and the
fallback `extern_call` (preserves name+args, evaluates 0.0 + warn) together cover most of
XMILE. Roster:

| XMILE builtin | WaSim | How |
|---|---|---|
| `ABS EXP LN LOG10 SQRT` | direct | `abs exp ln log sqrt` (`LOG10`→`log`, WaSim `log` is base-10; confirm base — WaSim also has `log2`) |
| `SIN COS TAN ARCSIN ARCCOS ARCTAN` | direct | `sin cos tan asin acos atan` |
| `INT` | direct | `int` (also `floor`/`ceil`/`round` available) |
| `MIN MAX` | direct | `min max` |
| `MOD` | direct | `mod` |
| `PI()` | transform | `literal{3.14159…}` |
| `INF()` | gap | no infinity literal → `extern_call` or a large `literal` (policy). |
| `TIME()` | direct | `time_ref{property:"elapsed"}` (+ `start` offset per §1b) |
| `DT()` | direct | `time_ref{property:"timestep"}` (or reserved global `TimestepLength`) |
| `STARTTIME()` `STOPTIME()` | transform | constants from `simulation_settings` (`start`; `start+duration`) → `literal`, or `time_ref{start}` + `SimDuration` reserved global. |
| `STEP(h,t0)` | direct | `call fn:"step"` (verify WaSim `step`'s signature matches height/start-time). |
| `PULSE(mag,first[,int])` | transform | no native pulse → compose from `time_ref` + `if`, or `extern_call`. |
| `RAMP(slope,t0)` | transform | compose `max(0, slope*(elapsed − t0))` from primitives. |
| `INIT(x)` | **gap** | "value of x at STARTTIME." WaSim has no init-value accessor. For a stock, `INIT(stock)` = its `initial_value` (resolvable at import); for a general var it needs a captured-initial node. Partial. |
| `PREVIOUS(x, init)` | direct | `value_rule:"lag"` node (`input:x, initial:init`) — exact one-step delay (semantics §2.7). |
| `DELAY(in,t[,init])` | transform | fixed delay → `convolution` (unit-impulse offset) or chained `lag` nodes (semantics §2.7 explicitly documents this). |
| `DELAY1/DELAY3/DELAYN` | transform | 1st/3rd/Nth-order exponential *material* delays → cascade of N first-order stocks (each a `stock` with in/outflow). Mechanical but expands one call into several elements. |
| `SMTH1/SMTH3/SMTHN` | transform | information smoothing → EMA. `SMTH1` ≈ `value_rule:"filter" statistic:"ema"` (semantics §2.11); `SMTH3/N` = cascade of N EMAs / first-order stocks. |
| `TREND` | transform | derived from a smooth (`(input − SMTH)/(SMTH·avg_time)`) → compose. |
| `FORCST`/`FORECAST` | transform | `input·(1 + trend·horizon)` → compose from TREND. |
| `NORMAL/RANDOM/POISSON/EXPRND/LOGNORMAL(...[,seed])` | transform | per-step draw → `sample` node with `resampling` each step (families: normal/uniform/poisson/exponential/lognormal all exist in WaSim's roster). Seed → run seed, not per-node. |
| `SELF()` | gap | self-reference inside PREVIOUS/SIZE → handle during `PREVIOUS` lowering; standalone → `extern_call`. |
| `SUM`/array reductions | direct | `sum_array mean_array min_array max_array size_array` etc. |
| anything unrecognized | fallback | `extern_call{fn, args}` — preserves connectivity, evaluates 0.0 + warn. **This is the safety net that lets a partial importer produce a loadable model.** |

**Net effort read.** The operator/arithmetic/logic/most-math core is a *direct* AST
lowering — a week-ish of parser + a visitor. The SD-specific builtins (`DELAY*`, `SMTH*`,
`TREND`, `FORCST`, conveyors) are where the effort concentrates, because each is a **macro
expansion into multiple WaSim elements**, not a single op. A first slice can `extern_call`
all of them and still round-trip structure.

---

## 3. Gaps & asymmetries

**XMILE has, WaSim can't (yet) express:**
- **RK2 / RK4 / RK45 / Gear integration.** WaSim is explicit-Euler only. Any model that
  relies on `method="rk4"` for accuracy (oscillators, stiff systems — e.g. the pendulum /
  Roessler test-models) will **diverge numerically** on import. Down-convert to Euler +
  loud warning; do not silently claim fidelity. This is the single most important
  numerical caveat.
- **Conveyors, queues.** WaSim's transit delay lives on **links** (`transit_time`, FIFO),
  not stocks; there is no queue primitive. These are stocks with internal transit/ordering
  state that WaSim's stock lacks — they need decomposition and are out of first-slice scope.
  (Non-negative stocks are NOT in this bucket — see the direct `floor` mapping above.)
- **Multi-order material/info delays and TREND/FORECAST** as single constructs — expressible
  but only by expanding into several elements.
- **≥3-D arrays** (`uses_arrays max_dimensions ≥ 3`). WaSim indexing is ≤2 axes (row/col).
- **Complex `<macro>`s** (embedded variables / recursion) and **user-defined unit algebra**.
- **`INIT()` / `SELF()`** initial-value/self accessors.

**Where the thesis's "stock = lag accumulator, flow = rate" claim is clean vs. not:**
- **Clean:** the plain SD case — a `<stock>` with `<inflow>`/`<outflow>` and a constant or
  formula `<eqn>` initial, integrated by Euler. This is *exactly* WaSim's stock
  (`initial_value` + `inflows[]`/`outflows[]`, `S_{t+1}=S_t+net_rate·dt`, semantics §3).
  The teacup, Lotka-Volterra, SIR, Population models all sit here. The claim holds.
- **Complicated:** (1) a stock's `<eqn>` is an **initial condition**, easy to misread as a
  rate — a real importer bug waiting to happen. (2) **`<non_negative>`** is a clamped
  integrator — maps directly to WaSim's `floor: 0` (a first-class stock field), with a
  minor post-step-clamp-vs-flow-curtailment fidelity footnote. (3) **conveyors/
  queues** are stocks with internal transit/ordering state that WaSim's stock does not
  have — the "lag accumulator" abstraction breaks entirely there. So the claim is true for
  *canonical* SD and needs qualification for the delay/clamp stock variants.

**WaSim has, XMILE lacks (the thesis's whole point, §4.3):**
- **Native per-realization Monte-Carlo / uncertainty.** XMILE randoms are per-step draws in
  a single deterministic run; WaSim's realization-major engine, distribution `sample`
  nodes, correlated sampling, LHS, and importance sampling have **no XMILE representation**.
  Import defaults `n_realizations:1`; the uncertainty is *added by the user after import*.
- **Diffable JSON** vs. verbose, review-hostile XML.
- **Optimization studies, submodel statistics, failure/event state machines, transport/
  species cells** — all WaSim primitives with no XMILE counterpart (they're irrelevant on
  import but underline the asymmetry direction).

---

## 4. Import-path implications (scouting sketch, not a design)

- **XML parser.** Use a streaming/DOM Rust XML crate (`quick-xml` for speed, or `roxmltree`
  for an ergonomic read-only tree — the importer only reads). The XML layer is the *easy*
  part; the XMILE XSD is small and well-structured.
- **Target the v2 JSON directly, via a thin intermediate.** A small in-memory IR
  (variables + parsed equation ASTs + scope table) between XML and v2 JSON pays for itself
  because two hard passes need a settled intermediate: (a) **id qualification / reference
  resolution** (§1a) and (b) **equation lowering / macro expansion** (§2), which turns one
  XMILE variable into several WaSim elements (embedded GFs, delay cascades). Emitting v2
  JSON straight from the XML tree while also doing those rewrites would tangle. Keep the IR
  minimal — it is a lowering scratchpad, not a second schema.
- **The equation-language parser is the hardest sub-component.** Not the XML, not the
  structural map. Budget the bulk of the effort here (tokenizer, precedence-climbing,
  quoted identifiers, subscripts, `IF/THEN/ELSE`, builtin dispatch). The `extern_call`
  fallback is what lets an incomplete parser still emit a **loadable** model — lean on it.
- **Round-trip fidelity expectations.** *Structural* round-trip (topology, names, flows,
  tables) is achievable and high-fidelity. *Numerical* round-trip is **not guaranteed**:
  Euler-only vs. RK4, delay-cascade discretization, and per-step-random reinterpretation
  all introduce divergence. Set the acceptance bar at "same trajectory under Euler," not
  "bit-identical to Stella/Vensim." Mirror the SDXorg test-models' *canonical output* only
  for Euler models.
- **Warnings, not rejections.** Follow the engine's own tolerance posture (dangling refs →
  0.0 + warn, unimplemented `submodel_stat` → 0.0 + warn): a partial import should produce
  a runnable model with a diagnostics list, not fail. Every gap in this report should
  surface as a named warning, not an abort.

---

## 5. Recommended first slice

Prove the mapping end-to-end on **one real, canonically-verified model** — mirroring the
thesis's "one end-to-end lens round-trip" acceptance test — with the **minimal** XMILE
subset:

**In scope:** `<stock>` (plain, with `<inflow>`/`<outflow>`, constant/formula `<eqn>`
initial) · `<flow>` (rate `<eqn>`) · `<aux>` (constant + formula) · `<gf>` (named and
embedded, `continuous`/`discrete`) · `<sim_specs>` Euler with `start/stop/dt` · the
arithmetic/logic/core-math + `IF/THEN/ELSE` slice of the equation language · id
qualification (§1a).

**Explicitly out:** arrays/subscripts, modules, macros, conveyors/queues,
`DELAY*`/`SMTH*`/`TREND`/`FORCST` (all → `extern_call` stub for this slice), RK integrators,
units algebra. (`<non_negative>` stocks/flows are *in* — a free `floor: 0` / `max(rate,0)`.)

**Target model: `teacup`** (https://raw.githubusercontent.com/SDXorg/test-models/master/samples/teacup/teacup.xmile).
Why: it is the canonical smallest XMILE model — one stock (`Teacup Temperature`, initial
180), one outflow (`Heat Loss to Room` = rate expression), two aux constants
(`Room Temperature`, `Characteristic Time`), Euler, and **SDXorg ships canonical numeric
output** for it. Every construct in it is in the "clean" column of §3. A second step adds
one embedded-`<gf>` model (OASIS `variables.xml`'s `effect_of_backlog_on_delivery_rate`) to
exercise the lookup-lowering transform.

**Acceptance test:** import teacup → v2 JSON → run WaSim engine (Euler) → compare the
`Teacup Temperature` trajectory against SDXorg's canonical output within Euler tolerance.
That single green round-trip validates the stock/flow/aux/Euler core — the 80% of real SD
models that live entirely in the clean column — and de-risks everything after it as
incremental builtin/array/module work behind `extern_call` stubs.

---

*Sources are inline throughout §0–§2. Primary: OASIS XMILE v1.0 Standard
(docs.oasis-open.org/xmile/xmile/v1.0/os/), SDXorg/test-models corpus, and the WaSim v2
schema + engine-semantics documents cited at the top.*
