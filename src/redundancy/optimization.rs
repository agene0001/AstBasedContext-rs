use crate::types::node::GraphNode;
use crate::types::Language;
use super::context::AnalysisContext;
use super::helpers::{extract_receiver, is_loop_start, brace_delta};
use super::types::{Tier, FindingKind, Finding};

use std::collections::HashMap;

// ─────────────────────────────────────────────────────────────────────────────
// Check 103: Clone / allocation in loop
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_clone_in_loop(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    let expensive_patterns = [
        (".clone()", "clone()"),
        (".to_string()", "to_string()"),
        (".to_owned()", "to_owned()"),
        ("format!(", "format!()"),
        ("String::from(", "String::from()"),
        (".to_vec()", "to_vec()"),
    ];

    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };
        let src = match &func.source {
            Some(s) => s,
            None => continue,
        };
        if src.lines().count() < 5 {
            continue;
        }

        let mut in_loop = false;
        let mut loop_depth: i32 = 0;
        let mut brace_at_loop_start: i32 = 0;
        let mut total_brace_depth: i32 = 0;

        for line in src.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('#') {
                continue;
            }

            if is_loop_start(trimmed) {
                if !in_loop {
                    brace_at_loop_start = total_brace_depth;
                }
                in_loop = true;
                loop_depth += 1;
            }

            total_brace_depth += brace_delta(trimmed);

            // Python: detect loop via indentation (for/while with colon)
            if !in_loop && (trimmed.starts_with("for ") || trimmed.starts_with("while "))
                && trimmed.ends_with(':')
            {
                in_loop = true;
                loop_depth += 1;
            }

            if in_loop {
                let mut found = false;
                for &(pattern, label) in &expensive_patterns {
                    if trimmed.contains(pattern) {
                        findings.push(Finding {
                            tier: Tier::Medium,
                            kind: FindingKind::CloneInLoop {
                                function_name: func.name.clone(),
                                pattern: label.to_string(),
                            },
                            node_indices: vec![idx.index()],
                            description: format!(
                                "`{}` in `{}`: `{}` inside loop — consider moving allocation outside or using references.",
                                label, func.name, label,
                            ),
                        });
                        found = true;
                        break;
                    }
                }
                if found { break; } // one finding per function
            }

            // Track when we exit the loop (brace-based languages)
            if in_loop && total_brace_depth <= brace_at_loop_start && loop_depth > 0 {
                loop_depth -= 1;
                if loop_depth == 0 {
                    in_loop = false;
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 104: Redundant collect then iterate
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_redundant_collect_iterate(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Pattern: .collect::<Vec<...>>().iter() or .collect().iter() or
    //          .collect::<Vec<_>>().into_iter() or list(...) then iterate
    let patterns = [
        ".collect().iter()",
        ".collect().into_iter()",
        ".collect().for_each(",
        ".collect::<Vec",
    ];

    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };
        let src = match &func.source {
            Some(s) => s,
            None => continue,
        };

        // Check single-line chains
        for line in src.lines() {
            let trimmed = line.trim();
            for pattern in &patterns {
                if trimmed.contains(pattern) {
                    // For .collect::<Vec, check if followed by .iter()/.into_iter() on same line
                    if *pattern == ".collect::<Vec" {
                        if !trimmed.contains(".iter()") && !trimmed.contains(".into_iter()")
                            && !trimmed.contains(".for_each(")
                            && !trimmed.contains(".len()")
                        {
                            continue;
                        }
                    }
                    findings.push(Finding {
                        tier: Tier::Medium,
                        kind: FindingKind::RedundantCollectIterate {
                            function_name: func.name.clone(),
                            pattern: pattern.to_string(),
                        },
                        node_indices: vec![idx.index()],
                        description: format!(
                            "`{}`: `{}` — remove intermediate `.collect()` and chain iterators directly.",
                            func.name, pattern,
                        ),
                    });
                    break;
                }
            }
        }

        // Check Python: list(generator) then iterate
        let lines: Vec<&str> = src.lines().collect();
        for i in 0..lines.len().saturating_sub(1) {
            let cur = lines[i].trim();
            let next = lines[i + 1].trim();
            // x = list(gen); for item in x:
            if cur.contains(" = list(") && cur.contains("for ") == false {
                if let Some(var) = cur.split('=').next().map(|s| s.trim()) {
                    if next.starts_with(&format!("for ")) && next.contains(&format!(" in {}:", var)) {
                        findings.push(Finding {
                            tier: Tier::Medium,
                            kind: FindingKind::RedundantCollectIterate {
                                function_name: func.name.clone(),
                                pattern: format!("list() → for ... in {}", var),
                            },
                            node_indices: vec![idx.index()],
                            description: format!(
                                "`{}`: `list()` materialized then iterated — iterate the generator directly.",
                                func.name,
                            ),
                        });
                    }
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 105: Repeated map lookup
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_repeated_map_lookup(
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
        if src.lines().count() < 4 {
            continue;
        }

        // Count occurrences of map[key] and map.get(key) patterns
        let mut lookup_counts: HashMap<String, usize> = HashMap::new();

        for line in src.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") || trimmed.starts_with('#') {
                continue;
            }

            // Pattern: var[key] (but not assignment to var[key] = ...)
            // We look for `ident[expr]` that appears in non-assignment context
            let mut pos = 0;
            while pos < trimmed.len() {
                if let Some(bracket_start) = trimmed[pos..].find('[') {
                    let abs_start = pos + bracket_start;
                    if let Some(bracket_end) = trimmed[abs_start..].find(']') {
                        let abs_end = abs_start + bracket_end;
                        let key_expr = &trimmed[abs_start..=abs_end];
                        // Get the receiver
                        let before = &trimmed[..abs_start];
                        let recv_start = before
                            .rfind(|c: char| !c.is_alphanumeric() && c != '_' && c != '.')
                            .map(|i| i + 1)
                            .unwrap_or(0);
                        let recv = &before[recv_start..];
                        if !recv.is_empty() && recv.len() > 1 {
                            let lookup_key = format!("{}{}", recv, key_expr);
                            *lookup_counts.entry(lookup_key).or_default() += 1;
                        }
                        pos = abs_end + 1;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }

            // Pattern: map.get(key) / map.get(&key)
            if let Some(recv) = extract_receiver(trimmed, ".get(") {
                if let Some(paren_start) = trimmed.find(".get(") {
                    if let Some(paren_end) = trimmed[paren_start..].find(')') {
                        let call = &trimmed[paren_start..paren_start + paren_end + 1];
                        let lookup_key = format!("{}{}", recv, call);
                        *lookup_counts.entry(lookup_key).or_default() += 1;
                    }
                }
            }
        }

        // Report any lookup appearing 3+ times
        for (key, count) in &lookup_counts {
            if *count >= 3 {
                findings.push(Finding {
                    tier: Tier::Low,
                    kind: FindingKind::RepeatedMapLookup {
                        function_name: func.name.clone(),
                        key_hint: key.clone(),
                        count: *count,
                    },
                    node_indices: vec![idx.index()],
                    description: format!(
                        "`{}`: `{}` looked up {} times — cache in a local variable.",
                        func.name, key, count,
                    ),
                });
                break; // one per function
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 106: Vec/list created then pushed in loop without pre-sizing
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_vec_no_presize(
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
        if src.lines().count() < 5 {
            continue;
        }

        // Already using with_capacity or pre-sized? Skip
        if src.contains("with_capacity") || src.contains("Vec::with_capacity")
            || src.contains("[None]") || src.contains("vec![0")
        {
            continue;
        }

        // Find empty Vec/list creation patterns
        let py_create = ["= []", "= list()"];

        let mut created_vars: Vec<&str> = Vec::new();

        let lines: Vec<&str> = src.lines().collect();
        for line in &lines {
            let trimmed = line.trim();

            // Rust: let mut x = Vec::new() or let mut x = vec![]
            if (trimmed.contains("Vec::new()") || trimmed.contains("vec![]"))
                && trimmed.contains("let ")
            {
                if let Some(var) = trimmed.split('=').next() {
                    let var = var.trim().trim_start_matches("let ").trim_start_matches("mut ").trim();
                    let var = var.split(':').next().unwrap_or(var).trim();
                    if !var.is_empty() {
                        created_vars.push(var);
                    }
                }
            }

            // Python: x = [] or x = list()
            for pat in &py_create {
                if trimmed.contains(pat) {
                    if let Some(var) = trimmed.split('=').next() {
                        let var = var.trim();
                        if !var.is_empty() && var.chars().all(|c| c.is_alphanumeric() || c == '_') {
                            created_vars.push(var);
                        }
                    }
                }
            }
        }

        if created_vars.is_empty() {
            continue;
        }

        // Check if any created var is pushed to inside a loop
        let mut in_loop = false;
        let mut loop_depth: i32 = 0;
        let mut brace_at_loop_start: i32 = 0;
        let mut total_brace_depth: i32 = 0;
        'lines: for line in &lines {
            let trimmed = line.trim();

            if is_loop_start(trimmed) || ((trimmed.starts_with("for ") || trimmed.starts_with("while ")) && trimmed.ends_with(':')) {
                if !in_loop {
                    brace_at_loop_start = total_brace_depth;
                }
                in_loop = true;
                loop_depth += 1;
            }

            total_brace_depth += brace_delta(trimmed);

            if in_loop {
                for var in &created_vars {
                    let push_pat = format!("{}.push(", var);
                    let append_pat = format!("{}.append(", var);
                    let extend_pat = format!("{}.extend(", var);
                    if trimmed.contains(&push_pat) || trimmed.contains(&append_pat) || trimmed.contains(&extend_pat) {
                        findings.push(Finding {
                            tier: Tier::Low,
                            kind: FindingKind::VecNoPresize {
                                function_name: func.name.clone(),
                                variable_hint: var.to_string(),
                            },
                            node_indices: vec![idx.index()],
                            description: format!(
                                "`{}`: `{}` created empty then grown in loop — use `with_capacity()` to avoid reallocations.",
                                func.name, var,
                            ),
                        });
                        break 'lines;
                    }
                }
            }

            if in_loop && total_brace_depth <= brace_at_loop_start && loop_depth > 0 {
                loop_depth -= 1;
                if loop_depth == 0 {
                    in_loop = false;
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 107: Sort then linear find
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_sort_then_find(
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

        // Already using binary_search? Skip
        if src.contains("binary_search") || src.contains("bisect") {
            continue;
        }

        // Find variables that are sorted then linearly searched
        let mut sorted_vars: Vec<&str> = Vec::new();

        for line in src.lines() {
            let trimmed = line.trim();

            // Rust: var.sort(), var.sort_by(), var.sort_unstable()
            if let Some(recv) = extract_receiver(trimmed, ".sort(")
                .or_else(|| extract_receiver(trimmed, ".sort_by("))
                .or_else(|| extract_receiver(trimmed, ".sort_unstable("))
            {
                sorted_vars.push(recv);
            }

            // Python: var.sort() or sorted(var)
            if let Some(recv) = extract_receiver(trimmed, ".sort(") {
                sorted_vars.push(recv);
            }

            // Now check if a previously sorted var is linearly searched
            for var in &sorted_vars {
                let find_pat = format!("{}.iter().find(", var);
                let position_pat = format!("{}.iter().position(", var);
                let contains_pat = format!("{}.contains(", var);
                let in_pat = format!(" in {}", var);
                if trimmed.contains(&find_pat) || trimmed.contains(&position_pat)
                    || trimmed.contains(&contains_pat)
                    || (trimmed.starts_with("if ") && trimmed.contains(&in_pat))
                {
                    findings.push(Finding {
                        tier: Tier::Medium,
                        kind: FindingKind::SortThenFind {
                            function_name: func.name.clone(),
                            variable_hint: var.to_string(),
                        },
                        node_indices: vec![idx.index()],
                        description: format!(
                            "`{}`: `{}` is sorted then linearly searched — use `.binary_search()` for O(log n) or a BTreeSet.",
                            func.name, var,
                        ),
                    });
                    break;
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 108: Python list concatenation in loop
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_list_concat_in_loop(
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

        // Only relevant for Python/JS (no Rust equivalent since + on Vec doesn't compile)
        if !matches!(func.language, crate::types::Language::Python | crate::types::Language::JavaScript | crate::types::Language::TypeScript) {
            continue;
        }

        let mut in_loop = false;
        let mut loop_indent: usize = 0;

        for line in src.lines() {
            let trimmed = line.trim();
            let indent = line.len() - line.trim_start().len();

            if (trimmed.starts_with("for ") || trimmed.starts_with("while "))
                && trimmed.ends_with(':')
            {
                in_loop = true;
                loop_indent = indent;
                continue;
            }

            // If indentation goes back to or before loop level, we're out
            if in_loop && indent <= loop_indent && !trimmed.is_empty() {
                in_loop = false;
            }

            if in_loop {
                // Pattern: result += [item] or result = result + [item]
                if trimmed.contains(" += [") || trimmed.contains(" += list(") {
                    let var = trimmed.split("+=").next().unwrap_or("").trim();
                    if !var.is_empty() {
                        findings.push(Finding {
                            tier: Tier::Medium,
                            kind: FindingKind::ListConcatInLoop {
                                function_name: func.name.clone(),
                                variable_hint: var.to_string(),
                            },
                            node_indices: vec![idx.index()],
                            description: format!(
                                "`{}`: `{} += [...]` in loop creates new list each iteration — use `.append()` instead.",
                                func.name, var,
                            ),
                        });
                        break;
                    }
                }
                // Pattern: result = result + [item]
                if let Some(eq_pos) = trimmed.find(" = ") {
                    let lhs = trimmed[..eq_pos].trim();
                    let rhs = &trimmed[eq_pos + 3..];
                    if rhs.starts_with(&format!("{} + [", lhs)) || rhs.starts_with(&format!("[")) && rhs.contains(&format!("] + {}", lhs)) {
                        findings.push(Finding {
                            tier: Tier::Medium,
                            kind: FindingKind::ListConcatInLoop {
                                function_name: func.name.clone(),
                                variable_hint: lhs.to_string(),
                            },
                            node_indices: vec![idx.index()],
                            description: format!(
                                "`{}`: `{} = {} + [...]` in loop is O(n²) — use `.append()` instead.",
                                func.name, lhs, lhs,
                            ),
                        });
                        break;
                    }
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 109: Unbounded recursion
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_unbounded_recursion(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    let depth_params = ["depth", "max_depth", "limit", "level", "remaining",
        "max_level", "n", "count", "ttl", "max_retries", "retries"];

    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };
        let src = match &func.source {
            Some(s) => s,
            None => continue,
        };
        if src.lines().count() < 3 {
            continue;
        }

        // Check if function calls itself
        let self_call_patterns = [
            format!("{}(", func.name),
            format!("self.{}(", func.name),
        ];

        let mut calls_self = false;
        for line in src.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") || trimmed.starts_with('#') || trimmed.starts_with("def ")
                || trimmed.starts_with("fn ") || trimmed.starts_with("func ")
            {
                continue;
            }
            for pat in &self_call_patterns {
                if trimmed.contains(pat.as_str()) {
                    calls_self = true;
                    break;
                }
            }
            if calls_self { break; }
        }

        if !calls_self {
            continue;
        }

        // Check if any parameter name suggests a depth bound
        let has_depth_param = func.args.iter().any(|arg| {
            let lower = arg.to_lowercase();
            depth_params.iter().any(|dp| lower.contains(dp))
        });

        if has_depth_param {
            continue;
        }

        // Check if source references a depth-like variable
        let has_depth_check = depth_params.iter().any(|dp| src.contains(dp));
        if has_depth_check {
            continue;
        }

        findings.push(Finding {
            tier: Tier::Low,
            kind: FindingKind::UnboundedRecursion {
                function_name: func.name.clone(),
            },
            node_indices: vec![idx.index()],
            description: format!(
                "`{}`: recursive call with no depth/limit parameter — risk of stack overflow.",
                func.name,
            ),
        });
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 110: SIMD / vectorization candidate
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_vectorization_candidate(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Arithmetic operators that are SIMD-friendly
    let elementwise_ops = [" + ", " - ", " * ", " / ", " += ", " -= ", " *= ", " /= "];
    // Patterns indicating array-indexed access: var[i], var[idx], etc.
    let index_chars: &[char] = &['i', 'j', 'k', 'n', 'x'];

    // Python math function patterns that NumPy can vectorize
    let py_math_calls = [
        "math.sqrt(", "math.sin(", "math.cos(", "math.exp(", "math.log(",
        "math.pow(", "math.fabs(", "math.floor(", "math.ceil(",
        "abs(", "pow(", "round(",
    ];

    // Reduction patterns (accumulator += array element)
    let reduction_keywords = ["sum", "total", "acc", "result", "product", "minimum", "maximum"];

    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };
        let src = match &func.source {
            Some(s) => s,
            None => continue,
        };
        if src.lines().count() < 4 {
            continue;
        }

        // Skip if already using SIMD/vectorized libs
        if src.contains("simd") || src.contains("SIMD") || src.contains("numpy")
            || src.contains("np.") || src.contains("packed_simd")
            || src.contains("std::arch") || src.contains("_mm")
            || src.contains("ndarray") || src.contains("torch")
            || src.contains("tensorflow")
        {
            continue;
        }

        let is_python = matches!(func.language, Language::Python);
        let is_compiled = matches!(func.language,
            Language::Rust | Language::C | Language::Cpp | Language::Go);

        let mut in_loop = false;
        let mut loop_depth: i32 = 0;
        let mut brace_at_loop: i32 = 0;
        let mut total_braces: i32 = 0;
        let mut loop_indent: usize = 0;

        // Counters for vectorization signals within a loop
        let mut array_arith_count = 0usize;
        let mut math_call_count = 0usize;
        let mut reduction_count = 0usize;
        let mut loop_body_lines = 0usize;
        let mut has_branch_in_loop = false;

        for line in src.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('#') {
                continue;
            }

            let indent = line.len() - line.trim_start().len();

            // Detect loop start
            let is_loop = is_loop_start(trimmed)
                || ((trimmed.starts_with("for ") || trimmed.starts_with("while "))
                    && trimmed.ends_with(':'));

            total_braces += brace_delta(trimmed);

            if is_loop {
                if !in_loop {
                    // Set brace_at_loop AFTER counting this line's braces
                    // so `for ... {` puts us at depth inside the loop
                    brace_at_loop = total_braces;
                    loop_indent = indent;
                    array_arith_count = 0;
                    math_call_count = 0;
                    reduction_count = 0;
                    loop_body_lines = 0;
                    has_branch_in_loop = false;
                }
                in_loop = true;
                loop_depth += 1;
                continue;
            }

            // Python: indentation-based loop exit
            if is_python && in_loop && indent <= loop_indent && !trimmed.is_empty() {
                // Emit finding if we had good signals
                if should_suggest_vectorize(array_arith_count, math_call_count,
                    reduction_count, loop_body_lines, has_branch_in_loop)
                {
                    emit_vectorize_finding(findings, idx, &func.name, is_python,
                        array_arith_count, math_call_count, reduction_count);
                }
                in_loop = false;
                loop_depth = 0;
            }

            // Brace-based loop exit (total_braces drops below the depth inside the loop)
            if !is_python && in_loop && total_braces < brace_at_loop && loop_depth > 0 {
                loop_depth -= 1;
                if loop_depth == 0 {
                    if should_suggest_vectorize(array_arith_count, math_call_count,
                        reduction_count, loop_body_lines, has_branch_in_loop)
                    {
                        emit_vectorize_finding(findings, idx, &func.name, is_python,
                            array_arith_count, math_call_count, reduction_count);
                    }
                    in_loop = false;
                }
            }

            if !in_loop {
                continue;
            }

            loop_body_lines += 1;

            // Check for branches in loop (SIMD-unfriendly unless masked)
            if trimmed.starts_with("if ") || trimmed.starts_with("match ")
                || trimmed.starts_with("switch ")
            {
                has_branch_in_loop = true;
            }

            // Check for array-indexed arithmetic: var[i] op var[j] or var[i] op= expr
            let has_indexed_access = trimmed.contains('[')
                && trimmed.chars().any(|c| index_chars.contains(&c));

            if has_indexed_access {
                for op in &elementwise_ops {
                    if trimmed.contains(op) {
                        array_arith_count += 1;
                        break;
                    }
                }
            }

            // Check for reduction patterns: accumulator += array[i] or sum += val
            for kw in &reduction_keywords {
                if trimmed.contains(kw) {
                    for op in &[" += ", " = ", " -= "] {
                        if trimmed.contains(op) {
                            reduction_count += 1;
                            break;
                        }
                    }
                    break;
                }
            }

            // Check for scalar math calls on array elements
            for call in &py_math_calls {
                if trimmed.contains(call) {
                    math_call_count += 1;
                    break;
                }
            }

            // Also check Rust math: .sqrt(), .sin(), .cos(), .exp(), .ln(), .abs(), .powi()
            if is_compiled {
                let math_methods = [".sqrt()", ".sin()", ".cos()", ".exp()", ".ln()",
                    ".abs()", ".powi(", ".powf(", ".floor()", ".ceil()"];
                for m in &math_methods {
                    if trimmed.contains(m) {
                        math_call_count += 1;
                        break;
                    }
                }
            }
        }

        // Handle end-of-function with active loop (Python indentation edge case)
        if in_loop && should_suggest_vectorize(array_arith_count, math_call_count,
            reduction_count, loop_body_lines, has_branch_in_loop)
        {
            emit_vectorize_finding(findings, idx, &func.name, is_python,
                array_arith_count, math_call_count, reduction_count);
        }
    }
}

fn should_suggest_vectorize(
    array_arith: usize,
    math_calls: usize,
    reductions: usize,
    body_lines: usize,
    has_branch: bool,
) -> bool {
    // Need at least some vectorizable work
    let total_signals = array_arith + math_calls + reductions;
    if total_signals < 2 {
        return false;
    }

    // Tight loops are best candidates — if body is huge with branches, less likely
    if body_lines > 15 && has_branch {
        return false;
    }

    // Good candidate: multiple array arithmetic ops, or math calls on elements
    array_arith >= 2 || math_calls >= 2 || (array_arith >= 1 && reductions >= 1)
        || (math_calls >= 1 && reductions >= 1)
}

fn emit_vectorize_finding(
    findings: &mut Vec<Finding>,
    idx: petgraph::graph::NodeIndex,
    func_name: &str,
    is_python: bool,
    array_arith: usize,
    math_calls: usize,
    reductions: usize,
) {
    let pattern = if array_arith > 0 && reductions > 0 {
        "element-wise arithmetic + reduction"
    } else if array_arith >= 2 {
        "element-wise array arithmetic"
    } else if math_calls >= 2 {
        "scalar math on array elements"
    } else if reductions > 0 {
        "reduction accumulation"
    } else {
        "vectorizable arithmetic"
    };

    let suggestion = if is_python {
        "use NumPy vectorized operations instead of a Python loop".to_string()
    } else {
        "consider SIMD intrinsics, `packed_simd`, or verify auto-vectorization with `-C target-cpu=native`".to_string()
    };

    findings.push(Finding {
        tier: Tier::Low,
        kind: FindingKind::SuggestVectorize {
            function_name: func_name.to_string(),
            pattern: pattern.to_string(),
            suggestion: suggestion.clone(),
        },
        node_indices: vec![idx.index()],
        description: format!(
            "`{}`: loop with {} — {}.",
            func_name, pattern, suggestion,
        ),
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 111: Suggest Polars over Pandas
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_suggest_polars(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Slow Pandas patterns and their Polars alternatives
    let slow_patterns: &[(&str, &str)] = &[
        (".iterrows()", "iterrows() is extremely slow — use Polars `iter_rows()` or vectorized expressions"),
        (".itertuples()", "itertuples() iterates row-by-row — Polars expressions are columnar and faster"),
        (".apply(", "apply() runs Python per-row — Polars `map_elements()` or native expressions avoid the overhead"),
        (".append(", "DataFrame.append() copies the entire frame — use `pl.concat()` in Polars"),
        (".groupby(", "Pandas groupby is single-threaded — Polars `group_by()` parallelizes automatically"),
        ("pd.merge(", "pd.merge() — Polars joins are multi-threaded and support lazy evaluation"),
        ("pandas.merge(", "pandas.merge() — Polars joins are multi-threaded and support lazy evaluation"),
    ];

    // General Pandas usage (lower tier — informational)
    let general_patterns: &[(&str, &str)] = &[
        ("pd.read_csv(", "Polars `pl.read_csv()` / `pl.scan_csv()` is significantly faster for large files"),
        ("pandas.read_csv(", "Polars `pl.read_csv()` / `pl.scan_csv()` is significantly faster for large files"),
        ("pd.read_parquet(", "Polars `pl.read_parquet()` / `pl.scan_parquet()` supports predicate pushdown"),
        (".to_csv(", "Polars `write_csv()` is faster; `sink_csv()` supports streaming for large data"),
        ("pd.DataFrame(", "consider Polars `pl.DataFrame()` for better performance on large datasets"),
        ("pandas.DataFrame(", "consider Polars `pl.DataFrame()` for better performance on large datasets"),
    ];

    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };

        // Only check Python files
        if !matches!(func.language, Language::Python) {
            continue;
        }

        let src = match &func.source {
            Some(s) => s,
            None => continue,
        };

        // Skip if already using Polars
        if src.contains("polars") || src.contains("pl.") {
            continue;
        }

        // Must use Pandas
        if !src.contains("pd.") && !src.contains("pandas.") && !src.contains("DataFrame")
            && !src.contains(".iterrows") && !src.contains(".itertuples")
            && !src.contains(".apply(") && !src.contains(".groupby(")
        {
            continue;
        }

        // Check slow patterns first (higher priority)
        for (pattern, suggestion) in slow_patterns {
            if src.contains(pattern) {
                findings.push(Finding {
                    tier: Tier::Medium,
                    kind: FindingKind::SuggestPolars {
                        function_name: func.name.clone(),
                        pattern: pattern.to_string(),
                    },
                    node_indices: vec![idx.index()],
                    description: format!(
                        "`{}`: `{}` — {}.",
                        func.name, pattern, suggestion,
                    ),
                });
                break; // one per function for slow patterns
            }
        }

        // Check general patterns (lower priority)
        for (pattern, suggestion) in general_patterns {
            if src.contains(pattern) {
                findings.push(Finding {
                    tier: Tier::Low,
                    kind: FindingKind::SuggestPolars {
                        function_name: func.name.clone(),
                        pattern: pattern.to_string(),
                    },
                    node_indices: vec![idx.index()],
                    description: format!(
                        "`{}`: `{}` — {}.",
                        func.name, pattern, suggestion,
                    ),
                });
                break; // one per function
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 112: Regex recompile in loop
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_regex_recompile_in_loop(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    let compile_patterns: &[(&str, &str)] = &[
        ("re.compile(", "re.compile()"),
        ("Regex::new(", "Regex::new()"),
        ("re.match(", "re.match()"),
        ("re.search(", "re.search()"),
        ("re.findall(", "re.findall()"),
        ("re.sub(", "re.sub()"),
        ("RegexBuilder::new(", "RegexBuilder::new()"),
        ("Pattern.compile(", "Pattern.compile()"),
        ("new RegExp(", "new RegExp()"),
    ];

    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };
        let src = match &func.source {
            Some(s) => s,
            None => continue,
        };
        if src.lines().count() < 4 {
            continue;
        }

        let lines: Vec<&str> = src.lines().collect();
        let mut in_loop = false;
        let mut loop_indent: Option<usize> = None;
        let mut loop_depth: i32 = 0;
        let mut brace_at_loop_start: i32 = 0;
        let mut total_brace_depth: i32 = 0;
        let mut found = false;

        for line in &lines {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('#') {
                continue;
            }

            let indent = line.len() - line.trim_start().len();

            // Python loop exit via indentation
            if in_loop && loop_indent.is_some() {
                if indent <= loop_indent.unwrap()
                    && !trimmed.starts_with("for ")
                    && !trimmed.starts_with("while ")
                {
                    in_loop = false;
                    loop_indent = None;
                }
            }

            if is_loop_start(trimmed)
                || ((trimmed.starts_with("for ") || trimmed.starts_with("while "))
                    && trimmed.ends_with(':'))
            {
                if !in_loop {
                    brace_at_loop_start = total_brace_depth;
                    loop_indent = Some(indent);
                }
                in_loop = true;
                loop_depth += 1;
            }

            total_brace_depth += brace_delta(trimmed);

            if in_loop {
                for &(pattern, label) in compile_patterns {
                    if trimmed.contains(pattern) {
                        findings.push(Finding {
                            tier: Tier::Medium,
                            kind: FindingKind::RegexRecompileInLoop {
                                function_name: func.name.clone(),
                                pattern: label.to_string(),
                            },
                            node_indices: vec![idx.index()],
                            description: format!(
                                "`{}`: `{}` inside loop — compile regex once outside the loop.",
                                func.name, label,
                            ),
                        });
                        found = true;
                        break;
                    }
                }
                if found {
                    break;
                }
            }

            if in_loop && loop_indent.is_none() && total_brace_depth <= brace_at_loop_start && loop_depth > 0 {
                loop_depth -= 1;
                if loop_depth == 0 {
                    in_loop = false;
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 113: Memoization candidate
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_memoization_candidate(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Look for the same function call with identical arguments appearing 3+ times
    // in a single function body.

    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };
        let src = match &func.source {
            Some(s) => s,
            None => continue,
        };
        if src.lines().count() < 5 {
            continue;
        }

        // Extract full call expressions: `name(args)`
        let mut call_counts: HashMap<String, usize> = HashMap::new();
        for line in src.lines() {
            let trimmed = line.trim();
            // Skip comments
            if trimmed.starts_with("//") || trimmed.starts_with('#') || trimmed.starts_with("*") {
                continue;
            }
            // Find function calls: identifier followed by parenthesized args
            // Simple heuristic: extract `word(...)` patterns
            let bytes = trimmed.as_bytes();
            let mut i = 0;
            while i < bytes.len() {
                // Find start of identifier
                if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
                    let start = i;
                    while i < bytes.len()
                        && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'.')
                    {
                        i += 1;
                    }
                    // Check if followed by '('
                    if i < bytes.len() && bytes[i] == b'(' {
                        let name_part = &trimmed[start..i];
                        // Skip common non-memoizable calls
                        if name_part == "print"
                            || name_part == "println"
                            || name_part == "format"
                            || name_part == "len"
                            || name_part.starts_with("self.")
                            || name_part == "range"
                            || name_part == "enumerate"
                        {
                            i += 1;
                            continue;
                        }
                        // Find matching close paren (simple, no nesting)
                        let paren_start = i;
                        let mut depth = 0i32;
                        while i < bytes.len() {
                            if bytes[i] == b'(' {
                                depth += 1;
                            } else if bytes[i] == b')' {
                                depth -= 1;
                                if depth == 0 {
                                    i += 1;
                                    break;
                                }
                            }
                            i += 1;
                        }
                        if depth == 0 {
                            let full_call = &trimmed[start..i];
                            // Only count calls with non-empty args
                            let args = &trimmed[paren_start + 1..i.saturating_sub(1)].trim();
                            if !args.is_empty() && !args.contains("=") {
                                *call_counts.entry(full_call.to_string()).or_insert(0) += 1;
                            }
                        }
                    }
                } else {
                    i += 1;
                }
            }
        }

        // Report calls that appear 3+ times with identical args
        for (call, count) in &call_counts {
            if *count >= 3 {
                // Extract the function name part
                let callee = call.split('(').next().unwrap_or(call);
                findings.push(Finding {
                    tier: Tier::Low,
                    kind: FindingKind::MemoizationCandidate {
                        function_name: func.name.clone(),
                        callee: callee.to_string(),
                        repeat_count: *count,
                    },
                    node_indices: vec![idx.index()],
                    description: format!(
                        "`{}`: `{}` called {} times with identical args — cache the result in a local variable.",
                        func.name, callee, count,
                    ),
                });
                break; // one per function
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 114: Exception for control flow
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_exception_for_control_flow(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Python: try/except KeyError → use .get()
    // Python: try/except IndexError → check len first
    // Python: try/except StopIteration → use next(iter, default)
    // Rust: .unwrap() in non-test code
    // JS/TS: try/catch for type checking

    let control_flow_patterns: &[(&str, &str)] = &[
        ("except KeyError", "use `.get()` or `in` check instead of catching KeyError"),
        ("except IndexError", "check `len()` or use `.get()` instead of catching IndexError"),
        ("except StopIteration", "use `next(iter, default)` instead of catching StopIteration"),
        ("except ValueError", "validate input before conversion instead of catching ValueError"),
        ("except AttributeError", "use `hasattr()` or check type instead of catching AttributeError"),
        ("catch (TypeError", "validate types before operation instead of catching TypeError"),
        ("catch (RangeError", "check bounds before access instead of catching RangeError"),
    ];

    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };
        let src = match &func.source {
            Some(s) => s,
            None => continue,
        };

        for &(pattern, suggestion) in control_flow_patterns {
            if src.contains(pattern) {
                findings.push(Finding {
                    tier: Tier::Medium,
                    kind: FindingKind::ExceptionForControlFlow {
                        function_name: func.name.clone(),
                        pattern: pattern.to_string(),
                    },
                    node_indices: vec![idx.index()],
                    description: format!(
                        "`{}`: `{}` — {}.",
                        func.name, pattern, suggestion,
                    ),
                });
                break; // one per function
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 115: N+1 query pattern
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_n_plus_one_query(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Look for DB/API call patterns inside loop bodies
    let query_patterns = [
        ".query(", ".execute(", ".fetch(", ".fetch_one(", ".fetch_all(",
        ".find_one(", ".find_by(", ".get_by_id(", ".select(", ".where(",
        "cursor.execute(", "session.query(", ".objects.get(",
        "requests.get(", "requests.post(", "requests.put(",
        "fetch(", "axios.get(", "axios.post(",
        "http.get(", "http.post(",
        ".send_request(", ".api_call(",
    ];

    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };
        let src = match &func.source {
            Some(s) => s,
            None => continue,
        };
        if src.lines().count() < 4 {
            continue;
        }

        let mut in_loop = false;
        let mut loop_indent: Option<usize> = None;
        let mut loop_depth: i32 = 0;
        let mut brace_at_loop_start: i32 = 0;
        let mut total_brace_depth: i32 = 0;
        let mut found = false;

        for line in src.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('#') {
                continue;
            }

            let indent = line.len() - line.trim_start().len();

            // Python loop exit via indentation
            if in_loop && loop_indent.is_some() {
                if indent <= loop_indent.unwrap()
                    && !trimmed.starts_with("for ")
                    && !trimmed.starts_with("while ")
                {
                    in_loop = false;
                    loop_indent = None;
                }
            }

            if is_loop_start(trimmed)
                || ((trimmed.starts_with("for ") || trimmed.starts_with("while "))
                    && trimmed.ends_with(':'))
            {
                if !in_loop {
                    brace_at_loop_start = total_brace_depth;
                    loop_indent = Some(indent);
                }
                in_loop = true;
                loop_depth += 1;
            }

            total_brace_depth += brace_delta(trimmed);

            if in_loop {
                for pattern in &query_patterns {
                    if trimmed.contains(pattern) {
                        findings.push(Finding {
                            tier: Tier::High,
                            kind: FindingKind::NPlusOneQuery {
                                function_name: func.name.clone(),
                                call_pattern: pattern.to_string(),
                            },
                            node_indices: vec![idx.index()],
                            description: format!(
                                "`{}`: `{}` inside loop — potential N+1 query. Batch or prefetch data before the loop.",
                                func.name, pattern,
                            ),
                        });
                        found = true;
                        break;
                    }
                }
                if found {
                    break;
                }
            }

            if in_loop && loop_indent.is_none() && total_brace_depth <= brace_at_loop_start && loop_depth > 0 {
                loop_depth -= 1;
                if loop_depth == 0 {
                    in_loop = false;
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 116: Sync/async conflict
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_sync_async_conflict(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    let blocking_calls: &[(&str, &str)] = &[
        ("time.sleep(", "use `asyncio.sleep()` instead"),
        ("requests.get(", "use `aiohttp` or `httpx` async client"),
        ("requests.post(", "use `aiohttp` or `httpx` async client"),
        ("requests.put(", "use `aiohttp` or `httpx` async client"),
        ("requests.delete(", "use `aiohttp` or `httpx` async client"),
        ("requests.patch(", "use `aiohttp` or `httpx` async client"),
        ("urllib.request.urlopen(", "use async HTTP client"),
        ("open(", "use `aiofiles.open()` for async file I/O"),
        ("subprocess.run(", "use `asyncio.create_subprocess_exec()`"),
        ("subprocess.call(", "use `asyncio.create_subprocess_exec()`"),
        ("os.system(", "use `asyncio.create_subprocess_shell()`"),
        ("std::thread::sleep(", "use `tokio::time::sleep()` instead"),
        ("thread::sleep(", "use `tokio::time::sleep()` instead"),
    ];

    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };
        let src = match &func.source {
            Some(s) => s,
            None => continue,
        };

        // Check if function is async
        let is_async = func.is_async
            || func.decorators.iter().any(|d| d.contains("async"))
            || func.name.contains("async")
            || src.lines().next().map_or(false, |first| {
                let t = first.trim();
                t.starts_with("async ") || t.starts_with("async fn") || t.contains("async def")
            })
            || src.contains("await ");

        if !is_async {
            continue;
        }

        for &(pattern, suggestion) in blocking_calls {
            // Skip `open(` false positive — only flag if it's a standalone call, not part of a method
            if pattern == "open(" {
                // Only match bare `open(`, not `file.open(` or `aiofiles.open(`
                let has_bare_open = src.lines().any(|line| {
                    let t = line.trim();
                    (t.contains(" open(") || t.starts_with("open("))
                        && !t.contains("aiofiles")
                });
                if !has_bare_open {
                    continue;
                }
            }

            if src.contains(pattern) {
                findings.push(Finding {
                    tier: Tier::High,
                    kind: FindingKind::SyncAsyncConflict {
                        function_name: func.name.clone(),
                        blocking_call: pattern.to_string(),
                    },
                    node_indices: vec![idx.index()],
                    description: format!(
                        "`{}` (async): blocking `{}` — {}.",
                        func.name, pattern, suggestion,
                    ),
                });
                break; // one per function
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 117: Repeated format in loop
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_repeated_format_in_loop(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Look for string formatting patterns inside loops that could be hoisted
    let format_patterns = [
        "format!(", "f\"", "f'", "\"{}\".format(", "\"{}\"",
        "String::from(", "str.format(",
    ];

    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };
        let src = match &func.source {
            Some(s) => s,
            None => continue,
        };
        if src.lines().count() < 5 {
            continue;
        }

        let mut in_loop = false;
        let mut loop_indent: Option<usize> = None;
        let mut loop_depth: i32 = 0;
        let mut brace_at_loop_start: i32 = 0;
        let mut total_brace_depth: i32 = 0;
        let mut format_in_loop: HashMap<String, usize> = HashMap::new();
        let mut found = false;

        let check_and_emit = |format_in_loop: &HashMap<String, usize>,
                              findings: &mut Vec<Finding>,
                              func_name: &str,
                              idx: petgraph::graph::NodeIndex| -> bool {
            for (pat, count) in format_in_loop {
                if *count >= 3 {
                    findings.push(Finding {
                        tier: Tier::Low,
                        kind: FindingKind::RepeatedFormatInLoop {
                            function_name: func_name.to_string(),
                            pattern: pat.clone(),
                        },
                        node_indices: vec![idx.index()],
                        description: format!(
                            "`{}`: `{}` used {} times in loop — consider pre-computing or using a template.",
                            func_name, pat, count,
                        ),
                    });
                    return true;
                }
            }
            false
        };

        for line in src.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('#') {
                continue;
            }

            let indent = line.len() - line.trim_start().len();

            // Python loop exit via indentation
            if in_loop && loop_indent.is_some() {
                if indent <= loop_indent.unwrap()
                    && !trimmed.starts_with("for ")
                    && !trimmed.starts_with("while ")
                {
                    found = check_and_emit(&format_in_loop, findings, &func.name, idx);
                    if found { break; }
                    in_loop = false;
                    loop_indent = None;
                    format_in_loop.clear();
                }
            }

            if is_loop_start(trimmed)
                || ((trimmed.starts_with("for ") || trimmed.starts_with("while "))
                    && trimmed.ends_with(':'))
            {
                if !in_loop {
                    brace_at_loop_start = total_brace_depth;
                    loop_indent = Some(indent);
                    format_in_loop.clear();
                }
                in_loop = true;
                loop_depth += 1;
            }

            total_brace_depth += brace_delta(trimmed);

            if in_loop {
                for pattern in &format_patterns {
                    if trimmed.contains(pattern) {
                        *format_in_loop.entry(pattern.to_string()).or_insert(0) += 1;
                    }
                }
            }

            if in_loop && loop_indent.is_none() && total_brace_depth <= brace_at_loop_start && loop_depth > 0 {
                loop_depth -= 1;
                if loop_depth == 0 {
                    in_loop = false;
                    found = check_and_emit(&format_in_loop, findings, &func.name, idx);
                    if found { break; }
                    format_in_loop.clear();
                }
            }
        }

        // If we ended while still in a loop (Python: no dedent at end of function)
        if in_loop && !found {
            check_and_emit(&format_in_loop, findings, &func.name, idx);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 118: Sleep in loop
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_sleep_in_loop(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    let sleep_patterns: &[(&str, &str)] = &[
        ("time.sleep(", "time.sleep()"),
        ("sleep(", "sleep()"),
        ("thread::sleep(", "thread::sleep()"),
        ("std::thread::sleep(", "std::thread::sleep()"),
        ("Thread.sleep(", "Thread.sleep()"),
        ("Sleep(", "Sleep()"),
    ];

    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };
        let src = match &func.source {
            Some(s) => s,
            None => continue,
        };
        if src.lines().count() < 4 {
            continue;
        }

        let mut in_loop = false;
        let mut loop_indent: Option<usize> = None;
        let mut loop_depth: i32 = 0;
        let mut brace_at_loop_start: i32 = 0;
        let mut total_brace_depth: i32 = 0;
        let mut found = false;

        for line in src.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('#') {
                continue;
            }

            let indent = line.len() - line.trim_start().len();

            if in_loop && loop_indent.is_some() {
                if indent <= loop_indent.unwrap()
                    && !trimmed.starts_with("for ")
                    && !trimmed.starts_with("while ")
                {
                    in_loop = false;
                    loop_indent = None;
                }
            }

            if is_loop_start(trimmed)
                || ((trimmed.starts_with("for ") || trimmed.starts_with("while "))
                    && trimmed.ends_with(':'))
            {
                if !in_loop {
                    brace_at_loop_start = total_brace_depth;
                    loop_indent = Some(indent);
                }
                in_loop = true;
                loop_depth += 1;
            }

            total_brace_depth += brace_delta(trimmed);

            if in_loop {
                for &(pattern, label) in sleep_patterns {
                    if trimmed.contains(pattern) {
                        // Skip asyncio.sleep
                        if trimmed.contains("asyncio.sleep") || trimmed.contains("tokio::time::sleep") {
                            continue;
                        }
                        findings.push(Finding {
                            tier: Tier::Medium,
                            kind: FindingKind::SleepInLoop {
                                function_name: func.name.clone(),
                                pattern: label.to_string(),
                            },
                            node_indices: vec![idx.index()],
                            description: format!(
                                "`{}`: `{}` inside loop — busy-wait/polling pattern. Consider event-driven or callback approach.",
                                func.name, label,
                            ),
                        });
                        found = true;
                        break;
                    }
                }
                if found { break; }
            }

            if in_loop && loop_indent.is_none() && total_brace_depth <= brace_at_loop_start && loop_depth > 0 {
                loop_depth -= 1;
                if loop_depth == 0 {
                    in_loop = false;
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 119: Generator over list
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_generator_over_list(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Python: sum([x for x in ...]) → sum(x for x in ...)
    // Also: any([...]), all([...]), min([...]), max([...]), "".join([...])
    let aggregate_patterns = [
        ("sum([", "sum(generator)"),
        ("any([", "any(generator)"),
        ("all([", "all(generator)"),
        ("min([", "min(generator)"),
        ("max([", "max(generator)"),
        (".join([", ".join(generator)"),
        ("sorted([", "sorted(generator)"),
        ("set([", "set(generator)"),
        ("tuple([", "tuple(generator)"),
    ];

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
            for &(pattern, label) in &aggregate_patterns {
                if trimmed.contains(pattern) && trimmed.contains(" for ") {
                    findings.push(Finding {
                        tier: Tier::Low,
                        kind: FindingKind::GeneratorOverList {
                            function_name: func.name.clone(),
                            pattern: label.to_string(),
                        },
                        node_indices: vec![idx.index()],
                        description: format!(
                            "`{}`: `{}` — use a generator expression instead of list comprehension to avoid materializing the full list.",
                            func.name, label,
                        ),
                    });
                    return; // one finding is enough
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 120: Unnecessary iterator chain
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_unnecessary_chain(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    let chain_patterns: &[(&str, &str, &str)] = &[
        // Rust
        (".map(", ".filter(", "consider `.filter_map()` to fuse map + filter"),
        (".filter(", ".map(", "consider `.filter_map()` to fuse filter + map"),
        (".filter(", ".next()", "use `.find()` instead of `.filter().next()`"),
        (".map(", ".flatten()", "use `.flat_map()` instead of `.map().flatten()`"),
        (".map(", ".collect::<Vec", "consider `.map().collect()` — ensure the intermediate is needed"),
        // Python
        ("list(filter(", "list(map(", "use a list comprehension instead of chained filter/map"),
    ];

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
            for &(first, second, suggestion) in chain_patterns {
                if trimmed.contains(first) && trimmed.contains(second) {
                    let pattern = format!("{} ... {}", first.trim_end_matches('('), second.trim_end_matches('('));
                    findings.push(Finding {
                        tier: Tier::Low,
                        kind: FindingKind::UnnecessaryChain {
                            function_name: func.name.clone(),
                            pattern: pattern.clone(),
                            suggestion: suggestion.to_string(),
                        },
                        node_indices: vec![idx.index()],
                        description: format!(
                            "`{}`: `{}` — {}.",
                            func.name, pattern, suggestion,
                        ),
                    });
                    break; // one per function
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 121: Large list membership test
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_large_list_in(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Python: `if x in [a, b, c, ...]` with 4+ elements → use a set
    // Also: `if x in (a, b, c, ...)` tuple with 4+ elements

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

            // Look for `in [` or `in (`
            for marker in &[" in [", " in ("] {
                if let Some(pos) = trimmed.find(marker) {
                    let after = &trimmed[pos + marker.len()..];
                    // Count commas to estimate element count
                    let close = if *marker == " in [" { ']' } else { ')' };
                    if let Some(end) = after.find(close) {
                        let inner = &after[..end];
                        let comma_count = inner.chars().filter(|&c| c == ',').count();
                        if comma_count >= 3 {
                            let container = if *marker == " in [" { "list" } else { "tuple" };
                            findings.push(Finding {
                                tier: Tier::Medium,
                                kind: FindingKind::LargeListIn {
                                    function_name: func.name.clone(),
                                    pattern: format!("in {} with {}+ elements", container, comma_count + 1),
                                },
                                node_indices: vec![idx.index()],
                                description: format!(
                                    "`{}`: membership test on {} literal with {}+ elements — use a `set` literal `{{...}}` for O(1) lookup.",
                                    func.name, container, comma_count + 1,
                                ),
                            });
                            break;
                        }
                    }
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 122: Dict keys iteration
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_dict_keys_iter(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    let patterns: &[(&str, &str)] = &[
        (".keys()", "iterate the dict directly: `for k in d` instead of `for k in d.keys()`"),
        (".values().enumerate()", "use `.items()` instead of `.values().enumerate()`"),
    ];

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
            // Only match `for ... in X.keys():`
            if trimmed.starts_with("for ") {
                for &(pattern, suggestion) in patterns {
                    if trimmed.contains(pattern) {
                        findings.push(Finding {
                            tier: Tier::Low,
                            kind: FindingKind::DictKeysIter {
                                function_name: func.name.clone(),
                                pattern: pattern.to_string(),
                            },
                            node_indices: vec![idx.index()],
                            description: format!(
                                "`{}`: `{}` — {}.",
                                func.name, pattern, suggestion,
                            ),
                        });
                        break;
                    }
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 123: Unclosed resource
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_unclosed_resource(
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

        let is_python = matches!(func.language, Language::Python);
        let is_go = matches!(func.language, Language::Go);

        if is_python {
            // Check for open() without `with`
            for line in src.lines() {
                let trimmed = line.trim();
                // Match `var = open(` but not inside a `with` statement
                if (trimmed.contains("= open(") || trimmed.contains("=open("))
                    && !trimmed.starts_with("with ")
                    && !trimmed.contains("aiofiles")
                {
                    findings.push(Finding {
                        tier: Tier::High,
                        kind: FindingKind::UnclosedResource {
                            function_name: func.name.clone(),
                            pattern: "open() without `with`".to_string(),
                            suggestion: "use `with open(...) as f:` to ensure the file is closed".to_string(),
                        },
                        node_indices: vec![idx.index()],
                        description: format!(
                            "`{}`: `open()` without `with` — use `with open(...) as f:` to ensure the file is closed.",
                            func.name,
                        ),
                    });
                    break;
                }
            }
        } else if is_go {
            // Check for os.Open / os.Create without defer
            let has_open = src.contains("os.Open(") || src.contains("os.Create(") || src.contains("os.OpenFile(");
            let has_defer_close = src.contains("defer ") && src.contains(".Close()");
            if has_open && !has_defer_close {
                findings.push(Finding {
                    tier: Tier::High,
                    kind: FindingKind::UnclosedResource {
                        function_name: func.name.clone(),
                        pattern: "os.Open() without defer Close()".to_string(),
                        suggestion: "add `defer f.Close()` after opening".to_string(),
                    },
                    node_indices: vec![idx.index()],
                    description: format!(
                        "`{}`: file opened without `defer Close()` — resource may leak.",
                        func.name,
                    ),
                });
            }
        } else {
            // Rust/C/C++: check for raw File::open without drop guard
            // This is less common in Rust (RAII), so only flag C/C++ fopen without fclose
            let is_c = matches!(func.language, Language::C | Language::Cpp);
            if is_c {
                let has_fopen = src.contains("fopen(");
                let has_fclose = src.contains("fclose(");
                if has_fopen && !has_fclose {
                    findings.push(Finding {
                        tier: Tier::High,
                        kind: FindingKind::UnclosedResource {
                            function_name: func.name.clone(),
                            pattern: "fopen() without fclose()".to_string(),
                            suggestion: "ensure fclose() is called on all code paths".to_string(),
                        },
                        node_indices: vec![idx.index()],
                        description: format!(
                            "`{}`: `fopen()` without `fclose()` — file handle may leak.",
                            func.name,
                        ),
                    });
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 124: Enumerate vs range(len())
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_enumerate_vs_range_len(
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

        for line in src.lines() {
            let trimmed = line.trim();
            // `for i in range(len(` pattern
            if trimmed.starts_with("for ") && trimmed.contains("in range(len(") {
                findings.push(Finding {
                    tier: Tier::Low,
                    kind: FindingKind::EnumerateVsRangeLen {
                        function_name: func.name.clone(),
                    },
                    node_indices: vec![idx.index()],
                    description: format!(
                        "`{}`: `for i in range(len(...))` — use `for i, val in enumerate(...)` instead.",
                        func.name,
                    ),
                });
                break; // one per function
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 125: yield from
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_yield_from(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Pattern: `for x in iterable:\n    yield x` → `yield from iterable`
    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };
        let src = match &func.source {
            Some(s) => s,
            None => continue,
        };

        let lines: Vec<&str> = src.lines().collect();
        for i in 0..lines.len().saturating_sub(1) {
            let trimmed = lines[i].trim();
            let next_trimmed = lines[i + 1].trim();

            // Match `for VAR in ITERABLE:` followed by `yield VAR`
            if trimmed.starts_with("for ") && trimmed.ends_with(':') {
                if let Some(var) = trimmed.strip_prefix("for ").and_then(|s| s.split(" in ").next()) {
                    let var = var.trim();
                    let expected_yield = format!("yield {}", var);
                    if next_trimmed == expected_yield {
                        findings.push(Finding {
                            tier: Tier::Low,
                            kind: FindingKind::YieldFrom {
                                function_name: func.name.clone(),
                            },
                            node_indices: vec![idx.index()],
                            description: format!(
                                "`{}`: `for {} in ...: yield {}` — use `yield from ...` instead.",
                                func.name, var, var,
                            ),
                        });
                        break; // one per function
                    }
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 126: Append in loop → extend
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_append_in_loop_extend(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Pattern: `for x in items:\n    result.append(x)` → `result.extend(items)`
    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };
        let src = match &func.source {
            Some(s) => s,
            None => continue,
        };

        let lines: Vec<&str> = src.lines().collect();
        for i in 0..lines.len().saturating_sub(1) {
            let trimmed = lines[i].trim();
            let next_trimmed = lines[i + 1].trim();

            // Match `for VAR in ITERABLE:` followed by `SOMETHING.append(VAR)`
            if trimmed.starts_with("for ") && trimmed.ends_with(':') {
                if let Some(var) = trimmed.strip_prefix("for ").and_then(|s| s.split(" in ").next()) {
                    let var = var.trim();
                    if next_trimmed.ends_with(&format!("{})", var))
                        && next_trimmed.contains(".append(")
                    {
                        // Check it's exactly `.append(var)` with nothing else on the line
                        if let Some(receiver) = extract_receiver(next_trimmed, ".append(") {
                            // Make sure the loop body is ONLY the append (next line at higher indent)
                            let loop_indent = lines[i].len() - lines[i].trim_start().len();
                            let body_indent = lines[i + 1].len() - lines[i + 1].trim_start().len();
                            if body_indent > loop_indent {
                                // Check there's no other body line at this indent level
                                let has_more_body = if i + 2 < lines.len() {
                                    let next2 = lines[i + 2];
                                    let next2_indent = next2.len() - next2.trim_start().len();
                                    !next2.trim().is_empty() && next2_indent > loop_indent
                                } else {
                                    false
                                };
                                if !has_more_body {
                                    findings.push(Finding {
                                        tier: Tier::Low,
                                        kind: FindingKind::AppendInLoopExtend {
                                            function_name: func.name.clone(),
                                            variable_hint: receiver.to_string(),
                                        },
                                        node_indices: vec![idx.index()],
                                        description: format!(
                                            "`{}`: `for {} in ...: {}.append({})` — use `{}.extend(...)` instead.",
                                            func.name, var, receiver, var, receiver,
                                        ),
                                    });
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 127: Double with statement
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_double_with_statement(
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

        let lines: Vec<&str> = src.lines().collect();
        for i in 0..lines.len().saturating_sub(1) {
            let trimmed = lines[i].trim();
            let next_trimmed = lines[i + 1].trim();
            let indent = lines[i].len() - lines[i].trim_start().len();
            let next_indent = lines[i + 1].len() - lines[i + 1].trim_start().len();

            // Nested with: `with X:\n    with Y:`
            if trimmed.starts_with("with ") && trimmed.ends_with(':')
                && next_trimmed.starts_with("with ") && next_trimmed.ends_with(':')
                && next_indent > indent
            {
                findings.push(Finding {
                    tier: Tier::Low,
                    kind: FindingKind::DoubleWithStatement {
                        function_name: func.name.clone(),
                    },
                    node_indices: vec![idx.index()],
                    description: format!(
                        "`{}`: nested `with` blocks — combine into `with X, Y:` (Python 3.1+).",
                        func.name,
                    ),
                });
                break; // one per function
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 128: Import in function
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_import_in_function(
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

        // Skip the first line (function def)
        let mut past_def = false;
        for line in src.lines() {
            let trimmed = line.trim();
            if !past_def {
                if trimmed.starts_with("def ") || trimmed.starts_with("async def ") {
                    past_def = true;
                }
                continue;
            }
            // Skip docstrings
            if trimmed.starts_with("\"\"\"") || trimmed.starts_with("'''") {
                continue;
            }

            if (trimmed.starts_with("import ") || trimmed.starts_with("from "))
                && !trimmed.contains("importlib")
            {
                let module = trimmed
                    .strip_prefix("import ")
                    .or_else(|| trimmed.strip_prefix("from ").and_then(|s| s.split_whitespace().next()))
                    .unwrap_or("?")
                    .to_string();

                findings.push(Finding {
                    tier: Tier::Low,
                    kind: FindingKind::ImportInFunction {
                        function_name: func.name.clone(),
                        module_name: module.clone(),
                    },
                    node_indices: vec![idx.index()],
                    description: format!(
                        "`{}`: `import {}` inside function body — move to module level to avoid repeated import overhead.",
                        func.name, module,
                    ),
                });
                break; // one per function
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 129: Constant condition
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_constant_condition(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    let constant_patterns: &[(&str, &str)] = &[
        // Python
        ("if True:", "if True"),
        ("if False:", "if False"),
        ("while True:", "while True"),
        ("while False:", "while False"),
        // Rust / C / Go / JS
        ("if true {", "if true"),
        ("if false {", "if false"),
        ("if (true)", "if (true)"),
        ("if (false)", "if (false)"),
        ("if 1 {", "if 1"),
        ("if 0 {", "if 0"),
        ("if (1)", "if (1)"),
        ("if (0)", "if (0)"),
    ];

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
            for &(pattern, label) in constant_patterns {
                if trimmed.starts_with(pattern) {
                    // Skip `while True:` in Python — it's an idiomatic infinite loop
                    if pattern == "while True:" || pattern == "while true {" || pattern == "loop {" {
                        continue;
                    }
                    findings.push(Finding {
                        tier: Tier::Medium,
                        kind: FindingKind::ConstantCondition {
                            function_name: func.name.clone(),
                            pattern: label.to_string(),
                        },
                        node_indices: vec![idx.index()],
                        description: format!(
                            "`{}`: `{}` — constant condition creates dead or unconditional branch.",
                            func.name, label,
                        ),
                    });
                    break;
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 130: Redundant negation
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_redundant_negation(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    let negation_patterns: &[(&str, &str, &str)] = &[
        // Python
        ("not ", " == ", "use `!=` instead of `not ... ==`"),
        ("not ", " != ", "use `==` instead of `not ... !=`"),
        ("not ", " is ", "use `is not` instead of `not ... is`"),
        ("not ", " in ", "use `not in` instead of `not ... in`"),
        ("not ", " > ", "use `<=` instead of `not ... >`"),
        ("not ", " < ", "use `>=` instead of `not ... <`"),
        // Rust/C/JS
        ("!(", " == ", "use `!=` instead of `!( ... == )`"),
        ("!(", " != ", "use `==` instead of `!( ... != )`"),
        ("!(", " > ", "use `<=` instead of `!( ... > )`"),
        ("!(", " < ", "use `>=` instead of `!( ... < )`"),
    ];

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
            for &(neg_prefix, op, suggestion) in negation_patterns {
                // For Python: `if not x == y`
                // For Rust/C: `if !(x == y)`
                if neg_prefix == "not " {
                    // Look for `not ` followed by an expression containing the operator
                    if let Some(pos) = trimmed.find("not ") {
                        let after_not = &trimmed[pos + 4..];
                        if after_not.contains(op) && !after_not.contains("not ") {
                            let pattern = format!("not ...{}", op.trim());
                            findings.push(Finding {
                                tier: Tier::Low,
                                kind: FindingKind::RedundantNegation {
                                    function_name: func.name.clone(),
                                    pattern: pattern.clone(),
                                    suggestion: suggestion.to_string(),
                                },
                                node_indices: vec![idx.index()],
                                description: format!(
                                    "`{}`: `{}` — {}.",
                                    func.name, pattern, suggestion,
                                ),
                            });
                            break;
                        }
                    }
                } else if neg_prefix == "!(" {
                    if trimmed.contains("!(") {
                        if let Some(pos) = trimmed.find("!(") {
                            let after = &trimmed[pos + 2..];
                            if after.contains(op) {
                                let pattern = format!("!(...{}...)", op.trim());
                                findings.push(Finding {
                                    tier: Tier::Low,
                                    kind: FindingKind::RedundantNegation {
                                        function_name: func.name.clone(),
                                        pattern: pattern.clone(),
                                        suggestion: suggestion.to_string(),
                                    },
                                    node_indices: vec![idx.index()],
                                    description: format!(
                                        "`{}`: `{}` — {}.",
                                        func.name, pattern, suggestion,
                                    ),
                                });
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
}

// ── Check 131: Default dict pattern ─────────────────────────────────────
/// Detects `if key not in d: d[key] = []` patterns that should use
/// `defaultdict` or `.setdefault()`.
pub fn detect_default_dict_pattern(ctx: &AnalysisContext, findings: &mut Vec<Finding>) {
    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };
        let src = match &func.source {
            Some(s) => s,
            None => continue,
        };

        let lines: Vec<&str> = src.lines().collect();
        for i in 0..lines.len().saturating_sub(1) {
            let trimmed = lines[i].trim();

            if !trimmed.starts_with("if ") || !trimmed.contains(" not in ") || !trimmed.ends_with(':') {
                continue;
            }

            let after_if = &trimmed[3..];
            let parts: Vec<&str> = after_if.splitn(2, " not in ").collect();
            if parts.len() != 2 {
                continue;
            }
            let key = parts[0].trim();
            let dict_name = parts[1].trim().trim_end_matches(':').trim();

            let next_trimmed = lines[i + 1].trim();
            let expected_prefix = format!("{}[{}]", dict_name, key);
            if next_trimmed.starts_with(&expected_prefix) && next_trimmed.contains(" = ") {
                let pattern = format!(
                    "if {} not in {}: {}",
                    key, dict_name, next_trimmed
                );
                findings.push(Finding {
                    tier: Tier::Low,
                    kind: FindingKind::DefaultDictPattern {
                        function_name: func.name.clone(),
                        pattern: pattern.clone(),
                    },
                    node_indices: vec![idx.index()],
                    description: format!(
                        "`{}`: `{}` — use `collections.defaultdict` or `{}.setdefault({}, ...)`.",
                        func.name, pattern, dict_name, key,
                    ),
                });
                break;
            }
        }
    }
}

// ── Check 132: Empty string check ───────────────────────────────────────
/// Detects `if s == ""` or `if s != ""` patterns that should use
/// `if not s` / `if s` (Python) or `s.is_empty()` (Rust).
pub fn detect_empty_string_check(ctx: &AnalysisContext, findings: &mut Vec<Finding>) {
    let check_patterns: &[(&str, &str)] = &[
        ("== \"\"", "equality"),
        ("== ''", "equality"),
        ("!= \"\"", "inequality"),
        ("!= ''", "inequality"),
    ];

    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };
        let src = match &func.source {
            Some(s) => s,
            None => continue,
        };

        let mut found = false;
        for line in src.lines() {
            if found {
                break;
            }
            let trimmed = line.trim();
            // Skip comments and docstrings
            if trimmed.starts_with('#')
                || trimmed.starts_with("\"\"\"")
                || trimmed.starts_with("'''")
                || trimmed.starts_with("//")
                || trimmed.starts_with("/*")
            {
                continue;
            }
            if !trimmed.starts_with("if ") && !trimmed.contains("if ") {
                continue;
            }

            for &(pat, kind) in check_patterns {
                if trimmed.contains(pat) {
                    let actual_suggestion = if trimmed.ends_with(':') {
                        if kind == "inequality" {
                            "use truthiness check (`if s:`)"
                        } else {
                            "use falsiness check (`if not s:`)"
                        }
                    } else {
                        if kind == "inequality" {
                            "use `!s.is_empty()`"
                        } else {
                            "use `s.is_empty()`"
                        }
                    };

                    findings.push(Finding {
                        tier: Tier::Low,
                        kind: FindingKind::EmptyStringCheck {
                            function_name: func.name.clone(),
                            pattern: trimmed.to_string(),
                            suggestion: actual_suggestion.to_string(),
                        },
                        node_indices: vec![idx.index()],
                        description: format!(
                            "`{}`: `{}` — {}.",
                            func.name, trimmed, actual_suggestion,
                        ),
                    });
                    found = true;
                    break;
                }
            }
        }
    }
}
