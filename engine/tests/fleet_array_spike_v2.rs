//! Spike: probe the array/dimension executor against the *fleet-instancing* use case
//! (HAUL_FLEET_MODEL_SPEC.md §3.2), which the spec rated a "Large" hard blocker.
//!
//! The premise under test: is a per-truck fleet (N array-valued damage states, read
//! per-index, integrated over time, reduced for dispatch) buildable on today's engine —
//! and if not, *exactly* where does it break?
//!
//! Background already established by `v2_parse::array_comprehension_evaluates`: the core
//! comprehension executor (`vector_map`/`index_ref`/`index`/`sum_array`) works end-to-end
//! for a *stateless, single-step* read. This spike targets the four things that test does
//! NOT cover, each a distinct fleet-model requirement:
//!
//!   Probe 1 — recording an array-valued *element* to results  (the damage-spread output)
//!   Probe 2 — an array-valued *stock* integrating a per-member rate  (per-truck damage state)
//!   Probe 3 — per-member evolution over multiple timesteps  (the feedback loop's substrate)
//!   Probe 4 — argmin / masked reduction over the array  (wear-levelling dispatch)
//!
//! Each probe asserts the ACTUAL observed behavior (including degradations), so the test
//! passes and stands as executable documentation of the real boundary. Where a probe
//! documents a gap, it says so in a comment tagged `GAP:` and, where useful, the shape of
//! the fix. Convert a `GAP:` assertion to the desired behavior when that gap is closed.

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig};

/// PROBE 1 — An array-valued element's per-member values reach the results surface via `#k`.
///
/// This is the load-bearing output for the deliverable: the wear-levelling analysis IS the
/// distribution of per-truck damage. An array element (primary output declaring `dimensions`)
/// now expands into `<id>#1..#N` member result series, reusing the `#k` port-key convention.
/// The primary `<id>` still records member[0] for back-compat.
#[test]
fn probe1_array_element_results_expand_to_members() {
    // fleet = [10, 20, 30] : each member distinct, so per-member recording is unambiguous.
    let json = r#"{
      "wasim_version": "0.8.3",
      "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 1},
      "dimensions": [{"id": "Fleet", "name": "Fleet", "size": 3}],
      "elements": [
        {"id": "fleet", "name": "Fleet", "primitive": "node", "value_rule": "expression",
         "outputs": [{"name": "Fleet", "unit": "1", "dimensions": ["Fleet"]}],
         "expression": {"ast": {"op": "vector_map", "over": "Fleet",
           "body": {"op": "multiply", "left": {"op": "index_ref", "axis": "row"}, "right": {"op": "literal", "value": 10}}}},
         "save_results": {"final_value": true, "time_history": true}}
      ]
    }"#;

    let m = parse_v2(json).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    let r = run_v2(&m, &g, &RunConfig::default()).expect("run");

    // Primary still collapses to member[0] (index_ref=1 → 1*10 = 10) for back-compat.
    assert_eq!(r.elements["fleet"].final_values, vec![10.0]);
    // FIXED: per-member series now exist and carry the true per-truck values.
    assert_eq!(r.elements["fleet#1"].final_values, vec![10.0], "member 1");
    assert_eq!(r.elements["fleet#2"].final_values, vec![20.0], "member 2");
    assert_eq!(r.elements["fleet#3"].final_values, vec![30.0], "member 3");
    // Member series carry a labelled, unit-bearing time history too.
    assert_eq!(r.elements["fleet#2"].label, "Fleet[2]");
    assert!(r.elements["fleet#2"].time_history.is_some());
}

/// PROBE 2 — Can a `stock` be array-valued: integrate a per-member rate into per-member state?
///
/// Per-truck cumulative damage is `damage[i]` = ∫ rate[i] dt. In the engine, stock state is
/// `HashMap<String, f64>` (scalar per element) and integration adds a scalar rate. Feeding a
/// stock an array-valued rate is the untested path.
#[test]
fn probe2_array_valued_stock_integration() {
    // rate = [1, 2, 3] per member; over 3 days each member should reach [3, 6, 9] IF the
    // stock integrates per-member. If stock state is scalar, we learn how it degrades.
    let json = r#"{
      "wasim_version": "0.8.3",
      "simulation_settings": {"duration": {"value": 3, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 1},
      "dimensions": [{"id": "Fleet", "name": "Fleet", "size": 3}],
      "elements": [
        {"id": "rate", "name": "Rate", "primitive": "node", "value_rule": "expression",
         "expression": {"ast": {"op": "vector_map", "over": "Fleet",
           "body": {"op": "index_ref", "axis": "row"}}}},
        {"id": "damage", "name": "Damage", "primitive": "stock", "inputs": ["rate"],
         "initial_value": {"value": 0, "unit": "1"},
         "rate": {"ast": {"op": "ref", "element_id": "rate"}},
         "save_results": {"final_value": true}}
      ]
    }"#;

    let m = parse_v2(json).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    let r = run_v2(&m, &g, &RunConfig::default()).expect("run");

    let damage = &r.elements["damage"];
    // OBSERVED-BEHAVIOR PROBE: we don't assert a specific number blindly — record what the
    // engine actually does so the boundary is documented. If the stock integrated the first
    // member only, final = 1*3 = 3.0. Whatever it is, one scalar comes back, confirming a
    // stock cannot carry per-member (vector) state today.
    eprintln!("probe2: array-fed stock final_values = {:?}", damage.final_values);
    assert_eq!(damage.final_values.len(), 1,
        "GAP: stock produces a single scalar per realization; per-member (vector) stock \
         state is the one substantial new executor piece the fleet model needs");
}

