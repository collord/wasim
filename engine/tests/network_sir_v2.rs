//! Network ABM proof: contagion on an explicit graph, built entirely on today's engine —
//! adjacency matrix + per-node state vector + axis-selective reduction over the neighbor axis
//! + `lag`. No new primitives. This is the "Virus on Network" shape from krABMaga, in the
//! synchronous / field-and-network subset WaSim covers natively.
//!
//! Model: deterministic threshold-SI (susceptible → infected, no recovery) on 6 nodes.
//! Graph (undirected): a path 1–2–3–4–5–6 PLUS a chord 2–5.
//! Rule per step: node i becomes/stays infected iff it was infected OR any neighbor was
//! infected last step. Neighbor aggregation is `exposure[i] = Σ_j W[i,j]·infected_prev[j]`,
//! a matrix–vector product expressed as an elementwise product over the [Node, NodeB] grid
//! reduced over axis 2 (the neighbor axis) — the axis-selective reduction shipped in
//! axis_reduction_v2.
//!
//! Because the dynamics are deterministic, every value is a BFS frontier from the seed (node 1):
//!   step:            1    2       3          4     5   6
//!   newly infected:  {1}  {2}     {3,5}      {4,6} -   -
//!   total infected:   1    2       4          6    6   6
//! Node 5 is infected at step 3 *via the chord* (2→5); on the bare path it is distance 4 from
//! node 1 and would not infect until step 5. Asserting node 5 at step 3 proves the reduction is
//! actually using the graph's edge structure, not linear diffusion.
//!
//! Two dimensions of equal size (Node, NodeB) give the adjacency matrix two distinct axes of the
//! same node set; `inf_prev` (over Node) is reindexed onto NodeB by a `vector_map`+`index` gather
//! — align-by-name matrix algebra with no transpose primitive.

use wasim_engine::{parse_v2, run_v2, ModelGraphV2, RunConfig, SimulationResults};

