//! B1 gap #1 — bound-crossing sub-stepping. Under `EventAccurate` the engine subdivides a grid
//! step at the exact instant a bounded stock reaches its floor/capacity (closed-form under Euler),
//! so that **coupled downstream elements re-evaluate at the crossing instant**. The payoff is not
//! single-stock mass (Euler + clamp already conserves that identically); it is that after a stock
//! is pinned to its bound mid-step, the *next* sub-interval's topo pass recomputes every element
//! that reads the stock against the clamped level for the remainder of the step.
//!
//! The companion invariants (Fixed untouched; EA-without-crossings bit-identical; RNG grid-only)
//! live in `timebase_bit_identity.rs` / `timebase_v2.rs`; here we exercise the crossing behavior
//! itself and its guards.

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig, TimebaseMode};

fn run(json: &str, mode: TimebaseMode, seed: u64) -> wasim_engine::SimulationResults {
    let m = parse_v2(json).expect("parse");
    let g = ModelGraphV2::build(&m).expect("build");
    let cfg = RunConfig { seed: Some(seed), timebase: mode, ..RunConfig::default() };
    run_v2(&m, &g, &cfg).expect("run")
}

fn final_of(r: &wasim_engine::SimulationResults, id: &str) -> f64 {
    r.elements[id].final_values[0]
}

/// **Corpus spot-run under EventAccurate.** A few stock-heavy corpus models must run to completion
/// under `EventAccurate` and produce finite results with no crash / hang — the crossing sub-step
/// path exercised against real models, not just the synthetic gate fixtures above. Skips silently
/// when the corpus is not checked out (same policy as `timebase_bit_identity.rs`).
#[test]
fn corpus_stock_models_run_under_event_accurate() {
    use std::path::PathBuf;
    let dir = PathBuf::from(std::env::var("HOME").unwrap()).join("openvsim/wasim/schema_examples");
    if !dir.join("reservoir.json").exists() {
        eprintln!("skipping: corpus not present");
        return;
    }
    // Stock-heavy models: reservoir (bounded stocks + overflow), pond (probabilistic stock),
    // sac_sma (hydrology store cascade with bounds). All should survive EventAccurate.
    for name in ["reservoir.json", "pond.json", "sac_sma.json"] {
        let p = dir.join(name);
        if !p.exists() {
            continue;
        }
        let json = std::fs::read_to_string(&p).unwrap();
        let m = parse_v2(&json).unwrap_or_else(|e| panic!("{name}: parse {e:?}"));
        let g = ModelGraphV2::build(&m).unwrap_or_else(|e| panic!("{name}: build {e:?}"));
        let cfg = RunConfig {
            seed: Some(20260720),
            n_realizations: Some(16),
            timebase: TimebaseMode::EventAccurate,
            ..RunConfig::default()
        };
        let r = run_v2(&m, &g, &cfg).unwrap_or_else(|e| panic!("{name}: EventAccurate run {e:?}"));
        // Every recorded final value must be finite (no NaN/inf from a mishandled sub-split).
        for (id, er) in &r.elements {
            for &v in &er.final_values {
                assert!(v.is_finite(), "{name}: element {id} produced non-finite final value {v}");
            }
        }
    }
}

