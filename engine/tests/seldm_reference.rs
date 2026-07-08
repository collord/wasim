//! SELDM reference statistical functions, transcribed from the original Access/VBA
//! `modStatistics.bas` (U.S. Geological Survey, G.E. Granato), for parity checking the
//! WASiM port. These are faithful ports of the VBA — kept deliberately close to the
//! source (naming, branch structure) so they can be audited against the `.bas` file.
//!
//! Scope (Phase 1 of the SELDM→WASiM port):
//!   - MRG32k3a  : L'Ecuyer combined multiple-recursive uniform RNG
//!   - AS241     : uniform → standard-normal inverse CDF (Wichura 1988)
//!   - Wilson-Hilferty-Kirby : standard-normal → Pearson-III (skewed) variate
//!   - plotting positions : rank → plotting position (Blom/Cunnane/… )
//!
//! This file defines no #[test]s itself; it is a support module `include!`d or referenced
//! by the parity tests. To keep it compiling as its own test target, we expose the
//! functions as `pub` and add one self-check test at the bottom.

#![allow(dead_code)]

/// L'Ecuyer (1999) MRG32k3a combined multiple-recursive generator.
/// Mirrors SELDM `MRG32k3a`. Seeds are carried as six f64 state words; the call
/// advances them in place and returns a uniform in (0, 1).
pub struct Mrg32k3a {
    s: [f64; 6], // s10, s11, s12, s20, s21, s22
}

impl Mrg32k3a {
    const NORM: f64 = 2.328_306_549_295_73e-10;
    const M1: f64 = 4_294_967_087.0;
    const M2: f64 = 4_294_944_443.0;
    const A12: f64 = 1_403_580.0;
    const A13N: f64 = 810_728.0;
    const A22: f64 = 527_612.0;
    const A23N: f64 = 1_370_589.0;

    /// Seed from two starting words (as SELDM does via GetStartSeed); the other four
    /// are back-filled the way `MRG32k3a` back-fills zero seeds.
    pub fn new(seed10: f64, seed20: f64) -> Self {
        Mrg32k3a { s: [seed10, seed10, seed10, seed20, seed20, seed20] }
    }

    pub fn next_u01(&mut self) -> f64 {
        // Component 1
        let mut p1 = Self::A12 * self.s[1] - Self::A13N * self.s[0];
        let k = (p1 / Self::M1).floor();
        p1 -= k * Self::M1;
        if p1 < 0.0 {
            p1 += Self::M1;
        }
        self.s[0] = self.s[1];
        self.s[1] = self.s[2];
        self.s[2] = p1;

        // Component 2
        let mut p2 = Self::A22 * self.s[5] - Self::A23N * self.s[3];
        let k = (p2 / Self::M2).floor();
        p2 -= k * Self::M2;
        if p2 < 0.0 {
            p2 += Self::M2;
        }
        self.s[3] = self.s[4];
        self.s[4] = self.s[5];
        self.s[5] = p2;

        if p1 <= p2 {
            ((p1 - p2) + Self::M1) * Self::NORM
        } else {
            (p1 - p2) * Self::NORM
        }
    }
}

