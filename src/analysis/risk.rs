
use crate::types::node::GraphNode;
use super::context::AnalysisContext;
use super::types::{Tier, FindingKind, Finding};

// ─────────────────────────────────────────────────────────────────────────────
// Check 74: High risk function (composite score)
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_high_risk_functions(
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

        let line_count = func.source.as_ref().map(|s| s.lines().count()).unwrap_or(0);
        if line_count < 5 {
            continue;
        }

        let fan_in = ctx.caller_count(idx);

        let has_tests = ctx.has_test_coverage(idx) || ctx.caller_indices(idx).iter().any(|&caller_idx| {
            if let Some(GraphNode::Function(cf)) = ctx.graph.get_node(caller_idx) {
                cf.name.starts_with("test") || cf.name.starts_with("test_")
            } else {
                false
            }
        });

        let has_mut = func.source.as_ref().map(|s| {
            s.contains("&mut self") || s.contains("mut ") || s.contains(".set(") || s.contains(".borrow_mut()")
        }).unwrap_or(false);

        let mut factors = Vec::new();

        let complexity_score = (func.cyclomatic_complexity as f64 / 30.0).min(1.0);
        if complexity_score > 0.5 {
            factors.push(format!("high complexity ({})", func.cyclomatic_complexity));
        }

        let line_score = (line_count as f64 / 200.0).min(1.0);
        if line_score > 0.5 {
            factors.push(format!("{} lines", line_count));
        }

        let fan_in_score = (fan_in as f64 / 20.0).min(1.0);
        if fan_in_score > 0.5 {
            factors.push(format!("{} callers", fan_in));
        }

        let todo_score = (func.todo_comments.len() as f64 / 5.0).min(1.0);
        if !func.todo_comments.is_empty() {
            factors.push(format!("{} TODOs", func.todo_comments.len()));
        }

        let test_score = if has_tests { 0.0 } else { 1.0 };
        if !has_tests {
            factors.push("no tests".into());
        }

        let mut_score = if has_mut { 1.0 } else { 0.0 };
        if has_mut {
            factors.push("mutates state".into());
        }

        let risk = complexity_score * 0.25
            + line_score * 0.20
            + fan_in_score * 0.15
            + todo_score * 0.10
            + test_score * 0.20
            + mut_score * 0.10;

        if risk >= ctx.config.risk_score_threshold && factors.len() >= 2 {
            findings.push(Finding {
                tier: Tier::High,
                kind: FindingKind::HighRiskFunction {
                    name: func.name.clone(),
                    risk_score: risk,
                    factors: factors.clone(),
                },
                node_indices: vec![idx.index()],
                description: format!(
                    "`{}`: risk={:.2} — {}.",
                    func.name, risk, factors.join(", ")
                ),
            });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 75: High risk file (composite score)
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_high_risk_files(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    for &(file_idx, file_node) in &ctx.files {
        let fd = match file_node {
            GraphNode::File(f) => f,
            _ => continue,
        };
        if fd.is_dependency || fd.total_lines == 0 {
            continue;
        }

        let child_indices = ctx.children_indices(file_idx);
        let func_count = child_indices.iter().filter(|&&cidx| {
            matches!(ctx.graph.get_node(cidx), Some(GraphNode::Function(_)))
        }).count();
        if func_count < 3 {
            continue;
        }

        // Average complexity of ctx.functions in this file
        let total_complexity: u32 = child_indices.iter().filter_map(|&cidx| {
            if let Some(GraphNode::Function(f)) = ctx.graph.get_node(cidx) { Some(f.cyclomatic_complexity) } else { None }
        }).sum();
        let avg_complexity = total_complexity as f64 / func_count.max(1) as f64;

        let mut factors = Vec::new();

        let size_score = (fd.total_lines as f64 / 1000.0).min(1.0);
        if size_score > 0.3 {
            factors.push(format!("{} lines", fd.total_lines));
        }

        let complexity_factor = (avg_complexity / 15.0).min(1.0);
        if complexity_factor > 0.5 {
            factors.push(format!("avg complexity {:.1}", avg_complexity));
        }

        let comment_ratio = fd.comment_line_count as f64 / fd.total_lines.max(1) as f64;
        let doc_score = if comment_ratio < 0.05 { 1.0 } else { 0.0 };
        if comment_ratio < 0.05 && fd.total_lines > 50 {
            factors.push("low documentation".into());
        }

        let test_file_score = if fd.is_test_file { 0.0 } else { 0.3 };

        let risk = size_score * 0.30
            + complexity_factor * 0.30
            + doc_score * 0.15
            + test_file_score * 0.25;

        if risk >= 0.5 && factors.len() >= 2 {
            findings.push(Finding {
                tier: Tier::Medium,
                kind: FindingKind::HighRiskFile {
                    name: fd.name.clone(),
                    risk_score: risk,
                    factors: factors.clone(),
                },
                node_indices: vec![file_idx.index()],
                description: format!(
                    "`{}` has file risk score {:.2}: {}.",
                    fd.name, risk, factors.join(", ")
                ),
            });
        }
    }
}
