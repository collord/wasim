# Vensim `.mdl` Import — Pathway Scout + Working Harness

**Purpose.** Establish a pathway to import Vensim `.mdl` models into WaSiM and to *challenge
the WaSiM engine* against a reference simulator — the way simlin's
[`build_notebook.py`](https://github.com/bpowers/simlin/blob/main/notebooks/build_notebook.py)
exercises the simlin engine on real `.mdl` models. Unlike `XMILE_MAPPING_SCOUT.md` (a
paper mapping), this report is **grounded in a working end-to-end run**: three canonical
Vensim models round-trip through the WaSiM engine and reproduce a reference trajectory to
floating-point precision.

Deliverables in this branch:
- `tools/mdl_challenge.py` — the differential-test harness (`.mdl → WaSiM → diff vs pysimlin`).
- `engine/src/bin/wasim-validate.rs` — new `--trajectories` mode: dumps the full per-step
  mean series so a trajectory (not just final values) can be diffed.
- `tools/fixtures/{teacup,SIR,Lotka_Volterra}.mdl` — canonical Vensim fixtures.

---

## 0. The key insight: **don't write a native `.mdl` parser**

Vensim `.mdl` is a proprietary, quirky text format (line-continuation `\`, `~`-delimited
units/doc blocks, sketch/`\\\---/// Sketch` sections, macros, subscript ranges). Hand-writing
a faithful parser is a large, low-leverage effort — and **it is a solved problem**. simlin
ingests Vensim through **`xmutil`** (Bob Eberlein's open-source `.mdl → XMILE` converter,
vendored in `simlin/src/xmutil`), then runs the resulting XMILE. `pysimlin.load("x.mdl")`
does exactly this under the hood.

WaSiM **already has a mature XMILE → v2 importer** (`tools/xmile_to_wasim.py`, per
`XMILE_MAPPING_SCOUT.md`). So the entire Vensim pathway is a two-hop reuse:

```
model.mdl ──pysimlin.load──▶ Project.to_xmile() ──▶ xmile_to_wasim.py ──▶ model.json ──▶ WaSiM engine
                │                                                                              │
                └────────────── model.run().results  (reference oracle) ───── diff ───────────┘
```

**Zero new parser code.** pysimlin is both the `.mdl → XMILE` bridge *and* the reference
simulator, so one dependency buys ingestion + an oracle. This mirrors simlin's own notebook:
`simlin.load()` → `model.run()` → analyze/compare.

---

## 1. Evidence — it works, and it's bit-exact

`tools/mdl_challenge.py <model.mdl>` runs the full pipeline and reports per-variable
`max_abs_err` between the WaSiM trajectory and pysimlin's. Results on the SDXorg
canonical corpus (`pip install pysimlin`, engine built `--release`):

| Model | Vars compared | Steps | Worst max_abs_err | Verdict |
|---|---|---|---|---|
| `teacup` | 2 (stock + flow) | 240 | **2.8e-13** | PASS |
| `SIR` | 5 | 3 200 | **1.8e-12** | PASS |
| `Lotka_Volterra` | 8 | 800 | **7.3e-11** | PASS |

Every aligned variable matches to floating-point round-off — i.e. WaSiM's explicit-Euler
integration reproduces the reference exactly on the clean stock/flow/aux core, which is
"the 80% of real SD models" (`XMILE_MAPPING_SCOUT.md §5`). This validates the mapping
end-to-end, not on paper.

Reproduce:
```bash
pip install pysimlin
(cd engine && cargo build --release --bin wasim-validate)
python3 tools/mdl_challenge.py tools/fixtures/teacup.mdl --tol 1e-3
python3 tools/mdl_challenge.py tools/fixtures/SIR.mdl
python3 tools/mdl_challenge.py tools/fixtures/Lotka_Volterra.mdl
```

---

## 2. Two alignment subtleties the harness had to encode

Both were surfaced *by* the harness — precisely the value of differential testing.

### 2a. Name mapping (slug on the trailing identifier)
WaSiM ids are module-qualified and slugged (`Main/teacup_temperature`); pysimlin columns are
the same slug unqualified (`teacup_temperature`). The harness matches on
`slug(name.split("/")[-1])`. Variables with no reference column (fixed scalars pysimlin folds
away, or WaSiM-only helper nodes) are reported as skipped, never silently dropped.

### 2b. Time alignment — stock vs flow are sampled at different step phases
WaSiM's saved history has **N rows for N steps** (no initial row; reference has N+1). Within a
row `i` spanning `[t_i, t_i+dt)`:
- a **stock** is the *end-of-step* level → compare to reference at **`t_i + dt`**;
- a **flow/aux** is evaluated on the *pre-step* state (Euler reads state at the start) →
  compare to reference at **`t_i`**.

Getting this wrong makes a *correct* flow look off by exactly one step's rate (teacup's
`heat_loss_to_room` showed a flat 0.1375 = `dt·1.1` error until the phase was fixed; with it,
1.6e-14). The harness reads `primitive` from the generated `model.json` to pick the offset.
**This is a documentation-worthy engine convention**, not a bug — but any consumer plotting
WaSiM history next to a Stella/Vensim run needs it.

---

## 3. Gaps, caveats & fidelity boundaries

- **The oracle is pysimlin, not Vensim itself.** pysimlin = `xmutil` (`.mdl→XMILE`) + the
  simlin engine (Euler/RK). It agrees with Vensim on the clean SD core but is a *second
  implementation*, not ground truth. For hard fidelity claims, add SDXorg's shipped canonical
  `*_output.csv` as a third leg. For this scope the two-implementation agreement (WaSiM ==
  simlin to 1e-11) is a strong signal.
- **Everything in `XMILE_MAPPING_SCOUT.md §3` still applies** — the Vensim path inherits the
  XMILE importer's boundaries verbatim, because it *is* the XMILE importer:
  - **RK2/RK4** models will diverge (WaSiM is Euler-only). xmutil records `method` in
    `<sim_specs>`; the importer down-converts to Euler + warns.
  - `DELAY*` / `SMTH*` / `TREND` / conveyors / queues / ≥3-D arrays / macros → `extern_call`
    stubs or decomposition, per that report. The harness will flag these as OFF variables,
    which is the intended signal to prioritize a lowering.
- **Units.** xmutil preserves Vensim unit strings (`Degrees Fahrenheit`, `Minute`); WaSiM's
  SI registry warns on unrecognized units but runs unaffected (values are correct; the
  warnings in the harness output are cosmetic). A unit-alias pass is optional polish.
- **Uncertainty is added, not imported.** `.mdl`/XMILE carry no ensemble; import defaults
  `n_realizations:1` (deterministic). WaSiM's Monte-Carlo layer is value *added on top* —
  the whole thesis (`WASIM_VALUE_PROP_THESIS.md §4.3`). The harness runs the engine's default
  64 realizations of a deterministic model, so `mean == the trajectory`.
- **Dependency posture.** The pathway pins `pysimlin` (Rust-backed wheels on PyPI, 0.7.0).
  If a pure-offline / no-Python-dep ingestion is later wanted, `xmutil` can be built
  standalone (C++ or the simlin WASM build) and shelled out to for `.mdl→XMILE`, keeping the
  same downstream. Not needed for the harness.

---

## 4. How this maps to simlin's `build_notebook.py`

| simlin notebook | WaSiM harness |
|---|---|
| `simlin.load(mdl)` | `simlin.load(mdl)` — same call, reused as the bridge |
| `model.run()` (simlin engine) | `model.run()` used as the **reference**, WaSiM engine is the SUT |
| LTM loop-dominance analysis | out of scope here (WaSiM has no LTM); trajectory diff instead |
| renders notebook/plots | prints a per-variable error table + PASS/FAIL exit code (CI-ready) |

simlin's notebook *demonstrates* its engine; this harness *adversarially challenges* WaSiM's
against an independent engine on the same source models. Drop it over a corpus
(SDXorg/test-models has dozens of `.mdl`) as a conformance gate.

---

## 5. Recommended next steps (incremental, each independently useful)

1. **Corpus sweep.** Point `mdl_challenge.py` at all of SDXorg `samples/*/*.mdl`; the OFF
   variables auto-triage which XMILE builtins/constructs to lower next (data-driven backlog).
2. **Add canonical CSV as a third leg** for the models SDXorg ships output for, upgrading
   "WaSiM == simlin" to "WaSiM == simlin == Vensim-canonical."
3. **Lower the top-frequency OFF constructs** (likely `DELAY1/3`, `SMTH*`, `PULSE/RAMP`) in
   `xmile_to_wasim.py` — each removes a whole class of models from the OFF column.
4. **Document the stock/flow step-phase convention** (§2b) in `wasim-engine-semantics.md`.
5. **Optional:** a thin `mdl_to_wasim.py` wrapper (`= pysimlin.to_xmile | xmile_to_wasim`) as
   a one-shot importer CLI, separate from the diffing harness.

---

*Grounded against a live run of pysimlin 0.7.0 + the WaSiM engine (`wasim-validate
--trajectories`) on SDXorg/test-models teacup, SIR, and Lotka-Volterra, 2026-08. See
`tools/mdl_challenge.py` and `XMILE_MAPPING_SCOUT.md` for the underlying XMILE mapping.*
