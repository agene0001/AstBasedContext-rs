use std::collections::HashSet;
use std::path::Path;

use streaming_iterator::StreamingIterator;
use tree_sitter::{Language as TsLanguage, Node, Parser, Query, QueryCursor};

use crate::error::{Error, Result};
use crate::types::node::*;
use crate::types::{FileParseResult, Language};

use super::common::*;
use super::LanguageParser;

// Tree-sitter query strings for JavaScript.
const Q_FUNCTIONS: &str = r#"
    (function_declaration
        name: (identifier) @name
        parameters: (formal_parameters) @params) @function_node
    (method_definition
        name: (property_identifier) @name
        parameters: (formal_parameters) @params) @function_node
"#;

const Q_CLASSES: &str = r#"
    (class_declaration
        name: (identifier) @name) @class_node
"#;

const Q_IMPORTS: &str = r#"
    (import_statement) @import
"#;

const Q_CALLS: &str = r#"
    (call_expression
        function: (identifier) @name)
    (call_expression
        function: (member_expression
            property: (property_identifier) @name))
    (new_expression
        constructor: (identifier) @name)
"#;

const Q_VARIABLES: &str = r#"
    (variable_declarator
        name: (identifier) @name)
"#;

/// JavaScript-specific complexity node types.
const JS_COMPLEXITY_KINDS: &[&str] = &[
    "if_statement",
    "for_statement",
    "while_statement",
    "do_statement",
    "switch_statement",
    "catch_clause",
    "conditional_expression",
    "binary_expression",
];

/// Compiled queries, created once per JavaScriptParser instance.
struct JavaScriptQueries {
    functions: Query,
    classes: Query,
    imports: Query,
    calls: Query,
    variables: Query,
}

impl JavaScriptQueries {
    fn new(ts_lang: &TsLanguage) -> Result<Self> {
        let mk = |src: &str| {
            Query::new(ts_lang, src).map_err(|e| Error::Query(format!("{e}")))
        };
        Ok(Self {
            functions: mk(Q_FUNCTIONS)?,
            classes: mk(Q_CLASSES)?,
            imports: mk(Q_IMPORTS)?,
            calls: mk(Q_CALLS)?,
            variables: mk(Q_VARIABLES)?,
        })
    }
}

pub struct JavaScriptParser {
    ts_language: TsLanguage,
    queries: JavaScriptQueries,
}

impl Default for JavaScriptParser {
    fn default() -> Self {
        Self::new()
    }
}

impl JavaScriptParser {
    pub fn new() -> Self {
        let ts_language: TsLanguage = tree_sitter_javascript::LANGUAGE.into();
        let queries = JavaScriptQueries::new(&ts_language)
            .expect("built-in JavaScript queries must compile");
        Self {
            ts_language,
            queries,
        }
    }

    fn make_parser(&self) -> Parser {
        let mut parser = Parser::new();
        parser
            .set_language(&self.ts_language)
            .expect("JavaScript language must load");
        parser
    }

    // ── extraction helpers ───────────────────────────────────────────────

    fn find_functions(&self, source: &[u8], root: &Node, path: &Path, cursor: &mut QueryCursor) -> Vec<FunctionData> {
        let mut functions = Vec::new();
        let Some(name_idx) = self.queries.functions.capture_index_for_name("name") else { return functions; };
        let Some(func_node_idx) = self.queries.functions.capture_index_for_name("function_node") else { return functions; };

        let mut matches = cursor.matches(&self.queries.functions, *root, source);
        while let Some(m) = { matches.advance(); matches.get() } {
            // Find the function_node capture for this match
            let func_node_cap = m.captures.iter().find(|c| c.index == func_node_idx);
            let name_cap = m.captures.iter().find(|c| c.index == name_idx);

            let (Some(name_cap), Some(func_cap)) = (name_cap, func_node_cap) else {
                continue;
            };

            let node = name_cap.node;
            let func_node = func_cap.node;
            let name = get_node_text(&node, source).to_string();

            let params_node = func_node.child_by_field_name("parameters");
            let args = extract_js_params(params_node.as_ref(), source);

            let complexity = calculate_cyclomatic_complexity(&func_node, JS_COMPLEXITY_KINDS);

            let ctx = get_parent_context(
                &func_node,
                source,
                &["function_declaration", "class_declaration", "method_definition"],
            );
            let class_ctx = get_parent_context(&func_node, source, &["class_declaration"]);

            let arg_types = vec![None; args.len()];
            functions.push(FunctionData {
                name,
                path: path.to_path_buf(),
                span: SourceSpan {
                    start_line: func_node.start_position().row as u32 + 1,
                    end_line: func_node.end_position().row as u32 + 1,
                    start_col: func_node.start_position().column as u32,
                    end_col: func_node.end_position().column as u32,
                },
                args,
                arg_types,
                return_type: None,
                visibility: None,
                is_static: false,
                is_abstract: false,
                cyclomatic_complexity: complexity,
                decorators: Vec::new(),
                context: ctx.as_ref().map(|(n, _, _)| n.clone()),
                context_type: ctx.as_ref().map(|(_, t, _)| t.clone()),
                class_context: class_ctx.map(|(n, _, _)| n),
                language: Language::JavaScript,
                is_dependency: false,
                source: None,
                docstring: None,
                is_async: func_node.kind().contains("async") || {
                    let prev = func_node.prev_sibling();
                    prev.is_some_and(|n| get_node_text(&n, source) == "async")
                },
                todo_comments: vec![],
                raises: vec![],
                has_error_handling: false,
            });
        }
        functions
    }

