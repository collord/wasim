//! Unit registry, conversion, and load-time dimensional validation.
//!
//! Per semantics §12, unit consistency is the model author's/transpiler's responsibility and
//! the engine does not perform runtime dimensional analysis. This module provides the SI
//! registry + a `convert` utility, and a load-time `validate` pass that *warns* when a model
//! mixes incompatible units (the real failure mode) — without changing numeric behavior, so
//! the v1≡v2 equivalence is preserved.

use std::collections::BTreeSet;

use crate::model::QuantityOrFormula;
use crate::model_v2::{FixedValue, Model, NodeRule, Primitive};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BaseDim {
    Time,
    Length,
    Mass,
    Volume,
    Dimensionless,
}

/// A unit's dimensional signature: a numerator dimension and an optional denominator
/// (e.g. a rate `m3/d` is `Volume` over `Time`; a concentration `kg/m3` is `Mass` over `Volume`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct UnitDim {
    pub num: BaseDim,
    pub denom: Option<BaseDim>,
}

/// SI factor + base dimension of a simple (non-composite) unit.
fn simple(unit: &str) -> Option<(f64, BaseDim)> {
    use BaseDim::*;
    Some(match unit.trim() {
        // time (→ seconds)
        "s" | "sec" | "second" | "seconds" => (1.0, Time),
        "min" | "minute" | "minutes" => (60.0, Time),
        "h" | "hr" | "hour" | "hours" => (3600.0, Time),
        "d" | "day" | "days" => (86400.0, Time),
        "wk" | "week" | "weeks" => (604800.0, Time),
        "mo" | "month" | "months" => (2_629_800.0, Time), // 365.25/12 d
        "yr" | "year" | "years" => (31_557_600.0, Time),  // 365.25 d
        // length (→ metres)
        "m" | "meter" | "meters" => (1.0, Length),
        "cm" => (0.01, Length),
        "mm" => (0.001, Length),
        "km" => (1000.0, Length),
        "ft" | "feet" => (0.3048, Length),
        "in" | "inch" => (0.0254, Length),
        "mi" | "mile" => (1609.344, Length),
        // mass (→ kg)
        "kg" => (1.0, Mass),
        "g" => (0.001, Mass),
        "mg" => (1e-6, Mass),
        "µg" | "ug" => (1e-9, Mass),
        "lb" => (0.453592, Mass),
        "t" | "tonne" => (1000.0, Mass),
        // volume (→ m3)
        "m3" | "m^3" => (1.0, Volume),
        "l" | "L" | "litre" | "liter" => (0.001, Volume),
        "ml" | "mL" => (1e-6, Volume),
        "kl" | "kL" | "kiloliter" | "kilolitre" => (1.0, Volume),       // 10³ L = 1 m³
        "Ml" | "ML" | "megaliter" | "megalitre" => (1000.0, Volume),    // 10⁶ L
        "gal" => (0.003_785_41, Volume),
        "ft3" => (0.028_316_8, Volume),
        // dimensionless
        "1" | "" | "-" | "frac" | "fraction" => (1.0, Dimensionless),
        "%" => (0.01, Dimensionless),
        _ => return None,
    })
}

/// Parse a unit string into (SI factor, dimensional signature). Supports one `/`.
pub fn parse_unit(unit: &str) -> Option<(f64, UnitDim)> {
    let u = unit.trim();
    if let Some(slash) = u.find('/') {
        let (n, d) = (u[..slash].trim(), u[slash + 1..].trim());
        let (nf, nd) = if n.is_empty() || n == "1" {
            (1.0, BaseDim::Dimensionless)
        } else {
            simple(n)?
        };
        let (df, dd) = simple(d)?;
        Some((nf / df, UnitDim { num: nd, denom: Some(dd) }))
    } else {
        let (f, dim) = simple(u)?;
        Some((f, UnitDim { num: dim, denom: None }))
    }
}

/// Convert a value between two units of the same dimensional signature. `None` if either
/// unit is unknown or their dimensions differ.
pub fn convert(value: f64, from: &str, to: &str) -> Option<f64> {
    let (ff, fd) = parse_unit(from)?;
    let (tf, td) = parse_unit(to)?;
    if fd != td {
        return None;
    }
    Some(value * ff / tf)
}

