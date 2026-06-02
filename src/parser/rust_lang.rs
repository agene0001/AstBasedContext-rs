use std::collections::HashSet;
use std::path::Path;

use streaming_iterator::StreamingIterator;
use tree_sitter::{Language as TsLanguage, Node, Parser, Query, QueryCursor};

use crate::error::{Error, Result};
use crate::types::node::*;
use crate::types::{FileParseResult, Language};

use super::common::*;
use super::LanguageParser;
use std::collections::HashMap;

// ── Tree-sitter query strings ────────────────────────────────────────────

const Q_FUNCTIONS: &str = r#"
    (function_item
        name: (identifier) @name
        parameters: (parameters) @params) @function_node
"#;

const Q_STRUCTS: &str = r#"
    (struct_item
        name: (type_identifier) @name) @struct_node
"#;

const Q_ENUMS: &str = r#"
    (enum_item
        name: (type_identifier) @name
        body: (enum_variant_list)? @body) @enum_node
"#;

const Q_TRAITS: &str = r#"
    (trait_item
        name: (type_identifier) @name) @trait_node
"#;

const Q_IMPORTS: &str = r#"
    (use_declaration) @import
"#;

const Q_CALLS: &str = r#"
    (call_expression
        function: [
            (identifier) @name
            (field_expression field: (field_identifier) @name)
            (scoped_identifier name: (identifier) @name)
        ])
    (token_tree (identifier) @name . (token_tree))
"#;

const Q_VARIABLES: &str = r#"
    (let_declaration
        pattern: (identifier) @name)
    (const_item
        name: (identifier) @name)
    (static_item
        name: (identifier) @name)
"#;

/// Complexity-contributing node types for Rust.
const RUST_COMPLEXITY_KINDS: &[&str] = &[
    "if_expression",
    "for_expression",
    "while_expression",
    "match_expression",
    "match_arm",
    "binary_expression", // counted only when operator is && or ||; approximated here
    "loop_expression",
];

/// Compiled queries, created once per RustParser instance.
struct RustQueries {
    functions: Query,
    structs: Query,
    enums: Query,
    traits: Query,
    imports: Query,
    calls: Query,
    variables: Query,
}

impl RustQueries {
    fn new(ts_lang: &TsLanguage) -> Result<Self> {
        let mk = |src: &str| {
            Query::new(ts_lang, src).map_err(|e| Error::Query(format!("{e}")))
        };
        Ok(Self {
            functions: mk(Q_FUNCTIONS)?,
            structs: mk(Q_STRUCTS)?,
            enums: mk(Q_ENUMS)?,
            traits: mk(Q_TRAITS)?,
            imports: mk(Q_IMPORTS)?,
            calls: mk(Q_CALLS)?,
            variables: mk(Q_VARIABLES)?,
        })
    }
}

pub struct RustParser {
    ts_language: TsLanguage,
    queries: RustQueries,
}

impl Default for RustParser {
    fn default() -> Self {
        Self::new()
    }
}

impl RustParser {
    pub fn new() -> Self {
        let ts_language: TsLanguage = tree_sitter_rust::LANGUAGE.into();
        let queries = RustQueries::new(&ts_language)
            .expect("built-in Rust queries must compile");
        Self {
            ts_language,
            queries,
        }
    }

    // ── extraction helpers ───────────────────────────────────────────────

