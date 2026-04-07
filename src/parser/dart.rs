use std::collections::HashSet;
use std::path::Path;

use streaming_iterator::StreamingIterator;
use tree_sitter::{Language as TsLanguage, Node, Parser, Query, QueryCursor};
use tree_sitter_language::LanguageFn;

use crate::error::{Error, Result};
use crate::types::node::*;
use crate::types::{FileParseResult, Language};

use super::common::*;
use super::LanguageParser;

// ── Vendored Dart grammar binding ─────────────────────────────────────────
// The tree-sitter-dart crate on crates.io targets tree-sitter <0.26.
// We vendor parser.c + scanner.c in grammars/dart/ and compile them via build.rs.

extern "C" {
    fn tree_sitter_dart() -> *const ();
}

/// LANGUAGE constant for the vendored Dart grammar.
pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_dart) };

// ── Tree-sitter query strings ─────────────────────────────────────────────

const Q_FUNCTIONS: &str = r#"
    (function_signature
        name: (identifier) @name) @function_node
    (method_signature
        name: (identifier) @name) @function_node
"#;

const Q_CLASSES: &str = r#"
    (class_definition
        name: (identifier) @name) @class_node
"#;

const Q_MIXINS: &str = r#"
    (mixin_declaration
        name: (identifier) @name) @mixin_node
"#;

const Q_ENUMS: &str = r#"
    (enum_declaration
        name: (identifier) @name) @enum_node
"#;

const Q_IMPORTS: &str = r#"
    (import_or_export
        (configured_uri
            (uri) @path)) @import
    (import_or_export
        (uri) @path) @import
"#;

const Q_CALLS: &str = r#"
    (argument_part
        (arguments
            (argument
                (expression_without_cascade
                    (assignable_expression
                        (primary
                            (identifier) @name))))))
    (postfix_expression
        (primary
            (identifier) @name)
        (selector
            (argument_part)))
"#;

/// Complexity-contributing node types for Dart.
const DART_COMPLEXITY_KINDS: &[&str] = &[
    "if_statement",
    "for_statement",
    "while_statement",
    "do_statement",
    "switch_statement",
    "switch_case",
    "catch_clause",
    "conditional_expression",
    "binary_expression",
];

struct DartQueries {
    functions: Query,
    classes: Query,
    mixins: Query,
    enums: Query,
    imports: Query,
    calls: Query,
}

impl DartQueries {
    fn new(ts_lang: &TsLanguage) -> Result<Self> {
        let mk = |src: &str| Query::new(ts_lang, src).map_err(|e| Error::Query(format!("{e}")));
        Ok(Self {
            functions: mk(Q_FUNCTIONS)?,
            classes: mk(Q_CLASSES)?,
            mixins: mk(Q_MIXINS)?,
            enums: mk(Q_ENUMS)?,
            imports: mk(Q_IMPORTS)?,
            calls: mk(Q_CALLS)?,
        })
    }
}

pub struct DartParser {
    ts_language: TsLanguage,
    queries: DartQueries,
}

impl Default for DartParser {
    fn default() -> Self {
        Self::new()
    }
}

impl DartParser {
    pub fn new() -> Self {
        let ts_language: TsLanguage = LANGUAGE.into();
        let queries =
            DartQueries::new(&ts_language).expect("built-in Dart queries must compile");
        Self {
            ts_language,
            queries,
        }
    }

    fn make_parser(&self) -> Parser {
        let mut parser = Parser::new();
        parser
            .set_language(&self.ts_language)
            .expect("Dart language must load");
        parser
    }

