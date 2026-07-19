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

pub mod dim {
    //! Full dimensional signature as an exponent vector over the independent base dimensions
    //! {Time, Length, Mass, Volume, Temperature}. Composes correctly under ×, ÷, and integer
    //! powers — unlike the single num/denom `UnitDim`, which cannot represent m², mass²/vol, etc.
    //! Used by the B5 static dimension checker (`infer_dim`). Dimensionless = all exponents zero.

    /// Exponent per base dimension: [Time, Length, Mass, Volume, Temperature].
    #[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
    pub struct Dim(pub [i8; 5]);

    pub const DIMENSIONLESS: Dim = Dim([0, 0, 0, 0, 0]);
    pub const TIME: Dim = Dim([1, 0, 0, 0, 0]);
    pub const LENGTH: Dim = Dim([0, 1, 0, 0, 0]);
    pub const MASS: Dim = Dim([0, 0, 1, 0, 0]);
    pub const VOLUME: Dim = Dim([0, 0, 0, 1, 0]);
    pub const TEMPERATURE: Dim = Dim([0, 0, 0, 0, 1]);

    impl Dim {
        pub fn is_dimensionless(&self) -> bool {
            self.0 == [0; 5]
        }
        pub fn mul(self, other: Dim) -> Dim {
            let mut d = self.0;
            for i in 0..5 {
                d[i] += other.0[i];
            }
            Dim(d)
        }
        pub fn div(self, other: Dim) -> Dim {
            let mut d = self.0;
            for i in 0..5 {
                d[i] -= other.0[i];
            }
            Dim(d)
        }
        /// Raise to an integer power (used for `pow` with a literal integer exponent).
        pub fn powi(self, n: i8) -> Dim {
            let mut d = self.0;
            for e in d.iter_mut() {
                *e *= n;
            }
            Dim(d)
        }
        /// Halve each exponent (for `sqrt`). `None` if any exponent is odd (non-integer result).
        pub fn sqrt(self) -> Option<Dim> {
            let mut d = self.0;
            for e in d.iter_mut() {
                if *e % 2 != 0 {
                    return None;
                }
                *e /= 2;
            }
            Some(Dim(d))
        }
    }

    /// The full dimension of a base dimension from the `super::BaseDim` enum.
    pub fn of_base(b: super::BaseDim) -> Dim {
        use super::BaseDim::*;
        match b {
            Time => TIME,
            Length => LENGTH,
            Mass => MASS,
            Volume => VOLUME,
            Dimensionless => DIMENSIONLESS,
        }
    }
}

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

/// Full dimensional signature of a unit string (temperature-aware; supports one `/`).
/// `None` = unrecognized unit → the checker treats that subtree as exempt (warn, don't reject).
pub fn dim_of_unit(unit: &str) -> Option<dim::Dim> {
    let u = unit.trim();
    if matches!(u, "1" | "" | "-" | "frac" | "fraction" | "%") {
        return Some(dim::DIMENSIONLESS);
    }
    if temp_to_celsius(u).is_some() {
        return Some(dim::TEMPERATURE);
    }
    // Composite num/denom (one slash).
    if let Some(slash) = u.find('/') {
        let (n, d) = (u[..slash].trim(), u[slash + 1..].trim());
        let nd = if n.is_empty() || n == "1" {
            dim::DIMENSIONLESS
        } else {
            dim::of_base(simple(n)?.1)
        };
        let dd = dim::of_base(simple(d)?.1);
        return Some(nd.div(dd));
    }
    simple(u).map(|(_, b)| dim::of_base(b))
}

/// The declared output dimension of an element (its primary output unit / fixed-scalar unit).
/// `None` if the unit is unrecognized (exempt from checking).
fn element_dim(elem: &crate::model_v2::Element) -> Option<dim::Dim> {
    let unit = match &elem.primitive {
        Primitive::Node(n) => match &n.rule {
            NodeRule::Fixed { value: FixedValue::Scalar(q), .. } => q.unit.as_str(),
            NodeRule::Fixed { value: FixedValue::Array { unit, .. }, .. } => unit.as_str(),
            _ => elem.base.outputs.first().map(|o| o.unit.as_str()).unwrap_or("1"),
        },
        Primitive::Stock(s) => s.initial_value.unit.as_str(),
        _ => elem.base.outputs.first().map(|o| o.unit.as_str()).unwrap_or("1"),
    };
    dim_of_unit(unit)
}

/// Outcome of inferring an AST subtree's dimension.
enum DimResult {
    /// A definite dimension.
    Known(dim::Dim),
    /// Unknown/unresolvable (unrecognized unit, external call, unresolved ref) — exempt.
    Exempt,
    /// A dimensional inconsistency inside the subtree, with a message.
    Mismatch(String),
}

