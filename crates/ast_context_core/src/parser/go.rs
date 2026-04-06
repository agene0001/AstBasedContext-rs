use std::collections::HashSet;
use std::path::Path;

use streaming_iterator::StreamingIterator;
use tree_sitter::{Language as TsLanguage, Node, Parser, Query, QueryCursor};

use crate::error::{Error, Result};
use crate::types::node::*;
use crate::types::{FileParseResult, Language};

use super::common::*;
use super::LanguageParser;

// ── Tree-sitter query strings ────────────────────────────────────────────

const Q_FUNCTIONS: &str = r#"
    (function_declaration
        name: (identifier) @name
        parameters: (parameter_list) @params) @function_node
    (method_declaration
        receiver: (parameter_list) @receiver
        name: (field_identifier) @name
        parameters: (parameter_list) @params) @function_node
"#;

const Q_STRUCTS: &str = r#"
    (type_declaration
        (type_spec
            name: (type_identifier) @name
            type: (struct_type) @struct_body)) @struct_node
"#;

const Q_INTERFACES: &str = r#"
    (type_declaration
        (type_spec
            name: (type_identifier) @name
            type: (interface_type) @interface_body)) @interface_node
"#;

const Q_IMPORTS: &str = r#"
    (import_declaration
        (import_spec
            path: (interpreted_string_literal) @path)) @import
"#;

const Q_CALLS: &str = r#"
    (call_expression
        function: (identifier) @name)
    (call_expression
        function: (selector_expression
            field: (field_identifier) @name))
"#;

const Q_VARIABLES: &str = r#"
    (var_declaration
        (var_spec
            name: (identifier) @name))
    (short_var_declaration
        left: (expression_list
            (identifier) @name))
"#;

/// Complexity-contributing node types for Go.
const GO_COMPLEXITY_KINDS: &[&str] = &[
    "if_statement",
    "for_statement",
    "switch_statement",
    "case_clause",
    "expression_switch_statement",
    "type_switch_statement",
    "binary_expression",
];

/// Compiled queries, created once per GoParser instance.
struct GoQueries {
    functions: Query,
    structs: Query,
    interfaces: Query,
    imports: Query,
    calls: Query,
    variables: Query,
}

impl GoQueries {
    fn new(ts_lang: &TsLanguage) -> Result<Self> {
        let mk = |src: &str| {
            Query::new(ts_lang, src).map_err(|e| Error::Query(format!("{e}")))
        };
        Ok(Self {
            functions: mk(Q_FUNCTIONS)?,
            structs: mk(Q_STRUCTS)?,
            interfaces: mk(Q_INTERFACES)?,
            imports: mk(Q_IMPORTS)?,
            calls: mk(Q_CALLS)?,
            variables: mk(Q_VARIABLES)?,
        })
    }
}

pub struct GoParser {
    ts_language: TsLanguage,
    queries: GoQueries,
}

impl GoParser {
    pub fn new() -> Self {
        let ts_language: TsLanguage = tree_sitter_go::LANGUAGE.into();
        let queries = GoQueries::new(&ts_language)
            .expect("built-in Go queries must compile");
        Self {
            ts_language,
            queries,
        }
    }

    fn make_parser(&self) -> Parser {
        let mut parser = Parser::new();
        parser
            .set_language(&self.ts_language)
            .expect("Go language must load");
        parser
    }

    // ── extraction helpers ───────────────────────────────────────────────

