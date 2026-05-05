use std::collections::HashSet;
use std::path::Path;

use streaming_iterator::StreamingIterator;
use tree_sitter::{Language as TsLanguage, Node, Parser, Query, QueryCursor};

use crate::error::{Error, Result};
use crate::types::node::*;
use crate::types::{FileParseResult, Language};

use super::common::*;
use super::LanguageParser;

const Q_FUNCTIONS: &str = r#"
    (function_definition name: (name) @name) @function_node
    (method_declaration name: (name) @name) @function_node
"#;

const Q_CLASSES: &str = r#"
    (class_declaration name: (name) @name) @class_node
    (interface_declaration name: (name) @name) @class_node
    (trait_declaration name: (name) @name) @class_node
"#;

const Q_IMPORTS: &str = r#"
    (namespace_use_declaration) @import
"#;

const Q_CALLS: &str = r#"
    (function_call_expression function: (name) @name) @call_node
    (member_call_expression name: (name) @name) @call_node
"#;

const Q_VARIABLES: &str = r#"
    (variable_name (name) @name)
"#;

const PHP_COMPLEXITY_KINDS: &[&str] = &[
    "if_statement",
    "for_statement",
    "foreach_statement",
    "while_statement",
    "do_statement",
    "switch_statement",
    "catch_clause",
    "conditional_expression",
];

struct PhpQueries {
    functions: Query,
    classes: Query,
    imports: Query,
    calls: Query,
    variables: Query,
}

impl PhpQueries {
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

pub struct PhpParser {
    ts_language: TsLanguage,
    queries: PhpQueries,
}

impl Default for PhpParser {
    fn default() -> Self {
        Self::new()
    }
}

impl PhpParser {
    pub fn new() -> Self {
        let ts_language = tree_sitter_php::LANGUAGE_PHP.into();
        let queries = PhpQueries::new(&ts_language)
            .expect("built-in PHP queries must compile");
        Self {
            ts_language,
            queries,
        }
    }

    fn make_parser(&self) -> Parser {
        let mut parser = Parser::new();
        parser
            .set_language(&self.ts_language)
            .expect("PHP language must load");
        parser
    }

