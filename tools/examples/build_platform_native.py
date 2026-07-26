"""Generate platform_decommissioning_native.wasim.json.

A WASiM-native distillation of the PLATFORM decommissioning decision tool
(tools/examples/Platform_2017.ana): pick a decommissioning option for an
offshore oil & gas platform by multi-attribute value under cost uncertainty.

Native pattern (mirrors the EVIU example and loss_exceedance_curve):
  * a `Decision_MC` submodel runs the multi-attribute scoring per realization —
    an uncertain cost multiplier (the real model's `Cost_uncertainty ~
    Lognormal(1.12, 1.23)`) flows into a min-max value function, combined with
    fixed attribute scores by swing weights into a multi-attribute score per
    option (a `vector_map` over the Option axis, then `argmax_array` to pick);
  * the submodel exposes SCALAR outputs (per-option score, chosen index, chosen
    cost) — because `submodel_stat` reduces scalars, not per-member vectors;
  * the parent reduces those across realizations natively: `mean` for expected
    scores / cost, `cumulative_prob`/`exceedance` differences for the
    probability each option is preferred, `exceedance` for cost-overrun risk,
    and `argmax_array` over the three expected scores for the recommendation.

Everything the real model does with Probability()/ranking over the Run sample is
here done in-graph with the engine's own reducers.
"""
import json, math, sys

OPTIONS = ["Full removal", "Reef (partial)", "Leave in place"]
ATTRS = ["Cost", "Reef habitat", "Access", "Air quality"]
N_OPT = len(OPTIONS)

# Fixed attribute data by option (illustrative, ordered as OPTIONS). Tuned so no
# option dominates — a genuine contest, mostly Reef vs Leave.
COST_RAW = [180.0, 130.0, 90.0]     # $M base capital cost (per-option uncertainty applied)
REEF     = [25.0, 90.0, 65.0]       # reef-habitat benefit (per-option uncertainty applied)
ACCESS   = [90.0, 55.0, 20.0]       # navigation/access benefit (full removal best)
AIR      = [30.0, 65.0, 95.0]       # air-quality score (no-removal best)
WEIGHTS  = {"cost": 0.30, "reef": 0.25, "access": 0.20, "air": 0.25}
BUDGET   = 200.0                     # $M cost-overrun threshold

# Per-option uncertainty. Independent cost draws (each platform/option carries its
# own cost risk) and independent reef-biomass estimate uncertainty — mirroring the
# real model's per-platform Cost_uncertainty and Biomass Lognormal nodes. This is
# what makes the *ranking itself* uncertain, so the preference probabilities are
# a real distribution rather than a foregone conclusion.
COST_GSDEV = 1.30                    # geometric SD of the cost multiplier (~±30%)
REEF_GSDEV = 1.40                    # geometric SD of the reef-benefit multiplier

def lognormal_unit(gsdev):
    """Lognormal with median 1 and geometric SD `gsdev` (a multiplicative shock)."""
    return {"family": "lognormal", "parameters": {
        "mean":   {"value": 0.0, "unit": "1"},
        "stddev": {"value": math.log(gsdev), "unit": "1"}}}

def lit(v): return {"op": "literal", "value": float(v)}
def ref(i): return {"op": "ref", "element_id": i}
def call(fn, *a): return {"op": "call", "fn": fn, "args": list(a)}
def op(o, l, r): return {"op": o, "left": l, "right": r}
def idx(arr): return {"op": "index", "array": ref(arr),
                      "indices": [{"op": "index_ref", "axis": "row"}]}

def sstat(output, stat, arg=None):
    n = {"op": "submodel_stat", "submodel_id": "Model/Decision_MC",
         "output": f"Model/Decision_MC/{output}", "statistic": stat}
    if arg is not None:
        n["arg"] = arg
    return n

def minmax_score(vec_id, higher_better=True):
    """0..100 value function over the Option axis via max_array/min_array."""
    hi = call("max_array", ref(vec_id))
    lo = call("min_array", ref(vec_id))
    span = op("subtract", hi, lo)
    if higher_better:
        num = op("subtract", idx(vec_id), lo)     # (x - min)/(max - min)
    else:
        num = op("subtract", hi, idx(vec_id))     # (max - x)/(max - min)  (lower better)
    return op("multiply", lit(100.0), op("divide", num, span))

SETTINGS = {"duration": {"value": 1, "unit": "1"}, "timestep": {"value": 1, "unit": "1"},
            "n_realizations": 4000, "seed": 20250725}

# ── multi-attribute score per option (inside the submodel) ────────────────────
# score = Σ_attr weight · valuefn(attr)   (Cost uses the uncertain cost vector)
multi_score_body = op("add",
    op("add",
       op("multiply", lit(WEIGHTS["cost"]),   minmax_score("Model/Decision_MC/cost", higher_better=False)),
       op("multiply", lit(WEIGHTS["reef"]),   minmax_score("Model/Decision_MC/reef", higher_better=True))),
    op("add",
       op("multiply", lit(WEIGHTS["access"]), minmax_score("Model/Decision_MC/access", higher_better=True)),
       op("multiply", lit(WEIGHTS["air"]),    minmax_score("Model/Decision_MC/air", higher_better=True))))

