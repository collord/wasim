use rand::Rng;
use rand_distr::{Beta, Exp, Gamma, LogNormal, Normal, StudentT, Triangular, Uniform, Weibull};

use crate::error::EngineError;
use crate::model::{
    CumulativePoint, DistributionKind, ProcessMeanType, ProcessSpec, Quantity, Truncation,
};

/// Keep a uniform draw strictly inside (0, 1) for inverse-CDF transforms.
fn open_unit(u: f64) -> f64 {
    u.max(1e-12).min(1.0 - 1e-12)
}

const MAX_REJECTION_ATTEMPTS: usize = 10_000;

pub fn sample<R: Rng>(kind: &DistributionKind, truncation: &Option<Truncation>, rng: &mut R) -> Result<f64, EngineError> {
    let lo = truncation.as_ref().and_then(|t| t.min);
    let hi = truncation.as_ref().and_then(|t| t.max);

    let raw = match kind {
        DistributionKind::Uniform { min, max } => {
            if min.value >= max.value {
                return Err(EngineError::Sampling(format!(
                    "uniform: min ({}) must be < max ({})", min.value, max.value
                )));
            }
            rng.sample(Uniform::new(min.value, max.value))
        }

        DistributionKind::Normal { mean, stddev } => {
            let dist = Normal::new(mean.value(), stddev.value())
                .map_err(|e| EngineError::Sampling(e.to_string()))?;
            rng.sample(dist)
        }

        DistributionKind::Lognormal { mean, stddev } => {
            // mean and stddev are log-space parameters (μ, σ)
            let dist = LogNormal::new(mean.value(), stddev.value())
                .map_err(|e| EngineError::Sampling(e.to_string()))?;
            rng.sample(dist)
        }

        DistributionKind::LognormalMoments { mean, stddev } => {
            // Convert real-space moments to log-space parameters
            let m = mean.value();
            let s = stddev.value();
            let sigma2 = (1.0 + (s / m).powi(2)).ln();
            let sigma = sigma2.sqrt();
            let mu = m.ln() - sigma2 / 2.0;
            let dist = LogNormal::new(mu, sigma)
                .map_err(|e| EngineError::Sampling(e.to_string()))?;
            rng.sample(dist)
        }

        DistributionKind::Triangular { min, mode, max } => {
            let dist = Triangular::new(min.value, max.value, mode.value)
                .map_err(|e| EngineError::Sampling(e.to_string()))?;
            rng.sample(dist)
        }

        DistributionKind::Trapezoidal { min, lower, upper, max } => {
            let u = rng.sample(Uniform::new(0.0_f64, 1.0));
            trapezoid_icdf(min.value, lower.value, upper.value, max.value, u)?
        }

        DistributionKind::Exponential { mean } => {
            let lambda = 1.0 / mean.value();
            let dist = Exp::new(lambda)
                .map_err(|e| EngineError::Sampling(e.to_string()))?;
            rng.sample(dist)
        }

        DistributionKind::Gamma { shape, scale } => {
            let dist = Gamma::new(shape.value, scale.value)
                .map_err(|e| EngineError::Sampling(e.to_string()))?;
            rng.sample(dist)
        }

        DistributionKind::Beta { alpha, beta, min, max } => {
            let dist = Beta::new(alpha.value, beta.value)
                .map_err(|e| EngineError::Sampling(e.to_string()))?;
            let b = rng.sample(dist);
            match (min, max) {
                (Some(lo), Some(hi)) => lo.value + (hi.value - lo.value) * b,
                _ => b,
            }
        }

        DistributionKind::Weibull { shape, scale } => {
            // rand_distr::Weibull::new(scale, shape)
            let dist = Weibull::new(scale.value, shape.value)
                .map_err(|e| EngineError::Sampling(e.to_string()))?;
            rng.sample(dist)
        }

        DistributionKind::PearsonV { shape, scale } => {
            // PearsonV = InverseGamma(shape, scale): sample Gamma then invert
            let dist = Gamma::new(shape.value, 1.0)
                .map_err(|e| EngineError::Sampling(e.to_string()))?;
            scale.value / rng.sample(dist)
        }

        DistributionKind::PearsonIii { mean, stddev, skewness } => {
            // Three-parameter gamma: X = location + Gamma(kappa, beta)
            let gamma_coeff = skewness.value;
            if gamma_coeff.abs() < 1e-12 {
                // Degenerate to normal
                let dist = Normal::new(mean.value, stddev.value)
                    .map_err(|e| EngineError::Sampling(e.to_string()))?;
                rng.sample(dist)
            } else {
                let kappa = (2.0 / gamma_coeff).powi(2);
                let beta = stddev.value * gamma_coeff / 2.0;
                let location = mean.value - kappa * beta;
                let dist = Gamma::new(kappa, beta.abs())
                    .map_err(|e| EngineError::Sampling(e.to_string()))?;
                let g = rng.sample(dist);
                if beta < 0.0 {
                    location - g
                } else {
                    location + g
                }
            }
        }

        DistributionKind::DiscreteUniform { min, max } => {
            if min > max {
                return Err(EngineError::Sampling(format!(
                    "discrete_uniform: min ({min}) must be ≤ max ({max})"
                )));
            }
            rng.sample(Uniform::new_inclusive(*min, *max)) as f64
        }

        DistributionKind::Bernoulli { prob } => {
            let u: f64 = rng.sample(Uniform::new(0.0, 1.0));
            if u < prob.value { 1.0 } else { 0.0 }
        }

        DistributionKind::Discrete { outcomes, probabilities } => {
            if outcomes.is_empty() || outcomes.len() != probabilities.len() {
                return Err(EngineError::Sampling(
                    "discrete: outcomes and probabilities must be non-empty and equal length".into()
                ));
            }
            let total: f64 = probabilities.iter().sum();
            if total <= 0.0 {
                return Err(EngineError::Sampling("discrete: probabilities sum to zero".into()));
            }
            let u: f64 = rng.sample(Uniform::new(0.0, total));
            let mut cumulative = 0.0;
            for (outcome, prob) in outcomes.iter().zip(probabilities.iter()) {
                cumulative += prob;
                if u < cumulative {
                    return Ok(*outcome);
                }
            }
            *outcomes.last().unwrap()
        }

        DistributionKind::Pert { min, mode, max } => {
            let (a, m, b) = (min.value, mode.value, max.value);
            if a >= b {
                return Err(EngineError::Sampling(format!("pert: min ({a}) must be < max ({b})")));
            }
            // Beta-PERT shape parameters (λ = 4).
            let alpha = 1.0 + 4.0 * (m - a) / (b - a);
            let beta = 1.0 + 4.0 * (b - m) / (b - a);
            let dist = Beta::new(alpha, beta).map_err(|e| EngineError::Sampling(e.to_string()))?;
            a + (b - a) * rng.sample(dist)
        }

        DistributionKind::Pareto { scale, shape, location } => {
            let u = open_unit(rng.sample(Uniform::new(0.0_f64, 1.0)));
            let loc = location.as_ref().map(|q| q.value).unwrap_or(0.0);
            loc + scale.value / (1.0 - u).powf(1.0 / shape.value)
        }

        DistributionKind::ExtremeValue { location, scale } => {
            let u = open_unit(rng.sample(Uniform::new(0.0_f64, 1.0)));
            location.value - scale.value * (-u.ln()).ln() // Gumbel (max) inverse CDF
        }

        DistributionKind::StudentT { degrees_of_freedom, location, scale } => {
            let dist = StudentT::new(degrees_of_freedom.value)
                .map_err(|e| EngineError::Sampling(e.to_string()))?;
            let t: f64 = rng.sample(dist);
            let loc = location.as_ref().map(|q| q.value).unwrap_or(0.0);
            let sc = scale.as_ref().map(|q| q.value).unwrap_or(1.0);
            loc + sc * t
        }

        DistributionKind::Cumulative { points } => {
            if points.is_empty() {
                return Err(EngineError::Sampling("cumulative: no points".into()));
            }
            cumulative_inverse(points, rng.sample(Uniform::new(0.0_f64, 1.0)))
        }

        DistributionKind::Sampled { samples, weights } => {
            if samples.is_empty() {
                return Err(EngineError::Sampling("sampled: no samples".into()));
            }
            sampled_inverse(samples, weights, rng.sample(Uniform::new(0.0_f64, 1.0)))
        }

        DistributionKind::External { .. } => {
            eprintln!("warn: 'external' distribution cannot be sampled by the engine; using 0.0");
            0.0
        }
    };

    // Apply truncation via rejection sampling
    if lo.is_none() && hi.is_none() {
        return Ok(raw);
    }
    if in_bounds(raw, lo, hi) {
        return Ok(raw);
    }
    for _ in 0..MAX_REJECTION_ATTEMPTS {
        let v = match kind {
            DistributionKind::Uniform { min, max } => {
                rng.sample(Uniform::new(min.value, max.value))
            }
            _ => {
                // Re-sample by recursing without truncation to avoid re-building dist
                // This is fine for moderate truncation; tight truncation may be slow.
                sample(kind, &None, rng)?
            }
        };
        if in_bounds(v, lo, hi) {
            return Ok(v);
        }
    }
    Err(EngineError::Sampling(format!(
        "truncation rejection limit reached (lo={lo:?}, hi={hi:?})"
    )))
}

