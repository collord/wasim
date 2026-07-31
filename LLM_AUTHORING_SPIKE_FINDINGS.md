# LLM-authoring feasibility spike — findings

**What this is.** A throwaway spike to answer one question before anyone builds the §17 copilot
(`WASIM_AUTHORING_ENVIRONMENT_SPEC.md`): *can an LLM produce schema-valid WASiM models, and does
the engine-validate feedback loop actually converge?* Prototyped with the `claude -p` trick — a
headless Claude Code agent given an authoring guide + one exemplar + a validator binary, looping
draft → validate → fix on its own. This is §17.3's `propose_model → validate()` loop, run by Claude
Code's native tool loop instead of an in-browser SDK.

**Verdict: feasible, and the loop is the crux.** Both test prompts converged to valid, *physically
correct* running models. The number of iterations tracked almost perfectly with how well the target
construct was documented — which is the actionable finding.

## What was built (all reusable)

- **`engine/src/bin/wasim-validate.rs`** — a native headless validator + quick-run. Reads a
  `model.json`, runs the engine's parse + graph + dimensional validation, optionally runs a quick
  sim, prints a JSON report (`{ok, errors[], warnings[], topo[], run?}`) + a human digest, exits
  non-zero on error. This is the engine ground-truth §17.3's `validate()`/`run()` tools would wrap.
  (It replicates `wasm.rs::validate_json`, which is `wasm32`-gated and unavailable off-wasm.)