    fn find_functions(&self, source: &[u8], root: &Node, path: &Path, cursor: &mut QueryCursor) -> Vec<FunctionData> {
        let mut functions = Vec::new();
        let Some(name_idx) = self.queries.functions.capture_index_for_name("name") else { return functions; };

        let mut matches = cursor.matches(&self.queries.functions, *root, source);
        while let Some(m) = { matches.advance(); matches.get() } {
            for cap in m.captures {
                if cap.index != name_idx {
                    continue;
                }
                let node = cap.node;
                let Some(func_node) = node.parent() else { continue; };
                let name = get_node_text(&node, source).to_string();

                let params_node = func_node.child_by_field_name("parameters");
                let args = extract_php_params(params_node.as_ref(), source);
                let arg_types = extract_php_param_types(params_node.as_ref(), source, args.len());

                let return_type = func_node
                    .child_by_field_name("return_type")
                    .map(|t| get_node_text(&t, source).to_string());

                let modifiers = extract_php_modifiers(&func_node, source);
                let visibility = if modifiers.contains(&"public".to_string()) {
                    Some("public".to_string())
                } else if modifiers.contains(&"private".to_string()) {
                    Some("private".to_string())
                } else if modifiers.contains(&"protected".to_string()) {
                    Some("protected".to_string())
                } else {
                    None
                };
                let is_static = modifiers.contains(&"static".to_string());
                let is_abstract = modifiers.contains(&"abstract".to_string());

                let complexity = calculate_cyclomatic_complexity(&func_node, PHP_COMPLEXITY_KINDS);

                let ctx = get_parent_context(
                    &func_node,
                    source,
                    &["function_definition", "method_declaration", "class_declaration", "interface_declaration", "trait_declaration"],
                );
                let class_ctx = get_parent_context(
                    &func_node,
                    source,
                    &["class_declaration", "interface_declaration", "trait_declaration"],
                );

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
                    is_abstract,
                    cyclomatic_complexity: complexity,
                    decorators: Vec::new(),
                    context: ctx.as_ref().map(|(n, _, _)| n.clone()),
                    context_type: ctx.as_ref().map(|(_, t, _)| t.clone()),
                    class_context: class_ctx.map(|(n, _, _)| n),
                    language: Language::Php,
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

    fn find_classes(&self, source: &[u8], root: &Node, path: &Path, cursor: &mut QueryCursor) -> Vec<ClassData> {
        let mut classes = Vec::new();
        let Some(name_idx) = self.queries.classes.capture_index_for_name("name") else { return classes; };

        let mut matches = cursor.matches(&self.queries.classes, *root, source);
        while let Some(m) = { matches.advance(); matches.get() } {
            for cap in m.captures {
                if cap.index != name_idx {
                    continue;
                }
                let node = cap.node;
                let Some(class_node) = node.parent() else { continue; };
                let name = get_node_text(&node, source).to_string();

                let mut bases = Vec::new();

                // Check for base class (extends)
                if let Some(base) = class_node.child_by_field_name("base_clause") {
                    bases.push(get_node_text(&base, source).to_string());
                }

                // Check for interfaces (implements)
                if let Some(implements) = class_node.child_by_field_name("class_implements") {
                    let mut child_cursor = implements.walk();
                    if child_cursor.goto_first_child() {
                        loop {
                            let child = child_cursor.node();
                            if child.kind() == "named_type" || child.kind() == "name" {
                                bases.push(get_node_text(&child, source).to_string());
                            }
                            if !child_cursor.goto_next_sibling() {
                                break;
                            }
                        }
                    }
                }

                let ctx = get_parent_context(
                    &class_node,
                    source,
                    &["class_declaration", "interface_declaration", "trait_declaration"],
                );

                classes.push(ClassData {
                    name,
                    path: path.to_path_buf(),
                    span: SourceSpan {
                        start_line: node.start_position().row as u32 + 1,
                        end_line: class_node.end_position().row as u32 + 1,
                        start_col: node.start_position().column as u32,
                        end_col: class_node.end_position().column as u32,
                    },
                    bases,
                    decorators: Vec::new(),
                    fields: Vec::new(),
                    context: ctx.map(|(n, _, _)| n),
                    language: Language::Php,
                    is_dependency: false,
                    source: None,
                    docstring: None,
                });
            }
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
                let node = cap.node;
                let text = get_node_text(&node, source).to_string();

                // Strip "use " prefix, trailing ";", and replace "\" with "/"
                let clean = text
                    .trim()
                    .strip_prefix("use ")
                    .unwrap_or(text.trim())
                    .trim_end_matches(';')
                    .trim()
                    .replace('\\', "/");

                if seen.contains(&clean) {
                    continue;
                }
                seen.insert(clean.clone());

                let short_name = clean
                    .rsplit('/')
                    .next()
                    .unwrap_or(&clean)
                    .to_string();

                imports.push(ImportData {
                    name: short_name,
                    full_import_name: Some(clean),
                    line_number: node.start_position().row as u32 + 1,
                    alias: None,
                    language: Language::Php,
                    is_dependency: false,
                });
            }
        }
        imports
    }

    fn find_calls(&self, source: &[u8], root: &Node, cursor: &mut QueryCursor) -> Vec<FunctionCallData> {
        let mut calls = Vec::new();
        let Some(name_idx) = self.queries.calls.capture_index_for_name("name") else { return calls; };
        let Some(call_node_idx) = self.queries.calls.capture_index_for_name("call_node") else { return calls; };

        let mut matches = cursor.matches(&self.queries.calls, *root, source);
        while let Some(m) = { matches.advance(); matches.get() } {
            let mut name_text = None;
            let mut call_node_opt = None;

            for cap in m.captures {
                if cap.index == name_idx {
                    name_text = Some(get_node_text(&cap.node, source).to_string());
                }
                if cap.index == call_node_idx {
                    call_node_opt = Some(cap.node);
                }
            }

            if let (Some(name), Some(call_node)) = (name_text, call_node_opt) {
                let full_name = get_node_text(&call_node, source).to_string();
                let args = extract_php_call_args(&call_node, source);

                let ctx = get_parent_context(
                    &call_node,
                    source,
                    &["function_definition", "method_declaration", "class_declaration"],
                );

                calls.push(FunctionCallData {
                    name,
                    full_name,
                    line_number: call_node.start_position().row as u32 + 1,
                    args,
                    inferred_obj_type: None,
                    context: ctx,
                    language: Language::Php,
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
                let name = get_node_text(&node, source).to_string();

                // Walk up to find the assignment for value/type
                let var_name_node = node.parent(); // variable_name node
                let assignment = var_name_node.and_then(|v| v.parent());

                let value = assignment
                    .and_then(|a| a.child_by_field_name("right"))
                    .map(|v| get_node_text(&v, source).to_string());

                let type_annotation = assignment
                    .and_then(|a| a.child_by_field_name("type"))
                    .map(|t| get_node_text(&t, source).to_string());

                let ctx = get_parent_context(
                    &node,
                    source,
                    &["function_definition", "method_declaration", "class_declaration"],
                );
                let class_ctx = get_parent_context(
                    &node,
                    source,
                    &["class_declaration", "interface_declaration", "trait_declaration"],
                );

                variables.push(VariableData {
                    name,
                    path: path.to_path_buf(),
                    line_number: node.start_position().row as u32 + 1,
                    value,
                    type_annotation,
                    context: ctx.map(|(n, _, _)| n),
                    class_context: class_ctx.map(|(n, _, _)| n),
                    language: Language::Php,
                    is_dependency: false,
                });
            }
        }
        variables
    }
}

impl LanguageParser for PhpParser {
    fn language(&self) -> Language {
        Language::Php
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

        let mut result = FileParseResult::new(path.to_path_buf(), Language::Php, is_dependency);
        result.functions = functions;
        result.classes = classes;
        result.variables = variables;
        result.imports = imports;
        result.function_calls = function_calls;
        result.total_lines = source.split(|&b| b == b'\n').count();
        result.comment_line_count = count_comment_lines(&root);
        let path_str = path.to_string_lossy();
        result.is_test_file = path_str.contains("test") || path_str.contains("Test") || path_str.contains("spec");
        Ok(result)
    }
}

// ── free helpers ──────────────────────────────────────────────────────────

fn extract_php_params(params_node: Option<&Node>, source: &[u8]) -> Vec<String> {
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
        if child.kind() == "simple_parameter" || child.kind() == "variadic_parameter" {
            if let Some(name_node) = child.child_by_field_name("name") {
                // PHP variable names are stored as variable_name nodes; get the inner name
                let raw = get_node_text(&name_node, source);
                // Strip leading "$" from PHP variable names
                args.push(raw.trim_start_matches('$').to_string());
            }
        }
        if !cursor.goto_next_sibling() {
            break;
        }
    }
    args
}

fn extract_php_param_types(params_node: Option<&Node>, source: &[u8], arg_count: usize) -> Vec<Option<String>> {
    let Some(params) = params_node else {
        return vec![None; arg_count];
    };
    let mut types = Vec::new();
    let mut cursor = params.walk();
    if !cursor.goto_first_child() {
        return vec![None; arg_count];
    }
    loop {
        let child = cursor.node();
        if child.kind() == "simple_parameter" || child.kind() == "variadic_parameter" {
            let ty = child
                .child_by_field_name("type")
                .map(|t| get_node_text(&t, source).to_string());
            types.push(ty);
        }
        if !cursor.goto_next_sibling() {
            break;
        }
    }
    while types.len() < arg_count {
        types.push(None);
    }
    types
}

fn extract_php_modifiers(node: &Node, source: &[u8]) -> Vec<String> {
    let mut modifiers = Vec::new();
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            let kind = child.kind();
            // PHP modifier keywords appear as direct children with their kind as the modifier name
            if matches!(kind, "public" | "private" | "protected" | "static" | "abstract" | "final" | "readonly")
                || kind == "visibility_modifier"
            {
                modifiers.push(get_node_text(&child, source).to_string());
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    modifiers
}

fn extract_php_call_args(call_node: &Node, source: &[u8]) -> Vec<String> {
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
