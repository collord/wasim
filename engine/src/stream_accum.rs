//! Incremental streaming accumulators (Phase 0 of sweep composition, standalone core).
//!
//! The batch engine retains every realization's value at every step (`hist_store:
//! Vec<Vec<f64>>`) and reduces post-hoc via `engine_v2::stats()` — memory
//! O(realizations × elements × steps). This module is the incremental alternative: fold one
//! realization at a time into per-step accumulators, merge chunks *associatively*, and reduce
//! to the same `TimeHistoryStats` at the end. It is the substrate a resumable / streaming run
//! (`run_chunk`, poll boundary) needs; wiring it into the `engine_v2` realization loop is a
//! separate step.
//!
//! **Two things the chunk-merge spike established, encoded here:**
//!   1. The merge is associative and order-free, so *any* chunking of a run yields the same
//!      accumulator state (chunk-invariance). The CI property is **chunk-vs-chunk agreement**.
//!   2. Mean/sd stream for free (Welford/Chan, O(1) per step). But an incremental sum visits
//!      values in a different order than a batch `sum()`, so the streamed mean differs from the
//!      batch mean by a few ULP (FP non-associativity) — it is *not* bit-identical to batch,
//!      and any assertion must be chunk-vs-chunk, never chunk-vs-batch.
//!
//! Percentile bands force a memory-vs-parity choice (spike verdict): exact bands require
//! retaining samples (no memory win but batch-identical); an O(1) sketch is approximate. This
//! module ships **`BandAccum::Exact` only** — the correct anchor. The sketch is a later add.

use crate::engine::{mean, percentile, TimeHistoryStats};

/// Running mean + M2 (Welford) for one series (one element at one step). Mergeable via Chan's
/// parallel-variance combine, so partial accumulators from different realization chunks combine
/// associatively.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RunningMoments {
    pub n: u64,
    pub mean: f64,
    /// Σ(xᵢ − mean)² — the sum of squared deviations (Welford's M2).
    pub m2: f64,
}

impl RunningMoments {
    /// Fold one sample in (Welford update).
    pub fn push(&mut self, x: f64) {
        self.n += 1;
        let delta = x - self.mean;
        self.mean += delta / self.n as f64;
        self.m2 += delta * (x - self.mean);
    }

    /// Associatively merge another partial's moments (Chan et al.). Order-free: `a.merge(b)`
    /// and `b.merge(a)` yield the same `(n, mean, m2)` up to FP rounding, and folding one
    /// sample at a time equals merging singletons — this is what makes chunking null.
    pub fn merge(&self, other: &RunningMoments) -> RunningMoments {
        if other.n == 0 {
            return self.clone();
        }
        if self.n == 0 {
            return other.clone();
        }
        let n = self.n + other.n;
        let delta = other.mean - self.mean;
        let mean = self.mean + delta * (other.n as f64 / n as f64);
        let m2 = self.m2 + other.m2 + delta * delta * (self.n as f64 * other.n as f64 / n as f64);
        RunningMoments { n, mean, m2 }
    }

    /// Sample standard deviation (n−1 denominator), matching `engine::std`. 0.0 for < 2 samples.
    pub fn sample_std(&self) -> f64 {
        if self.n < 2 {
            0.0
        } else {
            (self.m2 / (self.n - 1) as f64).sqrt()
        }
    }
}

/// Percentile-band accumulator. `Exact` retains per-step samples — batch-identical bands, at
/// O(realizations) memory per step (no memory win; this is the correctness anchor). A future
/// `Sketch(t-digest)` variant trades band exactness for O(1) memory and sets `bands_approximate`.
#[derive(Clone, Debug)]
pub enum BandAccum {
    Exact(Vec<f64>),
}

impl BandAccum {
    fn new_exact() -> Self {
        BandAccum::Exact(Vec::new())
    }

    fn push(&mut self, x: f64) {
        match self {
            BandAccum::Exact(v) => v.push(x),
        }
    }

    fn merge(&self, other: &BandAccum) -> BandAccum {
        match (self, other) {
            (BandAccum::Exact(a), BandAccum::Exact(b)) => {
                let mut out = a.clone();
                out.extend_from_slice(b);
                BandAccum::Exact(out)
            }
        }
    }

    /// True when bands are approximate (sketch-derived). Exact retention never is; a future
    /// `Sketch` variant will return true here (feeding the UI's `bandsApproximate` flag).
    pub fn approximate(&self) -> bool {
        match self {
            BandAccum::Exact(_) => false,
        }
    }
}

