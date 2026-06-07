
use crate::types::node::GraphNode;
use super::context::AnalysisContext;
use super::types::{Tier, FindingKind, Finding};

// ─────────────────────────────────────────────────────────────────────────────
// Check 76: Untested public function
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_untested_public_functions(
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

        // Only check public ctx.functions
        let is_public = func.visibility.as_deref() == Some("public");
        if !is_public {
            continue;
        }

        // Skip test ctx.functions themselves
        if func.name.starts_with("test") || func.name.starts_with("test_") {
            continue;
        }

        // Skip test/fixture/example files — their functions don't need "tests".
        let path_lower = func.path.to_string_lossy().to_lowercase();
        if path_lower.split('/').any(|seg| {
            matches!(seg, "tests" | "test" | "examples" | "fixtures") || seg.starts_with("test_project")
        }) || path_lower.ends_with("_test.rs")
            || path_lower.contains("fixture")
        {
            continue;
        }

        // Skip trivial ctx.functions (< 5 lines)
        let line_count = func.source.as_ref().map(|s| s.lines().count()).unwrap_or(0);
        if line_count < 5 {
            continue;
        }

        // Raise the bar: only nag about functions with real branching logic to
        // test. A linear public function (cc < 3) rarely warrants a dedicated test,
        // and flagging every one of them is high-volume, low-value noise.
        if func.cyclomatic_complexity < 3 {
            continue;
        }

        // Check for Tests edges (precomputed)
        if ctx.has_test_coverage(idx) {
            continue;
        }

        // Check if any caller is from a test file
        let called_from_test = ctx.caller_indices(idx).iter().any(|&caller_idx| {
            // Walk up to find the file node via precomputed parent_map
            ctx.parent_of(caller_idx)
                .and_then(|parent| ctx.graph.get_node(parent))
                .map(|n| matches!(n, GraphNode::File(fd) if fd.is_test_file))
                .unwrap_or(false)
        });
        if called_from_test {
            continue;
        }

        let caller_count = ctx.caller_indices(idx).len();
        findings.push(Finding {
            tier: Tier::Low, // coverage gaps are informational, not high-severity
            kind: FindingKind::UntestedPublicFunction {
                function_name: func.name.clone(),
                file_name: func.path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default(),
                caller_count,
            },
            node_indices: vec![idx.index()],
            description: format!(
                "Public `{}` in `{}` has no tests ({} callers) — untested public API.",
                func.name,
                func.path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default(),
                caller_count
            ),
        });
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 77: Low test ratio per file
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_low_test_ratio(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    for &(file_idx, file_node) in &ctx.files {
        let fd = match file_node {
            GraphNode::File(f) => f,
            _ => continue,
        };
        if fd.is_dependency || fd.is_test_file {
            continue;
        }

        let func_children: Vec<_> = ctx.children_indices(file_idx).iter().filter_map(|&cidx| {
            if let Some(GraphNode::Function(f)) = ctx.graph.get_node(cidx) {
                if !f.name.starts_with("test") && !f.name.starts_with("test_") {
                    return Some((cidx, f));
                }
            }
            None
        }).collect();

        let function_count = func_children.len();
        if function_count < 3 {
            continue;
        }

        let tested_count = func_children.iter().filter(|(cidx, _)| {
            ctx.has_test_coverage(*cidx)
        }).count();

        let ratio = tested_count as f64 / function_count as f64;
        if ratio < ctx.config.test_ratio_threshold {
            findings.push(Finding {
                tier: Tier::Medium,
                kind: FindingKind::LowTestRatio {
                    file_name: fd.name.clone(),
                    function_count,
                    tested_count,
                    ratio,
                },
                node_indices: vec![file_idx.index()],
                description: format!(
                    "`{}`: {}/{} tested ({:.0}%) — below {:.0}% threshold.",
                    fd.name, tested_count, function_count, ratio * 100.0, ctx.config.test_ratio_threshold * 100.0
                ),
            });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 78: Integration test smell
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_integration_test_smells(
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

        // Only look at test ctx.functions
        let is_test = func.name.starts_with("test") || func.name.starts_with("test_")
            || func.decorators.iter().any(|d| d.contains("test"));
        if !is_test {
            continue;
        }

        // BFS to depth 3 from this test function
        let chain = ctx.graph.get_call_chain(idx, 3);
        let distinct_files: std::collections::HashSet<_> = chain.iter().filter_map(|(_, cn, _)| {
            if let GraphNode::Function(f) = cn {
                Some(f.path.to_string_lossy().to_string())
            } else {
                None
            }
        }).collect();

        if distinct_files.len() >= ctx.config.integration_test_module_threshold {
            findings.push(Finding {
                tier: Tier::Low,
                kind: FindingKind::IntegrationTestSmell {
                    test_name: func.name.clone(),
                    modules_touched: distinct_files.len(),
                },
                node_indices: vec![idx.index()],
                description: format!(
                    "Test `{}` touches {} files — likely integration test disguised as unit test.",
                    func.name, distinct_files.len()
                ),
            });
        }
    }
}