/// AS241 (Wichura 1988): standard-normal inverse CDF. Transcribed from SELDM
/// `fndUniform01ToNormalAS241` with the coefficients primed in `PrimeAS241`.
pub fn as241_normal(u: f64) -> f64 {
    const A: [f64; 8] = [
        3.387_132_872_796_37,
        133.141_667_891_784,
        1_971.590_950_306_55,
        13_731.693_765_509_5,
        45_921.953_931_549_9,
        67_265.770_927_008_7,
        33_430.575_583_588,
        2_509.080_928_730_12,
    ];
    const B: [f64; 7] = [
        42.313_330_701_600_9,
        687.187_007_492_058,
        5_394.196_021_424_75,
        21_213.794_301_586_6,
        39_307.895_800_092_7,
        28_729.085_735_721_9,
        5_226.495_278_852_85,
    ];
    const C: [f64; 8] = [
        1.423_437_110_749_68,
        4.630_337_846_156_54,
        5.769_497_221_460_69,
        3.647_848_324_763_2,
        1.270_458_252_452_37,
        0.241_780_725_177_451,
        2.272_384_498_926_92e-2,
        7.745_450_142_783_41e-4,
    ];
    const D: [f64; 7] = [
        2.053_191_626_637_76,
        1.676_384_830_183_8,
        0.689_767_334_985_1,
        0.148_103_976_427_48,
        1.519_866_656_361_65e-2,
        5.475_938_084_995_34e-4,
        1.050_750_071_644_42e-9,
    ];
    const E: [f64; 8] = [
        6.657_904_643_501_1,
        5.463_784_911_164_11,
        1.784_826_539_917_29,
        0.296_560_571_828_504,
        2.653_218_952_657_61e-2,
        1.242_660_947_388_07e-3,
        2.711_555_568_743_48e-5,
        2.010_334_399_292_28e-7,
    ];
    const F: [f64; 7] = [
        0.599_832_206_555_887,
        0.136_929_880_922_735,
        1.487_536_129_085_06e-2,
        7.868_691_311_456_13e-4,
        1.846_318_317_510_05e-5,
        1.421_511_758_316_44e-7,
        2.044_263_103_389_93e-15,
    ];

    let mut p = u;
    if (0.499_999_999_999_999..0.500_000_000_000_001).contains(&p) {
        return 0.0;
    }
    if p <= 1e-21 {
        p = 1e-21;
    }
    if p >= 1.0 {
        p = 0.999_999_999_999_999;
    }
    let q = p - 0.5;
    if q.abs() <= 0.425 {
        let r = 0.180_625 - q * q;
        q * (((((((A[7] * r + A[6]) * r + A[5]) * r + A[4]) * r + A[3]) * r + A[2]) * r + A[1]) * r
            + A[0])
            / (((((((B[6] * r + B[5]) * r + B[4]) * r + B[3]) * r + B[2]) * r + B[1]) * r + B[0])
                * r
                + 1.0)
    } else {
        let mut r = if q < 0.0 { p } else { 1.0 - p };
        r = (-r.ln()).sqrt();
        let val = if r <= 5.0 {
            r -= 1.6;
            (((((((C[7] * r + C[6]) * r + C[5]) * r + C[4]) * r + C[3]) * r + C[2]) * r + C[1]) * r
                + C[0])
                / (((((((D[6] * r + D[5]) * r + D[4]) * r + D[3]) * r + D[2]) * r + D[1]) * r
                    + D[0])
                    * r
                    + 1.0)
        } else {
            r -= 5.0;
            (((((((E[7] * r + E[6]) * r + E[5]) * r + E[4]) * r + E[3]) * r + E[2]) * r + E[1]) * r
                + E[0])
                / (((((((F[6] * r + F[5]) * r + F[4]) * r + F[3]) * r + F[2]) * r + F[1]) * r
                    + F[0])
                    * r
                    + 1.0)
        };
        if q < 0.0 {
            -val
        } else {
            val
        }
    }
}

/// Kirby's computer-oriented Wilson-Hilferty transform (SELDM
/// `fndAdjustedWilsonHilfertyK`): map a standard-normal variate `normal_k` and a skew to
/// the equivalent Pearson-III standardized variate. This is SELDM's Pearson-III sampling
/// path (normal via AS241, then this transform), which the WASiM engine reaches instead
/// via a 3-parameter gamma. Parity is at the *distribution* level, not per-draw.
pub fn wilson_hilferty_kirby(input_skew: f64, normal_k: f64) -> f64 {
    let abs_skew = input_skew.abs();
    if abs_skew < 0.005 {
        return normal_k;
    }
    // Unadjusted Wilson-Hilferty for small skew (< 0.5).
    if abs_skew < 0.5 {
        let s6 = input_skew / 6.0;
        return (2.0 / input_skew) * ((s6 * (normal_k - s6) + 1.0).powi(3) - 1.0);
    }
    let mut skew = input_skew;
    if skew > 9.75 {
        skew = 9.75;
    }
    if skew < -9.75 {
        skew = -9.75;
    }
    let (a, b, g, h) = prime_whk(skew);
    if input_skew < 0.0 {
        let mut val = 1.0 - (g / 6.0) * (g / 6.0) - (g / 6.0) * normal_k;
        if h > val {
            val = h;
        }
        -a * (val * val * val - b)
    } else {
        let mut val = 1.0 - (g / 6.0) * (g / 6.0) + (g / 6.0) * normal_k;
        if h > val {
            val = h;
        }
        a * (val * val * val - b)
    }
}

