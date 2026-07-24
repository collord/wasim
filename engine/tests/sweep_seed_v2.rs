//! Phase-1 regression tests for the sweep-seed coordinate fix.
//!
//! Bug (pre-fix): every submodel re-seeds from the same base seed
//! (`submodel_v2.rs` built `RunConfig { seed: config.seed, .. }`), so two distinct
//! submodels drew *byte-identical* random streams. Latent because `submodel_stat`
//! reduces each submodel to a scalar — but a downstream node combining two submodels'
//! statistics inherits spurious correlation. See the plan / WORKPLAN_SWEEP_COMPOSITION.md.
//!
//! These tests reach past `submodel_stat` (which collapses to a scalar and hides the
//! streams) by calling `submodel_v2::run_submodels` directly, which returns the raw
//! per-realization sample vectors keyed by (submodel_id, output).

use std::collections::HashMap;
use wasim_engine::{parse_v2, RunConfig};
use wasim_engine::submodel_v2::run_submodels;

/// Two submodels, A and B, each a single `uniform(0,1)` sample saved as final, referenced
/// by a parent `submodel_stat`. A and B are independent models, so their per-realization
/// draws SHOULD differ. `n_realizations` is deliberately > 1 so a collision is unambiguous.
fn two_submodel_model(a_first: bool) -> String {
    // Declaration order of the two submodel containers is parameterized to exercise
    // reorder-invariance: the fix derives the child seed from the container *id* (stable),
    // not its position, so swapping A and B must not change either submodel's stream.
    let sub_a = r#"{"id": "Model/A", "name": "A", "parent": "Model", "kind": "submodel",
         "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 16},
         "interface": {"inputs": [], "outputs": ["Model/A/x"]}, "elements": ["Model/A/x"]}"#;
    let sub_b = r#"{"id": "Model/B", "name": "B", "parent": "Model", "kind": "submodel",
         "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 16},
         "interface": {"inputs": [], "outputs": ["Model/B/x"]}, "elements": ["Model/B/x"]}"#;
    let (first, second) = if a_first { (sub_a, sub_b) } else { (sub_b, sub_a) };

    format!(r#"{{
      "wasim_version": "0.8.3",
      "simulation_settings": {{"duration": {{"value": 1, "unit": "d"}}, "timestep": {{"value": 1, "unit": "d"}}, "n_realizations": 16, "seed": 42}},
      "containers": [
        {{"id": "Model", "name": "Model", "children": ["Model/A", "Model/B"], "elements": ["Model/Result"]}},
        {first},
        {second}
      ],
      "elements": [
        {{"id": "Model/A/x", "name": "x", "primitive": "node", "value_rule": "sample",
         "container": "Model/A",
         "distribution": {{"family": "uniform", "parameters": {{"min": {{"value": 0, "unit": "1"}}, "max": {{"value": 1, "unit": "1"}}}}}},
         "save_results": {{"final_value": true}}}},
        {{"id": "Model/B/x", "name": "x", "primitive": "node", "value_rule": "sample",
         "container": "Model/B",
         "distribution": {{"family": "uniform", "parameters": {{"min": {{"value": 0, "unit": "1"}}, "max": {{"value": 1, "unit": "1"}}}}}},
         "save_results": {{"final_value": true}}}},
        {{"id": "Model/Result", "name": "Result", "primitive": "node", "value_rule": "expression",
         "container": "Model", "inputs": ["Model/A", "Model/B"],
         "expression": {{"ast": {{"op": "add",
            "left": {{"op": "submodel_stat", "submodel_id": "Model/A", "output": "Model/A/x", "statistic": "mean"}},
            "right": {{"op": "submodel_stat", "submodel_id": "Model/B", "output": "Model/B/x", "statistic": "mean"}}}}}},
         "save_results": {{"final_value": true}}}}
      ]
    }}"#)
}

fn run_raw(json: &str) -> HashMap<(String, String), Vec<f64>> {
    let m = parse_v2(json).expect("parse");
    run_submodels(&m, &RunConfig::default()).expect("run_submodels")
}

/// Core regression: two distinct submodels with the same distribution must NOT draw
/// identical streams. Fails on pre-fix `main` (streams byte-identical), passes after.
#[test]
fn distinct_submodels_draw_disjoint_streams() {
    let out = run_raw(&two_submodel_model(true));
    let a = out.get(&("Model/A".to_string(), "Model/A/x".to_string())).expect("A vector");
    let b = out.get(&("Model/B".to_string(), "Model/B/x".to_string())).expect("B vector");

    assert_eq!(a.len(), 16, "A should have one sample per realization");
    assert_eq!(b.len(), 16, "B should have one sample per realization");
    assert_ne!(
        a, b,
        "two independent submodels drew IDENTICAL streams — the shared-seed bug \
         (submodel_v2.rs re-seeding every child from the same base seed)"
    );
}

