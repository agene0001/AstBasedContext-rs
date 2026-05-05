//! Source annotation: extracts source snippets for each parsed node.
//!
//! This is opt-in (behind `--annotate`) because it significantly increases
//! graph size, but it enables AI-driven analysis like:
//! - Finding redundant/duplicate functions (even with different names)
//! - Identifying consolidation opportunities across modules
//! - Suggesting codebase restructuring based on what each node actually does

use crate::types::node::SourceSpan;
use crate::types::FileParseResult;

/// Maximum source snippet length in bytes. Larger nodes get truncated with a
/// `... (truncated)` marker to keep the graph manageable.
const MAX_SNIPPET_BYTES: usize = 4096;

/// Annotate all nodes in a `FileParseResult` with their source snippets.
///
/// This reads source text for each node's span and stores it in the node's
/// `source` field. Call this after parsing but before adding to the graph.
pub fn annotate_sources(source: &[u8], result: &mut FileParseResult) {
    let source_str = match std::str::from_utf8(source) {
        Ok(s) => s,
        Err(_) => return,
    };
    let lines: Vec<&str> = source_str.lines().collect();

    for func in &mut result.functions {
        if func.source.is_none() {
            func.source = Some(extract_span(&lines, &func.span));
        }
    }
    for class in &mut result.classes {
        if class.source.is_none() {
            class.source = Some(extract_span(&lines, &class.span));
        }
    }
    for tr in &mut result.traits {
        if tr.source.is_none() {
            tr.source = Some(extract_span(&lines, &tr.span));
        }
    }
    for iface in &mut result.interfaces {
        if iface.source.is_none() {
            iface.source = Some(extract_span(&lines, &iface.span));
        }
    }
    for st in &mut result.structs {
        if st.source.is_none() {
            st.source = Some(extract_span(&lines, &st.span));
        }
    }
    for en in &mut result.enums {
        if en.source.is_none() {
            en.source = Some(extract_span(&lines, &en.span));
        }
    }
    for mac in &mut result.macros {
        if mac.source.is_none() {
            // Macros only have a line_number, not a full span — grab one line
            let line_idx = mac.line_number.saturating_sub(1) as usize;
            if line_idx < lines.len() {
                mac.source = Some(lines[line_idx].to_string());
            }
        }
    }
}

/// Extract source text for a given span, with truncation.
fn extract_span(lines: &[&str], span: &SourceSpan) -> String {
    let start = span.start_line.saturating_sub(1) as usize;
    let end = (span.end_line as usize).min(lines.len());

    if start >= lines.len() || start >= end {
        return String::new();
    }

    let mut result = String::new();
    for line in &lines[start..end] {
        if result.len() + line.len() + 1 > MAX_SNIPPET_BYTES {
            result.push_str("\n... (truncated)");
            break;
        }
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(line);
    }
    result
}