fn in_bounds(v: f64, lo: Option<f64>, hi: Option<f64>) -> bool {
    lo.map(|l| v >= l).unwrap_or(true) && hi.map(|h| v <= h).unwrap_or(true)
}

// ── AR(1) per-timestep sampler ────────────────────────────────────────────────

/// One AR(1) step in standard-normal driver space:
///   z_new = ρ × z_prev + √(1 − ρ²) × ε,   ε ~ N(0, 1)
/// then transform z_new through the inverse CDF of the target distribution.
///
/// Returns (sample_value, z_new). The caller persists z_new as the next z_prev.
///
/// Distributions without a closed-form inverse CDF (Gamma, Beta, Weibull,
/// PearsonV, PearsonIII) fall back to iid sampling with z_new = z_prev.
pub fn sample_autocorr_step<R: Rng>(
    kind: &DistributionKind,
    truncation: &Option<Truncation>,
    rho: f64,
    z_prev: f64,
    rng: &mut R,
) -> Result<(f64, f64), EngineError> {
    let eps: f64 = rng.sample(Normal::new(0.0_f64, 1.0_f64)
        .map_err(|e| EngineError::Sampling(e.to_string()))?);
    let z = if rho > 0.0 {
        rho * z_prev + (1.0 - rho * rho).sqrt() * eps
    } else {
        eps
    };

    match icdf(kind, standard_normal_cdf(z)) {
        Some(raw) => {
            // Truncation via clamp — rejection would break the Markov chain.
            let lo = truncation.as_ref().and_then(|t| t.min);
            let hi = truncation.as_ref().and_then(|t| t.max);
            let clamped = raw
                .max(lo.unwrap_or(f64::NEG_INFINITY))
                .min(hi.unwrap_or(f64::INFINITY));
            Ok((clamped, z))
        }
        None => {
            let v = sample(kind, truncation, rng)?;
            Ok((v, z_prev))
        }
    }
}

