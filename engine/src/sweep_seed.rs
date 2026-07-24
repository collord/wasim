//! Sweep-seed coordinate (Phase 1 of sweep composition).
//!
//! A **sweep** is one Monte-Carlo execution of one model — the top-level run, or a nested
//! submodel run. Each sweep needs its own RNG stream so that two sweeps in the same model are
//! statistically independent. Before this module, every submodel re-seeded from the *same* base
//! seed (`submodel_v2.rs` built `RunConfig { seed: config.seed, .. }`), so two distinct submodels
//! drew byte-identical streams — a latent correctness bug in the two-level-uncertainty case (a
//! downstream node combining two submodels' statistics inherited spurious correlation).
//!
//! The fix derives each nested sweep's seed as `child_seed(root, sweep_id(container_id))`, where:
//!   - `root` is the effective base seed (resolved identically to `engine_v2::run`),
//!   - `sweep_id` is a **content-derived** hash of the submodel's stable container id — *not* its
//!     positional index, so reordering the model document is semantically null (a routine,
//!     diff-friendly edit must never change results).
//!
//! Note: "sweep" here is unrelated to `sensitivity_v2`'s `SweepVar` / line-sweep (a deterministic
//! parameter scan). Different concept, deliberately different module.

/// Sweep coordinate of the **top-level** run. Documentation-only: the top-level seeding path in
/// `engine_v2::run` is intentionally left untouched (so top-level streams stay bit-identical to
/// prior releases); this constant records that the top-level sweep's identity is 0.
pub const TOP_LEVEL_SWEEP_ID: u64 = 0;

/// FNV-1a hash of a sweep's stable identifier (the container id string). Content-derived and
/// order-independent, so it is stable under document reordering.
pub fn sweep_id_fnv1a(id: &str) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut h = OFFSET;
    for b in id.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(PRIME);
    }
    h
}

/// SplitMix64 finalizer — a good avalanche mixer used to disperse the base seed before combining
/// with a sweep id, so nearby roots (0, 1, 2, …) don't produce nearby child seeds.
fn splitmix64(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Derive a nested sweep's base seed from the run's effective root seed and the sweep's id.
///
/// `child_seed(root, sweep_id) = splitmix64(root) + sweep_id`. This is a pure function of its two
/// inputs, so a sweep's stream depends only on `(root, its own id)` — adding, removing, or
/// reordering *other* sweeps cannot perturb it (composition isolation).
pub fn child_seed(root: u64, sweep_id: u64) -> u64 {
    splitmix64(root).wrapping_add(sweep_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinct_ids_yield_distinct_child_seeds() {
        let root = 42;
        let a = child_seed(root, sweep_id_fnv1a("Model/A"));
        let b = child_seed(root, sweep_id_fnv1a("Model/B"));
        assert_ne!(a, b, "distinct submodel ids must produce distinct child seeds");
    }

    #[test]
    fn child_seed_is_content_derived_not_positional() {
        // Same id → same seed regardless of any surrounding order.
        let root = 7;
        assert_eq!(
            child_seed(root, sweep_id_fnv1a("Model/Loss")),
            child_seed(root, sweep_id_fnv1a("Model/Loss")),
        );
    }

    #[test]
    fn fnv1a_matches_known_vector() {
        // FNV-1a of the empty string is the offset basis.
        assert_eq!(sweep_id_fnv1a(""), 0xcbf2_9ce4_8422_2325);
    }
}
