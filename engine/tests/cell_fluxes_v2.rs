//! Coupled-transport fluxes (trait coupled_transport). Cells carry `fluxes[]` describing
//! advective / diffusive inter-cell mass transfer per (species, medium):
//!
//!     advective: moved = rate · C_upstream · dt          (one-way; rate is volumetric flow)
//!     diffusive: moved = coefficient · (C_src − C_tgt) · dt   (signed; drives toward equilibrium)
//!
//! Concentration C = mass / (cell_volume · medium_fraction · medium_porosity). Fluxes are
//! cell-owned: `source` absent = the owning cell, `target` = the far cell. Mass-conserving —
//! what leaves one endpoint enters the other. Reads start-of-interval concentrations so every
//! flux in a sub-interval sees consistent state.

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

/// AirWater-shaped diffusive pair: two unit-pore-volume cells, species starts wholly in A.
/// Pore volume 1 in each (V=1, fraction 1, porosity 1) → C == mass. Coefficient 0.25/day:
///   t0: A=100 B=0
///   t1: moved = 0.25·(100−0)  = 25    → A=75   B=25
///   t2: moved = 0.25·(75−25)  = 12.5  → A=62.5 B=37.5
/// Total mass is conserved (A+B == 100) at every step.
#[test]
fn diffusive_pair_drives_to_equilibrium() {
    let r = run(
        r#"{"wasim_version": "0.9.7",
        "simulation_settings": {"duration": {"value": 2, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 1},
        "elements": [
          {"id": "X", "name": "X", "primitive": "species"},
          {"id": "w", "name": "Water", "primitive": "medium", "phase": "fluid"},
          {"id": "A", "name": "A", "primitive": "cell",
           "volume": {"value": 1, "unit": "m3"},
           "media": [{"medium": "w", "fraction": {"value": 1.0, "unit": "1"}}],
           "species": [{"species": "X", "initial_inventory": {"value": 100, "unit": "kg"}}],
           "fluxes": [{"mechanism": "diffusive", "species": "X", "medium": "w", "target": "B",
                       "coefficient": {"value": 0.25, "unit": "m3/d"}}],
           "save_results": {"time_history": true}},
          {"id": "B", "name": "B", "primitive": "cell",
           "volume": {"value": 1, "unit": "m3"},
           "media": [{"medium": "w", "fraction": {"value": 1.0, "unit": "1"}}],
           "species": [{"species": "X"}],
           "save_results": {"time_history": true}}
        ]}"#,
    );
    close(&hist(&r, "A:X@w"), &[75.0, 62.5]);
    close(&hist(&r, "B:X@w"), &[25.0, 37.5]);
}

/// Advective (one-way) transport: rate · upstream concentration. Unit pore volumes so C == mass.
/// Rate 0.1 m3/day, A starts 100:
///   t1: moved = 0.1·100 = 10 → A=90 B=10
///   t2: moved = 0.1·90  = 9  → A=81 B=19
/// Mass conserved; B never pushes back (advective is one-way even as C_B rises).
#[test]
fn advective_is_one_way() {
    let r = run(
        r#"{"wasim_version": "0.9.7",
        "simulation_settings": {"duration": {"value": 2, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 1},
        "elements": [
          {"id": "X", "name": "X", "primitive": "species"},
          {"id": "w", "name": "Water", "primitive": "medium", "phase": "fluid"},
          {"id": "A", "name": "A", "primitive": "cell",
           "volume": {"value": 1, "unit": "m3"},
           "media": [{"medium": "w", "fraction": {"value": 1.0, "unit": "1"}}],
           "species": [{"species": "X", "initial_inventory": {"value": 100, "unit": "kg"}}],
           "fluxes": [{"mechanism": "advective", "species": "X", "medium": "w", "target": "B",
                       "rate": {"value": 0.1, "unit": "m3/d"}}],
           "save_results": {"time_history": true}},
          {"id": "B", "name": "B", "primitive": "cell",
           "volume": {"value": 1, "unit": "m3"},
           "media": [{"medium": "w", "fraction": {"value": 1.0, "unit": "1"}}],
           "species": [{"species": "X"}],
           "save_results": {"time_history": true}}
        ]}"#,
    );
    close(&hist(&r, "A:X@w"), &[90.0, 81.0]);
    close(&hist(&r, "B:X@w"), &[10.0, 19.0]);
}

/// Diffusion cannot overdraw the source: a coefficient large enough to move more than the
/// available mass in one step is clamped so mass never goes negative. K=2/day, dt=1 would move
/// 2·(50−0)=100 > 50; clamped to 50 → A=0 B=50, then equilibrium holds (both C=25 gradient…
/// actually A=0,B=50 → next gradient (0−50) pulls back 2·(0−50) clamped to −50 → A=50 B=0…).
/// We only assert the clamp: A stays ≥ 0 and total is conserved.
#[test]
fn diffusive_never_overdraws_source() {
    let r = run(
        r#"{"wasim_version": "0.9.7",
        "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 1},
        "elements": [
          {"id": "X", "name": "X", "primitive": "species"},
          {"id": "w", "name": "Water", "primitive": "medium", "phase": "fluid"},
          {"id": "A", "name": "A", "primitive": "cell",
           "volume": {"value": 1, "unit": "m3"},
           "media": [{"medium": "w", "fraction": {"value": 1.0, "unit": "1"}}],
           "species": [{"species": "X", "initial_inventory": {"value": 50, "unit": "kg"}}],
           "fluxes": [{"mechanism": "diffusive", "species": "X", "medium": "w", "target": "B",
                       "coefficient": {"value": 2.0, "unit": "m3/d"}}],
           "save_results": {"time_history": true}},
          {"id": "B", "name": "B", "primitive": "cell",
           "volume": {"value": 1, "unit": "m3"},
           "media": [{"medium": "w", "fraction": {"value": 1.0, "unit": "1"}}],
           "species": [{"species": "X"}],
           "save_results": {"time_history": true}}
        ]}"#,
    );
    close(&hist(&r, "A:X@w"), &[0.0]);
    close(&hist(&r, "B:X@w"), &[50.0]);
}
