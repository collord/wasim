//! Configurable results/analysis layer (A3, gap #3). Runtime-configured (like `sensitivity_v2`),
//! NOT schema: a `ResultsSpec` on `RunConfig` opts an element into richer statistics beyond the
//! fixed `mean + p05/p25/p50/p75/p95` summary. All outputs are additive `Option` fields on
//! `ElementResults`, so default output (spec = None) is byte-identical and existing consumers
//! are untouched until they opt in.
//!
//! The four families the spec unlocks (per element, from the same run's stored samples):
//!   1. custom percentile bands over the time history,
//!   2. final-value distribution objects — PDF (binned), CDF, CCDF (exceedance),
//!   3. capture-time distribution snapshots at requested elapsed times,
//!   4. final-value summary stats — t-interval on the mean, skewness, excess kurtosis,
//!      and conditional tail expectation (mean beyond a percentile).

use serde::{Deserialize, Serialize};

use crate::engine::{mean, percentile, std};

/// Per-run analysis configuration. Empty (all `None`/default) reproduces the legacy summary.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ResultsSpec {
    /// Element ids to analyze. Empty = analyze every saved element.
    pub elements: Vec<String>,
    /// Custom percentiles (0–100) for the time-history bands. Empty = keep the default 5-band set.
    pub percentiles: Vec<f64>,
    /// Emit final-value distribution objects (PDF/CDF/CCDF). `bins` controls the PDF resolution.
    pub distribution: bool,
    pub bins: usize,
    /// Elapsed times (in the timestep unit) at which to snapshot the distribution of values
    /// across realizations. Each maps to the nearest stored step.
    pub capture_times: Vec<f64>,
    /// Emit final-value summary stats (confidence interval, skew/kurtosis, CTE).
    pub final_stats: bool,
    /// Confidence level for the mean's t-interval (e.g. 0.95). Default 0.95 if final_stats set.
    pub confidence: f64,
    /// Percentile (0–100) beyond which the conditional tail expectation is computed (upper tail).
    /// Default 95.0 if final_stats set.
    pub cte_percentile: f64,
}

impl ResultsSpec {
    /// True when this element should be analyzed (explicit list, or analyze-all when empty).
    fn covers(&self, id: &str) -> bool {
        self.elements.is_empty() || self.elements.iter().any(|e| e == id)
    }

    /// True when the spec requests any analysis at all.
    pub fn is_active(&self) -> bool {
        !self.percentiles.is_empty() || self.distribution || !self.capture_times.is_empty() || self.final_stats
    }
}