    fn find_classes(&self, source: &[u8], root: &Node, path: &Path, cursor: &mut QueryCursor) -> Vec<ClassData> {
        let mut classes = Vec::new();
        let Some(name_idx) = self.queries.classes.capture_index_for_name("name") else { return classes; };
        let Some(class_node_idx) = self.queries.classes.capture_index_for_name("class_node") else { return classes; };

        let mut matches = cursor.matches(&self.queries.classes, *root, source);
        while let Some(m) = { matches.advance(); matches.get() } {
            let name_cap = m.captures.iter().find(|c| c.index == name_idx);
            let class_cap = m.captures.iter().find(|c| c.index == class_node_idx);

            let (Some(name_cap), Some(class_cap)) = (name_cap, class_cap) else {
                continue;
            };

            let node = name_cap.node;
            let class_node = class_cap.node;
            let name = get_node_text(&node, source).to_string();

            // Extract base classes from class_heritage
            let bases = extract_js_class_heritage(&class_node, source);

            let ctx = get_parent_context(
                &class_node,
                source,
                &["function_declaration", "class_declaration"],
            );

            classes.push(ClassData {
                name,
                path: path.to_path_buf(),
                span: SourceSpan {
                    start_line: class_node.start_position().row as u32 + 1,
                    end_line: class_node.end_position().row as u32 + 1,
                    start_col: class_node.start_position().column as u32,
                    end_col: class_node.end_position().column as u32,
                },
                bases,
                fields: Vec::new(),
                decorators: Vec::new(),
                context: ctx.map(|(n, _, _)| n),
                language: Language::JavaScript,
                is_dependency: false,
                source: None,
                docstring: None,
            });
        }
        classes
    }

    fn find_imports(&self, source: &[u8], root: &Node, cursor: &mut QueryCursor) -> Vec<ImportData> {
        let mut imports = Vec::new();
        let mut seen = HashSet::new();
        let Some(import_idx) = self.queries.imports.capture_index_for_name("import") else { return imports; };

        let mut matches = cursor.matches(&self.queries.imports, *root, source);
        while let Some(m) = { matches.advance(); matches.get() } {
            for cap in m.captures {
                if cap.index != import_idx {
                    continue;
                }
                let import_node = cap.node;
                let line_number = import_node.start_position().row as u32 + 1;

                // Extract the source string (the module path)
                let source_str = find_child_by_kind(&import_node, "string")
                    .map(|n| strip_js_string_quotes(get_node_text(&n, source)));

                let module_name = source_str.unwrap_or_default();

                // Also check for CommonJS require patterns via the full text
                // For ES6 imports, iterate children to find import clauses
                let mut found_any = false;

                let mut child_cursor = import_node.walk();
                if child_cursor.goto_first_child() {
                    loop {
                        let child = child_cursor.node();
                        if child.kind() == "import_clause" {
                            // import_clause can contain: identifier (default), named_imports, namespace_import
                            let mut clause_cursor = child.walk();
                            if clause_cursor.goto_first_child() {
                                loop {
                                    let clause_child = clause_cursor.node();
                                    match clause_child.kind() {
                                        "identifier" => {
                                            // Default import
                                            let name = get_node_text(&clause_child, source).to_string();
                                            let full = format!("{module_name}:{name}");
                                            if !seen.contains(&full) {
                                                seen.insert(full.clone());
                                                imports.push(ImportData {
                                                    name,
                                                    full_import_name: Some(module_name.clone()),
                                                    line_number,
                                                    alias: None,
                                                    language: Language::JavaScript,
                                                    is_dependency: false,
                                                });
                                                found_any = true;
                                            }
                                        }
                                        "named_imports" => {
                                            // { foo, bar as baz }
                                            let mut named_cursor = clause_child.walk();
                                            if named_cursor.goto_first_child() {
                                                loop {
                                                    let spec = named_cursor.node();
                                                    if spec.kind() == "import_specifier" {
                                                        let spec_name = spec.child_by_field_name("name")
                                                            .map(|n| get_node_text(&n, source).to_string());
                                                        let spec_alias = spec.child_by_field_name("alias")
                                                            .map(|n| get_node_text(&n, source).to_string());
                                                        if let Some(name) = spec_name {
                                                            let full = format!("{module_name}:{name}");
                                                            if !seen.contains(&full) {
                                                                seen.insert(full.clone());
                                                                imports.push(ImportData {
                                                                    name,
                                                                    full_import_name: Some(module_name.clone()),
                                                                    line_number,
                                                                    alias: spec_alias,
                                                                    language: Language::JavaScript,
                                                                    is_dependency: false,
                                                                });
                                                                found_any = true;
                                                            }
                                                        }
                                                    }
                                                    if !named_cursor.goto_next_sibling() {
                                                        break;
                                                    }
                                                }
                                            }
                                        }
                                        "namespace_import" => {
                                            // import * as name
                                            if let Some(alias_node) = clause_child.child_by_field_name("name") {
                                                let alias_name = get_node_text(&alias_node, source).to_string();
                                                let full = format!("{module_name}:*");
                                                if !seen.contains(&full) {
                                                    seen.insert(full.clone());
                                                    imports.push(ImportData {
                                                        name: "*".to_string(),
                                                        full_import_name: Some(module_name.clone()),
                                                        line_number,
                                                        alias: Some(alias_name),
                                                        language: Language::JavaScript,
                                                        is_dependency: false,
                                                    });
                                                    found_any = true;
                                                }
                                            }
                                        }
                                        _ => {}
                                    }
                                    if !clause_cursor.goto_next_sibling() {
                                        break;
                                    }
                                }
                            }
                        }
                        if !child_cursor.goto_next_sibling() {
                            break;
                        }
                    }
                }

                // Side-effect import: import 'module' (no import clause)
                if !found_any && !module_name.is_empty() {
                    let full = module_name.clone();
                    if !seen.contains(&full) {
                        seen.insert(full);
                        imports.push(ImportData {
                            name: module_name.clone(),
                            full_import_name: Some(module_name),
                            line_number,
                            alias: None,
                            language: Language::JavaScript,
                            is_dependency: false,
                        });
                    }
                }
            }
        }
        imports
    }

