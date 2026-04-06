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
    (method_declaration name: (identifier) @name parameters: (formal_parameters) @params) @function_node
    (constructor_declaration name: (identifier) @name parameters: (formal_parameters) @params) @function_node
"#;

const Q_CLASSES: &str = r#"
    (class_declaration name: (identifier) @name) @class_node
    (interface_declaration name: (identifier) @name) @class_node
    (enum_declaration name: (identifier) @name) @class_node
"#;

const Q_IMPORTS: &str = r#"
    (import_declaration) @import
"#;

const Q_CALLS: &str = r#"
    (method_invocation name: (identifier) @name) @call_node
    (object_creation_expression type: (type_identifier) @name) @call_node
"#;

const Q_VARIABLES: &str = r#"
    (local_variable_declaration declarator: (variable_declarator name: (identifier) @name))
    (field_declaration declarator: (variable_declarator name: (identifier) @name))
"#;

const JAVA_COMPLEXITY_KINDS: &[&str] = &[
    "if_statement",
    "for_statement",
    "while_statement",
    "do_statement",
    "switch_expression",
    "catch_clause",
    "conditional_expression",
    "binary_expression",
];

struct JavaQueries {
    functions: Query,
    classes: Query,
    imports: Query,
    calls: Query,
    variables: Query,
}

impl JavaQueries {
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

pub struct JavaParser {
    ts_language: TsLanguage,
    queries: JavaQueries,
}

impl JavaParser {
    pub fn new() -> Self {
        let ts_language = tree_sitter_java::LANGUAGE.into();
        let queries = JavaQueries::new(&ts_language)
            .expect("built-in Java queries must compile");
        Self {
            ts_language,
            queries,
        }
    }

    fn make_parser(&self) -> Parser {
        let mut parser = Parser::new();
        parser
            .set_language(&self.ts_language)
            .expect("Java language must load");
        parser
    }

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
                let args = extract_java_params(params_node.as_ref(), source);
                let arg_types = extract_java_param_types(params_node.as_ref(), source, args.len());

                let return_type = func_node
                    .child_by_field_name("type")
                    .map(|t| get_node_text(&t, source).to_string());

                let modifiers = extract_java_modifiers(&func_node, source);
                let visibility = if modifiers.contains(&"public".to_string()) {
                    Some("public".to_string())
                } else if modifiers.contains(&"private".to_string()) {
                    Some("private".to_string())
                } else if modifiers.contains(&"protected".to_string()) {
                    Some("protected".to_string())
                } else {
                    Some("package-private".to_string())
                };
                let is_static = modifiers.contains(&"static".to_string());
                let is_abstract = modifiers.contains(&"abstract".to_string());

                let complexity = calculate_cyclomatic_complexity(&func_node, JAVA_COMPLEXITY_KINDS);

