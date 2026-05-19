use std::collections::{HashMap, HashSet};
use petgraph::algo::{kosaraju_scc, toposort};
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;

use crate::error::EngineError;
use crate::model::{ElementKind, WasimModel};

pub struct ModelGraph {
    pub topo_order: Vec<String>,
    pub element_index: HashMap<String, usize>,
    /// Element IDs excluded from topo_order because they participate in a dependency cycle.
    pub skipped_cycle_ids: Vec<String>,
}

impl ModelGraph {
    pub fn build(model: &WasimModel) -> Result<Self, EngineError> {
        let mut graph: DiGraph<&str, ()> = DiGraph::new();
        let mut node_map: HashMap<&str, NodeIndex> = HashMap::new();

        for elem in &model.elements {
            let idx = graph.add_node(elem.id.as_str());
            node_map.insert(elem.id.as_str(), idx);
        }

        // Add edges: dependency → dependent
        // Accumulators expose their stored state as output — no topo edges from their
        // rate inputs. Rate inputs are evaluated in a second pass after all outputs are set.
        for elem in &model.elements {
            let to = node_map[elem.id.as_str()];
            let deps = element_deps(&elem.kind);
            for dep_id in deps {
                // Skip self-references (transpiler artifact: element lists itself as a dep,
                // meaning it uses its own previous-step value — handled in the evaluator).
                // Skip dangling references (time properties named as element IDs, etc.) —
                // these surface as ElementNotFound during eval if actually hit.
                if dep_id != elem.id.as_str() {
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
            .map(|(i, e)| (e.id.clone(), i))
            .collect();

        // Fast path: no cycles.
        if let Ok(sorted) = toposort(&graph, None) {
            let topo_order = sorted.iter().map(|&idx| graph[idx].to_string()).collect();
            return Ok(Self { topo_order, element_index, skipped_cycle_ids: vec![] });
        }

        // Slow path: find all nodes in non-trivial SCCs and exclude them.
        // kosaraju_scc returns SCCs in reverse topological order; components with >1 node
        // are cycles. Single-node SCCs with a self-loop also cycle (e.g. `x = f(x)`).
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

        let skipped_cycle_ids: Vec<String> = cyclic
            .iter()
            .map(|&n| graph[n].to_string())
            .collect();

        eprintln!(
            "warn: {} element(s) skipped due to dependency cycles: {}",
            skipped_cycle_ids.len(),
            skipped_cycle_ids.join(", ")
        );

        // Build a pruned graph without the cyclic nodes, then toposort it.
        let mut pruned: DiGraph<&str, ()> = DiGraph::new();
        let mut pruned_map: HashMap<&str, NodeIndex> = HashMap::new();
        for elem in &model.elements {
            let orig = node_map[elem.id.as_str()];
            if !cyclic.contains(&orig) {
                let idx = pruned.add_node(elem.id.as_str());
                pruned_map.insert(elem.id.as_str(), idx);
            }
        }
        for elem in &model.elements {
            let orig_to = node_map[elem.id.as_str()];
            if cyclic.contains(&orig_to) { continue; }
            let deps = element_deps(&elem.kind);
            for dep_id in deps {
                if dep_id == elem.id.as_str() { continue; }
                let Some(&orig_from) = node_map.get(dep_id) else { continue };
                if cyclic.contains(&orig_from) { continue; }
                if let (Some(&pf), Some(&pt)) = (pruned_map.get(dep_id), pruned_map.get(elem.id.as_str())) {
                    pruned.add_edge(pf, pt, ());
                }
            }
        }

        let sorted = toposort(&pruned, None).map_err(|cycle| {
            let id = pruned[cycle.node_id()].to_string();
            EngineError::CycleDetected(id)
        })?;

        let topo_order = sorted.iter().map(|&idx| pruned[idx].to_string()).collect();
        Ok(Self { topo_order, element_index, skipped_cycle_ids })
    }
}

/// Returns the list of element IDs that `kind` depends on for its current-timestep output.
/// Accumulator rate inputs are intentionally excluded — they are evaluated in a separate pass.
fn element_deps(kind: &ElementKind) -> Vec<&str> {
    match kind {
        ElementKind::Constant { .. } => vec![],
        ElementKind::RandomVariable { .. } => vec![],
        ElementKind::Timeseries { .. } => vec![],
        ElementKind::Lookup { .. } => vec![],
        // Accumulators provide their stored state — no incoming topo edges.
        ElementKind::Accumulator { .. } => vec![],
        ElementKind::Expression { inputs, .. } => inputs.iter().map(|s| s.as_str()).collect(),
        ElementKind::Script { inputs, .. } => inputs.iter().map(|s| s.as_str()).collect(),
        ElementKind::Array { inputs, .. } => inputs.iter().map(|s| s.as_str()).collect(),
        ElementKind::StochasticProcess { .. } => vec![],
        // A delay outputs a buffered lagged value; its step-t output does not depend on
        // its input's step-t value. Like accumulators, it carries forward state and takes
        // no incoming topo edge — this lets feedback loops through a delay resolve.
        ElementKind::Delay { .. } => vec![],
    }
}
