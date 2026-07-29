//! Array-language layer, Phase 1 (WASIM_ARRAY_LANGUAGE_SCOPE.md): the shape-preserving
//! array→array builtins `sort_array` / `sort_index` / `rank_array` / `cumulate` /
//! `cumproduct`. Verified on both the scalar lane and the dimensioned array lane (which
//! reuses `eval_ast`, so the two must agree bit-for-bit), including a tie to exercise
//! the stable lowest-source-index tie-break.

use wasim_engine::{array_lane, parse_v2, run_v2, ModelGraphV2, RunConfig};

// x = [30, 10, 20, 10, 50] over a 5-wide dimension. The two 10s (positions 2 and 4,
// 1-based) test that ties keep source order.
const MODEL: &str = r#"{
  "wasim_version": "0.9.7",
  "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 1, "seed": 1},
  "dimensions": [{"id":"D","name":"D","size":5}],
  "containers": [{"id":"M","name":"M","elements":["M/x","M/sorted","M/perm","M/rank","M/cum","M/cumprod"]}],
  "elements": [
    {"id":"M/x","name":"x","primitive":"node","value_rule":"fixed","container":"M","values":[30,10,20,10,50],"unit":"1","outputs":[{"name":"x","unit":"1","dimensions":["D"]}],"save_results":{"final_value":true}},
    {"id":"M/sorted","name":"sorted","primitive":"node","value_rule":"expression","container":"M","inputs":["M/x"],
     "expression":{"ast":{"op":"call","fn":"sort_array","args":[{"op":"ref","element_id":"M/x"}]}},"outputs":[{"name":"s","unit":"1","dimensions":["D"]}],"save_results":{"final_value":true}},
    {"id":"M/perm","name":"perm","primitive":"node","value_rule":"expression","container":"M","inputs":["M/x"],
     "expression":{"ast":{"op":"call","fn":"sort_index","args":[{"op":"ref","element_id":"M/x"}]}},"outputs":[{"name":"p","unit":"1","dimensions":["D"]}],"save_results":{"final_value":true}},
    {"id":"M/rank","name":"rank","primitive":"node","value_rule":"expression","container":"M","inputs":["M/x"],
     "expression":{"ast":{"op":"call","fn":"rank_array","args":[{"op":"ref","element_id":"M/x"}]}},"outputs":[{"name":"r","unit":"1","dimensions":["D"]}],"save_results":{"final_value":true}},
    {"id":"M/cum","name":"cum","primitive":"node","value_rule":"expression","container":"M","inputs":["M/x"],
     "expression":{"ast":{"op":"call","fn":"cumulate","args":[{"op":"ref","element_id":"M/x"}]}},"outputs":[{"name":"c","unit":"1","dimensions":["D"]}],"save_results":{"final_value":true}},
    {"id":"M/cumprod","name":"cumprod","primitive":"node","value_rule":"expression","container":"M","inputs":["M/x"],
     "expression":{"ast":{"op":"call","fn":"cumproduct","args":[{"op":"ref","element_id":"M/x"}]}},"outputs":[{"name":"cp","unit":"1","dimensions":["D"]}],"save_results":{"final_value":true}}
  ]
}"#;

fn members(r: &wasim_engine::SimulationResults, id: &str, n: usize) -> Vec<f64> {
    (1..=n).map(|k| r.elements[&format!("{id}#{k}")].final_values[0]).collect()
}

#[test]
fn phase1_ops_match_expected_on_both_lanes() {
    let m = parse_v2(MODEL).unwrap();
    assert!(array_lane::eligible(&m).is_ok(), "array-language ops should be dim-lane-eligible");
    let g = ModelGraphV2::build(&m).unwrap();
    let scalar = run_v2(&m, &g, &RunConfig { array_lane: false, ..Default::default() }).unwrap();
    let array = run_v2(&m, &g, &RunConfig { array_lane: true, ..Default::default() }).unwrap();

    // Scalar-lane values are exactly the hand-computed results.
    assert_eq!(members(&scalar, "M/sorted", 5), vec![10.0, 10.0, 20.0, 30.0, 50.0]);
    assert_eq!(members(&scalar, "M/perm", 5), vec![2.0, 4.0, 3.0, 1.0, 5.0]);   // stable tie: 10@2 before 10@4
    assert_eq!(members(&scalar, "M/rank", 5), vec![4.0, 1.0, 3.0, 2.0, 5.0]);
    assert_eq!(members(&scalar, "M/cum", 5), vec![30.0, 40.0, 60.0, 70.0, 120.0]);
    assert_eq!(members(&scalar, "M/cumprod", 5), vec![30.0, 300.0, 6000.0, 60000.0, 3_000_000.0]);

    // The dimensioned array lane reuses eval_ast → must be bit-identical.
    for id in ["M/sorted", "M/perm", "M/rank", "M/cum", "M/cumprod"] {
        for k in 1..=5 {
            let key = format!("{id}#{k}");
            assert_eq!(scalar.elements[&key].final_values[0].to_bits(),
                       array.elements[&key].final_values[0].to_bits(),
                       "{key}: array lane != scalar lane");
        }
    }
}