/// Standard normal CDF Φ(z), Abramowitz & Stegun 26.2.17 (≤ 7.5×10⁻⁸ error).
pub(crate) fn standard_normal_cdf(z: f64) -> f64 {
    let sign = if z < 0.0 { -1.0 } else { 1.0 };
    let x = z.abs();
    let t = 1.0 / (1.0 + 0.2316419 * x);
    let phi = (-0.5 * x * x).exp() / (2.0 * std::f64::consts::PI).sqrt();
    let poly = t * (0.319381530
        + t * (-0.356563782
        + t * (1.781477937
        + t * (-1.821255978
        + t * 1.330274429))));
    let cdf_pos = 1.0 - phi * poly;
    if sign > 0.0 { cdf_pos } else { 1.0 - cdf_pos }
}

// ── Inverse CDF (quantile function) ──────────────────────────────────────────

/// Inverse standard normal CDF Φ⁻¹(p). Acklam's rational approximation,
/// max absolute error < 1.15×10⁻⁹ for p ∈ (0, 1).
pub(crate) fn standard_normal_quantile(p: f64) -> f64 {
    if p <= 0.0 { return f64::NEG_INFINITY; }
    if p >= 1.0 { return f64::INFINITY; }

    const A: [f64; 6] = [
        -3.969683028665376e+01,  2.209460984245205e+02,
        -2.759285104469687e+02,  1.383577518672690e+02,
        -3.066479806614716e+01,  2.506628277459239e+00,
    ];
    const B: [f64; 5] = [
        -5.447609879822406e+01,  1.615858368580409e+02,
        -1.556989798598866e+02,  6.680131188771972e+01,
        -1.328068155288572e+01,
    ];
    const C: [f64; 6] = [
        -7.784894002430293e-03, -3.223964580411365e-01,
        -2.400758277161838e+00, -2.549732539343734e+00,
         4.374664141464968e+00,  2.938163982698783e+00,
    ];
    const D: [f64; 4] = [
         7.784695709041462e-03,  3.224671290700398e-01,
         2.445134137142996e+00,  3.754408661907416e+00,
    ];

    const P_LOW: f64  = 0.02425;
    const P_HIGH: f64 = 1.0 - P_LOW;

    if p < P_LOW {
        let q = (-2.0 * p.ln()).sqrt();
        (((((C[0]*q+C[1])*q+C[2])*q+C[3])*q+C[4])*q+C[5]) /
        ((((D[0]*q+D[1])*q+D[2])*q+D[3])*q+1.0)
    } else if p <= P_HIGH {
        let q = p - 0.5;
        let r = q * q;
        (((((A[0]*r+A[1])*r+A[2])*r+A[3])*r+A[4])*r+A[5])*q /
        (((((B[0]*r+B[1])*r+B[2])*r+B[3])*r+B[4])*r+1.0)
    } else {
        let q = (-2.0 * (1.0 - p).ln()).sqrt();
        -(((((C[0]*q+C[1])*q+C[2])*q+C[3])*q+C[4])*q+C[5]) /
         ((((D[0]*q+D[1])*q+D[2])*q+D[3])*q+1.0)
    }
}

