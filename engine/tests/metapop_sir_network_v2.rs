//! End-to-end: the shipped `sir-on-network` metapop template (a snapshot fixture) runs on the
//! engine as a genuine dimensioned stock-and-flow SIR — exercising the array-eval fixes together
//! (scalar-initial broadcast, per-cell inflow/outflow integration, elementwise `min` clamp) plus
//! the coupling matrix + neighbour-axis reduction + `lag`. Asserts population conservation and that
//! the epidemic follows the graph's edges (node 5 infects via the 2–5 chord, before node 6).
use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig, SimulationResults};
fn h(r: &SimulationResults, id: &str) -> Vec<f64> {
    r.elements.get(id).and_then(|e| e.time_history.as_ref()).map(|x| x.mean.clone()).unwrap_or_else(|| panic!("no {id}"))
}
fn cell(r: &SimulationResults, id: &str, k: usize) -> Vec<f64> { h(r, &format!("{id}#{k}")) }
fn first_infected(r: &SimulationResults, n: usize) -> usize {
    cell(r, "I", n).iter().position(|&v| v > 1e-3).unwrap_or(usize::MAX)
}

#[test]
fn sir_on_network_template_runs_as_dimensioned_stocks() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/metapop_sir_network.json");
    let src = std::fs::read_to_string(&path).expect("read fixture");
    let m = parse_v2(&src).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    let r = run_v2(&m, &g, &RunConfig::default()).expect("run");

    // Population conserved per node (S+I+R = 1) → total across 6 nodes = 6, every step.
    let nsteps = h(&r, "totI").len();
    for step in 0..nsteps {
        let total: f64 = (1..=6).map(|n| cell(&r, "S", n)[step] + cell(&r, "I", n)[step] + cell(&r, "R", n)[step]).sum();
        assert!((total - 6.0).abs() < 1e-6, "step {step}: population {total} != 6");
        // Non-negativity (the min clamp).
        for n in 1..=6 { assert!(cell(&r, "S", n)[step] >= -1e-9 && cell(&r, "I", n)[step] >= -1e-9, "negative compartment at node {n} step {step}"); }
    }

    // Epidemic curve rises then falls.
    let ti = h(&r, "totI");
    let peak = ti.iter().cloned().fold(0.0_f64, f64::max);
    assert!(peak > ti[0] && peak > *ti.last().unwrap(), "SIR curve (peak {peak})");

    // Frontier follows graph edges: node1 seeded; node 5 infects via the 2–5 chord BEFORE node 6
    // (which is the far end of the path). This is the load-bearing "uses the network" check.
    assert_eq!(first_infected(&r, 1), 0, "node 1 seeded");
    assert!(first_infected(&r, 2) < first_infected(&r, 3), "2 before 3");
    assert!(first_infected(&r, 5) < first_infected(&r, 6), "node 5 (chord) infects before node 6 (path end)");
}
