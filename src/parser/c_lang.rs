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
    (function_definition declarator: (function_declarator declarator: (identifier) @name)) @function_node
"#;

const Q_STRUCTS: &str = r#"
    (struct_specifier name: (type_identifier) @name) @struct_node
"#;

const Q_ENUMS: &str = r#"
    (enum_specifier name: (type_identifier) @name) @enum_node
"#;

const Q_IMPORTS: &str = r#"
    (preproc_include path: [(string_literal) @path (system_lib_string) @path]) @import
"#;

const Q_CALLS: &str = r#"
    (call_expression function: (identifier) @name)
"#;

const Q_VARIABLES: &str = r#"
    (declaration declarator: (init_declarator declarator: (identifier) @name))
    (declaration declarator: (identifier) @name)
"#;

const Q_MACROS: &str = r#"
    (preproc_def name: (identifier) @name) @macro_node
"#;

const C_COMPLEXITY_KINDS: &[&str] = &[
    "if_statement",
    "for_statement",
    "while_statement",
    "do_statement",
    "switch_statement",
    "case_statement",
    "conditional_expression",
    "goto_statement",
];

struct CQueries {
    functions: Query,
    structs: Query,
    enums: Query,
    imports: Query,
    calls: Query,
    variables: Query,
    macros: Query,
}

impl CQueries {
    fn new(ts_lang: &TsLanguage) -> Result<Self> {
        let mk = |src: &str| {
            Query::new(ts_lang, src).map_err(|e| Error::Query(format!("{e}")))
        };
        Ok(Self {
            functions: mk(Q_FUNCTIONS)?,
            structs: mk(Q_STRUCTS)?,
            enums: mk(Q_ENUMS)?,
            imports: mk(Q_IMPORTS)?,
            calls: mk(Q_CALLS)?,
            variables: mk(Q_VARIABLES)?,
            macros: mk(Q_MACROS)?,
        })
    }
}

pub struct CParser {
    ts_language: TsLanguage,
    queries: CQueries,
}

impl Default for CParser {
    fn default() -> Self {
        Self::new()
    }
}

impl CParser {
    pub fn new() -> Self {
        let ts_language = tree_sitter_c::LANGUAGE.into();
        let queries = CQueries::new(&ts_language)
            .expect("built-in C queries must compile");
        Self {
            ts_language,
            queries,
        }
    }

    fn make_parser(&self) -> Parser {
        let mut parser = Parser::new();
        parser
            .set_language(&self.ts_language)
            .expect("C language must load");
        parser
    }

