use crate::types::EdgeKind;
use crate::types::node::GraphNode;
use petgraph::graph::NodeIndex;
use std::collections::HashSet;
use super::context::AnalysisContext;
use super::helpers::estimate_sections;
use super::helpers::extract_tokens_set;
use super::helpers::jaccard_sets;
use super::helpers::normalize_tokens_set;
use super::types::{Tier, FindingKind, Finding};

// ─────────────────────────────────────────────────────────────────────────────
// Check 1: Passthrough wrappers
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn find_passthroughs(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };

        let src = match &func.source {
            Some(s) => s,
            None => continue,
        };

        let line_count = src.lines().count();
        // Passthrough ctx.functions are short — typically 1-5 lines of actual body
        if line_count > 10 {
            continue;
        }

        // Check: does this function call exactly one other function?
        let callee_indices = ctx.callee_indices(idx);
        if callee_indices.len() != 1 {
            continue;
        }

        let callee_idx = callee_indices[0];
        let callee_node = match ctx.graph.get_node(callee_idx) {
            Some(n) => n,
            None => continue,
        };
        let callee_func = match callee_node {
            GraphNode::Function(f) => f,
            _ => continue,
        };

        // Check if wrapper params are forwarded to the callee
        let wrapper_args: HashSet<&str> = func.args.iter().map(|a| a.as_str()).collect();

        // Also check the call edge for actual args passed
        let call_edges: Vec<_> = ctx.graph
            .outgoing_edges(idx)
            .into_iter()
            .filter(|(target, kind)| {
                *target == callee_idx && matches!(kind, EdgeKind::Calls { .. })
            })
            .collect();

        let mut exact_forward = false;
        if let Some((_, EdgeKind::Calls { args, .. })) = call_edges.first() {
            // If call args match wrapper params (by name overlap)
            let call_arg_set: HashSet<&str> = args.iter().map(|a| a.as_str()).collect();
            if !wrapper_args.is_empty()
                && wrapper_args.iter().all(|a| call_arg_set.contains(a))
            {
                exact_forward = true;
            }
            // Also check if it's literally just forwarding all params
            if func.args.len() == callee_func.args.len() && func.args.len() == args.len() {
                exact_forward = true;
            }
        }

        // Determine tier
        let tier = if exact_forward && func.cyclomatic_complexity <= 1 {
            Tier::Critical
        } else if callee_indices.len() == 1 && line_count <= 5 {
            Tier::High
        } else {
            Tier::Medium
        };

        let desc = if exact_forward {
            format!(
                "`{}` is a passthrough wrapper that forwards all parameters to `{}`. \
                 Unless it serves as a public API facade, it can be replaced with a direct call.",
                func.name, callee_func.name,
            )
        } else {
            format!(
                "`{}` delegates to `{}` with minimal logic ({}  lines, complexity {}). \
                 Consider if this indirection is necessary.",
                func.name, callee_func.name, line_count, func.cyclomatic_complexity,
            )
        };

        findings.push(Finding {
            tier,
            kind: FindingKind::Passthrough {
                wrapper_name: func.name.clone(),
                target_name: callee_func.name.clone(),
                exact_forward,
            },
            node_indices: vec![idx.index(), callee_idx.index()],

            description: desc,
        });
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 2: Near-duplicates (normalized source comparison)
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn find_near_duplicates(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    let annotated: Vec<(NodeIndex, &str, &str, HashSet<&str>, usize)> = ctx.functions
        .iter()
        .filter_map(|&(idx, node)| {
            let src = node.source_snippet()?;
            let line_count = src.lines().count();
            if line_count < ctx.config.min_lines {
                return None;
            }
            let name = node.name();
            let token_set = normalize_tokens_set(src);
            Some((idx, name, src, token_set, line_count))
        })
        .collect();

    let mut used = vec![false; annotated.len()];

    for i in 0..annotated.len() {
        if used[i] {
            continue;
        }
        let mut group = vec![i];

        for j in (i + 1)..annotated.len() {
            if used[j] {
                continue;
            }
            let line_ratio = annotated[i].4.min(annotated[j].4) as f64
                / annotated[i].4.max(annotated[j].4) as f64;
            if line_ratio < 0.33 {
                continue;
            }
            let sim = jaccard_sets(&annotated[i].3, &annotated[j].3);
            if sim >= ctx.config.near_duplicate_threshold {
                group.push(j);
                used[j] = true;
            }
        }

        if group.len() >= 2 {
            used[i] = true;
            let sim = jaccard_sets(&annotated[group[0]].3, &annotated[group[1]].3);
            let names: Vec<String> = group.iter().map(|&g| annotated[g].1.to_string()).collect();
            let indices: Vec<usize> = group.iter().map(|&g| annotated[g].0.index()).collect();

            let tier = if sim >= 0.95 {
                Tier::Critical
            } else {
                Tier::High
            };

            findings.push(Finding {
                tier,
                kind: FindingKind::NearDuplicate {
                    names: names.clone(),
                    similarity: sim,
                },
                node_indices: indices,
                description: format!(
                    "Near-duplicate code ({:.0}% similar): {}. \
                     These likely do the same thing with cosmetic differences.",
                    sim * 100.0,
                    names.join(", "),
                ),
            });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 3: Structural similarity (existing approach, but with tier context)
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn find_structural_similar(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    let annotated: Vec<(NodeIndex, &str, HashSet<&str>, usize)> = ctx.functions
        .iter()
        .filter_map(|&(idx, node)| {
            let src = node.source_snippet()?;
            let line_count = src.lines().count();
            if line_count < ctx.config.min_lines {
                return None;
            }
            let token_set = extract_tokens_set(src);
            Some((idx, node.name(), token_set, line_count))
        })
        .collect();

    let mut used = vec![false; annotated.len()];

    for i in 0..annotated.len() {
        if used[i] {
            continue;
        }
        let mut group = vec![i];

        for j in (i + 1)..annotated.len() {
            if used[j] {
                continue;
            }
            let line_ratio = annotated[i].3.min(annotated[j].3) as f64
                / annotated[i].3.max(annotated[j].3) as f64;
            if line_ratio < 0.33 {
                continue;
            }
            let sim = jaccard_sets(&annotated[i].2, &annotated[j].2);
            // Only flag if above structural threshold but below near-duplicate
            // (near-duplicates are already caught in check 2)
            if sim >= ctx.config.structural_threshold && sim < ctx.config.near_duplicate_threshold {
                group.push(j);
                used[j] = true;
            }
        }

        if group.len() >= 2 {
            used[i] = true;
            let sim = jaccard_sets(&annotated[group[0]].2, &annotated[group[1]].2);
            let names: Vec<String> = group.iter().map(|&g| annotated[g].1.to_string()).collect();
            let indices: Vec<usize> = group.iter().map(|&g| annotated[g].0.index()).collect();

            findings.push(Finding {
                tier: Tier::Medium,
                kind: FindingKind::StructurallySimilar {
                    names: names.clone(),
                    similarity: sim,
                },
                node_indices: indices,
                description: format!(
                    "Structurally similar code ({:.0}% token overlap): {}. \
                     These may serve similar purposes — check if a shared abstraction is possible.",
                    sim * 100.0,
                    names.join(", "),
                ),
            });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 4: Merge candidates — functions with a shared core but different branches
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn find_merge_candidates(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    let annotated: Vec<(NodeIndex, &str, &str, HashSet<&str>)> = ctx.functions
        .iter()
        .filter_map(|&(idx, node)| {
            let src = node.source_snippet()?;
            if src.lines().count() < ctx.config.min_lines.max(8) {
                return None;
            }
            let line_set: HashSet<&str> = src.lines().map(|l| l.trim()).collect();
            Some((idx, node.name(), src, line_set))
        })
        .collect();

    let mut used = vec![false; annotated.len()];

    for i in 0..annotated.len() {
        if used[i] {
            continue;
        }

        let set_a = &annotated[i].3;

        for j in (i + 1)..annotated.len() {
            if used[j] {
                continue;
            }

            let set_b = &annotated[j].3;

            let shared = set_a.intersection(set_b).count();
            let total = set_a.union(set_b).count().max(1);
            let shared_ratio = shared as f64 / total as f64;

            // Must have substantial overlap but not be near-duplicates
            if shared_ratio >= ctx.config.merge_threshold && shared_ratio < 0.80 {
                // Must also have meaningful differences (not just whitespace)
                let unique_a = set_a.difference(set_b).count();
                let unique_b = set_b.difference(set_a).count();
                if unique_a >= 2 && unique_b >= 2 {
                    used[j] = true;
                    let names = vec![
                        annotated[i].1.to_string(),
                        annotated[j].1.to_string(),
                    ];
                    let indices = vec![annotated[i].0.index(), annotated[j].0.index()];

                    let tier = if shared_ratio >= 0.65 {
                        Tier::Medium
                    } else {
                        Tier::Low
                    };

                    findings.push(Finding {
                        tier,
                        kind: FindingKind::MergeCandidate {
                            names: names.clone(),
                            shared_line_ratio: shared_ratio,
                        },
                        node_indices: indices,
                        description: format!(
                            "`{}` and `{}` share {:.0}% of their lines but differ in specific sections. \
                             Consider merging into a single function with a parameter to select behavior.",
                            names[0], names[1], shared_ratio * 100.0,
                        ),
                    });
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 5: Split candidates — functions that are too big and do too many things
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn find_split_candidates(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };

        let src = match &func.source {
            Some(s) => s,
            None => continue,
        };

        let line_count = src.lines().count();
        let complexity = func.cyclomatic_complexity;

        // Skip if below both thresholds
        if complexity < ctx.config.split_complexity_threshold
            && line_count < ctx.config.split_line_threshold
        {
            continue;
        }

        let sections = estimate_sections(src);

        // Only flag if there are multiple distinct sections
        if sections < 2 {
            continue;
        }

        let tier = if complexity >= ctx.config.split_complexity_threshold * 2
            || line_count >= ctx.config.split_line_threshold * 2
        {
            Tier::Medium
        } else {
            Tier::Low
        };

        findings.push(Finding {
            tier,
            kind: FindingKind::SplitCandidate {
                name: func.name.clone(),
                line_count,
                complexity,
                estimated_sections: sections,
            },
            node_indices: vec![idx.index()],
            description: format!(
                "`{}` is {} lines with cyclomatic complexity {} and ~{} distinct sections. \
                 Consider extracting sections into separate ctx.functions for clarity and testability.",
                func.name, line_count, complexity, sections,
            ),
        });
    }
}
