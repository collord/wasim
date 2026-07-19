//! B6 true calendar / leap years: with a `calendar_start` anchor the time_ref calendar
//! properties use a real proleptic-Gregorian calendar (leap-aware); without one the fixed
//! 365-day calendar is unchanged.

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig};

/// A model whose elements expose year/month/day/day-of-year/days-in-month via time_ref, run on a
/// daily grid. `calendar_start` (seconds since epoch) anchors the real calendar when present.
fn calendar_model(days: u32, calendar_start: Option<f64>) -> String {
    let cs = match calendar_start {
        Some(s) => format!(r#", "calendar_start": {s}"#),
        None => String::new(),
    };
    format!(
        r#"{{"wasim_version": "0.9.4",
          "simulation_settings": {{"duration": {{"value": {days}, "unit": "d"}}, "timestep": {{"value": 1, "unit": "d"}}, "seed": 1{cs}}},
          "elements": [
            {{"id": "yr",  "name": "Yr",  "primitive": "node", "value_rule": "expression", "expression": {{"ast": {{"op": "time_ref", "property": "year"}}}}, "save_results": {{"time_history": true}}}},
            {{"id": "mo",  "name": "Mo",  "primitive": "node", "value_rule": "expression", "expression": {{"ast": {{"op": "time_ref", "property": "month"}}}}, "save_results": {{"time_history": true}}}},
            {{"id": "dom", "name": "Dom", "primitive": "node", "value_rule": "expression", "expression": {{"ast": {{"op": "time_ref", "property": "day_of_month"}}}}, "save_results": {{"time_history": true}}}},
            {{"id": "dim", "name": "Dim", "primitive": "node", "value_rule": "expression", "expression": {{"ast": {{"op": "time_ref", "property": "days_in_month"}}}}, "save_results": {{"time_history": true}}}}
          ]}}"#
    )
}

fn series(json: &str, id: &str) -> Vec<f64> {
    let m = parse_v2(json).expect("parse");
    let g = ModelGraphV2::build(&m).expect("build");
    let r = run_v2(&m, &g, &RunConfig { seed: Some(1), ..RunConfig::default() }).expect("run");
    r.elements[id].time_history.as_ref().unwrap().mean.clone()
}

/// 2024 is a leap year. Anchored at 2024-01-01 (1704067200s since epoch), Feb has 29 days and
/// day 59 (0-indexed from Jan 1) is Feb 29.
#[test]
fn leap_year_february_has_29_days() {
    // 2024-01-01T00:00:00Z = 1704067200 seconds since the Unix epoch.
    let json = calendar_model(70, Some(1_704_067_200.0));
    let mo = series(&json, "mo");
    let dom = series(&json, "dom");
    let dim = series(&json, "dim");
    // Step index 0 = Jan 1 (elapsed 0). Feb 29 is 31 (Jan) + 28 = day index 59 (0-based).
    // Feb runs from index 31 (Feb 1) to index 59 (Feb 29).
    assert_eq!(mo[31], 2.0, "index 31 should be February");
    assert_eq!(dom[31], 1.0, "index 31 should be Feb 1");
    assert_eq!(dim[31], 29.0, "February 2024 has 29 days (leap year)");
    assert_eq!(mo[59], 2.0, "index 59 should still be February");
    assert_eq!(dom[59], 29.0, "index 59 should be Feb 29");
    assert_eq!(mo[60], 3.0, "index 60 should be March 1");
    assert_eq!(dom[60], 1.0);
}

/// 2023 is NOT a leap year: February has 28 days and index 59 is March 1.
#[test]
fn non_leap_year_february_has_28_days() {
    // 2023-01-01T00:00:00Z = 1672531200 seconds.
    let json = calendar_model(70, Some(1_672_531_200.0));
    let mo = series(&json, "mo");
    let dom = series(&json, "dom");
    let dim = series(&json, "dim");
    assert_eq!(dim[31], 28.0, "February 2023 has 28 days (non-leap)");
    // Day index 59 = 31 (Jan) + 28 (Feb) = March 1.
    assert_eq!(mo[59], 3.0, "index 59 is March in a non-leap year");
    assert_eq!(dom[59], 1.0);
}

/// The year advances across a year boundary.
#[test]
fn year_advances_across_boundary() {
    // 2023-12-30 = 1703894400s. Run 5 days crosses into 2024.
    let json = calendar_model(5, Some(1_703_894_400.0));
    let yr = series(&json, "yr");
    let mo = series(&json, "mo");
    let dom = series(&json, "dom");
    assert_eq!(yr[0], 2023.0, "starts in 2023");
    assert_eq!(mo[0], 12.0);
    assert_eq!(dom[0], 30.0, "starts Dec 30");
    // index 0 = Dec 30, 1 = Dec 31, 2 = Jan 1 2024.
    assert_eq!(yr[2], 2024.0, "index 2 crosses into 2024");
    assert_eq!(mo[2], 1.0);
    assert_eq!(dom[2], 1.0);
}

/// Without a calendar_start anchor the fixed 365-day calendar is used (behavior unchanged):
/// February always has 28 days, no leap handling.
#[test]
fn no_anchor_uses_fixed_365_calendar() {
    let json = calendar_model(40, None);
    let dim = series(&json, "dim");
    // Fixed calendar: February is always 28 days (day index 31 = Feb 1).
    assert_eq!(dim[31], 28.0, "fixed calendar February is always 28 days");
}