// The case-study payoff: the Marginal Abatement ranking, computed natively. `sort_index`
// of the marginal cost gives Analytica's `Sorted_action` exactly.
#[test]
fn marginal_abatement_sort_index_matches_analytica() {
    // marginal cost per action = (gross - amount*50)/amount, computed in-engine.
    let json = r#"{
      "wasim_version": "0.9.7",
      "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 1, "seed": 1},
      "dimensions": [{"id":"Action","name":"Action","size":8}],
      "containers": [{"id":"M","name":"M","elements":["M/amount","M/gross","M/net","M/marg","M/sorted"]}],
      "elements": [
        {"id":"M/amount","name":"amount","primitive":"node","value_rule":"fixed","container":"M","values":[8,15,10,20,5,5,35,50],"unit":"1","outputs":[{"name":"a","unit":"1","dimensions":["Action"]}]},
        {"id":"M/gross","name":"gross","primitive":"node","value_rule":"fixed","container":"M","values":[20,1000,400,4000,2000,250,5500,3000],"unit":"1","outputs":[{"name":"g","unit":"1","dimensions":["Action"]}]},
        {"id":"M/net","name":"net","primitive":"node","value_rule":"expression","container":"M","inputs":["M/gross","M/amount"],
         "expression":{"ast":{"op":"subtract","left":{"op":"ref","element_id":"M/gross"},"right":{"op":"multiply","left":{"op":"ref","element_id":"M/amount"},"right":{"op":"literal","value":50}}}},"outputs":[{"name":"n","unit":"1","dimensions":["Action"]}]},
        {"id":"M/marg","name":"marg","primitive":"node","value_rule":"expression","container":"M","inputs":["M/net","M/amount"],
         "expression":{"ast":{"op":"divide","left":{"op":"ref","element_id":"M/net"},"right":{"op":"ref","element_id":"M/amount"}}},"outputs":[{"name":"m","unit":"1","dimensions":["Action"]}]},
        {"id":"M/sorted","name":"sorted","primitive":"node","value_rule":"expression","container":"M","inputs":["M/marg"],
         "expression":{"ast":{"op":"call","fn":"sort_index","args":[{"op":"ref","element_id":"M/marg"}]}},"outputs":[{"name":"s","unit":"1","dimensions":["Action"]}],"save_results":{"final_value":true}}
      ]
    }"#;
    let m = parse_v2(json).unwrap();
    let g = ModelGraphV2::build(&m).unwrap();
    let r = run_v2(&m, &g, &RunConfig::default()).unwrap();
    // Analytica Sorted_action (1-based action positions, by increasing marginal cost):
    // Weather(1), Ceiling(3), Thermostat(6), Windows(8), Door(2), Furnace(7), Wall(4), Subfloor(5)
    assert_eq!(members(&r, "M/sorted", 8), vec![1.0, 3.0, 6.0, 8.0, 2.0, 7.0, 4.0, 5.0]);
}

