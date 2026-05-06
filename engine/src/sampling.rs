use rand::Rng;
use rand_distr::{Beta, Exp, Gamma, LogNormal, Normal, Triangular, Uniform, Weibull};

use crate::error::EngineError;
use crate::model::{DistributionKind, Truncation};

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
            let dist = Normal::new(mean.value, stddev.value)
                .map_err(|e| EngineError::Sampling(e.to_string()))?;
            rng.sample(dist)
        }

        DistributionKind::Lognormal { mean, stddev } => {
            // mean and stddev are log-space parameters (μ, σ)
            let dist = LogNormal::new(mean.value, stddev.value)
                .map_err(|e| EngineError::Sampling(e.to_string()))?;
            rng.sample(dist)
        }

        DistributionKind::LognormalMoments { mean, stddev } => {
            // Convert real-space moments to log-space parameters
            let m = mean.value;
            let s = stddev.value;
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

        DistributionKind::Exponential { mean } => {
            let lambda = 1.0 / mean.value;
            let dist = Exp::new(lambda)
                .map_err(|e| EngineError::Sampling(e.to_string()))?;
            rng.sample(dist)
        }

        DistributionKind::Gamma { shape, scale } => {
            let dist = Gamma::new(shape.value, scale.value)
                .map_err(|e| EngineError::Sampling(e.to_string()))?;
            rng.sample(dist)
        }

        DistributionKind::Beta { alpha, beta } => {
            let dist = Beta::new(alpha.value, beta.value)
                .map_err(|e| EngineError::Sampling(e.to_string()))?;
            rng.sample(dist)
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
