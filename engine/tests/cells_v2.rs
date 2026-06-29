//! Cell primitive tests (v2-native): source_release (finite inventory), first-order decay,
//! and decay-chain ingrowth. Per-(cell, species) mass is exposed as "<cell>:<species>".

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig, SimulationResults};

fn run(json: &str) -> SimulationResults {
    let m = parse_v2(json).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    let cfg = RunConfig { n_realizations: Some(1), seed: Some(1), duration_override: None, timestep_override: None };
    run_v2(&m, &g, &cfg).expect("run")
}

fn hist(r: &SimulationResults, id: &str) -> Vec<f64> {
    r.elements[id].time_history.as_ref().unwrap_or_else(|| panic!("no history for {id}")).mean.clone()
}

fn close(a: &[f64], b: &[f64]) {
    assert_eq!(a.len(), b.len(), "{a:?} vs {b:?}");
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        assert!((x - y).abs() < 1e-9, "[{i}] {x} vs {y} in {a:?}");
    }
}

#[test]
fn source_release_depletes_finite_inventory() {
    // Source releases 10/d of A into the sink, from a finite inventory of 30 (3 steps' worth).
    let r = run(
        r#"{"wasim_version": "0.8.0",
        "simulation_settings": {"duration": {"value": 5, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 1},
        "elements": [
          {"id": "A", "name": "A", "primitive": "species"},
          {"id": "src", "name": "Src", "primitive": "cell", "species": [{"species": "A"}],
           "inventory": {"value": 30, "unit": "kg"}, "release_rate": {"value": 10, "unit": "kg/d"}, "release_target": "sink"},
          {"id": "sink", "name": "Sink", "primitive": "cell", "species": [{"species": "A", "initial_inventory": {"value": 0, "unit": "kg"}}],
           "save_results": {"time_history": true}}
        ]}"#,
    );
    close(&hist(&r, "sink:A"), &[10.0, 20.0, 30.0, 30.0, 30.0]);
}

#[test]
fn first_order_decay() {
    // Tc with half-life 1 d → mass halves each step: 100 → 50 → 25 → 12.5.
    let r = run(
        r#"{"wasim_version": "0.8.0",
        "simulation_settings": {"duration": {"value": 4, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 1},
        "elements": [
          {"id": "Tc", "name": "Tc", "primitive": "species", "half_life": {"value": 1, "unit": "d"}},
          {"id": "C", "name": "C", "primitive": "cell", "species": [{"species": "Tc", "initial_inventory": {"value": 100, "unit": "kg"}}],
           "save_results": {"time_history": true}}
        ]}"#,
    );
    close(&hist(&r, "C:Tc"), &[50.0, 25.0, 12.5, 6.25]);
}

#[test]
fn decay_chain_ingrowth_conserves_mass() {
    // P (half-life 1 d) → D (stable). P halves; D ingrows; total stays 100.
    let r = run(
        r#"{"wasim_version": "0.8.0",
        "simulation_settings": {"duration": {"value": 3, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 1},
        "elements": [
          {"id": "P", "name": "P", "primitive": "species", "half_life": {"value": 1, "unit": "d"},
           "decay_products": [{"species": "D", "branching_fraction": 1.0}]},
          {"id": "D", "name": "D", "primitive": "species"},
          {"id": "C", "name": "C", "primitive": "cell",
           "species": [{"species": "P", "initial_inventory": {"value": 100, "unit": "kg"}}, {"species": "D", "initial_inventory": {"value": 0, "unit": "kg"}}],
           "save_results": {"time_history": true}}
        ]}"#,
    );
    close(&hist(&r, "C:P"), &[50.0, 25.0, 12.5]);
    close(&hist(&r, "C:D"), &[50.0, 75.0, 87.5]);
    close(&hist(&r, "C"), &[100.0, 100.0, 100.0]); // total mass conserved
}
