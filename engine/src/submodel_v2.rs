//! SubModel execution (§12). A `kind: "submodel"` container is a nested simulation:
//! it runs its own realization loop under its own settings, and parent expressions read a
//! Monte-Carlo statistic of one of its outputs via the `submodel_stat` AST node.
//!
//! This module runs a pre-pass over the parent model: for every `(submodel_id, output)`
//! referenced by a `submodel_stat`, it extracts the submodel's interior into a runnable
//! sub-`Model`, runs it, and returns the output's per-realization final values. The
//! `submodel_stat` eval arm (eval.rs) then reduces those samples on demand.

use std::collections::{HashMap, HashSet};

use crate::error::EngineError;
use crate::model::AstNode;
use crate::model_v2::{ContainerKind, Element, Model, NodeRule, Primitive};
use crate::{ModelGraphV2, RunConfig};

/// Walk every element expression AST and collect the `(submodel_id, output)` pairs any
/// `submodel_stat` node references.
pub fn collect_submodel_refs(model: &Model) -> HashSet<(String, String)> {
    let mut refs = HashSet::new();
    for e in &model.elements {
        if let Primitive::Node(n) = &e.primitive {
            if let crate::model_v2::NodeRule::Expression(ef) = &n.rule {
                collect_ast(&ef.ast, &mut refs);
            }
        }
    }
    refs
}

fn collect_ast(node: &AstNode, out: &mut HashSet<(String, String)>) {
    match node {
        AstNode::SubmodelStat { submodel_id, output, arg, .. } => {
            out.insert((submodel_id.clone(), output.clone()));
            if let Some(a) = arg {
                collect_ast(a, out);
            }
        }
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
            collect_ast(left, out);
            collect_ast(right, out);
        }
        AstNode::Neg { operand } | AstNode::Not { operand } => collect_ast(operand, out),
        AstNode::Call { args, .. } | AstNode::ExternCall { args, .. } => {
            args.iter().for_each(|a| collect_ast(a, out));
        }
        AstNode::If { cond, then, else_ } => {
            collect_ast(cond, out);
            collect_ast(then, out);
            collect_ast(else_, out);
        }
        AstNode::LookupCall { input, input2, .. } => {
            collect_ast(input, out);
            if let Some(i2) = input2 {
                collect_ast(i2, out);
            }
        }
        AstNode::VectorMap { body, .. } => collect_ast(body, out),
        AstNode::Index { array, indices } => {
            collect_ast(array, out);
            indices.iter().for_each(|i| collect_ast(i, out));
        }
        AstNode::Array { elements } => elements.iter().for_each(|e| collect_ast(e, out)),
        AstNode::Literal { .. }
        | AstNode::Ref { .. }
        | AstNode::TimeRef { .. }
        | AstNode::IndexRef { .. } => {}
    }
}

/// Is `element_container` inside `submodel_id` (transitively via the container parent chain)?
fn is_under(
    element_container: Option<&str>,
    submodel_id: &str,
    parent_of: &HashMap<&str, Option<&str>>,
) -> bool {
    let mut cur = element_container;
    let mut seen = HashSet::new();
    while let Some(c) = cur {
        if !seen.insert(c) {
            break; // cycle guard
        }
        if c == submodel_id {
            return true;
        }
        cur = parent_of.get(c).copied().flatten();
    }
    false
}

