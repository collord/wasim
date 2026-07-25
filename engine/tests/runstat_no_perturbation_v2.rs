//! Phase-4 regression guard: adding a `run_stat` node (which triggers the two-pass
//! `run()`) must NOT perturb any other element. Because pass 2 re-runs with the
//! same per-realization seeds and the `run_stat` result does not feed the other
//! elements, their per-realization values — and the sampled `x` itself — must be
//! byte-for-byte identical to the single-pass run of the same model without the
//! `run_stat`. This pins down both the "gated / bit-identical" claim and two-pass
//! determinism.

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig};

fn model(with_run_stat: bool) -> String {
    // x ~ Uniform(0,10); y = x*2 + 1; z = a small stateful accumulator over x.
    // The run_stat variant adds `p = run_stat(x, mean)` that nothing else consumes.
    let run_stat_elem = if with_run_stat {
        r#", {"id": "M/p", "name": "p", "primitive": "node", "value_rule": "expression", "container": "M",
              "expression": {"ast": {"op": "run_stat", "element_id": "M/x", "statistic": "mean"}},
              "save_results": {"final_value": true}}"#
    } else {
        ""
    };
    let elems = if with_run_stat {
        r#"["M/x", "M/y", "M/p"]"#
    } else {
        r#"["M/x", "M/y"]"#
    };
    format!(r#"{{
      "wasim_version": "0.9.7",
      "simulation_settings": {{"duration": {{"value": 1, "unit": "d"}}, "timestep": {{"value": 1, "unit": "d"}}, "n_realizations": 500, "seed": 4242}},
      "containers": [{{"id": "M", "name": "M", "elements": {elems}}}],
      "elements": [
        {{"id": "M/x", "name": "x", "primitive": "node", "value_rule": "sample", "container": "M",
         "distribution": {{"family": "uniform", "parameters": {{"min": {{"value": 0, "unit": "1"}}, "max": {{"value": 10, "unit": "1"}}}}}},
         "save_results": {{"final_value": true}}}},
        {{"id": "M/y", "name": "y", "primitive": "node", "value_rule": "expression", "container": "M",
         "inputs": ["M/x"],
         "expression": {{"ast": {{"op": "add",
            "left": {{"op": "multiply", "left": {{"op": "ref", "element_id": "M/x"}}, "right": {{"op": "literal", "value": 2}}}},
            "right": {{"op": "literal", "value": 1}}}}}},
         "save_results": {{"final_value": true}}}}
        {run_stat_elem}
      ]
    }}"#)
}

fn run(json: &str) -> wasim_engine::SimulationResults {
    let m = parse_v2(json).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    run_v2(&m, &g, &RunConfig::default()).expect("run")
}

#[test]
fn adding_run_stat_does_not_perturb_other_elements() {
    let plain = run(&model(false)); // single pass
    let with = run(&model(true)); // two-pass (run_stat present)

    for id in ["M/x", "M/y"] {
        let a = &plain.elements[id].final_values;
        let b = &with.elements[id].final_values;
        assert_eq!(a.len(), b.len(), "{id}: length changed");
        assert_eq!(a, b, "{id}: values changed when a run_stat was added — the two-pass perturbed the run");
    }

    // And the run_stat itself is correct: mean of Uniform(0,10) ≈ 5, constant per realization.
    let p = &with.elements["M/p"].final_values;
    assert!((p[0] - 5.0).abs() < 0.3, "mean(x) ≈ 5, got {}", p[0]);
    assert!(p.iter().all(|&v| (v - p[0]).abs() < 1e-12), "run_stat must be constant per realization");
}