    fn find_calls(&self, source: &[u8], root: &Node, cursor: &mut QueryCursor) -> Vec<FunctionCallData> {
        let mut calls = Vec::new();
        let Some(name_idx) = self.queries.calls.capture_index_for_name("name") else { return calls; };

        let mut matches = cursor.matches(&self.queries.calls, *root, source);
        while let Some(m) = { matches.advance(); matches.get() } {
            for cap in m.captures {
                if cap.index != name_idx {
                    continue;
                }
                let node = cap.node;

                // Walk up to the call_expression or new_expression node
                let call_node = {
                    let Some(mut p) = node.parent() else { continue; };
                    while p.kind() != "call_expression" && p.kind() != "new_expression" {
                        p = match p.parent() {
                            Some(pp) => pp,
                            None => break,
                        };
                    }
                    p
                };

                let full_name = if call_node.kind() == "call_expression" {
                    call_node
                        .child_by_field_name("function")
                        .map(|n| get_node_text(&n, source).to_string())
                        .unwrap_or_else(|| get_node_text(&node, source).to_string())
                } else if call_node.kind() == "new_expression" {
                    call_node
                        .child_by_field_name("constructor")
                        .map(|n| format!("new {}", get_node_text(&n, source)))
                        .unwrap_or_else(|| get_node_text(&node, source).to_string())
                } else {
                    get_node_text(&node, source).to_string()
                };

                let args = extract_js_call_args(&call_node, source);

                let ctx = get_parent_context(
                    &node,
                    source,
                    &["function_declaration", "class_declaration", "method_definition"],
                );

                calls.push(FunctionCallData {
                    name: get_node_text(&node, source).to_string(),
                    full_name,
                    line_number: node.start_position().row as u32 + 1,
                    args,
                    inferred_obj_type: None,
                    context: ctx,
                    language: Language::JavaScript,
                });
            }
        }
        calls
    }