/// AST element references (for pulling a `from` driver's dependency closure into a submodel).
fn ast_refs(node: &AstNode, out: &mut HashSet<String>) {
    match node {
        AstNode::Ref { element_id, .. } | AstNode::LookupCall { element_id, .. } => {
            out.insert(element_id.clone());
        }
        AstNode::Add { left, right } | AstNode::Subtract { left, right }
        | AstNode::Multiply { left, right } | AstNode::Divide { left, right }
        | AstNode::Power { left, right } | AstNode::Lt { left, right }
        | AstNode::Gt { left, right } | AstNode::Lte { left, right }
        | AstNode::Gte { left, right } | AstNode::Eq { left, right }
        | AstNode::Neq { left, right } | AstNode::And { left, right }
        | AstNode::Or { left, right } => { ast_refs(left, out); ast_refs(right, out); }
        AstNode::Neg { operand } | AstNode::Not { operand } => ast_refs(operand, out),
        AstNode::Call { args, .. } | AstNode::ExternCall { args, .. } => {
            args.iter().for_each(|a| ast_refs(a, out));
        }
        AstNode::If { cond, then, else_ } => {
            ast_refs(cond, out); ast_refs(then, out); ast_refs(else_, out);
        }
        AstNode::VectorMap { body, .. } => ast_refs(body, out),
        AstNode::Index { array, indices } => {
            ast_refs(array, out); indices.iter().for_each(|i| ast_refs(i, out));
        }
        AstNode::Array { elements } => elements.iter().for_each(|e| ast_refs(e, out)),
        AstNode::SubmodelStat { arg, .. } => { if let Some(a) = arg { ast_refs(a, out); } }
        AstNode::Literal { .. } | AstNode::TimeRef { .. } | AstNode::IndexRef { .. } => {}
    }
}

/// The AST-reference dependency closure of `start` among elements not already interior.
fn driver_closure(model: &Model, start: &str, interior: &HashSet<String>) -> Vec<Element> {
    let by_id: HashMap<&str, &Element> = model.elements.iter().map(|e| (e.id(), e)).collect();
    let mut seen: HashSet<String> = HashSet::new();
    let mut stack = vec![start.to_string()];
    let mut out = Vec::new();
    while let Some(id) = stack.pop() {
        if seen.contains(&id) || interior.contains(&id) {
            continue;
        }
        seen.insert(id.clone());
        let Some(e) = by_id.get(id.as_str()) else { continue };
        out.push((*e).clone());
        // Follow the element's own dependencies (expression AST + declared inputs).
        if let Primitive::Node(n) = &e.primitive {
            if let NodeRule::Expression(ef) = &n.rule {
                let mut r = HashSet::new();
                ast_refs(&ef.ast, &mut r);
                stack.extend(r);
            }
        }
        stack.extend(e.base.inputs.iter().cloned());
    }
    out
}

