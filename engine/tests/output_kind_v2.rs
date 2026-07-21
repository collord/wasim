//! §1c `output_kind` — the accumulation axis on stock secondary outputs (schema 0.9.7).
//! `role` names the flow (addition / withdrawal / overflow / net_change); `output_kind` says how it
//! accumulates: `level` (the stock's own value), `rate` (per-step applied flow), or `cumulative`
//! (running total of the flow since the run start). The fused `*_rate` names from 0.9.6 are
//! retained as aliases (normalized to `<flow>` + `output_kind: rate`) — the existing
//! `stock_ports_and_qualified_refs` test in `globals_ports_v2.rs` guards that back-compat.

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig};

fn run_json(json: &str) -> wasim_engine::SimulationResults {
    let m = parse_v2(json).expect("parse");
    let g = ModelGraphV2::build(&m).expect("build");
    let cfg = RunConfig::default();
    run_v2(&m, &g, &cfg).expect("run")
}

fn final_of(r: &wasim_engine::SimulationResults, id: &str) -> f64 {
    r.elements[id].final_values[0]
}

/// A stock with constant inflow 5/d and outflow 2/d over 4 steps. Cumulative ports report the
/// running total of each flow; readers see the previous step's value (same causality as levels).
/// A `rate`-kind port and a `level`-kind port confirm the other two kinds are unaffected.
#[test]
fn cumulative_rate_and_level_kinds() {
    let json = r#"{
      "wasim_version": "0.9.7",
      "simulation_settings": {"duration": {"value": 4, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}},
      "elements": [
        {"id": "in_rate", "name": "InRate", "primitive": "node", "value_rule": "fixed", "value": {"value": 5.0, "unit": "1/d"}},
        {"id": "out_rate", "name": "OutRate", "primitive": "node", "value_rule": "fixed", "value": {"value": 2.0, "unit": "1/d"}},
        {"id": "s", "name": "S", "primitive": "stock",
         "outputs": [
           {"name": "s", "unit": "1"},
           {"name": "s#2", "unit": "1", "role": "addition", "output_kind": "cumulative"},
           {"name": "s#3", "unit": "1", "role": "withdrawal", "output_kind": "cumulative"},
           {"name": "s#4", "unit": "1", "role": "net_change", "output_kind": "cumulative"},
           {"name": "s#5", "unit": "1/d", "role": "addition", "output_kind": "rate"},
           {"name": "s#6", "unit": "1", "role": "net_change", "output_kind": "level"}
         ],
         "initial_value": {"value": 0.0, "unit": "1"},
         "inflows": ["in_rate"], "outflows": ["out_rate"]},
        {"id": "cum_add", "name": "CA", "primitive": "node", "value_rule": "expression", "inputs": ["s"],
         "expression": {"ast": {"op": "ref", "element_id": "s", "output": "s#2"}}},
        {"id": "cum_wd", "name": "CW", "primitive": "node", "value_rule": "expression", "inputs": ["s"],
         "expression": {"ast": {"op": "ref", "element_id": "s", "output": "s#3"}}},
        {"id": "cum_net", "name": "CN", "primitive": "node", "value_rule": "expression", "inputs": ["s"],
         "expression": {"ast": {"op": "ref", "element_id": "s", "output": "s#4"}}},
        {"id": "rate_add", "name": "RA", "primitive": "node", "value_rule": "expression", "inputs": ["s"],
         "expression": {"ast": {"op": "ref", "element_id": "s", "output": "s#5"}}},
        {"id": "lvl", "name": "LV", "primitive": "node", "value_rule": "expression", "inputs": ["s"],
         "expression": {"ast": {"op": "ref", "element_id": "s", "output": "s#6"}}}
      ]
    }"#;
    let r = run_json(json);
    // The reader at the final step (step 3) sees the cumulative through the end of step 2, i.e.
    // 3 steps of accumulation (steps 0,1,2). Deposits: 3·5 = 15; withdrawals: 3·2 = 6; net: 3·3 = 9.
    assert!((final_of(&r, "cum_add") - 15.0).abs() < 1e-9, "cumulative deposits, got {}", final_of(&r, "cum_add"));
    assert!((final_of(&r, "cum_wd") - 6.0).abs() < 1e-9, "cumulative withdrawals, got {}", final_of(&r, "cum_wd"));
    assert!((final_of(&r, "cum_net") - 9.0).abs() < 1e-9, "cumulative net change, got {}", final_of(&r, "cum_net"));
    // rate kind is the steady per-step rate (unchanged 0.9.6 behavior).
    assert!((final_of(&r, "rate_add") - 5.0).abs() < 1e-9, "addition rate, got {}", final_of(&r, "rate_add"));
    // level kind is the stock's own value at the reader's view (start of step 3) = 0 + 3·3 = 9.
    assert!((final_of(&r, "lvl") - 9.0).abs() < 1e-9, "level kind = stock value, got {}", final_of(&r, "lvl"));
}

