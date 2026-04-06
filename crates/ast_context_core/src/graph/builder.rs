use std::collections::HashMap;
use std::path::{Path, PathBuf};

use log::{info, warn};
use rayon::prelude::*;
use petgraph::graph::NodeIndex;

use super::code_graph::CodeGraph;
use crate::annotate;
use crate::error::{Error, Result};
use crate::parser;
use crate::types::node::*;
use crate::types::{EdgeKind, FileParseResult};
use crate::walker;

/// Builds a `CodeGraph` from a directory of source files.
///
/// Two-pass approach (mirrors the Python implementation):
///   1. Parse all files, add all nodes and CONTAINS/IMPORTS edges.
///   2. Resolve cross-file CALLS and INHERITS relationships using an imports_map.
pub struct GraphBuilder;

impl GraphBuilder {
    /// Combine name + path into a single HashMap key, avoiding tuple allocation.
    #[inline]
    fn node_key(name: &str, path: &Path) -> String {
        format!("{}\0{}", name, path.display())
    }

    /// Build a complete code graph from `root_path`.
    pub fn build(root_path: &Path) -> Result<CodeGraph> {
        Self::build_with_options(root_path, false)
    }

    /// Build a code graph with options.
    ///
    /// When `annotate` is true, each node gets its source code snippet attached.
    /// This enables AI-driven analysis (redundancy detection, refactoring
    /// suggestions) but significantly increases graph size.
    pub fn build_with_options(root_path: &Path, annotate_sources: bool) -> Result<CodeGraph> {
        Self::build_full(root_path, annotate_sources, &[])
    }

