use std::collections::HashSet;
use std::path::Path;

use streaming_iterator::StreamingIterator;
use tree_sitter::{Language as TsLanguage, Node, Parser, Query, QueryCursor};

use crate::error::{Error, Result};
use crate::types::node::*;
use crate::types::{FileParseResult, Language};

use super::common::*;
use super::LanguageParser;

// Tree-sitter query strings — ported directly from PY_QUERIES in the Python source.
const Q_IMPORTS: &str = r#"
    (import_statement name: (_) @import)
    (import_from_statement) @from_import_stmt
"#;

const Q_CLASSES: &str = r#"
    (class_definition
        name: (identifier) @name
        superclasses: (argument_list)? @superclasses
        body: (block) @body)
"#;

const Q_FUNCTIONS: &str = r#"
    (function_definition
        name: (identifier) @name
        parameters: (parameters) @parameters
        body: (block) @body
        return_type: (_)? @return_type)
"#;

const Q_CALLS: &str = r#"
    (call
        function: (identifier) @name)
    (call
        function: (attribute attribute: (identifier) @name) @full_call)
"#;

const Q_VARIABLES: &str = r#"
    (assignment
        left: (identifier) @name)
"#;

const Q_LAMBDA_ASSIGNMENTS: &str = r#"
    (assignment
        left: (identifier) @name
        right: (lambda) @lambda_node)
"#;

/// Compiled queries, created once per PythonParser instance.
struct PythonQueries {
    imports: Query,
    classes: Query,
    functions: Query,
    calls: Query,
    variables: Query,
    lambda_assignments: Query,
}

impl PythonQueries {
    fn new(ts_lang: &TsLanguage) -> Result<Self> {
        let mk = |src: &str| {
            Query::new(ts_lang, src).map_err(|e| Error::Query(format!("{e}")))
        };
        Ok(Self {
            imports: mk(Q_IMPORTS)?,
            classes: mk(Q_CLASSES)?,
            functions: mk(Q_FUNCTIONS)?,
            calls: mk(Q_CALLS)?,
            variables: mk(Q_VARIABLES)?,
            lambda_assignments: mk(Q_LAMBDA_ASSIGNMENTS)?,
        })
    }
}

pub struct PythonParser {
    ts_language: TsLanguage,
    queries: PythonQueries,
}

impl Default for PythonParser {
    fn default() -> Self {
        Self::new()
    }
}

impl PythonParser {
    pub fn new() -> Self {
        let ts_language = tree_sitter_python::LANGUAGE.into();
        let queries = PythonQueries::new(&ts_language)
            .expect("built-in Python queries must compile");
        Self {
            ts_language,
            queries,
        }
    }

    fn make_parser(&self) -> Parser {
        let mut parser = Parser::new();
        parser
            .set_language(&self.ts_language)
            .expect("Python language must load");
        parser
    }

    // ── extraction helpers ───────────────────────────────────────────────

