import json, math

# Departure grid (minutes before scheduled departure that I leave home).
grid = list(range(60, 185, 5))      # 60,65,...,180
N = len(grid)

def lit(v): return {"op": "literal", "value": v}
def ref(i): return {"op": "ref", "element_id": i}
def call(fn, *a): return {"op": "call", "fn": fn, "args": list(a)}
def binop(op, l, r): return {"op": op, "left": l, "right": r}
def depart_k():
    return {"op": "index", "array": ref("Model/depart_times"),
            "indices": [{"op": "index_ref", "axis": "row"}]}
def sstat(stat, arg=None):
    n = {"op": "submodel_stat", "submodel_id": "Model/Trip",
         "output": "Model/Trip/travel", "statistic": stat}
    if arg is not None:
        n["arg"] = arg
    return n

LOSS = 400.0

# E[Cost(depart)] = depart - mean(travel) + Loss * P(travel > depart)
exp_cost_body = binop("add",
    binop("subtract", depart_k(), ref("Model/mean_travel")),
    binop("multiply", lit(LOSS), sstat("exceedance", depart_k())))

# P(miss | depart) = P(travel > depart)
prob_miss_body = sstat("exceedance", depart_k())

# Deterministic cost (uncertainty ignored): travel fixed at its median.
#   det_cost(depart) = depart - median(travel) + Loss * 1{median > depart}
det_cost_body = binop("add",
    binop("subtract", depart_k(), ref("Model/median_travel")),
    binop("multiply", lit(LOSS),
          {"op": "if",
           "cond": binop("gt", ref("Model/median_travel"), depart_k()),
           "then": lit(1.0), "else": lit(0.0)}))

def vmap(body): return {"op": "vector_map", "over": "Depart", "body": body}

SETTINGS = {"duration": {"value": 1, "unit": "1"},
            "timestep": {"value": 1, "unit": "1"},
            "n_realizations": 2000, "seed": 12345}

def lognormal(median, gsdev):
    return {"family": "lognormal", "parameters": {
        "mean":   {"value": math.log(median), "unit": "1"},
        "stddev": {"value": math.log(gsdev),  "unit": "1"}}}

