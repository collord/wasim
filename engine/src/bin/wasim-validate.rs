//! `wasim-validate` — a headless engine validator + quick-run for the LLM-authoring spike
//! (WASIM_AUTHORING_ENVIRONMENT_SPEC.md §17.3 `validate()` / `run()` tools). It is the engine
//! ground-truth the copilot loop leans on: an LLM drafts a `model.json`, this reports structured
//! diagnostics (dangling refs, cycles, unit smells), and the drafter self-corrects until clean.
//!
//! This replicates the wasm bridge's `validate_json` (engine/src/wasm.rs) for a native target,
//! because that module is `#[cfg(target_arch = "wasm32")]`-gated and unavailable off-wasm.
//!
//! Usage:
//!   wasim-validate <model.json>            # validate; exit 0 if clean, 1 if errors
//!   wasim-validate <model.json> --run      # also run a quick sim + print a compact summary
//!   wasim-validate <model.json> --trajectories  # --run + full per-step mean series
//!   cat model.json | wasim-validate -      # read from stdin
//!
//! Always prints a JSON report to stdout: { ok, errors[], warnings[], topo[], run?, trajectories? }.
//! `--trajectories` adds `{ time_axis, time_unit, series: { <id>: [mean per step] } }` — the
//! per-timestep mean of each history-saved element. For a deterministic import (n_realizations
//! collapses to a single trajectory) the mean IS the trajectory, which is what the Vensim/XMILE
//! differential harness (tools/mdl_challenge.py) compares against a reference simulator.

use std::io::Read;
use wasim_engine::{parse_v2, run_v2, units, ModelGraphV2, RunConfig, TimebaseMode, UnitsMode};

fn read_input(arg: &str) -> std::io::Result<String> {
    if arg == "-" {
        let mut s = String::new();
        std::io::stdin().read_to_string(&mut s)?;
        Ok(s)
    } else {
        std::fs::read_to_string(arg)
    }
}

/// A default fast-run config: a few realizations, fixed seed, everything else off/default.
fn quick_config() -> RunConfig {
    RunConfig {
        n_realizations: Some(64),
        seed: Some(0),
        duration_override: None,
        timestep_override: None,
        results_spec: None,
        timebase: TimebaseMode::Fixed,
        units: UnitsMode::Warn,
        realization_weights: Vec::new(),
        array_lane: false,
        close_at_terminal: None,
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 || args[1] == "--help" || args[1] == "-h" {
        eprintln!("usage: wasim-validate <model.json|-> [--run]");
        std::process::exit(2);
    }
    let want_traj = args.iter().any(|a| a == "--trajectories");
    let want_run = want_traj || args.iter().any(|a| a == "--run");

    let json = match read_input(&args[1]) {
        Ok(s) => s,
        Err(e) => {
            println!("{{\"ok\":false,\"errors\":[\"cannot read input: {e}\"],\"warnings\":[],\"topo\":[]}}");
            std::process::exit(1);
        }
    };

    let mut errors: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut topo: Vec<String> = Vec::new();
    let mut run_summary: Option<String> = None;
    let mut trajectories: Option<serde_json::Value> = None;

    match parse_v2(&json) {
        Ok(model) => {
            warnings = units::validate(&model);
            match ModelGraphV2::build(&model) {
                Ok(graph) => {
                    topo = graph.topo_order.clone();
                    if want_run {
                        match run_v2(&model, &graph, &quick_config()) {
                            Ok(results) => {
                                run_summary = Some(summarize_run(&results));
                                if want_traj {
                                    trajectories = Some(dump_trajectories(&results));
                                }
                            }
                            Err(e) => errors.push(format!("run error: {e}")),
                        }
                    }
                }
                Err(e) => errors.push(format!("graph error: {e}")),
            }
        }
        Err(e) => errors.push(format!("parse error: {e}")),
    }

    let ok = errors.is_empty();
    // Human-readable digest to stderr (for a person watching the loop); JSON report to stdout.
    eprintln!(
        "{} — {} error(s), {} warning(s){}",
        if ok { "VALID" } else { "INVALID" },
        errors.len(),
        warnings.len(),
        if run_summary.is_some() { ", ran" } else { "" },
    );
    for e in &errors {
        eprintln!("  ✗ {e}");
    }
    for w in &warnings {
        eprintln!("  ⚠ {w}");
    }

    let report = serde_json::json!({
        "ok": ok,
        "errors": errors,
        "warnings": warnings,
        "topo": topo,
        "run": run_summary.map(|s| serde_json::from_str::<serde_json::Value>(&s).unwrap_or(serde_json::Value::Null)),
        "trajectories": trajectories,
    });
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    std::process::exit(if ok { 0 } else { 1 });
}

/// A compact per-output summary (mean/min/max of final values) — enough for the LLM to
/// sanity-check behavior without drowning in per-realization data (§17.3 `run()`).
fn summarize_run(results: &wasim_engine::SimulationResults) -> String {
    let mut outputs = serde_json::Map::new();
    for id in &results.output_ids {
        if let Some(er) = results.elements.get(id) {
            let fv = &er.final_values;
            if fv.is_empty() {
                continue;
            }
            let n = fv.len() as f64;
            let mean = fv.iter().sum::<f64>() / n;
            let min = fv.iter().cloned().fold(f64::INFINITY, f64::min);
            let max = fv.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            outputs.insert(
                id.clone(),
                serde_json::json!({ "label": er.label, "unit": er.unit, "mean": mean, "min": min, "max": max }),
            );
        }
    }
    serde_json::to_string(&serde_json::json!({
        "n_realizations": results.n_realizations,
        "n_steps": results.n_steps,
        "final_value_summary": outputs,
    }))
    .unwrap_or_else(|_| "{}".to_string())
}

/// Full per-timestep mean series for every history-saved element, plus the time axis.
/// For a deterministic import the mean over realizations equals the single trajectory, so this
/// is the panel the differential harness (tools/mdl_challenge.py) diffs against a reference
/// simulator such as pysimlin. Elements without a saved time history (e.g. fixed scalars) are
/// omitted; the harness matches on the intersection of names it can align.
fn dump_trajectories(results: &wasim_engine::SimulationResults) -> serde_json::Value {
    let mut series = serde_json::Map::new();
    for (id, er) in &results.elements {
        if let Some(th) = &er.time_history {
            series.insert(id.clone(), serde_json::json!(th.mean));
        }
    }
    serde_json::json!({
        "time_axis": results.time_axis,
        "time_unit": results.time_unit,
        "series": series,
    })
}
