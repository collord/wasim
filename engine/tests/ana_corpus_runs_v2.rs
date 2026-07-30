//! Executable-fidelity gate for the real `.ana` corpus (Rung 1).
//!
//! `tools/test_ana_corpus.py` already asserts every fixture in
//! `tools/fixtures/ana_corpus/` *converts* and is JSON-Schema valid. That is a
//! static check — it never executes anything. This test closes the loop: it
//! converts each model live via `tools/ana_to_wasim.py --stdout`, then runs the
//! output through the v2 engine and asserts it executes without error.
//!
//! Same philosophy as `platform_conversion_v2.rs`: the converter's *structural*
//! fidelity is the Python side's job; this only nails down "the converted model
//! still runs". It caught the two-arg `Round(x, digits)` defect the smoke test
//! could not see (Analytica's `Round` takes an optional decimal-places arg; the
//! engine's builtin was 1-arg only).
//!
//! Skipped when `python3` is unavailable (so it is a no-op in Python-less CI)
//! rather than failing spuriously.

use std::path::PathBuf;
use std::process::Command;
use wasim_engine::{simulate_json, RunConfig};

fn corpus_dir() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../tools/fixtures/ana_corpus"))
}

fn converter() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../tools/ana_to_wasim.py"))
}

fn python3_available() -> bool {
    Command::new("python3")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Every `.ana`/`.ANA` fixture, sorted, so the assertion order is deterministic.
fn corpus_files() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(corpus_dir())
        .expect("read ana_corpus dir")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|x| x.to_str())
                .map(|x| x.eq_ignore_ascii_case("ana"))
                .unwrap_or(false)
        })
        .collect();
    v.sort();
    v
}

#[test]
fn every_converted_corpus_model_runs() {
    if !python3_available() {
        eprintln!("skip: python3 not available — cannot run the converter");
        return;
    }

    let files = corpus_files();
    assert!(!files.is_empty(), "corpus is empty — expected .ana fixtures");

    // A few realizations is enough to exercise the whole graph; keep it small so
    // the ~38-model sweep stays fast.
    let cfg = RunConfig { n_realizations: Some(16), ..Default::default() };

    let mut failures: Vec<String> = Vec::new();
    for path in &files {
        let name = path.file_name().unwrap().to_string_lossy().to_string();

        let out = Command::new("python3")
            .arg(converter())
            .arg(path)
            .arg("--stdout")
            .output()
            .expect("spawn ana_to_wasim.py");
        if !out.status.success() {
            failures.push(format!(
                "{name}: converter exited {}: {}",
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            ));
            continue;
        }
        let json = String::from_utf8_lossy(&out.stdout);

        match simulate_json(&json, &cfg) {
            Ok(_) => {}
            Err(e) => failures.push(format!("{name}: engine run failed: {e:?}")),
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} converted corpus models failed to run:\n  {}",
        failures.len(),
        files.len(),
        failures.join("\n  ")
    );
}