/// SELDM `PrimeWilsonHilfertyKirby`: interpolation-table adjustment of the A/B/G/H
/// Wilson-Hilferty parameters. Returns (A, B, G, H).
fn prime_whk(in_skew: f64) -> (f64, f64, f64, f64) {
    // Difference tables vs. the approximations, skews 0..9.75 step 0.25 (indices 1..=40).
    const DG: [f64; 41] = [
        0.0, 0.0, -0.000144, -0.001137, -0.003762, -0.008674, -0.011555, -0.010076, -0.006049,
        -0.000921, 0.004189, 0.008515, 0.011584, 0.013139, 0.013122, 0.010945, 0.007546, 0.002767,
        -0.003181, -0.010089, -0.017528, -0.025476, -0.033609, -0.042434, -0.050525, -0.058192,
        -0.065221, -0.07141, -0.076638, -0.080655, -0.083349, -0.084584, -0.084203, -0.082089,
        -0.078126, -0.072165, -0.064188, -0.054059, -0.041633, -0.027005, -0.010188,
    ];
    const DA: [f64; 41] = [
        0.0, 0.0, 0.004614, 0.009159, 0.013553, 0.017753, 0.021764, 0.025834, 0.030406, 0.03571,
        0.04173, 0.048321, 0.055309, 0.062538, 0.069873, 0.077334, 0.084682, 0.091926, 0.099028,
        0.105967, 0.112695, 0.119245, 0.106551, 0.095488, 0.085671, 0.07699, 0.06929, 0.062443,
        0.056349, 0.050908, 0.046047, 0.041702, 0.037815, 0.034339, 0.031229, 0.028445, 0.025964,
        0.023753, 0.021782, 0.020043, 0.018528,
    ];
    const DB: [f64; 41] = [
        0.0, 0.0, 0.0, -0.000001, -0.000004, -0.000021, -0.000075, -0.00019, -0.000326, -0.000317,
        0.000116, 0.000434, 0.000116, -0.000464, -0.000981, -0.001165, -0.000743, 0.000435,
        0.002479, 0.005462, 0.009353, 0.014206, 0.019964, 0.026829, 0.034307, 0.042495, 0.051293,
        0.060593, 0.070324, 0.080332, 0.090532, 0.100831, 0.111114, 0.121283, 0.131245, 0.140853,
        0.15012, 0.158901, 0.167085, 0.174721, 0.181994,
    ];

    let mut skew = in_skew.abs();
    if skew < 0.0005 {
        skew = 0.0005;
    }
    if skew > 9.75 {
        skew = 9.75;
    }

    // Table skews: t_skew(i) = (i-1)*0.25 for i in 1..=40.
    let t_skew = |i: usize| (i as f64 - 1.0) * 0.25;
    let mut i = 2usize;
    while t_skew(i) < skew {
        i += 1;
    }
    let p = (t_skew(i) - skew) / 0.25;
    let q = 1.0 - p;

    let mut g = skew + (q * DG[i] + p * DG[i - 1]);
    if skew > 1.0 {
        g -= 0.063 * (skew - 1.0).powf(1.85);
    }
    let approx_a = if (2.0 / skew) > 0.4 { 2.0 / skew } else { 0.4 };
    let a = approx_a + (q * DA[i] + p * DA[i - 1]);
    let approx_b = if skew <= 2.25 {
        1.0
    } else {
        1.0 + 0.0144 * (skew - 2.25).powi(2)
    };
    let b = approx_b + (q * DB[i] + p * DB[i - 1]);
    let h = (b - ((2.0 / skew) / a)).powf(1.0 / 3.0);
    (a, b, g, h)
}