/// Apply the inverse CDF of `kind` to a uniform quantile u ∈ [0, 1].
/// Returns `None` for distributions without a closed-form inverse CDF
/// (Gamma, Beta, Weibull, PearsonV, PearsonIII); the caller should fall back to iid.
/// No truncation is applied — clamp the result yourself if needed.
pub fn icdf(kind: &DistributionKind, u: f64) -> Option<f64> {
    let raw = match kind {
        DistributionKind::Normal { mean, stddev } => {
            mean.value() + stddev.value() * standard_normal_quantile(u)
        }
        DistributionKind::Lognormal { mean, stddev } => {
            (mean.value() + stddev.value() * standard_normal_quantile(u)).exp()
        }
        DistributionKind::LognormalMoments { mean, stddev } => {
            let m = mean.value();
            let s = stddev.value();
            if m <= 0.0 { return None; }
            let sigma2 = (1.0 + (s / m).powi(2)).ln();
            let mu = m.ln() - sigma2 / 2.0;
            (mu + sigma2.sqrt() * standard_normal_quantile(u)).exp()
        }
        DistributionKind::Uniform { min, max } => {
            min.value + (max.value - min.value) * u
        }
        DistributionKind::Triangular { min, mode, max } => {
            let a = min.value;
            let b = max.value;
            let c = mode.value;
            let f = (c - a) / (b - a);
            if u < f {
                a + ((b - a) * (c - a) * u).sqrt()
            } else {
                b - ((b - a) * (b - c) * (1.0 - u)).sqrt()
            }
        }
        DistributionKind::Trapezoidal { min, lower, upper, max } => {
            match trapezoid_icdf(min.value, lower.value, upper.value, max.value, u) {
                Ok(v) => v,
                Err(_) => return None,
            }
        }
        DistributionKind::Exponential { mean } => {
            -mean.value() * (1.0 - u).ln()
        }
        DistributionKind::Bernoulli { prob } => {
            if u < prob.value { 1.0 } else { 0.0 }
        }
        DistributionKind::DiscreteUniform { min, max } => {
            let n = (*max - *min + 1) as f64;
            (*min as f64 + (n * u).floor()).min(*max as f64)
        }
        DistributionKind::Discrete { outcomes, probabilities } => {
            if outcomes.is_empty() || outcomes.len() != probabilities.len() { return None; }
            let total: f64 = probabilities.iter().sum();
            if total <= 0.0 { return None; }
            let target = u * total;
            let mut cum = 0.0;
            let mut chosen = *outcomes.last().unwrap();
            for (o, p) in outcomes.iter().zip(probabilities.iter()) {
                cum += p;
                if target <= cum { chosen = *o; break; }
            }
            chosen
        }
        DistributionKind::Pareto { scale, shape, location } => {
            let u = open_unit(u);
            let loc = location.as_ref().map(|q| q.value).unwrap_or(0.0);
            loc + scale.value / (1.0 - u).powf(1.0 / shape.value)
        }
        DistributionKind::ExtremeValue { location, scale } => {
            let u = open_unit(u);
            location.value - scale.value * (-u.ln()).ln()
        }
        DistributionKind::Cumulative { points } => {
            if points.is_empty() { return None; }
            cumulative_inverse(points, u)
        }
        DistributionKind::Sampled { samples, weights } => {
            if samples.is_empty() { return None; }
            sampled_inverse(samples, weights, u)
        }
        // No closed-form inverse CDF; caller falls back to iid.
        DistributionKind::Gamma { .. }
        | DistributionKind::Beta { .. }
        | DistributionKind::Weibull { .. }
        | DistributionKind::PearsonV { .. }
        | DistributionKind::PearsonIii { .. }
        | DistributionKind::Pert { .. }
        | DistributionKind::StudentT { .. }
        | DistributionKind::External { .. } => return None,
    };
    Some(raw)
}

