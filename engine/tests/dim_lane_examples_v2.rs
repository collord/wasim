//! Capstone for the dimensioned array lane (DIMENSIONED_ARRAY_LANE_SCOPE.md): the two
//! committed Analytica re-solutions — both dimensioned, both with a submodel read via
//! `submodel_stat` — must be array-lane-**eligible** and produce results **bit-identical**
//! to the scalar lane across every element and member, end to end.

use std::path::PathBuf;
use wasim_engine::{array_lane, parse_v2, run_v2, ModelGraphV2, RunConfig};

fn example(name: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tools/examples").join(name);
    std::fs::read_to_string(p).expect("read example")
}

fn assert_lane_matches_scalar(name: &str) {
    let m = parse_v2(&example(name)).expect("parse");
    array_lane::eligible(&m).unwrap_or_else(|e| panic!("{name} should be dim-lane-eligible: {e}"));
    let g = ModelGraphV2::build(&m).expect("graph");
    let scalar = run_v2(&m, &g, &RunConfig { array_lane: false, ..Default::default() }).expect("scalar");
    let array = run_v2(&m, &g, &RunConfig { array_lane: true, ..Default::default() }).expect("array");

    assert_eq!(scalar.elements.len(), array.elements.len(), "{name}: element-set size differs");
    let mut checked = 0usize;
    for (id, es) in &scalar.elements {
        let ea = array.elements.get(id).unwrap_or_else(|| panic!("{name}: lane missing element {id}"));
        assert_eq!(es.final_values.len(), ea.final_values.len(), "{name}/{id}: length differs");
        for (r, (a, b)) in es.final_values.iter().zip(&ea.final_values).enumerate() {
            assert_eq!(a.to_bits(), b.to_bits(), "{name}/{id}[{r}]: lane {b} != scalar {a}");
            checked += 1;
        }
    }
    assert!(checked > 100_000, "{name}: expected a substantial comparison, only {checked} values");
}

#[test]
fn native_eviu_runs_on_dim_lane_bit_identically() {
    assert_lane_matches_scalar("eviu_plane_catching_native.wasim.json");
}

#[test]
fn native_platform_runs_on_dim_lane_bit_identically() {
    assert_lane_matches_scalar("platform_decommissioning_native.wasim.json");
}