// Phase 2: gather (reindex-by-index) and ordinal (standalone @), on both lanes.
// x = [10,20,30,40,50], idx = [3,1,5,2,4] → gather picks x[idx[k]-1].
const GATHER: &str = r#"{
  "wasim_version": "0.9.7",
  "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 1, "seed": 1},
  "dimensions": [{"id":"D","name":"D","size":5}],
  "containers": [{"id":"M","name":"M","elements":["M/x","M/idx","M/g","M/o"]}],
  "elements": [
    {"id":"M/x","name":"x","primitive":"node","value_rule":"fixed","container":"M","values":[10,20,30,40,50],"unit":"1","outputs":[{"name":"x","unit":"1","dimensions":["D"]}],"save_results":{"final_value":true}},
    {"id":"M/idx","name":"idx","primitive":"node","value_rule":"fixed","container":"M","values":[3,1,5,2,4],"unit":"1","outputs":[{"name":"i","unit":"1","dimensions":["D"]}],"save_results":{"final_value":true}},
    {"id":"M/g","name":"g","primitive":"node","value_rule":"expression","container":"M","inputs":["M/x","M/idx"],
     "expression":{"ast":{"op":"call","fn":"gather","args":[{"op":"ref","element_id":"M/x"},{"op":"ref","element_id":"M/idx"}]}},"outputs":[{"name":"g","unit":"1","dimensions":["D"]}],"save_results":{"final_value":true}},
    {"id":"M/o","name":"o","primitive":"node","value_rule":"expression","container":"M","inputs":["M/x"],
     "expression":{"ast":{"op":"call","fn":"ordinal","args":[{"op":"ref","element_id":"M/x"}]}},"outputs":[{"name":"o","unit":"1","dimensions":["D"]}],"save_results":{"final_value":true}}
  ]
}"#;

#[test]
fn gather_and_ordinal_match_on_both_lanes() {
    let m = parse_v2(GATHER).unwrap();
    assert!(array_lane::eligible(&m).is_ok());
    let g = ModelGraphV2::build(&m).unwrap();
    let scalar = run_v2(&m, &g, &RunConfig { array_lane: false, ..Default::default() }).unwrap();
    let array = run_v2(&m, &g, &RunConfig { array_lane: true, ..Default::default() }).unwrap();

    assert_eq!(members(&scalar, "M/g", 5), vec![30.0, 10.0, 50.0, 20.0, 40.0]); // x[idx[k]-1]
    assert_eq!(members(&scalar, "M/o", 5), vec![1.0, 2.0, 3.0, 4.0, 5.0]);
    for id in ["M/g", "M/o"] {
        for k in 1..=5 {
            let key = format!("{id}#{k}");
            assert_eq!(scalar.elements[&key].final_values[0].to_bits(),
                       array.elements[&key].final_values[0].to_bits(), "{key}: lanes differ");
        }
    }
}

// Phase 3: null() → NaN, and it broadcasts through the vector-cond `if … else null()`
// (the staircase-gap idiom) so every false cell is NaN, not 0. Both lanes.
const NULLS: &str = r#"{
  "wasim_version": "0.9.7",
  "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 1, "seed": 1},
  "dimensions": [{"id":"D","name":"D","size":5}],
  "containers": [{"id":"M","name":"M","elements":["M/x","M/y"]}],
  "elements": [
    {"id":"M/x","name":"x","primitive":"node","value_rule":"fixed","container":"M","values":[-1,2,-3,4,-5],"unit":"1","outputs":[{"name":"x","unit":"1","dimensions":["D"]}],"save_results":{"final_value":true}},
    {"id":"M/y","name":"y","primitive":"node","value_rule":"expression","container":"M","inputs":["M/x"],
     "expression":{"ast":{"op":"if","cond":{"op":"gt","left":{"op":"ref","element_id":"M/x"},"right":{"op":"literal","value":0}},
        "then":{"op":"ref","element_id":"M/x"},
        "else":{"op":"call","fn":"null","args":[]}}},"outputs":[{"name":"y","unit":"1","dimensions":["D"]}],"save_results":{"final_value":true}}
  ]
}"#;

#[test]
fn null_broadcasts_as_nan_gaps_on_both_lanes() {
    let m = parse_v2(NULLS).unwrap();
    assert!(array_lane::eligible(&m).is_ok());
    let g = ModelGraphV2::build(&m).unwrap();
    let scalar = run_v2(&m, &g, &RunConfig { array_lane: false, ..Default::default() }).unwrap();
    let array = run_v2(&m, &g, &RunConfig { array_lane: true, ..Default::default() }).unwrap();
    let y = members(&scalar, "M/y", 5);
    assert!(y[0].is_nan() && y[2].is_nan() && y[4].is_nan(), "false cells must be NaN: {y:?}");
    assert_eq!(y[1], 2.0);
    assert_eq!(y[3], 4.0);
    for k in 1..=5 { // NaN has a canonical bit pattern; both lanes must match
        let key = format!("M/y#{k}");
        assert_eq!(scalar.elements[&key].final_values[0].to_bits(),
                   array.elements[&key].final_values[0].to_bits(), "{key}: lanes differ");
    }
}