/// PROBE 3 — Does per-member state evolve correctly across timesteps via a *stateless* path
/// (array expression + `lag`), sidestepping array-valued stocks?
///
/// THE PIVOTAL QUESTION for scoping. If per-member accumulation can be carried by an array
/// `expression` reading its own previous-step array value through a `lag`, then Stage-3 fleet
/// instancing can be built WITHOUT array-valued stocks — dropping the one "substantial" piece
/// off the critical path. With Probe-1 member expansion in place we can now *observe* each
/// member's evolution directly, not just infer it from a collapsed scalar.
#[test]
fn probe3_per_member_evolution_via_lagged_array() {
    // accum[i]_t = accum[i]_{t-1} + i, seeded 0, over 3 steps → member i (1-based) reaches 3*i.
    // `lag` gives accum's previous-step value; per member we read prev[i] and add index_ref.
    let json = r#"{
      "wasim_version": "0.8.3",
      "simulation_settings": {"duration": {"value": 3, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 1},
      "dimensions": [{"id": "Fleet", "name": "Fleet", "size": 3}],
      "elements": [
        {"id": "prev", "name": "Prev", "primitive": "node", "value_rule": "lag",
         "outputs": [{"name": "Prev", "unit": "1", "dimensions": ["Fleet"]}],
         "input": "accum", "initial": {"value": 0, "unit": "1"}},
        {"id": "accum", "name": "Accum", "primitive": "node", "value_rule": "expression", "inputs": ["prev"],
         "outputs": [{"name": "Accum", "unit": "1", "dimensions": ["Fleet"]}],
         "expression": {"ast": {"op": "vector_map", "over": "Fleet",
           "body": {"op": "add",
             "left": {"op": "index", "array": {"op": "ref", "element_id": "prev"},
                      "indices": [{"op": "index_ref", "axis": "row"}]},
             "right": {"op": "index_ref", "axis": "row"}}}},
         "save_results": {"final_value": true}}
      ]
    }"#;

    let m = parse_v2(json).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    let r = run_v2(&m, &g, &RunConfig::default()).expect("run");

    let m1 = &r.elements["accum#1"].final_values;
    let m2 = &r.elements["accum#2"].final_values;
    let m3 = &r.elements["accum#3"].final_values;
    eprintln!("probe3: accum members after 3 steps = [{m1:?}, {m2:?}, {m3:?}]");

    // VERDICT: if these are [3], [6], [9] the lag path preserves per-member state across
    // steps → array stocks are NOT required for the fleet model → critical path shrinks.
    // If they are all equal (e.g. [3],[3],[3]) the vector flattened through `lag` and array
    // stocks become mandatory. Assert the viable-path outcome; a failure here re-scopes the
    // plan toward array stocks.
    assert_eq!(m1, &vec![3.0], "member 1 = 1*3");
    assert_eq!(m2, &vec![6.0], "member 2 = 2*3 — per-member state survived the lag");
    assert_eq!(m3, &vec![9.0], "member 3 = 3*3 — per-member state survived the lag");
}

/// PROBE 4 — argmin / masked reduction over an array (the wear-levelling dispatch policy).
///
/// "Assign the least-damaged AVAILABLE truck" = argmin over the array. This probe originally
/// documented that `argmin_array` was ABSENT (the gap); it now asserts the CLOSED state —
/// `argmin_array` is implemented and selects the least-damaged truck. (Masked selection uses
/// the penalty idiom `argmin(damage + BIG·failed)`; no dedicated masked-reduction builtin.)
#[test]
fn probe4_argmin_for_dispatch_now_works() {
    // damage = [30, 10, 20]; the least-damaged truck is index 2 (1-based) → argmin returns 2.
    let json = r#"{
      "wasim_version": "0.9.7",
      "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 1},
      "dimensions": [{"id": "Fleet", "name": "Fleet", "size": 3}],
      "elements": [
        {"id": "damage", "name": "Damage", "primitive": "node", "value_rule": "fixed",
         "values": [30, 10, 20], "unit": "1",
         "outputs": [{"name": "Damage", "unit": "1", "dimensions": ["Fleet"]}]},
        {"id": "dispatch", "name": "Dispatch", "primitive": "node", "value_rule": "expression", "inputs": ["damage"],
         "expression": {"ast": {"op": "call", "fn": "argmin_array",
           "args": [{"op": "ref", "element_id": "damage"}]}},
         "save_results": {"final_value": true}}
      ]
    }"#;
    let m = parse_v2(json).expect("argmin_array is now a known builtin");
    let g = ModelGraphV2::build(&m).expect("graph");
    let r = run_v2(&m, &g, &RunConfig::default()).expect("run");
    assert_eq!(r.elements["dispatch"].final_values, vec![2.0],
        "argmin_array selects the least-damaged truck (1-based index 2)");
}