/// Inverse CDF of a trapezoidal distribution with breakpoints min ≤ lower ≤ upper ≤ max.
///
/// Ports SELDM's `fndUniform01ToTrapezoid` (Kacker & Lawrence, 2007). The density is a
/// trapezoid of height `h = 2 / ((max-min) + (upper-lower))`: a lower ramp on [min, lower],
/// a plateau on [lower, upper], and an upper ramp on [upper, max].
fn trapezoid_icdf(min: f64, lower: f64, upper: f64, max: f64, u: f64) -> Result<f64, EngineError> {
    if !(min <= lower && lower <= upper && upper <= max) || min >= max {
        return Err(EngineError::Sampling(format!(
            "trapezoidal: require min ({min}) ≤ lower ({lower}) ≤ upper ({upper}) ≤ max ({max}) and min < max"
        )));
    }
    // Uniform special case (both ramps degenerate).
    if min == lower && upper == max {
        return Ok(min + (max - min) * u);
    }
    let h = 2.0 / ((max - min) + (upper - lower));
    let lower_area = (h / 2.0) * (lower - min); // cumulative prob at end of lower ramp
    let upper_start = 1.0 - (h / 2.0) * (max - upper); // cumulative prob at start of upper ramp
    let v = if u <= lower_area {
        min + (2.0 * (lower - min) / h).sqrt() * u.sqrt()
    } else if u <= upper_start {
        (min + lower) / 2.0 + u / h
    } else {
        max - (2.0 * (max - upper) / h).sqrt() * (1.0 - u).sqrt()
    };
    Ok(v)
}