/// **Coupled rate change at capacity.** A reservoir `r` fills at a constant 4/d from level 8 with
/// capacity 10 → it reaches capacity at t=0.5 of a 1-day step. A node `full` reads `r` and outputs
/// 1 once `r >= 10`; that gate drives a second stock `acc`'s inflow.
///
/// - Under `Fixed`: `full` is evaluated once in the topo pass reading the *start-of-step* level
///   (8) → gate is 0 all step → `acc` gains nothing.
/// - Under `EventAccurate`: the step splits at t=0.5. Sub-interval 1 fills `r` to exactly 10 and
///   commits `outputs[r]=10`. Sub-interval 2's topo reads that clamped value → gate is 1 → `acc`
///   integrates the inflow over the remaining 0.5 d → `acc` gains 0.5.
#[test]
fn coupled_gate_switches_at_capacity_crossing() {
    let json = r#"{"wasim_version": "0.9.3",
      "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "seed": 1},
      "elements": [
        {"id": "r", "name": "R", "primitive": "stock", "initial_value": {"value": 8, "unit": "1"},
         "rate": {"value": 4, "unit": "1/d"}, "capacity": {"value": 10, "unit": "1"},
         "save_results": {"time_history": true, "final_value": true}},
        {"id": "full", "name": "Full", "primitive": "node", "value_rule": "expression",
         "expression": {"ast": {"op": "if",
           "cond": {"op": "gte", "left": {"op": "ref", "element_id": "r"}, "right": {"op": "literal", "value": 10}},
           "then": {"op": "literal", "value": 1.0},
           "else": {"op": "literal", "value": 0.0}}},
         "save_results": {"final_value": true}},
        {"id": "acc", "name": "Acc", "primitive": "stock", "initial_value": {"value": 0, "unit": "1"},
         "inputs": ["full"], "rate": {"ast": {"op": "ref", "element_id": "full"}},
         "save_results": {"final_value": true}}
      ]}"#;
    let fixed = run(json, TimebaseMode::Fixed, 1);
    let ea = run(json, TimebaseMode::EventAccurate, 1);

    // Both fill r to capacity identically (mass at grid granularity is unchanged).
    assert!((final_of(&fixed, "r") - 10.0).abs() < 1e-9, "fixed r should cap at 10");
    assert!((final_of(&ea, "r") - 10.0).abs() < 1e-9, "ea r should cap at 10");

    // The coupling is where they diverge: Fixed sees the gate closed all step, EventAccurate
    // opens it at the crossing and integrates the partial-step inflow.
    assert!((final_of(&fixed, "acc") - 0.0).abs() < 1e-9, "fixed acc must stay 0, got {}", final_of(&fixed, "acc"));
    assert!((final_of(&ea, "acc") - 0.5).abs() < 1e-9, "ea acc must gain the 0.5 d partial inflow, got {}", final_of(&ea, "acc"));
}

/// **Overflow conservation across a crossing.** A reservoir overflowing routes the *same total*
/// excess whether or not the step is split — single-stock mass is conserved either way, and the
/// integrated overflow (rate · time = excess) coincides between the modes. So a raw total does
/// not *distinguish* Fixed from EventAccurate here; what it guards is that inserting a crossing
/// sub-interval must **not** double-route or drop the routed excess. This is the mass-conservation
/// contract for the split path (the coupled *behavior* difference is covered by the gate tests).
///
/// r: level 8, inflow 4/d, capacity 10 → crosses at t=0.5, routes excess 2 to `sink` in both modes.
#[test]
fn overflow_total_conserved_across_crossing() {
    let json = r#"{"wasim_version": "0.9.3",
      "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "seed": 1},
      "elements": [
        {"id": "r", "name": "R", "primitive": "stock", "initial_value": {"value": 8, "unit": "1"},
         "rate": {"value": 4, "unit": "1/d"}, "capacity": {"value": 10, "unit": "1"},
         "overflow_target": "sink",
         "save_results": {"final_value": true}},
        {"id": "sink", "name": "Sink", "primitive": "stock", "initial_value": {"value": 0, "unit": "1"},
         "save_results": {"final_value": true}}
      ]}"#;
    let fixed = run(json, TimebaseMode::Fixed, 1);
    let ea = run(json, TimebaseMode::EventAccurate, 1);
    // r caps at 10 in both; the routed excess (2) reaches the sink identically. Splitting the step
    // at the crossing must NOT double-route or drop mass.
    assert!((final_of(&fixed, "r") - 10.0).abs() < 1e-9);
    assert!((final_of(&ea, "r") - 10.0).abs() < 1e-9);
    assert!((final_of(&fixed, "sink") - 2.0).abs() < 1e-9, "fixed sink got {}", final_of(&fixed, "sink"));
    assert!((final_of(&ea, "sink") - 2.0).abs() < 1e-9, "ea sink must conserve the routed excess, got {}", final_of(&ea, "sink"));
}

