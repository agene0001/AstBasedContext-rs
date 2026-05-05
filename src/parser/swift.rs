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
        name: (simple_identifier) @name) @function_node
    (init_declaration) @function_node
"#;

const Q_CLASSES: &str = r#"
    (class_declaration
        name: (type_identifier) @name) @class_node
"#;

const Q_STRUCTS: &str = r#"
    (struct_declaration
        name: (type_identifier) @name) @struct_node
"#;

const Q_PROTOCOLS: &str = r#"
    (protocol_declaration
        name: (type_identifier) @name) @protocol_node
"#;

const Q_ENUMS: &str = r#"
    (enum_declaration
        name: (type_identifier) @name) @enum_node
"#;

const Q_IMPORTS: &str = r#"
    (import_declaration
        path: (identifier) @path) @import
"#;

const Q_CALLS: &str = r#"
    (call_expression
        function: (simple_identifier) @name)
    (call_expression
        function: (navigation_expression
            suffix: (navigation_suffix
                suffix: (simple_identifier) @name)))
"#;

/// Complexity-contributing node types for Swift.
const SWIFT_COMPLEXITY_KINDS: &[&str] = &[
    "if_statement",
    "guard_statement",
    "for_statement",
    "while_statement",
    "repeat_while_statement",
    "switch_statement",
    "switch_entry",
    "catch_clause",
    "conditional_binding_pattern",
];

struct SwiftQueries {
    functions: Query,
    classes: Query,
    structs: Query,
    protocols: Query,
    enums: Query,
    imports: Query,
    calls: Query,
}

impl SwiftQueries {
    fn new(ts_lang: &TsLanguage) -> Result<Self> {
        let mk = |src: &str| Query::new(ts_lang, src).map_err(|e| Error::Query(format!("{e}")));
        Ok(Self {
            functions: mk(Q_FUNCTIONS)?,
            classes: mk(Q_CLASSES)?,
            structs: mk(Q_STRUCTS)?,
            protocols: mk(Q_PROTOCOLS)?,
            enums: mk(Q_ENUMS)?,
            imports: mk(Q_IMPORTS)?,
            calls: mk(Q_CALLS)?,
        })
    }
}

pub struct SwiftParser {
    ts_language: TsLanguage,
    queries: SwiftQueries,
}

impl Default for SwiftParser {
    fn default() -> Self {
        Self::new()
    }
}

impl SwiftParser {
    pub fn new() -> Self {
        let ts_language: TsLanguage = tree_sitter_swift::LANGUAGE.into();
        let queries =
            SwiftQueries::new(&ts_language).expect("built-in Swift queries must compile");
        Self {
            ts_language,
            queries,
        }
    }

