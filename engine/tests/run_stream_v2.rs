//! Phase-0b commit B: chunked `RunState::advance` + partial `assemble` semantics.
//!
//! These are the real streaming invariants over the REAL engine (the TS-side "chunk
//! invariance" script only ever covered a synthetic model):
//!   1. Chunking is semantically null — any advance() chunking yields a `SimulationResults`
//!      bit-identical to the one-shot batch `run_v2()`. (Holds exactly, not just chunk-vs-
//!      chunk: the stores append in realization order regardless of chunking, and the
//!      reduction runs once at assemble time.)
//!   2. A partial assemble is a valid result for exactly the realizations folded so far, and
//!      continuing afterwards still converges to the batch-identical final result.
//!   3. Cancel-at-k equals a fresh batch run of k realizations — realization streams are
//!      `(seed, real_idx)`-pure. (Stated for plain Monte Carlo; under LHS the stratification
//!      columns depend on the *total* n_real, so a fresh k-run is a different — equally
//!      valid — design. The partial IS the first k of the N-run in both cases.)

use wasim_engine::engine_v2::{Checkpoint, RunState};
use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig, SimulationResults};

/// Stochastic two-element model: a per-realization normal sample driving a stock, both saved
/// with history — exercises final_store, hist_store, and the percentile bands. Plain MC.
fn model_json(n_real: u32) -> String {
    format!(r#"{{
      "wasim_version": "0.9.7",
      "simulation_settings": {{"duration": {{"value": 12, "unit": "d"}}, "timestep": {{"value": 1, "unit": "d"}},
        "n_realizations": {n_real}, "seed": 42}},
      "elements": [
        {{"id": "inflow", "name": "Inflow", "primitive": "node", "value_rule": "sample",
         "distribution": {{"family": "normal", "parameters": {{"mean": {{"value": 5, "unit": "1"}}, "stddev": {{"value": 2, "unit": "1"}}}}}},
         "save_results": {{"final_value": true, "time_history": true}}}},
        {{"id": "tank", "name": "Tank", "primitive": "stock", "inputs": ["inflow"],
         "initial_value": {{"value": 0, "unit": "1"}},
         "rate": {{"ast": {{"op": "ref", "element_id": "inflow"}}}},
         "save_results": {{"final_value": true, "time_history": true}}}}
      ]
    }}"#)
}

/// Structural bit-equality of two results (elements is a HashMap, so JSON text comparison
/// would be key-order-unstable; compare field-by-field at the f64 bit level instead).
fn assert_bit_identical(a: &SimulationResults, b: &SimulationResults, what: &str) {
    let beq = |x: &[f64], y: &[f64]| x.len() == y.len()
        && x.iter().zip(y).all(|(p, q)| p.to_bits() == q.to_bits());
    assert_eq!(a.n_realizations, b.n_realizations, "{what}: n_realizations");
    assert_eq!(a.n_steps, b.n_steps, "{what}: n_steps");
    assert!(beq(&a.time_axis, &b.time_axis), "{what}: time_axis");
    assert_eq!(a.output_ids, b.output_ids, "{what}: output_ids");
    assert_eq!(a.elements.len(), b.elements.len(), "{what}: element count");
    for (id, ea) in &a.elements {
        let eb = b.elements.get(id).unwrap_or_else(|| panic!("{what}: missing element {id}"));
        assert!(beq(&ea.final_values, &eb.final_values), "{what}: {id} final_values");
        match (&ea.time_history, &eb.time_history) {
            (None, None) => {}
            (Some(ha), Some(hb)) => {
                for (label, xa, xb) in [
                    ("mean", &ha.mean, &hb.mean), ("p05", &ha.p05, &hb.p05),
                    ("p25", &ha.p25, &hb.p25), ("p50", &ha.p50, &hb.p50),
                    ("p75", &ha.p75, &hb.p75), ("p95", &ha.p95, &hb.p95),
                ] {
                    assert!(beq(xa, xb), "{what}: {id} time_history.{label}");
                }
            }
            _ => panic!("{what}: {id} time_history presence mismatch"),
        }
    }
}