    fn find_variables(&self, source: &[u8], root: &Node, path: &Path, cursor: &mut QueryCursor) -> Vec<VariableData> {
        let mut variables = Vec::new();
        let Some(name_idx) = self.queries.variables.capture_index_for_name("name") else { return variables; };

        let mut matches = cursor.matches(&self.queries.variables, *root, source);
        while let Some(m) = { matches.advance(); matches.get() } {
            for cap in m.captures {
                if cap.index != name_idx {
                    continue;
                }
                let node = cap.node;
                let Some(declarator) = node.parent() else { continue; };

                // Skip if the value is a function/arrow function (those are handled by find_functions)
                let value_node = declarator.child_by_field_name("value");
                if let Some(ref val) = value_node {
                    let kind = val.kind();
                    if kind == "function" || kind == "arrow_function" {
                        continue;
                    }
                }

                let name = get_node_text(&node, source).to_string();
                let value = value_node.as_ref().map(|v| get_node_text(v, source).to_string());

                let ctx = get_parent_context(
                    &node,
                    source,
                    &["function_declaration", "class_declaration", "method_definition"],
                );
                let class_ctx = get_parent_context(&node, source, &["class_declaration"]);

                variables.push(VariableData {
                    name,
                    path: path.to_path_buf(),
                    line_number: node.start_position().row as u32 + 1,
                    value,
                    type_annotation: None,
                    context: ctx.map(|(n, _, _)| n),
                    class_context: class_ctx.map(|(n, _, _)| n),
                    language: Language::JavaScript,
                    is_dependency: false,
                });
            }
        }
        variables
    }
}

impl LanguageParser for JavaScriptParser {
    fn language(&self) -> Language {
        Language::JavaScript
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
        let classes = self.find_classes(source, &root, path, &mut cursor);
        let imports = self.find_imports(source, &root, &mut cursor);
        let function_calls = self.find_calls(source, &root, &mut cursor);
        let variables = self.find_variables(source, &root, path, &mut cursor);

        let mut result = FileParseResult::new(path.to_path_buf(), Language::JavaScript, is_dependency);
        result.functions = functions;
        result.classes = classes;
        result.variables = variables;
        result.imports = imports;
        result.function_calls = function_calls;
        result.total_lines = source.split(|&b| b == b'\n').count();
        result.comment_line_count = count_comment_lines(&root);
        result.is_test_file = path.to_string_lossy().contains("test");
        Ok(result)
    }
}

// ── free helpers ──────────────────────────────────────────────────────────

fn extract_js_params(params_node: Option<&Node>, source: &[u8]) -> Vec<String> {
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
            "identifier" => {
                args.push(get_node_text(&child, source).to_string());
            }
            "assignment_pattern" => {
                if let Some(left) = child.child_by_field_name("left") {
                    args.push(get_node_text(&left, source).to_string());
                }
            }
            "rest_pattern" => {
                args.push(get_node_text(&child, source).to_string());
            }
            "object_pattern" | "array_pattern" => {
                args.push(get_node_text(&child, source).to_string());
            }
            _ => {}
        }
        if !cursor.goto_next_sibling() {
            break;
        }
    }
    args
}

fn extract_js_class_heritage(class_node: &Node, source: &[u8]) -> Vec<String> {
    let mut bases = Vec::new();
    let mut cursor = class_node.walk();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            if child.kind() == "class_heritage" {
                // class_heritage contains "extends Foo"
                let mut hcursor = child.walk();
                if hcursor.goto_first_child() {
                    loop {
                        let hchild = hcursor.node();
                        if hchild.kind() == "identifier" || hchild.kind() == "member_expression" {
                            bases.push(get_node_text(&hchild, source).to_string());
                        }
                        if !hcursor.goto_next_sibling() {
                            break;
                        }
                    }
                }
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    bases
}

fn find_child_by_kind<'a>(node: &'a Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            if child.kind() == kind {
                return Some(child);
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    None
}

fn strip_js_string_quotes(s: &str) -> String {
    let s = s.trim();
    for delim in &["\"", "'", "`"] {
        if s.starts_with(delim) && s.ends_with(delim) && s.len() >= 2 * delim.len() {
            return s[delim.len()..s.len() - delim.len()].to_string();
        }
    }
    s.to_string()
}

fn extract_js_call_args(call_node: &Node, source: &[u8]) -> Vec<String> {
    let mut args = Vec::new();
    let arguments = call_node
        .child_by_field_name("arguments")
        .or_else(|| {
            // new_expression may not have an "arguments" field name, search by kind
            find_child_by_kind(call_node, "arguments")
        });
    let Some(arguments) = arguments else {
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

/// Pre-scan JavaScript files to build an imports_map: name -> list of file paths.
pub fn pre_scan_javascript(
    files: &[std::path::PathBuf],
) -> std::collections::HashMap<String, Vec<String>> {
    let mut imports_map: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    let ts_lang: TsLanguage = tree_sitter_javascript::LANGUAGE.into();
    let query_str = r#"
        (class_declaration name: (identifier) @name)
        (function_declaration name: (identifier) @name)
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

/// Count the number of lines covered by comment nodes in the tree.
fn count_comment_lines(root: &Node) -> usize {
    let mut count = 0;
    let mut stack = vec![*root];
    while let Some(n) = stack.pop() {
        if n.kind().contains("comment") {
            count += n.end_position().row - n.start_position().row + 1;
        } else {
            for i in 0..n.child_count() {
                if let Some(child) = n.child(i as u32) {
                    stack.push(child);
                }
            }
        }
    }
    count
}