                let ctx = get_parent_context(
                    &func_node,
                    source,
                    &["method_declaration", "constructor_declaration", "class_declaration", "interface_declaration", "enum_declaration"],
                );
                let class_ctx = get_parent_context(
                    &func_node,
                    source,
                    &["class_declaration", "interface_declaration", "enum_declaration"],
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
                    language: Language::Java,
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
        let name_idx = self.queries.classes.capture_index_for_name("name").unwrap();

        let mut matches = cursor.matches(&self.queries.classes, *root, source);
        while let Some(m) = { matches.advance(); matches.get() } {
            for cap in m.captures {
                if cap.index != name_idx {
                    continue;
                }
                let node = cap.node;
                let class_node = node.parent().unwrap();
                let name = get_node_text(&node, source).to_string();

                let mut bases = Vec::new();

                // Check for superclass (extends)
                if let Some(superclass) = class_node.child_by_field_name("superclass") {
                    bases.push(get_node_text(&superclass, source).to_string());
                }

                // Check for interfaces (implements)
                if let Some(interfaces) = class_node.child_by_field_name("interfaces") {
                    let mut child_cursor = interfaces.walk();
                    if child_cursor.goto_first_child() {
                        loop {
                            let child = child_cursor.node();
                            if child.kind() == "type_identifier" || child.kind() == "generic_type" {
                                bases.push(get_node_text(&child, source).to_string());
                            }
                            if !child_cursor.goto_next_sibling() {
                                break;
                            }
                        }
                    }
                }

                // For interfaces: check extends_type_list via "type_list" field on extends
                let mut child_cursor = class_node.walk();
                if child_cursor.goto_first_child() {
                    loop {
                        let child = child_cursor.node();
                        if child.kind() == "extends_interfaces" || child.kind() == "type_list" {
                            let mut inner_cursor = child.walk();
                            if inner_cursor.goto_first_child() {
                                loop {
                                    let inner_child = inner_cursor.node();
                                    if inner_child.kind() == "type_identifier" || inner_child.kind() == "generic_type" {
                                        bases.push(get_node_text(&inner_child, source).to_string());
                                    }
                                    if !inner_cursor.goto_next_sibling() {
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

                let ctx = get_parent_context(
                    &class_node,
                    source,
                    &["class_declaration", "interface_declaration", "enum_declaration"],
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
                    language: Language::Java,
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
        let import_idx = self.queries.imports.capture_index_for_name("import").unwrap();

        let mut matches = cursor.matches(&self.queries.imports, *root, source);
        while let Some(m) = { matches.advance(); matches.get() } {
            for cap in m.captures {
                if cap.index != import_idx {
                    continue;
                }
                let node = cap.node;
                let text = get_node_text(&node, source).to_string();

                // Strip "import " prefix and trailing ";"
                let clean = text
                    .trim()
                    .strip_prefix("import ")
                    .unwrap_or(&text)
                    .trim_end_matches(';')
                    .trim()
                    .to_string();

                // Strip "static " if present
                let clean = clean
                    .strip_prefix("static ")
                    .unwrap_or(&clean)
                    .to_string();

                if seen.contains(&clean) {
                    continue;
                }
                seen.insert(clean.clone());

                // The short name is the last segment
                let short_name = clean
                    .rsplit('.')
                    .next()
                    .unwrap_or(&clean)
                    .to_string();

                imports.push(ImportData {
                    name: short_name,
                    full_import_name: Some(clean),
                    line_number: node.start_position().row as u32 + 1,
                    alias: None,
                    language: Language::Java,
                    is_dependency: false,
                });
            }
        }
        imports
    }

    fn find_calls(&self, source: &[u8], root: &Node, cursor: &mut QueryCursor) -> Vec<FunctionCallData> {
        let mut calls = Vec::new();
        let name_idx = self.queries.calls.capture_index_for_name("name").unwrap();
        let call_node_idx = self.queries.calls.capture_index_for_name("call_node").unwrap();

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
                let args = extract_java_call_args(&call_node, source);

                let ctx = get_parent_context(
                    &call_node,
                    source,
                    &["method_declaration", "constructor_declaration", "class_declaration"],
                );

                calls.push(FunctionCallData {
                    name,
                    full_name,
                    line_number: call_node.start_position().row as u32 + 1,
                    args,
                    inferred_obj_type: None,
                    context: ctx,
                    language: Language::Java,
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

                // The variable_declarator parent may have a value child
                let declarator = node.parent();
                let value = declarator
                    .and_then(|d| d.child_by_field_name("value"))
                    .map(|v| get_node_text(&v, source).to_string());

                // Get type from the declaration parent (local_variable_declaration or field_declaration)
                let type_annotation = declarator
                    .and_then(|d| d.parent())
                    .and_then(|decl| decl.child_by_field_name("type"))
                    .map(|t| get_node_text(&t, source).to_string());

                let ctx = get_parent_context(
                    &node,
                    source,
                    &["method_declaration", "constructor_declaration", "class_declaration"],
                );
                let class_ctx = get_parent_context(
                    &node,
                    source,
                    &["class_declaration", "interface_declaration", "enum_declaration"],
                );

                variables.push(VariableData {
                    name,
                    path: path.to_path_buf(),
                    line_number: node.start_position().row as u32 + 1,
                    value,
                    type_annotation,
                    context: ctx.map(|(n, _, _)| n),
                    class_context: class_ctx.map(|(n, _, _)| n),
                    language: Language::Java,
                    is_dependency: false,
                });
            }
        }
        variables
    }
}

impl LanguageParser for JavaParser {
    fn language(&self) -> Language {
        Language::Java
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

        let mut result = FileParseResult::new(path.to_path_buf(), Language::Java, is_dependency);
        result.functions = functions;
        result.classes = classes;
        result.variables = variables;
        result.imports = imports;
        result.function_calls = function_calls;
        result.total_lines = source.split(|&b| b == b'\n').count();
        result.comment_line_count = 0; // TODO: count comment nodes
        result.is_test_file = path.to_string_lossy().contains("test");
        Ok(result)
    }
}

// ── free helpers ──────────────────────────────────────────────────────────

fn extract_java_params(params_node: Option<&Node>, source: &[u8]) -> Vec<String> {
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
        if child.kind() == "formal_parameter" || child.kind() == "spread_parameter" {
            if let Some(name_node) = child.child_by_field_name("name") {
                args.push(get_node_text(&name_node, source).to_string());
            }
        }
        if !cursor.goto_next_sibling() {
            break;
        }
    }
    args
}

fn extract_java_param_types(params_node: Option<&Node>, source: &[u8], arg_count: usize) -> Vec<Option<String>> {
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
        if child.kind() == "formal_parameter" || child.kind() == "spread_parameter" {
            let ty = child
                .child_by_field_name("type")
                .map(|t| get_node_text(&t, source).to_string());
            types.push(ty);
        }
        if !cursor.goto_next_sibling() {
            break;
        }
    }
    // Pad if somehow mismatched
    while types.len() < arg_count {
        types.push(None);
    }
    types
}

fn extract_java_modifiers(node: &Node, source: &[u8]) -> Vec<String> {
    let mut modifiers = Vec::new();
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            if child.kind() == "modifiers" {
                let mut inner = child.walk();
                if inner.goto_first_child() {
                    loop {
                        let m = inner.node();
                        let text = get_node_text(&m, source).to_string();
                        if !text.is_empty() {
                            modifiers.push(text);
                        }
                        if !inner.goto_next_sibling() {
                            break;
                        }
                    }
                }
                break;
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    modifiers
}

fn extract_java_call_args(call_node: &Node, source: &[u8]) -> Vec<String> {
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