/// SELDM `fndPlottingPosition`: rank → plotting position. `formula`: 0 Blom, 1 Cunnane,
/// 2 Gringorten, 3 Hazen, 4 Median, 5 Weibull.
pub fn plotting_position(rank: usize, count: usize, formula: u8, ascending: bool) -> f64 {
    let a = match formula {
        0 => 0.375,
        1 => 0.4,
        2 => 0.44,
        3 => 0.5,
        4 => 0.3175,
        _ => 0.0,
    };
    let pp = (rank as f64 - a) / (count as f64 + 1.0 - 2.0 * a);
    if ascending {
        pp
    } else {
        1.0 - pp
    }
}

/// SELDM `fnlngRankFromPP`: inverse of `plotting_position` — the rank (1-based) whose
/// plotting position is nearest the requested `pp`. `formula` as in `plotting_position`.
pub fn fnlng_rank_from_pp(pp: f64, count: usize, formula: u8, ascending: bool) -> usize {
    let a = match formula {
        0 => 0.375,
        1 => 0.4,
        2 => 0.44,
        3 => 0.5,
        4 => 0.3175,
        _ => 0.0,
    };
    let p = if ascending { pp } else { 1.0 - pp };
    let rank = (p * (count as f64 + 1.0 - 2.0 * a) + a).round() as i64;
    rank.clamp(1, count as i64) as usize
}

/// SELDM `fndUniform01ToTrapezoid` (Kacker & Lawrence, 2007), non-degenerate branch.
/// Exposed for the parity tests. Assumes min ≤ lower ≤ upper ≤ max and min < max, and
/// excludes the buggy rectangle branch of the original (see the trapezoid unit tests).
pub fn seldm_trapezoid_public(u01: f64, min: f64, lower: f64, upper: f64, max: f64) -> f64 {
    let h = 2.0 / ((max - min) + (upper - lower));
    if u01 >= 0.0 && u01 <= (h / 2.0) * (lower - min) {
        min + (2.0 * ((lower - min) / h)).sqrt() * u01.sqrt()
    } else if u01 > (h / 2.0) * (lower - min) && u01 <= 1.0 - (h / 2.0) * (max - upper) {
        (min + lower) / 2.0 + u01 / h
    } else {
        max - (2.0 * (max - upper) / h).sqrt() * (1.0 - u01).sqrt()
    }
}

/// Convenience: full SELDM Pearson-III draw from a uniform — u → normal (AS241) → WHK.
pub fn seldm_pearson3(u: f64, mean: f64, stddev: f64, skew: f64) -> f64 {
    let k = wilson_hilferty_kirby(skew, as241_normal(u));
    mean + stddev * k
}

// ── Self-checks (sanity, not parity) ──────────────────────────────────────────

#[test]
fn as241_symmetry_and_median() {
    assert!(as241_normal(0.5).abs() < 1e-12);
    // Φ⁻¹(0.975) ≈ 1.959964
    assert!((as241_normal(0.975) - 1.959_964).abs() < 1e-4);
    assert!((as241_normal(0.025) + 1.959_964).abs() < 1e-4);
}

#[test]
fn mrg_uniform_in_unit_interval_and_mean_half() {
    let mut g = Mrg32k3a::new(12345.0, 67890.0);
    let mut sum = 0.0;
    let n = 50_000;
    for _ in 0..n {
        let u = g.next_u01();
        assert!(u > 0.0 && u < 1.0, "u out of (0,1): {u}");
        sum += u;
    }
    let mean = sum / n as f64;
    assert!((mean - 0.5).abs() < 0.01, "MRG mean {mean}");
}

#[test]
fn whk_zero_skew_is_identity() {
    // With ~zero skew, the Pearson-III variate collapses to the normal variate.
    for &z in &[-2.0, -0.5, 0.0, 0.7, 1.5] {
        assert!((wilson_hilferty_kirby(0.0, z) - z).abs() < 1e-9);
    }
}
