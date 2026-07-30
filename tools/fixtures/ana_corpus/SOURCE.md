# Real Analytica `.ana` corpus

A collection of **real** Analytica models (not synthetic) used to exercise the
Analytica reader / converter `tools/ana_to_wasim.py` against models written by
other people, in different Analytica versions, with real-world encodings and
constructs. Complements the two hand-written `tools/fixtures/*_synthetic.ana`
probes and the worked examples in `tools/examples/`.

Each file was fetched from a public GitHub repository at a pinned commit. The
`.ana` format is plain UTF-8/ASCII text; files are committed verbatim (original
line endings preserved — some are classic CRLF, which the reader normalizes).

`tools/test_ana_corpus.py` converts every file here and asserts the converter
parses it without crashing; where `jsonschema` is installed it also checks the
output against `oldmodel.schema.json`.

## Files

| file | source repo (pinned commit) | license | Analytica ver | exercises |
|---|---|---|---|---|
| `htma_hubbard_seiersen_ch3.ana` | [QuantinCS/CRQ](https://github.com/QuantinCS/CRQ) `@ca96a2b` | none stated | 6.3.6 | `Table(Index)(…)` data, `Probability`, cyber-risk quantification (Hubbard & Seiersen, *How to Measure Anything in Cybersecurity Risk*, ch. 3) |
| `iamamr_amr_framework.ana` | [iAM-AMR/iAM.AMR.MODEL.FRAMEWORK](https://github.com/iAM-AMR/iAM.AMR.MODEL.FRAMEWORK) `@e88f327` | none stated | 6.4.8 | user `Function`s (skipped), `~` line-wrap continuation markers, label `Index` definitions |
| `iamamr_amr_food_template.ana` | [iAM-AMR/iAM.AMR.MODEL](https://github.com/iAM-AMR/iAM.AMR.MODEL) `@a3cd670` | none stated | 6.4.8 | array/subscript-heavy antimicrobial-resistance model; emits `null()` empty cells (pinned the `minItems` schema bug — see below) |
| `sa_vehicle_parc_calibration.ana` | [brunomerven/SAVehicleParcModel](https://github.com/brunomerven/SAVehicleParcModel) `@1253808` | none stated | 6.4.8 | large `Time`-axis / time-series model, `x[Dim=idx]` reindexing → `gather` (pinned the missing-builtin schema bug — see below) |
| `rent_vs_buy_cz.ana` | [QuantinCS/CRQ](https://github.com/QuantinCS/CRQ) `@ca96a2b` | none stated | 6.4.8 | `Dynamic()` recurrence, non-ASCII (Czech) titles/descriptions — Unicode robustness |
| `drunk_driving_cost_benefit.ana` | [masirbu/AIDP-ANPRM](https://github.com/masirbu/AIDP-ANPRM) `@3bf66da` | **GPL-3.0** | 6.4.8 | largest model here (121 elements): cost–benefit decision analysis with CRLF line endings |

### License note

Most source repositories state **no license**, so these files are included as
small third-party test fixtures under fair-use for interoperability testing,
with provenance recorded above — the same posture as the pre-existing real
Lumina examples in `tools/examples/`. `drunk_driving_cost_benefit.ana` is from a
**GPL-3.0** repository; it is retained verbatim as an unmodified test input (mere
aggregation — it is data fed to the converter, not linked into any WASiM
program). If any rights-holder objects, drop the offending file and the
corresponding row here; the test harness tolerates a missing file.

## Converter findings surfaced by this corpus (fixed)

Two real models exposed a **schema/engine drift bug**: the converter and the
engine (`wasim_engine::simulate_json`) both supported these ops, but
`oldmodel.schema.json` — which the converter output is supposed to always
validate against — was stale and rejected them. Both are now fixed by syncing
the schema's `call` node with the engine's builtins:

1. **`sa_vehicle_parc_calibration.ana`** → emits `{"op":"call","fn":"gather",…}`
   (from `x[Dim=idx]` reindexing). `gather` — and the other array-language
   builtins the converter/engine already implement (`ordinal`, `sort_array`,
   `sort_index`, `rank_array`, `cumulate`, `cumproduct`) — were missing from the
   schema's `fn` enum and have been added.
2. **`iamamr_amr_food_template.ana`** → emits the nullary `null()` call
   (`args: []`, → NaN), which the schema's blanket `minItems: 1` on call
   arguments wrongly rejected. The `call` node now allows exactly-zero args for
   `null` and still requires ≥1 for every other builtin.

`tools/test_ana_corpus.py` keeps an `EXPECTED_INVALID` set (now empty) as a live
regression tracker: if any fixture converts to schema-invalid output again, the
test fails and names it.

## Findings surfaced by *executing* the corpus (fixed)

Schema validation is static — it never runs the converted model. The Rust
integration test `engine/tests/ana_corpus_runs_v2.rs` closes that gap: it
converts every fixture and runs it through `wasim_engine::simulate_json`,
asserting each executes without error. That executable-fidelity check caught a
defect schema validation could not:

1. **`drunk_driving_cost_benefit.ana`** → uses Analytica's two-arg
   `Round(x, digits)` (round to `digits` decimal places). The engine's `round`
   builtin was 1-arg-only, so the run failed with
   `function 'round' expects 1–1 args, got 2`. `round` now accepts 1 arg (round
   to integer) or 2 args (round to `digits` decimals, negative for tens/…), on
   both the scalar lane (`eval.rs`) and the fused array lane
   (`array_lane.rs`'s `Op::RoundN`). Pinned by `round_two_arg_*` in
   `engine/tests/array_lane_v2.rs`.
