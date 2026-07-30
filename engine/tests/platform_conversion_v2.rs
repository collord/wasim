//! Rot guard for the mechanically-converted `Platform_2017.wasim.json` — a large
//! real Analytica model (the PLATFORM decommissioning decision tool) run through
//! `tools/ana_to_wasim.py`, now emitted **v2-native** (named dimensions +
//! vector_map arrays; ~480 elements, 52 dimensions). It exists to keep the
//! converter's output engine-loadable: this is the stress case that surfaced the
//! dangling-ref and formula-distribution-parameter degradations, so a regression
//! that made either crash the engine would fail here.
//!
//! Assertion is deliberately coarse (parses, graph-builds, runs, produces a lot
//! of result elements — dimensioned members expand the count well past the
//! element total) — the converter's fidelity is checked by JSON-Schema validation
//! on the Python side; this only nails down "still runs".

use wasim_engine::{simulate_json, RunConfig};

#[test]
fn converted_platform_model_parses_and_runs() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../tools/examples/Platform_2017.wasim.json");
    let json = std::fs::read_to_string(path).expect("read converted platform model");
    let cfg = RunConfig { n_realizations: Some(30), ..Default::default() };
    let r = simulate_json(&json, &cfg).expect("engine run of converted platform model");
    assert!(r.elements.len() > 300, "expected a large result set, got {}", r.elements.len());
}