    fn find_functions(
        &self,
        source: &[u8],
        root: &Node,
        path: &Path,
        cursor: &mut QueryCursor,
    ) -> Vec<FunctionData> {
        let mut functions = Vec::new();
        let Some(name_idx) = self.queries.functions.capture_index_for_name("name") else {
            return functions;
        };

        let mut matches = cursor.matches(&self.queries.functions, *root, source);
        while let Some(m) = {
            matches.advance();
            matches.get()
        } {
            for cap in m.captures {
                if cap.index != name_idx {
                    continue;
                }
                let node = cap.node;
                let func_node = node.parent().unwrap_or(node);
                let name = get_node_text(&node, source).to_string();

                let complexity =
                    calculate_cyclomatic_complexity(&func_node, DART_COMPLEXITY_KINDS);
                let ctx = get_parent_context(
                    &func_node,
                    source,
                    &["class_definition", "mixin_declaration"],
                );

                // Dart async: look for "async" keyword in function body marker
                let is_async = func_node
                    .child_by_field_name("body")
                    .map(|b| get_node_text(&b, source).starts_with("async"))
                    .unwrap_or(false);

                functions.push(FunctionData {
                    name,
                    path: path.to_path_buf(),
                    span: SourceSpan {
                        start_line: func_node.start_position().row as u32 + 1,
                        end_line: func_node.end_position().row as u32 + 1,
                        start_col: func_node.start_position().column as u32,
                        end_col: func_node.end_position().column as u32,
                    },
                    args: Vec::new(),
                    arg_types: Vec::new(),
                    return_type: func_node
                        .child_by_field_name("return_type")
                        .map(|r| get_node_text(&r, source).to_string()),
                    visibility: None,
                    is_static: false,
                    is_abstract: false,
                    cyclomatic_complexity: complexity,
                    decorators: Vec::new(),
                    context: ctx.as_ref().map(|(n, _, _)| n.clone()),
                    context_type: ctx.as_ref().map(|(_, t, _)| t.clone()),
                    class_context: None,
                    language: Language::Dart,
                    is_dependency: false,
                    source: None,
                    docstring: None,
                    is_async,
                    todo_comments: vec![],
                    raises: vec![],
                    has_error_handling: false,
                });
            }
        }
        functions
    }

    fn find_classes(
        &self,
        source: &[u8],
        root: &Node,
        path: &Path,
        cursor: &mut QueryCursor,
    ) -> Vec<ClassData> {
        let mut classes = Vec::new();
        let Some(name_idx) = self.queries.classes.capture_index_for_name("name") else {
            return classes;
        };

        let mut matches = cursor.matches(&self.queries.classes, *root, source);
        while let Some(m) = {
            matches.advance();
            matches.get()
        } {
            for cap in m.captures {
                if cap.index != name_idx {
                    continue;
                }
                let node = cap.node;
                let class_node = node.parent().unwrap_or(node);
                let name = get_node_text(&node, source).to_string();

                // Extract superclass from "extends" clause
                let bases = extract_dart_bases(&class_node, source);

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
                    decorators: Vec::new(),
                    fields: Vec::new(),
                    context: None,
                    language: Language::Dart,
                    is_dependency: false,
                    source: None,
                    docstring: None,
                });
            }
        }
        classes
    }

    fn find_mixins(
        &self,
        source: &[u8],
        root: &Node,
        path: &Path,
        cursor: &mut QueryCursor,
    ) -> Vec<ClassData> {
        let mut mixins = Vec::new();
        let Some(name_idx) = self.queries.mixins.capture_index_for_name("name") else {
            return mixins;
        };

        let mut matches = cursor.matches(&self.queries.mixins, *root, source);
        while let Some(m) = {
            matches.advance();
            matches.get()
        } {
            for cap in m.captures {
                if cap.index != name_idx {
                    continue;
                }
                let node = cap.node;
                let mixin_node = node.parent().unwrap_or(node);
                let name = get_node_text(&node, source).to_string();

                mixins.push(ClassData {
                    name,
                    path: path.to_path_buf(),
                    span: SourceSpan {
                        start_line: mixin_node.start_position().row as u32 + 1,
                        end_line: mixin_node.end_position().row as u32 + 1,
                        start_col: mixin_node.start_position().column as u32,
                        end_col: mixin_node.end_position().column as u32,
                    },
                    bases: Vec::new(),
                    decorators: Vec::new(),
                    fields: Vec::new(),
                    context: None,
                    language: Language::Dart,
                    is_dependency: false,
                    source: None,
                    docstring: None,
                });
            }
        }
        mixins
    }

    fn find_enums(
        &self,
        source: &[u8],
        root: &Node,
        path: &Path,
        cursor: &mut QueryCursor,
    ) -> Vec<EnumData> {
        let mut enums = Vec::new();
        let Some(name_idx) = self.queries.enums.capture_index_for_name("name") else {
            return enums;
        };

        let mut matches = cursor.matches(&self.queries.enums, *root, source);
        while let Some(m) = {
            matches.advance();
            matches.get()
        } {
            for cap in m.captures {
                if cap.index != name_idx {
                    continue;
                }
                let node = cap.node;
                let enum_node = node.parent().unwrap_or(node);
                let name = get_node_text(&node, source).to_string();

                enums.push(EnumData {
                    name,
                    path: path.to_path_buf(),
                    span: SourceSpan {
                        start_line: enum_node.start_position().row as u32 + 1,
                        end_line: enum_node.end_position().row as u32 + 1,
                        start_col: enum_node.start_position().column as u32,
                        end_col: enum_node.end_position().column as u32,
                    },
                    variants: Vec::new(),
                    language: Language::Dart,
                    is_dependency: false,
                    source: None,
                });
            }
        }
        enums
    }

    fn find_imports(
        &self,
        source: &[u8],
        root: &Node,
        cursor: &mut QueryCursor,
    ) -> Vec<ImportData> {
        let mut imports = Vec::new();
        let mut seen = HashSet::new();
        let Some(path_idx) = self.queries.imports.capture_index_for_name("path") else {
            return imports;
        };

        let mut matches = cursor.matches(&self.queries.imports, *root, source);
        while let Some(m) = {
            matches.advance();
            matches.get()
        } {
            for cap in m.captures {
                if cap.index != path_idx {
                    continue;
                }
                let node = cap.node;
                let raw = get_node_text(&node, source)
                    .trim()
                    .trim_matches('\'')
                    .trim_matches('"')
                    .to_string();

                if seen.contains(&raw) || raw.is_empty() {
                    continue;
                }
                seen.insert(raw.clone());

                let short = raw
                    .rsplit('/')
                    .next()
                    .unwrap_or(&raw)
                    .trim_end_matches(".dart")
                    .to_string();

                imports.push(ImportData {
                    name: short,
                    full_import_name: Some(raw),
                    line_number: node.start_position().row as u32 + 1,
                    alias: None,
                    language: Language::Dart,
                    is_dependency: false,
                });
            }
        }
        imports
    }

    fn find_calls(
        &self,
        source: &[u8],
        root: &Node,
        cursor: &mut QueryCursor,
    ) -> Vec<FunctionCallData> {
        let mut calls = Vec::new();
        let Some(name_idx) = self.queries.calls.capture_index_for_name("name") else {
            return calls;
        };

        let mut matches = cursor.matches(&self.queries.calls, *root, source);
        while let Some(m) = {
            matches.advance();
            matches.get()
        } {
            for cap in m.captures {
                if cap.index != name_idx {
                    continue;
                }
                let node = cap.node;
                let ctx = get_parent_context(
                    &node,
                    source,
                    &["function_signature", "method_signature"],
                );

                calls.push(FunctionCallData {
                    name: get_node_text(&node, source).to_string(),
                    full_name: get_node_text(&node, source).to_string(),
                    line_number: node.start_position().row as u32 + 1,
                    args: Vec::new(),
                    inferred_obj_type: None,
                    context: ctx,
                    language: Language::Dart,
                });
            }
        }
        calls
    }
}