// Canonical axis order is dimension ids sorted: "Node" < "NodeB", so Node = axis 1, NodeB = axis 2.
const MODEL: &str = r#"{"wasim_version":"0.9.9","simulation_settings":{"duration":{"value":6,"unit":"d"},"timestep":{"value":1,"unit":"d"},"n_realizations":1,"seed":1},"dimensions":[{"id":"Node","name":"Node","size":6},{"id":"NodeB","name":"NodeB","size":6}],"elements":[{"id":"W","name":"W","primitive":"node","value_rule":"expression","expression":{"ast":{"op":"vector_map","over":"Node","body":{"op":"vector_map","over":"NodeB","body":{"op":"or","left":{"op":"or","left":{"op":"eq","left":{"op":"subtract","left":{"op":"index_ref","depth":1},"right":{"op":"index_ref","depth":0}},"right":{"op":"literal","value":1}},"right":{"op":"eq","left":{"op":"subtract","left":{"op":"index_ref","depth":0},"right":{"op":"index_ref","depth":1}},"right":{"op":"literal","value":1}}},"right":{"op":"or","left":{"op":"and","left":{"op":"eq","left":{"op":"index_ref","depth":1},"right":{"op":"literal","value":2}},"right":{"op":"eq","left":{"op":"index_ref","depth":0},"right":{"op":"literal","value":5}}},"right":{"op":"and","left":{"op":"eq","left":{"op":"index_ref","depth":1},"right":{"op":"literal","value":5}},"right":{"op":"eq","left":{"op":"index_ref","depth":0},"right":{"op":"literal","value":2}}}}}}}},"outputs":[{"name":"W","unit":"1","dimensions":["Node","NodeB"]}]},{"id":"seed","name":"seed","primitive":"node","value_rule":"expression","expression":{"ast":{"op":"eq","left":{"op":"vector_map","over":"Node","body":{"op":"index_ref","depth":0}},"right":{"op":"literal","value":1}}},"outputs":[{"name":"seed","unit":"1","dimensions":["Node"]}]},{"id":"inf_prev","name":"inf_prev","primitive":"node","value_rule":"lag","input":"infected","initial":{"value":0,"unit":"1"},"outputs":[{"name":"inf_prev","unit":"1","dimensions":["Node"]}]},{"id":"inf_prev_B","name":"inf_prev_B","primitive":"node","value_rule":"expression","inputs":["inf_prev"],"expression":{"ast":{"op":"vector_map","over":"NodeB","body":{"op":"index","array":{"op":"ref","element_id":"inf_prev"},"indices":[{"op":"index_ref","depth":0}]}}},"outputs":[{"name":"inf_prev_B","unit":"1","dimensions":["NodeB"]}]},{"id":"wprod","name":"wprod","primitive":"node","value_rule":"expression","inputs":["W","inf_prev_B"],"expression":{"ast":{"op":"multiply","left":{"op":"ref","element_id":"W"},"right":{"op":"ref","element_id":"inf_prev_B"}}},"outputs":[{"name":"wprod","unit":"1","dimensions":["Node","NodeB"]}]},{"id":"exposure","name":"exposure","primitive":"node","value_rule":"expression","inputs":["wprod"],"expression":{"ast":{"op":"call","fn":"sum_array","args":[{"op":"ref","element_id":"wprod"},{"op":"literal","value":2}]}},"outputs":[{"name":"exposure","unit":"1","dimensions":["Node"]}]},{"id":"infected","name":"infected","primitive":"node","value_rule":"expression","inputs":["seed","inf_prev","exposure"],"expression":{"ast":{"op":"gt","left":{"op":"add","left":{"op":"ref","element_id":"seed"},"right":{"op":"add","left":{"op":"ref","element_id":"inf_prev"},"right":{"op":"gt","left":{"op":"ref","element_id":"exposure"},"right":{"op":"literal","value":0}}}},"right":{"op":"literal","value":0}}},"outputs":[{"name":"infected","unit":"1","dimensions":["Node"]}],"save_results":{"final_value":true,"time_history":true}},{"id":"total","name":"total","primitive":"node","value_rule":"expression","inputs":["infected"],"expression":{"ast":{"op":"call","fn":"sum_array","args":[{"op":"ref","element_id":"infected"}]}},"save_results":{"final_value":true,"time_history":true}}]}"#;

fn hist(r: &SimulationResults, id: &str) -> Vec<f64> {
    r.elements
        .get(id)
        .and_then(|e| e.time_history.as_ref())
        .map(|h| h.mean.clone())
        .unwrap_or_else(|| panic!("missing history for {id}"))
}

#[test]
fn contagion_spreads_along_graph_edges() {
    let m = parse_v2(MODEL).expect("parse");
    let g = ModelGraphV2::build(&m).expect("graph");
    let r = run_v2(&m, &g, &RunConfig::default()).expect("run");

    // Total infected per step is the BFS frontier size from node 1: 1, 2, 4, 6, then saturated.
    let total = hist(&r, "total");
    println!("total infected per step = {total:?}  (expected [1,2,4,6,6,6])");
    assert_eq!(total, vec![1.0, 2.0, 4.0, 6.0, 6.0, 6.0], "contagion must follow the BFS frontier");

    // Node 1 is the seed: infected from step 1 on.
    assert_eq!(hist(&r, "infected#1"), vec![1.0, 1.0, 1.0, 1.0, 1.0, 1.0], "seed node stays infected");

    // Node 5 infects at step 3 THROUGH THE CHORD (2→5). On the bare path it is 4 hops from node 1
    // and would not infect until step 5 — so [0,0,1,...] proves the edge structure is used.
    assert_eq!(hist(&r, "infected#5"), vec![0.0, 0.0, 1.0, 1.0, 1.0, 1.0], "node 5 infects via the 2-5 chord at step 3");

    // Node 6 is the far end of the path: 5 hops on the path, but 3 hops via the chord (1-2-5-6),
    // so it infects at step 4.
    assert_eq!(hist(&r, "infected#6"), vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], "node 6 infects at step 4");

    // Everyone is infected by the end.
    assert_eq!(r.elements["total"].final_values[0], 6.0, "all nodes infected at termination");
}
