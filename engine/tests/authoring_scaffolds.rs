//! Verifies that the JSON shapes the web authoring environment emits (palette scaffolds in
//! `frontend/src/model/edits.ts`, and the reconcile round-trip) parse through the real v2
//! parser and build a graph — i.e. what the Inspector/Palette produce is engine-valid.

use wasim_engine::graph_v2::ModelGraphV2;
use wasim_engine::v2_parse;

fn parse_ok(json: &str) -> Result<(), String> {
    let model = v2_parse::parse(json).map_err(|e| format!("parse: {e}"))?;
    ModelGraphV2::build(&model).map_err(|e| format!("graph: {e}"))?;
    Ok(())
}

const SETTINGS: &str = r#""wasim_version":"0.1.0","simulation_settings":{"duration":{"value":100,"unit":"s"},"timestep":{"value":1,"unit":"s"},"n_realizations":1,"seed":42}"#;

fn model(elements: &str) -> String {
    format!("{{{SETTINGS},\"containers\":[],\"elements\":[{elements}]}}")
}

#[test]
fn blank_model_with_one_constant_parses() {
    let el = r#"{"id":"c","name":"Constant","primitive":"node","value_rule":"fixed","value":{"value":0,"unit":"1"},"editable":true,"bounds":{"min":0,"max":1}}"#;
    parse_ok(&model(el)).unwrap();
}

#[test]
fn stochastic_scaffold_parses() {
    let el = r#"{"id":"s","name":"Stochastic","primitive":"node","value_rule":"sample","distribution":{"family":"normal","parameters":{"mean":{"value":0,"unit":"1"},"stddev":{"value":1,"unit":"1"}}}}"#;
    parse_ok(&model(el)).unwrap();
}

#[test]
fn timeseries_scaffold_parses() {
    let el = r#"{"id":"ts","name":"Time Series","primitive":"node","value_rule":"series","timestamps":[0,1],"values":[0,0],"time_unit":"s","interpolation":"linear"}"#;
    parse_ok(&model(el)).unwrap();
}

#[test]
fn lookup_scaffold_parses() {
    let el = r#"{"id":"lk","name":"Lookup","primitive":"node","value_rule":"lookup","table":{"x":[0,1],"y":[0,1],"interpolation":"linear"}}"#;
    parse_ok(&model(el)).unwrap();
}

#[test]
fn expression_scaffold_parses() {
    let el = r#"{"id":"e","name":"Expression","primitive":"node","value_rule":"expression","expression":{"ast":{"op":"literal","value":0},"display":"0"},"inputs":[]}"#;
    parse_ok(&model(el)).unwrap();
}

#[test]
fn lag_scaffold_parses() {
    // Palette emits input:null; the parser must tolerate a not-yet-wired lag.
    let el = r#"{"id":"lg","name":"Lag","primitive":"node","value_rule":"lag","input":null,"initial":{"value":0,"unit":"1"}}"#;
    parse_ok(&model(el)).unwrap();
}

#[test]
fn stock_scaffold_parses() {
    let el = r#"{"id":"st","name":"Stock","primitive":"stock","initial_value":{"value":0,"unit":"1"},"rate":{"ast":{"op":"literal","value":0},"display":"0"},"inflows":[],"outflows":[]}"#;
    parse_ok(&model(el)).unwrap();
}

#[test]
fn expression_referencing_constant_builds_influence() {
    // The influence-graph case: an expression that references a constant (what the
    // ExpressionEditor produces after autocomplete). inputs must resolve.
    let els = r#"{"id":"a","name":"A","primitive":"node","value_rule":"fixed","value":{"value":2,"unit":"1"},"editable":true},{"id":"b","name":"B","primitive":"node","value_rule":"expression","expression":{"ast":{"op":"multiply","left":{"op":"ref","element_id":"a"},"right":{"op":"literal","value":3}},"display":"a × 3"},"inputs":["a"]}"#;
    parse_ok(&model(els)).unwrap();
}

#[test]
fn stock_with_expression_rate_referencing_input() {
    let els = r#"{"id":"inflow","name":"Inflow","primitive":"node","value_rule":"fixed","value":{"value":5,"unit":"1"},"editable":true},{"id":"tank","name":"Tank","primitive":"stock","initial_value":{"value":0,"unit":"1"},"rate":{"ast":{"op":"ref","element_id":"inflow"},"display":"inflow"},"inflows":[],"outflows":[],"inputs":["inflow"]}"#;
    parse_ok(&model(els)).unwrap();
}

// ── Optimization UI round-trip ──────────────────────────────────────────────────

#[test]
fn optimization_spec_from_ui_solves_quadratic() {
    // Minimize (x-3)^2 over x ∈ [0,10]; the OptimizationSpec shape mirrors what the
    // OptimizationTab emits (objective + variables with lower/upper/initial quantities).
    let els = r#"{"id":"x","name":"x","primitive":"node","value_rule":"fixed","value":{"value":5,"unit":"1"},"editable":true,"bounds":{"min":0,"max":10}},{"id":"obj","name":"obj","primitive":"node","value_rule":"expression","inputs":["x"],"expression":{"ast":{"op":"multiply","left":{"op":"subtract","left":{"op":"ref","element_id":"x"},"right":{"op":"literal","value":3}},"right":{"op":"subtract","left":{"op":"ref","element_id":"x"},"right":{"op":"literal","value":3}}},"display":"(x-3)*(x-3)"}}"#;
    let opt = r#""optimization":{"objective":{"element_id":"obj","direction":"minimize","statistic":null},"variables":[{"element_id":"x","lower":{"value":0,"unit":"1"},"upper":{"value":10,"unit":"1"},"initial":{"value":5,"unit":"1"}}],"constraints":[]}"#;
    let json = format!("{{{SETTINGS},{opt},\"containers\":[],\"elements\":[{els}]}}");

    let model = wasim_engine::v2_parse::parse(&json).expect("parse");
    let results = wasim_engine::optimize_v2::optimize(&model, &wasim_engine::engine::RunConfig::default())
        .expect("optimize");
    let x = results.variables.iter().find(|v| v.element_id == "x").unwrap().value;
    assert!((x - 3.0).abs() < 0.1, "expected x≈3, got {x}");
    assert!(results.objective < 0.05, "expected objective≈0, got {}", results.objective);
}
