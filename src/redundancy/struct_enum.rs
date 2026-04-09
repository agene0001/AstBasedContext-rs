use crate::types::node::GraphNode;
use petgraph::graph::NodeIndex;
use std::collections::HashSet;
use super::context::AnalysisContext;
use super::helpers::{extract_field_names, normalize_field_name};
use super::types::{Tier, FindingKind, Finding};

// ─────────────────────────────────────────────────────────────────────────────
// Check 6: Overlapping structs — structs with heavily shared field names
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn find_overlapping_structs(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Pre-compute field sets upfront to avoid per-pair HashSet allocation
    let structs: Vec<(NodeIndex, &str, HashSet<String>)> = ctx.graph
        .graph
        .node_indices()
        .filter_map(|idx| {
            let node = &ctx.graph.graph[idx];
            let src = node.source_snippet()?;
            match node {
                GraphNode::Struct(s) => {
                    let fields: HashSet<String> = extract_field_names(src).into_iter()
                        .map(|f| normalize_field_name(&f))
                        .collect();
                    if fields.len() >= 2 {
                        Some((idx, s.name.as_str(), fields))
                    } else {
                        None
                    }
                }
                GraphNode::Class(c) => {
                    let fields: HashSet<String> = extract_field_names(src).into_iter()
                        .map(|f| normalize_field_name(&f))
                        .collect();
                    if fields.len() >= 2 {
                        Some((idx, c.name.as_str(), fields))
                    } else {
                        None
                    }
                }
                _ => None,
            }
        })
        .collect();

    let mut used = vec![false; structs.len()];

    for i in 0..structs.len() {
        if used[i] {
            continue;
        }
        let fields_a = &structs[i].2;
        let mut group = vec![i];

        for j in (i + 1)..structs.len() {
            if used[j] {
                continue;
            }
            let fields_b = &structs[j].2;
            let shared = fields_a.intersection(fields_b).count();
            let union = fields_a.union(fields_b).count().max(1);
            let ratio = shared as f64 / union as f64;

            if ratio >= 0.5 && shared >= 2 {
                group.push(j);
                used[j] = true;
            }
        }

        if group.len() >= 2 {
            used[i] = true;
            // Compute shared fields across the group using retain (in-place)
            let mut common = structs[group[0]].2.clone();
            for &gi in &group[1..] {
                common.retain(|f| structs[gi].2.contains(f));
            }
            let shared_fields: Vec<String> = common.into_iter().collect();
            let names: Vec<String> = group.iter().map(|&g| structs[g].1.to_string()).collect();
            let indices: Vec<usize> = group.iter().map(|&g| structs[g].0.index()).collect();

            let overlap = structs[group[0]].2.intersection(&structs[group[1]].2).count() as f64
                / structs[group[0]].2.union(&structs[group[1]].2).count().max(1) as f64;

            let tier = if overlap >= 0.8 { Tier::High } else { Tier::Medium };

            findings.push(Finding {
                tier,
                kind: FindingKind::OverlappingStructs {
                    names: names.clone(),
                    shared_fields: shared_fields.clone(),
                    overlap_ratio: overlap,
                },
                node_indices: indices,
                description: format!(
                    "{} share {:.0}% fields ({}) — merge, compose, or extract shared base.",
                    names.join(", "),
                    overlap * 100.0,
                    shared_fields.join(", "),
                ),
            });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 7: Overlapping enums — enums with shared variant names
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn find_overlapping_enums(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Pre-compute variant sets upfront
    let enums: Vec<(NodeIndex, &str, HashSet<&str>)> = ctx.graph
        .graph
        .node_indices()
        .filter_map(|idx| {
            let node = &ctx.graph.graph[idx];
            if let GraphNode::Enum(e) = node {
                if e.variants.len() >= 2 {
                    let var_set: HashSet<&str> = e.variants.iter().map(|s| s.as_str()).collect();
                    Some((idx, e.name.as_str(), var_set))
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    let mut used = vec![false; enums.len()];

    for i in 0..enums.len() {
        if used[i] {
            continue;
        }
        let vars_a = &enums[i].2;
        let mut group = vec![i];

        for j in (i + 1)..enums.len() {
            if used[j] {
                continue;
            }
            let vars_b = &enums[j].2;
            let shared = vars_a.intersection(vars_b).count();
            let union = vars_a.union(vars_b).count().max(1);
            let ratio = shared as f64 / union as f64;

            if ratio >= 0.5 && shared >= 2 {
                group.push(j);
                used[j] = true;
            }
        }

        if group.len() >= 2 {
            used[i] = true;
            let mut common: HashSet<&str> = enums[group[0]].2.clone();
            for &gi in &group[1..] {
                common.retain(|v| enums[gi].2.contains(v));
            }
            let shared_variants: Vec<String> = common.iter().map(|s| s.to_string()).collect();
            let names: Vec<String> = group.iter().map(|&g| enums[g].1.to_string()).collect();
            let indices: Vec<usize> = group.iter().map(|&g| enums[g].0.index()).collect();

            let overlap = enums[group[0]].2.intersection(&enums[group[1]].2).count() as f64
                / enums[group[0]].2.union(&enums[group[1]].2).count().max(1) as f64;

            let tier = if overlap >= 0.8 { Tier::High } else { Tier::Medium };

            findings.push(Finding {
                tier,
                kind: FindingKind::OverlappingEnums {
                    names: names.clone(),
                    shared_variants: shared_variants.clone(),
                    overlap_ratio: overlap,
                },
                node_indices: indices,
                description: format!(
                    "{} share {:.0}% variants ({}) — merge or use shared base.",
                    names.join(", "),
                    overlap * 100.0,
                    shared_variants.join(", "),
                ),
            });
        }
    }
}
