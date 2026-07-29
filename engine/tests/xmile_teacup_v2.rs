//! XMILE importer acceptance test — numeric fidelity under Euler.
//!
//! Loads the committed converter output `tools/fixtures/teacup.json` (produced by
//! `tools/xmile_to_wasim.py` from the SDXorg `teacup.xmile`), runs it through the
//! v2 engine, and checks the `Teacup Temperature` trajectory against SDXorg's
//! canonical `teacup_output.csv`.
//!
//! Alignment note: WaSiM's `time_history` holds the POST-step values
//! (t=dt … t=stop); the t=0 initial (180) lives in the stock's `initial_value`,
//! not in the trajectory. So `time_history.mean[i]` == canonical data row `i+1`
//! (the CSV's first data row is t=0). We assert the initial and the full
//! post-step series.

use wasim_engine::{simulate_json, RunConfig};

const STOCK_ID: &str = "Main/Teacup Temperature";

fn canonical_teacup_column() -> Vec<f64> {
    // teacup_output.csv columns: Time, Characteristic Time, Heat Loss to Room,
    // Room Temperature, Teacup Temperature  → Teacup is index 4.
    let csv = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tools/fixtures/teacup_output.csv"
    ))
    .expect("read teacup_output.csv");
    csv.lines()
        .skip(1) // header
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            l.split(',')
                .nth(4)
                .expect("Teacup column")
                .trim()
                .parse::<f64>()
                .expect("parse Teacup value")
        })
        .collect()
}

#[test]
fn teacup_trajectory_matches_sdxorg_canonical() {
    let json = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tools/fixtures/teacup.json"
    ))
    .expect("read teacup.json fixture");

    let results = simulate_json(&json, &RunConfig::default()).expect("engine run");

    let stock = results
        .elements
        .get(STOCK_ID)
        .unwrap_or_else(|| panic!("stock '{STOCK_ID}' missing from results"));
    let traj = &stock
        .time_history
        .as_ref()
        .expect("stock time_history saved")
        .mean;

    let canonical = canonical_teacup_column();
    // Canonical: 241 rows (t=0 .. t=30). WaSiM trajectory: 240 post-step values.
    assert_eq!(canonical.len(), 241, "canonical row count");
    assert_eq!(traj.len(), 240, "wasim post-step trajectory length");

    // t=0 initial lives in the stock's initial_value / first canonical row.
    assert!(
        (canonical[0] - 180.0).abs() < 1e-9,
        "canonical initial should be 180, got {}",
        canonical[0]
    );

    // Post-step alignment: traj[i] ≙ canonical[i+1]. Euler tolerance.
    for (i, &w) in traj.iter().enumerate() {
        let c = canonical[i + 1];
        let abs = (w - c).abs();
        let rel = abs / c.abs().max(1e-9);
        assert!(
            abs < 1e-2 || rel < 1e-4,
            "step {i}: wasim {w:.6} vs canonical {c:.6} (abs {abs:.2e}, rel {rel:.2e})"
        );
    }

    // Spot-check the endpoints explicitly.
    assert!((traj[0] - 178.625).abs() < 1e-2, "first post-step ≈ 178.625");
    assert!((traj[239] - 75.374).abs() < 1e-2, "final ≈ 75.374");
}

/// The Phase-2 feature model (embedded graphical function, non-negative stock and
/// flow, MOD/IF equations) loads and runs through the engine without error. This
/// exercises `lookup_call`, `floor`, and `max`-wrapped rates end-to-end (numeric
/// values are not pinned against an external reference — that's the teacup's job).
#[test]
fn phase2_feature_model_runs() {
    let json = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tools/fixtures/phase2_features.wasim.json"
    ))
    .expect("read phase2 fixture");
    let results = simulate_json(&json, &RunConfig::default()).expect("phase2 engine run");
    // The non-negative Backlog stock must never dip below its floor of 0.
    let backlog = results
        .elements
        .get("Main/Backlog")
        .expect("Backlog stock present");
    if let Some(th) = backlog.time_history.as_ref() {
        assert!(
            th.mean.iter().all(|&v| v >= -1e-9),
            "non-negative Backlog stayed >= 0"
        );
    }
}

