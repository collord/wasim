//! Regression bridge tests: v1 models normalize into the v2 primitive model.
//!
//! These do not run the engine (the v2 engine path is a later increment) — they
//! assert the normalizer produces a structurally sound v2 model for hand-written
//! fixtures and for the whole transpiled corpus.

use std::fs;
use std::path::PathBuf;

use wasim_engine::model_v2::{FixedValue, NodeRule, Primitive};
use wasim_engine::v1_import::normalize;
use wasim_engine::WasimModel;

fn load(json: &str) -> WasimModel {
    serde_json::from_str(json).expect("parse failed")
}

/// v2-native models (first element has a `primitive` field) don't go through the v1
/// normalizer — this suite only exercises v1→v2 import.
fn is_v2_native(json: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(json)
        .ok()
        .and_then(|v| {
            v.get("elements")
                .and_then(|e| e.as_array())
                .and_then(|a| a.first())
                .map(|f| f.get("primitive").is_some())
        })
        .unwrap_or(false)
}

fn openvsim_examples_dir() -> PathBuf {
    std::env::var("WASIM_SCHEMA_EXAMPLES")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").expect("HOME not set");
            PathBuf::from(home).join("openvsim/wasim/schema_examples")
        })
}

// ── Focused mapping checks (corpus-independent) ───────────────────────────────

#[test]
fn constant_maps_to_node_fixed() {
    let m = load(
        r#"{
        "wasim_version": "0.1.0",
        "simulation_settings": {"duration": {"value": 1, "unit": "yr"}, "timestep": {"value": 1, "unit": "yr"}},
        "elements": [{"id": "a", "name": "A", "type": "constant", "value": {"value": 5.0, "unit": "kg"}}]
    }"#,
    );
    let v2 = normalize(&m);
    assert!(v2.from_v1);
    assert_eq!(v2.elements.len(), 1);
    match &v2.elements[0].primitive {
        Primitive::Node(n) => match &n.rule {
            NodeRule::Fixed { value: FixedValue::Scalar(q), .. } => {
                assert_eq!(q.value, 5.0);
                assert_eq!(q.unit, "kg");
            }
            other => panic!("expected fixed scalar, got {other:?}"),
        },
        other => panic!("expected node, got {other:?}"),
    }
}

#[test]
fn accumulator_maps_to_stock_with_floor() {
    let m = load(
        r#"{
        "wasim_version": "0.1.0",
        "simulation_settings": {"duration": {"value": 10, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}},
        "elements": [{
            "id": "tank", "name": "Tank", "type": "accumulator",
            "initial_value": {"value": 2.0, "unit": "m3"},
            "rate": {"ast": {"op": "literal", "value": 1.0}},
            "capacity": {"value": 100.0, "unit": "m3"}
        }]
    }"#,
    );
    let v2 = normalize(&m);
    let stock = v2.elements[0].as_stock().expect("expected stock");
    assert_eq!(stock.initial_value.value, 2.0);
    assert!(stock.rate.is_some());
    assert!(stock.capacity.is_some(), "capacity trait should carry over");
    // min_value defaults to 0.0 in v1 → floor 0.0 in v2.
    assert_eq!(stock.floor.as_ref().map(|q| q.value), Some(0.0));
}

#[test]
fn multistep_delay_expands_to_chained_lags() {
    // lag = 3 d, dt = 1 d → 3 chained one-step lag nodes (R3 / semantics §2.7).
    let m = load(
        r#"{
        "wasim_version": "0.1.0",
        "simulation_settings": {"duration": {"value": 10, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}},
        "elements": [
            {"id": "src", "name": "Src", "type": "constant", "value": {"value": 1.0, "unit": "1"}},
            {"id": "d", "name": "D", "type": "delay", "input": "src", "lag": {"value": 3.0, "unit": "d"}}
        ]
    }"#,
    );
    let v2 = normalize(&m);
    let ids: Vec<&str> = v2.elements.iter().map(|e| e.id()).collect();
    assert!(ids.contains(&"src"));
    // 3-step chain: two synthetic intermediates + the final node keeping id "d".
    assert!(ids.contains(&"d__lag1"), "ids = {ids:?}");
    assert!(ids.contains(&"d__lag2"), "ids = {ids:?}");
    assert!(ids.contains(&"d"), "final node keeps original id; ids = {ids:?}");

    // Chain wiring: d__lag1←src, d__lag2←d__lag1, d←d__lag2.
    let input_of = |id: &str| -> String {
        let e = v2.elements.iter().find(|e| e.id() == id).unwrap();
        match &e.primitive {
            Primitive::Node(n) => match &n.rule {
                NodeRule::Lag { input, .. } => input.clone().expect("lag input"),
                other => panic!("{id}: expected lag, got {other:?}"),
            },
            _ => panic!("{id}: expected node"),
        }
    };
    assert_eq!(input_of("d__lag1"), "src");
    assert_eq!(input_of("d__lag2"), "d__lag1");
    assert_eq!(input_of("d"), "d__lag2");
}

#[test]
fn singlestep_delay_stays_one_lag() {
    let m = load(
        r#"{
        "wasim_version": "0.1.0",
        "simulation_settings": {"duration": {"value": 5, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}},
        "elements": [
            {"id": "src", "name": "Src", "type": "constant", "value": {"value": 1.0, "unit": "1"}},
            {"id": "d", "name": "D", "type": "delay", "input": "src", "lag": {"value": 1.0, "unit": "d"}}
        ]
    }"#,
    );
    let v2 = normalize(&m);
    assert_eq!(v2.elements.len(), 2, "no chain expansion for a 1-step delay");
}

// ── Whole-corpus structural pass ──────────────────────────────────────────────

#[test]
fn normalize_all_schema_examples() {
    let dir = openvsim_examples_dir();
    if !dir.exists() {
        eprintln!("skipping normalize_all_schema_examples: {} not present", dir.display());
        return;
    }

    let mut count = 0;
    let mut failures: Vec<String> = vec![];

    for entry in fs::read_dir(&dir).expect("schema_examples not found") {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let json = fs::read_to_string(&path).unwrap();
        if is_v2_native(&json) {
            continue; // v2-native files skip the v1 normalizer
        }
        let model: WasimModel = match serde_json::from_str(&json) {
            Ok(m) => m,
            Err(e) => {
                failures.push(format!("{name}: v1 parse failed: {e}"));
                continue;
            }
        };

        let n_v1 = model.elements.len();
        // Delay elements expand into chains; account for the extra nodes.
        let v2 = normalize(&model);

        // Every non-delay v1 element id must survive into the v2 model (delays keep
        // their id on the final chain node, so they survive too).
        let v2_ids: std::collections::HashSet<&str> = v2.elements.iter().map(|e| e.id()).collect();
        for e in &model.elements {
            if !v2_ids.contains(e.id.as_str()) {
                failures.push(format!("{name}: element '{}' dropped during normalize", e.id));
            }
        }

        if v2.elements.len() < n_v1 {
            failures.push(format!(
                "{name}: v2 element count {} < v1 count {n_v1}",
                v2.elements.len()
            ));
        }
        if !v2.from_v1 {
            failures.push(format!("{name}: from_v1 flag not set"));
        }
        count += 1;
    }

    if !failures.is_empty() {
        panic!("normalize failures ({}):\n{}", failures.len(), failures.join("\n"));
    }
    // Most of the corpus is now v2-native; only the remaining v1 files exercise the normalizer.
    assert!(count >= 1, "expected at least one v1 model in the corpus, found {count}");
    eprintln!("normalized {count} v1 corpus models into v2 with no structural failures");
}
