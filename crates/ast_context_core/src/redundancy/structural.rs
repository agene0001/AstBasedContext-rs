use crate::types::EdgeKind;
use crate::types::node::GraphNode;

use std::collections::HashMap;
use std::collections::HashSet;
use super::context::AnalysisContext;
use super::types::{Tier, FindingKind, Finding};

// ─────────────────────────────────────────────────────────────────────────────
// Check 44: Hub module
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_hub_module(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    const HUB_THRESHOLD: usize = 10;

    for &(file_idx, file_node) in &ctx.files {
        // Count outgoing IMPORTS edges
        let import_count = ctx.graph.graph
            .edges_directed(file_idx, petgraph::Direction::Outgoing)
            .filter(|e| matches!(e.weight(), EdgeKind::Imports { .. }))
            .count();

        if import_count >= HUB_THRESHOLD {
            findings.push(Finding {
                tier: Tier::Medium,
                kind: FindingKind::HubModule {
                    file_name: file_node.name().to_string(),
                    import_count,
                },
                node_indices: vec![file_idx.index()],
                description: format!(
                    "`{}` imports from {} other modules — it may be a bottleneck. Consider splitting responsibilities.",
                    file_node.name(), import_count
                ),
            });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 45: Orphan module
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_orphan_module(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Skip common entry-point filenames (static, built once)
    static ENTRY_FILES: std::sync::LazyLock<HashSet<&str>> = std::sync::LazyLock::new(|| {
        [
            "main.rs", "main.py", "main.go", "main.java", "main.c", "main.cpp",
            "mod.rs", "lib.rs", "__init__.py", "__main__.py",
            "index.js", "index.ts", "index.tsx", "index.jsx",
            "setup.py", "conftest.py", "app.py", "app.js", "app.ts",
        ].iter().copied().collect()
    });
    let entry_files = &*ENTRY_FILES;

    for &(file_idx, file_node) in &ctx.files {
        let file_name = file_node.name();

        if entry_files.contains(file_name) {
            continue;
        }

        // Check if any other file calls ctx.functions in this file or imports from it
        let incoming = ctx.graph.graph
            .edges_directed(file_idx, petgraph::Direction::Incoming)
            .count();

        // Also check if any function inside this file is called from outside
        let children = ctx.get_children(file_idx);
        let has_external_callers = children.iter().any(|(child_idx, _)| {
            !ctx.caller_indices(*child_idx).is_empty()
        });

        if incoming == 0 && !has_external_callers {
            // Check that the file actually has some content (not empty)
            let has_functions = children.iter().any(|(_, n)| matches!(n, GraphNode::Function(_)));
            if has_functions {
                let file_path = match file_node {
                    GraphNode::File(fd) => fd.path.display().to_string(),
                    _ => String::new(),
                };
                findings.push(Finding {
                    tier: Tier::Low,
                    kind: FindingKind::OrphanModule {
                        file_name: file_name.to_string(),
                        file_path: file_path.clone(),
                    },
                    node_indices: vec![file_idx.index()],
                    description: format!(
                        "`{}` has no incoming calls or imports — it may be unused or only used as an entry point.",
                        file_name
                    ),
                });
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 56: Inconsistent naming
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_inconsistent_naming(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    for &(file_idx, file_node) in &ctx.files {
        let method_names: Vec<String> = ctx.get_children(file_idx)
            .into_iter()
            .filter_map(|(_, n)| {
                if let GraphNode::Function(f) = n {
                    if !f.name.starts_with("__") && (f.name.contains('_') || f.name.chars().any(|c| c.is_uppercase())) {
                        Some(f.name.clone())
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();

        if method_names.len() < 4 {
            continue;
        }

        let snake: Vec<String> = method_names.iter()
            .filter(|n| n.contains('_') && n.chars().all(|c| c.is_lowercase() || c == '_' || c.is_numeric()))
            .cloned()
            .collect();

        let camel: Vec<String> = method_names.iter()
            .filter(|n| !n.contains('_') && n.chars().any(|c| c.is_uppercase()))
            .cloned()
            .collect();

        if snake.len() >= 2 && camel.len() >= 2 {
            findings.push(Finding {
                tier: Tier::Low,
                kind: FindingKind::InconsistentNaming {
                    scope_name: file_node.name().to_string(),
                    snake_case_names: snake.iter().take(5).cloned().collect(),
                    camel_case_names: camel.iter().take(5).cloned().collect(),
                },
                node_indices: vec![file_idx.index()],
                description: format!(
                    "`{}` mixes snake_case ({} ctx.functions) and camelCase ({} ctx.functions) — consider standardizing.",
                    file_node.name(), snake.len(), camel.len()
                ),
            });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 57: Circular package dependency
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_circular_package_dependency(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    use petgraph::algo::tarjan_scc;
    use petgraph::graph::DiGraph;
    use petgraph::visit::EdgeRef;

    let mut dir_graph: DiGraph<String, ()> = DiGraph::new();
    let mut dir_indices: HashMap<String, petgraph::graph::NodeIndex> = HashMap::new();

    for &(_idx, node) in &ctx.files {
        let _fd = if let GraphNode::File(fd) = node { fd } else { continue };
        if let GraphNode::File(fd) = node {
            if let Some(parent) = fd.path.parent() {
                let dir = parent.display().to_string();
                if !dir_indices.contains_key(&dir) {
                    let didx = dir_graph.add_node(dir.clone());
                    dir_indices.insert(dir, didx);
                }
            }
        }
    }

    for &(idx, node) in &ctx.functions {
        let _f = if let GraphNode::Function(f) = node { f } else { continue };
        if let GraphNode::Function(f) = node {
            let caller_dir = match f.path.parent() {
                Some(p) => p.display().to_string(),
                None => continue,
            };

            for edge in ctx.graph.graph.edges_directed(idx, petgraph::Direction::Outgoing) {
                if !matches!(edge.weight(), EdgeKind::Calls { .. }) {
                    continue;
                }
                let target = &ctx.graph.graph[edge.target()];
                if let GraphNode::Function(tf) = target {
                    let callee_dir = match tf.path.parent() {
                        Some(p) => p.display().to_string(),
                        None => continue,
                    };

                    if caller_dir != callee_dir {
                        if let (Some(&src), Some(&dst)) = (dir_indices.get(&caller_dir), dir_indices.get(&callee_dir)) {
                            if !dir_graph.edges_directed(src, petgraph::Direction::Outgoing)
                                .any(|e| e.target() == dst) {
                                dir_graph.add_edge(src, dst, ());
                            }
                        }
                    }
                }
            }
        }
    }

    let sccs = tarjan_scc(&dir_graph);
    for scc in &sccs {
        if scc.len() < 2 {
            continue;
        }

        let cycle: Vec<String> = scc.iter()
            .map(|idx| {
                let full = &dir_graph[*idx];
                std::path::Path::new(full)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| full.clone())
            })
            .collect();

        findings.push(Finding {
            tier: Tier::High,
            kind: FindingKind::CircularPackageDependency {
                cycle: cycle.clone(),
            },
            node_indices: vec![],
            description: format!(
                "Circular dependency between packages: [{}]",
                cycle.join(" → ")
            ),
        });
    }
}