    fn find_functions(&self, source: &[u8], root: &Node, tree: &tree_sitter::Tree, path: &Path, cursor: &mut QueryCursor) -> Vec<FunctionData> {
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
                let body_node = func_node.child_by_field_name("body");

                let (args, arg_types) = extract_python_params_typed(params_node.as_ref(), source);
                let decorators = extract_decorators(&func_node, source);
                let complexity = calculate_cyclomatic_complexity(&func_node, PYTHON_COMPLEXITY_KINDS);

                // Extract return type annotation
                let return_type = func_node
                    .child_by_field_name("return_type")
                    .map(|t| get_node_text(&t, source).to_string());

                // Detect visibility and static/abstract from decorators
                let is_static = decorators.iter().any(|d| d.contains("staticmethod") || d.contains("classmethod"));
                let is_abstract = decorators.iter().any(|d| d.contains("abstractmethod"));
                let visibility = if name.starts_with("__") && !name.ends_with("__") {
                    Some("private".to_string())
                } else if name.starts_with('_') {
                    Some("protected".to_string())
                } else {
                    Some("public".to_string())
                };

                let ctx = get_parent_context(
                    &func_node,
                    source,
                    &["function_definition", "class_definition"],
                );
                let class_ctx = get_parent_context(&func_node, source, &["class_definition"]);

                let docstring = body_node.as_ref().and_then(|b| extract_docstring(b, source));

                // In tree-sitter-python, `async def` functions have a parent node
                // of kind "decorated_definition" or are wrapped in an expression
                // statement whose preceding sibling is the `async` keyword.
                // The most reliable signal is checking whether the function node's
                // immediate parent is an `async_statement` node, or whether the
                // source snippet starts with "async".
                let is_async = func_node
                    .parent()
                    .map(|p| p.kind() == "async_statement")
                    .unwrap_or(false)
                    || std::str::from_utf8(source)
                        .ok()
                        .and_then(|s| {
                            let start = func_node.start_byte();
                            s.get(start.saturating_sub(10)..start)
                        })
                        .map(|pre| pre.trim_end().ends_with("async"))
                        .unwrap_or(false);
                let todo_comments = body_node.as_ref()
                    .map(|b| collect_todo_comments(b, source))
                    .unwrap_or_default();
                let raises = body_node.as_ref()
                    .map(|b| collect_raises(b, source))
                    .unwrap_or_default();
                let has_error_handling = body_node.as_ref()
                    .map(|b| has_try_statement(b))
                    .unwrap_or(false);

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
                    decorators,
                    context: ctx.as_ref().map(|(n, _, _)| n.clone()),
                    context_type: ctx.as_ref().map(|(_, t, _)| t.clone()),
                    class_context: class_ctx.map(|(n, _, _)| n),
                    language: Language::Python,
                    is_dependency: false,
                    source: None,
                    docstring,
                    is_async,
                    todo_comments,
                    raises,
                    has_error_handling,
                });
            }
        }
        drop(matches);

        // Lambda assignments
        functions.extend(self.find_lambda_assignments(source, root, tree, path, cursor));
        functions
    }

    fn find_lambda_assignments(
        &self,
        source: &[u8],
        root: &Node,
        _tree: &tree_sitter::Tree,
        path: &Path,
        cursor: &mut QueryCursor,
    ) -> Vec<FunctionData> {
        let mut functions = Vec::new();
        let Some(name_idx) = self.queries.lambda_assignments.capture_index_for_name("name") else { return functions; };

        let mut matches = cursor.matches(&self.queries.lambda_assignments, *root, source);
        while let Some(m) = { matches.advance(); matches.get() } {
            for cap in m.captures {
                if cap.index != name_idx {
                    continue;
                }
                let node = cap.node;
                let Some(assignment_node) = node.parent() else { continue; };
                let Some(lambda_node) = assignment_node.child_by_field_name("right") else { continue; };
                let name = get_node_text(&node, source).to_string();

                let params_node: Option<Node> = lambda_node.child_by_field_name("parameters");
                let args: Vec<String> = params_node
                    .map(|p: Node| {
                        let mut a = Vec::new();
                        let mut child_cursor = p.walk();
                        if child_cursor.goto_first_child() {
                            loop {
                                let c = child_cursor.node();
                                if c.kind() == "identifier" {
                                    a.push(get_node_text(&c, source).to_string());
                                }
                                if !child_cursor.goto_next_sibling() {
                                    break;
                                }
                            }
                        }
                        a
                    })
                    .unwrap_or_default();

                let ctx = get_parent_context(
                    &assignment_node,
                    source,
                    &["function_definition", "class_definition"],
                );
                let class_ctx = get_parent_context(&assignment_node, source, &["class_definition"]);

                let arg_types = vec![None; args.len()];
                functions.push(FunctionData {
                    name,
                    path: path.to_path_buf(),
                    span: SourceSpan {
                        start_line: node.start_position().row as u32 + 1,
                        end_line: assignment_node.end_position().row as u32 + 1,
                        start_col: node.start_position().column as u32,
                        end_col: assignment_node.end_position().column as u32,
                    },
                    args,
                    arg_types,
                    return_type: None,
                    visibility: None,
                    is_static: false,
                    is_abstract: false,
                    cyclomatic_complexity: 1,
                    decorators: Vec::new(),
                    context: ctx.as_ref().map(|(n, _, _)| n.clone()),
                    context_type: ctx.as_ref().map(|(_, t, _)| t.clone()),
                    class_context: class_ctx.map(|(n, _, _)| n),
                    language: Language::Python,
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

                let body_node = class_node.child_by_field_name("body");
                let superclasses_node = class_node.child_by_field_name("superclasses");

                let bases = superclasses_node
                    .as_ref()
                    .map(|sc| extract_bases(sc, source))
                    .unwrap_or_default();

                let decorators = extract_decorators(&class_node, source);
                let fields = extract_python_class_fields(body_node.as_ref(), source);
                let ctx = get_parent_context(
                    &class_node,
                    source,
                    &["function_definition", "class_definition"],
                );
                let docstring = body_node.as_ref().and_then(|b| extract_docstring(b, source));

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
                    decorators,
                    fields,
                    context: ctx.map(|(n, _, _)| n),
                    language: Language::Python,
                    is_dependency: false,
                    source: None,
                    docstring,
                });
            }
        }
        classes
    }

    fn find_imports(&self, source: &[u8], root: &Node, cursor: &mut QueryCursor) -> Vec<ImportData> {
        let mut imports = Vec::new();
        let mut seen = HashSet::new();

        let Some(import_idx) = self.queries.imports.capture_index_for_name("import") else { return imports; };
        let Some(from_idx) = self.queries.imports.capture_index_for_name("from_import_stmt") else { return imports; };

        let mut matches = cursor.matches(&self.queries.imports, *root, source);
        while let Some(m) = { matches.advance(); matches.get() } {
            for cap in m.captures {
                let node = cap.node;

                if cap.index == import_idx {
                    // `import foo` or `import foo as bar`
                    let text = get_node_text(&node, source);
                    let (full_name, alias) = if let Some(pos) = text.find(" as ") {
                        (text[..pos].trim().to_string(), Some(text[pos + 4..].trim().to_string()))
                    } else {
                        (text.trim().to_string(), None)
                    };

                    if seen.contains(&full_name) {
                        continue;
                    }
                    seen.insert(full_name.clone());

                    imports.push(ImportData {
                        name: full_name.clone(),
                        full_import_name: Some(full_name),
                        line_number: node.start_position().row as u32 + 1,
                        alias,
                        language: Language::Python,
                        is_dependency: false,
                    });
                } else if cap.index == from_idx {
                    // `from module import name, name2 as alias`
                    let module_name_node: Node = match node.child_by_field_name("module_name") {
                        Some(n) => n,
                        None => continue,
                    };
                    let module_name = get_node_text(&module_name_node, source);

                    // Iterate ALL children with field name "name" (there can be multiple).
                    // They can be `dotted_name`, `identifier`, or `aliased_import` nodes.
                    let mut child_cursor = node.walk();
                    if child_cursor.goto_first_child() {
                        loop {
                            let child = child_cursor.node();
                            let is_name_field = child_cursor.field_name() == Some("name");

                            if is_name_field {
                                let (imported_name, alias) = match child.kind() {
                                    "aliased_import" => {
                                        let name_n = child.child_by_field_name("name");
                                        let alias_n = child.child_by_field_name("alias");
                                        // name inside aliased_import can be dotted_name
                                        let n = name_n.map(|n| get_node_text(&n, source).to_string());
                                        let a = alias_n.map(|n| get_node_text(&n, source).to_string());
                                        (n, a)
                                    }
                                    "dotted_name" | "identifier" => {
                                        (Some(get_node_text(&child, source).to_string()), None)
                                    }
                                    _ => (None, None),
                                };

                                if let Some(imported) = imported_name {
                                    let full = format!("{module_name}.{imported}");
                                    if !seen.contains(&full) {
                                        seen.insert(full.clone());
                                        imports.push(ImportData {
                                            name: imported,
                                            full_import_name: Some(full),
                                            line_number: child.start_position().row as u32 + 1,
                                            alias,
                                            language: Language::Python,
                                            is_dependency: false,
                                        });
                                    }
                                }
                            }

                            if !child_cursor.goto_next_sibling() {
                                break;
                            }
                        }
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

                // Walk up to the `call` node
                let call_node = {
                    let Some(p) = node.parent() else { continue; };
                    if p.kind() == "call" { p } else { match p.parent() { Some(pp) => pp, None => continue } }
                };
                let Some(func_node) = call_node.child_by_field_name("function") else { continue; };

                let args = extract_call_args(&call_node, source);

                let ctx = get_parent_context(
                    &node,
                    source,
                    &["function_definition", "class_definition"],
                );

                calls.push(FunctionCallData {
                    name: get_node_text(&node, source).to_string(),
                    full_name: get_node_text(&func_node, source).to_string(),
                    line_number: node.start_position().row as u32 + 1,
                    args,
                    inferred_obj_type: None,
                    context: ctx,
                    language: Language::Python,
                });
            }
        }
        calls
    }

    fn find_variables(
        &self,
        source: &[u8],
        root: &Node,
        path: &Path,
        cursor: &mut QueryCursor,
    ) -> Vec<VariableData> {
        let mut variables = Vec::new();
        let Some(name_idx) = self.queries.variables.capture_index_for_name("name") else { return variables; };

        let mut matches = cursor.matches(&self.queries.variables, *root, source);
        while let Some(m) = { matches.advance(); matches.get() } {
            for cap in m.captures {
                if cap.index != name_idx {
                    continue;
                }
                let node = cap.node;
                let Some(assignment) = node.parent() else { continue; };

                // Skip lambda assignments
                let right = assignment.child_by_field_name("right");
                if right.as_ref().map(|r: &Node| r.kind()) == Some("lambda") {
                    continue;
                }

                let name = get_node_text(&node, source).to_string();
                let value = right.as_ref().map(|r| get_node_text(r, source).to_string());
                let type_node = assignment.child_by_field_name("type");
                let type_annotation = type_node.as_ref().map(|t| get_node_text(t, source).to_string());

                let ctx = get_parent_context(
                    &node,
                    source,
                    &["function_definition", "class_definition"],
                );
                let class_ctx = get_parent_context(&node, source, &["class_definition"]);

                variables.push(VariableData {
                    name,
                    path: path.to_path_buf(),
                    line_number: node.start_position().row as u32 + 1,
                    value,
                    type_annotation,
                    context: ctx.map(|(n, _, _)| n),
                    class_context: class_ctx.map(|(n, _, _)| n),
                    language: Language::Python,
                    is_dependency: false,
                });
            }
        }
        variables
    }
}

impl LanguageParser for PythonParser {
    fn language(&self) -> Language {
        Language::Python
    }

    fn parse(&self, path: &Path, source: &[u8], is_dependency: bool) -> Result<FileParseResult> {
        let mut parser = self.make_parser();
        let tree = parser.parse(source, None).ok_or_else(|| Error::Parse {
            path: path.to_path_buf(),
            message: "tree-sitter failed to parse".into(),
        })?;
        let root = tree.root_node();
        let mut cursor = QueryCursor::new();

        let functions = self.find_functions(source, &root, &tree, path, &mut cursor);
        let classes = self.find_classes(source, &root, path, &mut cursor);
        let imports = self.find_imports(source, &root, &mut cursor);
        let function_calls = self.find_calls(source, &root, &mut cursor);
        let variables = self.find_variables(source, &root, path, &mut cursor);

        let mut result = FileParseResult::new(path.to_path_buf(), Language::Python, is_dependency);
        result.functions = functions;
        result.classes = classes;
        result.variables = variables;
        result.imports = imports;
        result.function_calls = function_calls;

        // Populate file-level metadata
        let source_str = String::from_utf8_lossy(source);
        result.total_lines = source_str.lines().count();
        result.comment_line_count = count_comment_nodes(&root);
        let path_str = path.to_string_lossy();
        let file_name = path.file_name().map(|f| f.to_string_lossy()).unwrap_or_default();
        result.is_test_file = path_str.contains("test")
            || file_name.starts_with("test_")
            || file_name.ends_with("_test.py");

        Ok(result)
    }
}

// ── free helpers ──────────────────────────────────────────────────────────

/// Collect TODO/FIXME/HACK/XXX comments from comment nodes inside a subtree.
fn collect_todo_comments(node: &Node, source: &[u8]) -> Vec<String> {
    let mut todos = Vec::new();
    let mut stack = vec![*node];
    while let Some(n) = stack.pop() {
        if n.kind() == "comment" {
            let text = get_node_text(&n, source);
            if text.contains("TODO") || text.contains("FIXME") || text.contains("HACK") || text.contains("XXX") {
                todos.push(text.to_string());
            }
        }
        let mut cursor = n.walk();
        if cursor.goto_first_child() {
            loop {
                stack.push(cursor.node());
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }
    todos
}

/// Collect exception type names from raise statements inside a subtree.
fn collect_raises(node: &Node, source: &[u8]) -> Vec<String> {
    let mut raises = Vec::new();
    let mut stack = vec![*node];
    while let Some(n) = stack.pop() {
        if n.kind() == "raise_statement" {
            // The first named child (if any) is the exception expression
            if let Some(child) = n.child(1) {
                let text = get_node_text(&child, source);
                // Extract just the type name (before any parentheses for constructor calls)
                let type_name = if let Some(pos) = text.find('(') {
                    text[..pos].trim()
                } else {
                    text.trim()
                };
                if !type_name.is_empty() {
                    raises.push(type_name.to_string());
                }
            }
        }
        let mut cursor = n.walk();
        if cursor.goto_first_child() {
            loop {
                stack.push(cursor.node());
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }
    raises
}

/// Check if a subtree contains a try_statement node.
fn has_try_statement(node: &Node) -> bool {
    let mut stack = vec![*node];
    while let Some(n) = stack.pop() {
        if n.kind() == "try_statement" {
            return true;
        }
        let mut cursor = n.walk();
        if cursor.goto_first_child() {
            loop {
                stack.push(cursor.node());
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }
    false
}

/// Count comment nodes in the entire tree.
fn count_comment_nodes(node: &Node) -> usize {
    let mut count = 0;
    let mut stack = vec![*node];
    while let Some(n) = stack.pop() {
        if n.kind() == "comment" {
            count += 1;
        }
        let mut cursor = n.walk();
        if cursor.goto_first_child() {
            loop {
                stack.push(cursor.node());
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }
    count
}

/// Extract parameter names and their type annotations from a Python function's parameters node.
fn extract_python_params_typed(params_node: Option<&Node>, source: &[u8]) -> (Vec<String>, Vec<Option<String>>) {
    let Some(params) = params_node else {
        return (Vec::new(), Vec::new());
    };
    let mut args = Vec::new();
    let mut arg_types = Vec::new();
    let mut cursor = params.walk();
    if !cursor.goto_first_child() {
        return (args, arg_types);
    }
    loop {
        let child = cursor.node();
        let (arg, typ) = match child.kind() {
            "identifier" => {
                (Some(get_node_text(&child, source).to_string()), None)
            }
            "default_parameter" => {
                let name = child
                    .child_by_field_name("name")
                    .map(|n| get_node_text(&n, source).to_string());
                (name, None)
            }
            "typed_default_parameter" => {
                let name = child
                    .child_by_field_name("name")
                    .map(|n| get_node_text(&n, source).to_string());
                let type_ann = child
                    .child_by_field_name("type")
                    .map(|t| get_node_text(&t, source).to_string());
                (name, type_ann)
            }
            "typed_parameter" => {
                // typed_parameter has no `name` field — first identifier child is the name
                let name = child.child(0)
                    .filter(|c| c.kind() == "identifier")
                    .map(|n| get_node_text(&n, source).to_string());
                let type_ann = child
                    .child_by_field_name("type")
                    .map(|t| get_node_text(&t, source).to_string());
                (name, type_ann)
            }
            "list_splat_pattern" | "dictionary_splat_pattern" => {
                (Some(get_node_text(&child, source).to_string()), None)
            }
            _ => (None, None),
        };
        if let Some(a) = arg {
            args.push(a);
            arg_types.push(typ);
        }
        if !cursor.goto_next_sibling() {
            break;
        }
    }
    (args, arg_types)
}

/// Extract class fields from `self.x = ...` and `self.x: Type = ...` patterns in the class body.
fn extract_python_class_fields(body_node: Option<&Node>, source: &[u8]) -> Vec<FieldDecl> {
    let Some(body) = body_node else {
        return Vec::new();
    };
    let mut fields = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Walk all descendants looking for attribute assignments like self.x = ... or self.x: Type
    let mut stack = vec![*body];
    while let Some(node) = stack.pop() {
        // Look for `self.X` in assignment targets
        if node.kind() == "assignment" || node.kind() == "augmented_assignment" {
            if let Some(left) = node.child_by_field_name("left") {
                if left.kind() == "attribute" {
                    let obj = left.child_by_field_name("object");
                    let attr = left.child_by_field_name("attribute");
                    if let (Some(o), Some(a)) = (obj, attr) {
                        if get_node_text(&o, source) == "self" {
                            let name = get_node_text(&a, source).to_string();
                            if seen.insert(name.clone()) {
                                // Check for type annotation
                                let type_ann = node.child_by_field_name("type")
                                    .map(|t| get_node_text(&t, source).to_string());
                                let is_static = false;
                                let visibility = if name.starts_with("__") && !name.ends_with("__") {
                                    Some("private".to_string())
                                } else if name.starts_with('_') {
                                    Some("protected".to_string())
                                } else {
                                    Some("public".to_string())
                                };
                                fields.push(FieldDecl {
                                    name,
                                    type_annotation: type_ann,
                                    visibility,
                                    is_static,
                                });
                            }
                        }
                    }
                }
            }
        }

        // Also handle type-annotated assignments: `self.x: Type = value`
        if node.kind() == "type_alias_statement" || node.kind() == "expression_statement" {
            // Check children for patterns
        }

        // Recurse into children
        let mut cursor = node.walk();
        if cursor.goto_first_child() {
            loop {
                stack.push(cursor.node());
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
        }
    }

    fields
}

fn extract_decorators(node: &Node, source: &[u8]) -> Vec<String> {
    // Decorators live on the `decorated_definition` parent, not on function_definition/class_definition.
    let target = if let Some(parent) = node.parent() {
        if parent.kind() == "decorated_definition" {
            parent
        } else {
            *node
        }
    } else {
        *node
    };

    let mut decorators = Vec::new();
    let mut cursor = target.walk();
    if cursor.goto_first_child() {
        loop {
            if cursor.node().kind() == "decorator" {
                let text = get_node_text(&cursor.node(), source).to_string();
                if !text.is_empty() {
                    decorators.push(text);
                }
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    decorators
}

fn extract_bases(superclasses: &Node, source: &[u8]) -> Vec<String> {
    let mut bases = Vec::new();
    let mut cursor = superclasses.walk();
    if cursor.goto_first_child() {
        loop {
            let child = cursor.node();
            if child.kind() == "identifier" || child.kind() == "attribute" {
                let text = get_node_text(&child, source).to_string();
                if !text.is_empty() {
                    bases.push(text);
                }
            }
            if !cursor.goto_next_sibling() {
                break;
            }
        }
    }
    bases
}

fn extract_docstring(body: &Node, source: &[u8]) -> Option<String> {
    let first = body.child(0)?;
    if first.kind() != "expression_statement" {
        return None;
    }
    let string_node = first.child(0)?;
    if string_node.kind() != "string" {
        return None;
    }
    let raw = get_node_text(&string_node, source);
    Some(strip_string_quotes(raw))
}

fn strip_string_quotes(s: &str) -> String {
    let s = s.trim();
    for delim in &["\"\"\"", "'''", "\"", "'"] {
        if s.starts_with(delim) && s.ends_with(delim) && s.len() >= 2 * delim.len() {
            return s[delim.len()..s.len() - delim.len()].to_string();
        }
    }
    s.to_string()
}

fn extract_call_args(call_node: &Node, source: &[u8]) -> Vec<String> {
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

/// Pre-scan Python files to build an imports_map: name → list of file paths.
pub fn pre_scan_python(
    files: &[std::path::PathBuf],
) -> std::collections::HashMap<String, Vec<String>> {
    let mut imports_map: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    let ts_lang: TsLanguage = tree_sitter_python::LANGUAGE.into();
    let query_str = r#"
        (class_definition name: (identifier) @name)
        (function_definition name: (identifier) @name)
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