// The Phase 2 payoff: Marginal Abatement `Cum_reduction` computed natively end-to-end —
// cumulate(gather(amount, sort_index(marg))) — matching Analytica exactly.
#[test]
fn marginal_abatement_cumulative_reduction_matches_analytica() {
    let json = r#"{
      "wasim_version": "0.9.7",
      "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 1, "seed": 1},
      "dimensions": [{"id":"Action","name":"Action","size":8}],
      "containers": [{"id":"M","name":"M","elements":["M/amount","M/gross","M/net","M/marg","M/sorted","M/amt_sorted","M/cum"]}],
      "elements": [
        {"id":"M/amount","name":"amount","primitive":"node","value_rule":"fixed","container":"M","values":[8,15,10,20,5,5,35,50],"unit":"1","outputs":[{"name":"a","unit":"1","dimensions":["Action"]}]},
        {"id":"M/gross","name":"gross","primitive":"node","value_rule":"fixed","container":"M","values":[20,1000,400,4000,2000,250,5500,3000],"unit":"1","outputs":[{"name":"g","unit":"1","dimensions":["Action"]}]},
        {"id":"M/net","name":"net","primitive":"node","value_rule":"expression","container":"M","inputs":["M/gross","M/amount"],
         "expression":{"ast":{"op":"subtract","left":{"op":"ref","element_id":"M/gross"},"right":{"op":"multiply","left":{"op":"ref","element_id":"M/amount"},"right":{"op":"literal","value":50}}}},"outputs":[{"name":"n","unit":"1","dimensions":["Action"]}]},
        {"id":"M/marg","name":"marg","primitive":"node","value_rule":"expression","container":"M","inputs":["M/net","M/amount"],
         "expression":{"ast":{"op":"divide","left":{"op":"ref","element_id":"M/net"},"right":{"op":"ref","element_id":"M/amount"}}},"outputs":[{"name":"m","unit":"1","dimensions":["Action"]}]},
        {"id":"M/sorted","name":"sorted","primitive":"node","value_rule":"expression","container":"M","inputs":["M/marg"],
         "expression":{"ast":{"op":"call","fn":"sort_index","args":[{"op":"ref","element_id":"M/marg"}]}},"outputs":[{"name":"s","unit":"1","dimensions":["Action"]}]},
        {"id":"M/amt_sorted","name":"amt_sorted","primitive":"node","value_rule":"expression","container":"M","inputs":["M/amount","M/sorted"],
         "expression":{"ast":{"op":"call","fn":"gather","args":[{"op":"ref","element_id":"M/amount"},{"op":"ref","element_id":"M/sorted"}]}},"outputs":[{"name":"as","unit":"1","dimensions":["Action"]}]},
        {"id":"M/cum","name":"cum","primitive":"node","value_rule":"expression","container":"M","inputs":["M/amt_sorted"],
         "expression":{"ast":{"op":"call","fn":"cumulate","args":[{"op":"ref","element_id":"M/amt_sorted"}]}},"outputs":[{"name":"c","unit":"1","dimensions":["Action"]}],"save_results":{"final_value":true}}
      ]
    }"#;
    let m = parse_v2(json).unwrap();
    let g = ModelGraphV2::build(&m).unwrap();
    // Both lanes; the whole chain is dimensioned array-language ops.
    let scalar = run_v2(&m, &g, &RunConfig { array_lane: false, ..Default::default() }).unwrap();
    let array = run_v2(&m, &g, &RunConfig { array_lane: true, ..Default::default() }).unwrap();
    // Analytica Cum_reduction: cumulative MBTU as the cheapest actions are done first.
    assert_eq!(members(&scalar, "M/cum", 8), vec![8.0, 18.0, 23.0, 73.0, 88.0, 123.0, 143.0, 148.0]);
    for k in 1..=8 {
        let key = format!("M/cum#{k}");
        assert_eq!(scalar.elements[&key].final_values[0].to_bits(),
                   array.elements[&key].final_values[0].to_bits(), "{key}: lanes differ");
    }
}