/// **Floor crossing (drain to empty) with a coupled reader.** A draining reservoir `r` hits its
/// floor mid-step; a node `low` reads `r` and outputs 1 once `r <= 3`, driving a stock `alarm`.
/// Under `Fixed` the low state is seen only from the *next* step (topo reads the start-of-step
/// level); under `EventAccurate` the crossing pins `r` to the floor mid-step and the next
/// sub-interval's topo sees the clamped level, so `alarm` integrates the extra post-crossing
/// window within the crossing step.
///
/// r: level 10, rate -4/d, floor 3. On step 1 the level starts at 6 (after step 0: 10→6) and the
/// unclamped trajectory 6−4·τ reaches the floor 3 at τ=0.75, i.e. t=1.75 — leaving a 0.25 d
/// post-crossing window in step 1. The EventAccurate run credits `alarm` that extra 0.25 d of
/// `low=1` on the crossing step that Fixed defers to step 2. We assert on the EA − Fixed
/// *difference* so the shared step-0 `ref`-fallback baseline (stocks read as 0 before their first
/// integration) cancels and only the crossing contribution remains.
#[test]
fn coupled_gate_switches_at_floor_crossing() {
    let json = r#"{"wasim_version": "0.9.3",
      "simulation_settings": {"duration": {"value": 2, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "seed": 1},
      "elements": [
        {"id": "r", "name": "R", "primitive": "stock", "initial_value": {"value": 10, "unit": "1"},
         "rate": {"value": -4, "unit": "1/d"}, "floor": {"value": 3, "unit": "1"},
         "save_results": {"final_value": true}},
        {"id": "low", "name": "Low", "primitive": "node", "value_rule": "expression",
         "expression": {"ast": {"op": "if",
           "cond": {"op": "lte", "left": {"op": "ref", "element_id": "r"}, "right": {"op": "literal", "value": 3}},
           "then": {"op": "literal", "value": 1.0},
           "else": {"op": "literal", "value": 0.0}}},
         "save_results": {"final_value": true}},
        {"id": "alarm", "name": "Alarm", "primitive": "stock", "initial_value": {"value": 0, "unit": "1"},
         "inputs": ["low"], "rate": {"ast": {"op": "ref", "element_id": "low"}},
         "save_results": {"final_value": true}}
      ]}"#;
    let fixed = run(json, TimebaseMode::Fixed, 1);
    let ea = run(json, TimebaseMode::EventAccurate, 1);
    assert!((final_of(&fixed, "r") - 3.0).abs() < 1e-9, "fixed r floors at 3");
    assert!((final_of(&ea, "r") - 3.0).abs() < 1e-9, "ea r floors at 3");
    // EventAccurate credits the extra 0.25 d of `low=1` within the crossing step (t=1.75→2.0).
    let delta = final_of(&ea, "alarm") - final_of(&fixed, "alarm");
    assert!((delta - 0.25).abs() < 1e-9,
        "EventAccurate must add the 0.25 d post-crossing window to alarm; delta was {delta} \
         (ea {}, fixed {})", final_of(&ea, "alarm"), final_of(&fixed, "alarm"));
}

/// **No crossing ⇒ bit-identical.** A bounded stock whose level never reaches its bound produces
/// identical results under Fixed and EventAccurate (the crossing detector fires on nothing, so no
/// sub-interval is inserted). r: level 0, rate +2/d, capacity 100 → never near the bound in a
/// 10-day run.
#[test]
fn no_crossing_is_bit_identical() {
    let json = r#"{"wasim_version": "0.9.3",
      "simulation_settings": {"duration": {"value": 10, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "seed": 1},
      "elements": [
        {"id": "r", "name": "R", "primitive": "stock", "initial_value": {"value": 0, "unit": "1"},
         "rate": {"value": 2, "unit": "1/d"}, "capacity": {"value": 100, "unit": "1"}, "floor": {"value": 0, "unit": "1"},
         "save_results": {"time_history": true, "final_value": true}}
      ]}"#;
    let fixed = run(json, TimebaseMode::Fixed, 1);
    let ea = run(json, TimebaseMode::EventAccurate, 1);
    assert_eq!(
        fixed.elements["r"].time_history.as_ref().unwrap().mean,
        ea.elements["r"].time_history.as_ref().unwrap().mean,
        "no bound crossing → EventAccurate must be bit-identical to Fixed"
    );
    assert_eq!(fixed.elements["r"].final_values, ea.elements["r"].final_values);
}