/// Extract a submodel container's interior into a fresh, runnable `Model`. Interior =
/// every element transitively under the container. Settings come from the container's own
/// `simulation_settings`, falling back to the parent model's.
///
/// Interface-input driving (§12): each `interface.inputs` binding `{input, from}` pins the
/// interior `input` element to the parent `from` element's value — for optimization, that
/// parent element is the search variable, so the submodel responds to the candidate.
fn extract_submodel(model: &Model, submodel_id: &str) -> Option<Model> {
    let container = model.containers.iter().find(|c| c.id == submodel_id)?;
    if container.kind != ContainerKind::Submodel {
        return None;
    }
    let parent_of: HashMap<&str, Option<&str>> = model
        .containers
        .iter()
        .map(|c| (c.id.as_str(), c.parent.as_deref()))
        .collect();

    let elements: Vec<Element> = model
        .elements
        .iter()
        .filter(|e| is_under(e.base.container.as_deref(), submodel_id, &parent_of))
        .cloned()
        .collect();

    let interior_ids: HashSet<String> = elements.iter().map(|e| e.id().to_string()).collect();
    // Dynamic optimization (§13a): a submodel-scoped optimization is re-solved per outer step,
    // so each variable becomes a per-timestep series — force its time history on (variables are
    // fixed scalars, which otherwise default to history-off).
    let opt_var_ids: HashSet<String> = container
        .optimization
        .as_ref()
        .map(|o| o.variables.iter().map(|v| v.element_id.clone()).collect())
        .unwrap_or_default();
    // Interior containers: the submodel itself + any container whose element(s) are interior.
    let containers = model
        .containers
        .iter()
        .filter(|c| c.id == submodel_id || is_under(Some(c.id.as_str()), submodel_id, &parent_of))
        .cloned()
        .collect();

    let settings = container
        .simulation_settings
        .clone()
        .unwrap_or_else(|| model.simulation_settings.clone());

    // Interface-input driving (§12): for each `{input, from}` binding with a resolvable driver,
    // pull the parent `from` element AND its dependency closure into the submodel (re-containered
    // so they run), then make the interior `input` id an expression that refs `from`. Copying
    // the driver's rule this way handles every driver kind uniformly — a fixed scalar, a
    // time-varying expression (e.g. `10 + 5·cos(2π·elapsed/T)`), a series read over the
    // submodel's own clock, or a per-realization sample. Bindings with `from: null`
    // (engine/dashboard-supplied) contribute nothing. The `input` id is accepted as-is: an
    // existing interior element is replaced by the alias; a synthesized boundary-port id is
    // injected, so interior references to it resolve to the driver.
    let mut extra: Vec<Element> = Vec::new();        // pulled-in driver closure elements
    let mut aliases: HashMap<String, String> = HashMap::new(); // input id -> from id (ref alias)
    let mut pulled: HashSet<String> = HashSet::new();
    if let Some(iface) = &container.interface {
        for binding in &iface.inputs {
            let Some(from) = &binding.from else { continue };
            if binding.input == *from {
                continue; // consumer already IS the driver; nothing to wire
            }
            for e in driver_closure(model, from, &interior_ids) {
                if pulled.insert(e.id().to_string()) {
                    extra.push(e);
                }
            }
            aliases.insert(binding.input.clone(), from.clone());
        }
    }
    // Re-container the pulled driver elements into the submodel so they're interior + run.
    for e in &mut extra {
        e.base.container = Some(submodel_id.to_string());
    }

    // Assemble the submodel's element list:
    // interior elements (interface-input consumers replaced by an alias-ref to their driver;
    // others keep their inputs, dropping references outside the submodel) + pulled driver
    // closure + injected alias elements for boundary-port inputs with no interior element.
    let mut out: Vec<Element> = elements
        .into_iter()
        .map(|mut e| {
            if let Some(from) = aliases.get(e.id()) {
                if let Primitive::Node(n) = &mut e.primitive {
                    n.rule = alias_ref(from);
                    e.base.inputs = vec![from.clone()];
                }
            } else {
                e.base.inputs.retain(|i| interior_ids.contains(i) || pulled.contains(i));
            }
            e.base.save_results.final_value = Some(true);
            if opt_var_ids.contains(e.id()) {
                e.base.save_results.time_history = Some(true);
            }
            e
        })
        .collect();
    out.extend(extra);
    for (input, from) in &aliases {
        if !interior_ids.contains(input) {
            out.push(Element {
                base: crate::model_v2::ElementBase {
                    id: input.clone(),
                    name: input.rsplit('/').next().unwrap_or(input).to_string(),
                    container: Some(submodel_id.to_string()),
                    inputs: vec![from.clone()],
                    ..Default::default()
                },
                primitive: Primitive::Node(crate::model_v2::Node { rule: alias_ref(from) }),
            });
        }
    }
    if out.is_empty() {
        return None; // hollow submodel — nothing to run, even after driving
    }

    Some(Model {
        wasim_version: model.wasim_version.clone(),
        source: model.source.clone(),
        simulation_settings: settings,
        reporting_periods: Vec::new(),
        dimensions: model.dimensions.clone(),
        // Carry the submodel's own (dynamic) optimization into the child model so its step
        // loop re-solves it per timestep (§13a). The top-level study optimization never
        // reaches here — this is strictly the submodel-scoped spec.
        optimization: container.optimization.clone(),
        containers,
        elements: out,
        time_history_displays: Vec::new(),
        from_v1: false,
        // A submodel-scoped optimization is dynamic (per-timestep, §13a); mark it so the
        // submodel's engine_v2::run applies the per-step solve.
        dynamic_optimization: container.optimization.is_some(),
    })
}

/// An expression rule that simply reads element `from` — used to alias an interface-input
/// consumer to its driver.
fn alias_ref(from: &str) -> NodeRule {
    NodeRule::Expression(crate::model::ExpressionField {
        ast: AstNode::Ref { element_id: from.to_string(), output: "value".to_string() },
        display: None,
        source: Default::default(),
    })
}