    fn find_functions(&self, source: &[u8], root: &Node, path: &Path, cursor: &mut QueryCursor) -> Vec<FunctionData> {
        let mut functions = Vec::new();
        let Some(name_idx) = self.queries.functions.capture_index_for_name("name") else { return functions; };
        let Some(func_node_idx) = self.queries.functions.capture_index_for_name("function_node") else { return functions; };

        let mut matches = cursor.matches(&self.queries.functions, *root, source);
        while let Some(m) = { matches.advance(); matches.get() } {
            let mut name_text = None;
            let mut name_node_opt = None;
            let mut func_node_opt = None;

            for cap in m.captures {
                if cap.index == name_idx {
                    name_text = Some(get_node_text(&cap.node, source).to_string());
                    name_node_opt = Some(cap.node);
                }
                if cap.index == func_node_idx {
                    func_node_opt = Some(cap.node);
                }
            }

            if let (Some(name), Some(name_node), Some(func_node)) = (name_text, name_node_opt, func_node_opt) {
                let params = extract_c_params(&func_node, source);
                let complexity = calculate_cyclomatic_complexity(&func_node, C_COMPLEXITY_KINDS);

                let ctx = get_parent_context(
                    &func_node,
                    source,
                    &["function_definition"],
                );

                functions.push(FunctionData {
                    name,
                    path: path.to_path_buf(),
                    span: SourceSpan {
                        start_line: name_node.start_position().row as u32 + 1,
                        end_line: func_node.end_position().row as u32 + 1,
                        start_col: name_node.start_position().column as u32,
                        end_col: func_node.end_position().column as u32,
                    },
                    arg_types: vec![None; params.len()],
                    args: params,
                    return_type: None,
                    visibility: None,
                    is_static: false,
                    is_abstract: false,
                    cyclomatic_complexity: complexity,
                    decorators: Vec::new(),
                    context: ctx.as_ref().map(|(n, _, _)| n.clone()),
                    context_type: ctx.as_ref().map(|(_, t, _)| t.clone()),
                    class_context: None,
                    language: Language::C,
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
        let mut seen = HashSet::new();
        let Some(name_idx) = self.queries.structs.capture_index_for_name("name") else { return structs; };
        let Some(struct_node_idx) = self.queries.structs.capture_index_for_name("struct_node") else { return structs; };

        let mut matches = cursor.matches(&self.queries.structs, *root, source);
        while let Some(m) = { matches.advance(); matches.get() } {
            let mut name_text = None;
            let mut name_node_opt = None;
            let mut struct_node_opt = None;

            for cap in m.captures {
                if cap.index == name_idx {
                    name_text = Some(get_node_text(&cap.node, source).to_string());
                    name_node_opt = Some(cap.node);
                }
                if cap.index == struct_node_idx {
                    struct_node_opt = Some(cap.node);
                }
            }

            if let (Some(name), Some(name_node), Some(struct_node)) = (name_text, name_node_opt, struct_node_opt) {
                // Only include struct definitions (those with a body), deduplicate by name
                if struct_node.child_by_field_name("body").is_some() && seen.insert(name.clone()) {
                    structs.push(StructData {
                        name,
                        path: path.to_path_buf(),
                        span: SourceSpan {
                            start_line: name_node.start_position().row as u32 + 1,
                            end_line: struct_node.end_position().row as u32 + 1,
                            start_col: name_node.start_position().column as u32,
                            end_col: struct_node.end_position().column as u32,
                        },
                        fields: Vec::new(),
                        language: Language::C,
                        is_dependency: false,
                        source: None,
                    });
                }
            }
        }
        structs
    }

    fn find_enums(&self, source: &[u8], root: &Node, path: &Path, cursor: &mut QueryCursor) -> Vec<EnumData> {
        let mut enums = Vec::new();
        let mut seen = HashSet::new();
        let Some(name_idx) = self.queries.enums.capture_index_for_name("name") else { return enums; };
        let Some(enum_node_idx) = self.queries.enums.capture_index_for_name("enum_node") else { return enums; };

        let mut matches = cursor.matches(&self.queries.enums, *root, source);
        while let Some(m) = { matches.advance(); matches.get() } {
            let mut name_text = None;
            let mut name_node_opt = None;
            let mut enum_node_opt = None;

            for cap in m.captures {
                if cap.index == name_idx {
                    name_text = Some(get_node_text(&cap.node, source).to_string());
                    name_node_opt = Some(cap.node);
                }
                if cap.index == enum_node_idx {
                    enum_node_opt = Some(cap.node);
                }
            }

            if let (Some(name), Some(name_node), Some(enum_node)) = (name_text, name_node_opt, enum_node_opt) {
                if enum_node.child_by_field_name("body").is_some() && seen.insert(name.clone()) {
                    let variants = extract_enum_variants(&enum_node, source);
                    enums.push(EnumData {
                        name,
                        path: path.to_path_buf(),
                        span: SourceSpan {
                            start_line: name_node.start_position().row as u32 + 1,
                            end_line: enum_node.end_position().row as u32 + 1,
                            start_col: name_node.start_position().column as u32,
                            end_col: enum_node.end_position().column as u32,
                        },
                        variants,
                        language: Language::C,
                        is_dependency: false,
                        source: None,
                    });
                }
            }
        }
        enums
    }

    fn find_imports(&self, source: &[u8], root: &Node, cursor: &mut QueryCursor) -> Vec<ImportData> {
        let mut imports = Vec::new();
        let mut seen = HashSet::new();
        let Some(path_idx) = self.queries.imports.capture_index_for_name("path") else { return imports; };

        let mut matches = cursor.matches(&self.queries.imports, *root, source);
        while let Some(m) = { matches.advance(); matches.get() } {
            for cap in m.captures {
                if cap.index != path_idx {
                    continue;
                }
                let node = cap.node;
                let raw = get_node_text(&node, source).to_string();

                // Strip quotes / angle brackets
                let clean = raw
                    .trim_matches(|c| c == '"' || c == '\'' || c == '<' || c == '>')
                    .to_string();

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
                    language: Language::C,
                    is_dependency: false,
                });
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
                let name = get_node_text(&node, source).to_string();

                let Some(call_node) = node.parent() else { continue; };
                let args = extract_c_call_args(&call_node, source);

                let ctx = get_parent_context(
                    &node,
                    source,
                    &["function_definition"],
                );

                calls.push(FunctionCallData {
                    name: name.clone(),
                    full_name: name,
                    line_number: node.start_position().row as u32 + 1,
                    args,
                    inferred_obj_type: None,
                    context: ctx,
                    language: Language::C,
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

                // Try to get the value from init_declarator
                let declarator_parent = node.parent();
                let value = declarator_parent
                    .filter(|p| p.kind() == "init_declarator")
                    .and_then(|p| p.child_by_field_name("value"))
                    .map(|v| get_node_text(&v, source).to_string());

                // Type comes from the declaration node
                let decl_node = declarator_parent
                    .and_then(|p| if p.kind() == "init_declarator" { p.parent() } else { Some(p) });
                let type_annotation = decl_node
                    .and_then(|d| d.child_by_field_name("type"))
                    .map(|t| get_node_text(&t, source).to_string());

                let ctx = get_parent_context(
                    &node,
                    source,
                    &["function_definition"],
                );

                variables.push(VariableData {
                    name,
                    path: path.to_path_buf(),
                    line_number: node.start_position().row as u32 + 1,
                    value,
                    type_annotation,
                    context: ctx.map(|(n, _, _)| n),
                    class_context: None,
                    language: Language::C,
                    is_dependency: false,
                });
            }
        }
        variables
    }

    fn find_macros(&self, source: &[u8], root: &Node, path: &Path, cursor: &mut QueryCursor) -> Vec<MacroData> {
        let mut macros = Vec::new();
        let Some(name_idx) = self.queries.macros.capture_index_for_name("name") else { return macros; };

        let mut matches = cursor.matches(&self.queries.macros, *root, source);
        while let Some(m) = { matches.advance(); matches.get() } {
            for cap in m.captures {
                if cap.index != name_idx {
                    continue;
                }
                let node = cap.node;
                let name = get_node_text(&node, source).to_string();

                macros.push(MacroData {
                    name,
                    path: path.to_path_buf(),
                    line_number: node.start_position().row as u32 + 1,
                    language: Language::C,
                    is_dependency: false,
                    source: None,
                });
            }
        }
        macros
    }
}

impl LanguageParser for CParser {
    fn language(&self) -> Language {
        Language::C
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
        let enums = self.find_enums(source, &root, path, &mut cursor);
        let imports = self.find_imports(source, &root, &mut cursor);
        let function_calls = self.find_calls(source, &root, &mut cursor);
        let variables = self.find_variables(source, &root, path, &mut cursor);
        let macros = self.find_macros(source, &root, path, &mut cursor);

        let mut result = FileParseResult::new(path.to_path_buf(), Language::C, is_dependency);
        result.functions = functions;
        result.structs = structs;
        result.enums = enums;
        result.variables = variables;
        result.imports = imports;
        result.function_calls = function_calls;
        result.macros = macros;
        result.total_lines = source.split(|&b| b == b'\n').count();
        result.comment_line_count = count_comment_lines(&root);
        result.is_test_file = path.to_string_lossy().contains("test");
        Ok(result)
    }
}

// ── free helpers ──────────────────────────────────────────────────────────

fn extract_c_params(func_node: &Node, source: &[u8]) -> Vec<String> {
    // Navigate: function_definition -> declarator (function_declarator) -> parameters
    let declarator = match func_node.child_by_field_name("declarator") {
        Some(d) => d,
        None => return Vec::new(),
    };
    let params_node = match declarator.child_by_field_name("parameters") {
        Some(p) => p,
        None => return Vec::new(),
    };

    let mut args = Vec::new();
    let mut cursor = params_node.walk();
    if !cursor.goto_first_child() {
        return args;
    }
    loop {
        let child = cursor.node();
        if child.kind() == "parameter_declaration" {
            if let Some(decl) = child.child_by_field_name("declarator") {
                // declarator might be a pointer_declarator wrapping an identifier
                let name = get_innermost_identifier(&decl, source);
                if let Some(n) = name {
                    args.push(n);
                }
            }
        }
        if !cursor.goto_next_sibling() {
            break;
        }
    }
    args
}

fn get_innermost_identifier(node: &Node, source: &[u8]) -> Option<String> {
    if node.kind() == "identifier" {
        return Some(get_node_text(node, source).to_string());
    }
    // Recurse into declarator children (pointer_declarator, array_declarator, etc.)
    if let Some(inner) = node.child_by_field_name("declarator") {
        return get_innermost_identifier(&inner, source);
    }
    None
}

fn extract_c_call_args(call_node: &Node, source: &[u8]) -> Vec<String> {
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

fn extract_enum_variants(enum_node: &Node, source: &[u8]) -> Vec<String> {
    let mut variants = Vec::new();
    let Some(body) = enum_node.child_by_field_name("body") else {
        return variants;
    };
    let mut cursor = body.walk();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            if child.kind() == "enumerator" {
                if let Some(name_node) = child.child_by_field_name("name") {
                    variants.push(get_node_text(&name_node, source).to_string());
                }
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    variants
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
