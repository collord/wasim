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
use crate::model_v2::{ContainerKind, Element, FixedValue, Model, NodeRule, Primitive};
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

/// Current scalar value of a fixed-value node, if `id` names one. Used to read the parent's
/// value of an interface-input driver.
fn fixed_scalar(model: &Model, id: &str) -> Option<f64> {
    model.elements.iter().find(|e| e.id() == id).and_then(|e| match &e.primitive {
        Primitive::Node(n) => match &n.rule {
            NodeRule::Fixed { value: FixedValue::Scalar(q), .. } => Some(q.value),
            _ => None,
        },
        _ => None,
    })
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
    if elements.is_empty() {
        return None; // hollow submodel — nothing to run
    }

    let interior_ids: HashSet<String> = elements.iter().map(|e| e.id().to_string()).collect();
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

    // Interface-input driving: for each `{input, from}` binding, pin the interior `input`
    // element to the current value of the parent `from` element. Bindings with `from: null`
    // (engine/dashboard-supplied inputs) are left at their authored value.
    let mut driven: HashMap<String, f64> = HashMap::new();
    if let Some(iface) = &container.interface {
        for binding in &iface.inputs {
            if !interior_ids.contains(&binding.input) {
                continue; // consumer isn't an interior element; nothing to override
            }
            if let Some(from) = &binding.from {
                if let Some(v) = fixed_scalar(model, from) {
                    driven.insert(binding.input.clone(), v);
                }
            }
        }
    }

    Some(Model {
        wasim_version: model.wasim_version.clone(),
        source: model.source.clone(),
        simulation_settings: settings,
        reporting_periods: Vec::new(),
        dimensions: model.dimensions.clone(),
        optimization: None,
        containers,
        // Drop any inputs[] that point outside the submodel — the interface would supply
        // them; without a wired interface they simply resolve to 0.0 (dangling policy).
        // Force-save every interior element's final value so a referenced interface output
        // is captured regardless of the emit side's save flags.
        elements: elements
            .into_iter()
            .map(|mut e| {
                // Pin a driven interface-input element to the parent's value (overrides its
                // placeholder rule with a fixed scalar).
                if let Some(&v) = driven.get(e.id()) {
                    if let Primitive::Node(n) = &mut e.primitive {
                        n.rule = NodeRule::Fixed {
                            value: FixedValue::Scalar(crate::model::Quantity {
                                value: v,
                                unit: "1".to_string(),
                                display_unit: None,
                            }),
                            editable: false,
                            bounds: None,
                        };
                    }
                }
                e.base.inputs.retain(|i| interior_ids.contains(i));
                e.base.save_results.final_value = Some(true);
                e
            })
            .collect(),
        time_history_displays: Vec::new(),
        from_v1: false,
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
