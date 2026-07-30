//! Numeric-fidelity pin for the v2-native `.ana` converter (Rung 2).
//!
//! `Month_to_quarter.ana` is the model that proved the OLD (v1-flat) converter
//! silently wrong: it dropped the axis of `Sum(Transfer_matrix, Month)` and
//! emitted `sum_array` (a grand total), yielding one number `4_336_252` instead
//! of the per-quarter series. The v2-native converter emits real `dimensions[]`,
//! nested `vector_map` tables, and axis-selective `sum_array(x, <1-based pos>)`
//! (the position computed against the engine's align-by-name axis sort), so the
//! quarterly totals now come out correct.
//!
//! Ground truth, hand-computed from the raw `.ana` tables (days-per-month ×
//! monthly BPD × 1000, summed per quarter):
//!   2003: [1_801_975, 1_788_354, 1_859_070, 1_862_881]
//!   2004: [1_874_180, 1_869_453, 1_915_018, 1_928_940]
//!   2005: [1_857_136, 1_229_679, 0, 0]   (data only runs through May 2005)
//!
//! Skipped when `python3` is unavailable (no-op in Python-less CI).

use std::process::Command;
use wasim_engine::{simulate_json, RunConfig};

fn convert(fixture: &str) -> Option<String> {
    let ok = Command::new("python3").arg("--version").output().map(|o| o.status.success()).unwrap_or(false);
    if !ok {
        eprintln!("skip: python3 unavailable");
        return None;
    }
    let ana = format!("{}/../tools/fixtures/ana_corpus/{fixture}", env!("CARGO_MANIFEST_DIR"));
    let conv = format!("{}/../tools/ana_to_wasim.py", env!("CARGO_MANIFEST_DIR"));
    let out = Command::new("python3").arg(conv).arg(ana).arg("--stdout").output().expect("run converter");
    assert!(out.status.success(), "converter failed: {}", String::from_utf8_lossy(&out.stderr));
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[test]
fn month_to_quarter_yields_correct_quarterly_vector() {
    let Some(json) = convert("Month_to_quarter.ana") else { return };
    let r = simulate_json(&json, &RunConfig { n_realizations: Some(1), ..Default::default() })
        .expect("engine run");

    // The dimensioned `Qtr_us_oil_consump` surfaces per-member as `id#k`, in
    // `[Quarter, Year]` layout → k = (quarter-1)*3 + year, 1-based.
    let cell = |quarter: usize, year: usize| -> f64 {
        let k = (quarter - 1) * 3 + year;
        let id = format!("Qtr_us_oil_consump#{k}");
        r.elements.get(&id)
            .and_then(|e| e.final_values.first().copied())
            .unwrap_or_else(|| panic!("missing {id}"))
    };

    let expected = [
        [1_801_975.0, 1_874_180.0, 1_857_136.0], // Q1 across 2003,2004,2005
        [1_788_354.0, 1_869_453.0, 1_229_679.0], // Q2
        [1_859_070.0, 1_915_018.0, 0.0],         // Q3
        [1_862_881.0, 1_928_940.0, 0.0],         // Q4
    ];
    for (qi, row) in expected.iter().enumerate() {
        for (yi, &want) in row.iter().enumerate() {
            let got = cell(qi + 1, yi + 1);
            assert!((got - want).abs() < 1.0,
                "Q{} {}: expected {want}, got {got} — axis-Sum fidelity broken",
                qi + 1, 2003 + yi);
        }
    }
}
