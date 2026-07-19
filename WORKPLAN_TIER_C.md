# Workplan — Tier C: big bets (decide deliberately, per-item go/no-go)

**Source:** `GOLDSIM_ENGINE_GAP_ANALYSIS.md` (§10) + feasibility triage 2026-07-19.
Tier C items are NOT a batch: each begins with an explicit **go/no-go gate** driven by
corpus demand or a named target model. Do not start an item because it is next in the
file; start it because the gate says so.

**Prerequisites:** Tier A done; Tier B at least through B1 (the timebase invariant
affects C1 and C2 design). State pinned at authoring (2026-07-19): HEAD `f47387c` +
uncommitted 0.9.2 changes; two tiers of drift are expected by the time this runs — the
kickoff prompt's drift check is the first work item.

**Standing conventions:** as Tier A/B (wasm rebuild, schema symlink + `$id`/CHANGELOG,
topical tests, additive result shapes, emit deltas appended to
`EMIT_ISSUES_0.9.1_CORPUS.md`).

**Explicit non-goals (documented so nobody re-litigates them by accident):**
DLL/ODBC/external coupling (contradicts the open-JSON/WASM thesis — gap-analysis §9.4
itself calls it a design difference); Localized-Container namespace scoping (that is
emit-side id-qualification — emit-spec item 2, not an engine gap); Clone; distributed
processing; per-container internal clocks (submodels + B1 cover the need); full
adaptive error-controlled timestepping (contradicts the shared Euler philosophy).

---

## C1. Procedural Script executor (gap #2 — highest corpus pressure)

**Go/no-go gate:** the target models are the script-heavy corpus set — `sac_sma`
(~19 leaked local names, ~150 dangling refs), `water_hammer`, `wind_model`,
`precipgen_par`. GO if faithful execution of these matters; they are currently the
largest population of silently-wrong (0.0-evaluating) models. Check first whether the
emit-side scoping-leak fix (emit-spec item 2) has landed — it changes what the decoder
can hand us and the two workstreams should be co-designed.

**Design sketch (engine side, ~medium):**
- New `NodeRule::Script`: `params` (bound from `inputs`), `locals`, `body:
  Vec<Statement>` where `Statement = Assign | If | While | For | Break | Continue |
  Return(expr)`; expressions reuse the existing AST (`eval.rs`) evaluated against a
  local-variable scope layered over `EvalCtx` (locals shadow element refs).
- Interpreter in a new `script_v2.rs`: statement walk with a hard step budget
  (e.g. 100k statements/eval, configurable) → exceeding it is an eval error naming the
  element, not a hang. No RNG access from scripts (determinism); no writes to other
  elements (a script computes its own output(s) only — GoldSim's element model, and it
  preserves the dependency graph).
- Per-realization persistent locals (GoldSim scripts can carry state across steps):
  opt-in `static_locals: true`, stored like other per-realization state maps.
  Under the B1 invariant, scripts evaluate **per grid step** (they are stateful).
- Vector locals: reuse `Value::Scalar|Vector`; indexing via existing `Index` semantics.

**Schema side (the real cost):** a `statement` union in the schema + semantics § with
the exact evaluation contract (scope rules, shadowing, step budget, determinism, no
side effects). Version bump. Design the encoding against what re-gsm's decoder can
actually extract from GoldSim Script elements — write the schema PROPOSAL doc first
(repo convention: standalone `WASIM_SCRIPT_*.md`, cf. `wasim_script_execution.md`
which already exists — READ IT FIRST, it is prior art from an earlier round), get emit
sign-off, then implement.

**Tests:** interpreter unit tests (control flow, budget exhaustion, scope shadowing,
static locals across steps); a hand-authored SAC-SMA-like fixture; determinism.
**Effort:** 2–3 weeks engine + schema, plus emit coordination. **Do engine-first with
hand-authored fixtures** so emit has a running target.

## C2. Looping + Conditional containers (gap #8)

**Go/no-go gate:** a corpus or target model that actually needs within-step
convergence (simultaneous equations) or dormancy. Grep the corpus/decoder notes for
GoldSim Looping/Conditional Container occurrences before starting; if zero, defer
indefinitely.

- **Looping Container**: bounded fixed-point iteration of a container's subgraph
  within a step: evaluate members repeatedly until max |Δ| < tol or max-iters (warn).
  Fits as a special evaluation group in the topo schedule (the members form a cycle
  that is currently *rejected* in v2 — this legalizes it under an explicit container
  flag). Interacts with B1: iteration happens within each sub-interval's evaluation
  (it is instantaneous logic, not state).
- **Conditional Container**: activation expression; when inactive, members are not
  evaluated and **hold last value** (define + document t=0-inactive semantics —
  initial values). State-machine-ish: activation edges could fire triggers.
**Schema:** `container_def` gains `condition` / `looping {tol, max_iters}`. Semantics
§12 extension. **Effort:** ~1 week each. Conditional is the cheaper, likelier one.

