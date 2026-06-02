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

// ─────────────────────────────────────────────────────────────────────────────
// Check: Repeated fully-qualified path — would shorten with a `use`/import alias
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_repeated_qualified_paths(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    use std::collections::HashMap;

    // file -> (importable path -> (count, a representative node index))
    let mut per_file: HashMap<String, HashMap<String, (usize, usize)>> = HashMap::new();

    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };
        let src = match &func.source {
            Some(s) => s,
            None => continue,
        };
        let file = func.path.to_string_lossy().to_string();
        let lower = file.to_lowercase();
        // Skip test/fixture/example files.
        if lower.split('/').any(|seg| {
            matches!(seg, "tests" | "test" | "examples" | "fixtures") || seg.starts_with("test_project")
        }) || lower.ends_with("_test.rs")
            || lower.contains("fixture")
        {
            continue;
        }
        let entry = per_file.entry(file).or_default();
        for path in qualified_paths(src) {
            entry.entry(path).or_insert((0, idx.index())).0 += 1;
        }
    }

    // 3+ uses of a long qualified path is enough to be worth a `use` alias. A path
    // that were already imported would appear in its SHORT form, so a recurring
    // LONG form inherently means it isn't aliased.
    const THRESHOLD: usize = 3;
    for (file, paths) in &per_file {
        let file_name = std::path::Path::new(file)
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| file.clone());
        for (path, &(count, node_idx)) in paths {
            if count >= THRESHOLD {
                findings.push(Finding {
                    tier: Tier::Low,
                    kind: FindingKind::RepeatedQualifiedPath {
                        path: path.clone(),
                        count,
                        file_name: file_name.clone(),
                    },
                    node_indices: vec![node_idx],
                    description: format!(
                        "`{}` is written out {} times in `{}` — add `use {};` and use the short name.",
                        path, count, file_name, path,
                    ),
                });
            }
        }
    }
}

/// Extract fully-qualified paths (`a::b::c`, 3+ segments) from source, each
/// reduced to its importable prefix (truncated after the last Type-cased segment;
/// an all-lowercase path is itself the importable function/module).
fn qualified_paths(src: &str) -> Vec<String> {
    let b = src.as_bytes();
    let n = b.len();
    let is_ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        let start_ok = (b[i].is_ascii_alphabetic() || b[i] == b'_')
            && (i == 0 || (!is_ident(b[i - 1]) && b[i - 1] != b':'));
        if !start_ok {
            i += 1;
            continue;
        }
        let start = i;
        let mut segments = 0;
        loop {
            let seg_start = i;
            while i < n && is_ident(b[i]) {
                i += 1;
            }
            if i == seg_start {
                break;
            }
            segments += 1;
            if i + 1 < n && b[i] == b':' && b[i + 1] == b':' {
                i += 2;
            } else {
                break;
            }
        }
        if segments >= 3 {
            let path = &src[start..i];
            let first = path.split("::").next().unwrap_or("");
            // self::/Self::/super:: refer to local scope — not import-worthy.
            if !matches!(first, "self" | "Self" | "super") {
                out.push(importable_prefix(path));
            }
        }
    }
    out
}

/// The importable unit of a path: everything up to and including the last
/// Type-cased (uppercase-initial) segment, e.g. `std::collections::HashMap::new`
/// → `std::collections::HashMap`. An all-lowercase path is returned whole.
fn importable_prefix(path: &str) -> String {
    let segs: Vec<&str> = path.split("::").collect();
    // First Type-cased segment is the type; later segments are usually
    // variants/methods/associated items (`Error::Graph`, `HashMap::new`).
    match segs
        .iter()
        .position(|s| s.chars().next().is_some_and(|c| c.is_ascii_uppercase()))
    {
        Some(pos) => segs[..=pos].join("::"),
        None => path.to_string(),
    }
}
