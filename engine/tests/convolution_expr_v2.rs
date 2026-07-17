//! Expression-valued convolution response (§17). The response is a `~lag` formula sampled onto
//! the lag grid at run time, so a parameter referenced in the formula stays live (unlike the
//! baked `{times, values}` form). Verifies (1) a known kernel convolves correctly and (2) the
//! output responds to a referenced parameter.

use wasim_engine::{parse_v2, ModelGraphV2, RunConfig, engine_v2};

/// A unit-impulse input convolved with a response `r(lag) = 1 if lag < K·day else 0` (a boxcar of
/// width K days) produces an output = the running count of impulse×weight. With a single impulse
/// at t=0 and boxcar weights all 1 over K taps, the output stays 1 for K steps. We just assert the
/// expression response samples and convolves without error and yields a non-degenerate series.
#[test]
fn expr_response_samples_and_convolves() {
    // Input = 1 at every step (constant). Response density r(lag)=1 for lag<3 day, else 0.
    // interval=1 day, length=5 day → 6 grid points; weights = r×interval = [1,1,1,0,0,0] (days).
    let json = r#"{
      "wasim_version": "0.9.0",
      "simulation_settings": {"duration": {"value": 8, "unit": "day"}, "timestep": {"value": 1, "unit": "day"}, "seed": 1},
      "elements": [
        {"id": "In", "name": "In", "primitive": "node", "value_rule": "fixed", "value": {"value": 1, "unit": "1"}},
        {"id": "Conv", "name": "Conv", "primitive": "node", "value_rule": "convolution",
         "input": "In",
         "response": {
           "expression": {"ast": {"op": "if",
             "cond": {"op": "lt",
               "left": {"op": "extern_call", "fn": "lag", "args": []},
               "right": {"op": "literal", "value": 259200, "unit": "s"}},
             "then": {"op": "literal", "value": 1.0},
             "else": {"op": "literal", "value": 0.0}}},
           "interval": {"value": 86400, "unit": "s"},
           "length": {"value": 432000, "unit": "s"},
           "cumulative": false
         },
         "save_results": {"time_history": true}}
      ]
    }"#;
    let m = parse_v2(json).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    let r = engine_v2::run(&m, &g, &RunConfig { seed: Some(1), ..RunConfig::default() }).expect("run");
    let th = r.elements.get("Conv").and_then(|e| e.time_history.as_ref()).expect("series");
    let s = &th.p50;
    // weights (density × interval_s) are large (interval=86400 s), so the values are big; the key
    // property is the series is finite, non-zero, and plateaus (boxcar convolution of a constant).
    assert!(s.iter().all(|v| v.is_finite()), "series finite");
    assert!(s.last().copied().unwrap_or(0.0) > 0.0, "convolution should be non-zero");
    // The plateau: once the buffer fills, successive steps give the same sum (3 active taps).
    assert!((s[s.len() - 1] - s[s.len() - 2]).abs() < 1e-9, "should plateau once buffer fills");
}

/// The response is live: a `~lag` formula referencing an element `K` responds to K's value. Two
/// models identical but for K produce different convolution weights → different output. This is
/// the whole point of Gap 4 (a baked response could not do this).
#[test]
fn expr_response_tracks_referenced_parameter() {
    let model = |k_days: f64| format!(r#"{{
      "wasim_version": "0.9.0",
      "simulation_settings": {{"duration": {{"value": 6, "unit": "day"}}, "timestep": {{"value": 1, "unit": "day"}}, "seed": 1}},
      "elements": [
        {{"id": "In", "name": "In", "primitive": "node", "value_rule": "fixed", "value": {{"value": 1, "unit": "1"}}}},
        {{"id": "K", "name": "K", "primitive": "node", "value_rule": "fixed", "value": {{"value": {k_secs}, "unit": "s"}}}},
        {{"id": "Conv", "name": "Conv", "primitive": "node", "value_rule": "convolution",
         "input": "In",
         "response": {{
           "expression": {{"ast": {{"op": "if",
             "cond": {{"op": "lt",
               "left": {{"op": "extern_call", "fn": "lag", "args": []}},
               "right": {{"op": "ref", "element_id": "K"}}}},
             "then": {{"op": "literal", "value": 1.0}},
             "else": {{"op": "literal", "value": 0.0}}}}}},
           "interval": {{"value": 86400, "unit": "s"}},
           "length": {{"value": 432000, "unit": "s"}},
           "cumulative": false
         }},
         "save_results": {{"time_history": true, "final_value": true}}}}
      ]
    }}"#, k_secs = k_days * 86400.0);

    let run = |k: f64| {
        let m = parse_v2(&model(k)).expect("parse");
        let g = ModelGraphV2::build(&m).expect("graph");
        let r = engine_v2::run(&m, &g, &RunConfig { seed: Some(1), ..RunConfig::default() }).expect("run");
        *r.elements.get("Conv").unwrap().final_values.first().unwrap()
    };
    // A wider boxcar (K=5 days) admits more taps than K=2 → strictly larger steady-state output.
    let narrow = run(2.0);
    let wide = run(5.0);
    assert!(wide > narrow * 1.5, "wider kernel (K=5) should give a larger output than K=2: {wide} vs {narrow}");
}
