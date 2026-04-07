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
    (method name: (identifier) @name) @function_node
    (singleton_method name: (identifier) @name) @function_node
"#;

const Q_CLASSES: &str = r#"
    (class name: (constant) @name) @class_node
    (module name: (constant) @name) @class_node
"#;

const Q_IMPORTS: &str = r#"
    (call method: (identifier) @method
         (#match? @method "^(require|require_relative|include|extend|prepend)$")
         arguments: (argument_list) @import)
"#;

const Q_CALLS: &str = r#"
    (call method: (identifier) @name) @call_node
"#;

const Q_VARIABLES: &str = r#"
    (assignment left: (identifier) @name)
    (assignment left: (instance_variable) @name)
"#;

const RUBY_COMPLEXITY_KINDS: &[&str] = &[
    "if",
    "unless",
    "while",
    "until",
    "for",
    "case",
    "rescue",
    "elsif",
    "when",
];

struct RubyQueries {
    functions: Query,
    classes: Query,
    imports: Query,
    calls: Query,
    variables: Query,
}

impl RubyQueries {
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

pub struct RubyParser {
    ts_language: TsLanguage,
    queries: RubyQueries,
}

impl Default for RubyParser {
    fn default() -> Self {
        Self::new()
    }
}

impl RubyParser {
    pub fn new() -> Self {
        let ts_language = tree_sitter_ruby::LANGUAGE.into();
        let queries = RubyQueries::new(&ts_language)
            .expect("built-in Ruby queries must compile");
        Self {
            ts_language,
            queries,
        }
    }

    fn make_parser(&self) -> Parser {
        let mut parser = Parser::new();
        parser
            .set_language(&self.ts_language)
            .expect("Ruby language must load");
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
                let args = extract_ruby_params(params_node.as_ref(), source);

                let complexity = calculate_cyclomatic_complexity(&func_node, RUBY_COMPLEXITY_KINDS);

                let ctx = get_parent_context(
                    &func_node,
                    source,
                    &["method", "singleton_method", "class", "module"],
                );
                let class_ctx = get_parent_context(
                    &func_node,
                    source,
                    &["class", "module"],
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
                    // Ruby has no explicit type annotations
                    arg_types: Vec::new(),
                    return_type: None,
                    // Ruby has no explicit visibility keywords in the same way
                    visibility: None,
                    is_static: func_node.kind() == "singleton_method",
                    is_abstract: false,
                    cyclomatic_complexity: complexity,
                    decorators: Vec::new(),
                    context: ctx.as_ref().map(|(n, _, _)| n.clone()),
                    context_type: ctx.as_ref().map(|(_, t, _)| t.clone()),
                    class_context: class_ctx.map(|(n, _, _)| n),
                    language: Language::Ruby,
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

                // Check for superclass (Ruby `class Foo < Bar`)
                if let Some(superclass) = class_node.child_by_field_name("superclass") {
                    bases.push(get_node_text(&superclass, source).to_string());
                }

                let ctx = get_parent_context(
                    &class_node,
                    source,
                    &["class", "module"],
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
                    language: Language::Ruby,
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

                // Extract the string argument from the argument_list
                let clean = extract_ruby_require_path(&node, source);
                if clean.is_empty() {
                    continue;
                }

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
                    language: Language::Ruby,
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
                let args = extract_ruby_call_args(&call_node, source);

                let ctx = get_parent_context(
                    &call_node,
                    source,
                    &["method", "singleton_method", "class", "module"],
                );

                calls.push(FunctionCallData {
                    name,
                    full_name,
                    line_number: call_node.start_position().row as u32 + 1,
                    args,
                    inferred_obj_type: None,
                    context: ctx,
                    language: Language::Ruby,
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

                let assignment = node.parent();
                let value = assignment
                    .and_then(|a| a.child_by_field_name("right"))
                    .map(|v| get_node_text(&v, source).to_string());

                let ctx = get_parent_context(
                    &node,
                    source,
                    &["method", "singleton_method", "class", "module"],
                );
                let class_ctx = get_parent_context(
                    &node,
                    source,
                    &["class", "module"],
                );

                variables.push(VariableData {
                    name,
                    path: path.to_path_buf(),
                    line_number: node.start_position().row as u32 + 1,
                    value,
                    // Ruby has no type annotations
                    type_annotation: None,
                    context: ctx.map(|(n, _, _)| n),
                    class_context: class_ctx.map(|(n, _, _)| n),
                    language: Language::Ruby,
                    is_dependency: false,
                });
            }
        }
        variables
    }
}

impl LanguageParser for RubyParser {
    fn language(&self) -> Language {
        Language::Ruby
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

        let mut result = FileParseResult::new(path.to_path_buf(), Language::Ruby, is_dependency);
        result.functions = functions;
        result.classes = classes;
        result.variables = variables;
        result.imports = imports;
        result.function_calls = function_calls;
        result.total_lines = source.split(|&b| b == b'\n').count();
        result.comment_line_count = count_comment_lines(&root);
        let path_str = path.to_string_lossy();
        result.is_test_file = path_str.contains("spec")
            || path_str.contains("_test")
            || path_str.contains("test_");
        Ok(result)
    }
}

// ── free helpers ──────────────────────────────────────────────────────────

fn extract_ruby_params(params_node: Option<&Node>, source: &[u8]) -> Vec<String> {
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
            "identifier" | "optional_parameter" | "splat_parameter"
            | "hash_splat_parameter" | "block_parameter" | "keyword_parameter" => {
                // For compound parameter nodes, try to get the name child; for plain identifier, use directly
                if child.kind() == "identifier" {
                    args.push(get_node_text(&child, source).to_string());
                } else if let Some(name_node) = child.child_by_field_name("name") {
                    args.push(get_node_text(&name_node, source).to_string());
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

/// Extract the string path from a require/require_relative call's argument_list.
fn extract_ruby_require_path(arg_list: &Node, source: &[u8]) -> String {
    let mut cursor = arg_list.walk();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            if child.kind() == "string" {
                // Get the string content node
                let mut inner = child.walk();
                if inner.goto_first_child() {
                    loop {
                        let content = inner.node();
                        if content.kind() == "string_content" {
                            return get_node_text(&content, source).to_string();
                        }
                        if !inner.goto_next_sibling() {
                            break;
                        }
                    }
                }
                // Fallback: strip surrounding quotes from the full string text
                let raw = get_node_text(&child, source);
                return raw.trim_matches(|c| c == '"' || c == '\'').to_string();
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    String::new()
}

fn extract_ruby_call_args(call_node: &Node, source: &[u8]) -> Vec<String> {
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