    fn find_functions(&self, source: &[u8], root: &Node, path: &Path, cursor: &mut QueryCursor) -> Vec<FunctionData> {
        let mut functions = Vec::new();
        let name_idx = self.queries.functions.capture_index_for_name("name").unwrap();

        let mut matches = cursor.matches(&self.queries.functions, *root, source);
        while let Some(m) = { matches.advance(); matches.get() } {
            for cap in m.captures {
                if cap.index != name_idx {
                    continue;
                }
                let node = cap.node;
                let func_node = node.parent().unwrap();
                let name = get_node_text(&node, source).to_string();

                let params_node = func_node.child_by_field_name("parameters");
                let args = extract_go_params(params_node.as_ref(), source);

                // For method_declaration, check receiver
                let receiver_text = if func_node.kind() == "method_declaration" {
                    func_node
                        .child_by_field_name("receiver")
                        .map(|r| get_node_text(&r, source).to_string())
                } else {
                    None
                };

                let complexity = calculate_cyclomatic_complexity(&func_node, GO_COMPLEXITY_KINDS);

                let ctx = get_parent_context(
                    &func_node,
                    source,
                    &["function_declaration", "method_declaration"],
                );

                // For Go methods, derive a class_context from the receiver type
                let class_context = receiver_text.as_ref().and_then(|r| {
                    extract_receiver_type(r)
                });

                let arg_types: Vec<Option<String>> = vec![None; args.len()];

                // Extract return type from the result field
                let return_type = func_node
                    .child_by_field_name("result")
                    .map(|r| get_node_text(&r, source).to_string());

                // Go visibility: uppercase first letter = public, lowercase = private
                let visibility = Some(go_visibility(&name));

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
                    is_static: false,
                    is_abstract: false,
                    cyclomatic_complexity: complexity,
                    decorators: Vec::new(),
                    context: ctx.as_ref().map(|(n, _, _)| n.clone()),
                    context_type: ctx.as_ref().map(|(_, t, _)| t.clone()),
                    class_context,
                    language: Language::Go,
                    is_dependency: false,
                    source: None,
                    docstring: None,
                    is_async: false,
                    todo_comments: vec![],
                    raises: vec![],
                    has_error_handling: false,
                });
            }
        }
        functions
    }

    fn find_structs(&self, source: &[u8], root: &Node, path: &Path, cursor: &mut QueryCursor) -> Vec<StructData> {
        let mut structs = Vec::new();
        let name_idx = self.queries.structs.capture_index_for_name("name").unwrap();

        let mut matches = cursor.matches(&self.queries.structs, *root, source);
        while let Some(m) = { matches.advance(); matches.get() } {
            for cap in m.captures {
                if cap.index != name_idx {
                    continue;
                }
                let node = cap.node;
                // Walk up to the type_declaration node
                let struct_node = {
                    let mut p = node.parent().unwrap();
                    while p.kind() != "type_declaration" {
                        p = p.parent().unwrap();
                    }
                    p
                };
                let name = get_node_text(&node, source).to_string();

                // Extract struct fields
                let fields = extract_go_struct_fields(&struct_node, source);

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
                    language: Language::Go,
                    is_dependency: false,
                    source: None,
                });
            }
        }
        structs
    }

    fn find_interfaces(&self, source: &[u8], root: &Node, path: &Path, cursor: &mut QueryCursor) -> Vec<InterfaceData> {
        let mut interfaces = Vec::new();
        let name_idx = self.queries.interfaces.capture_index_for_name("name").unwrap();

        let mut matches = cursor.matches(&self.queries.interfaces, *root, source);
        while let Some(m) = { matches.advance(); matches.get() } {
            for cap in m.captures {
                if cap.index != name_idx {
                    continue;
                }
                let node = cap.node;
                let iface_node = {
                    let mut p = node.parent().unwrap();
                    while p.kind() != "type_declaration" {
                        p = p.parent().unwrap();
                    }
                    p
                };
                let name = get_node_text(&node, source).to_string();

                interfaces.push(InterfaceData {
                    name,
                    path: path.to_path_buf(),
                    span: SourceSpan {
                        start_line: node.start_position().row as u32 + 1,
                        end_line: iface_node.end_position().row as u32 + 1,
                        start_col: node.start_position().column as u32,
                        end_col: iface_node.end_position().column as u32,
                    },
                    bases: Vec::new(),
                    language: Language::Go,
                    is_dependency: false,
                    source: None,
                });
            }
        }
        interfaces
    }

    fn find_imports(&self, source: &[u8], root: &Node, cursor: &mut QueryCursor) -> Vec<ImportData> {
        let mut imports = Vec::new();
        let mut seen = HashSet::new();
        let path_idx = self.queries.imports.capture_index_for_name("path").unwrap();

        let mut matches = cursor.matches(&self.queries.imports, *root, source);
        while let Some(m) = { matches.advance(); matches.get() } {
            for cap in m.captures {
                if cap.index != path_idx {
                    continue;
                }
                let node = cap.node;
                let raw_path = get_node_text(&node, source);

                // Strip surrounding quotes
                let import_path = raw_path
                    .trim()
                    .trim_matches('"')
                    .to_string();

                if seen.contains(&import_path) {
                    continue;
                }
                seen.insert(import_path.clone());

                // Short name is the last path segment
                let short_name = import_path
                    .rsplit('/')
                    .next()
                    .unwrap_or(&import_path)
                    .to_string();

                imports.push(ImportData {
                    name: short_name,
                    full_import_name: Some(import_path),
                    line_number: node.start_position().row as u32 + 1,
                    alias: None,
                    language: Language::Go,
                    is_dependency: false,
                });
            }
        }
        imports
    }

    fn find_calls(&self, source: &[u8], root: &Node, cursor: &mut QueryCursor) -> Vec<FunctionCallData> {
        let mut calls = Vec::new();
        let name_idx = self.queries.calls.capture_index_for_name("name").unwrap();

        let mut matches = cursor.matches(&self.queries.calls, *root, source);
        while let Some(m) = { matches.advance(); matches.get() } {
            for cap in m.captures {
                if cap.index != name_idx {
                    continue;
                }
                let node = cap.node;

                // Walk up to the call_expression node
                let call_node = {
                    let mut p = node.parent().unwrap();
                    while p.kind() != "call_expression" {
                        p = p.parent().unwrap();
                    }
                    p
                };
                let func_node = call_node.child_by_field_name("function").unwrap();

                let args = extract_go_call_args(&call_node, source);
                let ctx = get_parent_context(
                    &node,
                    source,
                    &["function_declaration", "method_declaration"],
                );

                calls.push(FunctionCallData {
                    name: get_node_text(&node, source).to_string(),
                    full_name: get_node_text(&func_node, source).to_string(),
                    line_number: node.start_position().row as u32 + 1,
                    args,
                    inferred_obj_type: None,
                    context: ctx,
                    language: Language::Go,
                });
            }
        }
        calls
    }

    fn find_variables(&self, source: &[u8], root: &Node, path: &Path, cursor: &mut QueryCursor) -> Vec<VariableData> {
        let mut variables = Vec::new();
        let name_idx = self.queries.variables.capture_index_for_name("name").unwrap();

        let mut matches = cursor.matches(&self.queries.variables, *root, source);
        while let Some(m) = { matches.advance(); matches.get() } {
            for cap in m.captures {
                if cap.index != name_idx {
                    continue;
                }
                let node = cap.node;
                let name = get_node_text(&node, source).to_string();

                // Walk up to the declaration node (var_declaration or short_var_declaration)
                let decl_node = {
                    let mut p = node.parent().unwrap();
                    while p.kind() != "var_declaration"
                        && p.kind() != "short_var_declaration"
                        && p.kind() != "var_spec"
                    {
                        if let Some(pp) = p.parent() {
                            p = pp;
                        } else {
                            break;
                        }
                    }
                    p
                };

                let value = if decl_node.kind() == "short_var_declaration" {
                    decl_node
                        .child_by_field_name("right")
                        .map(|r| get_node_text(&r, source).to_string())
                } else {
                    // var_spec may have a value child
                    decl_node
                        .child_by_field_name("value")
                        .map(|v| get_node_text(&v, source).to_string())
                };

                let type_annotation = decl_node
                    .child_by_field_name("type")
                    .map(|t| get_node_text(&t, source).to_string());

                let ctx = get_parent_context(
                    &node,
                    source,
                    &["function_declaration", "method_declaration"],
                );

                variables.push(VariableData {
                    name,
                    path: path.to_path_buf(),
                    line_number: node.start_position().row as u32 + 1,
                    value,
                    type_annotation,
                    context: ctx.map(|(n, _, _)| n),
                    class_context: None,
                    language: Language::Go,
                    is_dependency: false,
                });
            }
        }
        variables
    }
}

