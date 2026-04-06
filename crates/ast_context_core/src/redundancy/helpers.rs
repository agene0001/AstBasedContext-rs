use std::collections::HashSet;

/// Normalize an identifier to a canonical form for comparison.
/// Converts camelCase, PascalCase, snake_case, kebab-case all to lowercase without separators.
/// e.g. "firstName", "first_name", "FirstName" → "firstname"
pub(super) fn normalize_field_name(name: &str) -> String {
    let mut result = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch == '_' || ch == '-' {
            continue;
        }
        result.push(ch.to_ascii_lowercase());
    }
    result
}

/// Extract identifier-like tokens directly into a HashSet (avoids intermediate Vec).
pub(super) fn extract_tokens_set(source: &str) -> HashSet<&str> {
    source
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|t| t.len() > 2)
        .collect()
}

/// Normalize tokens directly into a HashSet (avoids intermediate Vec).
pub(super) fn normalize_tokens_set(source: &str) -> HashSet<&str> {
    source
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|t| t.len() > 1)
        .collect()
}

/// Jaccard similarity from pre-computed HashSets (avoids repeated allocation).
pub(super) fn jaccard_sets(set_a: &HashSet<&str>, set_b: &HashSet<&str>) -> f64 {
    let intersection = set_a.intersection(set_b).count();
    let union = set_a.union(set_b).count();
    if union == 0 {
        return 0.0;
    }
    intersection as f64 / union as f64
}

/// Extract field/member names from a struct/class source snippet.
///
/// Looks for patterns like `pub name:`, `name:`, `self.name`, `this.name`,
/// and common field declaration patterns across languages.
pub(super) fn extract_field_names(source: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut seen = HashSet::new();

    for line in source.lines() {
        let trimmed = line.trim();

        // Rust/Go: `pub field_name: Type` or `field_name Type`
        // Also catches `field_name: value,`
        if let Some(name) = trimmed
            .strip_prefix("pub ")
            .unwrap_or(trimmed)
            .split(':')
            .next()
        {
            let name = name.trim();
            if !name.is_empty()
                && !name.contains(' ')
                && !name.contains('(')
                && !name.contains('{')
                && !name.starts_with("//")
                && !name.starts_with('#')
                && !name.starts_with("fn ")
                && !name.starts_with("def ")
                && !name.starts_with("func ")
                && name != "pub"
                && name.len() > 1
                && name.chars().next().map(|c| c.is_alphabetic() || c == '_').unwrap_or(false)
            {
                if seen.insert(name.to_string()) {
                    fields.push(name.to_string());
                }
            }
        }
    }

    fields
}

/// Estimate the number of distinct "sections" in a function body.
///
/// A section is a block of code separated by blank lines or full-line comments.
/// This is a rough heuristic — not a real control flow analysis.
pub(super) fn estimate_sections(source: &str) -> usize {
    let mut sections = 0;
    let mut in_section = false;

    for line in source.lines() {
        let trimmed = line.trim();
        let is_separator = trimmed.is_empty()
            || trimmed.starts_with("//")
            || trimmed.starts_with('#')
            || trimmed.starts_with("/*")
            || trimmed.starts_with('*');

        if is_separator {
            if in_section {
                in_section = false;
            }
        } else if !in_section {
            sections += 1;
            in_section = true;
        }
    }

    sections
}

/// Extract the receiver identifier before a method call in a line.
///
/// Given `"    items.push(x)"` and `".push("`, returns `Some("items")`.
/// Handles chained access like `self.items.push(x)` → `"self.items"`.
pub(super) fn extract_receiver<'a>(line: &'a str, method: &str) -> Option<&'a str> {
    let pos = line.find(method)?;
    let before = &line[..pos];
    let trimmed = before.trim_end();
    if trimmed.is_empty() {
        return None;
    }
    // Scan backward to find the start of the identifier chain (alphanumeric, _, .)
    let start = trimmed
        .rfind(|c: char| !c.is_alphanumeric() && c != '_' && c != '.')
        .map(|i| i + 1)
        .unwrap_or(0);
    let receiver = &trimmed[start..];
    if receiver.is_empty() || receiver.starts_with('.') {
        None
    } else {
        Some(receiver)
    }
}

/// Check if a trimmed line starts a loop construct.
pub(super) fn is_loop_start(trimmed: &str) -> bool {
    trimmed.starts_with("for ")
        || trimmed.starts_with("for(")
        || trimmed.starts_with("while ")
        || trimmed.starts_with("while(")
        || trimmed == "loop {"
        || trimmed.starts_with("loop {")
}

/// Net brace-depth change for a line (counts `{` and `}`).
pub(super) fn brace_delta(line: &str) -> i32 {
    let mut delta = 0i32;
    for c in line.chars() {
        match c {
            '{' => delta += 1,
            '}' => delta -= 1,
            _ => {}
        }
    }
    delta
}

/// Return longest prefix match length from a set of patterns.
pub(super) fn p_min_len(name: &str, patterns: &[&str]) -> usize {
    patterns.iter()
        .filter(|p| name.starts_with(**p))
        .map(|p| p.len())
        .max()
        .unwrap_or(0)
}