/// Inverse CDF of a piecewise-linear cumulative distribution (`cumulative` family).
fn cumulative_inverse(points: &[CumulativePoint], u: f64) -> f64 {
    let mut pts: Vec<&CumulativePoint> = points.iter().collect();
    pts.sort_by(|a, b| a.cumulative_probability.total_cmp(&b.cumulative_probability));
    if u <= pts[0].cumulative_probability {
        return pts[0].x;
    }
    let last = *pts.last().unwrap();
    if u >= last.cumulative_probability {
        return last.x;
    }
    for w in pts.windows(2) {
        let (lo, hi) = (w[0], w[1]);
        if u >= lo.cumulative_probability && u <= hi.cumulative_probability {
            let dp = hi.cumulative_probability - lo.cumulative_probability;
            let t = if dp.abs() < 1e-12 { 0.0 } else { (u - lo.cumulative_probability) / dp };
            return lo.x + t * (hi.x - lo.x);
        }
    }
    last.x
}

/// Inverse CDF of a (weighted) empirical distribution (`sampled` family).
fn sampled_inverse(samples: &[f64], weights: &Option<Vec<f64>>, u: f64) -> f64 {
    match weights {
        Some(w) if w.len() == samples.len() => {
            let total: f64 = w.iter().sum();
            if total <= 0.0 {
                return samples[samples.len() / 2];
            }
            let target = u * total;
            let mut cum = 0.0;
            for (s, wi) in samples.iter().zip(w) {
                cum += wi;
                if target <= cum {
                    return *s;
                }
            }
            *samples.last().unwrap()
        }
        _ => {
            let i = ((u * samples.len() as f64).floor() as usize).min(samples.len() - 1);
            samples[i]
        }
    }
}

// ── GBM per-timestep sampler ──────────────────────────────────────────────────

/// Draw one GBM step and return a *rate* (per model time unit) suitable for use
/// in an accumulator rate expression as `balance × stochastic_process`.
///
/// The returned value r satisfies: balance × r × dt = balance × (exp(log_ret) − 1),
/// so Euler integration preserves exact GBM semantics.
pub fn sample_gbm<R: Rng>(
    process: &ProcessSpec,
    lower_bound: Option<&Quantity>,
    dt: f64,
    model_time_unit: &str,
    rng: &mut R,
) -> Result<f64, EngineError> {
    let t_ref = time_unit_to_seconds(&parse_rate_denominator(&process.stddev.unit))
        / time_unit_to_seconds(model_time_unit);

    let dt_ratio = if t_ref > 0.0 { dt / t_ref } else { dt };

    let sigma = process.stddev.value;
    let mean  = process.mean.value;

    let log_drift = match process.mean_type {
        ProcessMeanType::Geometric  => (1.0 + mean).ln(),
        ProcessMeanType::Arithmetic => mean - 0.5 * sigma * sigma,
        ProcessMeanType::LogDrift   => mean,
    };

    let mu_step    = log_drift * dt_ratio;
    let sigma_step = sigma * dt_ratio.sqrt();

    let z: f64 = rng.sample(Normal::new(0.0_f64, 1.0_f64)
        .map_err(|e| EngineError::Sampling(e.to_string()))?);

    let log_ret = mu_step + sigma_step * z;
    // Convert per-step return to a rate per model time unit.
    let rate = if dt > 0.0 { (log_ret.exp() - 1.0) / dt } else { 0.0 };

    // lower_bound is expressed in T_ref units; convert to model-time-unit rate.
    let lb_rate = lower_bound.map(|lb| if t_ref > 0.0 { lb.value / t_ref } else { lb.value });
    Ok(match lb_rate {
        Some(lb) => rate.max(lb),
        None => rate,
    })
}

fn parse_rate_denominator(unit: &str) -> String {
    // "1/yr" → "yr", "1/d" → "d", bare "yr" → "yr"
    unit.find('/').map_or_else(|| unit.to_string(), |pos| unit[pos + 1..].to_string())
}

fn time_unit_to_seconds(unit: &str) -> f64 {
    match unit.trim() {
        "yr" | "year" | "years" => 365.25 * 86400.0,
        "mo" | "month" | "months" => 365.25 * 86400.0 / 12.0,
        "wk" | "week" | "weeks" => 7.0 * 86400.0,
        "d" | "day" | "days" => 86400.0,
        "h" | "hr" | "hour" | "hours" => 3600.0,
        "min" | "minute" | "minutes" => 60.0,
        "s" | "sec" | "second" | "seconds" => 1.0,
        _ => 1.0,
    }
}

