use std::path::Path;

use streaming_iterator::StreamingIterator;
use tree_sitter::{Language as TsLanguage, Node, Parser, Query, QueryCursor};

use crate::error::{Error, Result};
use crate::types::node::*;
use crate::types::{FileParseResult, Language};

use super::common::*;
use super::LanguageParser;

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
        name: (type_identifier) @name) @class_node

    (abstract_class_declaration
        name: (type_identifier) @name) @class_node
"#;

const Q_INTERFACES: &str = r#"
    (interface_declaration
        name: (type_identifier) @name) @interface_node
"#;

const Q_IMPORTS: &str = r#"
    (import_statement) @import
"#;

const Q_CALLS: &str = r#"
    (call_expression function: (identifier) @name)
    (call_expression function: (member_expression property: (property_identifier) @name))
    (new_expression constructor: (identifier) @name)
"#;

const Q_VARIABLES: &str = r#"
    (variable_declarator name: (identifier) @name)
"#;

const TS_COMPLEXITY_KINDS: &[&str] = &[
    "if_statement",
    "for_statement",
    "for_in_statement",
    "while_statement",
    "do_statement",
    "switch_statement",
    "catch_clause",
    "conditional_expression",
    "binary_expression",
];

struct TsQueries {
    functions: Query,
    classes: Query,
    interfaces: Query,
    imports: Query,
    calls: Query,
    variables: Query,
}

impl TsQueries {
    fn new(ts_lang: &TsLanguage) -> Result<Self> {
        let mk = |src: &str| Query::new(ts_lang, src).map_err(|e| Error::Query(format!("{e}")));
        Ok(Self {
            functions: mk(Q_FUNCTIONS)?,
            classes: mk(Q_CLASSES)?,
            interfaces: mk(Q_INTERFACES)?,
            imports: mk(Q_IMPORTS)?,
            calls: mk(Q_CALLS)?,
            variables: mk(Q_VARIABLES)?,
        })
    }
}

pub struct TypeScriptParser {
    ts_language: TsLanguage,
    queries: TsQueries,
}

impl TypeScriptParser {
    pub fn new() -> Self {
        let ts_language: TsLanguage = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
        let queries = TsQueries::new(&ts_language).expect("TS queries must compile");
        Self { ts_language, queries }
    }

    fn make_parser(&self) -> Parser {
        let mut parser = Parser::new();
        parser.set_language(&self.ts_language).expect("TS language must load");
        parser
    }

