use crate::types::node::GraphNode;
use super::context::AnalysisContext;
use super::types::{Finding, FindingKind, Tier};

// ─────────────────────────────────────────────────────────────────────────────
// Check 100: Unused imports — imported symbol not referenced in file
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_unused_imports(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // For each file, collect its imports and check if any imported name
    // appears in the source of functions/classes in that file.
    for &(file_idx, file_node) in &ctx.files {
        let file_data = match file_node {
            GraphNode::File(f) => f,
            _ => continue,
        };

        // Collect all source text from children of this file
        let children = ctx.children_indices(file_idx);
        let mut all_source = String::new();
        let mut import_names: Vec<(String, String)> = Vec::new(); // (module, name)

        for &child_idx in children {
            let child = match ctx.graph.get_node(child_idx) {
                Some(n) => n,
                None => continue,
            };
            match child {
                GraphNode::Function(f) => {
                    if let Some(ref src) = f.source {
                        all_source.push_str(src);
                        all_source.push('\n');
                    }
                }
                GraphNode::Class(c) => {
                    if let Some(ref src) = c.source {
                        all_source.push_str(src);
                        all_source.push('\n');
                    }
                }
                GraphNode::Variable(v) => {
                    all_source.push_str(&v.name);
                    all_source.push('\n');
                }
                _ => {}
            }
        }

        // Collect imports for this file by checking IMPORTS edges
        for &child_idx in children {
            if let Some(GraphNode::Module(m)) = ctx.graph.get_node(child_idx) {
                // Module nodes from imports
                let name = m.name.clone();
                let short = name.rsplit(&['.', '/', ':']).next().unwrap_or(&name).to_string();
                import_names.push((name, short));
            }
        }

        // Also check the graph's import edges from this file
        use petgraph::visit::EdgeRef;
        use crate::types::EdgeKind;
        for edge in ctx.graph.graph.edges(file_idx) {
            if matches!(edge.weight(), EdgeKind::Imports { .. }) {
                if let Some(target_node) = ctx.graph.get_node(edge.target()) {
                    let name = target_node.name().to_string();
                    let short = name.rsplit(&['.', '/', ':']).next().unwrap_or(&name).to_string();
                    import_names.push((name, short));
                }
            }
        }

        if import_names.is_empty() || all_source.is_empty() {
            continue;
        }

        for (module, short_name) in &import_names {
            // Check if the short name appears anywhere in the file's source
            if !all_source.contains(short_name.as_str()) {
                findings.push(Finding {
                    tier: Tier::Low,
                    kind: FindingKind::UnusedImport {
                        module_name: module.clone(),
                        import_name: short_name.clone(),
                    },
                    node_indices: vec![file_idx.index()],
                    description: format!(
                        "Import `{}` in {} unused — remove it.",
                        module,
                        file_data.path.display(),
                    ),
                });
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 101: Inconsistent error handling — mixed patterns in the same file
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_inconsistent_error_handling(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    for &(file_idx, file_node) in &ctx.files {
        let file_data = match file_node {
            GraphNode::File(f) => f,
            _ => continue,
        };

        let children = ctx.children_indices(file_idx);
        let mut patterns_found = Vec::new();
        let mut has_result = false;
        let mut has_unwrap = false;
        let mut has_expect = false;
        let mut has_panic = false;
        let mut has_try_catch = false;
        let mut has_throw = false;
        let mut has_error_return = false;

        for &child_idx in children {
            let src = match ctx.graph.get_node(child_idx).and_then(|n| n.source_snippet()) {
                Some(s) => s,
                None => continue,
            };

            if src.contains(".unwrap()") { has_unwrap = true; }
            if src.contains(".expect(") { has_expect = true; }
            if src.contains("panic!(") || src.contains("panic(") { has_panic = true; }
            if src.contains("Result<") || src.contains("-> Result") { has_result = true; }
            if src.contains("try {") || src.contains("try:") || src.contains("try!(") { has_try_catch = true; }
            if src.contains("catch ") || src.contains("except ") || src.contains("except:") { has_try_catch = true; }
            if src.contains("throw ") || src.contains("raise ") { has_throw = true; }
            if src.contains("if err != nil") || src.contains("return Err(") { has_error_return = true; }
        }

        if has_result { patterns_found.push("Result/? operator".to_string()); }
        if has_unwrap || has_expect { patterns_found.push("unwrap/expect".to_string()); }
        if has_panic { patterns_found.push("panic".to_string()); }
        if has_try_catch { patterns_found.push("try/catch".to_string()); }
        if has_throw { patterns_found.push("throw/raise".to_string()); }
        if has_error_return { patterns_found.push("error return".to_string()); }

        // Only flag if 3+ different patterns are mixed (some mixing is normal)
        if patterns_found.len() >= 3 {
            findings.push(Finding {
                tier: Tier::Low,
                kind: FindingKind::InconsistentErrorHandling {
                    file_name: file_data.path.display().to_string(),
                    patterns_found: patterns_found.clone(),
                },
                node_indices: vec![file_idx.index()],
                description: format!(
                    "{}: {} mixed error patterns ({}) — standardize.",
                    file_data.path.display(),
                    patterns_found.len(),
                    patterns_found.join(", "),
                ),
            });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 102: TODO/FIXME/HACK comments — tech debt markers
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_tech_debt_comments(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    let markers = ["TODO", "FIXME", "HACK", "XXX", "WORKAROUND"];

    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };

        let src = match &func.source {
            Some(s) => s,
            None => continue,
        };

        for line in src.lines() {
            let trimmed = line.trim();
            // Only check actual comments
            let is_comment = trimmed.starts_with("//")
                || trimmed.starts_with('#')
                || trimmed.starts_with("/*")
                || trimmed.starts_with('*')
                || trimmed.starts_with("--");

            if !is_comment {
                continue;
            }

            let upper = trimmed.to_uppercase();
            for marker in &markers {
                if upper.contains(marker) {
                    let comment_text = trimmed
                        .trim_start_matches("//")
                        .trim_start_matches('#')
                        .trim_start_matches("/*")
                        .trim_start_matches('*')
                        .trim_start_matches("--")
                        .trim();

                    let tier = match *marker {
                        "FIXME" | "HACK" | "XXX" => Tier::Medium,
                        _ => Tier::Low,
                    };

                    findings.push(Finding {
                        tier,
                        kind: FindingKind::TechDebtComment {
                            function_name: func.name.clone(),
                            marker: marker.to_string(),
                            comment_text: comment_text.chars().take(100).collect(),
                        },
                        node_indices: vec![idx.index()],
                        description: format!(
                            "{} in `{}`: {}",
                            marker,
                            func.name,
                            comment_text.chars().take(100).collect::<String>(),
                        ),
                    });
                    break; // one finding per line
                }
            }
        }
    }
}