    fn make_parser(&self) -> Parser {
        let mut parser = Parser::new();
        parser
            .set_language(&self.ts_language)
            .expect("Swift language must load");
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
        let name_idx = self.queries.functions.capture_index_for_name("name");
        let func_idx = self
            .queries
            .functions
            .capture_index_for_name("function_node");

        let mut matches = cursor.matches(&self.queries.functions, *root, source);
        while let Some(m) = {
            matches.advance();
            matches.get()
        } {
            // Find the function_node capture (or init_declaration which has no name capture)
            let func_node = if let Some(fi) = func_idx {
                m.captures.iter().find(|c| c.index == fi).map(|c| c.node)
            } else {
                None
            };

            // Name from the name capture, or "init" for init_declaration
            let (name, func_node) = if let Some(ni) = name_idx {
                if let Some(cap) = m.captures.iter().find(|c| c.index == ni) {
                    let n = get_node_text(&cap.node, source).to_string();
                    let fn_node = func_node.unwrap_or_else(|| cap.node.parent().unwrap_or(cap.node));
                    (n, fn_node)
                } else {
                    // init_declaration with no name capture
                    if let Some(fn_n) = func_node {
                        ("init".to_string(), fn_n)
                    } else {
                        continue;
                    }
                }
            } else {
                continue;
            };

            let complexity = calculate_cyclomatic_complexity(&func_node, SWIFT_COMPLEXITY_KINDS);
            let ctx = get_parent_context(
                &func_node,
                source,
                &[
                    "class_declaration",
                    "struct_declaration",
                    "extension_declaration",
                ],
            );

            // Determine visibility from modifiers
            let visibility = extract_swift_visibility(&func_node, source);

            // is_async: look for async modifier
            let is_async = has_child_kind(&func_node, "async");

            functions.push(FunctionData {
                name,
                path: path.to_path_buf(),
                span: SourceSpan {
                    start_line: func_node.start_position().row as u32 + 1,
                    end_line: func_node.end_position().row as u32 + 1,
                    start_col: func_node.start_position().column as u32,
                    end_col: func_node.end_position().column as u32,
                },
                args: extract_swift_params(&func_node, source),
                arg_types: Vec::new(),
                return_type: func_node
                    .child_by_field_name("return_type")
                    .map(|r| get_node_text(&r, source).to_string()),
                visibility,
                is_static: has_modifier(&func_node, source, "static"),
                is_abstract: false,
                cyclomatic_complexity: complexity,
                decorators: Vec::new(),
                context: ctx.as_ref().map(|(n, _, _)| n.clone()),
                context_type: ctx.as_ref().map(|(_, t, _)| t.clone()),
                class_context: None,
                language: Language::Swift,
                is_dependency: false,
                source: None,
                docstring: None,
                is_async,
                todo_comments: vec![],
                raises: vec![],
                has_error_handling: false,
            });
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

                // Extract superclass and protocol conformances
                let bases = extract_swift_bases(&class_node, source);

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
                    language: Language::Swift,
                    is_dependency: false,
                    source: None,
                    docstring: None,
                });
            }
        }
        classes
    }

    fn find_structs(
        &self,
        source: &[u8],
        root: &Node,
        path: &Path,
        cursor: &mut QueryCursor,
    ) -> Vec<StructData> {
        let mut structs = Vec::new();
        let Some(name_idx) = self.queries.structs.capture_index_for_name("name") else {
            return structs;
        };

        let mut matches = cursor.matches(&self.queries.structs, *root, source);
        while let Some(m) = {
            matches.advance();
            matches.get()
        } {
            for cap in m.captures {
                if cap.index != name_idx {
                    continue;
                }
                let node = cap.node;
                let struct_node = node.parent().unwrap_or(node);
                let name = get_node_text(&node, source).to_string();

                structs.push(StructData {
                    name,
                    path: path.to_path_buf(),
                    span: SourceSpan {
                        start_line: struct_node.start_position().row as u32 + 1,
                        end_line: struct_node.end_position().row as u32 + 1,
                        start_col: struct_node.start_position().column as u32,
                        end_col: struct_node.end_position().column as u32,
                    },
                    fields: Vec::new(),
                    language: Language::Swift,
                    is_dependency: false,
                    source: None,
                });
            }
        }
        structs
    }

    fn find_protocols(
        &self,
        source: &[u8],
        root: &Node,
        path: &Path,
        cursor: &mut QueryCursor,
    ) -> Vec<InterfaceData> {
        let mut interfaces = Vec::new();
        let Some(name_idx) = self.queries.protocols.capture_index_for_name("name") else {
            return interfaces;
        };

        let mut matches = cursor.matches(&self.queries.protocols, *root, source);
        while let Some(m) = {
            matches.advance();
            matches.get()
        } {
            for cap in m.captures {
                if cap.index != name_idx {
                    continue;
                }
                let node = cap.node;
                let proto_node = node.parent().unwrap_or(node);
                let name = get_node_text(&node, source).to_string();

                let bases = extract_swift_bases(&proto_node, source);

                interfaces.push(InterfaceData {
                    name,
                    path: path.to_path_buf(),
                    span: SourceSpan {
                        start_line: proto_node.start_position().row as u32 + 1,
                        end_line: proto_node.end_position().row as u32 + 1,
                        start_col: proto_node.start_position().column as u32,
                        end_col: proto_node.end_position().column as u32,
                    },
                    bases,
                    language: Language::Swift,
                    is_dependency: false,
                    source: None,
                });
            }
        }
        interfaces
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
                    language: Language::Swift,
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
                let import_path = get_node_text(&node, source).to_string();

                if seen.contains(&import_path) {
                    continue;
                }
                seen.insert(import_path.clone());

                imports.push(ImportData {
                    name: import_path.clone(),
                    full_import_name: Some(import_path),
                    line_number: node.start_position().row as u32 + 1,
                    alias: None,
                    language: Language::Swift,
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
                let call_node = {
                    let Some(mut p) = node.parent() else { continue };
                    while p.kind() != "call_expression" {
                        p = match p.parent() {
                            Some(pp) => pp,
                            None => break,
                        };
                    }
                    p
                };
                let ctx = get_parent_context(
                    &node,
                    source,
                    &["function_declaration", "init_declaration"],
                );

                calls.push(FunctionCallData {
                    name: get_node_text(&node, source).to_string(),
                    full_name: get_node_text(&call_node, source)
                        .chars()
                        .take(80)
                        .collect(),
                    line_number: node.start_position().row as u32 + 1,
                    args: Vec::new(),
                    inferred_obj_type: None,
                    context: ctx,
                    language: Language::Swift,
                });
            }
        }
        calls
    }
}

