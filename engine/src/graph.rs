use std::collections::HashMap;
use petgraph::algo::toposort;
use petgraph::graph::{DiGraph, NodeIndex};

use crate::error::EngineError;
use crate::model::{ElementKind, WasimModel};

pub struct ModelGraph {
    pub topo_order: Vec<String>,
    pub element_index: HashMap<String, usize>,
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

        let sorted = toposort(&graph, None).map_err(|cycle| {
            let id = graph[cycle.node_id()].to_string();
            EngineError::CycleDetected(id)
        })?;

        let topo_order: Vec<String> = sorted
            .iter()
            .map(|&idx| graph[idx].to_string())
            .collect();

        let element_index: HashMap<String, usize> = model
            .elements
            .iter()
            .enumerate()
            .map(|(i, e)| (e.id.clone(), i))
            .collect();

        Ok(Self { topo_order, element_index })
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
        ElementKind::Delay { input, .. } => vec![input.as_str()],
    }
}
