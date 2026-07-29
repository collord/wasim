//! Schema drift guard.
//!
//! The JSON schema (`schema/wasim-schema-v2.json`) is hand-maintained, separate from the Rust
//! wire types. This test fails when the engine gains a JSON construct the schema doesn't declare —
//! specifically a new `AstNode` op (`$defs.ast_node`) or `DistributionKind` family
//! (`$defs.distribution`) with no matching schema branch. It exists because exactly that drift
//! happened once: engine work on cloud containers (no schema symlink) added 9 ast ops + 2
//! distribution families the schema never documented. See CHANGELOG 0.9.8.
//!
//! Direction guarded: engine ⊆ schema (every construct the engine emits must be declared). The
//! reverse (schema-only, unimplemented ops) is not an error here. Variant names come from the
//! enums themselves via `strum::VariantNames` with `serialize_all = "snake_case"`, which matches
//! serde's `rename_all = "snake_case"` wire names (verified for digit variants like
//! `submodel_stat2`, `run_stat2`, `triangular1090`) — so the Rust side can't silently drift from a
//! hardcoded list.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use strum::VariantNames;
use wasim_engine::model::{AstNode, DistributionKind};

fn schema_path() -> PathBuf {
    // Mirrors integration.rs::manual_examples_dir — the schema is in-repo at ../schema/ relative
    // to the engine crate, always present, so this test is unconditional (no skip-if-absent).
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("schema/wasim-schema-v2.json")
}

fn schema_json() -> serde_json::Value {
    let path = schema_path();
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read schema at {}: {e}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("schema at {} is not valid JSON: {e}", path.display()))
}

/// Op strings the schema declares under `$defs.ast_node.oneOf[*].properties.op`, unioning both
/// `const` (single-op branches) and `enum` (grouped branches like the binary/unary ops).
fn schema_ast_ops(schema: &serde_json::Value) -> BTreeSet<String> {
    let mut ops = BTreeSet::new();
    let branches = schema["$defs"]["ast_node"]["oneOf"]
        .as_array()
        .expect("$defs.ast_node.oneOf must be an array");
    for b in branches {
        let op = &b["properties"]["op"];
        if let Some(c) = op["const"].as_str() {
            ops.insert(c.to_string());
        }
        if let Some(list) = op["enum"].as_array() {
            for v in list {
                if let Some(s) = v.as_str() {
                    ops.insert(s.to_string());
                }
            }
        }
    }
    ops
}

/// Family strings the schema declares under `$defs.distribution.oneOf[*].properties.family.const`.
fn schema_distribution_families(schema: &serde_json::Value) -> BTreeSet<String> {
    let mut fams = BTreeSet::new();
    let branches = schema["$defs"]["distribution"]["oneOf"]
        .as_array()
        .expect("$defs.distribution.oneOf must be an array");
    for b in branches {
        if let Some(c) = b["properties"]["family"]["const"].as_str() {
            fams.insert(c.to_string());
        }
    }
    fams
}

fn missing(engine: &[&'static str], schema: &BTreeSet<String>) -> Vec<String> {
    engine
        .iter()
        .filter(|v| !schema.contains(**v))
        .map(|v| v.to_string())
        .collect()
}

#[test]
fn ast_ops_are_all_declared_in_schema() {
    let schema = schema_json();
    let declared = schema_ast_ops(&schema);
    let undeclared = missing(AstNode::VARIANTS, &declared);
    assert!(
        undeclared.is_empty(),
        "engine AstNode ops absent from schema $defs.ast_node.oneOf: {undeclared:?}\n\
         Add a schema branch for each (mirror an existing op branch), or the engine has gained an \
         op the schema doesn't document. Declared ops: {declared:?}"
    );
}

#[test]
fn distribution_families_are_all_declared_in_schema() {
    let schema = schema_json();
    let declared = schema_distribution_families(&schema);
    let undeclared = missing(DistributionKind::VARIANTS, &declared);
    assert!(
        undeclared.is_empty(),
        "engine DistributionKind families absent from schema $defs.distribution.oneOf: \
         {undeclared:?}\nAdd a schema branch for each. Declared families: {declared:?}"
    );
}
