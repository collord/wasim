//! S2 — cell concentration output (gap analysis Rev 2 §5 / §2.8). Cells publish mass (primary,
//! unchanged) plus, when the cell has a bulk `volume`, a per-(cell,species,medium) concentration
//! under the result id `"<cell>:<species>@<medium>:C"`:
//!
//!     C = mass / (cell_volume · medium_fraction · medium_porosity)
//!
//! Concentration is additive — the mass outputs are byte-for-byte unchanged (cells_v2 stays green),
//! and cells without a volume emit mass only. NOTE: no corpus model currently carries cell
//! `volume`/`porosity` (cells are emitted as bare stubs — the emitter cell-body decode is the
//! separate corpus blocker), so these are synthetic fixtures exercising the engine path directly.

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig, SimulationResults};

fn run(json: &str) -> SimulationResults {
    let m = parse_v2(json).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    let cfg = RunConfig { n_realizations: Some(1), seed: Some(1), ..RunConfig::default() };
    run_v2(&m, &g, &cfg).expect("run")
}

fn hist(r: &SimulationResults, id: &str) -> Vec<f64> {
    let el = r.elements.get(id).unwrap_or_else(|| {
        let mut keys: Vec<&str> = r.elements.keys().map(|s| s.as_str()).collect();
        keys.sort();
        panic!("no element '{id}'; have: {keys:?}")
    });
    el.time_history.as_ref().unwrap_or_else(|| panic!("no history for {id}")).mean.clone()
}

fn close(a: &[f64], b: &[f64]) {
    assert_eq!(a.len(), b.len(), "{a:?} vs {b:?}");
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        assert!((x - y).abs() < 1e-9, "[{i}] {x} vs {y} in {a:?}");
    }
}

/// Single cell, stable species, one medium: C = mass / (V · fraction · porosity). Mass 100,
/// V = 10, fraction 1.0, porosity 0.5 → pore volume 5 → C = 20. Mass output is unchanged.
#[test]
fn single_cell_concentration() {
    let r = run(
        r#"{"wasim_version": "0.9.7",
        "simulation_settings": {"duration": {"value": 3, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 1},
        "elements": [
          {"id": "X", "name": "X", "primitive": "species"},
          {"id": "water", "name": "Water", "primitive": "medium", "phase": "fluid", "porosity": {"value": 0.5, "unit": "1"}},
          {"id": "C", "name": "C", "primitive": "cell",
           "volume": {"value": 10, "unit": "m3"},
           "media": [{"medium": "water", "fraction": {"value": 1.0, "unit": "1"}}],
           "species": [{"species": "X", "initial_inventory": {"value": 100, "unit": "kg"}}],
           "save_results": {"time_history": true}}
        ]}"#,
    );
    close(&hist(&r, "C:X@water"), &[100.0, 100.0, 100.0]); // mass unchanged
    close(&hist(&r, "C:X@water:C"), &[20.0, 20.0, 20.0]);  // concentration = 100/(10·1·0.5)
}

/// Porosity defaults to 1.0 when the medium omits it: C = mass / (V · fraction).
#[test]
fn porosity_defaults_to_one() {
    let r = run(
        r#"{"wasim_version": "0.9.7",
        "simulation_settings": {"duration": {"value": 2, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 1},
        "elements": [
          {"id": "X", "name": "X", "primitive": "species"},
          {"id": "bulk", "name": "Bulk", "primitive": "medium", "phase": "fluid"},
          {"id": "C", "name": "C", "primitive": "cell",
           "volume": {"value": 4, "unit": "m3"},
           "media": [{"medium": "bulk", "fraction": {"value": 1.0, "unit": "1"}}],
           "species": [{"species": "X", "initial_inventory": {"value": 40, "unit": "kg"}}],
           "save_results": {"time_history": true}}
        ]}"#,
    );
    close(&hist(&r, "C:X@bulk:C"), &[10.0, 10.0]); // 40/(4·1·1) = 10
}

/// Multi-medium partition (Kd): each medium's concentration is its partitioned mass ÷ that
/// medium's pore volume. Reuses the partitioning fixture from cells_v2 (solid 80 / fluid 20 at
/// fractions 0.5), with V = 10: C_solid = 80/(10·0.5·φ_solid), C_fluid = 20/(10·0.5·φ_fluid).
/// With φ_solid = 0.2, φ_fluid = 1.0: C_solid = 80/1 = 80, C_fluid = 20/5 = 4.
#[test]
fn multi_medium_partition_concentration() {
    let r = run(
        r#"{"wasim_version": "0.9.7",
        "simulation_settings": {"duration": {"value": 2, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 1},
        "elements": [
          {"id": "solid", "name": "Solid", "primitive": "medium", "phase": "solid", "porosity": {"value": 0.2, "unit": "1"}},
          {"id": "fluid", "name": "Fluid", "primitive": "medium", "phase": "fluid", "porosity": {"value": 1.0, "unit": "1"}},
          {"id": "C", "name": "C", "primitive": "cell",
           "volume": {"value": 10, "unit": "m3"},
           "media": [{"medium": "solid", "fraction": {"value": 0.5, "unit": "1"}}, {"medium": "fluid", "fraction": {"value": 0.5, "unit": "1"}}],
           "species": [{"species": "X", "initial_inventory": {"value": 100, "unit": "kg"}}],
           "partitioning": [{"species": "X", "from_medium": "fluid", "to_medium": "solid", "coefficient": {"value": 4, "unit": "1"}}],
           "save_results": {"time_history": true}}
        ]}"#,
    );
    // Mass split unchanged from the cells_v2 partitioning test.
    close(&hist(&r, "C:X@solid"), &[80.0, 80.0]);
    close(&hist(&r, "C:X@fluid"), &[20.0, 20.0]);
    // Concentrations per medium pore volume.
    close(&hist(&r, "C:X@solid:C"), &[80.0, 80.0]); // 80 / (10·0.5·0.2) = 80/1
    close(&hist(&r, "C:X@fluid:C"), &[4.0, 4.0]);   // 20 / (10·0.5·1.0) = 20/5
}

/// A cell with NO volume emits mass only — no `:C` result id is produced (concentration undefined).
#[test]
fn no_volume_no_concentration() {
    let r = run(
        r#"{"wasim_version": "0.9.7",
        "simulation_settings": {"duration": {"value": 2, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 1},
        "elements": [
          {"id": "X", "name": "X", "primitive": "species"},
          {"id": "C", "name": "C", "primitive": "cell",
           "media": [{"medium": "water", "fraction": {"value": 1.0, "unit": "1"}}],
           "species": [{"species": "X", "initial_inventory": {"value": 50, "unit": "kg"}}],
           "save_results": {"time_history": true}}
        ]}"#,
    );
    close(&hist(&r, "C:X@water"), &[50.0, 50.0]); // mass present
    assert!(!r.elements.contains_key("C:X@water:C"), "no volume → no concentration result id");
}