    /// Build a code graph with all options including exclude patterns.
    ///
    /// `exclude_patterns` uses gitignore glob syntax (e.g. `"vendor/**"`, `"*.generated.go"`).
    /// These are applied in addition to `.gitignore` and `.astcontextignore` files.
    pub fn build_full(
        root_path: &Path,
        annotate_sources: bool,
        exclude_patterns: &[String],
    ) -> Result<CodeGraph> {
        let root_path = root_path
            .canonicalize()
            .map_err(|e| Error::Io {
                path: root_path.to_path_buf(),
                source: e,
            })?;

        let files = walker::walk_source_files_with_excludes(&root_path, exclude_patterns);
        info!("Found {} source files in {}", files.len(), root_path.display());

        // Pre-scan: build imports_map (name → file paths) from all source files.
        // Maps a file's stem (e.g. "utils" for "utils.py") and any top-level
        // class/function names to the file paths that define them.
        let mut imports_map: HashMap<String, Vec<String>> = HashMap::new();
        for file_path in &files {
            let stem = file_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            if !stem.is_empty() {
                imports_map
                    .entry(stem)
                    .or_default()
                    .push(file_path.to_string_lossy().to_string());
            }
        }

        // Parse all files in parallel
        let all_results: Vec<FileParseResult> = files
            .par_iter()
            .filter_map(|file_path| {
                let ext = file_path.extension().and_then(|e| e.to_str())?;
                let parser = parser::parser_for_extension(ext)?;
                let source = match std::fs::read(file_path) {
                    Ok(s) => s,
                    Err(e) => {
                        warn!("Failed to read {}: {}", file_path.display(), e);
                        return None;
                    }
                };
                match parser.parse(file_path, &source, false) {
                    Ok(mut result) => {
                        if annotate_sources {
                            annotate::annotate_sources(&source, &mut result);
                        }
                        Some(result)
                    }
                    Err(e) => {
                        warn!("Failed to parse {}: {}", file_path.display(), e);
                        None
                    }
                }
            })
            .collect();

        let mut graph = CodeGraph::new();

        // ── Pass 1: add all nodes and structural edges ──────────────────

        // Repository node
        let repo_idx = graph.add_node(GraphNode::Repository(RepositoryData {
            name: root_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            path: root_path.clone(),
            is_dependency: false,
        }));

        // Track directory nodes we've already created
        let mut dir_nodes: HashMap<PathBuf, NodeIndex> = HashMap::new();

        // File-level node index tracking for pass 2
        let mut file_nodes: HashMap<PathBuf, NodeIndex> = HashMap::new();
        // Function/class nodes by "name\0path" for call/inheritance resolution
        // Uses \0-separated key to avoid cloning tuples on every lookup.
        let mut func_nodes: HashMap<String, Vec<NodeIndex>> = HashMap::new();
        let mut class_nodes: HashMap<String, Vec<NodeIndex>> = HashMap::new();

        for result in &all_results {
            let file_path = &result.path;
            let file_name = file_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let rel = file_path
                .strip_prefix(&root_path)
                .unwrap_or(file_path);
            let relative_path = rel.to_string_lossy().to_string();

            // Create directory hierarchy
            let parent_parts: Vec<_> = rel.parent().map(|p| p.components().collect()).unwrap_or_default();
            let mut current_parent_idx = repo_idx;
            let mut current_dir_path = root_path.clone();

            for component in &parent_parts {
                let part = component.as_os_str().to_string_lossy().to_string();
                current_dir_path = current_dir_path.join(&part);

                let dir_idx = *dir_nodes.entry(current_dir_path.clone()).or_insert_with(|| {
                    let idx = graph.add_node(GraphNode::Directory(DirectoryData {
                        name: part.clone(),
                        path: current_dir_path.clone(),
                    }));
                    graph.add_edge(current_parent_idx, idx, EdgeKind::Contains);
                    idx
                });
                current_parent_idx = dir_idx;
            }

            // File node
            // Compute file-level visibility stats
            let public_count = result.functions.iter()
                .filter(|f| f.visibility.as_deref() == Some("public"))
                .count();
            let private_count = result.functions.iter()
                .filter(|f| f.visibility.as_deref() == Some("private") || f.visibility.as_deref() == Some("protected"))
                .count();

            let file_idx = graph.add_node(GraphNode::File(FileData {
                name: file_name,
                path: file_path.clone(),
                relative_path,
                language: result.language,
                is_dependency: result.is_dependency,
                public_count,
                private_count,
                comment_line_count: result.comment_line_count,
                total_lines: result.total_lines,
                is_test_file: result.is_test_file,
            }));
            graph.add_edge(current_parent_idx, file_idx, EdgeKind::Contains);
            file_nodes.insert(file_path.clone(), file_idx);

            // Functions
            for func in &result.functions {
                let func_idx = graph.add_node(GraphNode::Function(func.clone()));
                graph.add_edge(file_idx, func_idx, EdgeKind::Contains);
                func_nodes
                    .entry(Self::node_key(&func.name, file_path))
                    .or_default()
                    .push(func_idx);

                // Nested function containment
                if func.context_type.as_deref() == Some("function_definition") {
                    if let Some(parent_name) = &func.context {
                        if let Some(parent_indices) =
                            func_nodes.get(&Self::node_key(parent_name, file_path))
                        {
                            if let Some(&parent_idx) = parent_indices.last() {
                                graph.add_edge(parent_idx, func_idx, EdgeKind::Contains);
                            }
                        }
                    }
                }

                // Class method containment
                if let Some(class_name) = &func.class_context {
                    if let Some(class_indices) =
                        class_nodes.get(&Self::node_key(class_name, file_path))
                    {
                        if let Some(&class_idx) = class_indices.last() {
                            graph.add_edge(class_idx, func_idx, EdgeKind::Contains);
                        }
                    }
                }
            }

            // Classes
            for class in &result.classes {
                let class_idx = graph.add_node(GraphNode::Class(class.clone()));
                graph.add_edge(file_idx, class_idx, EdgeKind::Contains);
                class_nodes
                    .entry(Self::node_key(&class.name, file_path))
                    .or_default()
                    .push(class_idx);
            }

            // Re-link methods to their classes (classes may have been added after the functions)
            for func in &result.functions {
                if let Some(class_name) = &func.class_context {
                    if let Some(class_indices) =
                        class_nodes.get(&Self::node_key(class_name, file_path))
                    {
                        if let Some(&class_idx) = class_indices.last() {
                            if let Some(func_indices) =
                                func_nodes.get(&Self::node_key(&func.name, file_path))
                            {
                                if let Some(&func_idx) = func_indices.last() {
                                    graph.add_edge(class_idx, func_idx, EdgeKind::Contains);
                                }
                            }
                        }
                    }
                }
            }

            // Variables
            for var in &result.variables {
                let var_idx = graph.add_node(GraphNode::Variable(var.clone()));
                graph.add_edge(file_idx, var_idx, EdgeKind::Contains);
            }

            // Imports → Module nodes
            for imp in &result.imports {
                let module_idx = graph.add_node(GraphNode::Module(ModuleData {
                    name: imp.name.clone(),
                    full_import_name: imp.full_import_name.clone(),
                    language: imp.language,
                }));
                graph.add_edge(
                    file_idx,
                    module_idx,
                    EdgeKind::Imports {
                        line_number: imp.line_number,
                        alias: imp.alias.clone(),
                        imported_name: imp.full_import_name.clone(),
                    },
                );
            }

            // Traits
            for tr in &result.traits {
                let idx = graph.add_node(GraphNode::Trait(tr.clone()));
                graph.add_edge(file_idx, idx, EdgeKind::Contains);
            }

            // Interfaces
            for iface in &result.interfaces {
                let idx = graph.add_node(GraphNode::Interface(iface.clone()));
                graph.add_edge(file_idx, idx, EdgeKind::Contains);
            }

            // Structs
            for st in &result.structs {
                let idx = graph.add_node(GraphNode::Struct(st.clone()));
                graph.add_edge(file_idx, idx, EdgeKind::Contains);
            }

            // Enums
            for en in &result.enums {
                let idx = graph.add_node(GraphNode::Enum(en.clone()));
                graph.add_edge(file_idx, idx, EdgeKind::Contains);
            }

            // Macros
            for mac in &result.macros {
                let idx = graph.add_node(GraphNode::Macro(mac.clone()));
                graph.add_edge(file_idx, idx, EdgeKind::Contains);
            }
        }

        // ── Pass 2: resolve CALLS and INHERITS ─────────────────────────

        // Build lookup: function name → Vec<NodeIndex> (for global fallback resolution)
        // Uses owned String keys since we need mutable graph access later.
        let mut global_func_lookup: HashMap<String, Vec<NodeIndex>> = HashMap::new();
        for idx in graph.graph.node_indices() {
            if let GraphNode::Function(f) = &graph.graph[idx] {
                global_func_lookup
                    .entry(f.name.clone())
                    .or_default()
                    .push(idx);
            }
        }

        for result in &all_results {
            let caller_path = &result.path;

            // Build local name sets
            let local_names: std::collections::HashSet<&str> = result
                .functions
                .iter()
                .map(|f| f.name.as_str())
                .chain(result.classes.iter().map(|c| c.name.as_str()))
                .collect();

            let local_imports: HashMap<&str, &str> = result
                .imports
                .iter()
                .map(|imp| {
                    let key = imp
                        .alias
                        .as_deref()
                        .unwrap_or_else(|| imp.name.split('.').last().unwrap_or(&imp.name));
                    let val = imp.name.as_str();
                    (key, val)
                })
                .collect();

            // Pre-compute import needles (replace '.' with '/') to avoid per-call allocation
            let import_needles: HashMap<&str, String> = local_imports
                .iter()
                .map(|(&key, &val)| (key, val.replace('.', "/")))
                .collect();

            // ── CALLS resolution ────────────────────────────────────────
            for call in &result.function_calls {
                let called_name = &call.name;
                let full_call = &call.full_name;
                let base_obj = if full_call.contains('.') {
                    Some(full_call.split('.').next().unwrap())
                } else {
                    None
                };

                let mut resolved_path: Option<PathBuf> = None;

                // 1. self/this/cls calls → same file
                if let Some(base) = base_obj {
                    if ["self", "this", "super", "cls", "@"].contains(&base) {
                        resolved_path = Some(caller_path.clone());
                    }
                }

                // 2. Local name
                if resolved_path.is_none() {
                    let lookup = base_obj.unwrap_or(called_name.as_str());
                    if local_names.contains(lookup) {
                        resolved_path = Some(caller_path.clone());
                    }
                }

                // 3. Via imports_map
                if resolved_path.is_none() {
                    let lookup = base_obj.unwrap_or(called_name.as_str());
                    if let Some(paths) = imports_map.get(lookup) {
                        if paths.len() == 1 {
                            resolved_path = Some(PathBuf::from(&paths[0]));
                        } else if let Some(needle) = import_needles.get(lookup) {
                            // Disambiguate via import path (pre-computed needle)
                            for p in paths {
                                if p.contains(needle.as_str()) {
                                    resolved_path = Some(PathBuf::from(p));
                                    break;
                                }
                            }
                        }
                    }
                }

                // 4. Fallback: try called_name directly
                if resolved_path.is_none() {
                    if local_names.contains(called_name.as_str()) {
                        resolved_path = Some(caller_path.clone());
                    } else if let Some(paths) = imports_map.get(called_name.as_str()) {
                        if !paths.is_empty() {
                            resolved_path = Some(PathBuf::from(&paths[0]));
                        }
                    } else {
                        resolved_path = Some(caller_path.clone());
                    }
                }

                let resolved = resolved_path.unwrap_or_else(|| caller_path.clone());

                // Find caller node
                let caller_idx = if let Some(ctx) = &call.context {
                    // Caller is the enclosing function/class
                    let caller_name = &ctx.0;
                    func_nodes
                        .get(&Self::node_key(caller_name, caller_path))
                        .and_then(|v| v.last().copied())
                        .or_else(|| {
                            class_nodes
                                .get(&Self::node_key(caller_name, caller_path))
                                .and_then(|v| v.last().copied())
                        })
                } else {
                    // File-level call
                    file_nodes.get(caller_path).copied()
                };

                // Find callee node
                let callee_idx = func_nodes
                    .get(&Self::node_key(called_name, &resolved))
                    .and_then(|v| v.last().copied())
                    .or_else(|| {
                        // Try class (constructor call)
                        class_nodes
                            .get(&Self::node_key(called_name, &resolved))
                            .and_then(|v| v.last().copied())
                    })
                    .or_else(|| {
                        // Global fallback by name
                        global_func_lookup
                            .get(called_name.as_str())
                            .and_then(|v| v.first().copied())
                    });

                if let (Some(from), Some(to)) = (caller_idx, callee_idx) {
                    if from != to {
                        graph.add_edge(
                            from,
                            to,
                            EdgeKind::Calls {
                                line_number: call.line_number,
                                args: call.args.clone(),
                                full_call_name: call.full_name.clone(),
                            },
                        );
                    }
                }
            }

            // ── INHERITS resolution ─────────────────────────────────────
            for class in &result.classes {
                let child_indices = match class_nodes.get(&Self::node_key(&class.name, caller_path))
                {
                    Some(v) => v.as_slice(),
                    None => continue,
                };

                for base_str in &class.bases {
                    if base_str == "object" {
                        continue;
                    }
                    let target_class = base_str.split('.').last().unwrap_or(base_str);

                    let mut resolved_path: Option<PathBuf> = None;

                    if base_str.contains('.') {
                        let prefix = base_str.split('.').next().unwrap();
                        if let Some(needle) = import_needles.get(prefix) {
                            if let Some(paths) = imports_map.get(target_class) {
                                for p in paths {
                                    if p.contains(needle.as_str()) {
                                        resolved_path = Some(PathBuf::from(p));
                                        break;
                                    }
                                }
                            }
                        }
                    } else {
                        // Local class?
                        if class_nodes.contains_key(&Self::node_key(target_class, caller_path))
                        {
                            resolved_path = Some(caller_path.clone());
                        } else if let Some(needle) = import_needles.get(target_class) {
                            if let Some(paths) = imports_map.get(target_class) {
                                for p in paths {
                                    if p.contains(needle.as_str()) {
                                        resolved_path = Some(PathBuf::from(p));
                                        break;
                                    }
                                }
                            }
                        } else if let Some(paths) = imports_map.get(target_class) {
                            if paths.len() == 1 {
                                resolved_path = Some(PathBuf::from(&paths[0]));
                            }
                        }
                    }

                    if let Some(resolved) = resolved_path {
                        let parent_indices = class_nodes
                            .get(&Self::node_key(target_class, &resolved))
                            .map(|v| v.as_slice())
                            .unwrap_or(&[]);

                        for &child_idx in child_indices {
                            for &parent_idx in parent_indices {
                                if child_idx != parent_idx {
                                    graph.add_edge(child_idx, parent_idx, EdgeKind::Inherits);
                                }
                            }
                        }
                    }
                }
            }
        }

        // ── Pass 3: resolve test → function mapping ────────────────────
        //
        // For each function named test_foo or testFoo in a test file,
        // try to find a matching function `foo` and add a Tests edge.
        // Uses HashMap for O(1) lookup instead of O(n) inner loop.
        {
            // Pre-build set of test file paths for O(1) lookup
            let test_file_paths: std::collections::HashSet<&Path> = file_nodes
                .iter()
                .filter_map(|(_, fidx)| {
                    if let GraphNode::File(fd) = &graph.graph[*fidx] {
                        if fd.is_test_file { Some(fd.path.as_path()) } else { None }
                    } else {
                        None
                    }
                })
                .collect();

            // Build prod function lookup: lowercase name → NodeIndex
            let mut prod_lookup: HashMap<String, NodeIndex> = HashMap::new();
            let mut test_funcs: Vec<(NodeIndex, String)> = Vec::new();

            for idx in graph.graph.node_indices() {
                if let GraphNode::Function(f) = &graph.graph[idx] {
                    let is_test = test_file_paths.contains(f.path.as_path())
                        || f.name.starts_with("test_")
                        || f.name.starts_with("test")
                        || f.decorators.iter().any(|d| d.contains("test"));

                    if is_test {
                        test_funcs.push((idx, f.name.clone()));
                    } else {
                        prod_lookup.entry(f.name.to_lowercase()).or_insert(idx);
                    }
                }
            }

            for (test_idx, test_name) in &test_funcs {
                let target = test_name
                    .strip_prefix("test_")
                    .or_else(|| test_name.strip_prefix("test"))
                    .unwrap_or("");

                if target.len() < 2 {
                    continue;
                }

                if let Some(&prod_idx) = prod_lookup.get(&target.to_lowercase()) {
                    graph.graph.add_edge(*test_idx, prod_idx, EdgeKind::Tests);
                }
            }
        }

        info!(
            "Graph built: {} nodes, {} edges",
            graph.node_count(),
            graph.edge_count()
        );
        Ok(graph)
    }
}
