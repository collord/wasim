#!/usr/bin/env python3
"""Generate the WASiM LLM-authoring guide (WASIM_AUTHORING_ENVIRONMENT_SPEC.md §17.2).

The guide is the schema context an LLM copilot needs to draft engine-native model JSON: the
construct catalog, the required fields per construct, the distribution catalog, the expression
function reference, and the engine-native AST. The feasibility spike (LLM_AUTHORING_SPIKE_FINDINGS.md)
showed convergence cost is dominated by this context's completeness — adding the distribution + field
schemas dropped a test prompt from 6 validate-iterations to 1.

Sources, in order of authority:
  * `frontend/src/lenses/element-registry.json` + `functions.json` — the committed registries
    (already drift-checked against the engine by frontend tests). Projected verbatim.
  * The distribution / required-field / AST tables below — hand-authored from the engine
    (`engine/src/model.rs` `DistributionKind` + `AstNode`; the `.ok_or_else(missing(...))` checks in
    `engine/src/v2_parse.rs`). Drift-guarded by `engine/tests/authoring_guide_schemas.rs`, which
    round-trips one minimal model per distribution family / construct through the real parser.

Usage:
  python3 tools/gen_authoring_guide.py [out.md]     # default: prints to stdout
"""
import json
import os
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
REG = json.load(open(f"{ROOT}/frontend/src/lenses/element-registry.json"))
FNS = json.load(open(f"{ROOT}/frontend/src/model/functions.json"))

# ── Engine-sourced tables (drift-guarded by engine/tests/authoring_guide_schemas.rs) ──────────────

# Distribution families → parameter names. Each param is a {value,unit} Quantity unless the note
# says otherwise. Source: engine/src/model.rs `DistributionKind` (#[serde(tag="family",
# content="parameters")]).
DISTRIBUTIONS = [
    ("uniform", "min, max"),
    ("normal", "mean, stddev"),
    ("lognormal", "mean, stddev"),
    ("lognormal_moments", "mean, stddev (moments of the values, not the logs)"),
    ("triangular", "min, mode, max"),
    ("trapezoidal", "min, lower, upper, max"),
    ("pert", "min, mode, max"),
    ("exponential", "mean"),
    ("gamma", "shape, scale"),
    ("beta", "alpha, beta (optional min, max to rescale)"),
    ("weibull", "shape, scale"),
    ("pareto", "scale, shape (optional location)"),
    ("extreme_value", "location, scale"),
    ("student_t", "degrees_of_freedom (optional location, scale)"),
    ("pearson_iii", "mean, stddev, skewness"),
    ("discrete_uniform", "min, max — PLAIN INTEGERS, not Quantities"),
    ("bernoulli", "prob — a plain Quantity in 0..1"),
    ("binomial", "n, prob"),
    ("negative_binomial", "r, prob"),
    ("poisson", "lambda"),
    ("discrete", "outcomes:[numbers], probabilities:[numbers]"),
    ("sampled", "samples:[numbers] (optional weights:[numbers])"),
    ("log_uniform", "min, max"),
    ("log_triangular", "min, mode, max"),
    ("triangular1090", "p10, mode, p90"),
    ("cumulative", "points:[{x, cumulative_probability}]"),
]

# Required fields per construct. Source: the `.ok_or_else(missing(...))` checks in
# engine/src/v2_parse.rs `lower_*`. Fields not listed are optional.
REQUIRED_FIELDS = [
    ("stock", "`initial_value` (Quantity)", "`inflows`/`outflows` (arrays of flow element ids), `rate` (net-rate — mutually exclusive with flows), `capacity`, `floor`, `return_rate`"),
    ("node/fixed (Constant)", "`value` (Quantity) OR `values` (number array)", "`editable`, `bounds:{min,max}`, `unit`"),
    ("node/sample (Stochastic)", "`distribution` (see the distribution catalog)", "`resampling`, `autocorrelation`, `correlations`"),
    ("node/expression", "`expression:{ast}`", "`inputs:[ids]` — set to the referenced element ids"),
    ("node/lookup", "`table:{x:[...], y:[...]}`", "`interpolation`"),
    ("node/series (Time Series)", "`timestamps:[...]` AND `values:[...]`", "`time_unit`, `interpolation`"),
    ("node/lag (Previous Value)", "none", "`input:'id'`, `initial:{value,unit}`"),
    ("node/filter", "`statistic` (mean|min|max|sum|ema)", "`input:'id'`, `window` (0 = expanding)"),
    ("node/pid", "`input:'id'` AND `setpoint` (Quantity)", "`kp`, `ki`, `kd`, `output_min`, `output_max`"),
    ("event", "none strictly; a repairable-component event sets `failure_process` (see its sub-schema below)", "`trigger`, `effects:[{target, change, mode}]`, `event_value`, `count_limit`, `rate`"),
    ("gate", "`root` (op-tagged gate tree: and|or|not|n_vote|reference|condition|input)", "`semantics` (success|failure, default success)"),
]

# Engine-native AST ops. Source: engine/src/model.rs `AstNode` (#[serde(tag="op")]).
AST_OPS = [
    ("literal", "`{op:'literal', value:N, unit?:'…'}`"),
    ("ref", "`{op:'ref', element_id:'other_id'}` — the field is `element_id`, NOT `reference`"),
    ("time_ref", "`{op:'time_ref', property:'elapsed'}`"),
    ("add / subtract / multiply / divide / power", "`{op, left, right}`"),
    ("lt / gt / lte / gte / eq / neq / and / or", "`{op, left, right}`"),
    ("neg / not", "`{op, operand}`"),
    ("call", "`{op:'call', fn:'min', args:[…]}` — the function-name field is `fn`, NOT `function`"),
    ("if", "`{op:'if', cond, then, else}`"),
    ("lookup_call", "`{op:'lookup_call', element_id:'table_id', input, input2?}`"),
    ("array", "`{op:'array', elements:[…]}`"),
]