/// **RNG stability across a crossing.** A probabilistic model with a bounded stock that crosses
/// its capacity mid-step must draw identically under both modes: the crossing re-run consumes no
/// randomness (the load-bearing invariant). `x` is a sampled node; `r` fills to capacity (forcing
/// a crossing sub-step); `x`'s per-realization draws must be bit-identical between modes.
#[test]
fn rng_stable_across_bound_crossing() {
    let json = r#"{"wasim_version": "0.9.3",
      "simulation_settings": {"duration": {"value": 5, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 200, "seed": 99},
      "elements": [
        {"id": "x", "name": "X", "primitive": "node", "value_rule": "sample",
         "distribution": {"family": "normal", "parameters": {"mean": {"value": 5, "unit": "1"}, "stddev": {"value": 2, "unit": "1"}}},
         "save_results": {"final_value": true}},
        {"id": "r", "name": "R", "primitive": "stock", "initial_value": {"value": 8, "unit": "1"},
         "rate": {"value": 4, "unit": "1/d"}, "capacity": {"value": 10, "unit": "1"},
         "save_results": {"final_value": true}}
      ]}"#;
    let fixed = run(json, TimebaseMode::Fixed, 99);
    let ea = run(json, TimebaseMode::EventAccurate, 99);
    assert_eq!(
        fixed.elements["x"].final_values, ea.elements["x"].final_values,
        "sample draws must be bit-identical across a bound crossing (re-run consumes no RNG)"
    );
}

/// **Cascade: two stocks crossing at different sub-times in one grid step.** `a` (level 8, +4/d,
/// cap 10) crosses at t=0.25 of a 2-day step; `b` (level 6, +2/d, cap 10) crosses at t=1.0. The
/// engine resolves the earliest crossing first, re-evaluates, then finds the second on a later
/// sub-interval — both stocks land exactly on their caps and the run completes.
#[test]
fn cascade_two_crossings_resolved() {
    let json = r#"{"wasim_version": "0.9.3",
      "simulation_settings": {"duration": {"value": 2, "unit": "d"}, "timestep": {"value": 2, "unit": "d"}, "seed": 1},
      "elements": [
        {"id": "a", "name": "A", "primitive": "stock", "initial_value": {"value": 8, "unit": "1"},
         "rate": {"value": 4, "unit": "1/d"}, "capacity": {"value": 10, "unit": "1"},
         "save_results": {"final_value": true}},
        {"id": "b", "name": "B", "primitive": "stock", "initial_value": {"value": 6, "unit": "1"},
         "rate": {"value": 2, "unit": "1/d"}, "capacity": {"value": 10, "unit": "1"},
         "save_results": {"final_value": true}}
      ]}"#;
    let ea = run(json, TimebaseMode::EventAccurate, 1);
    assert!((final_of(&ea, "a") - 10.0).abs() < 1e-9, "a caps at 10, got {}", final_of(&ea, "a"));
    assert!((final_of(&ea, "b") - 10.0).abs() < 1e-9, "b caps at 10, got {}", final_of(&ea, "b"));
}

/// **Max-splits guard — pathological rate must not hang.** A stock whose rate always drives it
/// back across a bound every sub-interval would split forever; the per-step cap stops it after
/// `MAX_SPLITS_PER_STEP` and integrates the remainder grid-quantized. This test just asserts the
/// run terminates and produces finite results (the guard fires; a `warn:` is emitted to stderr).
///
/// We approximate a pathological case with a stock hovering right at its capacity with a small
/// positive rate on a long step, so many crossings are attempted. The exact final value is not
/// asserted (it depends on where the cap truncates); termination + finiteness is the contract.
#[test]
fn max_splits_guard_terminates() {
    // A large step with a tiny inflow against a capacity just above the start level forces a
    // crossing very early, and the residual keeps nudging the cap. The guard must bound the work.
    let json = r#"{"wasim_version": "0.9.3",
      "simulation_settings": {"duration": {"value": 1000, "unit": "d"}, "timestep": {"value": 1000, "unit": "d"}, "seed": 1},
      "elements": [
        {"id": "r", "name": "R", "primitive": "stock", "initial_value": {"value": 9.999999, "unit": "1"},
         "rate": {"value": 1, "unit": "1/d"}, "capacity": {"value": 10, "unit": "1"},
         "save_results": {"final_value": true}}
      ]}"#;
    let ea = run(json, TimebaseMode::EventAccurate, 1);
    let v = final_of(&ea, "r");
    assert!(v.is_finite(), "run must terminate with a finite result under the split guard, got {v}");
    assert!((v - 10.0).abs() < 1e-6, "stock still lands at its capacity, got {v}");
}
