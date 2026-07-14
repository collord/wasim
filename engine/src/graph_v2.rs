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
use crate::model::{AstNode, QuantityOrFormula};
use crate::model_v2::{Element, GateNode, Model, NodeRule, Primitive};

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

        // Version-discriminated cycle policy (semantics §9): v2-native models are expected to
        // be cycle-free by construction, so reject; v1-imported models warn and skip the
        // cyclic elements (matching the v1 engine's behavior, preserving corpus equivalence).
        if !model.from_v1 {
            let id = skipped_cycle_ids.first().cloned().unwrap_or_default();
            return Err(EngineError::CycleDetected(id));
        }
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
            // Markov is autonomous (no input dependency).
            NodeRule::Markov { .. } => vec![],
            // These read the current-step value of `input`.
            NodeRule::Filter { input, .. }
            | NodeRule::Hysteresis { input, .. }
            | NodeRule::Convolution { input, .. } => {
                let mut d = vec![input.as_str()];
                d.extend(elem.base.inputs.iter().map(|s| s.as_str()));
                d
            }
            // Gate logic depends on the elements its tree references.
            NodeRule::GateLogic { root, .. } => {
                let mut d: Vec<&str> = elem.base.inputs.iter().map(|s| s.as_str()).collect();
                collect_gate_deps(root, &mut d);
                d
            }
            // Everything else evaluates from current-step inputs declared on the base.
            // (fixed/sample/process/lookup/series carry no inputs; expression carries them.)
            _ => elem.base.inputs.iter().map(|s| s.as_str()).collect(),
        },
        Primitive::Gate(g) => {
            let mut d: Vec<&str> = elem.base.inputs.iter().map(|s| s.as_str()).collect();
            collect_gate_deps(&g.root, &mut d);
            d
        }
        // link/event/cell are not produced by the v1 normalizer; M3/M4.
        _ => vec![],
    }
}

/// Collect element ids a gate tree depends on (reference/input leaves + AST refs in
/// `condition` leaves).
fn collect_gate_deps<'a>(node: &'a GateNode, out: &mut Vec<&'a str>) {
    match node {
        GateNode::And(c) | GateNode::Or(c) | GateNode::NVote { children: c, .. } => {
            for child in c {
                collect_gate_deps(child, out);
            }
        }
        GateNode::Not(child) => collect_gate_deps(child, out),
        GateNode::Reference(id) | GateNode::Input(id) => out.push(id.as_str()),
        GateNode::Condition(qof) => {
            if let QuantityOrFormula::Expression(ef) = qof {
                collect_ast_refs(&ef.ast, out);
            }
        }
    }
}

/// Collect element ids referenced by `ref` nodes in an AST. `lookup_call` targets are
/// static tables, not runtime deps, so only its input sub-expressions are walked.
fn collect_ast_refs<'a>(node: &'a AstNode, out: &mut Vec<&'a str>) {
    match node {
        AstNode::Ref { element_id, .. } => out.push(element_id.as_str()),
        AstNode::Add { left, right }
        | AstNode::Subtract { left, right }
        | AstNode::Multiply { left, right }
        | AstNode::Divide { left, right }
        | AstNode::Power { left, right }
        | AstNode::Lt { left, right }
        | AstNode::Gt { left, right }
        | AstNode::Lte { left, right }
        | AstNode::Gte { left, right }
        | AstNode::Eq { left, right }
        | AstNode::Neq { left, right }
        | AstNode::And { left, right }
        | AstNode::Or { left, right } => {
            collect_ast_refs(left, out);
            collect_ast_refs(right, out);
        }
        AstNode::Neg { operand } | AstNode::Not { operand } => collect_ast_refs(operand, out),
        AstNode::Call { args, .. } => {
            for a in args {
                collect_ast_refs(a, out);
            }
        }
        AstNode::If { cond, then, else_ } => {
            collect_ast_refs(cond, out);
            collect_ast_refs(then, out);
            collect_ast_refs(else_, out);
        }
        AstNode::LookupCall { input, input2, .. } => {
            collect_ast_refs(input, out);
            if let Some(i2) = input2 {
                collect_ast_refs(i2, out);
            }
        }
        AstNode::Array { elements } => {
            for e in elements {
                collect_ast_refs(e, out);
            }
        }
        // The expression depends on the submodel it reads a statistic from. The
        // `arg` sub-node may itself reference elements (e.g. a percentile index).
        AstNode::SubmodelStat { submodel_id, arg, .. } => {
            out.push(submodel_id.as_str());
            if let Some(a) = arg {
                collect_ast_refs(a, out);
            }
        }
        // Array-comprehension nodes (§15). Refs live in the sub-expressions; the
        // `over` dimension is an ordinal set, not an element, so it is not a dep.
        AstNode::VectorMap { body, .. } => collect_ast_refs(body, out),
        AstNode::Index { array, indices } => {
            collect_ast_refs(array, out);
            for i in indices {
                collect_ast_refs(i, out);
            }
        }
        AstNode::ExternCall { args, .. } => {
            for a in args {
                collect_ast_refs(a, out);
            }
        }
        AstNode::IndexRef { .. } | AstNode::Literal { .. } | AstNode::TimeRef { .. } => {}
    }
}
