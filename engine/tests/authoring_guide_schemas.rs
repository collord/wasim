//! Drift guard for the LLM-authoring guide (`tools/gen_authoring_guide.py`). The guide's
//! distribution catalog is hand-authored from `model.rs::DistributionKind`; this test asserts each
//! family + its documented parameters actually round-trips through the real v2 parser, so the guide
//! can never drift from what the engine accepts (the failure mode the feasibility spike caught).
//!
//! If a family's parameter names change in the engine, the matching case here fails to parse and
//! this test goes red — the signal to update `DISTRIBUTIONS` in the generator.

use wasim_engine::parse_v2;

/// Wrap a `distribution` object into a one-node sample model and assert it parses.
fn assert_distribution_parses(family: &str, distribution_json: &str) {
    let json = format!(
        r#"{{
          "wasim_version": "0.1.0",
          "simulation_settings": {{"duration": {{"value": 10, "unit": "s"}}, "timestep": {{"value": 1, "unit": "s"}}}},
          "elements": [
            {{ "id": "x", "name": "X", "primitive": "node", "value_rule": "sample", "distribution": {distribution_json} }}
          ]
        }}"#
    );
    parse_v2(&json).unwrap_or_else(|e| panic!("distribution family `{family}` failed to parse: {e:?}"));
}

/// A `{value,unit}` quantity literal for a parameter.
fn q(v: f64) -> String {
    format!(r#"{{"value": {v}, "unit": "1"}}"#)
}

#[test]
fn every_documented_distribution_family_parses() {
    let quantity_families: &[(&str, &[&str])] = &[
        ("uniform", &["min", "max"]),
        ("normal", &["mean", "stddev"]),
        ("lognormal", &["mean", "stddev"]),
        ("lognormal_moments", &["mean", "stddev"]),
        ("triangular", &["min", "mode", "max"]),
        ("trapezoidal", &["min", "lower", "upper", "max"]),
        ("pert", &["min", "mode", "max"]),
        ("exponential", &["mean"]),
        ("gamma", &["shape", "scale"]),
        ("beta", &["alpha", "beta"]),
        ("weibull", &["shape", "scale"]),
        ("pareto", &["scale", "shape"]),
        ("extreme_value", &["location", "scale"]),
        ("student_t", &["degrees_of_freedom"]),
        ("pearson_iii", &["mean", "stddev", "skewness"]),
        ("binomial", &["n", "prob"]),
        ("negative_binomial", &["r", "prob"]),
        ("poisson", &["lambda"]),
        ("log_uniform", &["min", "max"]),
        ("log_triangular", &["min", "mode", "max"]),
        ("triangular1090", &["p10", "mode", "p90"]),
    ];
    for (family, params) in quantity_families {
        let body = params.iter().map(|p| format!("\"{p}\": {}", q(0.5))).collect::<Vec<_>>().join(", ");
        assert_distribution_parses(family, &format!(r#"{{"family": "{family}", "parameters": {{{body}}}}}"#));
    }

    // Families whose parameters are not plain Quantities.
    assert_distribution_parses(
        "discrete_uniform",
        r#"{"family": "discrete_uniform", "parameters": {"min": 0, "max": 5}}"#,
    );
    assert_distribution_parses(
        "bernoulli",
        r#"{"family": "bernoulli", "parameters": {"prob": {"value": 0.5, "unit": "1"}}}"#,
    );
    assert_distribution_parses(
        "discrete",
        r#"{"family": "discrete", "parameters": {"outcomes": [1, 2, 3], "probabilities": [0.2, 0.3, 0.5]}}"#,
    );
    assert_distribution_parses(
        "sampled",
        r#"{"family": "sampled", "parameters": {"samples": [1.0, 2.0, 3.0]}}"#,
    );
    assert_distribution_parses(
        "cumulative",
        r#"{"family": "cumulative", "parameters": {"points": [{"x": 0.0, "cumulative_probability": 0.0}, {"x": 1.0, "cumulative_probability": 1.0}]}}"#,
    );
}

/// A few required-field claims from the guide's "Required fields per construct" section, asserted
/// against the parser (a companion to `lens_scaffolds_reconcile.rs`, which covers the scaffolds).
#[test]
fn documented_required_fields_hold() {
    // stock requires initial_value.
    parse_v2(&model(r#"{"id":"s","name":"S","primitive":"stock","initial_value":{"value":0,"unit":"1"}}"#))
        .expect("stock with initial_value parses");
    assert!(
        parse_v2(&model(r#"{"id":"s","name":"S","primitive":"stock"}"#)).is_err(),
        "stock without initial_value must be rejected"
    );

    // node/expression requires expression.
    parse_v2(&model(
        r#"{"id":"e","name":"E","primitive":"node","value_rule":"expression","expression":{"ast":{"op":"literal","value":1}}}"#,
    ))
    .expect("expression node parses");

    // gate requires root.
    parse_v2(&model(
        r#"{"id":"g","name":"G","primitive":"gate","root":{"op":"n_vote","threshold":1,"children":[]}}"#,
    ))
    .expect("gate with root parses");

    // The documented `failure_process` sub-schema: basis + time_to_failure distribution +
    // repair.{policy,time_to_repair}. Guards the guide's repairable-component recipe.
    parse_v2(&model(
        r#"{"id":"pump","name":"Pump","primitive":"event",
            "failure_process":{
              "basis":"exposure_time",
              "time_to_failure":{"family":"exponential","parameters":{"mean":{"value":100,"unit":"h"}}},
              "repair":{"policy":"repair","time_to_repair":{"family":"exponential","parameters":{"mean":{"value":8,"unit":"h"}}}}
            }}"#,
    ))
    .expect("event with failure_process (basis + ttf + repair) parses");
}

fn model(element_json: &str) -> String {
    format!(
        r#"{{
          "wasim_version": "0.1.0",
          "simulation_settings": {{"duration": {{"value": 10, "unit": "s"}}, "timestep": {{"value": 1, "unit": "s"}}}},
          "elements": [ {element_json} ]
        }}"#
    )
}
