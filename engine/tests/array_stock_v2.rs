//! Dimensioned (array-valued) stocks. Two gaps are now closed:
//!   • a scalar `initial_value` on a dimensioned stock broadcasts to EVERY cell (was: cell 0 only,
//!     the rest zero-filled);
//!   • a stock driven by inflows/outflows integrates the flows PER CELL (was: flows collapsed to
//!     their cell-0 value via `as_scalar`, so the stock could never stay dimensioned).

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig, SimulationResults};

fn member_final(r: &SimulationResults, id: &str, k: usize) -> f64 {
    r.elements.get(&format!("{id}#{k}")).map(|e| e.final_values[0]).unwrap_or_else(|| panic!("missing {id}#{k}"))
}

// A dimensioned stock with a SCALAR initial (5), no flows: every cell must read 5, not [5,0,0].
const SCALAR_INIT_BROADCAST: &str = r#"{
  "wasim_version":"0.9.9",
  "simulation_settings":{"duration":{"value":2,"unit":"d"},"timestep":{"value":1,"unit":"d"},"n_realizations":1},
  "dimensions":[{"id":"F","name":"F","size":3}],
  "elements":[
    {"id":"S","name":"S","primitive":"stock","initial_value":{"value":5,"unit":"1"},"inflows":[],"outflows":[],
     "outputs":[{"name":"S","unit":"1","dimensions":["F"]}],"save_results":{"final_value":true,"time_history":true}}
  ]
}"#;

#[test]
fn scalar_initial_broadcasts_across_cells() {
    let m = parse_v2(SCALAR_INIT_BROADCAST).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    let r = run_v2(&m, &g, &RunConfig::default()).expect("run");
    assert_eq!(
        [member_final(&r, "S", 1), member_final(&r, "S", 2), member_final(&r, "S", 3)],
        [5.0, 5.0, 5.0],
        "a scalar initial must fill every cell of a dimensioned stock",
    );
}

// A dimensioned stock integrating a per-cell inflow rate=[1,2,3] over 3 days → [3,6,9].
const FLOW_DRIVEN_PER_CELL: &str = r#"{
  "wasim_version":"0.9.9",
  "simulation_settings":{"duration":{"value":3,"unit":"d"},"timestep":{"value":1,"unit":"d"},"n_realizations":1},
  "dimensions":[{"id":"F","name":"F","size":3}],
  "elements":[
    {"id":"rate","name":"rate","primitive":"node","value_rule":"expression",
     "expression":{"ast":{"op":"vector_map","over":"F","body":{"op":"index_ref","depth":0}}},
     "outputs":[{"name":"rate","unit":"1","dimensions":["F"]}]},
    {"id":"damage","name":"damage","primitive":"stock","inputs":["rate"],
     "initial_value":{"value":0,"unit":"1"},"inflows":["rate"],"outflows":[],
     "outputs":[{"name":"damage","unit":"1","dimensions":["F"]}],"save_results":{"final_value":true,"time_history":true}}
  ]
}"#;

#[test]
fn flow_driven_stock_integrates_per_cell() {
    let m = parse_v2(FLOW_DRIVEN_PER_CELL).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    let r = run_v2(&m, &g, &RunConfig::default()).expect("run");
    // member k (1-based) integrates rate=k over 3 steps → 3k. Was: all cells but #1 stuck at 0.
    assert_eq!(member_final(&r, "damage", 1), 3.0, "cell 1 = 1*3");
    assert_eq!(member_final(&r, "damage", 2), 6.0, "cell 2 = 2*3 (per-cell inflow, not collapsed)");
    assert_eq!(member_final(&r, "damage", 3), 9.0, "cell 3 = 3*3");
}