/// **Cumulative is conserved: net = deposits − withdrawals.** For a simple stock with no
/// floor/capacity, the cumulative net_change must equal cumulative additions minus cumulative
/// withdrawals at every step (an accounting identity the accumulators must preserve).
#[test]
fn cumulative_flows_balance() {
    let json = r#"{
      "wasim_version": "0.9.7",
      "simulation_settings": {"duration": {"value": 6, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}},
      "elements": [
        {"id": "in_rate", "name": "InRate", "primitive": "node", "value_rule": "fixed", "value": {"value": 7.0, "unit": "1/d"}},
        {"id": "out_rate", "name": "OutRate", "primitive": "node", "value_rule": "fixed", "value": {"value": 3.0, "unit": "1/d"}},
        {"id": "s", "name": "S", "primitive": "stock",
         "outputs": [
           {"name": "s", "unit": "1"},
           {"name": "s#2", "unit": "1", "role": "addition", "output_kind": "cumulative"},
           {"name": "s#3", "unit": "1", "role": "withdrawal", "output_kind": "cumulative"},
           {"name": "s#4", "unit": "1", "role": "net_change", "output_kind": "cumulative"}
         ],
         "initial_value": {"value": 0.0, "unit": "1"},
         "inflows": ["in_rate"], "outflows": ["out_rate"]},
        {"id": "ca", "name": "CA", "primitive": "node", "value_rule": "expression", "inputs": ["s"],
         "expression": {"ast": {"op": "ref", "element_id": "s", "output": "s#2"}}},
        {"id": "cw", "name": "CW", "primitive": "node", "value_rule": "expression", "inputs": ["s"],
         "expression": {"ast": {"op": "ref", "element_id": "s", "output": "s#3"}}},
        {"id": "cn", "name": "CN", "primitive": "node", "value_rule": "expression", "inputs": ["s"],
         "expression": {"ast": {"op": "ref", "element_id": "s", "output": "s#4"}}}
      ]
    }"#;
    let r = run_json(json);
    let (ca, cw, cn) = (final_of(&r, "ca"), final_of(&r, "cw"), final_of(&r, "cn"));
    assert!((cn - (ca - cw)).abs() < 1e-9, "cumulative net {cn} must equal deposits {ca} − withdrawals {cw}");
    assert!(ca > 0.0 && cw > 0.0, "both flows accumulated: deposits {ca}, withdrawals {cw}");
}

