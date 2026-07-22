//! Species-set decay read path (§10a items C + E). re-gsm emits per-nuclide decay data under a
//! single species *set* element's `members[]` (the GoldSim SSpeciesElem is one dimension element
//! holding N nuclides), not one species element per nuclide. Cells and `decay_products` reference
//! nuclides by bare member `name`, so the engine builds its per-nuclide decay table keyed by member
//! name. This exercises that read path end-to-end:
//!   • first-order decay of a set member,
//!   • cross-member daughter ingrowth (mass-conserving),
//!   • half-life unit conversion into the sim's dt unit.

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

/// One species SET element (id "Materials/Species") holding member "Tc" with half-life 1 d.
/// The cell references the member by bare name "Tc" — decay must resolve against the member table,
/// not the set element id. Mass halves each day: 100 → 50 → 25 → 12.5.
#[test]
fn set_member_first_order_decay() {
    let r = run(
        r#"{"wasim_version": "0.9.7",
        "simulation_settings": {"duration": {"value": 4, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 1},
        "elements": [
          {"id": "Materials/Species", "name": "Species", "primitive": "species",
           "members": [{"name": "Tc", "half_life": {"value": 1, "unit": "d"}}]},
          {"id": "C", "name": "C", "primitive": "cell",
           "species": [{"species": "Tc", "initial_inventory": {"value": 100, "unit": "kg"}}],
           "save_results": {"time_history": true}}
        ]}"#,
    );
    close(&hist(&r, "C:Tc"), &[50.0, 25.0, 12.5, 6.25]);
}

/// Cross-member ingrowth: parent member P → daughter member D (both nuclides of the same set).
/// `decay_products[].species` names a sibling member ("D"), which resolves within the set. Parent
/// halves, daughter ingrows, total conserved — the byte-parallel of the per-element chain test.
#[test]
fn set_member_chain_ingrowth_conserves_mass() {
    let r = run(
        r#"{"wasim_version": "0.9.7",
        "simulation_settings": {"duration": {"value": 3, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 1},
        "elements": [
          {"id": "Materials/Species", "name": "Species", "primitive": "species",
           "members": [
             {"name": "P", "half_life": {"value": 1, "unit": "d"},
              "decay_products": [{"species": "D", "branching_fraction": 1.0}]},
             {"name": "D"}
           ]},
          {"id": "C", "name": "C", "primitive": "cell",
           "species": [{"species": "P", "initial_inventory": {"value": 100, "unit": "kg"}},
                       {"species": "D", "initial_inventory": {"value": 0, "unit": "kg"}}],
           "save_results": {"time_history": true}}
        ]}"#,
    );
    close(&hist(&r, "C:P"), &[50.0, 25.0, 12.5]);
    close(&hist(&r, "C:D"), &[50.0, 75.0, 87.5]);
    close(&hist(&r, "C"), &[100.0, 100.0, 100.0]);
}

/// Half-life unit conversion: member half-life is emitted in YEARS (as re-gsm does), the sim runs
/// in years too. 2-year half-life → mass halves every 2 steps: 100 → 70.71 → 50 → 35.36.
/// (Confirms hl is read in its own unit and converted to dt_unit, not taken as a raw day count.)
#[test]
fn set_member_half_life_unit_converted() {
    let r = run(
        r#"{"wasim_version": "0.9.7",
        "simulation_settings": {"duration": {"value": 2, "unit": "yr"}, "timestep": {"value": 1, "unit": "yr"}, "n_realizations": 1},
        "elements": [
          {"id": "Materials/Species", "name": "Species", "primitive": "species",
           "members": [{"name": "R", "half_life": {"value": 2, "unit": "yr"}}]},
          {"id": "C", "name": "C", "primitive": "cell",
           "species": [{"species": "R", "initial_inventory": {"value": 100, "unit": "kg"}}],
           "save_results": {"time_history": true}}
        ]}"#,
    );
    // factor per 1-yr step = 2^(-1/2) = 0.70710678…
    close(&hist(&r, "C:R"), &[70.71067811865476, 50.0]);
}
