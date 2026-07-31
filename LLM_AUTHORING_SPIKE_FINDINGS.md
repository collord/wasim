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
- **`scratchpad/spike/`** (not committed) — `gen_guide.py` (projects the element + function
  registries into a ~9KB authoring guide), `exemplar_bathtub.json` (a validated v2 exemplar),
  `spike.sh` (the harness), and `runs/` (transcripts + resulting models).

## Runs

| Prompt | Domain | Iterations to valid | Model correct? |
|---|---|---|---|
| Two-tank overflow into a creek | stock-flow | **2** | ✅ mass-conserving at equilibrium |
| 30-yr retirement, N(μ,σ) returns, fixed withdrawal | stochastic finance | **6** | ✅ right-skewed, sequence-of-returns ruin visible |

The stock-flow model (well-documented constructs) converged in 2. The retirement model spent 5 of
its 6 iterations chasing the **distribution parameter schema**, which the registries do not describe.

## Findings (in priority order)

1. **The registries alone are NOT sufficient copilot context.** The element registry lists *that* a
   `sample` construct exists, but not its **nested parameter schema** (`{family, parameters:{mean,
   stddev}}`, each param a `{value, unit}` quantity). The LLM had to reverse-engineer it from
   validator errors — 5 wasted iterations. §17.2's "authoring guide" must project **more than the
   registry**: at minimum the distribution catalog and the per-construct field schemas. This is the
   single biggest lever on convergence speed.

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
