// Basket scope, Phase 2: a `correlation_matrices` block is an ergonomic alternative to declaring
// pairwise `correlations` on each driver. It expands to the same pairwise edges during normalization,
// so it lowers to the existing Cholesky machinery. This test proves the equivalence is EXACT: a model
// specifying a correlation matrix produces bit-identical correlated draws to the hand-written pairwise
// model, and the realized sample correlations match the targets.
use wasim_engine::{normalize_v1, run_v2, ModelGraphV2, RunConfig, WasimModel};

// Three standard-normal drivers with target correlations rho12=0.3, rho13=0.5, rho23=0.2.
const PAIRWISE: &str = r#"{
  "wasim_version":"0.1.0",
  "simulation_settings":{"duration":{"value":1.0,"unit":"yr"},"timestep":{"value":1.0,"unit":"yr"},"n_realizations":40000,"seed":99},
  "elements":[
    {"id":"Z1","name":"Z1","type":"random_variable","distribution":{"family":"normal","parameters":{"mean":{"value":0.0,"unit":"1"},"stddev":{"value":1.0,"unit":"1"}}},
     "correlations":[{"partner":"Z2","coefficient":0.3},{"partner":"Z3","coefficient":0.5}],"save_results":{"final_value":true}},
    {"id":"Z2","name":"Z2","type":"random_variable","distribution":{"family":"normal","parameters":{"mean":{"value":0.0,"unit":"1"},"stddev":{"value":1.0,"unit":"1"}}},
     "correlations":[{"partner":"Z3","coefficient":0.2}],"save_results":{"final_value":true}},
    {"id":"Z3","name":"Z3","type":"random_variable","distribution":{"family":"normal","parameters":{"mean":{"value":0.0,"unit":"1"},"stddev":{"value":1.0,"unit":"1"}}},"save_results":{"final_value":true}}
  ]
}"#;

// Same three drivers, no pairwise correlations — a single correlation_matrices block instead.
const MATRIX: &str = r#"{
  "wasim_version":"0.1.0",
  "simulation_settings":{"duration":{"value":1.0,"unit":"yr"},"timestep":{"value":1.0,"unit":"yr"},"n_realizations":40000,"seed":99},
  "correlation_matrices":[
    {"variables":["Z1","Z2","Z3"],"matrix":[[1.0,0.3,0.5],[0.3,1.0,0.2],[0.5,0.2,1.0]]}
  ],
  "elements":[
    {"id":"Z1","name":"Z1","type":"random_variable","distribution":{"family":"normal","parameters":{"mean":{"value":0.0,"unit":"1"},"stddev":{"value":1.0,"unit":"1"}}},"save_results":{"final_value":true}},
    {"id":"Z2","name":"Z2","type":"random_variable","distribution":{"family":"normal","parameters":{"mean":{"value":0.0,"unit":"1"},"stddev":{"value":1.0,"unit":"1"}}},"save_results":{"final_value":true}},
    {"id":"Z3","name":"Z3","type":"random_variable","distribution":{"family":"normal","parameters":{"mean":{"value":0.0,"unit":"1"},"stddev":{"value":1.0,"unit":"1"}}},"save_results":{"final_value":true}}
  ]
}"#;

fn finals(json: &str) -> std::collections::HashMap<String, Vec<f64>> {
    let m = normalize_v1(&serde_json::from_str::<WasimModel>(json).unwrap());
    let g = ModelGraphV2::build(&m).unwrap();
    let r = run_v2(&m, &g, &RunConfig::default()).unwrap();
    ["Z1", "Z2", "Z3"].iter().map(|id| (id.to_string(), r.elements[*id].final_values.clone())).collect()
}
fn corr(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len() as f64;
    let (ma, mb) = (a.iter().sum::<f64>() / n, b.iter().sum::<f64>() / n);
    let mut cov = 0.0; let (mut va, mut vb) = (0.0, 0.0);
    for i in 0..a.len() { cov += (a[i]-ma)*(b[i]-mb); va += (a[i]-ma).powi(2); vb += (b[i]-mb).powi(2); }
    cov / (va.sqrt() * vb.sqrt())
}

#[test]
fn correlation_matrix_lowers_to_pairwise_exactly() {
    let pw = finals(PAIRWISE);
    let mx = finals(MATRIX);
    // (1) EXACT equivalence: the matrix expands to the same pairwise edges → same Cholesky → identical
    // correlated draws, per realization.
    for id in ["Z1", "Z2", "Z3"] {
        let (a, b) = (&pw[id], &mx[id]);
        assert_eq!(a.len(), b.len());
        for i in 0..a.len() {
            assert!((a[i] - b[i]).abs() < 1e-12, "{id} differs at {i}: {} vs {}", a[i], b[i]);
        }
    }
    // (2) The correlations are actually applied and match the targets (~0.3, 0.5, 0.2).
    let (c12, c13, c23) = (corr(&mx["Z1"], &mx["Z2"]), corr(&mx["Z1"], &mx["Z3"]), corr(&mx["Z2"], &mx["Z3"]));
    println!("realized correlations: rho12={c12:.3} rho13={c13:.3} rho23={c23:.3}");
    assert!((c12 - 0.3).abs() < 0.03 && (c13 - 0.5).abs() < 0.03 && (c23 - 0.2).abs() < 0.03,
        "realized correlations should match the matrix targets");
    println!("correlation_matrix == pairwise correlations (bit-identical draws), correlations on target");
}
