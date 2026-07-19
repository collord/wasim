//! Pluggable timebase (B1, gap #1 core). A `TimebaseProvider` yields the ordered interior
//! split points that refine *integration* within a single grid step. The grid remains the
//! statistical / state-machine / reporting lattice — sub-steps refine integration only, consume
//! no randomness, and never change the `n_steps`-shaped results contract.
//!
//! Providers compose by union of split points. `FixedGrid` yields none → the engine's original
//! fixed-step behavior, bit-identical (the phase-1 regression gate).
//!
//! This module is deliberately state-light: it computes split *times* from a read-only view of
//! the step (start time, dt, and — for bound crossings — each stock's level and current net
//! rate). The engine owns all mutation; the provider only answers "where should this grid step
//! be subdivided?".

/// A read-only view of one grid step's starting state, passed to providers.
pub struct StepView<'a> {
    /// 0-based grid step index.
    pub step_idx: usize,
    /// Absolute elapsed time at the start of this grid step (grid `t`).
    pub t_start: f64,
    /// Grid timestep length (grid `dt`).
    pub dt: f64,
    /// Per-stock (level_at_substep_start, net_rate) for bound-crossing analysis. The net rate is
    /// the level's time-derivative held constant across the (sub-)interval under Euler. Only
    /// stocks with a finite lower/upper bound need be present.
    pub stock_bounds: &'a [StockBoundView],
}

/// One stock's bound-crossing inputs for the current sub-interval.
pub struct StockBoundView {
    /// Level at the start of the current sub-interval.
    pub level: f64,
    /// Net rate (d level / d t) held constant across the sub-interval (Euler).
    pub rate: f64,
    /// Lower bound (floor), if any.
    pub floor: Option<f64>,
    /// Upper bound (capacity), if any.
    pub capacity: Option<f64>,
}

/// Yields interior split points (absolute times strictly inside `(t_start, t_start+dt)`), sorted
/// ascending and deduplicated. An empty result means "no subdivision" (fixed grid).
pub trait TimebaseProvider {
    fn split_points(&self, view: &StepView) -> Vec<f64>;
}

/// The default: never subdivides. Bit-identical to the pre-B1 engine.
pub struct FixedGrid;
impl TimebaseProvider for FixedGrid {
    fn split_points(&self, _view: &StepView) -> Vec<f64> {
        Vec::new()
    }
}

/// Exact known instants: scheduled event / trigger times collected at run start. Any that fall
/// strictly inside this grid step become split points.
pub struct ScheduledTimes {
    /// Absolute scheduled times (seconds / model time units), sorted ascending.
    pub times: Vec<f64>,
}
impl TimebaseProvider for ScheduledTimes {
    fn split_points(&self, view: &StepView) -> Vec<f64> {
        let (lo, hi) = (view.t_start, view.t_start + view.dt);
        self.times
            .iter()
            .copied()
            .filter(|&t| t > lo + EPS && t < hi - EPS)
            .collect()
    }
}

/// Bound crossings: under Euler the within-step trajectory is linear (rate constant), so the time
/// a stock's level reaches its floor/capacity is closed-form — no root finding. Emits the single
/// earliest crossing time in the interval (the engine re-evaluates after applying up to it, so
/// cascading crossings are found by re-invocation).
pub struct BoundCrossing;
impl TimebaseProvider for BoundCrossing {
    fn split_points(&self, view: &StepView) -> Vec<f64> {
        let hi = view.t_start + view.dt;
        let mut earliest = f64::INFINITY;
        for sb in view.stock_bounds {
            if sb.rate == 0.0 || !sb.rate.is_finite() {
                continue;
            }
            // Time to reach each bound from the current level at the constant rate.
            for bound in [sb.floor, sb.capacity].into_iter().flatten() {
                let dt_cross = (bound - sb.level) / sb.rate;
                if dt_cross > EPS {
                    let t = view.t_start + dt_cross;
                    if t > view.t_start + EPS && t < hi - EPS {
                        earliest = earliest.min(t);
                    }
                }
            }
        }
        if earliest.is_finite() {
            vec![earliest]
        } else {
            Vec::new()
        }
    }
}

/// Compose several providers: the union of their split points, sorted and deduplicated.
pub struct Composite {
    pub providers: Vec<Box<dyn TimebaseProvider>>,
}
impl TimebaseProvider for Composite {
    fn split_points(&self, view: &StepView) -> Vec<f64> {
        let mut pts: Vec<f64> = self
            .providers
            .iter()
            .flat_map(|p| p.split_points(view))
            .collect();
        dedup_sorted(&mut pts);
        pts
    }
}