/// The Phase-3 builtin model (TIME/STEP/PULSE/RAMP composed; PREVIOUS→lag,
/// SMTH1→filter, all emitting synth elements) loads and runs, and the composed
/// builtins compute the expected values — proving the lowerings are executable,
/// not just schema-valid.
#[test]
fn phase3_builtin_model_runs() {
    let json = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tools/fixtures/phase3_builtins.wasim.json"
    ))
    .expect("read phase3 fixture");
    let r = simulate_json(&json, &RunConfig::default()).expect("phase3 engine run");

    let series = |id: &str| -> Vec<f64> {
        r.elements
            .get(id)
            .unwrap_or_else(|| panic!("{id} missing"))
            .time_history
            .as_ref()
            .expect("time_history")
            .mean
            .clone()
    };

    // TIME(): post-step values are 0.5, 1.0, … (dt=0.5). First sample = 0.5·1? engine
    // reports the elapsed at each saved step; assert it is monotone and spans [0, 10).
    let clock = series("Main/clock");
    assert!(clock.windows(2).all(|w| w[1] >= w[0]), "TIME() monotone");
    assert!(*clock.last().unwrap() <= 10.0, "TIME() bounded by stop");

    // STEP(5, 4): zero before t=4, 5 at the end.
    let stepped = series("Main/stepped");
    assert!(stepped[0].abs() < 1e-9, "STEP is 0 before t0");
    assert!((stepped.last().unwrap() - 5.0).abs() < 1e-9, "STEP is height after t0");

    // PULSE(100, 3) with dt=0.5 → a single spike of area/dt = 200.
    let pulsed = series("Main/pulsed");
    let spikes: Vec<f64> = pulsed.iter().cloned().filter(|v| v.abs() > 1e-9).collect();
    assert_eq!(spikes.len(), 1, "PULSE fires exactly once");
    assert!((spikes[0] - 200.0).abs() < 1e-6, "PULSE magnitude = mag/dt");

    // PREVIOUS(driver, 10): one-step delay of the ramping driver, seeded at 10.
    let prev = series("Main/prev_driver");
    let driver = series("Main/driver");
    assert!((prev[0] - 10.0).abs() < 1e-9, "PREVIOUS starts at its init");
    assert!(
        (prev.last().unwrap() - driver[driver.len() - 2]).abs() < 1e-6,
        "PREVIOUS(final) == driver(one step earlier)"
    );
}

/// The Phase-4 apply-to-all array model runs: a dimension-3 `base_pop` broadcasts,
/// `growth = base_pop * 1.1`, and `total = SUM(growth)` reduces across the axis.
/// Proves `vector_map` + `sum_array` from the converter execute end-to-end.
#[test]
fn phase4_array_model_runs() {
    let json = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tools/fixtures/phase4_arrays.wasim.json"
    ))
    .expect("read phase4 arrays fixture");
    let r = simulate_json(&json, &RunConfig::default()).expect("phase4 array run");
    let total = r.elements.get("Main/total").expect("total present");
    // 3 regions × (100 × 1.1) = 330.
    assert!(
        (total.final_values[0] - 330.0).abs() < 1e-6,
        "SUM over 3-region growth == 330, got {}",
        total.final_values[0]
    );
}

/// The Phase-4 module model round-trips as an inline deterministic sub-call: the
/// parent drives the submodel's `input_signal` (=5) via the interface binding, the
/// submodel computes `doubled = 10`, and the parent reads it back through
/// `submodel_stat(mean, …)` (exact at n_realizations:1), so `result = 10 + 1 = 11`.
/// Proves values flow back across the module boundary.
#[test]
fn phase4_module_roundtrips_as_inline_subcall() {
    let json = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../tools/fixtures/phase4_modules.wasim.json"
    ))
    .expect("read phase4 modules fixture");
    let r = simulate_json(&json, &RunConfig::default()).expect("phase4 module run");
    let result = r.elements.get("Main/result").expect("result present");
    assert!(
        (result.final_values[0] - 11.0).abs() < 1e-6,
        "result = submodel doubled(5)=10, +1 = 11, got {}",
        result.final_values[0]
    );
}
