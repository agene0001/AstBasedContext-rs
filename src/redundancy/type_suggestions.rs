use crate::types::node::GraphNode;
use petgraph::graph::NodeIndex;
use std::collections::HashMap;
use std::collections::HashSet;
use super::context::AnalysisContext;
use super::types::{Tier, FindingKind, Finding};

// ─────────────────────────────────────────────────────────────────────────────
// Check 8: Suggest parameter structs — functions sharing many params
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn suggest_parameter_structs(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Only consider ctx.functions with 4+ params
    let candidates: Vec<(NodeIndex, &str, &[String])> = ctx.functions
        .iter()
        .filter_map(|&(idx, node)| {
            if let GraphNode::Function(f) = node {
                if f.args.len() >= 4 {
                    Some((idx, f.name.as_str(), f.args.as_slice()))
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    // Group ctx.functions by their parameter set (using a sorted param key)
    let mut param_groups: HashMap<Vec<String>, Vec<(NodeIndex, &str)>> = HashMap::new();
    for &(idx, name, args) in &candidates {
        let mut key: Vec<String> = args.to_vec();
        key.sort();
        param_groups.entry(key).or_default().push((idx, name));
    }

    for (params, group) in &param_groups {
        if group.len() < 2 || params.len() < 4 {
            continue;
        }

        let names: Vec<String> = group.iter().map(|(_, n)| n.to_string()).collect();
        let indices: Vec<usize> = group.iter().map(|(idx, _)| idx.index()).collect();

        findings.push(Finding {
            tier: if params.len() >= 5 { Tier::Medium } else { Tier::Low },
            kind: FindingKind::SuggestParameterStruct {
                function_names: names.clone(),
                shared_params: params.clone(),
            },
            node_indices: indices,
            description: format!(
                "Functions {} all take the same {} parameters ({}). \
                 Consider grouping these into a config/options struct.",
                names.join(", "),
                params.len(),
                params.join(", "),
            ),
        });
    }

    // Also check for partial overlap: ctx.functions sharing 4+ params even if they have different extras
    // Pre-compute param sets to avoid per-pair allocation
    let param_sets: Vec<HashSet<&str>> = candidates.iter()
        .map(|(_, _, params)| params.iter().map(|s| s.as_str()).collect())
        .collect();
    let mut checked_pairs: HashSet<(usize, usize)> = HashSet::new();
    for i in 0..candidates.len() {
        let params_a = &param_sets[i];
        for j in (i + 1)..candidates.len() {
            if checked_pairs.contains(&(i, j)) {
                continue;
            }
            let params_b = &param_sets[j];
            let shared: Vec<&str> = params_a.intersection(params_b).copied().collect();

            if shared.len() >= 4 {
                checked_pairs.insert((i, j));
                let names = vec![
                    candidates[i].1.to_string(),
                    candidates[j].1.to_string(),
                ];
                // Skip if already caught by exact-match grouping above
                let mut key_a: Vec<String> = candidates[i].2.to_vec();
                key_a.sort();
                let mut key_b: Vec<String> = candidates[j].2.to_vec();
                key_b.sort();
                if key_a == key_b {
                    continue;
                }

                let shared_params: Vec<String> = shared.iter().map(|s| s.to_string()).collect();
                let indices = vec![candidates[i].0.index(), candidates[j].0.index()];

                findings.push(Finding {
                    tier: Tier::Low,
                    kind: FindingKind::SuggestParameterStruct {
                        function_names: names.clone(),
                        shared_params: shared_params.clone(),
                    },
                    node_indices: indices,
                    description: format!(
                        "Functions {} share {} parameters ({}). \
                         Consider grouping the common parameters into a struct.",
                        names.join(" and "),
                        shared_params.len(),
                        shared_params.join(", "),
                    ),
                });
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 9: Suggest enum dispatch — boolean/flag params that control branching
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn suggest_enum_dispatch(
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

        // Look for boolean/flag-like parameters
        let flag_params: Vec<String> = func
            .args
            .iter()
            .filter(|arg| {
                let lower = arg.to_lowercase();
                // Heuristic: param names suggesting boolean/mode flags
                lower.starts_with("is_")
                    || lower.starts_with("use_")
                    || lower.starts_with("enable_")
                    || lower.starts_with("disable_")
                    || lower.starts_with("should_")
                    || lower.starts_with("has_")
                    || lower.starts_with("allow_")
                    || lower.starts_with("no_")
                    || lower.ends_with("_mode")
                    || lower.ends_with("_type")
                    || lower.ends_with("_kind")
                    || lower.ends_with("_flag")
                    || lower == "mode"
                    || lower == "kind"
                    || lower == "verbose"
                    || lower == "debug"
                    || lower == "dry_run"
                    || lower == "force"
                    || lower == "strict"
                    || lower == "recursive"
            })
            .cloned()
            .collect();

        if flag_params.is_empty() {
            continue;
        }

        // Only flag if the function is non-trivial and has branching
        if func.cyclomatic_complexity < 3 {
            continue;
        }

        // Check if the flag params appear in conditionals in the source
        // Pre-build search strings to avoid format!() per param per pattern
        let has_branching_on_flag = flag_params.iter().any(|param| {
            let p = param.as_str();
            let patterns = [
                ["if ", p, ""].concat(),
                ["if !", p, ""].concat(),
                ["if not ", p, ""].concat(),
                ["if (", p, ")"].concat(),
                ["if (!", p, ")"].concat(),
                ["match ", p, ""].concat(),
                ["switch (", p, ")"].concat(),
                ["switch ", p, ""].concat(),
            ];
            patterns.iter().any(|pat| src.contains(pat.as_str()))
        });

        if !has_branching_on_flag {
            continue;
        }

        findings.push(Finding {
            tier: Tier::Low,
            kind: FindingKind::SuggestEnumDispatch {
                function_name: func.name.clone(),
                flag_params: flag_params.clone(),
            },
            node_indices: vec![idx.index()],
            description: format!(
                "`{}` uses flag parameter(s) {} to control branching (complexity {}). \
                 Consider replacing with an enum type for type-safe dispatch.",
                func.name,
                flag_params.iter().map(|p| format!("`{p}`")).collect::<Vec<_>>().join(", "),
                func.cyclomatic_complexity,
            ),
        });
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 10: Suggest trait extraction — classes/structs with overlapping methods
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn suggest_trait_extraction(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Collect classes/structs and their method names (via CONTAINS edges to Functions)
    let types_with_methods: Vec<(NodeIndex, &str, Vec<String>)> = ctx.graph
        .graph
        .node_indices()
        .filter_map(|idx| {
            let node = &ctx.graph.graph[idx];
            let name = match node {
                GraphNode::Class(c) => c.name.as_str(),
                GraphNode::Struct(s) => s.name.as_str(),
                _ => return None,
            };

            let methods: Vec<String> = ctx.graph
                .get_children(idx)
                .into_iter()
                .filter_map(|(_, child)| {
                    if let GraphNode::Function(f) = child {
                        // Skip constructors and special methods
                        let n = f.name.as_str();
                        if n == "__init__"
                            || n == "__new__"
                            || n == "new"
                            || n == "constructor"
                            || n.starts_with("__")
                        {
                            return None;
                        }
                        Some(f.name.clone())
                    } else {
                        None
                    }
                })
                .collect();

            if methods.len() >= 2 {
                Some((idx, name, methods))
            } else {
                None
            }
        })
        .collect();

    let mut used = vec![false; types_with_methods.len()];

    for i in 0..types_with_methods.len() {
        if used[i] {
            continue;
        }
        let methods_a: HashSet<&str> = types_with_methods[i].2.iter().map(|s| s.as_str()).collect();
        let mut group = vec![i];

        for j in (i + 1)..types_with_methods.len() {
            if used[j] {
                continue;
            }
            let methods_b: HashSet<&str> =
                types_with_methods[j].2.iter().map(|s| s.as_str()).collect();
            let shared: HashSet<&str> = methods_a.intersection(&methods_b).copied().collect();

            // Need at least 3 shared methods for a meaningful trait
            if shared.len() >= 3 {
                group.push(j);
                used[j] = true;
            }
        }

        if group.len() >= 2 {
            used[i] = true;
            let mut common: HashSet<&str> = types_with_methods[group[0]]
                .2
                .iter()
                .map(|s| s.as_str())
                .collect();
            for &gi in &group[1..] {
                let other: HashSet<&str> =
                    types_with_methods[gi].2.iter().map(|s| s.as_str()).collect();
                common = common.intersection(&other).copied().collect();
            }

            if common.len() < 3 {
                continue;
            }

            let shared_methods: Vec<String> = common.iter().map(|s| s.to_string()).collect();
            let names: Vec<String> = group
                .iter()
                .map(|&g| types_with_methods[g].1.to_string())
                .collect();
            let indices: Vec<usize> = group
                .iter()
                .map(|&g| types_with_methods[g].0.index())
                .collect();

            findings.push(Finding {
                tier: if shared_methods.len() >= 5 {
                    Tier::Medium
                } else {
                    Tier::Low
                },
                kind: FindingKind::SuggestTraitExtraction {
                    type_names: names.clone(),
                    shared_methods: shared_methods.clone(),
                },
                node_indices: indices,
                description: format!(
                    "Types {} share {} methods ({}). \
                     Consider extracting a trait/interface for the common behavior.",
                    names.join(", "),
                    shared_methods.len(),
                    shared_methods.join(", "),
                ),
            });
        }
    }
}
