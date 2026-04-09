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
        // Passthrough functions are short — typically 1-5 lines of actual body
        if line_count > 10 {
            continue;
        }

        // A true passthrough has very few statements (just a call and maybe a
        // let binding). Functions with 2+ semicolons are doing real work even
        // if the graph only resolved one callee (common when method calls like
        // .insert(), .clone() fail to resolve, leaving only e.g. HashMap::new()
        // matched to a project-local new()).
        let stmt_count = src.matches(';').count();
        if stmt_count > 1 {
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

        // ── Rust-specific false-positive filters ────────────────────────
        if func.language == crate::types::Language::Rust {
            // FP #2: `impl Default for X { fn default() -> Self { Self::new() } }`
            // This is the idiomatic Rust pattern — the Default trait requires it.
            if func.name == "default"
                && func.context_type.as_deref() == Some("impl_item")
                && func.context.as_deref() == Some("Default")
            {
                continue;
            }

            if let Some(src) = &func.source {
                // FP #3 & #5: Constructors that build structs (Self { ... } or ..Default::default())
                // These aren't passthroughs — they construct and return a struct, possibly
                // with a side-effect call (e.g., logging).
                if src.contains("Self {") || src.contains("Self{")
                    || src.contains("..Default::default()")
                    || src.contains("..Self::default()")
                {
                    continue;
                }

                // FP #4: Accessor/getter methods that access self fields.
                // `fn foo(&self) -> T { self.field.clone() }` is a getter, not a passthrough.
                if func.args.iter().any(|a| a.contains("self"))
                    && src.contains("self.")
                {
                    continue;
                }
            }
        }

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
                "`{}` forwards all args to `{}` — replace with direct call unless it's a facade",
                func.name, callee_func.name,
            )
        } else {
            format!(
                "`{}` delegates to `{}` ({}L, cc={}) — unnecessary indirection?",
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

/// Check if two functions should be skipped for near-duplicate detection.
///
/// Returns true for Rust-specific patterns where identical code is required:
/// - Trait impl methods: same trait implemented on different types (language-mandated)
/// - Same-name constructors on different types returning Self (can't be shared)
/// - Display/Debug trait fmt() implementations
fn should_skip_near_duplicate(node_a: &GraphNode, node_b: &GraphNode) -> bool {
    let (func_a, func_b) = match (node_a, node_b) {
        (GraphNode::Function(a), GraphNode::Function(b)) => (a, b),
        _ => return false,
    };

    // Only apply to Rust code
    if func_a.language != crate::types::Language::Rust
        || func_b.language != crate::types::Language::Rust
    {
        return false;
    }

    let different_types = func_a.class_context.is_some()
        && func_b.class_context.is_some()
        && func_a.class_context != func_b.class_context;

    if different_types {
        // FP #1: Both are trait impl methods of the same trait on different types.
        // e.g., `impl DataSource for MLB` and `impl DataSource for NBA` both have `sport_id()`.
        let same_trait_impl = func_a.context_type.as_deref() == Some("impl_item")
            && func_b.context_type.as_deref() == Some("impl_item")
            && func_a.context.is_some()
            && func_a.context == func_b.context
            && func_a.name == func_b.name;
        if same_trait_impl {
            return true;
        }

        // FP #6: Same-name constructors/factory methods on different types that return Self.
        // e.g., `ESPNApiClient::football()` and `ESPNGameScraper::football()`.
        if func_a.name == func_b.name {
            let a_returns_self = func_a.source.as_deref().is_some_and(|s| s.contains("Self"));
            let b_returns_self = func_b.source.as_deref().is_some_and(|s| s.contains("Self"));
            if a_returns_self && b_returns_self {
                return true;
            }
        }
    }

    false
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 2: Near-duplicates (normalized source comparison)
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn find_near_duplicates(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    let annotated: Vec<(NodeIndex, &str, &str, HashSet<&str>, usize, &GraphNode)> = ctx.functions
        .iter()
        .filter_map(|&(idx, node)| {
            let src = node.source_snippet()?;
            let line_count = src.lines().count();
            if line_count < ctx.config.min_lines {
                return None;
            }
            let name = node.name();
            let token_set = normalize_tokens_set(src);
            Some((idx, name, src, token_set, line_count, node))
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
                // Check for Rust-specific false positives before grouping
                if should_skip_near_duplicate(&annotated[i].5, &annotated[j].5) {
                    continue;
                }
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
                    "near-duplicate ({:.0}%): {} — likely cosmetic differences only",
                    sim * 100.0, names.join(", "),
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
    let annotated: Vec<(NodeIndex, &str, HashSet<&str>, usize, &GraphNode)> = ctx.functions
        .iter()
        .filter_map(|&(idx, node)| {
            let src = node.source_snippet()?;
            let line_count = src.lines().count();
            if line_count < ctx.config.min_lines {
                return None;
            }
            let token_set = extract_tokens_set(src);
            Some((idx, node.name(), token_set, line_count, node))
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
                if should_skip_near_duplicate(&annotated[i].4, &annotated[j].4) {
                    continue;
                }
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
                    "structurally similar ({:.0}%): {} — consider shared abstraction",
                    sim * 100.0, names.join(", "),
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
                            "`{}` and `{}` share {:.0}% lines — merge with a parameter to select behavior",
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
                "`{}`: {}L, cc={}, ~{} sections — consider splitting",
                func.name, line_count, complexity, sections,
            ),
        });
    }
}