/// A small epsilon (in model time units) below which two split points are considered coincident
/// and points are treated as on the grid boundary rather than interior.
pub const EPS: f64 = 1e-9;

/// Sort ascending and drop near-duplicates (within EPS).
pub fn dedup_sorted(pts: &mut Vec<f64>) {
    pts.sort_by(f64::total_cmp);
    pts.dedup_by(|a, b| (*a - *b).abs() <= EPS);
}

/// Given a grid step `[t_start, t_start+dt)` and interior split points, produce the ordered
/// sub-interval boundaries `[t_start, s_0, s_1, ..., t_start+dt]` (always begins at t_start and
/// ends at the grid boundary). Points outside the open interval are ignored.
pub fn sub_boundaries(t_start: f64, dt: f64, mut splits: Vec<f64>) -> Vec<f64> {
    let hi = t_start + dt;
    splits.retain(|&t| t > t_start + EPS && t < hi - EPS);
    dedup_sorted(&mut splits);
    let mut b = Vec::with_capacity(splits.len() + 2);
    b.push(t_start);
    b.extend(splits);
    b.push(hi);
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(t_start: f64, dt: f64, stocks: Vec<StockBoundView>) -> StepView<'static> {
        // Leak the stock vec for a 'static view in tests (small, test-only).
        let leaked: &'static [StockBoundView] = Box::leak(stocks.into_boxed_slice());
        StepView { step_idx: 0, t_start, dt, stock_bounds: leaked }
    }

    #[test]
    fn fixed_grid_yields_nothing() {
        let v = view(0.0, 1.0, vec![]);
        assert!(FixedGrid.split_points(&v).is_empty());
    }

    #[test]
    fn scheduled_times_inside_only() {
        let s = ScheduledTimes { times: vec![0.0, 0.5, 1.0, 1.5] };
        let v = view(0.0, 1.0, vec![]);
        // Only 0.5 is strictly interior to (0, 1).
        assert_eq!(s.split_points(&v), vec![0.5]);
    }

    #[test]
    fn bound_crossing_capacity_closed_form() {
        // Level 8, rate +4/unit, capacity 10 → crosses at t = (10-8)/4 = 0.5 inside (0,1).
        let v = view(0.0, 1.0, vec![StockBoundView { level: 8.0, rate: 4.0, floor: None, capacity: Some(10.0) }]);
        let pts = BoundCrossing.split_points(&v);
        assert_eq!(pts.len(), 1);
        assert!((pts[0] - 0.5).abs() < 1e-9, "got {:?}", pts);
    }

    #[test]
    fn bound_crossing_floor_and_earliest_wins() {
        // Two stocks: one hits floor at 0.25, one hits capacity at 0.75 → earliest 0.25.
        let v = view(0.0, 1.0, vec![
            StockBoundView { level: 1.0, rate: -4.0, floor: Some(0.0), capacity: None },
            StockBoundView { level: 0.0, rate: 4.0, floor: None, capacity: Some(3.0) },
        ]);
        let pts = BoundCrossing.split_points(&v);
        assert_eq!(pts.len(), 1);
        assert!((pts[0] - 0.25).abs() < 1e-9, "got {:?}", pts);
    }

    #[test]
    fn bound_crossing_no_crossing_when_rate_moves_away() {
        // Level 8, rate -4 (moving away from capacity 10, toward absent floor) → no crossing.
        let v = view(0.0, 1.0, vec![StockBoundView { level: 8.0, rate: -4.0, floor: None, capacity: Some(10.0) }]);
        assert!(BoundCrossing.split_points(&v).is_empty());
    }

    #[test]
    fn composite_unions_and_dedups() {
        let c = Composite { providers: vec![
            Box::new(ScheduledTimes { times: vec![0.5] }),
            Box::new(ScheduledTimes { times: vec![0.5, 0.25] }),
        ]};
        let v = view(0.0, 1.0, vec![]);
        assert_eq!(c.split_points(&v), vec![0.25, 0.5]);
    }

    #[test]
    fn sub_boundaries_wraps_grid() {
        let b = sub_boundaries(2.0, 1.0, vec![2.25, 2.75]);
        assert_eq!(b, vec![2.0, 2.25, 2.75, 3.0]);
        // No splits → just the grid boundary.
        assert_eq!(sub_boundaries(2.0, 1.0, vec![]), vec![2.0, 3.0]);
    }
}