## C3. Matrix algebra + label-set arrays (gap #12)

**Go/no-go gate:** corpus matrix sites are currently *intentionally-opaque*
`extern_call`s (matrix linalg, `vIndex` — see emit-issues "promoted builtins" note).
GO only when a named model needs solved systems; the blast radius is wide.

- `Value` gains `Matrix(rows, cols, Vec<f64>)` (or nested-Vec) — every `zip_with`/
  broadcast/`as_scalar` site in `eval.rs` must handle it: this is the invasive part;
  audit `Value` usage exhaustively first (also wasm serialization + results paths).
- Label sets: schema `dimensions[]` already exist (0.8.3) — matrices reference two of
  them; `output_spec.dimensions` already carries the declaration.
- Builtins: `matmul`, `transpose`, `solve` (small dense LU with partial pivoting —
  hand-rolled, no new dependency), `identity`, `diag`; promote the corresponding
  `extern_call` names.
**Effort:** 2–3 weeks, dominated by the `Value` audit. Consider gating with a feature
flag while the audit soaks.

## C4. Spreadsheet cell evaluation (gap #13's salvageable slice)

**Go/no-go gate:** 5 corpus models carry `cells` (0.9.1 emits Excel ranges + values).
GO only if any of them matters beyond load-and-report-0; check what the cells actually
contain first (values only vs formulas — if emit ships computed values snapshotted at
export, a *reader* suffices and the mini formula evaluator is unnecessary; that
finding goes back into the emit doc and this item shrinks to a day).

If formulas are present and live: mini evaluator over the embedded cell graph
(arithmetic, refs, ranges, SUM/IF/VLOOKUP-class functions), evaluated per step with
model inputs written into bound cells. Strictly no external file I/O (WASM thesis).
**Effort:** 2 days (reader) … 2 weeks (evaluator), gate decides.

---

## Sequencing & posture

C1 is the only item with standing corpus pressure — default next after Tier B unless
the gate says otherwise. C2–C4 are demand-driven; leave them dormant until a model
names them. Re-run the go/no-go gates whenever the corpus regenerates (emit rounds
change the facts: scoping fix changes C1's input; cell population changes C4).

## Definition of done (per item, since the tier is per-item gated)

- Gate decision recorded at the top of the item (date, evidence, go/no-go).
- Standard conventions checklist (suite, wasm, schema/CHANGELOG/semantics, corpus
  211/211, emit-delta note).
- For C1 specifically: proposal doc reviewed before implementation; hand-authored
  fixture runs before any emit dependency.

---

## Kickoff prompt (copy-paste to start a fresh session on this tier)

```
Read WORKPLAN_TIER_C.md at the repo root. This tier is per-item gated: your first
deliverable is the go/no-go assessment for the item you were asked to start (default:
C1), not code.

Context recovery:
1. Read memory: MEMORY.md + project_wasim_schema_arc.md (conventions, arc, traps).
2. Read GOLDSIM_ENGINE_GAP_ANALYSIS.md §2.7, §3, §9.3 for gap intent; treat its WASiM
   claims as stale-until-verified (two tiers of work have landed since it was
   written).
3. For C1: read wasim_script_execution.md and notes_to_transpiler.md (prior art on
   script/tier boundaries) and the emit-spec section at the end of
   EMIT_ISSUES_0.9.1_CORPUS.md (item 2, scoping leaks) — check whether emit's
   scoping fix landed and what the regenerated corpus now contains for sac_sma /
   water_hammer / wind_model.
4. Code analysis: eval.rs (EvalCtx, Value — exhaustively for C3), engine_v2.rs
   step-loop structure AS IT NOW EXISTS post-Tier-B (the timebase provider + grid
   invariant from WORKPLAN_TIER_B.md B1 constrains where scripts/looping containers
   evaluate), graph_v2.rs cycle policy (C2 legalizes a rejected case).

Drift check (mandatory before any design work):
5. Run: git log --oneline --since=2026-07-18 -- engine/src engine/tests schema
   frontend/src, plus git status. Read the diffs of anything touching eval.rs,
   engine_v2.rs, model*.rs, v2_parse.rs, graph_v2.rs, or schema/. Reconcile this
   workplan in place: (a) schema version + semantics § numbering, (b) whether Tier A/B
   items this plan assumes (A3 results layer, B1 timebase invariant) landed and in
   what form, (c) whether any corpus regeneration changed the go/no-go evidence
   (dangling-ref counts, cells content, matrix extern_call sites — re-run the counts,
   do not reuse this document's numbers). Update the plan, THEN gate, THEN build.
6. Baseline: cargo test green on the targeted subset for the item's area before the
   first change.

For C1, follow the stated order strictly: proposal doc → user/emit sign-off →
interpreter with hand-authored fixtures → schema land → emit coordination. Commit
only when asked.
```