def build():
    out = []
    out.append("# WASiM v2 model authoring guide")
    out.append("")
    out.append("_Generated by `tools/gen_authoring_guide.py` — do not edit by hand. This is the schema")
    out.append("context for LLM-assisted authoring (WASIM_AUTHORING_ENVIRONMENT_SPEC.md §17.2)._")
    out.append("")
    out.append("A model is JSON: `{ wasim_version, simulation_settings, elements: [...] }`.")
    out.append("`simulation_settings`: `{ duration:{value,unit}, timestep:{value,unit}, n_realizations, seed }`.")
    out.append("Every element has `id`, `name`, and a `primitive` (node | stock | event | gate | link | cell | species | medium | resource).")
    out.append("A `node` also carries a `value_rule`. Behavior is determined by which fields are present (traits), not a fixed type taxonomy.")
    out.append("A `Quantity` is `{value, unit}` (unit `\"1\"` = dimensionless).")
    out.append("")

    out.append("## Expression AST (engine-native)")
    out.append("")
    out.append("Expressions are an AST, NOT a string. A `value_rule: expression` node carries")
    out.append("`{ expression: { ast: <node> }, inputs: [referenced element ids] }`. AST node ops:")
    out.append("")
    for op, shape in AST_OPS:
        out.append(f"- `{op}` — {shape}")
    out.append("")

    out.append("## Constructs")
    out.append("")
    by = {}
    for e in REG["entries"]:
        by.setdefault(e["section"], []).append(e)
    for s in REG["sections"]:
        out.append(f"### {s['label']}")
        for e in by.get(s["id"], []):
            out.append(f"- **{e['label']}** → `primitive`/`value_rule` = `{e['construct']}` ({e['editor']}). {e.get('doc','')}")
        out.append("")

    out.append("## Required fields per construct")
    out.append("")
    out.append("The v2 parser rejects a model missing these. Fields not listed are optional.")
    out.append("")
    for name, req, opt in REQUIRED_FIELDS:
        out.append(f"- **{name}**: {req}." + (f" Optional: {opt}." if opt and opt != "none" else ""))
    out.append("")

    out.append("### `failure_process` sub-schema (repairable-component events)")
    out.append("")
    out.append("`{ \"basis\": <basis>, \"time_to_failure\": <distribution>, \"repair\": { \"policy\": <policy>, \"time_to_repair\": <distribution> } }`.")
    out.append("- `basis` (required): `exposure_time` | `operating_time` | `demand` | `capacity_demand` | `event` | `condition`.")
    out.append("- `time_to_failure`: a distribution object (same shape as the distribution catalog below), e.g. exponential with `mean` = MTTF.")
    out.append("- `repair.policy`: `repair` | `replace` | `preventive_maintenance` | `none` (default). `repair.time_to_repair`: a distribution (MTTR).")
    out.append("")
    out.append("## Distribution catalog (the `distribution` field of a `node/sample`)")
    out.append("")
    out.append("Shape: `{ \"family\": <name>, \"parameters\": { <param>: <Quantity>, … }, \"truncation\":{min,max}? }`.")
    out.append("Each parameter is a `{value, unit}` Quantity unless noted. Families and their parameters:")
    out.append("")
    for fam, params in DISTRIBUTIONS:
        out.append(f"- `{fam}` — {params}")
    out.append("")
    out.append("Example: `{\"id\":\"ret\",\"name\":\"Return\",\"primitive\":\"node\",\"value_rule\":\"sample\","
               "\"distribution\":{\"family\":\"normal\",\"parameters\":{\"mean\":{\"value\":0.06,\"unit\":\"1\"},"
               "\"stddev\":{\"value\":0.1,\"unit\":\"1\"}}}}`")
    out.append("")

    out.append("## Expression functions (`{op:'call', fn:NAME, args:[…]}`)")
    out.append("")
    for f in FNS["functions"]:
        out.append(f"- `{f['name']}` — {f.get('doc','')} e.g. `{f.get('example','')}`")
    out.append("")
    out.append("## Time references (`{op:'time_ref', property:NAME}`)")
    out.append("")
    for t in FNS["timeRefs"]:
        out.append(f"- `{t['name']}` — {t.get('doc','')}")
    out.append("")

    return "\n".join(out)


# The committed browser-bundle copy of the guide. The LLM-authoring copilot imports this via
# Vite `?raw` (it can't shell out to this script), so regenerate it whenever the registries change:
#   python3 tools/gen_authoring_guide.py --frontend
# (Content is drift-guarded against the engine by engine/tests/authoring_guide_schemas.rs.)
FRONTEND_GUIDE = f"{ROOT}/frontend/src/copilot/authoring-guide.generated.md"

if __name__ == "__main__":
    text = build()
    if "--frontend" in sys.argv[1:]:
        open(FRONTEND_GUIDE, "w").write(text)
        print(f"wrote {FRONTEND_GUIDE}: {len(text)} chars", file=sys.stderr)
    elif len(sys.argv) > 1:
        open(sys.argv[1], "w").write(text)
        print(f"wrote {sys.argv[1]}: {len(text)} chars", file=sys.stderr)
    else:
        print(text)