/// One element's streaming accumulator across all steps: per-step moments (for mean/sd) and
/// per-step band accumulators (for p05..p95). `n_steps` fixed at construction.
#[derive(Clone, Debug)]
pub struct ElementAccum {
    moments: Vec<RunningMoments>,
    bands: Vec<BandAccum>,
}

impl ElementAccum {
    pub fn new_exact(n_steps: usize) -> Self {
        ElementAccum {
            moments: vec![RunningMoments::default(); n_steps],
            bands: (0..n_steps).map(|_| BandAccum::new_exact()).collect(),
        }
    }

    /// Fold one realization's full time series (one value per step) into the accumulators.
    pub fn push_series(&mut self, series: &[f64]) {
        for (step, &x) in series.iter().enumerate() {
            self.moments[step].push(x);
            self.bands[step].push(x);
        }
    }

    /// Associatively merge another chunk's accumulator (same `n_steps`).
    pub fn merge(&self, other: &ElementAccum) -> ElementAccum {
        ElementAccum {
            moments: self.moments.iter().zip(&other.moments).map(|(a, b)| a.merge(b)).collect(),
            bands: self.bands.iter().zip(&other.bands).map(|(a, b)| a.merge(b)).collect(),
        }
    }

    /// Reduce to the batch `TimeHistoryStats` shape.
    ///
    /// In `Exact` mode the samples are retained, so **both** mean and percentiles are computed
    /// from them via the same fixed-order reductions the batch path uses (`engine::mean` /
    /// `engine::percentile`). This makes the reduction bit-identical to batch *and* invariant
    /// across chunkings — the retained samples are appended in realization order regardless of
    /// how they were chunked (concatenation preserves order; each chunk covers a contiguous
    /// realization range folded in order).
    ///
    /// Note: `RunningMoments` (Welford/Chan) is *not* used for the mean here. Its incremental
    /// merge is associative only in exact arithmetic — under FP, different chunk groupings give
    /// means that differ by a few ULP, breaking chunk-invariance. It is retained for a future
    /// `Sketch` mode that does *not* keep samples and must stream the mean; in `Exact` mode the
    /// samples are the ground truth.
    pub fn to_stats(&self) -> TimeHistoryStats {
        let reduce = |f: &dyn Fn(&[f64]) -> f64| -> Vec<f64> {
            self.bands
                .iter()
                .map(|b| match b {
                    BandAccum::Exact(v) => f(v),
                })
                .collect()
        };
        TimeHistoryStats {
            mean: reduce(&|v| mean(v)),
            p05: reduce(&|v| percentile(v, 5.0)),
            p25: reduce(&|v| percentile(v, 25.0)),
            p50: reduce(&|v| percentile(v, 50.0)),
            p75: reduce(&|v| percentile(v, 75.0)),
            p95: reduce(&|v| percentile(v, 95.0)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The batch `stats()` shape recomputed directly from per-step sample vectors — the oracle
    // this module must reproduce. (Mirrors engine_v2::stats, which is private.)
    fn batch_stats(per_step: &[Vec<f64>]) -> TimeHistoryStats {
        TimeHistoryStats {
            mean: per_step.iter().map(|vs| mean(vs)).collect(),
            p05: per_step.iter().map(|vs| percentile(vs, 5.0)).collect(),
            p25: per_step.iter().map(|vs| percentile(vs, 25.0)).collect(),
            p50: per_step.iter().map(|vs| percentile(vs, 50.0)).collect(),
            p75: per_step.iter().map(|vs| percentile(vs, 75.0)).collect(),
            p95: per_step.iter().map(|vs| percentile(vs, 95.0)).collect(),
        }
    }

    /// Deterministic synthetic per-realization series: value(real, step) — no RNG, so the test
    /// is reproducible (mirrors the engine's seed determinism discipline).
    fn synth(real: u64, step: usize) -> f64 {
        // SplitMix-ish → skewed positive value with step drift, so percentiles are non-trivial.
        let mut z = real
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ (step as u64).wrapping_mul(0xD1B5_4A32_D192_ED03);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z ^= z >> 31;
        let u = (z >> 11) as f64 / (1u64 << 53) as f64;
        -(1.0 - u).ln() * 10.0 + step as f64 * 0.5
    }

    fn build_per_step(n_real: u64, n_steps: usize) -> Vec<Vec<f64>> {
        (0..n_steps)
            .map(|s| (0..n_real).map(|r| synth(r, s)).collect())
            .collect()
    }

    /// Fold realizations [from,to) into an accumulator (one run_chunk's worth).
    fn fold_chunk(n_steps: usize, from: u64, to: u64) -> ElementAccum {
        let mut acc = ElementAccum::new_exact(n_steps);
        for r in from..to {
            let series: Vec<f64> = (0..n_steps).map(|s| synth(r, s)).collect();
            acc.push_series(&series);
        }
        acc
    }

    /// Chunk [0,n_real) into blocks of `chunk`, fold each, merge — the run_chunk + merge shape.
    fn accumulate_chunked(n_real: u64, n_steps: usize, chunk: u64) -> ElementAccum {
        let mut acc = ElementAccum::new_exact(n_steps);
        let mut from = 0;
        while from < n_real {
            let to = (from + chunk).min(n_real);
            acc = acc.merge(&fold_chunk(n_steps, from, to));
            from = to;
        }
        acc
    }

    fn bits(a: f64, b: f64) -> bool {
        a.to_bits() == b.to_bits()
    }

    #[test]
    fn exact_bands_match_batch_percentiles_bit_for_bit() {
        // Percentiles come from retained samples, so they must equal batch stats() EXACTLY
        // (percentile sorts, so accumulation order is irrelevant to the band result).
        let (n_real, n_steps) = (500u64, 20usize);
        let batch = batch_stats(&build_per_step(n_real, n_steps));
        let acc = accumulate_chunked(n_real, n_steps, 1).to_stats();
        for s in 0..n_steps {
            for (label, ba, bb) in [
                ("p05", acc.p05[s], batch.p05[s]),
                ("p25", acc.p25[s], batch.p25[s]),
                ("p50", acc.p50[s], batch.p50[s]),
                ("p75", acc.p75[s], batch.p75[s]),
                ("p95", acc.p95[s], batch.p95[s]),
            ] {
                assert!(bits(ba, bb), "{label} step {s}: {ba} != batch {bb}");
            }
        }
    }

    #[test]
    fn mean_matches_batch_bit_for_bit_in_exact_mode() {
        // In Exact mode the mean is computed from retained samples in realization order — the
        // same order the batch path sums — so it is bit-identical to batch, not merely close.
        // (The Welford/Chan RunningMoments path, which is NOT bit-identical to batch, is kept
        // only for a future sketch mode; see `moments_merge_is_order_free`.)
        let (n_real, n_steps) = (500u64, 20usize);
        let batch = batch_stats(&build_per_step(n_real, n_steps));
        let acc = accumulate_chunked(n_real, n_steps, 1).to_stats();
        for s in 0..n_steps {
            assert!(bits(acc.mean[s], batch.mean[s]), "mean step {s}: {} != batch {}", acc.mean[s], batch.mean[s]);
        }
    }

    #[test]
    fn chunk_invariance_all_chunkings_agree_bit_for_bit() {
        // The load-bearing streaming property: every chunking of the same run produces an
        // identical reduced result. This is what CI asserts (chunk-vs-chunk, not vs-batch).
        let (n_real, n_steps) = (500u64, 20usize);
        let reference = accumulate_chunked(n_real, n_steps, 1).to_stats();
        for chunk in [7u64, 33, 200, 499, 500] {
            let got = accumulate_chunked(n_real, n_steps, chunk).to_stats();
            for s in 0..n_steps {
                assert!(bits(got.mean[s], reference.mean[s]), "mean step {s} chunk {chunk} differs");
                assert!(bits(got.p50[s], reference.p50[s]), "p50 step {s} chunk {chunk} differs");
                assert!(bits(got.p95[s], reference.p95[s]), "p95 step {s} chunk {chunk} differs");
            }
        }
    }

    #[test]
    fn moments_merge_is_order_free() {
        // a.merge(b) == b.merge(a) for mean and n (m2 too, up to FP).
        let a = fold_chunk(1, 0, 137).moments[0].clone();
        let b = fold_chunk(1, 137, 500).moments[0].clone();
        let ab = a.merge(&b);
        let ba = b.merge(&a);
        assert_eq!(ab.n, ba.n);
        assert!(bits(ab.mean, ba.mean), "merge mean not order-free: {} vs {}", ab.mean, ba.mean);
    }

    #[test]
    fn sample_std_matches_engine_std() {
        let vals: Vec<f64> = (0..100).map(|r| synth(r, 3)).collect();
        let mut acc = RunningMoments::default();
        for &x in &vals {
            acc.push(x);
        }
        let want = crate::engine::std(&vals);
        assert!((acc.sample_std() - want).abs() < 1e-9, "std {} != engine {}", acc.sample_std(), want);
    }
}
