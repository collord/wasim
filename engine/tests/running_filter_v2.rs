// Gap #2: the `filter` node as a native running/time-average statistic. An expanding window
// (window omitted / 0) gives a cumulative mean/sum/min/max over every step so far — replacing the
// hand-rolled sumX + step-counter + divide idiom (and its one-step stock-read-lag footgun), and
// unlocking running extremes for barrier/lookback payoffs. Validates the filter's internal
// consistency (mean == sum/count, computed with three filters), bracketing, the step count, and a
// bounded window — all reachable from the v1 schema (this is a v1 model, normalize_v1 -> run_v2).
use wasim_engine::{normalize_v1, run_v2, ModelGraphV2, RunConfig, WasimModel};

const JSON: &str = r#"{
  "wasim_version":"0.1.0",
  "simulation_settings":{"duration":{"value":1.0,"unit":"yr"},"timestep":{"value":0.05,"unit":"yr"},"n_realizations":4000,"seed":3},
  "elements":[
    {"id":"one","name":"one","type":"constant","value":{"value":1.0,"unit":"1"},"editable":false,"bounds":{"min":null,"max":null}},
    {"id":"P","name":"P","type":"stochastic_process","process":{"family":"gbm","mean_type":"arithmetic","mean":{"value":0.08,"unit":"1/yr"},"stddev":{"value":0.25,"unit":"1/yr"}}},
    {"id":"S","name":"S","type":"accumulator","initial_value":{"value":100.0,"unit":"USD"},"inputs":["S","P"],
     "rate":{"ast":{"op":"multiply","left":{"op":"ref","element_id":"S"},"right":{"op":"ref","element_id":"P"}},"display":"S*P"},
     "min_value":null,"capacity":null,"save_results":{"final_value":true}},

    {"id":"favg","name":"favg","type":"filter","input":"S","statistic":"mean","save_results":{"final_value":true}},
    {"id":"fsum","name":"fsum","type":"filter","input":"S","statistic":"sum","save_results":{"final_value":true}},
    {"id":"fcount","name":"fcount","type":"filter","input":"one","statistic":"sum","save_results":{"final_value":true}},
    {"id":"fmin","name":"fmin","type":"filter","input":"S","statistic":"min","save_results":{"final_value":true}},
    {"id":"fmax","name":"fmax","type":"filter","input":"S","statistic":"max","save_results":{"final_value":true}},
    {"id":"flast","name":"flast","type":"filter","input":"S","window":1,"statistic":"mean","save_results":{"final_value":true}}
  ]
}"#;

fn run() -> std::collections::HashMap<String, Vec<f64>> {
    let m = normalize_v1(&serde_json::from_str::<WasimModel>(JSON).unwrap());
    let g = ModelGraphV2::build(&m).unwrap();
    let r = run_v2(&m, &g, &RunConfig::default()).unwrap();
    ["favg", "fsum", "fcount", "fmin", "fmax", "flast"]
        .iter().map(|id| (id.to_string(), r.elements[*id].final_values.clone())).collect()
}

#[test]
fn expanding_filter_is_a_native_running_statistic() {
    let e = run();
    let (avg, sum, cnt, fmin, fmax, flast) =
        (&e["favg"], &e["fsum"], &e["fcount"], &e["fmin"], &e["fmax"], &e["flast"]);
    let n = avg.len();

    for i in 0..n {
        // (1) Internal consistency: the expanding mean equals sum/count over exactly the points the
        // filter observed — a running average with its own exact count, no lag trick.
        assert!((avg[i] - sum[i] / cnt[i]).abs() < 1e-9, "mean==sum/count at {i}: {} vs {}", avg[i], sum[i] / cnt[i]);
        // (2) Extremes bracket the mean.
        assert!(fmin[i] <= avg[i] + 1e-9 && avg[i] <= fmax[i] + 1e-9, "min<=mean<=max at {i}");
        // (3) The path starts at 100, so the running min never exceeds 100.
        assert!(fmin[i] <= 100.0 + 1e-9, "running min <= 100 at {i}");
    }

    // (4) Every realization saw the same number of monitoring points: duration/timestep = 20.
    assert!(cnt.iter().all(|&c| (c - 20.0).abs() < 1e-9), "expanding filter counts all 20 steps");

    // (5) A bounded window=1 mean is just the current value (bracketed by the running extremes).
    for i in 0..n {
        assert!(flast[i] >= fmin[i] - 1e-9 && flast[i] <= fmax[i] + 1e-9, "window=1 value within [min,max]");
    }
    println!("expanding filter: mean==sum/count, count==20, min<=mean<=max — native running statistic");
}
