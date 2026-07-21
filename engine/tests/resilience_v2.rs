//! Engine resilience: malformed / incomplete model input (the emitter can produce partial data)
//! must yield a diagnosable `Result::Err` or a finite degraded value — never a panic that aborts
//! the run. These pin the defensive guards added after the 0.9.7-corpus resilience audit.

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig};

/// Run and return Ok(results) or Err — but catch any panic and turn it into a test failure, so a
/// regression that reintroduces a panic is caught here rather than aborting the process.
fn run_no_panic(json: &str) -> Result<wasim_engine::SimulationResults, String> {
    let res = std::panic::catch_unwind(|| {
        let m = parse_v2(json).map_err(|e| format!("parse: {e:?}"))?;
        let g = ModelGraphV2::build(&m).map_err(|e| format!("build: {e:?}"))?;
        run_v2(&m, &g, &RunConfig::default()).map_err(|e| format!("run: {e:?}"))
    });
    match res {
        Ok(inner) => inner,
        Err(_) => Err("PANIC".to_string()),
    }
}

/// A lookup table with an empty x/y must not panic — it errors (or degrades) gracefully.
#[test]
fn empty_lookup_table_does_not_panic() {
    let json = r#"{"wasim_version": "0.9.2",
      "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "seed": 1},
      "elements": [
        {"id": "tbl", "name": "T", "primitive": "node", "value_rule": "lookup",
         "table": {"x": [], "y": [], "x_unit": "1", "y_unit": "1"}},
        {"id": "r", "name": "R", "primitive": "node", "value_rule": "expression",
         "expression": {"ast": {"op": "lookup_call", "element_id": "tbl",
           "input": {"op": "literal", "value": 3}}},
         "save_results": {"final_value": true}}
      ]}"#;
    let r = run_no_panic(json);
    assert!(!matches!(r.as_ref().err().map(|s| s.as_str()), Some("PANIC")), "empty lookup must not panic");
    // Whether it errors or degrades to a finite value, both are acceptable — just no panic.
    if let Ok(res) = r {
        if let Some(el) = res.elements.get("r") {
            if let Some(&v) = el.final_values.first() {
                assert!(v.is_finite(), "degraded lookup value must be finite, got {v}");
            }
        }
    }
}

/// A lookup table whose x and y have mismatched lengths must not panic (index-out-of-bounds).
#[test]
fn mismatched_lookup_lengths_do_not_panic() {
    let json = r#"{"wasim_version": "0.9.2",
      "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "seed": 1},
      "elements": [
        {"id": "tbl", "name": "T", "primitive": "node", "value_rule": "lookup",
         "table": {"x": [0, 5, 10], "y": [1], "x_unit": "1", "y_unit": "1"}},
        {"id": "r", "name": "R", "primitive": "node", "value_rule": "expression",
         "expression": {"ast": {"op": "lookup_call", "element_id": "tbl",
           "input": {"op": "literal", "value": 7}}},
         "save_results": {"final_value": true}}
      ]}"#;
    let r = run_no_panic(json);
    assert!(!matches!(r.as_ref().err().map(|s| s.as_str()), Some("PANIC")), "mismatched lookup lengths must not panic");
}

/// An inverse-mode lookup (TBL_Inverse) on a degenerate table must not panic (the inverse path
/// passes the y-array as the x-axis — historically the worst empty-table crash).
#[test]
fn inverse_lookup_on_empty_does_not_panic() {
    let json = r#"{"wasim_version": "0.9.2",
      "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "seed": 1},
      "elements": [
        {"id": "tbl", "name": "T", "primitive": "node", "value_rule": "lookup",
         "table": {"x": [], "y": [], "x_unit": "1", "y_unit": "1"}},
        {"id": "r", "name": "R", "primitive": "node", "value_rule": "expression",
         "expression": {"ast": {"op": "lookup_call", "element_id": "tbl",
           "input": {"op": "literal", "value": 3},
           "input2": {"op": "ref", "element_id": "TBL_Inverse"}}},
         "save_results": {"final_value": true}}
      ]}"#;
    let r = run_no_panic(json);
    assert!(!matches!(r.as_ref().err().map(|s| s.as_str()), Some("PANIC")), "inverse empty lookup must not panic");
}