/// Conversion from a canonical `unit` to a `display_unit`, as `(factor, offset)`:
/// `display_value = value * factor + offset`. Temperature is affine (offset ≠ 0).
/// Returns `None` when no valid conversion exists (unknown units or a genuine dimension
/// mismatch) — callers should then show the canonical unit unchanged.
pub fn display_conversion(unit: &str, display_unit: &str) -> Option<(f64, f64)> {
    let (u, du) = (unit.trim(), display_unit.trim());
    if u == du {
        return Some((1.0, 0.0));
    }
    // Temperature is an affine (offset) conversion, not a pure factor.
    if let (Some((a, b)), Some((c, d))) = (temp_to_celsius(u), celsius_to_temp(du)) {
        return Some((c * a, c * b + d));
    }
    // Same-dimension factor conversion.
    if let (Some((uf, ud)), Some((df, dd))) = (parse_unit(u), parse_unit(du)) {
        return if ud == dd { Some((uf / df, 0.0)) } else { None };
    }
    // Dimensionless relabel (persons, currency, counts): canonical is dimensionless and the
    // display unit is a non-unit label.
    if matches!(u, "1" | "") && parse_unit(du).is_none() && temp_to_celsius(du).is_none() {
        return Some((1.0, 0.0));
    }
    None
}

/// (a, b) s.t. `celsius = a·value + b`, for a temperature unit.
fn temp_to_celsius(unit: &str) -> Option<(f64, f64)> {
    match unit {
        "C" | "°C" | "degC" | "celsius" | "Celsius" => Some((1.0, 0.0)),
        "K" | "kelvin" | "Kelvin" => Some((1.0, -273.15)),
        "F" | "°F" | "degF" | "fahrenheit" | "Fahrenheit" => Some((1.0 / 1.8, -32.0 / 1.8)),
        _ => None,
    }
}

/// (c, d) s.t. `display = c·celsius + d`, for a temperature unit.
fn celsius_to_temp(unit: &str) -> Option<(f64, f64)> {
    match unit {
        "C" | "°C" | "degC" | "celsius" | "Celsius" => Some((1.0, 0.0)),
        "K" | "kelvin" | "Kelvin" => Some((1.0, 273.15)),
        "F" | "°F" | "degF" | "fahrenheit" | "Fahrenheit" => Some((1.8, 32.0)),
        _ => None,
    }
}

/// Load-time dimensional validation. Returns human-readable warnings; the engine continues
/// with declared values regardless.
pub fn validate(model: &Model) -> Vec<String> {
    let mut warnings = Vec::new();
    let mut unknown: BTreeSet<String> = BTreeSet::new();

    let ts_unit = &model.simulation_settings.timestep.unit;
    note(&model.simulation_settings.duration.unit, &mut unknown);
    note(ts_unit, &mut unknown);

    for elem in &model.elements {
        let id = elem.id();
        match &elem.primitive {
            Primitive::Node(n) => match &n.rule {
                NodeRule::Fixed { value: FixedValue::Scalar(q), .. } => note(&q.unit, &mut unknown),
                NodeRule::Fixed { value: FixedValue::Array { unit, .. }, .. } => note(unit, &mut unknown),
                _ => {}
            },
            Primitive::Stock(s) => {
                note(&s.initial_value.unit, &mut unknown);
                check_rate(&s.rate, ts_unit, &mut warnings, id);
            }
            Primitive::Link(l) => check_rate(&l.rate, ts_unit, &mut warnings, id),
            Primitive::Event(e) => check_rate(&e.rate, ts_unit, &mut warnings, id),
            _ => {}
        }
    }

    for u in unknown {
        warnings.push(format!("unrecognized unit '{u}' (not in the SI registry)"));
    }
    warnings
}

fn note(unit: &str, unknown: &mut BTreeSet<String>) {
    if !unit.is_empty() && parse_unit(unit).is_none() {
        unknown.insert(unit.to_string());
    }
}

/// Warn if a rate's time denominator is on a different time scale than the timestep
/// (e.g. a `/yr` rate integrated against a `d` timestep is off by ~365×).
fn check_rate(rate: &Option<QuantityOrFormula>, ts_unit: &str, w: &mut Vec<String>, id: &str) {
    let Some(QuantityOrFormula::Quantity(q)) = rate else { return };
    let Some(slash) = q.unit.find('/') else { return };
    let denom = q.unit[slash + 1..].trim();
    let (Some((df, dd)), Some((tf, td))) = (parse_unit(denom), parse_unit(ts_unit)) else { return };
    if dd.num == BaseDim::Time && dd.denom.is_none() && td.num == BaseDim::Time && (df - tf).abs() > 1e-6 {
        w.push(format!(
            "element '{id}': rate per '{denom}' but timestep is '{ts_unit}' — integration may be off by ~{:.0}×",
            df / tf
        ));
    }
}