impl LanguageParser for DartParser {
    fn language(&self) -> Language {
        Language::Dart
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
        let mut classes = self.find_classes(source, &root, path, &mut cursor);
        // Treat Dart mixins as classes
        classes.extend(self.find_mixins(source, &root, path, &mut cursor));
        let enums = self.find_enums(source, &root, path, &mut cursor);
        let imports = self.find_imports(source, &root, &mut cursor);
        let function_calls = self.find_calls(source, &root, &mut cursor);

        let mut result =
            FileParseResult::new(path.to_path_buf(), Language::Dart, is_dependency);
        result.functions = functions;
        result.classes = classes;
        result.enums = enums;
        result.imports = imports;
        result.function_calls = function_calls;
        result.total_lines = source.split(|&b| b == b'\n').count();
        result.is_test_file = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.ends_with("_test.dart"))
            .unwrap_or(false);
        Ok(result)
    }
}

// ── helpers ───────────────────────────────────────────────────────────────

fn extract_dart_bases(class_node: &Node, source: &[u8]) -> Vec<String> {
    let mut bases = Vec::new();
    let mut cursor = class_node.walk();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            match child.kind() {
                "superclass" | "interfaces" | "mixins" => {
                    let text = get_node_text(&child, source).to_string();
                    // Strip leading keywords like "extends " / "implements " / "with "
                    let cleaned = text
                        .trim_start_matches("extends")
                        .trim_start_matches("implements")
                        .trim_start_matches("with")
                        .trim()
                        .to_string();
                    if !cleaned.is_empty() {
                        for part in cleaned.split(',') {
                            let p = part.trim().to_string();
                            if !p.is_empty() {
                                bases.push(p);
                            }
                        }
                    }
                }
                _ => {}
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    bases
}