#[cfg(test)]
mod trapezoid_tests {
    use super::trapezoid_icdf;

    /// Verbatim transcription of SELDM's `fndUniform01ToTrapezoid`
    /// (modStatistics.bas, G.E. Granato) for parity checking. Parameters map:
    /// dMin=min, dLower=lower, dUpper=upper, dMax=max.
    fn seldm_trapezoid(u01: f64, min: f64, lower: f64, upper: f64, max: f64) -> f64 {
        // Error cases return the input U01 in SELDM; we don't exercise those here.
        if (min > lower) || (min > upper) || (min >= max) {
            return u01;
        } else if (lower > upper) || (lower > max) {
            return u01;
        } else if upper > max {
            return u01;
        }
        if min == lower && upper == max {
            // NOTE: SELDM's rectangle branch reads `dMin * (dMax - dMin) * dU01`,
            // which is dimensionally wrong (a transcription bug in the original).
            // Our engine implements the correct uniform inverse-CDF instead, so we
            // deliberately do NOT compare against this branch.
            return min * (max - min) * u01;
        }
        let h = 2.0 / ((max - min) + (upper - lower));
        if u01 >= 0.0 && u01 <= (h / 2.0) * (lower - min) {
            min + (2.0 * ((lower - min) / h)).sqrt() * u01.sqrt()
        } else if u01 > (h / 2.0) * (lower - min) && u01 <= 1.0 - (h / 2.0) * (max - upper) {
            (min + lower) / 2.0 + u01 / h
        } else {
            max - (2.0 * (max - upper) / h).sqrt() * (1.0 - u01).sqrt()
        }
    }

    #[test]
    fn matches_seldm_trapezoid_across_quantiles() {
        // Non-degenerate trapezoid; compare the two ramps + plateau, excluding the
        // rectangle branch (which is a known bug in the SELDM original).
        let (min, lower, upper, max) = (1.0, 3.0, 7.0, 12.0);
        for i in 1..1000 {
            let u = i as f64 / 1000.0;
            let ours = trapezoid_icdf(min, lower, upper, max, u).unwrap();
            let seldm = seldm_trapezoid(u, min, lower, upper, max);
            assert!(
                (ours - seldm).abs() < 1e-12,
                "u={u}: ours={ours} seldm={seldm}"
            );
        }
    }

    #[test]
    fn triangular_degenerate_matches_seldm() {
        // lower == upper → triangular. SELDM handles this via the same trapezoid math.
        let (min, lower, upper, max) = (0.0, 5.0, 5.0, 10.0);
        for i in 1..1000 {
            let u = i as f64 / 1000.0;
            let ours = trapezoid_icdf(min, lower, upper, max, u).unwrap();
            let seldm = seldm_trapezoid(u, min, lower, upper, max);
            assert!((ours - seldm).abs() < 1e-12, "u={u}: ours={ours} seldm={seldm}");
        }
    }

    #[test]
    fn monotonic_and_in_support() {
        let (min, lower, upper, max) = (2.0, 4.0, 9.0, 15.0);
        let mut prev = f64::NEG_INFINITY;
        for i in 0..=1000 {
            let u = i as f64 / 1000.0;
            let x = trapezoid_icdf(min, lower, upper, max, u).unwrap();
            assert!(x >= min - 1e-9 && x <= max + 1e-9, "u={u} x={x} out of support");
            assert!(x >= prev - 1e-9, "non-monotonic at u={u}: {prev} -> {x}");
            prev = x;
        }
    }

    #[test]
    fn rejects_bad_ordering() {
        assert!(trapezoid_icdf(5.0, 3.0, 7.0, 10.0, 0.5).is_err()); // min > lower
        assert!(trapezoid_icdf(0.0, 0.0, 0.0, 0.0, 0.5).is_err()); // min == max
    }
}
