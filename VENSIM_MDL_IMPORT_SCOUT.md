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

## 4a. Corpus sweep results — SDXorg/test-models (154 `.mdl`)

Ran `tools/mdl_corpus_sweep.py` over the full SDXorg corpus (154 `.mdl`, samples + the
Vensim conformance `tests/`). The raw counts are diluted by two buckets that say nothing
about the WaSiM engine, so read them layered, not as one PASS rate:

| Bucket | Count | What it means |
|---|---:|---|
| **Not benchmarkable — upstream** | **39** | pysimlin/xmutil *itself* can't load (17) or simulate (22) the model. The reference oracle doesn't exist, so WaSiM can't be scored. Not a WaSiM signal. |
| **Not measured — harness** | **43** | WaSiM ran, but the harness couldn't align names: **39 array/subscript** models (dimensioned member ids vs `name[sub]` columns) + **4 constant-only** models (no saved trajectory). A measurement gap, not an engine gap. |
| **Measurable dynamic-scalar** | **72** | the models where the comparison is meaningful ↓ |
| &nbsp;&nbsp;• **PASS** (≤1e-2, mostly ≤1e-11) | **50** | WaSiM reproduces pysimlin to floating-point precision |
| &nbsp;&nbsp;• **FAIL** (ran, drifted) | **21** | root-caused to a short list below |
| &nbsp;&nbsp;• **WaSiM engine reject** | **1** | `test_active_initial_circular` — see backlog #3 |

**On the 72 measurable models, WaSiM matches an independent engine on 50 = 69%** (was 44/61%
before the builtin work below). The remaining 21 misses are not 21 different problems — they
collapse to a **short importer-construct list**, all in `xmile_to_wasim.py` (the engine is not
implicated except where noted):

| # | Construct (Vensim/XMILE builtin) | FAIL models | Effort | Note |
|---|---|---:|---|---|
| 1 | **`<macro>`** user functions (`EXPRESSION_MACRO`/`SECOND_MACRO`) | 6 | hard | `test_macro_*`; inline-expand simple single-`<eqn>` macros (scout §2). Now the biggest single lever. |
| 2 | **Delay/smooth family** `DELAY FIXED`/`DELAY1`/`SMTH3`/`TREND` | 4 | medium | `test_delay_fixed` (worst 4.7e6), `test_smooth_and_stock`, `test_delay_parentheses`, `test_trend` (already 0.084 — nearly passing). Each is a macro-expansion into stocks/EMAs (scout §2). |
| 3 | **`INIT` / `ACTIVE INITIAL` / `RAMP`** | 3 | medium | `test_active_initial{,_circular}`, `test_initial`, `test_inputs`. `ACTIVE INITIAL(active,init)` breaks a t=0 init loop — unmapped, so the circular one is the lone **engine reject** (WaSiM correctly refuses the unbroken cycle). Mapping it fixes both the drift and the reject. |
| — | array/subscript semantics | ~4 | (see harness note) | `test_elm_count` (`ELMCOUNT`), `test_except`, `test_subscript_definition`, `test_repeated_subscript` — array-side, tied to `XMILE_MAPPING_SCOUT.md §1`. |
| — | `SAMPLE IF TRUE`, `Single_Pendulum` | 2 | mixed | `sample_if_true` = per-step sample-and-hold (unmapped). `Single_Pendulum` (worst 1e3) is likely genuine **Euler-vs-RK numeric divergence**, not a builtin gap — SDXorg canonical CSV would confirm. |

**✅ Done (this branch):** `LOOKUP` (+ the bare `<aux><gf/></aux>` no-`<eqn>` table shape it
calls), safe-divide `ZIDZ`/`XIDZ`/`SAFEDIV`, and `INTEGER`/`MODULO`. Six models flipped
FAIL→PASS with zero regressions: `workforce` (one `LOOKUP`→0 was poisoning all 15 downstream
vars), `test_lookups`, `test_lookups_without_range`, `test_subscripted_lookups`,
`test_subscripted_xidz`, `xidz_zidz`.

**⚠️ Oracle divergence, not a WaSiM bug — `test_rounding`.** The sweep lists it as FAIL, but
the SDXorg **canonical Vensim output** (`output.tab`) shows WaSiM is *correct* and the pysimlin
oracle is the one that's wrong: Vensim `INT` truncates toward zero (`INT(-9.9) = -9`) and `MOD`
is C-style (`-10 MOD 3 = -1`) — exactly what WaSiM produces — whereas simlin floors both
(`-10`, `2`). Left as-is; this is the concrete case that motivates adding the canonical CSV as
a third comparison leg (§5 step 6), and a live demonstration that "== simlin" ≠ "== Vensim".

Reproduce: `python3 tools/mdl_corpus_sweep.py <clone-of-SDXorg/test-models>`.

**Top harness improvement (unblocks the biggest unmeasured bucket):** teach `mdl_challenge.py`
to align WaSiM dimensioned output (array members) to the reference's `name[subscript]` columns.
That alone makes **39 array-conformance models** measurable — currently the largest blind spot,
and exactly where the `XMILE_MAPPING_SCOUT.md §1` array-lowering work needs a scoreboard.

## 5. Recommended next steps (incremental, each independently useful — now data-driven)

Ordered by leverage per §4a:

1. ~~**`LOOKUP` + safe-divide + rounding**~~ — **done this branch** (+6 models, 61%→69%). See §4a
   "Done". `LOOKUP` de-poisoned `workforce`; `INTEGER`/`MODULO` were already Vensim-correct
   (the `test_rounding` FAIL is an oracle bug, not ours).
2. **`ACTIVE INITIAL` / `INIT` / `RAMP`** (backlog #3). Fixes the drift *and* the one
   engine-reject (`test_active_initial_circular`), since mapping `ACTIVE INITIAL` breaks the
   init cycle. Next easiest win.
3. **Delay/smooth family** `DELAY FIXED`/`DELAY1`/`SMTH3`/`TREND` (backlog #2). Macro-expansions
   into stocks/EMAs per `XMILE_MAPPING_SCOUT.md §2`; `test_trend` is already at 0.084.
4. **Array-member alignment in the harness.** Unblocks the 39 unmeasured array models — the
   largest blind spot — and gives the array-lowering work (`XMILE_MAPPING_SCOUT.md §1`) a
   scoreboard. Higher effort than 1–3 but highest coverage payoff.
5. **`<macro>` inlining** (#1). Hardest; 6 models. Inline single-`<eqn>` macros first.
6. **Add SDXorg canonical CSV as a third leg** for models that ship output — upgrades
   "WaSiM == simlin" to "WaSiM == simlin == Vensim-canonical" and separates true numeric
   divergence (e.g. `Single_Pendulum`, Euler-vs-RK) from importer gaps.
7. **Document the stock/flow step-phase convention** (§2b) in `wasim-engine-semantics.md`.
8. **Optional:** a thin `mdl_to_wasim.py` one-shot importer CLI (`= pysimlin.to_xmile |
   xmile_to_wasim`), separate from the diffing harness.

---

*Grounded against a live run of pysimlin 0.7.0 + the WaSiM engine (`wasim-validate
--trajectories`) over the full SDXorg/test-models corpus (154 `.mdl`), 2026-08. §4a numbers
reproduce via `tools/mdl_corpus_sweep.py`. See `tools/mdl_challenge.py` and
`XMILE_MAPPING_SCOUT.md` for the underlying XMILE mapping.*