/// Additive analysis block attached to an `ElementResults` when a `ResultsSpec` opts it in.
#[derive(Debug, Clone, Serialize)]
pub struct ElementAnalysis {
    /// Custom percentile bands over the time history: one series per requested percentile.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub percentile_bands: Vec<PercentileBand>,
    /// Final-value distribution as PDF / CDF / CCDF.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distribution: Option<Distribution>,
    /// Snapshots of the value distribution at requested capture times.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub captures: Vec<CaptureSnapshot>,
    /// Final-value summary statistics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_stats: Option<FinalStats>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PercentileBand {
    pub percentile: f64,
    /// One value per timestep.
    pub values: Vec<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Distribution {
    /// Bin centers for the PDF.
    pub bin_centers: Vec<f64>,
    /// Probability density per bin (integrates to ~1 over the support).
    pub pdf: Vec<f64>,
    /// Sorted sample values (x) for the empirical CDF/CCDF.
    pub x: Vec<f64>,
    /// Cumulative probability P(X ≤ x) at each `x`.
    pub cdf: Vec<f64>,
    /// Exceedance probability P(X > x) = 1 − CDF at each `x`.
    pub ccdf: Vec<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaptureSnapshot {
    /// Requested elapsed time.
    pub time: f64,
    /// Step index actually used (nearest stored step).
    pub step: usize,
    pub mean: f64,
    pub p05: f64,
    pub p50: f64,
    pub p95: f64,
    /// The full per-realization values at that step (for downstream custom analysis).
    pub values: Vec<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FinalStats {
    pub mean: f64,
    pub std: f64,
    /// Half-width of the two-sided confidence interval on the mean (t-interval).
    pub ci_half_width: f64,
    pub ci_lower: f64,
    pub ci_upper: f64,
    pub confidence: f64,
    /// Sample skewness (Fisher-Pearson).
    pub skewness: f64,
    /// Excess kurtosis (normal → 0).
    pub excess_kurtosis: f64,
    /// Conditional tail expectation: mean of the samples beyond `cte_percentile` (upper tail).
    pub cte: f64,
    pub cte_percentile: f64,
}

/// Compute the analysis block for one element from its stored samples. `final_values` are the
/// per-realization finals; `hist` is `[step][realization]` (empty if history wasn't saved).
/// Returns `None` when the spec doesn't cover this element or requests nothing.
pub fn compute_analysis(
    spec: &ResultsSpec,
    id: &str,
    final_values: &[f64],
    hist: &[Vec<f64>],
    dt: f64,
) -> Option<ElementAnalysis> {
    if !spec.covers(id) || !spec.is_active() {
        return None;
    }

    // Custom percentile bands over the time history.
    let percentile_bands = if spec.percentiles.is_empty() || hist.is_empty() {
        Vec::new()
    } else {
        spec.percentiles
            .iter()
            .map(|&p| PercentileBand {
                percentile: p,
                values: hist.iter().map(|step| percentile(step, p)).collect(),
            })
            .collect()
    };

    // Final-value distribution objects.
    let distribution = if spec.distribution && !final_values.is_empty() {
        Some(build_distribution(final_values, spec.bins.max(1)))
    } else {
        None
    };

    // Capture-time snapshots.
    let captures = spec
        .capture_times
        .iter()
        .filter_map(|&t| {
            if hist.is_empty() {
                return None;
            }
            // Nearest stored step to the requested elapsed time.
            let step = if dt > 0.0 { (t / dt).round() as usize } else { 0 };
            let step = step.min(hist.len() - 1);
            let vals = &hist[step];
            Some(CaptureSnapshot {
                time: t,
                step,
                mean: mean(vals),
                p05: percentile(vals, 5.0),
                p50: percentile(vals, 50.0),
                p95: percentile(vals, 95.0),
                values: vals.clone(),
            })
        })
        .collect();

    // Final-value summary stats.
    let final_stats = if spec.final_stats && !final_values.is_empty() {
        Some(build_final_stats(
            final_values,
            if spec.confidence > 0.0 { spec.confidence } else { 0.95 },
            if spec.cte_percentile > 0.0 { spec.cte_percentile } else { 95.0 },
        ))
    } else {
        None
    };

    Some(ElementAnalysis { percentile_bands, distribution, captures, final_stats })
}

/// Build PDF (binned density), CDF, and CCDF (exceedance) from sample values.
fn build_distribution(values: &[f64], bins: usize) -> Distribution {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let n = sorted.len() as f64;
    let (lo, hi) = (sorted[0], sorted[sorted.len() - 1]);

    // PDF: equal-width bins over [lo, hi]; density = count / (n · width).
    let width = if hi > lo { (hi - lo) / bins as f64 } else { 1.0 };
    let mut counts = vec![0.0f64; bins];
    for &v in &sorted {
        let mut b = if hi > lo { ((v - lo) / width).floor() as usize } else { 0 };
        if b >= bins {
            b = bins - 1;
        }
        counts[b] += 1.0;
    }
    let bin_centers: Vec<f64> = (0..bins).map(|i| lo + (i as f64 + 0.5) * width).collect();
    let pdf: Vec<f64> = counts.iter().map(|c| c / (n * width)).collect();

    // Empirical CDF/CCDF at each sorted sample (i/n convention).
    let x = sorted.clone();
    let cdf: Vec<f64> = (0..sorted.len()).map(|i| (i + 1) as f64 / n).collect();
    let ccdf: Vec<f64> = cdf.iter().map(|c| 1.0 - c).collect();

    Distribution { bin_centers, pdf, x, cdf, ccdf }
}

/// Confidence interval on the mean (t-interval), skewness, excess kurtosis, and the upper-tail
/// conditional tail expectation.
fn build_final_stats(values: &[f64], confidence: f64, cte_pct: f64) -> FinalStats {
    let n = values.len();
    let m = mean(values);
    let s = std(values);

    // t-interval half-width = t_{α/2, n−1} · s/√n. Use a normal-quantile approximation for the
    // t critical value (adequate at the realization counts Monte Carlo uses; exact for large n).
    let z = crate::sampling::standard_normal_quantile(0.5 + confidence / 2.0);
    let ci_half_width = if n >= 2 { z * s / (n as f64).sqrt() } else { 0.0 };

    // Skewness / excess kurtosis (population moments about the sample mean).
    let (skewness, excess_kurtosis) = if n >= 2 && s > 0.0 {
        let m2: f64 = values.iter().map(|x| (x - m).powi(2)).sum::<f64>() / n as f64;
        let m3: f64 = values.iter().map(|x| (x - m).powi(3)).sum::<f64>() / n as f64;
        let m4: f64 = values.iter().map(|x| (x - m).powi(4)).sum::<f64>() / n as f64;
        let sd = m2.sqrt();
        (m3 / sd.powi(3), m4 / m2.powi(2) - 3.0)
    } else {
        (0.0, 0.0)
    };

    // Conditional tail expectation: mean of samples strictly beyond the cte percentile.
    let threshold = percentile(values, cte_pct);
    let tail: Vec<f64> = values.iter().cloned().filter(|&x| x >= threshold).collect();
    let cte = if tail.is_empty() { threshold } else { mean(&tail) };

    FinalStats {
        mean: m,
        std: s,
        ci_half_width,
        ci_lower: m - ci_half_width,
        ci_upper: m + ci_half_width,
        confidence,
        skewness,
        excess_kurtosis,
        cte,
        cte_percentile: cte_pct,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cte_hand_computation() {
        // Samples 1..=10. The 80th percentile (nearest-rank on 0-based idx round((0.8)·9)=7) is
        // 8.0; the CTE is the mean of samples ≥ 8 = mean(8,9,10) = 9.0.
        let vals: Vec<f64> = (1..=10).map(|x| x as f64).collect();
        let fs = build_final_stats(&vals, 0.95, 80.0);
        assert!((fs.cte - 9.0).abs() < 1e-9, "CTE(80th) of 1..=10 should be 9.0, got {}", fs.cte);
    }

    #[test]
    fn distribution_pdf_integrates_to_one() {
        let vals: Vec<f64> = (0..1000).map(|i| i as f64 / 100.0).collect();
        let d = build_distribution(&vals, 10);
        let width = d.bin_centers[1] - d.bin_centers[0];
        let area: f64 = d.pdf.iter().map(|p| p * width).sum();
        assert!((area - 1.0).abs() < 1e-9, "PDF area {area} should be 1");
        // CDF ends at 1, CCDF starts near 1.
        assert!((d.cdf.last().unwrap() - 1.0).abs() < 1e-12);
    }
}
