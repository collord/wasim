pub mod engine;
pub mod engine_v2;
pub mod error;
pub mod eval;
pub mod eval_harness;
pub mod graph;
pub mod graph_v2;
pub mod model;
pub mod model_v2;
pub mod optimize_v2;
pub mod params;
pub mod sampling;
pub mod sensitivity_v2;
pub mod submodel_v2;
pub mod summary;
pub mod units;
pub mod v1_import;
pub mod v2_parse;

#[cfg(target_arch = "wasm32")]
pub mod wasm;

pub use engine::{run, ElementResults, RunConfig, SimulationResults, TimeHistoryStats};
pub use engine_v2::run as run_v2;
pub use error::EngineError;
pub use graph::ModelGraph;
pub use graph_v2::ModelGraphV2;
pub use model::WasimModel;
pub use model_v2::Model as ModelV2;
pub use optimize_v2::{optimize, StudyResults};
pub use params::ModelParams;
pub use sensitivity_v2::{sensitivity, SensitivityResults, SensitivitySpec};
pub use v1_import::normalize as normalize_v1;
pub use v2_parse::parse as parse_v2;

// ── Canonical entry points ────────────────────────────────────────────────────
//
// The v2 engine core (`run_v2`) is the engine. All input flows through it: a v1
// model is normalized into the v2 primitive model first. The legacy v1 engine
// (`run`) is retained only as the equivalence reference behind the corpus test.

/// Run a v1 model through the v2 engine core (normalize → v2 graph → v2 run).
pub fn simulate(model: &WasimModel, config: &RunConfig) -> Result<SimulationResults, EngineError> {
    let v2 = normalize_v1(model);
    warn_units(&v2);
    let graph = ModelGraphV2::build(&v2)?;
    run_v2(&v2, &graph, config)
}

/// Load a model from JSON — v2-native when its first element carries a `primitive`
/// field, else v1 — and run it through the v2 engine core.
pub fn simulate_json(json: &str, config: &RunConfig) -> Result<SimulationResults, EngineError> {
    if is_v2_native(json)? {
        let m = parse_v2(json)?;
        warn_units(&m);
        let graph = ModelGraphV2::build(&m)?;
        run_v2(&m, &graph, config)
    } else {
        let m: WasimModel = serde_json::from_str(json)?;
        simulate(&m, config)
    }
}

fn warn_units(model: &ModelV2) {
    for w in units::validate(model) {
        eprintln!("warn: unit check: {w}");
    }
}

fn is_v2_native(json: &str) -> Result<bool, EngineError> {
    let v: serde_json::Value = serde_json::from_str(json)?;
    Ok(v.get("elements")
        .and_then(|e| e.as_array())
        .and_then(|arr| arr.first())
        .map(|first| first.get("primitive").is_some())
        .unwrap_or(false))
}