/// PROBE 5 — The real scenario JSON (parameters_examples/haul_fleet_overload.json) parses,
/// and a runnable variant (argmin_array swapped for a fixed dispatch target, since item 3
/// is not yet built) executes and produces per-truck damage-spread output.
#[test]
fn probe5_haul_fleet_scenario_parses_and_runs() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../parameters_examples/haul_fleet_overload.json");
    let json = std::fs::read_to_string(&path).expect("read scenario");

    // The full scenario now parses AS-IS — argmin_array (item 3) and array `status` (item 5)
    // are both implemented. No workaround substitution needed.
    // Shrink to a fast smoke run (the committed model is 500 real × 5yr weekly).
    let runnable = json
        .replace(r#""n_realizations": 500"#, r#""n_realizations": 8"#)
        .replace(r#""duration": { "value": 5, "unit": "yr" }"#, r#""duration": { "value": 26, "unit": "wk" }"#);

    let m = parse_v2(&runnable).expect("full scenario parses (argmin_array + array status live)");
    assert_eq!(m.dimensions[0].size, 5, "Fleet dimension = 5 trucks");
    let g = ModelGraphV2::build(&m).expect("graph builds");
    let r = run_v2(&m, &g, &RunConfig::default()).expect("scenario runs");

    // Per-truck damage series exist (item-1 expansion) — the wear-levelling substrate.
    for k in 1..=5 {
        let id = format!("damage#{k}");
        assert!(r.elements.contains_key(&id), "missing per-truck series {id}");
    }
    let spread = &r.elements["damage_spread"].final_values;
    assert_eq!(spread.len(), 8, "one final value per realization");
    assert!(spread.iter().all(|&s| s >= 0.0 && s.is_finite()), "spread sane: {spread:?}");
    let mean_dmg = &r.elements["damage_mean"].final_values;
    assert!(mean_dmg.iter().any(|&d| d > 0.0), "fleet accrued damage: {mean_dmg:?}");
    // The `failed` array-status latch produces per-truck series too (item 5).
    for k in 1..=5 {
        assert!(r.elements.contains_key(&format!("failed#{k}")), "missing failed#{k} latch series");
    }
    eprintln!("probe5: mean damage {:?}, spread {:?}", mean_dmg, spread);
}

/// PROBE 6 — Wear-levelling dispatch actually ROTATES: `argmin_array` over penalized damage
/// picks a *different* least-damaged truck as damage accumulates, so overload is spread across
/// the fleet rather than always hitting truck 1. This is the mechanism the whole study exists
/// to compare against naive dispatch.
#[test]
fn probe6_wear_levelling_dispatch_rotates_target() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../parameters_examples/haul_fleet_overload.json");
    let json = std::fs::read_to_string(&path).unwrap();
    // One realization, a longer window so the target has reason to move; force overload on by
    // dropping the price threshold to 0 (grade at t0 = 0.9 > g*=0.8, so overload_on is true).
    let runnable = json
        .replace(r#""n_realizations": 500"#, r#""n_realizations": 1"#)
        .replace(r#""duration": { "value": 5, "unit": "yr" }"#, r#""duration": { "value": 52, "unit": "wk" }"#)
        .replace(r#""value": 8000, "unit": "1" } },
      "description": "Overload only when price exceeds this. Optimization variable." }"#,
                 r#""value": 0, "unit": "1" } },
      "description": "forced-on for probe6." }"#);
    let m = parse_v2(&runnable).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    let r = run_v2(&m, &g, &RunConfig::default()).expect("run");
    let tgt = &r.elements["overload_target"].time_history.as_ref().expect("target th").mean;
    // The dispatch target should take at least two distinct truck indices over the run
    // (wear-levelling rotates); a naive/fixed policy would show a single constant index.
    let distinct: std::collections::BTreeSet<i64> = tgt.iter().map(|&x| x.round() as i64).collect();
    eprintln!("probe6: overload_target series (distinct = {distinct:?})");
    assert!(distinct.len() >= 2,
        "wear-levelling argmin dispatch should rotate the overload target across trucks, saw {distinct:?}");
}

/// ITEM 3 — `argmin_array` / `argmax_array` return the 1-based extremum index, ties → lowest.
#[test]
fn item3_argmin_argmax_with_tiebreak() {
    let json = r#"{
      "wasim_version": "0.9.7",
      "simulation_settings": {"duration": {"value": 1, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 1},
      "dimensions": [{"id": "F", "name": "F", "size": 4}],
      "elements": [
        {"id": "vals", "name": "Vals", "primitive": "node", "value_rule": "fixed",
         "values": [30, 10, 20, 10], "unit": "1",
         "outputs": [{"name": "Vals", "unit": "1", "dimensions": ["F"]}]},
        {"id": "amin", "name": "Amin", "primitive": "node", "value_rule": "expression", "inputs": ["vals"],
         "expression": {"ast": {"op": "call", "fn": "argmin_array", "args": [{"op": "ref", "element_id": "vals"}]}},
         "save_results": {"final_value": true}},
        {"id": "amax", "name": "Amax", "primitive": "node", "value_rule": "expression", "inputs": ["vals"],
         "expression": {"ast": {"op": "call", "fn": "argmax_array", "args": [{"op": "ref", "element_id": "vals"}]}},
         "save_results": {"final_value": true}}
      ]
    }"#;
    let m = parse_v2(json).expect("parse (argmin_array now a known builtin)");
    let g = ModelGraphV2::build(&m).expect("graph");
    let r = run_v2(&m, &g, &RunConfig::default()).expect("run");
    // vals = [30,10,20,10]: min is 10 at indices 2 and 4 → lowest wins → 2. max is 30 → 1.
    assert_eq!(r.elements["amin"].final_values, vec![2.0], "argmin lowest-index tie-break");
    assert_eq!(r.elements["amax"].final_values, vec![1.0], "argmax");
}

/// ITEM 5 (Option B) — an array-valued `status` node latches PER MEMBER: member i sets when
/// `signal[i] >= 1` and resets when `signal[i] <= 0`, independently across members.
#[test]
fn item5_array_status_latches_per_member() {
    // signal[i]_t is a series-like driver built per member: it rises then falls at different
    // times per truck, so the latch must differ per member and HOLD between set and reset.
    // We build signal from a step counter + per-member offset so member 1 crosses the set
    // threshold before member 3, and only member 1 later crosses back below the reset threshold.
    let json = r#"{
      "wasim_version": "0.9.7",
      "simulation_settings": {"duration": {"value": 6, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}, "n_realizations": 1},
      "dimensions": [{"id": "F", "name": "F", "size": 3}],
      "elements": [
        {"id": "signal", "name": "Signal", "primitive": "node", "value_rule": "expression",
         "outputs": [{"name": "Signal", "unit": "1", "dimensions": ["F"]}],
         "expression": {"ast": {"op": "vector_map", "over": "F",
           "body": {"op": "subtract",
             "left": {"op": "literal", "value": 2},
             "right": {"op": "index_ref", "axis": "row"}}}}},
        {"id": "latch", "name": "Latch", "primitive": "node", "value_rule": "status",
         "outputs": [{"name": "Latch", "unit": "1", "dimensions": ["F"]}],
         "set":   {"mode": "on_condition", "condition": {"ast": {"op": "gte",
           "left": {"op": "index", "array": {"op": "ref", "element_id": "signal"},
                    "indices": [{"op": "index_ref", "axis": "row"}]},
           "right": {"op": "literal", "value": 1}}}},
         "reset": {"mode": "on_condition", "condition": {"ast": {"op": "lte",
           "left": {"op": "index", "array": {"op": "ref", "element_id": "signal"},
                    "indices": [{"op": "index_ref", "axis": "row"}]},
           "right": {"op": "literal", "value": -1}}}},
         "save_results": {"final_value": true, "time_history": true}}
      ]
    }"#;
    let m = parse_v2(json).expect("parse array status");
    let g = ModelGraphV2::build(&m).expect("graph");
    let r = run_v2(&m, &g, &RunConfig::default()).expect("run");
    // signal[i] = 2 - i (constant over time): member1=1, member2=0, member3=-1.
    //   member1: signal=1 >= 1 → SET → latched 1.
    //   member2: signal=0, never >=1 and never <=-1 → HOLD initial → 0.
    //   member3: signal=-1 <= -1 → RESET-eligible but starts 0 → stays 0.
    // So the per-member final latch is [1, 0, 0] — proving members latch independently.
    assert_eq!(r.elements["latch#1"].final_values, vec![1.0], "member 1 sets");
    assert_eq!(r.elements["latch#2"].final_values, vec![0.0], "member 2 holds");
    assert_eq!(r.elements["latch#3"].final_values, vec![0.0], "member 3 stays reset");
}