impl LanguageParser for GoParser {
    fn language(&self) -> Language {
        Language::Go
    }

    fn parse(&self, path: &Path, source: &[u8], is_dependency: bool) -> Result<FileParseResult> {
        let mut parser = self.make_parser();
        let tree = parser.parse(source, None).ok_or_else(|| Error::Parse {
            path: path.to_path_buf(),
            message: "tree-sitter failed to parse".into(),
        })?;
        let root = tree.root_node();
        let mut cursor = QueryCursor::new();

        let functions = self.find_functions(source, &root, path, &mut cursor);
        let structs = self.find_structs(source, &root, path, &mut cursor);
        let interfaces = self.find_interfaces(source, &root, path, &mut cursor);
        let imports = self.find_imports(source, &root, &mut cursor);
        let function_calls = self.find_calls(source, &root, &mut cursor);
        let variables = self.find_variables(source, &root, path, &mut cursor);

        let mut result = FileParseResult::new(path.to_path_buf(), Language::Go, is_dependency);
        result.functions = functions;
        result.structs = structs;
        result.interfaces = interfaces;
        result.imports = imports;
        result.function_calls = function_calls;
        result.variables = variables;
        result.total_lines = source.split(|&b| b == b'\n').count();
        result.comment_line_count = 0; // TODO: count comment nodes
        result.is_test_file = path.to_string_lossy().contains("test");
        Ok(result)
    }
}