    fn find_functions(&self, source: &[u8], root: &Node, path: &Path, cursor: &mut QueryCursor) -> Vec<FunctionData> {
        let mut functions = Vec::new();
        for_each_capture(cursor, &self.queries.functions, "name", *root, source, |node| {
                let Some(func_node) = node.parent() else { return; };
                let name = get_node_text(&node, source).to_string();

                let params_node = func_node.child_by_field_name("parameters");
                let args = extract_rust_params(params_node.as_ref(), source);
                let complexity = calculate_cyclomatic_complexity(&func_node, RUST_COMPLEXITY_KINDS);

                // For Rust, manually extract impl block info since impl_item
                // uses `type` and `trait` fields, not `name`.
                let (impl_type_name, impl_trait_name) = extract_impl_context(&func_node, source);

                // Build the general context: prefer trait name (for trait impls),
                // then fall back to enclosing function/trait_item context.
                let ctx = if impl_trait_name.is_some() {
                    // Inside `impl Trait for Type` — context is the trait name
                    impl_trait_name.as_ref().map(|t| (t.clone(), "impl_item".to_string(), func_node.start_position().row as u32 + 1))
                } else if impl_type_name.is_some() {
                    // Inside `impl Type` — context is the type name
                    impl_type_name.as_ref().map(|t| (t.clone(), "impl_item".to_string(), func_node.start_position().row as u32 + 1))
                } else {
                    // Not in an impl block — fall back to normal parent context
                    get_parent_context(
                        &func_node,
                        source,
                        &["function_item", "trait_item"],
                    )
                };

                // class_context maps to the impl block's type name
                let class_ctx = impl_type_name;

                let arg_types = extract_rust_param_types(params_node.as_ref(), source);
                let return_type = func_node
                    .child_by_field_name("return_type")
                    .map(|rt| get_node_text(&rt, source).to_string());
                let visibility = if has_visibility_modifier(&func_node) {
                    Some("public".to_string())
                } else {
                    None
                };
                let is_static = !args.first().is_some_and(|a| {
                    a == "self" || a == "&self" || a == "&mut self"
                });

                // Check if the function is async by looking for an `async` keyword child
                let is_async = {
                    let mut found = false;
                    let mut c = func_node.walk();
                    if c.goto_first_child() {
                        loop {
                            if c.node().kind() == "async" {
                                found = true;
                                break;
                            }
                            if !c.goto_next_sibling() {
                                break;
                            }
                        }
                    }
                    found
                };

                // Scan for TODO/FIXME/HACK/XXX markers in comments inside this function
                let todo_comments = collect_todo_comments(&func_node, source);

                functions.push(FunctionData {
                    name,
                    path: path.to_path_buf(),
                    span: SourceSpan {
                        start_line: node.start_position().row as u32 + 1,
                        end_line: func_node.end_position().row as u32 + 1,
                        start_col: node.start_position().column as u32,
                        end_col: func_node.end_position().column as u32,
                    },
                    args,
                    arg_types,
                    return_type,
                    visibility,
                    is_static,
                    is_async,
                    todo_comments,
                    cyclomatic_complexity: complexity,
                    decorators: Vec::new(), // Rust uses attributes, not decorators
                    context: ctx.as_ref().map(|(n, _, _)| n.clone()),
                    context_type: ctx.as_ref().map(|(_, t, _)| t.clone()),
                    class_context: class_ctx,
                    ..FunctionData::template(Language::Rust)
                });
        });
        functions
    }

    fn find_structs(&self, source: &[u8], root: &Node, path: &Path, cursor: &mut QueryCursor) -> Vec<StructData> {
        let mut structs = Vec::new();
        for_each_capture(cursor, &self.queries.structs, "name", *root, source, |node| {
                let Some(struct_node) = node.parent() else { return; };
                let name = get_node_text(&node, source).to_string();

                let mut fields = extract_struct_fields(&struct_node, source);
                // Fill in default values from any `impl Default for <name>` block,
                // so queries can answer "what's the default for field X".
                let defaults = extract_default_values(root, source, &name);
                if !defaults.is_empty() {
                    for f in &mut fields {
                        if f.default_value.is_none()
                            && let Some(v) = defaults.get(&f.name)
                        {
                            f.default_value = Some(v.clone());
                        }
                    }
                }

                structs.push(StructData {
                    name,
                    path: path.to_path_buf(),
                    span: SourceSpan {
                        start_line: node.start_position().row as u32 + 1,
                        end_line: struct_node.end_position().row as u32 + 1,
                        start_col: node.start_position().column as u32,
                        end_col: struct_node.end_position().column as u32,
                    },
                    fields,
                    language: Language::Rust,
                    is_dependency: false,
                    source: None,
                });
        });
        structs
    }