/// Reorder-invariance: swapping the declaration order of the two submodel containers must
/// not change either submodel's stream, because the seed is derived from the stable
/// container id, not its position (the position-derived trap the spike demonstrated).
#[test]
fn submodel_streams_are_reorder_invariant() {
    let a_first = run_raw(&two_submodel_model(true));
    let b_first = run_raw(&two_submodel_model(false));

    for id in ["Model/A", "Model/B"] {
        let key = (id.to_string(), format!("{id}/x"));
        assert_eq!(
            a_first.get(&key), b_first.get(&key),
            "submodel {id} stream changed when container declaration order was swapped — \
             the seed must derive from the stable id, not the position"
        );
    }
}

// ── Phase 2: CI invariants over the seed coordinate ─────────────────────────────

/// Build a model with one `uniform(0,1)` submodel per name in `names`, each referenced by a
/// parent `submodel_stat`, so `run_submodels` runs them all. Generalizes `two_submodel_model`.
fn n_submodel_model(names: &[&str]) -> String {
    let containers: Vec<String> = names.iter().map(|n| format!(
        r#"{{"id": "Model/{n}", "name": "{n}", "parent": "Model", "kind": "submodel",
           "simulation_settings": {{"duration": {{"value": 1, "unit": "d"}}, "timestep": {{"value": 1, "unit": "d"}}, "n_realizations": 16}},
           "interface": {{"inputs": [], "outputs": ["Model/{n}/x"]}}, "elements": ["Model/{n}/x"]}}"#)).collect();

    let samples: Vec<String> = names.iter().map(|n| format!(
        r#"{{"id": "Model/{n}/x", "name": "x", "primitive": "node", "value_rule": "sample",
           "container": "Model/{n}",
           "distribution": {{"family": "uniform", "parameters": {{"min": {{"value": 0, "unit": "1"}}, "max": {{"value": 1, "unit": "1"}}}}}},
           "save_results": {{"final_value": true}}}}"#)).collect();

    // A parent element that sums each submodel's mean, so every submodel is referenced.
    let refs: Vec<String> = names.iter().map(|n| format!(
        r#"{{"op": "submodel_stat", "submodel_id": "Model/{n}", "output": "Model/{n}/x", "statistic": "mean"}}"#)).collect();
    let sum_ast = refs.iter().skip(1).fold(refs[0].clone(), |acc, r| format!(
        r#"{{"op": "add", "left": {acc}, "right": {r}}}"#));

    let child_ids: Vec<String> = names.iter().map(|n| format!("\"Model/{n}\"")).collect();
    format!(r#"{{
      "wasim_version": "0.8.3",
      "simulation_settings": {{"duration": {{"value": 1, "unit": "d"}}, "timestep": {{"value": 1, "unit": "d"}}, "n_realizations": 16, "seed": 42}},
      "containers": [
        {{"id": "Model", "name": "Model", "children": [{}], "elements": ["Model/Result"]}},
        {}
      ],
      "elements": [
        {},
        {{"id": "Model/Result", "name": "Result", "primitive": "node", "value_rule": "expression",
         "container": "Model", "inputs": [{}],
         "expression": {{"ast": {sum_ast}}}, "save_results": {{"final_value": true}}}}
      ]
    }}"#,
        child_ids.join(", "),
        containers.join(",\n        "),
        samples.join(",\n        "),
        child_ids.join(", "),
    )
}

/// Composition isolation: adding an unrelated submodel C to the graph must not perturb A's or
/// B's streams by a single bit. Direct proof that `child_seed` depends only on a submodel's own
/// id — a new sweep cannot shift any existing sweep's draws.
#[test]
fn adding_a_submodel_does_not_perturb_existing_streams() {
    let without_c = run_raw(&n_submodel_model(&["A", "B"]));
    let with_c = run_raw(&n_submodel_model(&["A", "B", "C"]));

    for id in ["Model/A", "Model/B"] {
        let key = (id.to_string(), format!("{id}/x"));
        assert_eq!(
            without_c.get(&key), with_c.get(&key),
            "submodel {id} stream changed when unrelated submodel C was added — \
             composition is not isolated (child_seed must depend only on the submodel's own id)"
        );
    }
    // Sanity: C actually ran (so the test isn't vacuously comparing absent keys).
    assert!(
        with_c.contains_key(&("Model/C".to_string(), "Model/C/x".to_string())),
        "submodel C did not run — isolation assertion would be vacuous"
    );
}

/// Determinism baseline: the same model run twice yields byte-identical submodel streams.
/// Guards the seed derivation against any accidental dependence on run-to-run state.
#[test]
fn identical_runs_are_bit_identical() {
    let json = n_submodel_model(&["A", "B"]);
    assert_eq!(
        run_raw(&json), run_raw(&json),
        "two runs of the same model produced different submodel streams — determinism broken"
    );
}
