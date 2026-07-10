//! Unit registry/conversion and load-time dimensional validation.

use wasim_engine::parse_v2;
use wasim_engine::units::{convert, display_conversion, validate};

fn approx(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-6, "{a} vs {b}");
}

#[test]
fn conversion_within_dimension() {
    approx(convert(1.0, "yr", "d").unwrap(), 365.25);
    approx(convert(1000.0, "g", "kg").unwrap(), 1.0);
    approx(convert(1.0, "km", "m").unwrap(), 1000.0);
    // Composite (rate): 1 m3/d = 365.25 m3/yr.
    approx(convert(1.0, "m3/d", "m3/yr").unwrap(), 365.25);
}

#[test]
fn display_conversion_factor_and_offset() {
    let close = |a: Option<(f64, f64)>, f: f64, o: f64| {
        let (af, ao) = a.expect("expected a conversion");
        assert!((af - f).abs() < 1e-6 * f.abs().max(1.0), "factor {af} vs {f}");
        assert!((ao - o).abs() < 1e-6, "offset {ao} vs {o}");
    };
    close(display_conversion("m^3/s", "m3/day"), 86400.0, 0.0); // SI rate → per-day
    close(display_conversion("1", "%"), 100.0, 0.0);           // fraction → percent
    close(display_conversion("m", "mm"), 1000.0, 0.0);
    close(display_conversion("m^3", "m3"), 1.0, 0.0);          // notation relabel
    close(display_conversion("K", "C"), 1.0, -273.15);         // temperature offset
    close(display_conversion("1", "pers"), 1.0, 0.0);          // dimensionless relabel
    assert!(display_conversion("m", "s").is_none(), "dimension mismatch → None");
}

#[test]
fn conversion_rejects_cross_dimension_and_unknown() {
    assert!(convert(1.0, "kg", "m").is_none(), "mass↛length");
    assert!(convert(1.0, "m3/d", "kg/d").is_none(), "volume-rate↛mass-rate");
    assert!(convert(1.0, "furlong", "m").is_none(), "unknown unit");
}

fn model(json: &str) -> wasim_engine::ModelV2 {
    parse_v2(json).expect("parse")
}

#[test]
fn validate_passes_consistent_model() {
    let m = model(
        r#"{"wasim_version": "0.8.0",
        "simulation_settings": {"duration": {"value": 10, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}},
        "elements": [
          {"id": "s", "name": "S", "primitive": "stock", "initial_value": {"value": 0, "unit": "m3"}, "rate": {"value": 1, "unit": "m3/d"}}
        ]}"#,
    );
    assert!(validate(&m).is_empty(), "consistent units → no warnings: {:?}", validate(&m));
}

#[test]
fn validate_flags_rate_timestep_mismatch() {
    let m = model(
        r#"{"wasim_version": "0.8.0",
        "simulation_settings": {"duration": {"value": 10, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}},
        "elements": [
          {"id": "s", "name": "S", "primitive": "stock", "initial_value": {"value": 0, "unit": "m3"}, "rate": {"value": 1, "unit": "m3/yr"}}
        ]}"#,
    );
    let w = validate(&m);
    assert!(w.iter().any(|s| s.contains("timestep") && s.contains("off by")), "expected rate/timestep warning, got {w:?}");
}

#[test]
fn validate_flags_unknown_unit() {
    let m = model(
        r#"{"wasim_version": "0.8.0",
        "simulation_settings": {"duration": {"value": 10, "unit": "d"}, "timestep": {"value": 1, "unit": "d"}},
        "elements": [
          {"id": "k", "name": "K", "primitive": "node", "value_rule": "fixed", "value": {"value": 1, "unit": "furlong"}}
        ]}"#,
    );
    let w = validate(&m);
    assert!(w.iter().any(|s| s.contains("furlong") && s.contains("unrecognized")), "expected unknown-unit warning, got {w:?}");
}