/// Statically infer an AST's dimension, resolving `ref`s against `elem_dims` (element id →
/// declared output dim). Detects: add/sub/compare of unequal dims, transcendentals of a
/// dimensioned argument, sqrt of an odd-exponent dim, and `if` branches of unequal dims.
/// Unknown units / external calls / unresolved refs make the subtree Exempt (never a hard error),
/// so partially-emitted models still load under strict mode.
fn infer_dim(
    ast: &crate::model::AstNode,
    elem_dims: &std::collections::HashMap<String, Option<dim::Dim>>,
    lookups: &std::collections::HashMap<String, (Option<dim::Dim>, Option<dim::Dim>)>,
) -> DimResult {
    use crate::model::{AstNode::*, TimeProperty};
    use DimResult::*;

    // Combine two subtrees, propagating Mismatch/Exempt.
    let both = |l: &crate::model::AstNode, r: &crate::model::AstNode| -> Result<(dim::Dim, dim::Dim), DimResult> {
        let (ld, rd) = (infer_dim(l, elem_dims, lookups), infer_dim(r, elem_dims, lookups));
        match (ld, rd) {
            (Mismatch(m), _) | (_, Mismatch(m)) => Err(Mismatch(m)),
            (Exempt, _) | (_, Exempt) => Err(Exempt),
            (Known(a), Known(b)) => Ok((a, b)),
        }
    };

    match ast {
        // A literal WITH an explicit unit carries that dimension. A bare (unit-less) literal is
        // dimension-agnostic — it takes on whatever dimension its context needs (a constant `5`
        // may be a count, a rate multiplier, or a dimensioned magnitude the emitter left unitless).
        // Treat it as Exempt so a pure-constant expression never trips the declared-vs-inferred
        // check; the real bugs (dimensioned args to transcendentals, add/compare of mismatched
        // *dimensioned* operands) still surface.
        Literal { unit, .. } => match unit {
            Some(u) => dim_of_unit(u).map(Known).unwrap_or(Exempt),
            None => Exempt,
        },
        Ref { element_id, .. } => match elem_dims.get(element_id) {
            Some(Some(d)) => Known(*d),
            Some(None) => Exempt, // referenced element's unit is unrecognized
            None => Exempt,       // unresolved ref (reserved global, submodel port, …)
        },
        TimeRef { property } => match property {
            // Elapsed/timestep carry Time; calendar fields are dimensionless counts.
            TimeProperty::Elapsed | TimeProperty::Timestep => Known(dim::TIME),
            _ => Known(dim::DIMENSIONLESS),
        },
        Add { left, right } | Subtract { left, right } => match both(left, right) {
            Ok((a, b)) if a == b => Known(a),
            Ok((a, b)) => Mismatch(format!("add/subtract of incompatible dimensions {a:?} and {b:?}")),
            Err(e) => e,
        },
        Multiply { left, right } => match both(left, right) {
            Ok((a, b)) => Known(a.mul(b)),
            Err(e) => e,
        },
        Divide { left, right } => match both(left, right) {
            Ok((a, b)) => Known(a.div(b)),
            Err(e) => e,
        },
        Power { left, right } => {
            // Only a literal integer exponent yields a definite dimension.
            let base = infer_dim(left, elem_dims, lookups);
            match (&base, &**right) {
                (Mismatch(_), _) | (Exempt, _) => base,
                (Known(d), Literal { value, .. }) if value.fract() == 0.0 && value.abs() <= 8.0 => {
                    if d.is_dimensionless() { Known(dim::DIMENSIONLESS) } else { Known(d.powi(*value as i8)) }
                }
                // Dimensionless base to any power stays dimensionless; else exempt (non-integer/var exponent).
                (Known(d), _) if d.is_dimensionless() => Known(dim::DIMENSIONLESS),
                _ => Exempt,
            }
        }
        // Comparisons and boolean ops yield a dimensionless 1/0; operands must be comparable.
        Lt { left, right } | Gt { left, right } | Lte { left, right }
        | Gte { left, right } | Eq { left, right } | Neq { left, right } => match both(left, right) {
            Ok((a, b)) if a == b => Known(dim::DIMENSIONLESS),
            Ok((a, b)) => Mismatch(format!("comparison of incompatible dimensions {a:?} and {b:?}")),
            Err(e) => e,
        },
        And { left, right } | Or { left, right } => match both(left, right) {
            Ok(_) => Known(dim::DIMENSIONLESS),
            Err(e) => e,
        },
        Neg { operand } | Not { operand } => infer_dim(operand, elem_dims, lookups),
        If { cond, then, else_ } => {
            if let Mismatch(m) = infer_dim(cond, elem_dims, lookups) {
                return Mismatch(m);
            }
            match both(then, else_) {
                Ok((a, b)) if a == b => Known(a),
                Ok((a, b)) => Mismatch(format!("if branches have incompatible dimensions {a:?} and {b:?}")),
                Err(e) => e,
            }
        }
        Call { func, args } => infer_call(func, args, elem_dims, lookups),
        LookupCall { element_id, input, input2 } => {
            // The lookup's declared y (output) dim, adjusted by TBL_* mode.
            if let Mismatch(m) = infer_dim(input, elem_dims, lookups) {
                return Mismatch(m);
            }
            let (x_dim, y_dim) = lookups.get(element_id).copied().unwrap_or((None, None));
            let mode = match input2.as_deref() {
                Some(Ref { element_id: n, .. }) => n.as_str(),
                _ => "",
            };
            match (y_dim, x_dim) {
                (Some(y), x) => match mode {
                    // ∫y dx → y·x ; d/dx → y/x ; inverse → x ; else y.
                    "TBL_Integral" => x.map(|xd| Known(y.mul(xd))).unwrap_or(Exempt),
                    "TBL_Derivative" => x.map(|xd| Known(y.div(xd))).unwrap_or(Exempt),
                    "TBL_Inverse" | "TBL_Inv_Integral" => x.map(Known).unwrap_or(Exempt),
                    _ => Known(y),
                },
                _ => Exempt,
            }
        }
        // Arrays/comprehensions/submodel-stats/extern: not dimension-checked (exempt).
        _ => Exempt,
    }
}

