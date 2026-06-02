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

    // Cluster functions that share 4+ params (transitively) into connected
    // components, and emit ONE finding per component — not one per pair. A flat
    // pairwise pass explodes O(N²): e.g. ~50 trait methods all sharing
    // (&self, source, root, cursor) would otherwise produce ~1200 findings for
    // what is a single "these share a param signature" observation.
    let param_sets: Vec<HashSet<&str>> = candidates
        .iter()
        .map(|(_, _, params)| params.iter().map(|s| s.as_str()).collect())
        .collect();

    let mut uf = UnionFind::new(candidates.len());
    for i in 0..candidates.len() {
        for j in (i + 1)..candidates.len() {
            if param_sets[i].intersection(&param_sets[j]).count() >= 4 {
                uf.union(i, j);
            }
        }
    }

    let mut components: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..candidates.len() {
        components.entry(uf.find(i)).or_default().push(i);
    }

    for members in components.values() {
        if members.len() < 2 {
            continue;
        }
        // The struct should hold exactly the params common to ALL members. If the
        // component is only loosely connected (transitive links with no shared
        // core of 4+), it isn't a single coherent struct suggestion — skip it.
        let mut common: HashSet<&str> = param_sets[members[0]].clone();
        for &m in &members[1..] {
            common = common.intersection(&param_sets[m]).copied().collect();
        }
        if common.len() < 4 {
            continue;
        }

        let mut params: Vec<String> = common.iter().map(|s| s.to_string()).collect();
        params.sort();
        let names: Vec<String> = members.iter().map(|&m| candidates[m].1.to_string()).collect();
        let indices: Vec<usize> = members.iter().map(|&m| candidates[m].0.index()).collect();

        // Keep the description compact: name a few, count the rest (the full list
        // is in node_indices, rendered as the └─ line).
        let subject = if names.len() <= 4 {
            names.join(", ")
        } else {
            format!("{} and {} more functions", names[..3].join(", "), names.len() - 3)
        };

        findings.push(Finding {
            tier: if params.len() >= 5 { Tier::Medium } else { Tier::Low },
            kind: FindingKind::SuggestParameterStruct {
                function_names: names,
                shared_params: params.clone(),
            },
            node_indices: indices,
            description: format!(
                "{subject} share {} params ({}) — group into a config struct.",
                params.len(),
                params.join(", "),
            ),
        });
    }
}

/// Minimal union-find for clustering functions by shared-parameter relations.
struct UnionFind {
    parent: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        Self { parent: (0..n).collect() }
    }
    fn find(&mut self, x: usize) -> usize {
        if self.parent[x] != x {
            self.parent[x] = self.find(self.parent[x]);
        }
        self.parent[x]
    }
    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.parent[ra] = rb;
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
                "`{}`: flag params {} control branching (cc={}) — use enum for type-safe dispatch.",
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

            let mut shared_methods: Vec<String> = common.iter().map(|s| s.to_string()).collect();
            shared_methods.sort(); // HashSet order is randomized — sort for determinism
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
                    "{} share {} methods ({}) — extract a trait/interface.",
                    names.join(", "),
                    shared_methods.len(),
                    shared_methods.join(", "),
                ),
            });
        }
    }
}