    fn find_enums(&self, source: &[u8], root: &Node, path: &Path, cursor: &mut QueryCursor) -> Vec<EnumData> {
        let mut enums = Vec::new();
        for_each_capture(cursor, &self.queries.enums, "name", *root, source, |node| {
                let Some(enum_node) = node.parent() else { return; };
                let name = get_node_text(&node, source).to_string();

                // Extract variant names from the enum body
                let variants = extract_enum_variants(&enum_node, source);

                enums.push(EnumData {
                    name,
                    path: path.to_path_buf(),
                    span: SourceSpan {
                        start_line: node.start_position().row as u32 + 1,
                        end_line: enum_node.end_position().row as u32 + 1,
                        start_col: node.start_position().column as u32,
                        end_col: enum_node.end_position().column as u32,
                    },
                    variants,
                    language: Language::Rust,
                    is_dependency: false,
                    source: None,
                });
        });
        enums
    }

    fn find_traits(&self, source: &[u8], root: &Node, path: &Path, cursor: &mut QueryCursor) -> Vec<TraitData> {
        let mut traits = Vec::new();
        for_each_capture(cursor, &self.queries.traits, "name", *root, source, |node| {
                let Some(trait_node) = node.parent() else { return; };
                let name = get_node_text(&node, source).to_string();

                traits.push(TraitData {
                    name,
                    path: path.to_path_buf(),
                    span: SourceSpan {
                        start_line: node.start_position().row as u32 + 1,
                        end_line: trait_node.end_position().row as u32 + 1,
                        start_col: node.start_position().column as u32,
                        end_col: trait_node.end_position().column as u32,
                    },
                    language: Language::Rust,
                    is_dependency: false,
                    source: None,
                });
        });
        traits
    }

    fn find_imports(&self, source: &[u8], root: &Node, cursor: &mut QueryCursor) -> Vec<ImportData> {
        let mut imports = Vec::new();
        let mut seen = HashSet::new();
        for_each_capture(cursor, &self.queries.imports, "import", *root, source, |node| {
                let text = get_node_text(&node, source).to_string();

                // Strip "use " prefix and trailing ";"
                let import_path = text
                    .trim()
                    .strip_prefix("use ")
                    .unwrap_or(&text)
                    .trim_end_matches(';')
                    .trim()
                    .to_string();

                if seen.contains(&import_path) {
                    return;
                }
                seen.insert(import_path.clone());

                // Extract the short name (last segment)
                let short_name = import_path
                    .rsplit("::")
                    .next()
                    .unwrap_or(&import_path)
                    .to_string();

                imports.push(ImportData {
                    name: short_name,
                    full_import_name: Some(import_path),
                    line_number: node.start_position().row as u32 + 1,
                    alias: None,
                    language: Language::Rust,
                    is_dependency: false,
                });
        });
        imports
    }

    fn find_calls(&self, source: &[u8], root: &Node, cursor: &mut QueryCursor) -> Vec<FunctionCallData> {
        let mut calls = Vec::new();
        for_each_capture(cursor, &self.queries.calls, "name", *root, source, |node| {
                let name = get_node_text(&node, source).to_string();

                // Find the enclosing call_expression, if any. For calls inside a
                // macro (`format!(.., foo(x))`) there is none — the name lives in a
                // token_tree — but it's still a real call, so we keep it (args
                // unknown). Resolution downstream only links names that resolve to a
                // known function, so bare-identifier arguments that are variables
                // simply produce no edge.
                let mut call_node = None;
                let mut p = node.parent();
                while let Some(cur) = p {
                    match cur.kind() {
                        "call_expression" => {
                            call_node = Some(cur);
                            break;
                        }
                        "function_item" | "source_file" => break,
                        _ => p = cur.parent(),
                    }
                }

                let (full_name, args) = match call_node {
                    Some(c) => (
                        c.child_by_field_name("function")
                            .map(|f| get_node_text(&f, source).to_string())
                            .unwrap_or_else(|| name.clone()),
                        extract_rust_call_args(&c, source),
                    ),
                    None => (name.clone(), Vec::new()),
                };

                let ctx = get_parent_context(
                    &node,
                    source,
                    &["function_item", "impl_item", "trait_item"],
                );

                calls.push(FunctionCallData {
                    name,
                    full_name,
                    line_number: node.start_position().row as u32 + 1,
                    args,
                    inferred_obj_type: None,
                    context: ctx,
                    language: Language::Rust,
                });
        });
        calls
    }

