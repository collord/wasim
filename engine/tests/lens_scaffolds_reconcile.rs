//! P2 gate for the lens-manifest work (WASIM_LENS_MANIFEST_SPEC.md §13): every construct the
//! General-lens palette can insert must scaffold to a model the v2 parser accepts. These fixtures
//! mirror the frontend `PALETTE.make()` scaffolds (frontend/src/model/edits.ts) — one element per
//! model, so a parse failure names the offending construct. The load-bearing cases are the ones
//! that carry a required `input`/endpoint left empty at insert time (`pid`, `convolution`,
//! `queue`, `hysteresis`): this test proves the engine tolerates the empty ref, rather than
//! assuming it.

use wasim_engine::parse_v2;

/// Wrap a single scaffolded element (the JSON `PALETTE.make()` emits, minus id/name) into a
/// minimal model and assert it parses.
fn assert_scaffold_parses(construct: &str, element_body: &str) {
    let json = format!(
        r#"{{
          "wasim_version": "0.1.0",
          "simulation_settings": {{"duration": {{"value": 10, "unit": "s"}}, "timestep": {{"value": 1, "unit": "s"}}}},
          "elements": [ {{ "id": "e", "name": "E", {element_body} }} ]
        }}"#
    );
    parse_v2(&json).unwrap_or_else(|err| panic!("scaffold `{construct}` failed to parse: {err:?}"));
}

#[test]
fn all_general_palette_scaffolds_reconcile() {
    // Node value_rules.
    assert_scaffold_parses("filter", r#""primitive": "node", "value_rule": "filter", "statistic": "mean""#);
    assert_scaffold_parses(
        "pid",
        r#""primitive": "node", "value_rule": "pid", "input": "", "setpoint": {"value": 0, "unit": "1"}"#,
    );
    assert_scaffold_parses(
        "terminal_expression",
        r#""primitive": "node", "value_rule": "terminal_expression", "expression": {"ast": {"op": "literal", "value": 0}, "display": "0"}"#,
    );
    assert_scaffold_parses(
        "convolution",
        r#""primitive": "node", "value_rule": "convolution", "input": "", "response": {"times": [0], "values": [0]}"#,
    );
    assert_scaffold_parses(
        "queue",
        r#""primitive": "node", "value_rule": "queue", "input": "", "delay_time": {"value": 1, "unit": "1"}"#,
    );
    assert_scaffold_parses("status", r#""primitive": "node", "value_rule": "status", "set": {}, "reset": {}"#);
    assert_scaffold_parses("milestone", r#""primitive": "node", "value_rule": "milestone", "trigger": {}"#);
    assert_scaffold_parses(
        "hysteresis",
        r#""primitive": "node", "value_rule": "hysteresis", "input": "",
           "high_threshold": {"value": 1, "unit": "1"}, "low_threshold": {"value": 0, "unit": "1"},
           "output_above": {"value": 1, "unit": "1"}, "output_below": {"value": 0, "unit": "1"}"#,
    );
    assert_scaffold_parses(
        "markov",
        r#""primitive": "node", "value_rule": "markov", "states": ["ok", "bad"], "initial_state": "ok",
           "transition_matrix": [[0.9, 0.1], [0, 1]], "output_values": [0, 1]"#,
    );
    assert_scaffold_parses(
        "process",
        r#""primitive": "node", "value_rule": "process",
           "process": {"family": "gbm", "mean_type": "arithmetic", "mean": {"value": 0, "unit": "1"}, "stddev": {"value": 0, "unit": "1"}}"#,
    );

    // Primitives.
    assert_scaffold_parses("resource", r#""primitive": "resource", "initial_value": {"value": 0, "unit": "1"}"#);
    assert_scaffold_parses("cell", r#""primitive": "cell", "volume": {"value": 1, "unit": "m^3"}"#);
    assert_scaffold_parses("species", r#""primitive": "species", "molecular_weight": {"value": 0.001, "unit": "kg/mol"}"#);
    assert_scaffold_parses("medium", r#""primitive": "medium", "phase": "fluid""#);
    assert_scaffold_parses("link", r#""primitive": "link""#);
}
