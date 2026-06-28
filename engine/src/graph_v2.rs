//! Dependency graph over the v2 primitive model.
//!
//! Mirrors `crate::graph` but derives edges from v2 primitives. For M1 the cycle
//! policy is identical to v1 (warn + skip cyclic elements) so the corpus regresses
//! cleanly; the version-discriminated reject/implicit-lag policy (semantics §9) is a
//! later, separately-tested change.

use std::collections::{HashMap, HashSet};

use petgraph::algo::{kosaraju_scc, toposort};
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;

use crate::error::EngineError;
use crate::model_v2::{Element, Model, NodeRule, Primitive};

pub struct ModelGraphV2 {
    pub topo_order: Vec<String>,
    pub element_index: HashMap<String, usize>,
    pub skipped_cycle_ids: Vec<String>,
}

impl ModelGraphV2 {
    pub fn build(model: &Model) -> Result<Self, EngineError> {
        let mut graph: DiGraph<&str, ()> = DiGraph::new();
        let mut node_map: HashMap<&str, NodeIndex> = HashMap::new();

        for elem in &model.elements {
            let idx = graph.add_node(elem.id());
            node_map.insert(elem.id(), idx);
        }

        for elem in &model.elements {
            let to = node_map[elem.id()];
            for dep_id in element_deps(elem) {
                if dep_id != elem.id() {
                    if let Some(&from) = node_map.get(dep_id) {
                        graph.add_edge(from, to, ());
                    }
                }
            }
        }

        let element_index: HashMap<String, usize> = model
            .elements
            .iter()
            .enumerate()
            .map(|(i, e)| (e.id().to_string(), i))
            .collect();

        if let Ok(sorted) = toposort(&graph, None) {
            let topo_order = sorted.iter().map(|&idx| graph[idx].to_string()).collect();
            return Ok(Self { topo_order, element_index, skipped_cycle_ids: vec![] });
        }

        // Slow path: exclude nodes participating in cycles (matches v1 graph behavior).
        let self_loop_nodes: HashSet<NodeIndex> = graph
            .edge_references()
            .filter(|e| e.source() == e.target())
            .map(|e| e.source())
            .collect();

        let sccs = kosaraju_scc(&graph);
        let mut cyclic: HashSet<NodeIndex> = HashSet::new();
        for scc in &sccs {
            if scc.len() > 1 || scc.iter().any(|n| self_loop_nodes.contains(n)) {
                for &n in scc {
                    cyclic.insert(n);
                }
            }
        }

        let skipped_cycle_ids: Vec<String> = cyclic.iter().map(|&n| graph[n].to_string()).collect();
        eprintln!(
            "warn: {} element(s) skipped due to dependency cycles: {}",
            skipped_cycle_ids.len(),
            skipped_cycle_ids.join(", ")
        );

        let mut pruned: DiGraph<&str, ()> = DiGraph::new();
        let mut pruned_map: HashMap<&str, NodeIndex> = HashMap::new();
        for elem in &model.elements {
            let orig = node_map[elem.id()];
            if !cyclic.contains(&orig) {
                let idx = pruned.add_node(elem.id());
                pruned_map.insert(elem.id(), idx);
            }
        }
        for elem in &model.elements {
            let orig_to = node_map[elem.id()];
            if cyclic.contains(&orig_to) {
                continue;
            }
            for dep_id in element_deps(elem) {
                if dep_id == elem.id() {
                    continue;
                }
                let Some(&orig_from) = node_map.get(dep_id) else { continue };
                if cyclic.contains(&orig_from) {
                    continue;
                }
                if let (Some(&pf), Some(&pt)) = (pruned_map.get(dep_id), pruned_map.get(elem.id())) {
                    pruned.add_edge(pf, pt, ());
                }
            }
        }

        let sorted = toposort(&pruned, None).map_err(|cycle| {
            EngineError::CycleDetected(pruned[cycle.node_id()].to_string())
        })?;
        let topo_order = sorted.iter().map(|&idx| pruned[idx].to_string()).collect();
        Ok(Self { topo_order, element_index, skipped_cycle_ids })
    }
}

/// Element ids this element depends on for its current-timestep output.
///
/// Stocks take no incoming edge (their rate inputs evaluate in a second pass) and
/// `lag` nodes are back-edges (resolved from the previous step) — both match v1.
fn element_deps(elem: &Element) -> Vec<&str> {
    match &elem.primitive {
        Primitive::Stock(_) => vec![],
        Primitive::Node(n) => match &n.rule {
            // Back-edge: lag reads the previous timestep, breaking cycles.
            NodeRule::Lag { .. } => vec![],
            // Everything else evaluates from current-step inputs declared on the base.
            // (fixed/sample/process/lookup/series carry no inputs; expression carries them.)
            _ => elem.base.inputs.iter().map(|s| s.as_str()).collect(),
        },
        // link/event/gate/cell are not produced by the v1 normalizer; M2+.
        _ => vec![],
    }
}