impl LanguageParser for SwiftParser {
    fn language(&self) -> Language {
        Language::Swift
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
        let structs = self.find_structs(source, &root, path, &mut cursor);
        let interfaces = self.find_protocols(source, &root, path, &mut cursor);
        let enums = self.find_enums(source, &root, path, &mut cursor);
        let imports = self.find_imports(source, &root, &mut cursor);
        let function_calls = self.find_calls(source, &root, &mut cursor);

        let mut result =
            FileParseResult::new(path.to_path_buf(), Language::Swift, is_dependency);
        result.functions = functions;
        result.classes = classes;
        result.structs = structs;
        result.interfaces = interfaces;
        result.enums = enums;
        result.imports = imports;
        result.function_calls = function_calls;
        result.total_lines = source.split(|&b| b == b'\n').count();
        result.is_test_file = path.to_string_lossy().contains("Test")
            || path.to_string_lossy().contains("Spec");
        Ok(result)
    }
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn extract_swift_params(func_node: &Node, source: &[u8]) -> Vec<String> {
    let Some(params) = func_node.child_by_field_name("params") else {
        return Vec::new();
    };
    let mut args = Vec::new();
    let mut cursor = params.walk();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            if child.kind() == "parameter" {
                args.push(get_node_text(&child, source).to_string());
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    args
}

fn extract_swift_bases(decl_node: &Node, source: &[u8]) -> Vec<String> {
    let mut bases = Vec::new();
    let Some(inheritance) = decl_node.child_by_field_name("type_inheritance_clause") else {
        return bases;
    };
    let mut cursor = inheritance.walk();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            if child.kind() == "type_identifier" || child.kind() == "user_type" {
                bases.push(get_node_text(&child, source).to_string());
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    bases
}

fn extract_swift_visibility(func_node: &Node, source: &[u8]) -> Option<String> {
    let mut cursor = func_node.walk();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            if child.kind() == "modifier" || child.kind() == "visibility_modifier" {
                let text = get_node_text(&child, source);
                match text {
                    "public" | "open" => return Some("public".to_string()),
                    "private" | "fileprivate" => return Some("private".to_string()),
                    "internal" => return Some("internal".to_string()),
                    _ => {}
                }
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    Some("internal".to_string()) // Swift default
}

fn has_child_kind(node: &Node, kind: &str) -> bool {
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            if cursor.node().kind() == kind {
                return true;
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    false
}

fn has_modifier(node: &Node, source: &[u8], modifier: &str) -> bool {
    let mut cursor = node.walk();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            if (child.kind() == "modifier" || child.kind() == "member_modifier")
                && get_node_text(&child, source) == modifier
            {
                return true;
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    false
}