model = {
  "wasim_version": "0.9.7",
  "source": {"generator": "hand-authored",
             "notes": "WASiM-native EVIU (expected value of including uncertainty) for the "
                      "airport plane-catching problem. The across-realization expected-cost "
                      "curve is composed natively from submodel_stat (mean / exceedance) swept "
                      "over the Depart decision axis via vector_map — the same sweep-composition "
                      "pattern as loss_exceedance_curve. See tools/examples/README section."},
  "simulation_settings": dict(SETTINGS),
  "dimensions": [{"id": "Depart", "name": "Departure lead time", "size": N,
                  "labels": [str(g) for g in grid]}],
  "containers": [
    {"id": "Model", "name": "Plane catching (EVIU, native)",
     "children": ["Model/Trip"],
     "elements": ["Model/depart_times", "Model/mean_travel", "Model/median_travel",
                  "Model/prob_miss", "Model/exp_cost", "Model/min_exp_cost",
                  "Model/best_depart_idx", "Model/best_depart",
                  "Model/det_cost", "Model/det_best_idx", "Model/det_best_depart",
                  "Model/exp_cost_at_det", "Model/eviu"]},
    {"id": "Model/Trip", "name": "Trip", "parent": "Model", "kind": "submodel",
     "simulation_settings": dict(SETTINGS),
     "interface": {"inputs": [], "outputs": ["Model/Trip/travel"]},
     "elements": ["Model/Trip/drive", "Model/Trip/park", "Model/Trip/gate",
                  "Model/Trip/travel"]},
  ],
  "elements": [
    # ── inner submodel: the uncertain journey ─────────────────────────────
    {"id": "Model/Trip/drive", "name": "Drive to airport", "primitive": "node",
     "value_rule": "sample", "container": "Model/Trip",
     "distribution": lognormal(40, 1.3), "save_results": {"final_value": True}},
    {"id": "Model/Trip/park", "name": "Park to gate", "primitive": "node",
     "value_rule": "sample", "container": "Model/Trip",
     "distribution": lognormal(30, 1.3), "save_results": {"final_value": True}},
    {"id": "Model/Trip/gate", "name": "Gate buffer", "primitive": "node",
     "value_rule": "sample", "container": "Model/Trip",
     "distribution": {"family": "triangular", "parameters": {
         "min": {"value": 20, "unit": "1"}, "mode": {"value": 30, "unit": "1"},
         "max": {"value": 40, "unit": "1"}}},
     "save_results": {"final_value": True}},
    {"id": "Model/Trip/travel", "name": "Total home-to-gate time", "primitive": "node",
     "value_rule": "expression", "container": "Model/Trip",
     "inputs": ["Model/Trip/drive", "Model/Trip/park", "Model/Trip/gate"],
     "expression": {"ast": binop("add", binop("add", ref("Model/Trip/drive"),
                                              ref("Model/Trip/park")),
                                 ref("Model/Trip/gate"))},
     "outputs": [{"name": "travel", "unit": "min", "dimensions": []}],
     "save_results": {"final_value": True}},

    # ── decision axis ─────────────────────────────────────────────────────
    {"id": "Model/depart_times", "name": "Candidate lead times", "primitive": "node",
     "value_rule": "fixed", "container": "Model", "values": [float(g) for g in grid],
     "unit": "min", "outputs": [{"name": "depart_times", "unit": "min",
                                 "dimensions": ["Depart"]}]},

    # ── across-realization reductions of the trip (scalars) ───────────────
    {"id": "Model/mean_travel", "name": "Mean travel time", "primitive": "node",
     "value_rule": "expression", "container": "Model", "inputs": ["Model/Trip"],
     "expression": {"ast": sstat("mean")},
     "outputs": [{"name": "mean_travel", "unit": "min"}],
     "save_results": {"final_value": True}},
    {"id": "Model/median_travel", "name": "Median travel time", "primitive": "node",
     "value_rule": "expression", "container": "Model", "inputs": ["Model/Trip"],
     "expression": {"ast": sstat("percentile", lit(50.0))},
     "outputs": [{"name": "median_travel", "unit": "min"}],
     "save_results": {"final_value": True}},

    # ── swept curves over the decision axis ───────────────────────────────
    {"id": "Model/prob_miss", "name": "P(miss) vs lead time", "primitive": "node",
     "value_rule": "expression", "container": "Model",
     "inputs": ["Model/Trip", "Model/depart_times"],
     "expression": {"ast": vmap(prob_miss_body)},
     "outputs": [{"name": "prob_miss", "unit": "1", "dimensions": ["Depart"]}],
     "save_results": {"final_value": True}},
    {"id": "Model/exp_cost", "name": "Expected cost vs lead time", "primitive": "node",
     "value_rule": "expression", "container": "Model",
     "inputs": ["Model/Trip", "Model/depart_times", "Model/mean_travel"],
     "expression": {"ast": vmap(exp_cost_body)},
     "outputs": [{"name": "exp_cost", "unit": "min", "dimensions": ["Depart"]}],
     "save_results": {"final_value": True}},
    {"id": "Model/det_cost", "name": "Deterministic cost vs lead time", "primitive": "node",
     "value_rule": "expression", "container": "Model",
     "inputs": ["Model/depart_times", "Model/median_travel"],
     "expression": {"ast": vmap(det_cost_body)},
     "outputs": [{"name": "det_cost", "unit": "min", "dimensions": ["Depart"]}],
     "save_results": {"final_value": True}},

    # ── stochastic optimum ────────────────────────────────────────────────
    {"id": "Model/min_exp_cost", "name": "Min expected cost", "primitive": "node",
     "value_rule": "expression", "container": "Model", "inputs": ["Model/exp_cost"],
     "expression": {"ast": call("min_array", ref("Model/exp_cost"))},
     "outputs": [{"name": "min_exp_cost", "unit": "min"}],
     "save_results": {"final_value": True}},
    {"id": "Model/best_depart_idx", "name": "Best lead-time index", "primitive": "node",
     "value_rule": "expression", "container": "Model", "inputs": ["Model/exp_cost"],
     "expression": {"ast": call("argmin_array", ref("Model/exp_cost"))},
     "outputs": [{"name": "best_depart_idx", "unit": "1"}],
     "save_results": {"final_value": True}},
    {"id": "Model/best_depart", "name": "Best lead time (min)", "primitive": "node",
     "value_rule": "expression", "container": "Model",
     "inputs": ["Model/depart_times", "Model/best_depart_idx"],
     "expression": {"ast": call("get_element", ref("Model/depart_times"),
                                ref("Model/best_depart_idx"))},
     "outputs": [{"name": "best_depart", "unit": "min"}],
     "save_results": {"final_value": True}},

    # ── deterministic optimum (uncertainty ignored) ───────────────────────
    {"id": "Model/det_best_idx", "name": "Deterministic best index", "primitive": "node",
     "value_rule": "expression", "container": "Model", "inputs": ["Model/det_cost"],
     "expression": {"ast": call("argmin_array", ref("Model/det_cost"))},
     "outputs": [{"name": "det_best_idx", "unit": "1"}],
     "save_results": {"final_value": True}},
    {"id": "Model/det_best_depart", "name": "Deterministic best lead time (min)",
     "primitive": "node", "value_rule": "expression", "container": "Model",
     "inputs": ["Model/depart_times", "Model/det_best_idx"],
     "expression": {"ast": call("get_element", ref("Model/depart_times"),
                                ref("Model/det_best_idx"))},
     "outputs": [{"name": "det_best_depart", "unit": "min"}],
     "save_results": {"final_value": True}},

    # ── EVIU = E[cost | deterministic decision] − E[cost | stochastic decision] ──
    {"id": "Model/exp_cost_at_det", "name": "Expected cost of the naive decision",
     "primitive": "node", "value_rule": "expression", "container": "Model",
     "inputs": ["Model/exp_cost", "Model/det_best_idx"],
     "expression": {"ast": call("get_element", ref("Model/exp_cost"),
                                ref("Model/det_best_idx"))},
     "outputs": [{"name": "exp_cost_at_det", "unit": "min"}],
     "save_results": {"final_value": True}},
    {"id": "Model/eviu", "name": "EVIU (minutes saved)", "primitive": "node",
     "value_rule": "expression", "container": "Model",
     "inputs": ["Model/exp_cost_at_det", "Model/min_exp_cost"],
     "expression": {"ast": binop("subtract", ref("Model/exp_cost_at_det"),
                                 ref("Model/min_exp_cost"))},
     "outputs": [{"name": "eviu", "unit": "min"}],
     "save_results": {"final_value": True}},
  ],
}

import sys
open(sys.argv[1], "w").write(json.dumps(model, indent=2) + "\n")
print("grid:", grid)
print("wrote", sys.argv[1], "N=", N)
