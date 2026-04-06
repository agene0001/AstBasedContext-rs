use crate::types::EdgeKind;
use crate::types::node::GraphNode;
use petgraph::graph::NodeIndex;
use std::collections::HashSet;
use super::context::AnalysisContext;
use super::types::{Tier, FindingKind, Finding};

// ─────────────────────────────────────────────────────────────────────────────
// Check 23: Singleton — private constructor + static self-typed field + accessor
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_singleton(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    for &(idx, node) in &ctx.classes {
        let (class_name, fields) = match node {
            GraphNode::Class(c) => (c.name.clone(), &c.fields),
            _ => continue,
        };

        let children = ctx.get_children(idx);
        let methods: Vec<(NodeIndex, &GraphNode)> = children
            .iter()
            .filter(|(_, n)| matches!(n, GraphNode::Function(_)))
            .cloned()
            .collect();

        // Check 1: Has a private constructor?
        let has_private_constructor = methods.iter().any(|(_, n)| {
            if let GraphNode::Function(f) = n {
                let is_constructor = f.name == "__init__" || f.name == "new" || f.name == "constructor";
                let is_private = f.visibility.as_deref() == Some("private");
                is_constructor && is_private
            } else {
                false
            }
        });

        // Check 2: Has a static field of its own type (or named instance/shared/_instance)?
        let has_self_typed_field = fields.iter().any(|f| {
            let type_matches = f.type_annotation.as_ref().map_or(false, |t| {
                t.contains(&class_name)
            });
            let name_matches = f.name == "instance" || f.name == "_instance" || f.name == "shared"
                || f.name == "INSTANCE" || f.name == "_singleton";
            (type_matches || name_matches) && f.is_static
        });

        // Check 3: Has a static accessor method (getInstance, instance, shared)?
        let has_static_accessor = methods.iter().any(|(_, n)| {
            if let GraphNode::Function(f) = n {
                let name_matches = f.name == "getInstance" || f.name == "get_instance"
                    || f.name == "instance" || f.name == "shared";
                let is_static = f.is_static;
                let returns_self = f.return_type.as_ref().map_or(false, |t| t.contains(&class_name));
                name_matches || (is_static && returns_self)
            } else {
                false
            }
        });

        // Need at least 2 of 3 signals to flag
        let signals = [has_private_constructor, has_self_typed_field, has_static_accessor];
        let signal_count = signals.iter().filter(|&&s| s).count();

        if signal_count >= 2 {
            findings.push(Finding {
                tier: Tier::Medium,
                kind: FindingKind::DetectedSingleton {
                    class_name: class_name.clone(),
                },
                node_indices: vec![idx.index()],
                description: format!(
                    "Class `{}` appears to implement the singleton pattern{}{}{}.",
                    class_name,
                    if has_private_constructor { " (private constructor)" } else { "" },
                    if has_self_typed_field { " (static self-typed field)" } else { "" },
                    if has_static_accessor { " (static accessor)" } else { "" },
                ),
            });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 24: Adapter — wraps a different type, implements an interface
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_adapter(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Collect all interface/trait names for quick lookup
    let mut interface_names: HashSet<String> = HashSet::new();
    for idx in ctx.graph.graph.node_indices() {
        match &ctx.graph.graph[idx] {
            GraphNode::Trait(t) => { interface_names.insert(t.name.clone()); }
            GraphNode::Interface(i) => { interface_names.insert(i.name.clone()); }
            _ => {}
        }
    }

    for idx in ctx.graph.graph.node_indices() {
        let (class_name, fields) = match &ctx.graph.graph[idx] {
            GraphNode::Class(c) => (c.name.clone(), &c.fields),
            GraphNode::Struct(s) => (s.name.clone(), &s.fields),
            _ => continue,
        };

        // Check: does this class implement an interface?
        let implemented_interfaces: Vec<String> = ctx.graph
            .outgoing_edges(idx)
            .into_iter()
            .filter(|(_, k)| matches!(k, EdgeKind::Implements))
            .filter_map(|(target, _)| ctx.graph.get_node(target).map(|n| n.name().to_string()))
            .collect();

        if implemented_interfaces.is_empty() {
            continue;
        }

        // Check: does it wrap a different type (has a field whose type is NOT the same interface)?
        for field in fields {
            let field_type = match &field.type_annotation {
                Some(t) => t.clone(),
                None => continue,
            };

            // The wrapped type should not be the same interface this class implements
            if implemented_interfaces.contains(&field_type) {
                continue; // This would be a Proxy, not an Adapter
            }

            // The wrapped type should be a known class/struct (not a primitive)
            // We check if the type name looks like a class (capitalized) and isn't a common primitive
            let looks_like_class = field_type.chars().next().map_or(false, |c| c.is_uppercase())
                && !["String", "Vec", "HashMap", "HashSet", "Option", "Result", "Box", "Arc",
                     "bool", "i32", "i64", "u32", "u64", "f32", "f64", "usize", "isize"]
                    .contains(&field_type.as_str());

            if looks_like_class {
                // Check: do the class methods delegate to this field?
                let children = ctx.get_children(idx);
                let method_count = children.iter().filter(|(_, n)| matches!(n, GraphNode::Function(_))).count();
                if method_count >= 2 {
                    findings.push(Finding {
                        tier: Tier::Medium,
                        kind: FindingKind::DetectedAdapter {
                            adapter_name: class_name.clone(),
                            adaptee_type: field_type.clone(),
                            interface_name: implemented_interfaces[0].clone(),
                        },
                        node_indices: vec![idx.index()],
                        description: format!(
                            "Class `{}` implements `{}` and wraps a `{}` field — \
                             this looks like the adapter pattern, translating \
                             the `{}` interface to `{}`.",
                            class_name, implemented_interfaces[0], field_type,
                            implemented_interfaces[0], field_type,
                        ),
                    });
                    break; // One finding per class
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 25: Proxy — wraps same-interface type, delegates
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_proxy(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    for idx in ctx.graph.graph.node_indices() {
        let (class_name, fields) = match &ctx.graph.graph[idx] {
            GraphNode::Class(c) => (c.name.clone(), &c.fields),
            GraphNode::Struct(s) => (s.name.clone(), &s.fields),
            _ => continue,
        };

        // What interfaces/traits does this class implement?
        let implemented: HashSet<String> = ctx.graph
            .outgoing_edges(idx)
            .into_iter()
            .filter(|(_, k)| matches!(k, EdgeKind::Implements) || matches!(k, EdgeKind::Inherits))
            .filter_map(|(target, _)| ctx.graph.get_node(target).map(|n| n.name().to_string()))
            .collect();

        if implemented.is_empty() {
            continue;
        }

        // Check: has a field whose type matches one of the implemented interfaces?
        for field in fields {
            let field_type = match &field.type_annotation {
                Some(t) => t.clone(),
                None => continue,
            };

            if implemented.contains(&field_type) || implemented.iter().any(|i| field_type.contains(i)) {
                findings.push(Finding {
                    tier: Tier::Medium,
                    kind: FindingKind::DetectedProxy {
                        proxy_name: class_name.clone(),
                        wrapped_type: field_type.clone(),
                    },
                    node_indices: vec![idx.index()],
                    description: format!(
                        "Class `{}` implements `{}` and wraps a field of the same type — \
                         this looks like the proxy pattern (adds a layer of indirection \
                         before delegating to the real implementation).",
                        class_name, field_type,
                    ),
                });
                break;
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 26: Command — multiple classes implement a single-method interface
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_command(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Find interfaces/traits with exactly 1 method
    for idx in ctx.graph.graph.node_indices() {
        let interface_name = match &ctx.graph.graph[idx] {
            GraphNode::Trait(t) => t.name.clone(),
            GraphNode::Interface(i) => i.name.clone(),
            _ => continue,
        };

        let children = ctx.get_children(idx);
        let methods: Vec<String> = children
            .iter()
            .filter_map(|(_, n)| {
                if let GraphNode::Function(f) = n {
                    Some(f.name.clone())
                } else {
                    None
                }
            })
            .collect();

        // Single-method interface with a command-like name
        if methods.len() != 1 {
            continue;
        }
        let method_name = &methods[0];

        // Check if 3+ classes implement this interface
        let implementors = ctx.graph.get_implementors(idx);
        if implementors.len() < 3 {
            continue;
        }

        let command_names: Vec<String> = implementors
            .iter()
            .map(|(_, n)| n.name().to_string())
            .collect();

        findings.push(Finding {
            tier: Tier::Medium,
            kind: FindingKind::DetectedCommand {
                interface_name: interface_name.clone(),
                command_names: command_names.clone(),
                method_name: method_name.clone(),
            },
            node_indices: std::iter::once(idx.index())
                .chain(implementors.iter().map(|(i, _)| i.index()))
                .collect(),
            description: format!(
                "Interface `{}` has a single method `{}` with {} implementors ({}). \
                 This is the command pattern — each implementor encapsulates a different action.",
                interface_name, method_name, command_names.len(), command_names.join(", "),
            ),
        });
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 27: Chain of Responsibility — self-referencing field + conditional delegation
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_chain_of_responsibility(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    for idx in ctx.graph.graph.node_indices() {
        let (class_name, fields) = match &ctx.graph.graph[idx] {
            GraphNode::Class(c) => (c.name.clone(), &c.fields),
            GraphNode::Struct(s) => (s.name.clone(), &s.fields),
            _ => continue,
        };

        // Also check interfaces this class implements
        let implemented: HashSet<String> = ctx.graph
            .outgoing_edges(idx)
            .into_iter()
            .filter(|(_, k)| matches!(k, EdgeKind::Implements) || matches!(k, EdgeKind::Inherits))
            .filter_map(|(target, _)| ctx.graph.get_node(target).map(|n| n.name().to_string()))
            .collect();

        // Look for a field typed as self or an implemented interface
        let chain_field_names = ["next", "successor", "handler", "next_handler", "chain", "parent"];

        for field in fields {
            let field_type = match &field.type_annotation {
                Some(t) => t.clone(),
                None => continue,
            };

            // Field type matches own class name or an implemented interface
            let is_self_referencing = field_type.contains(&class_name)
                || implemented.iter().any(|i| field_type.contains(i));

            // Field name suggests a chain link
            let lower_name = field.name.to_lowercase();
            let has_chain_name = chain_field_names.iter().any(|n| {
                lower_name.contains(n)
            });

            if is_self_referencing && has_chain_name {
                findings.push(Finding {
                    tier: Tier::Medium,
                    kind: FindingKind::DetectedChainOfResponsibility {
                        class_name: class_name.clone(),
                        next_field: field.name.clone(),
                    },
                    node_indices: vec![idx.index()],
                    description: format!(
                        "Class `{}` has a field `{}` of type `{}` that references \
                         its own type/interface — this looks like the chain of responsibility \
                         pattern, where each handler either processes a request or passes it on.",
                        class_name, field.name, field_type,
                    ),
                });
                break;
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 28: Dependency Injection — constructor params typed as interfaces
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_dependency_injection(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Collect all interface/trait names
    let interface_names: HashSet<String> = ctx.graph
        .graph
        .node_indices()
        .filter_map(|idx| {
            match &ctx.graph.graph[idx] {
                GraphNode::Trait(t) => Some(t.name.clone()),
                GraphNode::Interface(i) => Some(i.name.clone()),
                _ => None,
            }
        })
        .collect();

    if interface_names.is_empty() {
        return;
    }

    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };

        // Only check constructors
        let is_constructor = func.name == "__init__" || func.name == "new"
            || func.name == "constructor" || func.name == "create";
        if !is_constructor {
            continue;
        }

        // Find the enclosing class
        let class_name = match &func.class_context {
            Some(n) => n.clone(),
            None => continue,
        };

        // Check if any constructor params are typed as interfaces
        let interface_params: Vec<(String, String)> = func
            .args
            .iter()
            .zip(func.arg_types.iter())
            .filter_map(|(name, type_opt)| {
                if let Some(type_name) = type_opt {
                    // Check if this type is a known interface/trait
                    if interface_names.contains(type_name) {
                        return Some((name.clone(), type_name.clone()));
                    }
                    // Also check if the type contains an interface name (e.g. Box<dyn Trait>)
                    for iface in &interface_names {
                        if type_name.contains(iface) {
                            return Some((name.clone(), type_name.clone()));
                        }
                    }
                }
                None
            })
            .collect();

        if interface_params.len() >= 1 {
            let tier = if interface_params.len() >= 2 {
                Tier::Medium
            } else {
                Tier::Low
            };

            findings.push(Finding {
                tier,
                kind: FindingKind::DetectedDependencyInjection {
                    class_name: class_name.clone(),
                    constructor_name: func.name.clone(),
                    interface_params: interface_params.clone(),
                },
                node_indices: vec![idx.index()],
                description: format!(
                    "Constructor `{}::{}` takes {} interface-typed parameter(s): {}. \
                     This is dependency injection — the class depends on abstractions, \
                     not concrete implementations.",
                    class_name,
                    func.name,
                    interface_params.len(),
                    interface_params
                        .iter()
                        .map(|(n, t)| format!("`{}: {}`", n, t))
                        .collect::<Vec<_>>()
                        .join(", "),
                ),
            });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 38: Visitor pattern
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_visitor(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Look for classes with methods named `visit_*` (visitors)
    // and classes with `accept` methods (elements)
    let classes: Vec<(NodeIndex, &GraphNode)> = ctx.graph
        .graph
        .node_indices()
        .filter_map(|idx| {
            let node = &ctx.graph.graph[idx];
            if matches!(node, GraphNode::Class(_)) {
                Some((idx, node))
            } else {
                None
            }
        })
        .collect();

    for (class_idx, class_node) in &classes {
        let methods: Vec<String> = ctx.get_children(*class_idx)
            .into_iter()
            .filter_map(|(_, n)| {
                if let GraphNode::Function(f) = n { Some(f.name.clone()) } else { None }
            })
            .collect();

        let visit_methods: Vec<&String> = methods.iter()
            .filter(|m| m.starts_with("visit_") || m.starts_with("visit"))
            .collect();

        if visit_methods.len() >= 3 {
            // This looks like a visitor — extract element names from visit_X
            let element_names: Vec<String> = visit_methods.iter()
                .filter_map(|m| m.strip_prefix("visit_").or_else(|| m.strip_prefix("visit")))
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect();

            findings.push(Finding {
                tier: Tier::Medium,
                kind: FindingKind::DetectedVisitor {
                    visitor_name: class_node.name().to_string(),
                    element_names,
                },
                node_indices: vec![class_idx.index()],
                description: format!(
                    "`{}` implements the Visitor pattern with {} visit methods.",
                    class_node.name(), visit_methods.len()
                ),
            });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 39: Iterator pattern
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_iterator(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    let iterator_method_sets: &[&[&str]] = &[
        &["__iter__", "__next__"],          // Python
        &["next", "has_next"],               // Java-style
        &["next", "hasNext"],                // Java
        &["next", "Symbol.iterator"],        // JS/TS
    ];

    let classes: Vec<(NodeIndex, &GraphNode)> = ctx.graph
        .graph
        .node_indices()
        .filter_map(|idx| {
            let node = &ctx.graph.graph[idx];
            if matches!(node, GraphNode::Class(_)) {
                Some((idx, node))
            } else {
                None
            }
        })
        .collect();

    for (class_idx, class_node) in &classes {
        let method_names: HashSet<String> = ctx.get_children(*class_idx)
            .into_iter()
            .filter_map(|(_, n)| {
                if let GraphNode::Function(f) = n { Some(f.name.clone()) } else { None }
            })
            .collect();

        for method_set in iterator_method_sets {
            if method_set.iter().all(|m| method_names.contains(*m)) {
                findings.push(Finding {
                    tier: Tier::Medium,
                    kind: FindingKind::DetectedIterator {
                        class_name: class_node.name().to_string(),
                    },
                    node_indices: vec![class_idx.index()],
                    description: format!(
                        "`{}` implements the Iterator pattern.",
                        class_node.name()
                    ),
                });
                break;
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 40: State pattern
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_state(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // State pattern: an interface/trait with 2+ implementors where implementors
    // have a field typed as the interface itself (state transitions).
    // Similar to Strategy but with self-referencing.
    let abstractions: Vec<(NodeIndex, &GraphNode)> = ctx.graph
        .graph
        .node_indices()
        .filter_map(|idx| {
            let node = &ctx.graph.graph[idx];
            if matches!(node, GraphNode::Trait(_) | GraphNode::Interface(_)) {
                Some((idx, node))
            } else {
                None
            }
        })
        .collect();

    for (abs_idx, abs_node) in &abstractions {
        let abs_name = abs_node.name();
        let implementors = ctx.graph.get_implementors(*abs_idx);

        if implementors.len() < 2 {
            continue;
        }

        // Check if any implementor has a field typed as the interface
        let mut state_names = Vec::new();
        for (_, imp_node) in &implementors {
            if let GraphNode::Class(cd) = imp_node {
                let has_state_field = cd.fields.iter().any(|f| {
                    f.type_annotation.as_deref().map(|t| t.contains(abs_name)).unwrap_or(false)
                });
                if has_state_field {
                    state_names.push(cd.name.clone());
                }
            }
        }

        if state_names.len() >= 2 {
            findings.push(Finding {
                tier: Tier::Medium,
                kind: FindingKind::DetectedState {
                    state_interface: abs_name.to_string(),
                    state_names: state_names.clone(),
                },
                node_indices: vec![abs_idx.index()],
                description: format!(
                    "`{}` with self-referencing implementors [{}] suggests the State pattern.",
                    abs_name,
                    state_names.join(", ")
                ),
            });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 41: Composite pattern
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_composite(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // A class that contains a collection (list/vec/array) of its own type
    let classes: Vec<(NodeIndex, &GraphNode)> = ctx.graph
        .graph
        .node_indices()
        .filter_map(|idx| {
            let node = &ctx.graph.graph[idx];
            if let GraphNode::Class(_d) = node {
                Some((idx, node))
            } else {
                None
            }
        })
        .collect();

    let collection_patterns = ["Vec<", "List<", "ArrayList<", "list[", "Array<", "[]"];

    for (class_idx, class_node) in &classes {
        let class_data = match class_node {
            GraphNode::Class(d) => d,
            _ => continue,
        };

        for field in &class_data.fields {
            if let Some(ref type_ann) = field.type_annotation {
                let is_collection = collection_patterns.iter().any(|p| type_ann.contains(p));
                let contains_self_type = type_ann.contains(&class_data.name);

                if is_collection && contains_self_type {
                    findings.push(Finding {
                        tier: Tier::Medium,
                        kind: FindingKind::DetectedComposite {
                            class_name: class_data.name.clone(),
                            collection_field: field.name.clone(),
                        },
                        node_indices: vec![class_idx.index()],
                        description: format!(
                            "`{}` has field `{}` containing a collection of itself — Composite pattern.",
                            class_data.name, field.name
                        ),
                    });
                    break;
                }
            }
        }
    }

    // Also check structs
    for &(idx, node) in &ctx.structs {
        let _sd = if let GraphNode::Struct(sd) = node { sd } else { continue };
        if let GraphNode::Struct(sd) = node {
            for field in &sd.fields {
                if let Some(ref type_ann) = field.type_annotation {
                    let is_collection = collection_patterns.iter().any(|p| type_ann.contains(p));
                    let contains_self_type = type_ann.contains(&sd.name);

                    if is_collection && contains_self_type {
                        findings.push(Finding {
                            tier: Tier::Medium,
                            kind: FindingKind::DetectedComposite {
                                class_name: sd.name.clone(),
                                collection_field: field.name.clone(),
                            },
                            node_indices: vec![idx.index()],
                            description: format!(
                                "`{}` has field `{}` containing a collection of itself — Composite pattern.",
                                sd.name, field.name
                            ),
                        });
                        break;
                    }
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 42: Repository pattern
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_repository(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    let crud_keywords: &[&str] = &[
        "find", "get", "save", "create", "update", "delete", "remove",
        "insert", "fetch", "store", "list", "add",
    ];
    let min_crud = 3;

    let classes: Vec<(NodeIndex, &GraphNode)> = ctx.graph
        .graph
        .node_indices()
        .filter_map(|idx| {
            let node = &ctx.graph.graph[idx];
            if matches!(node, GraphNode::Class(_)) {
                Some((idx, node))
            } else {
                None
            }
        })
        .collect();

    for (class_idx, class_node) in &classes {
        let methods: Vec<String> = ctx.get_children(*class_idx)
            .into_iter()
            .filter_map(|(_, n)| {
                if let GraphNode::Function(f) = n { Some(f.name.clone()) } else { None }
            })
            .collect();

        let crud_methods: Vec<String> = methods.iter()
            .filter(|m| {
                let lower = m.to_lowercase();
                crud_keywords.iter().any(|k| lower.starts_with(k))
            })
            .cloned()
            .collect();

        if crud_methods.len() >= min_crud {
            // Try to extract entity name from class name
            let class_name = class_node.name();
            let entity_hint = class_name
                .strip_suffix("Repository")
                .or_else(|| class_name.strip_suffix("Repo"))
                .or_else(|| class_name.strip_suffix("Store"))
                .or_else(|| class_name.strip_suffix("DAO"))
                .or_else(|| class_name.strip_suffix("Dao"))
                .unwrap_or(class_name)
                .to_string();

            findings.push(Finding {
                tier: Tier::Medium,
                kind: FindingKind::DetectedRepository {
                    class_name: class_name.to_string(),
                    entity_hint,
                    crud_methods: crud_methods.clone(),
                },
                node_indices: vec![class_idx.index()],
                description: format!(
                    "`{}` implements the Repository pattern with {} CRUD methods: [{}].",
                    class_name, crud_methods.len(), crud_methods.join(", ")
                ),
            });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 43: Prototype pattern
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_prototype(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    let clone_names: HashSet<&str> = [
        "clone", "copy", "deep_copy", "deepcopy", "dup", "duplicate",
        "__copy__", "__deepcopy__",
    ].iter().copied().collect();

    let classes: Vec<(NodeIndex, &GraphNode)> = ctx.graph
        .graph
        .node_indices()
        .filter_map(|idx| {
            let node = &ctx.graph.graph[idx];
            if matches!(node, GraphNode::Class(_)) {
                Some((idx, node))
            } else {
                None
            }
        })
        .collect();

    for (class_idx, class_node) in &classes {
        let methods: Vec<String> = ctx.get_children(*class_idx)
            .into_iter()
            .filter_map(|(_, n)| {
                if let GraphNode::Function(f) = n { Some(f.name.clone()) } else { None }
            })
            .collect();

        for method in &methods {
            if clone_names.contains(method.as_str()) {
                findings.push(Finding {
                    tier: Tier::Medium,
                    kind: FindingKind::DetectedPrototype {
                        class_name: class_node.name().to_string(),
                        clone_method: method.clone(),
                    },
                    node_indices: vec![class_idx.index()],
                    description: format!(
                        "`{}` implements the Prototype pattern via `{}` method.",
                        class_node.name(), method
                    ),
                });
                break;
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 51: Flyweight pattern
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_flyweight(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    let cache_field_names: HashSet<&str> = [
        "cache", "_cache", "instances", "_instances", "pool", "_pool",
        "flyweights", "_flyweights", "registry", "_registry",
    ].iter().copied().collect();

    for &(idx, node) in &ctx.classes {
        let _cd = if let GraphNode::Class(cd) = node { cd } else { continue };
        if let GraphNode::Class(cd) = node {
            // Pre-lowercase field names once to avoid repeated allocation
            let cache_field = cd.fields.iter().find(|f| {
                f.is_static && cache_field_names.contains(f.name.to_ascii_lowercase().as_str())
            });

            let cache_field = match cache_field {
                Some(f) => f,
                None => continue,
            };

            let methods: Vec<String> = ctx.get_children(idx)
                .into_iter()
                .filter_map(|(_, n)| {
                    if let GraphNode::Function(f) = n { Some(f.name.clone()) } else { None }
                })
                .collect();

            let has_factory = methods.iter().any(|m| {
                let lower = m.to_ascii_lowercase();
                lower.starts_with("get") || lower.starts_with("create")
                    || lower.starts_with("of") || lower == "instance"
                    || lower == "get_instance" || lower == "get_or_create"
            });

            if has_factory {
                let cache_name = cache_field.name.clone();

                findings.push(Finding {
                    tier: Tier::Medium,
                    kind: FindingKind::DetectedFlyweight {
                        class_name: cd.name.clone(),
                        cache_field: cache_name,
                    },
                    node_indices: vec![idx.index()],
                    description: format!(
                        "`{}` uses a static cache + factory method — Flyweight pattern.",
                        cd.name
                    ),
                });
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 52: Event emitter / observer (method-based)
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_event_emitter(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    let event_method_sets: &[&[&str]] = &[
        &["subscribe", "unsubscribe", "notify"],
        &["on", "off", "emit"],
        &["addEventListener", "removeEventListener"],
        &["addListener", "removeListener", "emit"],
        &["register", "unregister", "notify"],
        &["attach", "detach", "notify"],
        &["add_observer", "remove_observer", "notify_observers"],
        &["bind", "unbind", "trigger"],
    ];

    for &(idx, node) in &ctx.classes {
        if !matches!(node, GraphNode::Class(_)) {
            continue;
        }

        let method_names: HashSet<String> = ctx.get_children(idx)
            .into_iter()
            .filter_map(|(_, n)| {
                if let GraphNode::Function(f) = n { Some(f.name.clone()) } else { None }
            })
            .collect();

        for method_set in event_method_sets {
            let matched: Vec<String> = method_set.iter()
                .filter(|m| method_names.contains(**m))
                .map(|m| m.to_string())
                .collect();

            if matched.len() >= 2 {
                findings.push(Finding {
                    tier: Tier::Medium,
                    kind: FindingKind::DetectedEventEmitter {
                        class_name: node.name().to_string(),
                        event_methods: matched.clone(),
                    },
                    node_indices: vec![idx.index()],
                    description: format!(
                        "`{}` implements the Observer/EventEmitter pattern with methods [{}].",
                        node.name(), matched.join(", ")
                    ),
                });
                break;
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 53: Memento pattern
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_memento(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    let memento_pairs: &[(&str, &str)] = &[
        ("save_state", "restore_state"),
        ("save", "restore"),
        ("undo", "redo"),
        ("checkpoint", "rollback"),
        ("snapshot", "restore"),
        ("createMemento", "setMemento"),
        ("create_memento", "set_memento"),
        ("getState", "setState"),
        ("get_state", "set_state"),
    ];

    for &(idx, node) in &ctx.classes {
        if !matches!(node, GraphNode::Class(_)) {
            continue;
        }

        let method_names: HashSet<String> = ctx.get_children(idx)
            .into_iter()
            .filter_map(|(_, n)| {
                if let GraphNode::Function(f) = n { Some(f.name.clone()) } else { None }
            })
            .collect();

        for (save, restore) in memento_pairs {
            if method_names.contains(*save) && method_names.contains(*restore) {
                findings.push(Finding {
                    tier: Tier::Medium,
                    kind: FindingKind::DetectedMemento {
                        class_name: node.name().to_string(),
                        method_pair: (save.to_string(), restore.to_string()),
                    },
                    node_indices: vec![idx.index()],
                    description: format!(
                        "`{}` implements the Memento pattern via `{}`/`{}`.",
                        node.name(), save, restore
                    ),
                });
                break;
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 54: Fluent builder (detected)
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_fluent_builder(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    let self_return_patterns: &[&str] = &["Self", "self", "&self", "&mut self"];

    for &(idx, node) in &ctx.classes {
        let class_name = match node {
            GraphNode::Class(d) => &d.name,
            _ => continue,
        };

        let methods: Vec<(String, Option<String>)> = ctx.get_children(idx)
            .into_iter()
            .filter_map(|(_, n)| {
                if let GraphNode::Function(f) = n {
                    Some((f.name.clone(), f.return_type.clone()))
                } else {
                    None
                }
            })
            .collect();

        let fluent_methods: Vec<String> = methods.iter()
            .filter(|(_name, ret)| {
                if let Some(rt) = ret {
                    let clean = rt.trim();
                    self_return_patterns.iter().any(|p| clean == *p)
                        || clean == class_name.as_str()
                        || clean == &format!("&{}", class_name)
                        || clean == &format!("&mut {}", class_name)
                } else {
                    false
                }
            })
            .map(|(name, _)| name.clone())
            .collect();

        if fluent_methods.len() >= 3 {
            findings.push(Finding {
                tier: Tier::Medium,
                kind: FindingKind::DetectedFluentBuilder {
                    class_name: class_name.clone(),
                    fluent_methods: fluent_methods.clone(),
                },
                node_indices: vec![idx.index()],
                description: format!(
                    "`{}` uses a fluent/builder interface with {} chainable methods: [{}].",
                    class_name, fluent_methods.len(), fluent_methods.join(", ")
                ),
            });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 55: Null object pattern
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_null_object(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    for &(idx, node) in &ctx.classes {
        let (class_name, bases) = match node {
            GraphNode::Class(d) if !d.bases.is_empty() => (&d.name, &d.bases),
            _ => continue,
        };

        let lower = class_name.to_lowercase();
        let name_suggests = lower.contains("null") || lower.contains("noop")
            || lower.contains("no_op") || lower.contains("dummy")
            || lower.contains("stub") || lower.contains("empty")
            || lower.contains("default");

        let methods: Vec<(NodeIndex, &GraphNode)> = ctx.get_children(idx)
            .into_iter()
            .filter(|(_, n)| matches!(n, GraphNode::Function(_)))
            .collect();

        if methods.is_empty() {
            continue;
        }

        let all_trivial = methods.iter().all(|(m_idx, m_node)| {
            let is_short = m_node.source_snippet()
                .map(|s| s.lines().count() <= 3)
                .unwrap_or(true);
            let no_calls = ctx.callee_indices(*m_idx).is_empty();
            is_short && no_calls
        });

        if all_trivial && (name_suggests || methods.len() >= 2) {
            findings.push(Finding {
                tier: Tier::Medium,
                kind: FindingKind::DetectedNullObject {
                    class_name: class_name.clone(),
                    interface_name: bases[0].clone(),
                },
                node_indices: vec![idx.index()],
                description: format!(
                    "`{}` inherits from `{}` with all trivial/no-op methods — Null Object pattern.",
                    class_name, bases[0]
                ),
            });
        }
    }
}