    fn find_variables(&self, source: &[u8], root: &Node, path: &Path, cursor: &mut QueryCursor) -> Vec<VariableData> {
        let mut variables = Vec::new();
        for_each_capture(cursor, &self.queries.variables, "name", *root, source, |node| {
                let Some(let_node) = node.parent() else { return; };
                let name = get_node_text(&node, source).to_string();

                let value = let_node
                    .child_by_field_name("value")
                    .map(|v| get_node_text(&v, source).to_string());

                let type_annotation = let_node
                    .child_by_field_name("type")
                    .map(|t| get_node_text(&t, source).to_string());

                let (impl_type, _) = extract_impl_context(&node, source);
                let ctx = impl_type.clone().or_else(|| {
                    get_parent_context(&node, source, &["function_item", "trait_item"])
                        .map(|(n, _, _)| n)
                });

                variables.push(VariableData {
                    name,
                    path: path.to_path_buf(),
                    line_number: node.start_position().row as u32 + 1,
                    value,
                    type_annotation,
                    context: ctx,
                    class_context: impl_type,
                    language: Language::Rust,
                    is_dependency: false,
                });
        });
        variables
    }
}

impl LanguageParser for RustParser {
    fn language(&self) -> Language {
        Language::Rust
    }

    fn parse(&self, path: &Path, source: &[u8], is_dependency: bool) -> Result<FileParseResult> {
        let mut parser = build_parser(&self.ts_language);
        let tree = parser.parse(source, None).ok_or_else(|| Error::Parse {
            path: path.to_path_buf(),
            message: "tree-sitter failed to parse".into(),
        })?;
        let root = tree.root_node();
        let mut cursor = QueryCursor::new();

        let functions = self.find_functions(source, &root, path, &mut cursor);
        let structs = self.find_structs(source, &root, path, &mut cursor);
        let enums = self.find_enums(source, &root, path, &mut cursor);
        let traits = self.find_traits(source, &root, path, &mut cursor);
        let imports = self.find_imports(source, &root, &mut cursor);
        let function_calls = self.find_calls(source, &root, &mut cursor);
        let variables = self.find_variables(source, &root, path, &mut cursor);

        let mut result = FileParseResult::new(path.to_path_buf(), Language::Rust, is_dependency);
        result.functions = functions;
        result.structs = structs;
        result.enums = enums;
        result.traits = traits;
        result.imports = imports;
        result.function_calls = function_calls;
        result.variables = variables;

        // File-level stats
        let source_str = std::str::from_utf8(source).unwrap_or("");
        result.total_lines = source_str.lines().count();
        result.comment_line_count = count_comment_nodes(&root);
        result.is_test_file = {
            let path_str = path.to_string_lossy();
            path_str.contains("tests/")
                || path
                    .file_name()
                    .is_some_and(|f| f.to_string_lossy().contains("test"))
                || source_str.contains("#[cfg(test)]")
        };

        Ok(result)
    }
}

// ── free helpers ──────────────────────────────────────────────────────────

fn extract_rust_params(params_node: Option<&Node>, source: &[u8]) -> Vec<String> {
    let Some(params) = params_node else {
        return Vec::new();
    };
    let mut args = Vec::new();
    let mut cursor = params.walk();
    if !cursor.goto_first_child() {
        return args;
    }
    loop {
        let child = cursor.node();
        match child.kind() {
            "parameter" => {
                // Extract just the pattern (name), not the full "name: Type" text.
                // The `pattern` field contains the identifier for simple params.
                let name = child
                    .child_by_field_name("pattern")
                    .map(|p| get_node_text(&p, source).to_string())
                    .unwrap_or_else(|| get_node_text(&child, source).to_string());
                if !name.is_empty() {
                    args.push(name);
                }
            }
            "self_parameter" => {
                // self / &self / &mut self — use full text
                let text = get_node_text(&child, source).to_string();
                if !text.is_empty() {
                    args.push(text);
                }
            }
            _ => {}
        }
        if !cursor.goto_next_sibling() {
            break;
        }
    }
    args
}