/// Invariant 1: every chunking of advance() produces the batch result, bit for bit.
#[test]
fn chunked_advance_is_bit_identical_to_batch() {
    let json = model_json(40);
    let m = parse_v2(&json).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    let cfg = RunConfig::default();
    let batch = run_v2(&m, &g, &cfg).expect("batch run");

    for chunk in [1u32, 7, 33, 40] {
        let mut st = RunState::new(&m, &g, &cfg).expect("new");
        while !st.is_complete() {
            st.advance(chunk).expect("advance");
        }
        assert_eq!(st.completed(), 40);
        let streamed = st.assemble().expect("assemble");
        assert_bit_identical(&streamed, &batch, &format!("chunk={chunk}"));
    }
}

/// Invariant 2: a mid-run assemble is a valid partial result (n_realizations = completed),
/// and the run still finishes batch-identical afterwards — snapshots don't perturb the run.
#[test]
fn partial_assemble_then_resume_matches_batch() {
    let json = model_json(40);
    let m = parse_v2(&json).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    let cfg = RunConfig::default();
    let batch = run_v2(&m, &g, &cfg).expect("batch run");

    let mut st = RunState::new(&m, &g, &cfg).expect("new");
    st.advance(17).expect("first half");
    let partial = st.assemble().expect("partial assemble");
    assert_eq!(partial.n_realizations, 17, "partial reports completed count");
    assert_eq!(partial.n_steps, batch.n_steps);
    // Partial columns are the first 17 realizations of the batch, in order.
    for (id, ep) in &partial.elements {
        let eb = &batch.elements[id];
        assert_eq!(ep.final_values.len(), 17, "{id}: one final per completed realization");
        assert!(ep.final_values.iter().zip(&eb.final_values)
            .all(|(p, q)| p.to_bits() == q.to_bits()),
            "{id}: partial finals are a prefix of batch finals");
    }

    st.advance(u32::MAX).expect("rest");
    let done = st.assemble().expect("final assemble");
    assert_bit_identical(&done, &batch, "resume-after-snapshot");
}

/// Invariant 3: cancelling at k yields exactly a fresh batch run of k realizations (plain
/// Monte Carlo — see the module docs for the LHS caveat).
#[test]
fn cancel_at_k_equals_batch_of_k() {
    let json_full = model_json(40);
    let m_full = parse_v2(&json_full).expect("parse full");
    let g_full = ModelGraphV2::build(&m_full).expect("graph full");
    let cfg = RunConfig::default();

    let mut st = RunState::new(&m_full, &g_full, &cfg).expect("new");
    st.advance(13).expect("advance 13");
    let cancelled = st.assemble().expect("assemble cancelled");

    let json_k = model_json(13);
    let m_k = parse_v2(&json_k).expect("parse k");
    let g_k = ModelGraphV2::build(&m_k).expect("graph k");
    let fresh_k = run_v2(&m_k, &g_k, &cfg).expect("batch k");

    assert_bit_identical(&cancelled, &fresh_k, "cancel-at-13 vs fresh 13-run");
}

/// Checkpoint round-trip: rebuild a fresh RunState each chunk, restore the prior checkpoint,
/// advance, checkpoint back out — the exact pattern the WASM poll handle uses (it owns the
/// model/graph and cannot hold a borrowing RunState<'a> across calls). Must equal batch.
#[test]
fn checkpoint_rebuild_per_chunk_matches_batch() {
    let json = model_json(40);
    let m = parse_v2(&json).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    let cfg = RunConfig::default();
    let batch = run_v2(&m, &g, &cfg).expect("batch run");

    // Simulate the handle: only a Checkpoint persists between "polls"; the RunState is rebuilt
    // fresh (re-preparing) each time, restoring the checkpoint before advancing.
    let mut cp = Checkpoint::default();
    loop {
        let mut st = RunState::new(&m, &g, &cfg).expect("rebuild");
        st.restore(cp.clone());
        if st.is_complete() {
            let done = st.assemble().expect("assemble");
            assert_bit_identical(&done, &batch, "checkpoint-rebuild");
            break;
        }
        st.advance(9).expect("advance one poll");
        cp = st.checkpoint();
    }
    // A default (empty) checkpoint must mean a from-scratch run.
    assert_eq!(cp.next_real, 40, "all realizations folded via checkpoint round-trips");
}
