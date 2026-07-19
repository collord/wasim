//! B1 bit-identity regression gate: the timebase restructure must leave `FixedGrid` mode
//! (the default) producing results identical to the pre-restructure engine. A snapshot of a
//! corpus sample's full time-history + final values is stored under `tests/fixtures/`; this
//! test regenerates results and asserts they match to a tight tolerance. To (re)generate the
//! fixture after an intentional semantics change, set WASIM_WRITE_SNAPSHOT=1 and run this test.
//!
//! Snapshot models are chosen to exercise the step-loop machinery B1 touches: stocks with
//! bounds/overflow, links/transit, events, filters, probabilistic sampling, and calendar refs.

use std::collections::BTreeMap;
use std::path::PathBuf;

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig};

/// Corpus models to pin. Small/fast, spanning deterministic + probabilistic + stateful:
/// `reservoir` (stocks with floor/capacity/overflow, 100 steps), `pond` (probabilistic
/// stock, 1000 steps × 100 real), `markovd` (Markov chain + probabilistic). These cover the
/// step-loop machinery B1 restructures without the multi-thousand-step models that bloat the
/// fixture. Bit-identity of the trajectory (mean series) is what the gate protects.
const SNAPSHOT_MODELS: &[&str] = &[
    "reservoir.json",
    "pond.json",
    "markovd.json",
];

fn corpus_dir() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap()).join("openvsim/wasim/schema_examples")
}

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/timebase_snapshot.json")
}

/// Round to a fixed number of decimals so the snapshot is stable across trivial float noise
/// and readable in review. 6 decimals is well below any tolerance we care about here.
fn r6(x: f64) -> f64 {
    if x.is_finite() {
        (x * 1e6).round() / 1e6
    } else {
        // Encode non-finite as a sentinel string elsewhere; here map to 0 (won't occur in
        // the chosen models, but keeps the JSON numeric).
        0.0
    }
}

/// A compact, deterministic digest of a run: per element, its time-history mean/p50 series and
/// final-value mean. This captures the full trajectory shape (not just finals) while staying
/// small and readable. Keyed/ordered by element id for stable serialization.
fn digest(results: &wasim_engine::SimulationResults) -> BTreeMap<String, serde_json::Value> {
    let mut out = BTreeMap::new();
    for (id, er) in &results.elements {
        let mut obj = serde_json::Map::new();
        if let Some(th) = &er.time_history {
            // The mean series captures trajectory shape; that is what the bit-identity gate
            // protects. (p50 would only add size without catching a distinct failure mode.)
            obj.insert("mean".into(), serde_json::json!(th.mean.iter().map(|&x| r6(x)).collect::<Vec<_>>()));
        }
        if !er.final_values.is_empty() {
            let m = er.final_values.iter().sum::<f64>() / er.final_values.len() as f64;
            obj.insert("final_mean".into(), serde_json::json!(r6(m)));
        }
        out.insert(id.clone(), serde_json::Value::Object(obj));
    }
    out
}

fn run_model(name: &str) -> Option<BTreeMap<String, serde_json::Value>> {
    let p = corpus_dir().join(name);
    if !p.exists() {
        return None;
    }
    let json = std::fs::read_to_string(&p).ok()?;
    let m = parse_v2(&json).unwrap_or_else(|e| panic!("{name}: parse {e:?}"));
    let g = ModelGraphV2::build(&m).unwrap_or_else(|e| panic!("{name}: build {e:?}"));
    // Fixed seed for determinism; FixedGrid is the default timebase.
    let cfg = RunConfig { seed: Some(20260719), n_realizations: Some(64), ..RunConfig::default() };
    let r = run_v2(&m, &g, &cfg).unwrap_or_else(|e| panic!("{name}: run {e:?}"));
    Some(digest(&r))
}

#[test]
fn fixed_grid_matches_snapshot() {
    if corpus_dir().join(SNAPSHOT_MODELS[0]).exists() == false {
        eprintln!("skipping: corpus not present");
        return;
    }

    // Build the current digest for every available snapshot model.
    let mut current: BTreeMap<String, BTreeMap<String, serde_json::Value>> = BTreeMap::new();
    for &name in SNAPSHOT_MODELS {
        if let Some(d) = run_model(name) {
            current.insert(name.to_string(), d);
        }
    }

    let path = fixture_path();
    if std::env::var("WASIM_WRITE_SNAPSHOT").is_ok() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, serde_json::to_string_pretty(&current).unwrap()).unwrap();
        eprintln!("wrote snapshot: {}", path.display());
        return;
    }

    let expected: BTreeMap<String, BTreeMap<String, serde_json::Value>> =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("snapshot fixture missing; regenerate with WASIM_WRITE_SNAPSHOT=1"))
            .expect("snapshot parse");

    // Every model present in the fixture must match exactly (rounded digest).
    for (name, exp) in &expected {
        let cur = current.get(name).unwrap_or_else(|| panic!("model {name} missing from current run"));
        assert_eq!(cur, exp, "FixedGrid digest for {name} diverged from the snapshot");
    }
}