fn extract_enum_variants(enum_node: &Node, source: &[u8]) -> Vec<String> {
    let mut variants = Vec::new();
    let Some(body) = enum_node.child_by_field_name("body") else {
        return variants;
    };
    let mut cursor = body.walk();
    if !cursor.goto_first_child() {
        return variants;
    }
    loop {
        let child = cursor.node();
        if child.kind() == "enum_variant" {
            if let Some(name_node) = child.child_by_field_name("name") {
                variants.push(get_node_text(&name_node, source).to_string());
            }
        }
        if !cursor.goto_next_sibling() {
            break;
        }
    }
    variants
}

/// Extract type annotations for each parameter in a Rust function signature.
fn extract_rust_param_types(params_node: Option<&Node>, source: &[u8]) -> Vec<Option<String>> {
    let Some(params) = params_node else {
        return Vec::new();
    };
    let mut types = Vec::new();
    let mut cursor = params.walk();
    if !cursor.goto_first_child() {
        return types;
    }
    loop {
        let child = cursor.node();
        match child.kind() {
            "parameter" => {
                let ty = child
                    .child_by_field_name("type")
                    .map(|t| get_node_text(&t, source).to_string());
                types.push(ty);
            }
            "self_parameter" => {
                // self/&self/&mut self — no separate type annotation
                types.push(None);
            }
            _ => {}
        }
        if !cursor.goto_next_sibling() {
            break;
        }
    }
    types
}

/// Check whether a tree-sitter node has a `visibility_modifier` child.
fn has_visibility_modifier(node: &Node) -> bool {
    let mut cursor = node.walk();
    if !cursor.goto_first_child() {
        return false;
    }
    loop {
        if cursor.node().kind() == "visibility_modifier" {
            return true;
        }
        if !cursor.goto_next_sibling() {
            break;
        }
    }
    false
}

/// Map `field name → default value literal` parsed from `impl Default for <struct_name>`.
fn extract_default_values(
    root: &Node,
    source: &[u8],
    struct_name: &str,
) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut stack = vec![*root];
    while let Some(n) = stack.pop() {
        if n.kind() == "impl_item" {
            let is_default = n
                .child_by_field_name("trait")
                .is_some_and(|t| get_node_text(&t, source) == "Default");
            let is_target = n
                .child_by_field_name("type")
                .is_some_and(|t| get_node_text(&t, source) == struct_name);
            if is_default && is_target {
                collect_field_initializers(&n, source, &mut out);
                continue; // no need to descend further into this impl
            }
        }
        let mut c = n.walk();
        for child in n.children(&mut c) {
            stack.push(child);
        }
    }
    out
}

/// Collect `field: value` initializers (e.g. inside a `Self { ... }` expression).
fn collect_field_initializers(
    node: &Node,
    source: &[u8],
    out: &mut HashMap<String, String>,
) {
    let mut stack = vec![*node];
    while let Some(n) = stack.pop() {
        if n.kind() == "field_initializer"
            && let (Some(f), Some(v)) =
                (n.child_by_field_name("field"), n.child_by_field_name("value"))
        {
            out.insert(
                get_node_text(&f, source).to_string(),
                get_node_text(&v, source).trim().to_string(),
            );
        }
        let mut c = n.walk();
        for child in n.children(&mut c) {
            stack.push(child);
        }
    }
}

/// Extract field declarations from a Rust struct body.
fn extract_struct_fields(struct_node: &Node, source: &[u8]) -> Vec<FieldDecl> {
    let mut fields = Vec::new();
    let Some(body) = struct_node.child_by_field_name("body") else {
        return fields;
    };
    let mut cursor = body.walk();
    if !cursor.goto_first_child() {
        return fields;
    }
    loop {
        let child = cursor.node();
        if child.kind() == "field_declaration" {
            let name = child
                .child_by_field_name("name")
                .map(|n| get_node_text(&n, source).to_string())
                .unwrap_or_default();
            let type_annotation = child
                .child_by_field_name("type")
                .map(|t| get_node_text(&t, source).to_string());
            let visibility = if has_visibility_modifier(&child) {
                Some("public".to_string())
            } else {
                None
            };
            fields.push(FieldDecl {
                name,
                type_annotation,
                visibility,
                is_static: false,
                default_value: None,
            });
        }
        if !cursor.goto_next_sibling() {
            break;
        }
    }
    fields
}