/// Run each referenced submodel once and return the per-realization sample vector for each
/// `(submodel_id, output)`. Outputs that don't resolve to a runnable element are omitted
/// (the eval arm then degrades to 0.0). A submodel that fails to build/run is skipped with
/// a warning rather than failing the whole parent run.
pub fn run_submodels(
    model: &Model,
    config: &RunConfig,
) -> Result<HashMap<(String, String), Vec<f64>>, EngineError> {
    let refs = collect_submodel_refs(model);
    let mut out: HashMap<(String, String), Vec<f64>> = HashMap::new();
    if refs.is_empty() {
        return Ok(out);
    }

    // Group referenced outputs by submodel so each submodel runs once.
    let mut by_submodel: HashMap<&str, Vec<&str>> = HashMap::new();
    for (sub, output) in &refs {
        by_submodel.entry(sub.as_str()).or_default().push(output.as_str());
    }

    for (sub_id, outputs) in by_submodel {
        let Some(sub_model) = extract_submodel(model, sub_id) else {
            eprintln!("warn: submodel '{sub_id}' has no runnable interior; submodel_stat → 0.0");
            continue;
        };
        // The submodel runs its own realizations; the parent config only supplies a seed.
        let sub_config = RunConfig { seed: config.seed, ..RunConfig::default() };
        let graph = match ModelGraphV2::build(&sub_model) {
            Ok(g) => g,
            Err(e) => {
                eprintln!("warn: submodel '{sub_id}' graph build failed ({e:?}); submodel_stat → 0.0");
                continue;
            }
        };
        let results = match crate::engine_v2::run(&sub_model, &graph, &sub_config) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("warn: submodel '{sub_id}' run failed ({e:?}); submodel_stat → 0.0");
                continue;
            }
        };
        for output in outputs {
            if let Some(er) = results.elements.get(output) {
                if !er.final_values.is_empty() {
                    out.insert((sub_id.to_string(), output.to_string()), er.final_values.clone());
                }
            }
        }
    }
    Ok(out)
}

/// Run each submodel that carries a **dynamic** (per-timestep) optimization (§13a) over the
/// PARENT's timeline, so its interface drivers vary step-to-step and the per-step solve yields
/// a series. Returns each such submodel's full element results (keyed by element id) for the
/// parent to merge — notably the optimized variables' time histories. A submodel-scoped
/// optimization implies the submodel's own `duration: 0` is just the authoring default; the
/// dynamic run must span the outer clock (GoldSim re-solves it once per outer step).
pub fn run_dynamic_submodels(
    model: &Model,
    config: &RunConfig,
    parent_settings: &crate::model_v2::SimulationSettings,
) -> Result<HashMap<String, crate::engine::ElementResults>, EngineError> {
    let mut out: HashMap<String, crate::engine::ElementResults> = HashMap::new();
    for c in &model.containers {
        if c.kind != ContainerKind::Submodel || c.optimization.is_none() {
            continue;
        }
        let Some(mut sub_model) = extract_submodel(model, &c.id) else { continue };
        // Run over the parent's clock (duration + timestep), keeping the submodel's own MC count.
        sub_model.simulation_settings.duration = parent_settings.duration.clone();
        sub_model.simulation_settings.timestep = parent_settings.timestep.clone();
        let sub_config = RunConfig { seed: config.seed, ..RunConfig::default() };
        let graph = match ModelGraphV2::build(&sub_model) {
            Ok(g) => g,
            Err(e) => {
                eprintln!("warn: dynamic-opt submodel '{}' graph build failed ({e:?})", c.id);
                continue;
            }
        };
        match crate::engine_v2::run(&sub_model, &graph, &sub_config) {
            Ok(r) => {
                for (id, er) in r.elements {
                    out.insert(id, er);
                }
            }
            Err(e) => eprintln!("warn: dynamic-opt submodel '{}' run failed ({e:?})", c.id),
        }
    }
    Ok(out)
}
