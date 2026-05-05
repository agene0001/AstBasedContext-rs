use crate::types::node::GraphNode;
use super::context::AnalysisContext;
use super::types::{Tier, FindingKind, Finding};

// ─────────────────────────────────────────────────────────────────────────────
// Check 79: High blast radius
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_high_blast_radius(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };
        if func.is_dependency {
            continue;
        }

        // Only run BFS for ctx.functions with >= 3 direct callers (performance)
        let direct_caller_count = ctx.caller_indices(idx).len();
        if direct_caller_count < 3 {
            continue;
        }

        let transitive = ctx.graph.get_transitive_callers(idx, 15);
        let modules: std::collections::HashSet<_> = transitive.iter().filter_map(|(_, cn, _)| {
            if let GraphNode::Function(f) = cn {
                Some(f.path.to_string_lossy().to_string())
            } else {
                None
            }
        }).collect();

        if modules.len() >= ctx.config.blast_radius_module_threshold {
            findings.push(Finding {
                tier: Tier::High,
                kind: FindingKind::HighBlastRadius {
                    function_name: func.name.clone(),
                    direct_callers: direct_caller_count,
                    transitive_callers: transitive.len(),
                    modules_affected: modules.len(),
                },
                node_indices: vec![idx.index()],
                description: format!(
                    "`{}`: {} modules affected ({} direct, {} transitive callers) — stabilize interface.",
                    func.name, modules.len(), direct_caller_count, transitive.len()
                ),
            });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 80: Misplaced function
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_misplaced_functions(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };
        if func.is_dependency {
            continue;
        }

        // Skip functions with no cross-file interactions
        let caller_count = ctx.caller_count(idx);
        let callee_count = ctx.callee_count(idx);
        if caller_count + callee_count < 3 {
            continue;
        }

        let own_file = func.path.to_string_lossy().to_string();
        let mut file_interactions: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

        // Count interactions with callers
        for (_, caller_node) in ctx.get_callers_of(idx) {
            if let GraphNode::Function(cf) = caller_node {
                let f = cf.path.to_string_lossy().to_string();
                *file_interactions.entry(f).or_default() += 1;
            }
        }

        // Count interactions with callees
        for (_, callee_node) in ctx.get_callees_of(idx) {
            if let GraphNode::Function(cf) = callee_node {
                let f = cf.path.to_string_lossy().to_string();
                *file_interactions.entry(f).or_default() += 1;
            }
        }

        let own_count = file_interactions.get(&own_file).copied().unwrap_or(0);

        // Find the file with the most interactions (excluding own file)
        let max_other = file_interactions.iter()
            .filter(|(f, _)| **f != own_file)
            .max_by_key(|(_, &count)| count);

        if let Some((other_file, &count)) = max_other {
            if count > own_count && count >= 3 {
                let short_name = std::path::Path::new(other_file)
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_else(|| other_file.clone());
                findings.push(Finding {
                    tier: Tier::Medium,
                    kind: FindingKind::MisplacedFunction {
                        function_name: func.name.clone(),
                        current_file: func.path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default(),
                        suggested_cluster: short_name.clone(),
                    },
                    node_indices: vec![idx.index()],
                    description: format!(
                        "`{}`: {} connections to `{}` vs {} in own file — may belong there.",
                        func.name, count, short_name, own_count
                    ),
                });
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 81: Implicit module (semantic clustering)
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_implicit_modules(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // For each pair of files, count cross-file call edges
    let mut cross_file_calls: std::collections::HashMap<(String, String), Vec<String>> = std::collections::HashMap::new();

    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };
        if func.is_dependency {
            continue;
        }

        let own_file = func.path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();

        for (_, callee_node) in ctx.get_callees_of(idx) {
            if let GraphNode::Function(cf) = callee_node {
                let other_file = cf.path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
                if own_file != other_file {
                    let key = if own_file < other_file {
                        (own_file.clone(), other_file)
                    } else {
                        (other_file, own_file.clone())
                    };
                    let names = cross_file_calls.entry(key).or_default();
                    if !names.contains(&func.name) {
                        names.push(func.name.clone());
                    }
                }
            }
        }
    }

    // Report file pairs with 5+ cross-file ctx.functions involved
    for ((file_a, file_b), func_names) in &cross_file_calls {
        if func_names.len() >= 5 {
            let mut display_names = func_names.clone();
            display_names.truncate(10);
            findings.push(Finding {
                tier: Tier::Low,
                kind: FindingKind::ImplicitModule {
                    cluster_functions: display_names.clone(),
                    spanning_files: vec![file_a.clone(), file_b.clone()],
                },
                node_indices: vec![],
                description: format!(
                    "{} functions tightly coupled across `{}` and `{}`: {} — implicit module, consider colocating.",
                    func_names.len(), file_a, file_b,
                    display_names.join(", ")
                ),
            });
        }
    }
}