fn extract_rust_call_args(call_node: &Node, source: &[u8]) -> Vec<String> {
    let mut args = Vec::new();
    let Some(arguments) = call_node.child_by_field_name("arguments") else {
        return args;
    };
    let mut cursor = arguments.walk();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            let text = get_node_text(&child, source);
            if !text.is_empty() && text != "(" && text != ")" && text != "," {
                args.push(text.to_string());
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    args
}

/// Recursively collect TODO/FIXME/HACK/XXX markers from comment nodes inside a subtree.
fn collect_todo_comments(node: &Node, source: &[u8]) -> Vec<String> {
    let mut todos = Vec::new();
    let mut stack = vec![*node];
    while let Some(n) = stack.pop() {
        if n.kind() == "line_comment" || n.kind() == "block_comment" {
            let text = get_node_text(&n, source);
            if text.contains("TODO")
                || text.contains("FIXME")
                || text.contains("HACK")
                || text.contains("XXX")
            {
                todos.push(text.trim().to_string());
            }
        }
        for i in 0..n.child_count() {
            if let Some(child) = n.child(i as u32) {
                stack.push(child);
            }
        }
    }
    todos
}

/// Count comment nodes (line_comment and block_comment) in the tree.
fn count_comment_nodes(root: &Node) -> usize {
    let mut count = 0;
    let mut stack = vec![*root];
    while let Some(n) = stack.pop() {
        if n.kind() == "line_comment" || n.kind() == "block_comment" {
            // Count lines covered by the comment node
            let lines = n.end_position().row - n.start_position().row + 1;
            count += lines;
        }
        for i in 0..n.child_count() {
            if let Some(child) = n.child(i as u32) {
                stack.push(child);
            }
        }
    }
    count
}

/// Walk up from a node to find the enclosing `impl_item` and extract:
/// - The type being implemented (from the `type` field)
/// - The trait being implemented, if any (from the `trait` field in `impl Trait for Type`)
///
/// Returns `(impl_type_name, impl_trait_name)`.
fn extract_impl_context(node: &Node, source: &[u8]) -> (Option<String>, Option<String>) {
    let mut curr = node.parent();
    while let Some(parent) = curr {
        if parent.kind() == "impl_item" {
            let type_name = parent
                .child_by_field_name("type")
                .map(|t| get_node_text(&t, source).to_string());
            let trait_name = parent
                .child_by_field_name("trait")
                .map(|t| get_node_text(&t, source).to_string());
            return (type_name, trait_name);
        }
        curr = parent.parent();
    }
    (None, None)
}

/// Pre-scan Rust files to build an imports_map: name -> list of file paths.
pub fn pre_scan_rust(
    files: &[std::path::PathBuf],
) -> HashMap<String, Vec<String>> {
    let mut imports_map: HashMap<String, Vec<String>> =
        HashMap::new();
    let ts_lang: TsLanguage = tree_sitter_rust::LANGUAGE.into();
    let query_str = r#"
        (struct_item name: (type_identifier) @name)
        (enum_item name: (type_identifier) @name)
        (function_item name: (identifier) @name)
        (trait_item name: (type_identifier) @name)
    "#;
    let query = match Query::new(&ts_lang, query_str) {
        Ok(q) => q,
        Err(_) => return imports_map,
    };

    let mut parser = Parser::new();
    parser.set_language(&ts_lang).ok();

    for file_path in files {
        let source = match std::fs::read(file_path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let tree = match parser.parse(&source, None) {
            Some(t) => t,
            None => continue,
        };
        let root = tree.root_node();
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, root, source.as_slice());
        while let Some(m) = { matches.advance(); matches.get() } {
            for cap in m.captures {
                let name = get_node_text(&cap.node, &source).to_string();
                let path_str = file_path.to_string_lossy().to_string();
                imports_map.entry(name).or_default().push(path_str);
            }
        }
    }
    imports_map
}
