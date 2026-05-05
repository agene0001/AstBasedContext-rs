use crate::types::EdgeKind;
use crate::types::node::GraphNode;
use std::collections::HashSet;
use super::context::AnalysisContext;
use super::types::{Tier, FindingKind, Finding};

// ─────────────────────────────────────────────────────────────────────────────
// Check 70: Lack of cohesion (LCOM)
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_lack_of_cohesion(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // LCOM4 simplified: for each class, count pairs of methods that share
    // no instance field access. High LCOM = low cohesion = class should be split.
    for &(idx, node) in &ctx.classes {
        let class_data = match node {
            GraphNode::Class(d) => d,
            _ => continue,
        };

        let field_names: HashSet<&str> = class_data.fields.iter().map(|f| f.name.as_str()).collect();
        if field_names.is_empty() {
            continue;
        }

        // Pre-build field access patterns once per class to avoid format! per (method, field) pair
        let field_patterns: Vec<(&str, String, String, String)> = field_names.iter()
            .map(|&field| (
                field,
                format!("self.{}", field),
                format!("this.{}", field),
                format!("self->{}", field),
            ))
            .collect();

        let methods: Vec<(String, HashSet<String>)> = ctx.children_indices(idx)
            .iter()
            .filter_map(|&child_idx| {
                let n = ctx.graph.get_node(child_idx)?;
                if let GraphNode::Function(f) = n {
                    if f.name.starts_with("__") && f.name != "__init__" {
                        return None;
                    }
                    // Find which fields this method references (from source)
                    let accessed: HashSet<String> = if let Some(src) = &f.source {
                        field_patterns.iter()
                            .filter(|(_, self_pat, this_pat, arrow_pat)|
                                src.contains(self_pat.as_str())
                                || src.contains(this_pat.as_str())
                                || src.contains(arrow_pat.as_str()))
                            .map(|(field, _, _, _)| field.to_string())
                            .collect()
                    } else {
                        HashSet::new()
                    };
                    Some((f.name.clone(), accessed))
                } else {
                    None
                }
            })
            .collect();

        if methods.len() < 3 {
            continue;
        }

        // Count pairs with no shared fields
        let mut no_shared = 0usize;
        let mut total_pairs = 0usize;
        for i in 0..methods.len() {
            for j in (i + 1)..methods.len() {
                total_pairs += 1;
                if methods[i].1.is_disjoint(&methods[j].1) {
                    no_shared += 1;
                }
            }
        }

        if total_pairs == 0 {
            continue;
        }

        let lcom = no_shared as f64 / total_pairs as f64;

        if lcom >= 0.7 && methods.len() >= 4 {
            findings.push(Finding {
                tier: Tier::Medium,
                kind: FindingKind::LackOfCohesion {
                    class_name: class_data.name.clone(),
                    lcom_score: lcom,
                    method_count: methods.len(),
                },
                node_indices: vec![idx.index()],
                description: format!(
                    "`{}`: LCOM={:.2}, {} methods, {:.0}% pairs share no fields — split class.",
                    class_data.name, lcom, methods.len(), lcom * 100.0
                ),
            });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 71: High coupling (CBO)
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_high_coupling(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    const CBO_THRESHOLD: usize = 10;

    for &(idx, node) in &ctx.classes {
        if !matches!(node, GraphNode::Class(_)) {
            continue;
        }

        // Count distinct classes this class depends on (via method calls)
        let method_indices = ctx.children_indices(idx);
        let mut coupled_classes: HashSet<String> = HashSet::new();

        for &m_idx in method_indices {
            for &callee_idx in ctx.callee_indices(m_idx) {
                if let Some(GraphNode::Function(cf)) = ctx.graph.get_node(callee_idx) {
                    if let Some(ref cc) = cf.class_context {
                        if cc != node.name() {
                            coupled_classes.insert(cc.clone());
                        }
                    }
                }
            }
        }

        if coupled_classes.len() >= CBO_THRESHOLD {
            findings.push(Finding {
                tier: Tier::Medium,
                kind: FindingKind::HighCoupling {
                    class_name: node.name().to_string(),
                    coupled_classes: coupled_classes.len(),
                },
                node_indices: vec![idx.index()],
                description: format!(
                    "`{}`: CBO={} — high coupling makes changes risky.",
                    node.name(), coupled_classes.len()
                ),
            });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 72: Module instability
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_module_instability(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // For each file, compute afferent (incoming) and efferent (outgoing) coupling
    // Instability = efferent / (afferent + efferent)
    // Instability 1.0 = all deps are outgoing (maximally unstable)
    for &(file_idx, file_node) in &ctx.files {
        let efferent = ctx.graph.graph
            .edges_directed(file_idx, petgraph::Direction::Outgoing)
            .filter(|e| matches!(e.weight(), EdgeKind::Imports { .. }))
            .count();

        let afferent = ctx.graph.graph
            .edges_directed(file_idx, petgraph::Direction::Incoming)
            .filter(|e| matches!(e.weight(), EdgeKind::Imports { .. }))
            .count();

        let total = afferent + efferent;
        if total < 5 {
            continue;
        }

        let instability = efferent as f64 / total as f64;

        // Flag modules that are highly unstable AND have high efferent coupling
        if instability >= 0.8 && efferent >= 8 {
            findings.push(Finding {
                tier: Tier::Low,
                kind: FindingKind::ModuleInstability {
                    file_name: file_node.name().to_string(),
                    afferent,
                    efferent,
                    instability,
                },
                node_indices: vec![file_idx.index()],
                description: format!(
                    "`{}`: instability={:.2} ({} out, {} in deps) — many outgoing deps, few incoming.",
                    file_node.name(), instability, efferent, afferent
                ),
            });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 73: Cognitive complexity
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_cognitive_complexity(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Cognitive complexity: penalizes nesting more than cyclomatic.
    // We approximate from source: each control flow keyword at nesting depth N adds (1 + N).
    const THRESHOLD: u32 = 25;

    let control_keywords = ["if ", "else ", "elif ", "for ", "while ", "switch ", "case ",
        "catch ", "except ", "match ", "? "];
    // Pre-build "} keyword" patterns to avoid format!() per keyword per line
    let brace_keywords: Vec<String> = control_keywords.iter().map(|kw| format!("}} {}", kw)).collect();

    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };

        let src = match &func.source {
            Some(s) => s,
            None => continue,
        };

        let mut score: u32 = 0;
        let base_indent = src.lines().next()
            .map(|l| l.len() - l.trim_start().len())
            .unwrap_or(0);

        for line in src.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('#') {
                continue;
            }

            // Estimate nesting from indentation
            let indent = line.len() - line.trim_start().len();
            let relative_indent = indent.saturating_sub(base_indent);
            let nesting = (relative_indent / 4) as u32; // Assume 4-space indent

            // Check for control flow keywords
            for (kw, brace_kw) in control_keywords.iter().zip(brace_keywords.iter()) {
                if trimmed.starts_with(kw) || trimmed.starts_with(brace_kw.as_str()) {
                    score += 1 + nesting;
                    break;
                }
            }

            // Bonus for boolean operators in conditions (adds cognitive load)
            let bool_ops = trimmed.matches(" && ").count() + trimmed.matches(" || ").count()
                + trimmed.matches(" and ").count() + trimmed.matches(" or ").count();
            score += bool_ops as u32;
        }

        if score >= THRESHOLD {
            findings.push(Finding {
                tier: if score >= 50 { Tier::High } else { Tier::Medium },
                kind: FindingKind::HighCognitiveComplexity {
                    function_name: func.name.clone(),
                    score,
                },
                node_indices: vec![idx.index()],
                description: format!(
                    "`{}`: cognitive complexity {} (threshold {}) — deeply nested or branching logic.",
                    func.name, score, THRESHOLD,
                ),
            });
        }
    }
}
