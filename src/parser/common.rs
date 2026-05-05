use tree_sitter::Node;

/// Extract the text of a tree-sitter node from the source bytes.
pub fn get_node_text<'a>(node: &Node, source: &'a [u8]) -> &'a str {
    node.utf8_text(source).unwrap_or("")
}

/// Walk up the tree to find the nearest enclosing node whose type is in `types`.
/// Returns (name, node_type, start_line_1based) or None.
pub fn get_parent_context(
    node: &Node,
    source: &[u8],
    types: &[&str],
) -> Option<(String, String, u32)> {
    let mut curr = node.parent();
    while let Some(parent) = curr {
        if types.contains(&parent.kind()) {
            if let Some(name_node) = parent.child_by_field_name("name") {
                let name = get_node_text(&name_node, source).to_string();
                let kind = parent.kind().to_string();
                let line = parent.start_position().row as u32 + 1;
                return Some((name, kind, line));
            }
        }
        curr = parent.parent();
    }
    None
}

/// Calculate cyclomatic complexity for a subtree.
/// Starts at 1 and increments for each branching/looping construct.
pub fn calculate_cyclomatic_complexity(node: &Node, complexity_kinds: &[&str]) -> u32 {
    let mut count = 1u32;
    let mut cursor = node.walk();
    traverse_complexity(&mut cursor, complexity_kinds, &mut count);
    count
}

fn traverse_complexity(
    cursor: &mut tree_sitter::TreeCursor,
    complexity_kinds: &[&str],
    count: &mut u32,
) {
    if complexity_kinds.contains(&cursor.node().kind()) {
        *count += 1;
    }
    if cursor.goto_first_child() {
        loop {
            traverse_complexity(cursor, complexity_kinds, count);
            if !cursor.goto_next_sibling() {
                break;
            }
        }
        cursor.goto_parent();
    }
}

/// Python-specific complexity node types.
pub const PYTHON_COMPLEXITY_KINDS: &[&str] = &[
    "if_statement",
    "for_statement",
    "while_statement",
    "except_clause",
    "with_statement",
    "boolean_operator",
    "list_comprehension",
    "generator_expression",
    "case_clause",
];