- **`tools/gen_authoring_guide.py`** — the committed authoring-guide generator (the durable form of
  the spike's throwaway `gen_guide.py`). Projects the construct catalog, required fields, distribution
  catalog, `failure_process` sub-schema, function reference, and engine-native AST. Sources the
  registries (committed JSON) verbatim and carries the engine-derived schema tables inline.
- **`engine/tests/authoring_guide_schemas.rs`** — the drift guard: round-trips one minimal model per
  distribution family + per required-field claim + the `failure_process` recipe through the real
  parser, so the guide's schema tables can never drift from what the engine accepts.
- **`scratchpad/spike/`** (not committed) — the exemplar, harness (`spike.sh`), and run transcripts.

## Runs

| Prompt | Domain | Iterations to valid | Model correct? |
|---|---|---|---|
| Two-tank overflow into a creek | stock-flow | **2** | ✅ mass-conserving at equilibrium |
| 30-yr retirement, N(μ,σ) returns, fixed withdrawal | stochastic finance | **6** | ✅ right-skewed, sequence-of-returns ruin visible |
| ↳ same prompt, **after** adding distribution + field schemas to the guide | stochastic finance | **1** | ✅ same result, valid on first attempt |
| Repairable pump, exponential MTTF + repair | reliability (event FSM) | **4** | ✅ availability 0.9375 vs theoretical 0.926 |

The reliability run (against the committed guide) surfaced the *next* nested sub-schema gap —
`failure_process` (`basis` values + the `repair` sub-structure) wasn't spelled out, costing
iterations. It has since been added to the generator (and drift-guarded). The recurring pattern is
clear: **nested sub-schemas** (distribution params, then `failure_process`) are where the LLM stalls,
and each is a concrete, engine-sourced block to add to the guide.

The stock-flow model (well-documented constructs) converged in 2. The retirement model spent 5 of
its 6 iterations chasing the **distribution parameter schema**, which the registries do not describe.

**The controlled follow-up nails the lever.** After extending the guide with the distribution
catalog (families + parameter names, sourced from the engine's `DistributionKind`) and per-construct
required-field schemas, the *identical* retirement prompt validated **on the first attempt — 6 → 1
iterations**. Nothing about the model or the loop changed; only the context. This is direct evidence
that convergence cost is context quality, and that the distribution/field schemas are the missing
piece — not a smarter model or a better loop.

## Findings (in priority order)

1. **The registries alone are NOT sufficient copilot context — and fixing that is the whole game.**
   The element registry lists *that* a `sample` construct exists, but not its **nested parameter
   schema** (`{family, parameters:{mean, stddev}}`, each param a `{value, unit}` quantity). The LLM
   had to reverse-engineer it from validator errors — 5 wasted iterations. Adding the distribution
   catalog + required-field schemas to the guide dropped the same prompt to **1 iteration** (see the
   runs table). §17.2's "authoring guide" must project **more than the registry**: the distribution
   catalog, the per-construct required fields, and the engine-native AST. This is the single biggest,
   and now measured, lever on convergence speed. The schemas exist authoritatively in the engine
   (`engine/src/model.rs` `DistributionKind`; the `.ok_or_else(missing(...))` checks in
   `v2_parse.rs`) — a generator should project from there.

2. **The engine-native AST ≠ the frontend AST — and it's a trap.** The engine parses `{op:'ref',
   element_id:...}` and `{op:'call', fn:...}`; the frontend authors/prints `reference` and (my guide
   wrongly said) `function`. Both spikes hit this. Whatever generates the copilot's guide must
   project from the **engine** shapes (`model.rs` AST variants), not the frontend's `ast.ts`. My
   guide had the `fn`/`function` bug — the LLM caught it, which is the loop working as intended.

3. **The validate-loop works and is cheap insurance.** Precise serde errors ("unknown variant
   `reference`, expected `ref`…", "missing field `element_id`") are directly actionable — the LLM
   fixed each in one step. This confirms §17's core thesis: the LLM doesn't need to have memorized
   the schema; the engine catches and the LLM corrects in-loop. Convergence was monotonic (no
   thrashing) in both runs.

4. **The LLM reports honest expressiveness gaps.** For the two-tank model it noted a true within-step
   hard cap (instantaneous spillway) isn't expressible with the current constructs, and chose the
   faithful rate-based approximation rather than faking it. That's the behavior §17.6 wants (the
   copilot cannot exceed the engine) — and it surfaces real engine feature gaps as a side effect.

## Implications for building §17

- **Do build it — the bet holds.** An LLM + this validate loop produces correct models today, with a
  single small exemplar and a partial guide.
- **Invest in the authoring-guide projection, not the loop.** The loop is nearly free (Claude Code
  already does it; the browser copilot reuses the reconcile loop from §13.2). The convergence cost is
  entirely in *context quality*. Before §17 UI work, extend the guide projection to include: the
  distribution catalog with parameter schemas, per-construct required-field schemas, and the
  engine-native AST reference. Source these from the engine (`model.rs`, the distribution enum), not
  the frontend.
- **`wasim-validate` is worth keeping.** It is the concrete `validate()`/`run()` tool for any
  copilot, browser or CLI, and it's a useful standalone lint for hand-authored models and CI.
- **Scope note:** this validated *feasibility and the loop*, not §17's shipped architecture (browser
  SPA, BYO-key, provider abstraction, Copilot panel UX). Those are still to design/build; nothing
  here shortcuts them.

## Open measurement — adaptive thinking on/off (deferred; toggle removed with the provider abstraction)

The copilot briefly had a "use extended reasoning" (adaptive thinking) toggle. The provider
abstraction (§17.1) adopted a **lowest-common-denominator interface** — `chat()` carries only what
every provider shares — so the Anthropic-only thinking knob was **removed** from the config and UI.
The `tools/copilot_thinking_ab.mjs` harness stays committed (a standalone Anthropic experiment) but
no longer feeds a UI default. If thinking is re-introduced later, it belongs *inside* the Anthropic
adapter (read from an Anthropic-specific config field), not in the neutral interface — and its
default should still be set by the A/B, not asserted.

The unanswered question is unchanged: the spike ran through `claude -p` (thinking on by default) and
never isolated thinking-on vs -off on the raw API. To settle it, run the harness (needs
`ANTHROPIC_API_KEY` + a built `wasim-validate`) — it drives the real loop over 3 prompts ×
{thinking off, on} and reports validate-iterations to convergence.