// ── free helpers ──────────────────────────────────────────────────────────

fn extract_go_params(params_node: Option<&Node>, source: &[u8]) -> Vec<String> {
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
        if child.kind() == "parameter_declaration" {
            let text = get_node_text(&child, source).to_string();
            if !text.is_empty() {
                args.push(text);
            }
        }
        if !cursor.goto_next_sibling() {
            break;
        }
    }
    args
}

fn extract_go_call_args(call_node: &Node, source: &[u8]) -> Vec<String> {
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

/// Extract the type name from a Go method receiver string like "(s *Server)" or "(s Server)".
fn extract_receiver_type(receiver: &str) -> Option<String> {
    let inner = receiver.trim().trim_start_matches('(').trim_end_matches(')');
    // receiver is typically "varname Type" or "varname *Type"
    let parts: Vec<&str> = inner.split_whitespace().collect();
    if parts.len() >= 2 {
        Some(parts[1].trim_start_matches('*').to_string())
    } else if parts.len() == 1 {
        Some(parts[0].trim_start_matches('*').to_string())
    } else {
        None
    }
}

/// Determine Go visibility from the first character of a name.
fn go_visibility(name: &str) -> String {
    if name.starts_with(|c: char| c.is_uppercase()) {
        "public".to_string()
    } else {
        "private".to_string()
    }
}

/// Extract fields from a Go struct type_declaration node.
fn extract_go_struct_fields(type_decl_node: &Node, source: &[u8]) -> Vec<FieldDecl> {
    let mut fields = Vec::new();
    // Walk all descendants looking for field_declaration nodes
    let mut cursor = type_decl_node.walk();
    walk_descendants(&mut cursor, &mut |node| {
        if node.kind() == "field_declaration" {
            let name_node = node.child_by_field_name("name");
            let type_node = node.child_by_field_name("type");
            if let Some(n) = name_node {
                let fname = get_node_text(&n, source).to_string();
                let type_ann = type_node.map(|t| get_node_text(&t, source).to_string());
                let vis = Some(go_visibility(&fname));
                fields.push(FieldDecl {
                    name: fname,
                    type_annotation: type_ann,
                    visibility: vis,
                    is_static: false,
                });
            }
        }
    });
    fields
}

/// Walk all descendants of the cursor's current node, calling `f` on each.
fn walk_descendants(cursor: &mut tree_sitter::TreeCursor, f: &mut impl FnMut(Node)) {
    if !cursor.goto_first_child() {
        return;
    }
    loop {
        f(cursor.node());
        walk_descendants(cursor, f);
        if !cursor.goto_next_sibling() {
            break;
        }
    }
    cursor.goto_parent();
}

/// Pre-scan Go files to build an imports_map: name -> list of file paths.
pub fn pre_scan_go(
    files: &[std::path::PathBuf],
) -> std::collections::HashMap<String, Vec<String>> {
    let mut imports_map: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    let ts_lang: TsLanguage = tree_sitter_go::LANGUAGE.into();
    let query_str = r#"
        (type_declaration (type_spec name: (type_identifier) @name))
        (function_declaration name: (identifier) @name)
        (method_declaration name: (field_identifier) @name)
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