    fn find_functions(&self, source: &[u8], root: &Node, path: &Path, cursor: &mut QueryCursor) -> Vec<FunctionData> {
        let mut functions = Vec::new();
        let name_idx = self.queries.functions.capture_index_for_name("name").unwrap();
        let fn_idx = self.queries.functions.capture_index_for_name("function_node").unwrap();

        let mut matches = cursor.matches(&self.queries.functions, *root, source);
        while let Some(m) = { matches.advance(); matches.get() } {
            let mut name_text = "";
            let mut name_node = None;
            let mut fn_node = None;
            for cap in m.captures {
                if cap.index == name_idx {
                    name_text = get_node_text(&cap.node, source);
                    name_node = Some(cap.node);
                }
                if cap.index == fn_idx {
                    fn_node = Some(cap.node);
                }
            }
            let Some(n_node) = name_node else { continue };
            let Some(f_node) = fn_node else { continue };

            let params = f_node.child_by_field_name("parameters");
            let args = params.map(|p| extract_formal_params(&p, source)).unwrap_or_default();
            let complexity = calculate_cyclomatic_complexity(&f_node, TS_COMPLEXITY_KINDS);
            let ctx = get_parent_context(&f_node, source, &["function_declaration", "class_declaration", "method_definition"]);
            let class_ctx = get_parent_context(&f_node, source, &["class_declaration", "abstract_class_declaration"]);

            let arg_types = vec![None; args.len()];
            let return_type = None;

            // Check for visibility, static, abstract modifiers on method nodes
            let visibility = if f_node.kind() == "method_definition" {
                f_node.parent().and_then(|p| {
                    let mut c = p.walk();
                    if !c.goto_first_child() { return None; }
                    loop {
                        let n = c.node();
                        if n.kind() == "accessibility_modifier" {
                            return Some(get_node_text(&n, source).to_string());
                        }
                        if !c.goto_next_sibling() { break; }
                    }
                    None
                })
            } else {
                None
            };

            let is_static = if f_node.kind() == "method_definition" {
                f_node.parent().map_or(false, |p| {
                    let mut c = p.walk();
                    if !c.goto_first_child() { return false; }
                    loop {
                        let n = c.node();
                        if get_node_text(&n, source) == "static" { return true; }
                        if !c.goto_next_sibling() { break; }
                    }
                    false
                })
            } else {
                false
            };

            let is_abstract = if f_node.kind() == "method_definition" {
                f_node.parent().map_or(false, |p| {
                    let mut c = p.walk();
                    if !c.goto_first_child() { return false; }
                    loop {
                        let n = c.node();
                        if get_node_text(&n, source) == "abstract" { return true; }
                        if !c.goto_next_sibling() { break; }
                    }
                    false
                })
            } else {
                false
            };

            functions.push(FunctionData {
                name: name_text.to_string(),
                path: path.to_path_buf(),
                span: SourceSpan {
                    start_line: n_node.start_position().row as u32 + 1,
                    end_line: f_node.end_position().row as u32 + 1,
                    start_col: n_node.start_position().column as u32,
                    end_col: f_node.end_position().column as u32,
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
                language: Language::TypeScript,
                is_dependency: false,
                source: None,
                docstring: None,
                is_async: f_node.kind().contains("async") || {
                    let prev = f_node.prev_sibling();
                    prev.map_or(false, |n| get_node_text(&n, source) == "async")
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
        let name_idx = self.queries.classes.capture_index_for_name("name").unwrap();
        let class_idx = self.queries.classes.capture_index_for_name("class_node").unwrap();

        let mut matches = cursor.matches(&self.queries.classes, *root, source);
        while let Some(m) = { matches.advance(); matches.get() } {
            let mut name_text = "";
            let mut class_node = None;
            for cap in m.captures {
                if cap.index == name_idx { name_text = get_node_text(&cap.node, source); }
                if cap.index == class_idx { class_node = Some(cap.node); }
            }
            let Some(c_node) = class_node else { continue };

            let bases = extract_ts_heritage(&c_node, source);

            classes.push(ClassData {
                name: name_text.to_string(),
                path: path.to_path_buf(),
                span: SourceSpan {
                    start_line: c_node.start_position().row as u32 + 1,
                    end_line: c_node.end_position().row as u32 + 1,
                    start_col: c_node.start_position().column as u32,
                    end_col: c_node.end_position().column as u32,
                },
                bases,
                fields: Vec::new(),
                decorators: Vec::new(),
                context: None,
                language: Language::TypeScript,
                is_dependency: false,
                source: None,
                docstring: None,
            });
        }
        classes
    }

    fn find_interfaces(&self, source: &[u8], root: &Node, path: &Path, cursor: &mut QueryCursor) -> Vec<InterfaceData> {
        let mut interfaces = Vec::new();
        let name_idx = self.queries.interfaces.capture_index_for_name("name").unwrap();
        let iface_idx = self.queries.interfaces.capture_index_for_name("interface_node").unwrap();

        let mut matches = cursor.matches(&self.queries.interfaces, *root, source);
        while let Some(m) = { matches.advance(); matches.get() } {
            let mut name_text = "";
            let mut iface_node = None;
            for cap in m.captures {
                if cap.index == name_idx { name_text = get_node_text(&cap.node, source); }
                if cap.index == iface_idx { iface_node = Some(cap.node); }
            }
            let Some(i_node) = iface_node else { continue };

            interfaces.push(InterfaceData {
                name: name_text.to_string(),
                path: path.to_path_buf(),
                span: SourceSpan {
                    start_line: i_node.start_position().row as u32 + 1,
                    end_line: i_node.end_position().row as u32 + 1,
                    start_col: i_node.start_position().column as u32,
                    end_col: i_node.end_position().column as u32,
                },
                bases: Vec::new(),
                language: Language::TypeScript,
                is_dependency: false,
                source: None,
            });
        }
        interfaces
    }

    fn find_imports(&self, source: &[u8], root: &Node, cursor: &mut QueryCursor) -> Vec<ImportData> {
        let mut imports = Vec::new();
        let import_idx = self.queries.imports.capture_index_for_name("import").unwrap();

        let mut matches = cursor.matches(&self.queries.imports, *root, source);
        while let Some(m) = { matches.advance(); matches.get() } {
            for cap in m.captures {
                if cap.index != import_idx { continue; }
                let node = cap.node;
                let source_node = node.child_by_field_name("source");
                let source_text = source_node.map(|s| {
                    let t = get_node_text(&s, source);
                    t.trim_matches(|c| c == '\'' || c == '"').to_string()
                });

                if let Some(src) = source_text {
                    imports.push(ImportData {
                        name: src.clone(),
                        full_import_name: Some(src),
                        line_number: node.start_position().row as u32 + 1,
                        alias: None,
                        language: Language::TypeScript,
                        is_dependency: false,
                    });
                }
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
                if cap.index != name_idx { continue; }
                let node = cap.node;
                let call_node = {
                    let p = node.parent().unwrap();
                    if p.kind() == "call_expression" || p.kind() == "new_expression" {
                        p
                    } else {
                        p.parent().unwrap_or(p)
                    }
                };
                let func_node = call_node.child_by_field_name("function")
                    .or_else(|| call_node.child_by_field_name("constructor"));
                let full_name = func_node
                    .map(|f| get_node_text(&f, source).to_string())
                    .unwrap_or_else(|| get_node_text(&node, source).to_string());

                let ctx = get_parent_context(&node, source, &["function_declaration", "method_definition", "class_declaration"]);

                calls.push(FunctionCallData {
                    name: get_node_text(&node, source).to_string(),
                    full_name,
                    line_number: node.start_position().row as u32 + 1,
                    args: Vec::new(),
                    inferred_obj_type: None,
                    context: ctx,
                    language: Language::TypeScript,
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
                if cap.index != name_idx { continue; }
                let node = cap.node;
                let declarator = node.parent().unwrap();
                // Skip function/arrow assignments
                let value = declarator.child_by_field_name("value");
                if let Some(v) = &value {
                    let k = v.kind();
                    if k == "function_expression" || k == "arrow_function" || k == "function" {
                        continue;
                    }
                }
                let ctx = get_parent_context(&node, source, &["function_declaration", "method_definition", "class_declaration"]);

                variables.push(VariableData {
                    name: get_node_text(&node, source).to_string(),
                    path: path.to_path_buf(),
                    line_number: node.start_position().row as u32 + 1,
                    value: value.map(|v| get_node_text(&v, source).to_string()),
                    type_annotation: None,
                    context: ctx.map(|(n, _, _)| n),
                    class_context: None,
                    language: Language::TypeScript,
                    is_dependency: false,
                });
            }
        }
        variables
    }
}

impl LanguageParser for TypeScriptParser {
    fn language(&self) -> Language { Language::TypeScript }

    fn parse(&self, path: &Path, source: &[u8], is_dependency: bool) -> Result<FileParseResult> {
        let mut parser = self.make_parser();
        let tree = parser.parse(source, None).ok_or_else(|| Error::Parse {
            path: path.to_path_buf(), message: "tree-sitter failed to parse".into(),
        })?;
        let root = tree.root_node();

        let mut cursor = QueryCursor::new();
        let mut result = FileParseResult::new(path.to_path_buf(), Language::TypeScript, is_dependency);
        result.functions = self.find_functions(source, &root, path, &mut cursor);
        result.classes = self.find_classes(source, &root, path, &mut cursor);
        result.interfaces = self.find_interfaces(source, &root, path, &mut cursor);
        result.imports = self.find_imports(source, &root, &mut cursor);
        result.function_calls = self.find_calls(source, &root, &mut cursor);
        result.variables = self.find_variables(source, &root, path, &mut cursor);
        result.total_lines = source.split(|&b| b == b'\n').count();
        result.comment_line_count = 0; // TODO: count comment nodes
        result.is_test_file = path.to_string_lossy().contains("test");
        Ok(result)
    }
}

fn extract_formal_params(params: &Node, source: &[u8]) -> Vec<String> {
    let mut args = Vec::new();
    let mut cursor = params.walk();
    if !cursor.goto_first_child() { return args; }
    loop {
        let child = cursor.node();
        match child.kind() {
            "identifier" => args.push(get_node_text(&child, source).to_string()),
            "required_parameter" | "optional_parameter" => {
                if let Some(n) = child.child_by_field_name("pattern")
                    .or_else(|| child.child(0).filter(|c| c.kind() == "identifier"))
                {
                    args.push(get_node_text(&n, source).to_string());
                }
            }
            "rest_pattern" => args.push(get_node_text(&child, source).to_string()),
            "assignment_pattern" => {
                if let Some(left) = child.child_by_field_name("left") {
                    args.push(get_node_text(&left, source).to_string());
                }
            }
            _ => {}
        }
        if !cursor.goto_next_sibling() { break; }
    }
    args
}

fn extract_ts_heritage(class_node: &Node, source: &[u8]) -> Vec<String> {
    let mut bases = Vec::new();
    let mut cursor = class_node.walk();
    if !cursor.goto_first_child() { return bases; }
    loop {
        let child = cursor.node();
        if child.kind() == "class_heritage" {
            let mut inner = child.walk();
            if inner.goto_first_child() {
                loop {
                    let ic = inner.node();
                    if ic.kind() == "extends_clause" || ic.kind() == "implements_clause" {
                        let mut deep = ic.walk();
                        if deep.goto_first_child() {
                            loop {
                                let d = deep.node();
                                if d.kind() == "type_identifier" || d.kind() == "identifier" || d.kind() == "generic_type" {
                                    let text = get_node_text(&d, source).to_string();
                                    if !text.is_empty() { bases.push(text); }
                                }
                                if !deep.goto_next_sibling() { break; }
                            }
                        }
                    }
                    if !inner.goto_next_sibling() { break; }
                }
            }
        }
        if !cursor.goto_next_sibling() { break; }
    }
    bases
}

pub fn pre_scan_typescript(files: &[std::path::PathBuf]) -> std::collections::HashMap<String, Vec<String>> {
    let mut map: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    let ts_lang: TsLanguage = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
    let query_str = r#"
        (class_declaration name: (type_identifier) @name)
        (function_declaration name: (identifier) @name)
        (interface_declaration name: (type_identifier) @name)
    "#;
    let query = match Query::new(&ts_lang, query_str) { Ok(q) => q, Err(_) => return map };
    let mut parser = Parser::new();
    parser.set_language(&ts_lang).ok();

    for file_path in files {
        let source = match std::fs::read(file_path) { Ok(s) => s, Err(_) => continue };
        let tree = match parser.parse(&source, None) { Some(t) => t, None => continue };
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source.as_slice());
        while let Some(m) = { matches.advance(); matches.get() } {
            for cap in m.captures {
                let name = get_node_text(&cap.node, &source).to_string();
                map.entry(name).or_default().push(file_path.to_string_lossy().to_string());
            }
        }
    }
    map
}