/// **Alias back-compat.** A `role: "addition_rate"` (0.9.6 fused name) must behave exactly as
/// `role: "addition", output_kind: "rate"` after normalization — same published rate.
#[test]
fn rate_alias_normalizes() {
    let mk = |role_json: &str| format!(r#"{{
      "wasim_version": "0.9.7",
      "simulation_settings": {{"duration": {{"value": 3, "unit": "d"}}, "timestep": {{"value": 1, "unit": "d"}}}},
      "elements": [
        {{"id": "in_rate", "name": "InRate", "primitive": "node", "value_rule": "fixed", "value": {{"value": 4.0, "unit": "1/d"}}}},
        {{"id": "s", "name": "S", "primitive": "stock",
         "outputs": [
           {{"name": "s", "unit": "1"}},
           {{"name": "s#2", "unit": "1/d", {role_json}}}
         ],
         "initial_value": {{"value": 0.0, "unit": "1"}},
         "inflows": ["in_rate"]}},
        {{"id": "reader", "name": "RD", "primitive": "node", "value_rule": "expression", "inputs": ["s"],
         "expression": {{"ast": {{"op": "ref", "element_id": "s", "output": "s#2"}}}}}}
      ]
    }}"#);
    let aliased = run_json(&mk(r#""role": "addition_rate""#));
    let explicit = run_json(&mk(r#""role": "addition", "output_kind": "rate""#));
    assert_eq!(
        final_of(&aliased, "reader"), final_of(&explicit, "reader"),
        "role:addition_rate must equal role:addition+kind:rate after normalization"
    );
    assert!((final_of(&aliased, "reader") - 4.0).abs() < 1e-9, "both publish the 4/d addition rate");
}

/// **Role-less `output_kind`.** re-gsm emits `output_kind` on outputs with *no* `role` — a bare
/// `level` (the stock value) and a `cumulative` (running net change). These must publish (a
/// role-less output defaults to the `net_change` flow), not fall through to the primary value. A
/// role-less output with *neither* role nor kind stays inert (pre-0.9.2 fallback to primary).
#[test]
fn role_less_output_kind_publishes() {
    let json = r#"{
      "wasim_version": "0.9.7",
      "simulation_settings": {"duration": {"value": 4, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}},
      "elements": [
        {"id": "in_rate", "name": "InRate", "primitive": "node", "value_rule": "fixed", "value": {"value": 5.0, "unit": "1/d"}},
        {"id": "s", "name": "S", "primitive": "stock",
         "outputs": [
           {"name": "s", "unit": "1"},
           {"name": "s#2", "unit": "1", "output_kind": "level"},
           {"name": "s#3", "unit": "1", "output_kind": "cumulative"},
           {"name": "s#4", "unit": "1"}
         ],
         "initial_value": {"value": 0.0, "unit": "1"},
         "inflows": ["in_rate"]},
        {"id": "lvl", "name": "L", "primitive": "node", "value_rule": "expression", "inputs": ["s"],
         "expression": {"ast": {"op": "ref", "element_id": "s", "output": "s#2"}}},
        {"id": "cum", "name": "C", "primitive": "node", "value_rule": "expression", "inputs": ["s"],
         "expression": {"ast": {"op": "ref", "element_id": "s", "output": "s#3"}}},
        {"id": "roleless", "name": "R", "primitive": "node", "value_rule": "expression", "inputs": ["s"],
         "expression": {"ast": {"op": "ref", "element_id": "s", "output": "s#4"}}}
      ]
    }"#;
    let r = run_json(json);
    // Reader at final step (step 3) sees end-of-step-2 values: level = 3·5 = 15; cumulative net
    // through step 2 = 15 (accumulates the net change, = deposits since no outflow).
    assert!((final_of(&r, "lvl") - 15.0).abs() < 1e-9, "bare level kind = stock value, got {}", final_of(&r, "lvl"));
    assert!((final_of(&r, "cum") - 15.0).abs() < 1e-9, "bare cumulative kind = running net, got {}", final_of(&r, "cum"));
    // s#4 has neither role nor kind → inert → the qualified ref falls back to the primary level.
    assert!((final_of(&r, "roleless") - 15.0).abs() < 1e-9, "role/kind-less port → primary level, got {}", final_of(&r, "roleless"));
}