/// Dimension of a builtin call. Transcendentals require a dimensionless argument; abs/min/max
/// pass the (shared) operand dimension; sqrt halves; math on angles etc. is dimensionless.
fn infer_call(
    func: &crate::model::BuiltinFn,
    args: &[crate::model::AstNode],
    elem_dims: &std::collections::HashMap<String, Option<dim::Dim>>,
    lookups: &std::collections::HashMap<String, (Option<dim::Dim>, Option<dim::Dim>)>,
) -> DimResult {
    use crate::model::BuiltinFn::*;
    use DimResult::*;

    let arg = |i: usize| -> DimResult {
        args.get(i).map(|a| infer_dim(a, elem_dims, lookups)).unwrap_or(Exempt)
    };

    match func {
        // Transcendental / dimensionless-argument functions: argument must be dimensionless.
        Exp | Ln | Log | Log2 | Sin | Cos | Tan | Asin | Acos | Atan | Sinh | Cosh | Tanh
        | Erf | Erfc | Gamma => match arg(0) {
            Known(d) if d.is_dimensionless() => Known(dim::DIMENSIONLESS),
            Known(d) => Mismatch(format!("{func:?} requires a dimensionless argument, got {d:?}")),
            other => other,
        },
        // Abs/Min/Max/Round/Floor/Ceil preserve the operand dimension (min/max require equal dims).
        Abs | Floor | Ceil | Round | Int | Sign => arg(0),
        Min | Max => {
            let (a, b) = (arg(0), arg(1));
            match (a, b) {
                (Mismatch(m), _) | (_, Mismatch(m)) => Mismatch(m),
                (Exempt, x) | (x, Exempt) => x,
                (Known(x), Known(y)) if x == y => Known(x),
                (Known(x), Known(y)) => Mismatch(format!("{func:?} of incompatible dimensions {x:?} and {y:?}")),
            }
        }
        Sqrt => match arg(0) {
            Known(d) => d.sqrt().map(Known).unwrap_or(Exempt),
            other => other,
        },
        // Everything else (atan2, pv/annuity, array ops, table introspection): exempt.
        _ => Exempt,
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

/// Static dimensional analysis (B5): infer each expression element's AST dimension and check it
/// against the element's declared output dimension. Returns a list of dimensional errors (empty =
/// consistent). Unknown units / unresolved refs / unsupported nodes are exempt (never reported),
/// so a partially-emitted model produces no false positives.
///
/// The engine calls this at graph-build time; `RunConfig.units == Strict` turns a non-empty list
/// into a hard load error, `Warn` (default) logs each and continues.
pub fn check_dimensions(model: &Model) -> Vec<String> {
    use std::collections::HashMap;

    // Element id → declared output dimension (None = unrecognized unit → exempt).
    let elem_dims: HashMap<String, Option<dim::Dim>> = model
        .elements
        .iter()
        .map(|e| (e.id().to_string(), element_dim(e)))
        .collect();

    // Lookup id → (x-axis dim, y/output dim) for TBL_* dimension adjustment.
    let lookups: HashMap<String, (Option<dim::Dim>, Option<dim::Dim>)> = model
        .elements
        .iter()
        .filter_map(|e| {
            if let Primitive::Node(n) = &e.primitive {
                if let NodeRule::Lookup(t) = &n.rule {
                    let xd = t.x_unit.as_deref().and_then(dim_of_unit);
                    let yd = t.y_unit.as_deref().and_then(dim_of_unit);
                    return Some((e.id().to_string(), (xd, yd)));
                }
            }
            None
        })
        .collect();

    let mut errors = Vec::new();
    for elem in &model.elements {
        let Primitive::Node(n) = &elem.primitive else { continue };
        let NodeRule::Expression(ef) = &n.rule else { continue };
        match infer_dim(&ef.ast, &elem_dims, &lookups) {
            DimResult::Mismatch(m) => {
                errors.push(format!("element '{}': {m}", elem.id()));
            }
            DimResult::Known(inferred) => {
                // Compare against the declared output dim, when both are known.
                if let Some(Some(declared)) = elem_dims.get(elem.id()) {
                    if inferred != *declared {
                        errors.push(format!(
                            "element '{}': expression dimension {inferred:?} ≠ declared output dimension {declared:?}",
                            elem.id()
                        ));
                    }
                }
            }
            DimResult::Exempt => {}
        }
    }
    errors
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
