use crate::types::EdgeKind;
use crate::types::node::GraphNode;
use petgraph::graph::NodeIndex;
use std::collections::HashMap;
use std::collections::HashSet;
use super::context::AnalysisContext;
use super::types::{Tier, FindingKind, Finding};

// ─────────────────────────────────────────────────────────────────────────────
// Check 58: Tagged union / suggest sum type
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_tagged_union(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Look for classes/structs with a field named type/kind/tag/variant/discriminator
    // that is typed as string/int, and ctx.functions that branch on it.
    let tag_field_names: HashSet<&str> = [
        "type", "kind", "tag", "variant", "discriminator", "node_type",
        "type_", "_type", "event_type", "msg_type", "message_type",
    ].iter().copied().collect();

    let tag_switch_patterns: &[&str] = &[
        "type", "kind", "tag", "variant", "self.type", "self.kind",
        "self.tag", "self._type", ".type", ".kind", ".tag",
    ];

    // Check classes
    for idx in ctx.graph.graph.node_indices() {
        let node = &ctx.graph.graph[idx];
        let (name, fields) = match node {
            GraphNode::Class(d) => (&d.name, &d.fields),
            GraphNode::Struct(d) => (&d.name, &d.fields),
            _ => continue,
        };

        let tag_field = fields.iter().find(|f| {
            tag_field_names.contains(f.name.to_lowercase().as_str())
        });

        if let Some(tf) = tag_field {
            // Verify the tag field is a primitive type (string/int)
            let is_primitive_tag = tf.type_annotation.as_ref().map(|t| {
                let lower = t.to_lowercase();
                lower.contains("str") || lower.contains("int")
                    || lower.contains("i32") || lower.contains("u32")
                    || lower == "string" || lower == "number"
            }).unwrap_or(true); // If no type annotation, assume it could be

            if !is_primitive_tag {
                continue;
            }

            // Check if any method branches on this field (look at source for switch/match/if)
            let methods = ctx.get_children(idx);
            let has_branching = methods.iter().any(|(_, mn)| {
                if let Some(src) = mn.source_snippet() {
                    let lower = src.to_lowercase();
                    tag_switch_patterns.iter().any(|p| lower.contains(p))
                        && (lower.contains("match") || lower.contains("switch")
                            || lower.contains("if ") || lower.contains("case "))
                } else {
                    false
                }
            });

            // Also check free ctx.functions that take this type and branch
            if has_branching || !methods.is_empty() {
                // Only report if there's actual branching evidence
                if has_branching {
                    findings.push(Finding {
                        tier: Tier::High,
                        kind: FindingKind::SuggestSumType {
                            class_name: name.clone(),
                            tag_field: tf.name.clone(),
                        },
                        node_indices: vec![idx.index()],
                        description: format!(
                            "`{}::{}` tag with conditional branching — replace with enum/sum type.",
                            name, tf.name
                        ),
                    });
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 59: Class hierarchy → enum
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_hierarchy_to_enum(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    use petgraph::visit::EdgeRef;

    // Find base classes with 3+ subclasses where the subclasses have no fields of their own
    let mut base_to_children: HashMap<NodeIndex, Vec<(NodeIndex, String)>> = HashMap::new();

    for &(idx, node) in &ctx.classes {
        let _cd = if let GraphNode::Class(cd) = node { cd } else { continue };
        if let GraphNode::Class(cd) = node {
            // Find parent via INHERITS edge
            for edge in ctx.graph.graph.edges_directed(idx, petgraph::Direction::Outgoing) {
                if matches!(edge.weight(), EdgeKind::Inherits) {
                    base_to_children
                        .entry(edge.target())
                        .or_default()
                        .push((idx, cd.name.clone()));
                }
            }
        }
    }

    for (base_idx, children) in &base_to_children {
        if children.len() < 3 {
            continue;
        }

        let base_name = ctx.graph.graph[*base_idx].name().to_string();

        // Check if children are "leaf" classes — no fields, only method overrides
        let mut leaf_names = Vec::new();
        for (child_idx, child_name) in children {
            let child_node = &ctx.graph.graph[*child_idx];
            let has_own_fields = match child_node {
                GraphNode::Class(d) => !d.fields.is_empty(),
                _ => false,
            };

            let child_methods = ctx.get_children(*child_idx);
            let method_count = child_methods.iter()
                .filter(|(_, n)| matches!(n, GraphNode::Function(_)))
                .count();

            // Leaf: no fields, few methods (just overrides)
            if !has_own_fields && method_count <= 3 {
                leaf_names.push(child_name.clone());
            }
        }

        if leaf_names.len() >= 3 {
            findings.push(Finding {
                tier: Tier::Medium,
                kind: FindingKind::SuggestEnumFromHierarchy {
                    base_name: base_name.clone(),
                    leaf_names: leaf_names.clone(),
                },
                node_indices: vec![base_idx.index()],
                description: format!(
                    "`{}`: {} data-free leaf subclasses [{}] — model as enum/ADT.",
                    base_name, leaf_names.len(), leaf_names.join(", ")
                ),
            });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 60: Boolean blindness
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_boolean_blindness(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };

        // Find boolean parameters
        let bool_params: Vec<String> = func.args.iter()
            .zip(func.arg_types.iter().chain(std::iter::repeat(&None)))
            .filter(|(name, _)| *name != "self" && *name != "&self" && *name != "&mut self")
            .filter(|(name, type_opt)| {
                // Check type annotation
                if let Some(t) = type_opt {
                    let lower = t.to_lowercase();
                    return lower == "bool" || lower == "boolean";
                }
                // Heuristic: param name suggests boolean
                let lower = name.to_lowercase();
                lower.starts_with("is_") || lower.starts_with("has_")
                    || lower.starts_with("should_") || lower.starts_with("enable")
                    || lower.starts_with("use_") || lower.starts_with("allow_")
                    || lower.starts_with("force") || lower.starts_with("verbose")
                    || lower.starts_with("debug") || lower.starts_with("dry_run")
            })
            .map(|(name, _)| name.clone())
            .collect();

        if bool_params.len() >= 2 {
            findings.push(Finding {
                tier: Tier::Medium,
                kind: FindingKind::BooleanBlindness {
                    function_name: func.name.clone(),
                    bool_params: bool_params.clone(),
                },
                node_indices: vec![idx.index()],
                description: format!(
                    "`{}`: {} bool params [{}] — use descriptive enums.",
                    func.name, bool_params.len(), bool_params.join(", ")
                ),
            });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 61: Suggest newtype
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_suggest_newtype(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    let primitives: HashSet<&str> = [
        "str", "string", "String", "int", "i32", "i64", "u32", "u64",
        "float", "f32", "f64", "usize", "isize", "number", "double",
        "long",
    ].iter().copied().collect();

    // Check structs with exactly 1 field that is a primitive
    for idx in ctx.graph.graph.node_indices() {
        let node = &ctx.graph.graph[idx];
        let (name, fields) = match node {
            GraphNode::Struct(d) => (&d.name, &d.fields),
            GraphNode::Class(d) => (&d.name, &d.fields),
            _ => continue,
        };

        if fields.len() != 1 {
            continue;
        }

        let field = &fields[0];
        if let Some(ref type_ann) = field.type_annotation {
            let clean = type_ann.trim_start_matches('&').trim_start_matches("mut ").trim();
            if primitives.contains(clean) {
                // Check if the struct name suggests a domain concept
                // (skip generic names like "Wrapper", "Value")
                let lower = name.to_lowercase();
                if lower == "wrapper" || lower == "value" || lower == "box" {
                    continue;
                }

                // Check if struct has methods (if it does, it's already a proper newtype)
                let methods = ctx.get_children(idx);
                let method_count = methods.iter()
                    .filter(|(_, n)| matches!(n, GraphNode::Function(_)))
                    .count();

                if method_count == 0 {
                    findings.push(Finding {
                        tier: Tier::Low,
                        kind: FindingKind::SuggestNewtype {
                            type_name: name.clone(),
                            wrapped_field: field.name.clone(),
                            wrapped_type: type_ann.clone(),
                        },
                        node_indices: vec![idx.index()],
                        description: format!(
                            "`{}` wraps `{}` with no methods — add methods for newtype safety.",
                            name, type_ann
                        ),
                    });
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 62: Suggest sealed type
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_suggest_sealed_type(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Find interfaces/traits where ALL implementors are in the same file
    let abstractions: Vec<(NodeIndex, &GraphNode)> = ctx.graph
        .graph
        .node_indices()
        .filter_map(|idx| {
            let node = &ctx.graph.graph[idx];
            if matches!(node, GraphNode::Trait(_) | GraphNode::Interface(_) | GraphNode::Class(_)) {
                Some((idx, node))
            } else {
                None
            }
        })
        .collect();

    for (abs_idx, abs_node) in &abstractions {
        let implementors = ctx.graph.get_implementors(*abs_idx);
        if implementors.len() < 2 {
            continue;
        }

        // Get the file path of the interface
        let abs_path = match abs_node {
            GraphNode::Trait(d) => &d.path,
            GraphNode::Interface(d) => &d.path,
            GraphNode::Class(d) => &d.path,
            _ => continue,
        };

        // Check if all implementors are in the same file
        let all_same_file = implementors.iter().all(|(_, imp_node)| {
            match imp_node {
                GraphNode::Class(d) => d.path == *abs_path,
                GraphNode::Struct(d) => d.path == *abs_path,
                _ => false,
            }
        });

        if all_same_file {
            let imp_names: Vec<String> = implementors.iter()
                .map(|(_, n)| n.name().to_string())
                .collect();

            let file_name = abs_path.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            findings.push(Finding {
                tier: Tier::Medium,
                kind: FindingKind::SuggestSealedType {
                    interface_name: abs_node.name().to_string(),
                    implementor_names: imp_names.clone(),
                    file_name: file_name.clone(),
                },
                node_indices: vec![abs_idx.index()],
                description: format!(
                    "`{}` + implementors [{}] all in `{}` — closed sum type, seal or use enum.",
                    abs_node.name(), imp_names.join(", "), file_name
                ),
            });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 63: Large product type
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_large_product_type(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    const FIELD_THRESHOLD: usize = 10;

    let optional_patterns: &[&str] = &[
        "Option<", "Optional<", "option<", "Maybe",
        "None", "null", "nil", "undefined",
    ];

    for idx in ctx.graph.graph.node_indices() {
        let node = &ctx.graph.graph[idx];
        let (name, fields) = match node {
            GraphNode::Struct(d) => (&d.name, &d.fields),
            GraphNode::Class(d) => (&d.name, &d.fields),
            _ => continue,
        };

        if fields.len() < FIELD_THRESHOLD {
            continue;
        }

        let optional_count = fields.iter()
            .filter(|f| {
                f.type_annotation.as_ref().map(|t| {
                    optional_patterns.iter().any(|p| t.contains(p))
                }).unwrap_or(false)
            })
            .count();

        findings.push(Finding {
            tier: Tier::High,
            kind: FindingKind::LargeProductType {
                type_name: name.clone(),
                field_count: fields.len(),
                optional_count,
            },
            node_indices: vec![idx.index()],
            description: format!(
                "`{}`: {} fields ({} optional) — decompose into smaller structs or use a builder.",
                name, fields.len(), optional_count
            ),
        });
    }
}
