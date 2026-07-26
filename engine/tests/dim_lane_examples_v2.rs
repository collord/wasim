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

// Perf guard (ignored by default): the dimensioned lane vs the scalar engine on the
// two native models. Run: cargo test --release --test dim_lane_examples_v2 -- --ignored --nocapture
#[test]
#[ignore]
fn bench_dim_lane_vs_scalar() {
    use std::time::Instant;
    for name in ["eviu_plane_catching_native.wasim.json", "platform_decommissioning_native.wasim.json"] {
        let m = parse_v2(&example(name)).unwrap();
        let g = ModelGraphV2::build(&m).unwrap();
        let sc = RunConfig { n_realizations: Some(20_000), array_lane: false, ..Default::default() };
        let ac = RunConfig { n_realizations: Some(20_000), array_lane: true, ..Default::default() };
        run_v2(&m, &g, &sc).unwrap(); run_v2(&m, &g, &ac).unwrap();
        let (mut st, mut at) = (f64::INFINITY, f64::INFINITY);
        for _ in 0..5 {
            let t = Instant::now(); std::hint::black_box(run_v2(&m, &g, &sc).unwrap()); st = st.min(t.elapsed().as_secs_f64());
            let t = Instant::now(); std::hint::black_box(run_v2(&m, &g, &ac).unwrap()); at = at.min(t.elapsed().as_secs_f64());
        }
        eprintln!("BENCH {name:45} scalar={st:.4}s dimlane={at:.4}s speedup={:.2}x", st / at);
    }
}
