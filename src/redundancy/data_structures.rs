use crate::types::node::GraphNode;
use crate::types::Language;
use super::context::AnalysisContext;
use super::helpers::{extract_receiver, is_loop_start, brace_delta};
use super::types::{Tier, FindingKind, Finding};

// ─────────────────────────────────────────────────────────────────────────────
// Check 92: Vec used as Set
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_vec_used_as_set(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Language-specific append/contains method pairs
    let append_methods = [".push(", ".append(", ".add("];
    let contains_methods = [".contains(", ".includes(", ".has("];

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

        // Skip if source already uses Set/HashSet types
        if src.contains("HashSet") || src.contains("BTreeSet")
            || src.contains("new Set(") || src.contains(": set")
            || src.contains("= set(")
        {
            continue;
        }

        // Find variables that are both appended to and checked for membership
        let mut appended: Vec<&str> = Vec::new();
        let mut contained: Vec<&str> = Vec::new();
        let mut indexed: Vec<&str> = Vec::new();

        for line in src.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") || trimmed.starts_with('#') {
                continue;
            }

            for method in &append_methods {
                if let Some(recv) = extract_receiver(trimmed, method) {
                    appended.push(recv);
                }
            }
            for method in &contains_methods {
                if let Some(recv) = extract_receiver(trimmed, method) {
                    contained.push(recv);
                }
            }
            // Also check Python `in` pattern: `if x in variable`
            if trimmed.starts_with("if ") && trimmed.contains(" in ") {
                if let Some(after_in) = trimmed.split(" in ").nth(1) {
                    let var = after_in.trim().trim_end_matches(':').trim_end_matches('{');
                    if !var.is_empty() && var.chars().next().map(|c| c.is_alphabetic() || c == '_').unwrap_or(false) {
                        contained.push(var.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '.').next().unwrap_or(var));
                    }
                }
            }
            // Track index access: var[
            if let Some(bracket_pos) = trimmed.find('[') {
                if bracket_pos > 0 {
                    let before = &trimmed[..bracket_pos];
                    let var = before.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '.').next_back().unwrap_or("");
                    if !var.is_empty() {
                        indexed.push(var);
                    }
                }
            }
        }

        // Find overlap: variables that are both appended to and searched
        for &recv in &appended {
            if contained.contains(&recv) && !indexed.contains(&recv) {
                findings.push(Finding {
                    tier: Tier::Medium,
                    kind: FindingKind::VecUsedAsSet {
                        function_name: func.name.clone(),
                        variable_hint: recv.to_string(),
                    },
                    node_indices: vec![idx.index()],
                    description: format!(
                        "`{}` in `{}`: `{}` appended+searched but never indexed — use HashSet for O(1).",
                        recv, func.name, recv
                    ),
                });
                break; // one finding per function
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 93: Vec used as Map
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_vec_used_as_map(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Patterns that indicate a Vec of tuples being searched by key
    let map_search_patterns = [
        ".iter().find(|(",     // Rust: vec.iter().find(|(k, _)| ...)
        ".iter().find(|&(",    // Rust: vec.iter().find(|&(k, _)| ...)
        ".find(([",            // TS/JS: arr.find(([k, v]) => ...)
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
            for pattern in &map_search_patterns {
                if trimmed.contains(pattern) {
                    let variable = extract_receiver(trimmed, ".iter()")
                        .or_else(|| extract_receiver(trimmed, ".find("))
                        .unwrap_or("collection")
                        .to_string();

                    findings.push(Finding {
                        tier: Tier::Medium,
                        kind: FindingKind::VecUsedAsMap {
                            function_name: func.name.clone(),
                            variable_hint: variable.clone(),
                        },
                        node_indices: vec![idx.index()],
                        description: format!(
                            "`{}` in `{}`: Vec-of-tuples key search is O(n) — use HashMap for O(1).",
                            variable, func.name
                        ),
                    });
                    break;
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 94: Linear search in loop
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_linear_search_in_loop(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    let search_patterns = [
        ".contains(",
        ".includes(",
        ".indexOf(",
        ".index(",
        ".iter().find(",
        ".iter().position(",
        ".iter().any(",
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

        // Skip if source already uses Set/HashSet types (they already have O(1) contains)
        if src.contains("HashSet") || src.contains("BTreeSet")
            || src.contains("new Set(") || src.contains("= set(")
        {
            continue;
        }

        let is_python = func.language == Language::Python;
        let mut loop_depth = 0i32;
        let mut python_loop_indent: Option<usize> = None;
        let mut found = false;

        for line in src.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('#') {
                continue;
            }

            if is_python {
                let indent = line.len() - line.trim_start().len();
                if let Some(loop_ind) = python_loop_indent {
                    if indent <= loop_ind {
                        loop_depth = 0;
                        python_loop_indent = None;
                    }
                }
                if is_loop_start(trimmed) {
                    loop_depth = 1;
                    python_loop_indent = Some(indent);
                }
            } else {
                if is_loop_start(trimmed) {
                    loop_depth += 1;
                }
                let delta = brace_delta(line);
                if delta < 0 && loop_depth > 0 {
                    loop_depth = (loop_depth + delta).max(0);
                }
            }

            if loop_depth > 0 && !is_loop_start(trimmed) {
                for pattern in &search_patterns {
                    if trimmed.contains(pattern) {
                        let matched = pattern.to_string();
                        findings.push(Finding {
                            tier: Tier::High,
                            kind: FindingKind::LinearSearchInLoop {
                                function_name: func.name.clone(),
                                search_pattern: matched.clone(),
                            },
                            node_indices: vec![idx.index()],
                            description: format!(
                                "`{}` in `{}`: `{}` in loop is O(n)/iter — pre-compute HashSet for O(1).",
                                matched.trim_end_matches('('), func.name, matched.trim_end_matches('(')
                            ),
                        });
                        found = true;
                        break;
                    }
                }
                // Python: `if x in collection` inside loop
                if !found && is_python && loop_depth > 0
                    && trimmed.starts_with("if ") && trimmed.contains(" in ")
                    && !trimmed.contains(" in range(")
                    && !trimmed.contains(" in enumerate(")
                {
                    findings.push(Finding {
                        tier: Tier::High,
                        kind: FindingKind::LinearSearchInLoop {
                            function_name: func.name.clone(),
                            search_pattern: "in".to_string(),
                        },
                        node_indices: vec![idx.index()],
                        description: format!(
                            "`{}`: `if x in list` in loop may be O(n)/iter — use a set for O(1).",
                            func.name
                        ),
                    });
                    found = true;
                }
            }
            if found {
                break;
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 95: String concatenation in loop
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_string_concat_in_loop(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Patterns indicating string concatenation
    let concat_patterns: &[&str] = &[
        ".push_str(",
        "+= &format!(",
        "+= \"",
        "+= '",
        "+= f\"",
        "+= `",
        "result = result + ",
    ];

    // Patterns that indicate the dev already optimized
    let capacity_hints = [
        "with_capacity",
        "strings.Builder",
        "StringBuilder",
        "StringBuffer",
        "join(",
        "' '.join(",
        "\".join(",
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

        // Skip if capacity hint already present
        if capacity_hints.iter().any(|h| src.contains(h)) {
            continue;
        }

        let is_python = func.language == Language::Python;
        let mut loop_depth = 0i32;
        let mut python_loop_indent: Option<usize> = None;
        let mut found = false;

        for line in src.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('#') {
                continue;
            }

            if is_python {
                let indent = line.len() - line.trim_start().len();
                if let Some(loop_ind) = python_loop_indent {
                    if indent <= loop_ind {
                        loop_depth = 0;
                        python_loop_indent = None;
                    }
                }
                if is_loop_start(trimmed) {
                    loop_depth = 1;
                    python_loop_indent = Some(indent);
                }
            } else {
                if is_loop_start(trimmed) {
                    loop_depth += 1;
                }
                let delta = brace_delta(line);
                if delta < 0 && loop_depth > 0 {
                    loop_depth = (loop_depth + delta).max(0);
                }
            }

            if loop_depth > 0 && !is_loop_start(trimmed) {
                for pattern in concat_patterns {
                    if trimmed.contains(pattern) {
                        let suggestion = match func.language {
                            Language::Rust => "use `String::with_capacity()` or `.collect::<String>()`",
                            Language::Python => "use `''.join(parts)` instead",
                            Language::Go => "use `strings.Builder`",
                            Language::Java => "use `StringBuilder`",
                            Language::TypeScript | Language::JavaScript => "use `Array.push()` + `.join()`",
                            _ => "pre-allocate or use a string builder",
                        };
                        findings.push(Finding {
                            tier: Tier::Medium,
                            kind: FindingKind::StringConcatInLoop {
                                function_name: func.name.clone(),
                                concat_pattern: pattern.to_string(),
                            },
                            node_indices: vec![idx.index()],
                            description: format!(
                                "`{}`: `{}` in loop is O(n²) — {}.",
                                func.name, pattern.trim(), suggestion
                            ),
                        });
                        found = true;
                        break;
                    }
                }
            }
            if found {
                break;
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 96: Sorted Vec for lookup
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_sorted_vec_for_lookup(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    let sort_patterns = [
        ".sort()", ".sort_unstable()", ".sort_by(", ".sort_by_key(",
        "sorted(", "Collections.sort(", "std::sort(",
        "sort.Slice(", "sort.Sort(",
    ];
    let search_patterns = [
        ".binary_search(", ".binary_search_by(",
        "bisect.bisect(", "bisect_left(", "bisect_right(",
        "Collections.binarySearch(",
        "std::lower_bound(", "std::binary_search(",
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

        let has_sort = sort_patterns.iter().any(|p| src.contains(p));
        let has_search = search_patterns.iter().any(|p| src.contains(p));

        if has_sort && has_search {
            // Try to extract the variable being sorted
            let variable = sort_patterns.iter()
                .filter_map(|p| {
                    src.lines()
                        .find(|l| l.contains(p))
                        .and_then(|l| extract_receiver(l.trim(), p))
                })
                .next()
                .unwrap_or("collection")
                .to_string();

            let suggestion = match func.language {
                Language::Rust => "BTreeSet/BTreeMap",
                Language::Python => "SortedList (sortedcontainers) or a set",
                Language::Java => "TreeSet/TreeMap",
                _ => "a sorted collection type",
            };

            findings.push(Finding {
                tier: Tier::Low,
                kind: FindingKind::SortedVecForLookup {
                    function_name: func.name.clone(),
                    variable_hint: variable.clone(),
                },
                node_indices: vec![idx.index()],
                description: format!(
                    "`{}`: `{}` sorted then binary-searched — use {} (auto-ordered).",
                    func.name, variable, suggestion
                ),
            });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 97: Nested loop lookup
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_nested_loop_lookup(
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
        if src.lines().count() < 6 {
            continue;
        }

        let is_python = func.language == Language::Python;
        let lines: Vec<&str> = src.lines().collect();
        let mut found = false;

        // Look for nested loop pattern: two for/while loops with an equality check
        let mut loop_depth = 0i32;
        let mut outer_loop_line: Option<usize> = None;
        let mut python_indents: Vec<usize> = Vec::new();

        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with('#') {
                continue;
            }

            if is_python {
                let indent = line.len() - line.trim_start().len();
                // Pop loop levels when we dedent
                while let Some(&last_ind) = python_indents.last() {
                    if indent <= last_ind {
                        python_indents.pop();
                        loop_depth -= 1;
                    } else {
                        break;
                    }
                }
                if is_loop_start(trimmed) {
                    loop_depth += 1;
                    python_indents.push(indent);
                    if loop_depth == 1 {
                        outer_loop_line = Some(i);
                    }
                }
            } else {
                if is_loop_start(trimmed) {
                    loop_depth += 1;
                    if loop_depth == 1 {
                        outer_loop_line = Some(i);
                    }
                }
                let delta = brace_delta(line);
                if delta < 0 {
                    loop_depth = (loop_depth + delta).max(0);
                }
            }

            // At depth >= 2, look for equality checks
            if loop_depth >= 2 && outer_loop_line.is_some()
                && (trimmed.contains(" == ") || trimmed.contains(" === "))
            {
                findings.push(Finding {
                    tier: Tier::High,
                    kind: FindingKind::NestedLoopLookup {
                        function_name: func.name.clone(),
                        estimated_pattern: "nested for-loop with == check".to_string(),
                    },
                    node_indices: vec![idx.index()],
                    description: format!(
                        "`{}`: nested loops + equality check is O(n²) — build a HashSet from one collection for O(1) membership.",
                        func.name
                    ),
                });
                found = true;
                break;
            }
        }
        if found {
            continue;
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 98: HashMap with sequential integer keys
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_hashmap_sequential_keys(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Patterns: .insert(0, ...), .insert(1, ...), etc. or map[0] = ..., map[1] = ...
    let insert_patterns = [".insert(", ".put(", ".set("];

    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };
        let src = match &func.source {
            Some(s) => s,
            None => continue,
        };

        // Collect (receiver, integer_key) pairs from insert calls
        let mut inserts: Vec<(&str, u32)> = Vec::new();

        for line in src.lines() {
            let trimmed = line.trim();
            for pattern in &insert_patterns {
                if let Some(recv) = extract_receiver(trimmed, pattern) {
                    // Extract the first argument (should be an integer)
                    if let Some(after_method) = trimmed.find(pattern).map(|p| &trimmed[p + pattern.len()..]) {
                        let arg = after_method.split([',', ')']).next().unwrap_or("").trim();
                        if let Ok(n) = arg.parse::<u32>() {
                            inserts.push((recv, n));
                        }
                    }
                }
            }

            // Also check dict[0] = ..., dict[1] = ... patterns
            if let Some(bracket_pos) = trimmed.find('[') {
                if trimmed[bracket_pos..].contains("] =") || trimmed[bracket_pos..].contains("]=") {
                    let before = &trimmed[..bracket_pos];
                    let var = before.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '.').next_back().unwrap_or("");
                    if let Some(close) = trimmed[bracket_pos + 1..].find(']') {
                        let key = trimmed[bracket_pos + 1..bracket_pos + 1 + close].trim();
                        if let Ok(n) = key.parse::<u32>() {
                            if !var.is_empty() {
                                inserts.push((var, n));
                            }
                        }
                    }
                }
            }
        }

        // Group by receiver, check for sequential keys
        if inserts.len() < 3 {
            continue;
        }

        let mut by_receiver: std::collections::HashMap<&str, Vec<u32>> = std::collections::HashMap::new();
        for (recv, key) in &inserts {
            by_receiver.entry(recv).or_default().push(*key);
        }

        for (recv, keys) in &by_receiver {
            if keys.len() < 3 {
                continue;
            }
            let mut sorted = keys.clone();
            sorted.sort();
            sorted.dedup();
            // Check if keys form a sequence (0,1,2... or 1,2,3...)
            let is_sequential = sorted.len() >= 3
                && sorted.windows(2).all(|w| w[1] == w[0] + 1);

            if is_sequential {
                findings.push(Finding {
                    tier: Tier::Low,
                    kind: FindingKind::HashMapWithSequentialKeys {
                        function_name: func.name.clone(),
                        variable_hint: recv.to_string(),
                    },
                    node_indices: vec![idx.index()],
                    description: format!(
                        "`{}`: `{}` has sequential int keys {:?} — use Vec/array instead.",
                        func.name, recv, &sorted[..sorted.len().min(5)]
                    ),
                });
                break;
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 99: Excessive collect-iterate
// ─────────────────────────────────────────────────────────────────────────────

#[allow(unused_assignments)]
pub(super) fn detect_excessive_collect_iterate(
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

        // Only targets Rust primarily (collect is a Rust idiom)
        if func.language != Language::Rust {
            continue;
        }

        let lines: Vec<&str> = src.lines().collect();
        let mut found = false;

        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();

            // Pattern 1: .collect::<Vec<...>>().iter() on same line
            if trimmed.contains(".collect::<Vec") && trimmed.contains(".iter()") {
                findings.push(Finding {
                    tier: Tier::High,
                    kind: FindingKind::ExcessiveCollectIterate {
                        function_name: func.name.clone(),
                        collect_pattern: ".collect::<Vec<_>>().iter()".to_string(),
                    },
                    node_indices: vec![idx.index()],
                    description: format!(
                        "`{}`: `.collect::<Vec<_>>().iter()` needlessly allocates — remove `.collect()` and iterate directly.",
                        func.name
                    ),
                });
                found = true;
                break;
            }

            // Pattern 2: .collect::<Vec on one line, .iter()/.into_iter()/.for_each( on next
            if trimmed.contains(".collect::<Vec") || trimmed.contains(".collect();") {
                if let Some(next_line) = lines.get(i + 1) {
                    let next = next_line.trim();
                    if next.starts_with(".iter()") || next.starts_with(".into_iter()")
                        || next.starts_with(".for_each(")
                    {
                        findings.push(Finding {
                            tier: Tier::High,
                            kind: FindingKind::ExcessiveCollectIterate {
                                function_name: func.name.clone(),
                                collect_pattern: "collect then iterate".to_string(),
                            },
                            node_indices: vec![idx.index()],
                            description: format!(
                                "`{}`: collects to Vec then immediately iterates — unnecessary allocation, use iterator chain directly.",
                                func.name
                            ),
                        });
                        found = true;
                        break;
                    }
                }
            }

            // Pattern 3: let var: Vec<_> = ...collect(); followed by for _ in &var / var.iter()
            if (trimmed.contains("Vec<") || trimmed.contains("Vec ="))
                && trimmed.contains(".collect(")
            {
                // Extract variable name: `let var_name` or `let var_name:`
                if let Some(var_start) = trimmed.find("let ") {
                    let after_let = &trimmed[var_start + 4..];
                    let var_name = after_let
                        .split(|c: char| !c.is_alphanumeric() && c != '_')
                        .next()
                        .unwrap_or("");
                    if !var_name.is_empty() && var_name != "mut" {
                        let actual_var = if var_name == "mut" {
                            after_let.get(4..).and_then(|s|
                                s.trim().split(|c: char| !c.is_alphanumeric() && c != '_').next()
                            ).unwrap_or("")
                        } else {
                            var_name
                        };
                        // Look ahead for iteration on this variable
                        let search_iter = format!("{}.iter()", actual_var);
                        let search_into = format!("{}.into_iter()", actual_var);
                        let search_ref = format!("in &{}", actual_var);
                        let search_for = format!("in {}", actual_var);
                        for next in lines.iter().skip(i + 1).take(5) {
                            let nt = next.trim();
                            if nt.contains(&search_iter) || nt.contains(&search_into)
                                || nt.contains(&search_ref) || nt.contains(&search_for)
                            {
                                findings.push(Finding {
                                    tier: Tier::High,
                                    kind: FindingKind::ExcessiveCollectIterate {
                                        function_name: func.name.clone(),
                                        collect_pattern: format!("let {} = ...collect() then iterate", actual_var),
                                    },
                                    node_indices: vec![idx.index()],
                                    description: format!(
                                        "`{}`: `{}` collected to Vec then iterated — remove intermediate allocation.",
                                        func.name, actual_var
                                    ),
                                });
                                found = true;
                                break;
                            }
                        }
                    }
                }
            }
            if found {
                break;
            }
        }
    }
}