def sub_node(id_, name, rule, **kw):
    e = {"id": f"Model/Decision_MC/{id_}", "name": name, "primitive": "node",
         "value_rule": rule, "container": "Model/Decision_MC",
         "save_results": {"final_value": True}}
    e.update(kw)
    return e

def par_node(id_, name, ast, inputs, unit="1"):
    return {"id": f"Model/{id_}", "name": name, "primitive": "node",
            "value_rule": "expression", "container": "Model", "inputs": inputs,
            "expression": {"ast": ast}, "outputs": [{"name": id_, "unit": unit}],
            "save_results": {"final_value": True}}

def fixed_vec(id_, name, values):
    return sub_node(id_, name, "fixed", values=[float(v) for v in values], unit="1",
                    outputs=[{"name": id_, "unit": "1", "dimensions": ["Option"]}])

model = {
  "wasim_version": "0.9.7",
  "source": {"generator": "hand-authored",
             "notes": "WASiM-native distillation of the PLATFORM decommissioning "
                      "decision tool (Platform_2017.ana): multi-attribute value "
                      "ranking of decommissioning options under cost uncertainty, "
                      "with expected scores, preference probabilities and cost-overrun "
                      "risk composed from submodel_stat reducers. See tools/README.md."},
  "simulation_settings": dict(SETTINGS),
  "dimensions": [{"id": "Option", "name": "Decommissioning option", "size": N_OPT,
                  "labels": OPTIONS}],
  "containers": [
    {"id": "Model", "name": "Platform decommissioning (MADA, native)",
     "children": ["Model/Decision_MC"],
     "elements": ["Model/exp_score_full", "Model/exp_score_reef", "Model/exp_score_leave",
                  "Model/exp_scores", "Model/recommended_idx",
                  "Model/pref_full", "Model/pref_reef", "Model/pref_leave",
                  "Model/exp_chosen_cost", "Model/prob_cost_over_budget"]},
    {"id": "Model/Decision_MC", "name": "Decision_MC", "parent": "Model", "kind": "submodel",
     "simulation_settings": dict(SETTINGS),
     "interface": {"inputs": [], "outputs": [
         "Model/Decision_MC/score_full", "Model/Decision_MC/score_reef",
         "Model/Decision_MC/score_leave", "Model/Decision_MC/best_idx",
         "Model/Decision_MC/chosen_cost"]},
     "elements": [f"Model/Decision_MC/cost_mult_{k+1}" for k in range(N_OPT)]
                + [f"Model/Decision_MC/reef_mult_{k+1}" for k in range(N_OPT)]
                + ["Model/Decision_MC/access", "Model/Decision_MC/air",
                   "Model/Decision_MC/cost", "Model/Decision_MC/reef",
                   "Model/Decision_MC/multi_score",
                   "Model/Decision_MC/score_full", "Model/Decision_MC/score_reef",
                   "Model/Decision_MC/score_leave", "Model/Decision_MC/best_idx",
                   "Model/Decision_MC/chosen_cost"]},
  ],
  "elements": [
    # ── submodel: per-realization multi-attribute scoring ─────────────────
    #   independent per-option cost draws + reef-benefit draws
    *[sub_node(f"cost_mult_{k+1}", f"Cost multiplier — {OPTIONS[k]}", "sample",
               distribution=lognormal_unit(COST_GSDEV)) for k in range(N_OPT)],
    *[sub_node(f"reef_mult_{k+1}", f"Reef-benefit multiplier — {OPTIONS[k]}", "sample",
               distribution=lognormal_unit(REEF_GSDEV)) for k in range(N_OPT)],
    fixed_vec("access", "Access / navigation benefit", ACCESS),
    fixed_vec("air", "Air-quality score", AIR),

    # uncertain cost / reef per option = base × the option's own multiplier
    {"id": "Model/Decision_MC/cost", "name": "Cost by option (uncertain)", "primitive": "node",
     "value_rule": "expression", "container": "Model/Decision_MC",
     "inputs": [f"Model/Decision_MC/cost_mult_{k+1}" for k in range(N_OPT)],
     "expression": {"ast": {"op": "array", "elements": [
         op("multiply", lit(COST_RAW[k]), ref(f"Model/Decision_MC/cost_mult_{k+1}"))
         for k in range(N_OPT)]}},
     "outputs": [{"name": "cost", "unit": "1", "dimensions": ["Option"]}],
     "save_results": {"final_value": True}},
    {"id": "Model/Decision_MC/reef", "name": "Reef benefit by option (uncertain)",
     "primitive": "node", "value_rule": "expression", "container": "Model/Decision_MC",
     "inputs": [f"Model/Decision_MC/reef_mult_{k+1}" for k in range(N_OPT)],
     "expression": {"ast": {"op": "array", "elements": [
         op("multiply", lit(REEF[k]), ref(f"Model/Decision_MC/reef_mult_{k+1}"))
         for k in range(N_OPT)]}},
     "outputs": [{"name": "reef", "unit": "1", "dimensions": ["Option"]}],
     "save_results": {"final_value": True}},

    # multi-attribute score per option
    {"id": "Model/Decision_MC/multi_score", "name": "Multi-attribute score by option",
     "primitive": "node", "value_rule": "expression", "container": "Model/Decision_MC",
     "inputs": ["Model/Decision_MC/cost", "Model/Decision_MC/reef",
                "Model/Decision_MC/access", "Model/Decision_MC/air"],
     "expression": {"ast": {"op": "vector_map", "over": "Option", "body": multi_score_body}},
     "outputs": [{"name": "multi_score", "unit": "1", "dimensions": ["Option"]}],
     "save_results": {"final_value": True}},

    # scalar outputs (submodel_stat reduces scalars, not per-member vectors)
    sub_node("score_full", "Score — Full removal", "expression",
             inputs=["Model/Decision_MC/multi_score"],
             expression={"ast": call("get_element", ref("Model/Decision_MC/multi_score"), lit(1))}),
    sub_node("score_reef", "Score — Reef (partial)", "expression",
             inputs=["Model/Decision_MC/multi_score"],
             expression={"ast": call("get_element", ref("Model/Decision_MC/multi_score"), lit(2))}),
    sub_node("score_leave", "Score — Leave in place", "expression",
             inputs=["Model/Decision_MC/multi_score"],
             expression={"ast": call("get_element", ref("Model/Decision_MC/multi_score"), lit(3))}),
    sub_node("best_idx", "Preferred option index (this realization)", "expression",
             inputs=["Model/Decision_MC/multi_score"],
             expression={"ast": call("argmax_array", ref("Model/Decision_MC/multi_score"))}),
    sub_node("chosen_cost", "Cost of the preferred option", "expression",
             inputs=["Model/Decision_MC/cost", "Model/Decision_MC/best_idx"],
             expression={"ast": call("get_element", ref("Model/Decision_MC/cost"),
                                     ref("Model/Decision_MC/best_idx"))}),

    # ── parent: native across-realization reductions ──────────────────────
    par_node("exp_score_full", "Expected score — Full removal",
             sstat("score_full", "mean"), ["Model/Decision_MC"]),
    par_node("exp_score_reef", "Expected score — Reef (partial)",
             sstat("score_reef", "mean"), ["Model/Decision_MC"]),
    par_node("exp_score_leave", "Expected score — Leave in place",
             sstat("score_leave", "mean"), ["Model/Decision_MC"]),

    # expected-score vector + the recommendation (highest expected score)
    {"id": "Model/exp_scores", "name": "Expected score by option", "primitive": "node",
     "value_rule": "expression", "container": "Model",
     "inputs": ["Model/exp_score_full", "Model/exp_score_reef", "Model/exp_score_leave"],
     "expression": {"ast": {"op": "array", "elements": [
         ref("Model/exp_score_full"), ref("Model/exp_score_reef"), ref("Model/exp_score_leave")]}},
     "outputs": [{"name": "exp_scores", "unit": "1", "dimensions": ["Option"]}],
     "save_results": {"final_value": True}},
    par_node("recommended_idx", "Recommended option (max expected score)",
             call("argmax_array", ref("Model/exp_scores")), ["Model/exp_scores"]),

    # probability each option is the preferred one:  best_idx ∈ {1,2,3}
    par_node("pref_full", "P(Full removal preferred)",
             sstat("best_idx", "cumulative_prob", lit(1.5)), ["Model/Decision_MC"]),
    par_node("pref_reef", "P(Reef preferred)",
             op("subtract", sstat("best_idx", "cumulative_prob", lit(2.5)),
                            sstat("best_idx", "cumulative_prob", lit(1.5))),
             ["Model/Decision_MC"]),
    par_node("pref_leave", "P(Leave preferred)",
             sstat("best_idx", "exceedance", lit(2.5)), ["Model/Decision_MC"]),

    # cost outlook for the preferred option
    par_node("exp_chosen_cost", "Expected cost of preferred option ($M)",
             sstat("chosen_cost", "mean"), ["Model/Decision_MC"]),
    par_node("prob_cost_over_budget", f"P(preferred-option cost > ${BUDGET:.0f}M)",
             sstat("chosen_cost", "exceedance", lit(BUDGET)), ["Model/Decision_MC"]),
  ],
}

open(sys.argv[1], "w").write(json.dumps(model, indent=2) + "\n")
print("wrote", sys.argv[1], "options:", OPTIONS)
