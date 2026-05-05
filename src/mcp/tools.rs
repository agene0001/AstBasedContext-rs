//! MCP tool definitions and handlers.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::graph::CodeGraph;
use crate::types::node::GraphNode;
use crate::types::EdgeKind;
use crate::GraphBuilder;
use serde_json::json;

use super::protocol::{ToolContent, ToolDefinition, ToolResult};

/// Shared server state.
pub struct ServerState {
    /// Indexed graphs keyed by root path.
    pub graphs: HashMap<PathBuf, CodeGraph>,
}

impl ServerState {
    pub fn new() -> Self {
        Self {
            graphs: HashMap::new(),
        }
    }
}

pub type SharedState = Arc<Mutex<ServerState>>;

/// Return all tool definitions.
pub fn list_tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "index_directory".to_string(),
            description: "Index a directory and build its code graph. \
                Results are automatically cached to .ast_context_cache.json inside the directory. \
                On subsequent calls the cache is reloaded instantly if no source files have changed; \
                if any source file is newer than the cache it automatically re-indexes. \
                Use force_reindex=true to force a full rebuild regardless.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the directory to index"
                    },
                    "annotate": {
                        "type": "boolean",
                        "description": "Attach source snippets to nodes for similarity/redundancy analysis (slower, larger graph)"
                    },
                    "exclude": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Glob patterns to exclude (e.g. [\"vendor/**\", \"*.generated.go\"]). Also reads .astcontextignore files."
                    },
                    "max_file_size_mb": {
                        "type": "integer",
                        "description": "Maximum file size in MB to index (default: 50). Files larger than this are skipped."
                    },
                    "skip_tests": {
                        "type": "boolean",
                        "description": "Exclude test files from the graph for a smaller, faster index focused on production code (default: false)."
                    },
                    "force_reindex": {
                        "type": "boolean",
                        "description": "Force a full re-index even if the cache is up-to-date (default: false)."
                    }
                },
                "required": ["path"]
            }),
        },
        ToolDefinition {
            name: "find_code".to_string(),
            description: "Search for functions, classes, or other code elements by name".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query (name or partial name)"
                    },
                    "kind": {
                        "type": "string",
                        "description": "Node type filter: Function, Class, Struct, Trait, Interface, Enum, Variable, Module",
                        "enum": ["Function", "Class", "Struct", "Trait", "Interface", "Enum", "Variable", "Module"]
                    },
                    "repository": {
                        "type": "string",
                        "description": "Path of the indexed repository to query (optional — defaults to the first indexed repo)"
                    }
                },
                "required": ["query"]
            }),
        },
        ToolDefinition {
            name: "get_file_summary".to_string(),
            description: "List all symbols (functions, classes, structs, etc.) defined in a specific file.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the source file (absolute or partial — e.g. 'src/parser/python.rs')"
                    },
                    "repository": {
                        "type": "string",
                        "description": "Path of the indexed repository to query (optional)"
                    }
                },
                "required": ["path"]
            }),
        },
        ToolDefinition {
            name: "analyze_relationships".to_string(),
            description: "Analyze code relationships: callers, callees, inheritance, call chains"
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Name of the function or class to analyze"
                    },
                    "relationship": {
                        "type": "string",
                        "description": "Type of relationship to analyze",
                        "enum": ["callers", "callees", "inheritance", "call_chain", "implementors", "children"]
                    },
                    "max_depth": {
                        "type": "integer",
                        "description": "Maximum depth for call chain analysis (default: 5)"
                    },
                    "repository": {
                        "type": "string",
                        "description": "Path of the indexed repository to query (optional)"
                    }
                },
                "required": ["name", "relationship"]
            }),
        },
        ToolDefinition {
            name: "find_dead_code".to_string(),
            description: "Find functions that are never called (dead code candidates)".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results (default: 50)"
                    },
                    "repository": {
                        "type": "string",
                        "description": "Path of the indexed repository to query (optional)"
                    }
                }
            }),
        },
        ToolDefinition {
            name: "find_complex_functions".to_string(),
            description: "Find the most complex functions ranked by cyclomatic complexity"
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results (default: 20)"
                    },
                    "repository": {
                        "type": "string",
                        "description": "Path of the indexed repository to query (optional)"
                    }
                }
            }),
        },
        ToolDefinition {
            name: "get_stats".to_string(),
            description: "Get statistics about the indexed code graph".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "repository": {
                        "type": "string",
                        "description": "Path of the indexed repository to query (optional — omit to show all)"
                    }
                }
            }),
        },
        ToolDefinition {
            name: "list_repositories".to_string(),
            description: "List all indexed repositories".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        ToolDefinition {
            name: "find_similar".to_string(),
            description: "Find groups of potentially redundant/similar code. Requires index with annotate=true. Returns groups of nodes with similar structure and token overlap.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "kind": {
                        "type": "string",
                        "description": "Node type filter: Function, Class, Struct, Trait, Interface, Enum",
                        "enum": ["Function", "Class", "Struct", "Trait", "Interface", "Enum"]
                    },
                    "min_lines": {
                        "type": "integer",
                        "description": "Minimum lines for a node to be considered (default: 5)"
                    },
                    "repository": {
                        "type": "string",
                        "description": "Path of the indexed repository to query (optional)"
                    }
                }
            }),
        },
        ToolDefinition {
            name: "analyze_redundancy".to_string(),
            description: "Run tiered redundancy analysis: finds passthrough wrappers, near-duplicates, \
                structural similarity, merge candidates, and split candidates. Returns findings ranked \
                Critical > High > Medium > Low. \
                Output uses compact tags for Tiers ([C], [H], [M], [L]) and Type Initials (e.g. [PT]=PASSTHROUGH, [ND]=NEAR-DUPLICATE). \
                Requires annotate=true on index.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "min_tier": {
                        "type": "string",
                        "description": "Minimum tier to include (default: low)",
                        "enum": ["critical", "high", "medium", "low"]
                    },
                    "min_lines": {
                        "type": "integer",
                        "description": "Minimum function lines to consider (default: 3)"
                    },
                    "near_dup_threshold": {
                        "type": "number",
                        "description": "Similarity threshold for near-duplicate detection 0.0-1.0 (default: 0.80)"
                    },
                    "structural_threshold": {
                        "type": "number",
                        "description": "Similarity threshold for structural similarity 0.0-1.0 (default: 0.50)"
                    },
                    "merge_threshold": {
                        "type": "number",
                        "description": "Shared line ratio for merge candidates 0.0-1.0 (default: 0.40)"
                    },
                    "skip_checks": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "List of specific checks or categories to skip (e.g., ['detect_dead_code', 'anti_patterns'])"
                    },
                    "include_source": {
                        "type": "boolean",
                        "description": "Include full source code snippets in output. Significantly increases context usage. (default: false)"
                    },
                    "category": {
                        "type": "string",
                        "description": "Filter findings to a specific category",
                        "enum": ["redundancy", "struct_enum", "type_suggestions", "design_patterns",
                                 "anti_patterns", "pattern_detection", "structural", "type_system",
                                 "metrics", "risk", "testing", "blast_radius", "api_surface",
                                 "cross_language", "config_detection", "data_structures",
                                 "code_quality", "optimization"]
                    },
                    "limit_per_type": {
                        "type": "integer",
                        "description": "Maximum number of findings to return per redundancy type (default: 5, 0 = all)"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of total findings to return (default: no limit)"
                    },
                    "repository": {
                        "type": "string",
                        "description": "Path of the indexed repository to query (optional)"
                    }
                }
            }),
        },
        ToolDefinition {
            name: "get_context_for_symbol".to_string(),
            description: "Get all context an LLM needs before editing a symbol: its source, \
                direct callers, direct callees, and similar functions — in one call.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Name of the symbol to get context for"
                    },
                    "kind": {
                        "type": "string",
                        "description": "Optional type filter: Function, Class, Struct, Trait, Interface, Enum",
                        "enum": ["Function", "Class", "Struct", "Trait", "Interface", "Enum"]
                    },
                    "repository": {
                        "type": "string",
                        "description": "Path of the indexed repository to query (optional)"
                    }
                },
                "required": ["name"]
            }),
        },
        ToolDefinition {
            name: "find_references".to_string(),
            description: "Find all usages of a symbol across the codebase: callers, inheritors, \
                implementors, and files that import it — more thorough than analyze_relationships.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Name of the symbol to find references to"
                    },
                    "repository": {
                        "type": "string",
                        "description": "Path of the indexed repository to query (optional)"
                    }
                },
                "required": ["name"]
            }),
        },
        ToolDefinition {
            name: "get_module_overview".to_string(),
            description: "Get a directory-level overview: files, their public/private symbol \
                counts, lines of code, and cross-file call relationships within the module.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Directory path to summarise (absolute or partial, e.g. 'src/parser')"
                    },
                    "repository": {
                        "type": "string",
                        "description": "Path of the indexed repository to query (optional)"
                    }
                },
                "required": ["path"]
            }),
        },
        ToolDefinition {
            name: "save_graph".to_string(),
            description: "Save an indexed graph to a file so it can be reloaded in future sessions without re-indexing.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path to save the graph to (e.g. /tmp/myproject.json)"
                    }
                },
                "required": ["path"]
            }),
        },
        ToolDefinition {
            name: "load_graph".to_string(),
            description: "Load a previously saved graph from a file, restoring it into the session without re-indexing.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path to load the graph from"
                    }
                },
                "required": ["path"]
            }),
        },
        ToolDefinition {
            name: "get_source".to_string(),
            description: "Get the source code snippet for a named symbol (function, class, struct, etc.). Requires index with annotate=true.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Name of the symbol to retrieve source for"
                    },
                    "kind": {
                        "type": "string",
                        "description": "Optional node type filter: Function, Class, Struct, Trait, Interface, Enum",
                        "enum": ["Function", "Class", "Struct", "Trait", "Interface", "Enum"]
                    },
                    "repository": {
                        "type": "string",
                        "description": "Path of the indexed repository to query (optional)"
                    }
                },
                "required": ["name"]
            }),
        },
    ]
}

/// Dispatch a tool call to its handler.
pub fn handle_tool(state: &SharedState, tool_name: &str, args: &serde_json::Value) -> ToolResult {
    match tool_name {
        "index_directory" => handle_index_directory(state, args),
        "find_code" => handle_find_code(state, args),
        "get_file_summary" => handle_get_file_summary(state, args),
        "analyze_relationships" => handle_analyze_relationships(state, args),
        "find_dead_code" => handle_find_dead_code(state, args),
        "find_complex_functions" => handle_find_complex_functions(state, args),
        "get_stats" => handle_get_stats(state, args),
        "list_repositories" => handle_list_repositories(state),
        "find_similar" => handle_find_similar(state, args),
        "analyze_redundancy" => handle_analyze_redundancy(state, args),
        "get_context_for_symbol" => handle_get_context_for_symbol(state, args),
        "find_references" => handle_find_references(state, args),
        "get_module_overview" => handle_get_module_overview(state, args),
        "save_graph" => handle_save_graph(state, args),
        "load_graph" => handle_load_graph(state, args),
        "get_source" => handle_get_source(state, args),
        _ => ToolResult {
            content: vec![ToolContent::text(format!("Unknown tool: {tool_name}"))],
            is_error: Some(true),
        },
    }
}

fn handle_index_directory(state: &SharedState, args: &serde_json::Value) -> ToolResult {
    let path_str = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => {
            return ToolResult {
                content: vec![ToolContent::text("Missing required parameter: path".into())],
                is_error: Some(true),
            }
        }
    };

    let path = PathBuf::from(path_str);
    if !path.exists() {
        return ToolResult {
            content: vec![ToolContent::text(format!(
                "Path does not exist: {path_str}"
            ))],
            is_error: Some(true),
        };
    }

    let annotate = args
        .get("annotate")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let exclude: Vec<String> = args
        .get("exclude")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let max_file_size: Option<u64> = args
        .get("max_file_size_mb")
        .and_then(|v| v.as_u64())
        .map(|mb| mb * 1024 * 1024);

    let skip_tests = args
        .get("skip_tests")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let force_reindex = args
        .get("force_reindex")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let canonical = path.canonicalize().unwrap_or(path.clone());
    let cache_path = canonical.join(".ast_context_cache.json");

    // Try loading from cache unless force_reindex was requested or source files have changed.
    if !force_reindex && cache_path.exists() {
        let stale = !cache_is_fresh(&canonical, &cache_path);
        if stale {
            log::info!(
                "Source files changed since last index, re-indexing {}",
                canonical.display()
            );
        } else {
            // load_with_config rejects the cache if annotate or exclude patterns changed.
            match crate::graph::CodeGraph::load_with_config(
                &cache_path,
                Some(annotate),
                Some(&exclude),
            ) {
                Ok(graph) => {
                    let node_count = graph.node_count();
                    let edge_count = graph.edge_count();
                    let annotated = graph.has_annotations();
                    let mut s = state.lock().unwrap();
                    s.graphs.insert(canonical.clone(), graph);
                    return ToolResult {
                        content: vec![ToolContent::text(format!(
                            "Loaded from cache: {}.\nGraph: {} nodes, {} edges{}.",
                            canonical.display(),
                            node_count,
                            edge_count,
                            if annotated { ", annotated" } else { "" },
                        ))],
                        is_error: None,
                    };
                }
                Err(e) => {
                    // Cache stale, config mismatch, or version-mismatched — fall through to re-index.
                    log::info!(
                        "Cache invalid ({}), re-indexing: {}",
                        cache_path.display(),
                        e
                    );
                }
            }
        }
    }

    match GraphBuilder::build_full_with_options(
        &canonical,
        annotate,
        &exclude,
        max_file_size,
        skip_tests,
    ) {
        Ok(graph) => {
            let node_count = graph.node_count();
            let edge_count = graph.edge_count();

            // Auto-save cache with config fingerprint.
            let cache_msg = match graph.save_with_config(&cache_path, annotate, &exclude) {
                Ok(()) => {
                    ensure_gitignore(&canonical);
                    format!(" (cached to {})", cache_path.display())
                }
                Err(e) => format!(" (cache write failed: {e})"),
            };

            let mut s = state.lock().unwrap();
            s.graphs.insert(canonical.clone(), graph);

            ToolResult {
                content: vec![ToolContent::text(format!(
                    "Successfully indexed {}{}.\nGraph: {} nodes, {} edges.",
                    canonical.display(),
                    cache_msg,
                    node_count,
                    edge_count,
                ))],
                is_error: None,
            }
        }
        Err(e) => ToolResult {
            content: vec![ToolContent::text(format!("Indexing failed: {e}"))],
            is_error: Some(true),
        },
    }
}

/// Run `f` against the graph for `repository` (if specified) or the first indexed graph.
fn with_graph<F>(state: &SharedState, repository: Option<&str>, f: F) -> ToolResult
where
    F: FnOnce(&CodeGraph) -> ToolResult,
{
    let s = state.lock().unwrap();
    if s.graphs.is_empty() {
        return ToolResult {
            content: vec![ToolContent::text(
                "No repositories indexed. Use index_directory first.".into(),
            )],
            is_error: Some(true),
        };
    }
    if let Some(repo) = repository {
        let target = PathBuf::from(repo);
        // Try exact match first, then suffix match.
        let graph = s.graphs.get(&target).or_else(|| {
            s.graphs
                .iter()
                .find(|(k, _)| k.ends_with(&target))
                .map(|(_, v)| v)
        });
        match graph {
            Some(g) => f(g),
            None => ToolResult {
                content: vec![ToolContent::text(format!(
                    "No indexed repository matching '{repo}'. \
                     Use list_repositories to see what is indexed."
                ))],
                is_error: Some(true),
            },
        }
    } else {
        f(s.graphs.values().next().unwrap())
    }
}

/// Convenience wrapper — no repository filtering.
fn with_any_graph<F>(state: &SharedState, f: F) -> ToolResult
where
    F: FnOnce(&CodeGraph) -> ToolResult,
{
    with_graph(state, None, f)
}

fn handle_find_code(state: &SharedState, args: &serde_json::Value) -> ToolResult {
    let query = match args.get("query").and_then(|v| v.as_str()) {
        Some(q) => q,
        None => {
            return ToolResult {
                content: vec![ToolContent::text(
                    "Missing required parameter: query".into(),
                )],
                is_error: Some(true),
            }
        }
    };
    let kind_filter = args.get("kind").and_then(|v| v.as_str());
    let repo = args.get("repository").and_then(|v| v.as_str());

    with_graph(state, repo, |graph| {
        let results = graph.search_by_name(query);
        let filtered: Vec<_> = results
            .into_iter()
            .filter(|(_, node)| {
                if let Some(kind) = kind_filter {
                    node.label() == kind
                } else {
                    true
                }
            })
            .take(50)
            .collect();

        if filtered.is_empty() {
            return ToolResult {
                content: vec![ToolContent::text(format!("No results found for '{query}'"))],
                is_error: None,
            };
        }

        let mut text = format!("Found {} results for '{query}':\n\n", filtered.len());
        for (_, node) in &filtered {
            text.push_str(&format_node(node));
            text.push('\n');
        }

        ToolResult {
            content: vec![ToolContent::text(text)],
            is_error: None,
        }
    })
}

fn handle_get_file_summary(state: &SharedState, args: &serde_json::Value) -> ToolResult {
    let file_path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => {
            return ToolResult {
                content: vec![ToolContent::text("Missing required parameter: path".into())],
                is_error: Some(true),
            }
        }
    };
    let repo = args.get("repository").and_then(|v| v.as_str());

    with_graph(state, repo, |graph| {
        // Collect all nodes whose file path ends with the provided path string.
        let needle = std::path::Path::new(file_path);
        let mut matches: Vec<&GraphNode> = graph
            .graph
            .node_indices()
            .filter_map(|idx| {
                let node = &graph.graph[idx];
                let node_path = match node {
                    GraphNode::Function(f) => Some(f.path.as_path()),
                    GraphNode::Class(c) => Some(c.path.as_path()),
                    GraphNode::Struct(s) => Some(s.path.as_path()),
                    GraphNode::Trait(t) => Some(t.path.as_path()),
                    GraphNode::Interface(i) => Some(i.path.as_path()),
                    GraphNode::Enum(e) => Some(e.path.as_path()),
                    GraphNode::Variable(v) => Some(v.path.as_path()),
                    GraphNode::Macro(m) => Some(m.path.as_path()),
                    _ => None,
                }?;
                if node_path.ends_with(needle) || needle.ends_with(node_path) || node_path == needle {
                    Some(node)
                } else {
                    None
                }
            })
            .collect();

        if matches.is_empty() {
            return ToolResult {
                content: vec![ToolContent::text(format!(
                    "No symbols found in '{file_path}'. \
                     Check the path is correct and the directory is indexed."
                ))],
                is_error: None,
            };
        }

        // Sort by line number for readable output.
        matches.sort_by_key(|n| match n {
            GraphNode::Function(f) => f.span.start_line,
            GraphNode::Class(c) => c.span.start_line,
            GraphNode::Struct(s) => s.span.start_line,
            GraphNode::Trait(t) => t.span.start_line,
            GraphNode::Interface(i) => i.span.start_line,
            GraphNode::Enum(e) => e.span.start_line,
            GraphNode::Variable(v) => v.line_number,
            _ => 0,
        });

        // Determine the canonical file path from the first match for the header.
        let canonical_path = match matches[0] {
            GraphNode::Function(f) => f.path.display().to_string(),
            GraphNode::Class(c) => c.path.display().to_string(),
            _ => file_path.to_string(),
        };

        let mut text = format!(
            "Symbols in {} ({} found):\n\n",
            canonical_path,
            matches.len()
        );
        for node in &matches {
            text.push_str(&format_node(node));
            text.push('\n');
        }

        ToolResult {
            content: vec![ToolContent::text(text)],
            is_error: None,
        }
    })
}

fn handle_analyze_relationships(state: &SharedState, args: &serde_json::Value) -> ToolResult {
    let name = match args.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => {
            return ToolResult {
                content: vec![ToolContent::text("Missing required parameter: name".into())],
                is_error: Some(true),
            }
        }
    };
    let relationship = match args.get("relationship").and_then(|v| v.as_str()) {
        Some(r) => r,
        None => {
            return ToolResult {
                content: vec![ToolContent::text(
                    "Missing required parameter: relationship".into(),
                )],
                is_error: Some(true),
            }
        }
    };
    let max_depth = args.get("max_depth").and_then(|v| v.as_u64()).unwrap_or(5) as usize;
    let repo = args.get("repository").and_then(|v| v.as_str());

    with_graph(state, repo, |graph| {
        // Find the node by name (try functions first, then classes)
        let indices = graph.find_functions(name);
        let indices = if indices.is_empty() {
            graph.find_classes(name)
        } else {
            indices
        };
        let indices = if indices.is_empty() {
            // Broader search
            graph
                .search_by_name(name)
                .into_iter()
                .map(|(idx, _)| idx)
                .collect()
        } else {
            indices
        };

        if indices.is_empty() {
            return ToolResult {
                content: vec![ToolContent::text(format!(
                    "No node found with name '{name}'"
                ))],
                is_error: None,
            };
        }

        let idx = indices[0];
        let mut text = String::new();

        match relationship {
            "callers" => {
                let callers = graph.get_callers_of(idx);
                text.push_str(&format!("Callers of '{name}' ({} found):\n", callers.len()));
                let list: Vec<_> = callers.iter().map(|(_, n)| format_node_brief(n)).collect();
                if !list.is_empty() {
                    text.push_str(&format!("  └─ {}\n", list.join(", ")));
                }
            }
            "callees" => {
                let callees = graph.get_callees_of(idx);
                text.push_str(&format!(
                    "Functions called by '{name}' ({} found):\n",
                    callees.len()
                ));
                let list: Vec<_> = callees.iter().map(|(_, n)| format_node_brief(n)).collect();
                if !list.is_empty() {
                    text.push_str(&format!("  └─ {}\n", list.join(", ")));
                }
            }
            "inheritance" => {
                let chain = graph.get_inheritance_chain(idx);
                text.push_str(&format!("Inheritance chain for '{name}':\n"));
                text.push_str(&format!("  {name}\n"));
                for (i, (_, node)) in chain.iter().enumerate() {
                    text.push_str(&format!(
                        "  {}↳ {}\n",
                        "  ".repeat(i + 1),
                        format_node_brief(node)
                    ));
                }
            }
            "call_chain" => {
                let chain = graph.get_call_chain(idx, max_depth);
                text.push_str(&format!(
                    "Call chain from '{name}' (depth {max_depth}, {} nodes):\n",
                    chain.len()
                ));
                for (_, node, depth) in &chain {
                    text.push_str(&format!(
                        "  {}→ {}\n",
                        "  ".repeat(*depth),
                        format_node_brief(node)
                    ));
                }
            }
            "implementors" => {
                let impls = graph.get_implementors(idx);
                text.push_str(&format!(
                    "Implementors of '{name}' ({} found):\n",
                    impls.len()
                ));
                let list: Vec<_> = impls.iter().map(|(_, n)| format_node_brief(n)).collect();
                if !list.is_empty() {
                    text.push_str(&format!("  └─ {}\n", list.join(", ")));
                }
            }
            "children" => {
                let children = graph.get_children(idx);
                text.push_str(&format!(
                    "Children of '{name}' ({} found):\n",
                    children.len()
                ));
                let list: Vec<_> = children.iter().map(|(_, n)| format_node_brief(n)).collect();
                if !list.is_empty() {
                    text.push_str(&format!("  └─ {}\n", list.join(", ")));
                }
            }
            _ => {
                return ToolResult {
                    content: vec![ToolContent::text(format!(
                        "Unknown relationship type: {relationship}"
                    ))],
                    is_error: Some(true),
                };
            }
        }

        ToolResult {
            content: vec![ToolContent::text(text)],
            is_error: None,
        }
    })
}

fn handle_find_dead_code(state: &SharedState, args: &serde_json::Value) -> ToolResult {
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize;
    let repo = args.get("repository").and_then(|v| v.as_str());

    with_graph(state, repo, |graph| {
        let dead: Vec<_> = graph.find_dead_code().into_iter().take(limit).collect();
        if dead.is_empty() {
            return ToolResult {
                content: vec![ToolContent::text("No dead code candidates found.".into())],
                is_error: None,
            };
        }

        let mut text = format!("Dead code candidates ({} found):\n\n", dead.len());
        for (_, node) in &dead {
            text.push_str(&format_node(node));
            text.push('\n');
        }

        ToolResult {
            content: vec![ToolContent::text(text)],
            is_error: None,
        }
    })
}

fn handle_find_complex_functions(state: &SharedState, args: &serde_json::Value) -> ToolResult {
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;
    let repo = args.get("repository").and_then(|v| v.as_str());

    with_graph(state, repo, |graph| {
        let funcs = graph.most_complex_functions(limit);
        if funcs.is_empty() {
            return ToolResult {
                content: vec![ToolContent::text("No functions found.".into())],
                is_error: None,
            };
        }

        let mut text = format!("Most complex functions (top {}):\n\n", funcs.len());
        for (_, node, complexity) in &funcs {
            text.push_str(&format!(
                "  complexity={complexity}  {}\n",
                format_node_brief(node)
            ));
        }

        ToolResult {
            content: vec![ToolContent::text(text)],
            is_error: None,
        }
    })
}

fn handle_get_stats(state: &SharedState, args: &serde_json::Value) -> ToolResult {
    let repo = args.get("repository").and_then(|v| v.as_str());

    let s = state.lock().unwrap();
    if s.graphs.is_empty() {
        return ToolResult {
            content: vec![ToolContent::text(
                "No repositories indexed. Use index_directory first.".into(),
            )],
            is_error: Some(true),
        };
    }

    let graphs_to_show: Vec<_> = if let Some(r) = repo {
        let target = PathBuf::from(r);
        s.graphs
            .iter()
            .filter(|(k, _)| **k == target || k.ends_with(&target))
            .collect()
    } else {
        s.graphs.iter().collect()
    };

    if graphs_to_show.is_empty() {
        return ToolResult {
            content: vec![ToolContent::text(format!(
                "No indexed repository matching '{}'.",
                repo.unwrap_or("")
            ))],
            is_error: Some(true),
        };
    }

    let mut text = String::new();
    for (path, graph) in graphs_to_show {
        text.push_str(&format!("Repository: {}\n", path.display()));
        text.push_str(&format!("  Nodes: {}\n", graph.node_count()));
        text.push_str(&format!("  Edges: {}\n", graph.edge_count()));
        text.push_str(&format!("  Annotated: {}\n", graph.has_annotations()));

        let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        for idx in graph.graph.node_indices() {
            let label = graph.graph[idx].label();
            *counts.entry(label).or_default() += 1;
        }
        let mut sorted: Vec<_> = counts.into_iter().collect();
        sorted.sort_by_key(|(_, c)| std::cmp::Reverse(*c));
        text.push_str("  By type:\n");
        for (label, count) in &sorted {
            text.push_str(&format!("    {label}: {count}\n"));
        }
        text.push('\n');
    }

    ToolResult {
        content: vec![ToolContent::text(text)],
        is_error: None,
    }
}

fn handle_list_repositories(state: &SharedState) -> ToolResult {
    let s = state.lock().unwrap();
    if s.graphs.is_empty() {
        return ToolResult {
            content: vec![ToolContent::text("No repositories indexed.".into())],
            is_error: None,
        };
    }

    let mut text = format!("Indexed repositories ({}):\n", s.graphs.len());
    for (path, graph) in &s.graphs {
        text.push_str(&format!(
            "  {} ({} nodes, {} edges)\n",
            path.display(),
            graph.node_count(),
            graph.edge_count()
        ));
    }

    ToolResult {
        content: vec![ToolContent::text(text)],
        is_error: None,
    }
}

fn handle_find_similar(state: &SharedState, args: &serde_json::Value) -> ToolResult {
    let kind = args.get("kind").and_then(|v| v.as_str());
    let min_lines = args.get("min_lines").and_then(|v| v.as_u64()).unwrap_or(5) as usize;
    let repo = args.get("repository").and_then(|v| v.as_str());

    with_graph(state, repo, |graph| {
        let groups = graph.find_similar_nodes(kind, min_lines);

        if groups.is_empty() {
            return ToolResult {
                content: vec![ToolContent::text(
                    "No similar code groups found.\nMake sure the graph was indexed with annotate=true."
                        .into(),
                )],
                is_error: None,
            };
        }

        let mut text = format!(
            "Found {} groups of potentially similar/redundant code:\n\n",
            groups.len()
        );

        for (i, group) in groups.iter().enumerate().take(20) {
            text.push_str(&format!("── Group {} ({} nodes) ──\n", i + 1, group.len()));
            for (_, node) in group {
                text.push_str(&format!(
                    "  [{}] {}\n",
                    node.short_label(),
                    format_node_brief(node)
                ));
                if let Some(src) = node.source_snippet() {
                    // Show first 8 lines as preview
                    for line in src.lines().take(8) {
                        text.push_str(&format!("    │ {line}\n"));
                    }
                    let total = src.lines().count();
                    if total > 8 {
                        text.push_str(&format!("    │ ... ({} more lines)\n", total - 8));
                    }
                }
                text.push('\n');
            }
        }

        if groups.len() > 20 {
            text.push_str(&format!("... and {} more groups\n", groups.len() - 20));
        }

        ToolResult {
            content: vec![ToolContent::text(text)],
            is_error: None,
        }
    })
}

fn handle_analyze_redundancy(state: &SharedState, args: &serde_json::Value) -> ToolResult {
    use crate::redundancy::{self, AnalysisConfig, FindingKind, Tier};

    let min_tier = match args
        .get("min_tier")
        .and_then(|v| v.as_str())
        .unwrap_or("low")
    {
        "critical" => Tier::Critical,
        "high" => Tier::High,
        "medium" => Tier::Medium,
        _ => Tier::Low,
    };
    let min_lines = args.get("min_lines").and_then(|v| v.as_u64()).unwrap_or(3) as usize;

    let near_dup = args.get("near_dup_threshold").and_then(|v| v.as_f64());
    let structural = args.get("structural_threshold").and_then(|v| v.as_f64());
    let merge = args.get("merge_threshold").and_then(|v| v.as_f64());
    let skip_checks = args
        .get("skip_checks")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let include_source = args
        .get("include_source")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let limit_per_type = args
        .get("limit_per_type")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(5);
    let limit = args
        .get("limit")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize);
    let category = args
        .get("category")
        .and_then(|v| v.as_str())
        .map(String::from);
    let repo = args.get("repository").and_then(|v| v.as_str());

    with_graph(state, repo, |graph| {
        if !graph.has_annotations() {
            return ToolResult {
                content: vec![ToolContent::text(
                    "Error: Graph was not indexed with annotate=true. \
                     Re-index with annotate=true to enable redundancy analysis."
                        .into(),
                )],
                is_error: Some(true),
            };
        }

        let mut config = AnalysisConfig {
            min_lines,
            skip_checks,
            category,
            ..Default::default()
        };
        if let Some(v) = near_dup {
            config.near_duplicate_threshold = v;
        }
        if let Some(v) = structural {
            config.structural_threshold = v;
        }
        if let Some(v) = merge {
            config.merge_threshold = v;
        }

        let findings = redundancy::analyze(graph, &config);
        let mut filtered: Vec<_> = findings
            .into_iter()
            .filter(|f| f.tier <= min_tier)
            .collect();

        // Randomize findings so that limits don't always return the exact same items
        use rand::seq::SliceRandom;
        let mut rng = rand::rng();
        filtered.shuffle(&mut rng);

        if limit_per_type > 0 {
            let mut counts = std::collections::HashMap::new();
            filtered.retain(|f| {
                let count = counts.entry(std::mem::discriminant(&f.kind)).or_insert(0);
                *count += 1;
                *count <= limit_per_type
            });
        }

        // Restore ordering by tier (Critical first)
        filtered.sort_by_key(|f| f.tier);

        if filtered.is_empty() {
            return ToolResult {
                content: vec![ToolContent::text(
                    "No redundancy findings at the requested tier or above.".into(),
                )],
                is_error: None,
            };
        }

        let mut text = String::new();

        let critical = filtered.iter().filter(|f| f.tier == Tier::Critical).count();
        let high = filtered.iter().filter(|f| f.tier == Tier::High).count();
        let medium = filtered.iter().filter(|f| f.tier == Tier::Medium).count();
        let low = filtered.iter().filter(|f| f.tier == Tier::Low).count();

        text.push_str(&format!(
            "Redundancy Analysis: {} findings ({} critical, {} high, {} medium, {} low)\n\n",
            filtered.len(),
            critical,
            high,
            medium,
            low,
        ));

        let display_filtered = if let Some(l) = limit {
            if filtered.len() > l {
                text.push_str(&format!(
                    "(Showing top {} findings due to limit parameter)\n\n",
                    l
                ));
            }
            filtered.into_iter().take(l).collect::<Vec<_>>()
        } else {
            filtered
        };

        // Emit a compact legend so the LLM knows what each code means.
        // Only include codes that actually appear in this result set.
        {
            use std::collections::BTreeSet;
            let used_tags: BTreeSet<&str> = display_filtered.iter().map(|f| match &f.kind {
                FindingKind::Passthrough { .. } => "P=passthrough",
                FindingKind::NearDuplicate { .. } => "ND=near-dup",
                FindingKind::StructurallySimilar { .. } => "S=similar",
                FindingKind::MergeCandidate { .. } => "M=merge",
                FindingKind::SplitCandidate { .. } => "SP=split",
                FindingKind::OverlappingStructs { .. } => "SO=struct-overlap",
                FindingKind::OverlappingEnums { .. } => "EO=enum-overlap",
                FindingKind::SuggestParameterStruct { .. } => "SS=suggest-struct",
                FindingKind::SuggestEnumDispatch { .. } => "SED=suggest-enum",
                FindingKind::SuggestTraitExtraction { .. } => "STE=suggest-trait",
                FindingKind::SuggestFacade { .. } => "FAC=facade",
                FindingKind::SuggestFactory { .. } => "FTY=factory",
                FindingKind::SuggestBuilder { .. } => "SB=builder",
                FindingKind::SuggestStrategy { .. } => "STR=strategy",
                FindingKind::SuggestTemplateMethod { .. } => "TM=template-method",
                FindingKind::SuggestObserver { .. } => "OBS=observer",
                FindingKind::SuggestDecorator { .. } => "SD=decorator",
                FindingKind::SuggestMediator { .. } => "MED=mediator",
                FindingKind::GodClass { .. } => "GC=god-class",
                FindingKind::CircularDependency { .. } => "CD=circular-dep",
                FindingKind::FeatureEnvy { .. } => "FE=feature-envy",
                FindingKind::ShotgunSurgery { .. } => "SG=shotgun-surgery",
                FindingKind::DetectedSingleton { .. } => "SNG=singleton",
                FindingKind::DetectedAdapter { .. } => "ADP=adapter",
                FindingKind::DetectedProxy { .. } => "PRX=proxy",
                FindingKind::DetectedCommand { .. } => "CMD=command",
                FindingKind::DetectedChainOfResponsibility { .. } => "COR=chain-of-resp",
                FindingKind::DetectedDependencyInjection { .. } => "DI=dep-injection",
                FindingKind::DeadCode { .. } => "DC=dead-code",
                FindingKind::LongParameterList { .. } => "LP=long-params",
                FindingKind::DataClump { .. } => "DK=data-clump",
                FindingKind::MiddleMan { .. } => "MM=middle-man",
                FindingKind::LazyClass { .. } => "LZ=lazy-class",
                FindingKind::RefusedBequest { .. } => "RB=refused-bequest",
                FindingKind::SpeculativeGenerality { .. } => "SPG=speculative-generality",
                FindingKind::InappropriateIntimacy { .. } => "II=inappropriate-intimacy",
                FindingKind::DeepNesting { .. } => "DN=deep-nesting",
                FindingKind::DetectedVisitor { .. } => "VIS=visitor",
                FindingKind::DetectedIterator { .. } => "ITR=iterator",
                FindingKind::DetectedState { .. } => "STA=state",
                FindingKind::DetectedComposite { .. } => "CMP=composite",
                FindingKind::DetectedRepository { .. } => "R=repository",
                FindingKind::DetectedPrototype { .. } => "PRT=prototype",
                FindingKind::HubModule { .. } => "HM=hub-module",
                FindingKind::OrphanModule { .. } => "OM=orphan-module",
                FindingKind::DivergentChange { .. } => "DV=divergent-change",
                FindingKind::ParallelInheritance { .. } => "PI=parallel-inherit",
                FindingKind::PrimitiveObsession { .. } => "PO=primitive-obsession",
                FindingKind::LargeClass { .. } => "LCL=large-class",
                FindingKind::UnstableDependency { .. } => "UD=unstable-dep",
                FindingKind::DetectedFlyweight { .. } => "FLY=flyweight",
                FindingKind::DetectedEventEmitter { .. } => "EE=event-emitter",
                FindingKind::DetectedMemento { .. } => "MEM=memento",
                FindingKind::DetectedFluentBuilder { .. } => "FB=fluent-builder",
                FindingKind::DetectedNullObject { .. } => "NO=null-object",
                FindingKind::InconsistentNaming { .. } => "IN=inconsistent-naming",
                FindingKind::CircularPackageDependency { .. } => "CPD=circular-pkg-dep",
                FindingKind::SuggestSumType { .. } => "SST=sum-type",
                FindingKind::SuggestEnumFromHierarchy { .. } => "HTE=hierarchy-to-enum",
                FindingKind::BooleanBlindness { .. } => "BB=bool-blindness",
                FindingKind::SuggestNewtype { .. } => "SN=newtype",
                FindingKind::SuggestSealedType { .. } => "SEL=sealed-type",
                FindingKind::LargeProductType { .. } => "LPT=large-product-type",
                FindingKind::AnemicDomainModel { .. } => "AM=anemic-model",
                FindingKind::MagicNumber { .. } => "MN=magic-number",
                FindingKind::MutableGlobalState { .. } => "MG=mutable-global",
                FindingKind::EmptyCatch { .. } => "EC=empty-catch",
                FindingKind::CallbackHell { .. } => "CH=callback-hell",
                FindingKind::ApiInconsistency { .. } => "AI=api-inconsistency",
                FindingKind::LackOfCohesion { .. } => "LC=low-cohesion",
                FindingKind::HighCoupling { .. } => "HC=high-coupling",
                FindingKind::ModuleInstability { .. } => "UM=unstable-module",
                FindingKind::HighCognitiveComplexity { .. } => "CC=cognitive-complexity",
                FindingKind::HighRiskFunction { .. } => "HRF=high-risk-fn",
                FindingKind::HighRiskFile { .. } => "HRL=high-risk-file",
                FindingKind::UntestedPublicFunction { .. } => "UP=untested-public",
                FindingKind::LowTestRatio { .. } => "LTR=low-test-ratio",
                FindingKind::IntegrationTestSmell { .. } => "IS=integration-smell",
                FindingKind::HighBlastRadius { .. } => "HBR=high-blast-radius",
                FindingKind::MisplacedFunction { .. } => "MF=misplaced-fn",
                FindingKind::ImplicitModule { .. } => "IM=implicit-module",
                FindingKind::UnstablePublicApi { .. } => "UPA=unstable-api",
                FindingKind::UndocumentedPublicApi { .. } => "UA=undocumented-api",
                FindingKind::LeakyAbstraction { .. } => "LA=leaky-abstraction",
                FindingKind::FfiBoundary { .. } => "FFI=ffi-boundary",
                FindingKind::SubprocessCall { .. } => "SUB=subprocess",
                FindingKind::IpcBoundary { .. } => "IPC=ipc-boundary",
                FindingKind::EnvVarUsage { .. } => "EV=env-var",
                FindingKind::HardcodedEndpoint { .. } => "HE=hardcoded-endpoint",
                FindingKind::FeatureFlag { .. } => "FF=feature-flag",
                FindingKind::ConfigFileUsage { .. } => "CF=config-file",
                FindingKind::VecUsedAsSet { .. } => "VAS=vec-as-set",
                FindingKind::VecUsedAsMap { .. } => "VAM=vec-as-map",
                FindingKind::LinearSearchInLoop { .. } => "LSIL=linear-search-in-loop",
                FindingKind::StringConcatInLoop { .. } => "SCIL=string-concat-in-loop",
                FindingKind::SortedVecForLookup { .. } => "SVL=sorted-vec-lookup",
                FindingKind::NestedLoopLookup { .. } => "NLL=nested-loop-lookup",
                FindingKind::HashMapWithSequentialKeys { .. } => "HSK=hashmap-seq-keys",
                FindingKind::ExcessiveCollectIterate { .. } => "CI=collect-iterate",
                FindingKind::UnusedImport { .. } => "UI=unused-import",
                FindingKind::InconsistentErrorHandling { .. } => "IEH=inconsistent-error",
                FindingKind::TechDebtComment { .. } => "TD=tech-debt",
                FindingKind::CloneInLoop { .. } => "CIL=clone-in-loop",
                FindingKind::RedundantCollectIterate { .. } => "RCI=redundant-collect",
                FindingKind::RepeatedMapLookup { .. } => "RML=repeated-lookup",
                FindingKind::VecNoPresize { .. } => "VNP=vec-no-presize",
                FindingKind::SortThenFind { .. } => "STF=sort-then-find",
                FindingKind::ListConcatInLoop { .. } => "LCO=list-concat-loop",
                FindingKind::UnboundedRecursion { .. } => "URB=unbounded-recursion",
                FindingKind::SuggestVectorize { .. } => "VEC=vectorize",
                FindingKind::SuggestPolars { .. } => "POL=suggest-polars",
                FindingKind::RegexRecompileInLoop { .. } => "RRC=regex-recompile",
                FindingKind::MemoizationCandidate { .. } => "MCM=memoize-candidate",
                FindingKind::ExceptionForControlFlow { .. } => "EFC=exception-control-flow",
                FindingKind::NPlusOneQuery { .. } => "N1Q=n-plus-one-query",
                FindingKind::SyncAsyncConflict { .. } => "SAC=sync-async-conflict",
                FindingKind::RepeatedFormatInLoop { .. } => "RFI=repeated-format-loop",
                FindingKind::SleepInLoop { .. } => "SLA=sleep-in-loop",
                FindingKind::GeneratorOverList { .. } => "GEN=generator-over-list",
                FindingKind::UnnecessaryChain { .. } => "UCH=unnecessary-chain",
                FindingKind::LargeListIn { .. } => "LLI=large-list-in",
                FindingKind::DictKeysIter { .. } => "DLK=dict-keys-iter",
                FindingKind::UnclosedResource { .. } => "UCM=unclosed-resource",
                FindingKind::EnumerateVsRangeLen { .. } => "ELV=enumerate-vs-range-len",
                FindingKind::YieldFrom { .. } => "YLD=yield-from",
                FindingKind::AppendInLoopExtend { .. } => "APD=append-loop-extend",
                FindingKind::DoubleWithStatement { .. } => "DWS=double-with",
                FindingKind::ImportInFunction { .. } => "IIF=import-in-function",
                FindingKind::ConstantCondition { .. } => "CST=constant-condition",
                FindingKind::RedundantNegation { .. } => "RNE=redundant-negation",
                FindingKind::DefaultDictPattern { .. } => "DFC=default-dict-pattern",
                FindingKind::EmptyStringCheck { .. } => "ESE=empty-string-check",
            }).collect();
            text.push_str("Tiers: C=critical H=high M=medium L=low\nCodes: ");
            text.push_str(&used_tags.into_iter().collect::<Vec<_>>().join(" "));
            text.push_str("\n\n");
        }

        for finding in &display_filtered {
            let tag = match &finding.kind {
                FindingKind::Passthrough { .. } => "P",
                FindingKind::NearDuplicate { .. } => "ND",
                FindingKind::StructurallySimilar { .. } => "S",
                FindingKind::MergeCandidate { .. } => "M",
                FindingKind::SplitCandidate { .. } => "SP",
                FindingKind::OverlappingStructs { .. } => "SO",
                FindingKind::OverlappingEnums { .. } => "EO",
                FindingKind::SuggestParameterStruct { .. } => "SS",
                FindingKind::SuggestEnumDispatch { .. } => "SED",
                FindingKind::SuggestTraitExtraction { .. } => "STE",
                FindingKind::SuggestFacade { .. } => "FAC",
                FindingKind::SuggestFactory { .. } => "FTY",
                FindingKind::SuggestBuilder { .. } => "SB",
                FindingKind::SuggestStrategy { .. } => "STR",
                FindingKind::SuggestTemplateMethod { .. } => "TM",
                FindingKind::SuggestObserver { .. } => "OBS",
                FindingKind::SuggestDecorator { .. } => "SD",
                FindingKind::SuggestMediator { .. } => "MED",
                FindingKind::GodClass { .. } => "GC",
                FindingKind::CircularDependency { .. } => "CD",
                FindingKind::FeatureEnvy { .. } => "FE",
                FindingKind::ShotgunSurgery { .. } => "SG",
                FindingKind::DetectedSingleton { .. } => "SNG",
                FindingKind::DetectedAdapter { .. } => "ADP",
                FindingKind::DetectedProxy { .. } => "PRX",
                FindingKind::DetectedCommand { .. } => "CMD",
                FindingKind::DetectedChainOfResponsibility { .. } => "COR",
                FindingKind::DetectedDependencyInjection { .. } => "DI",
                FindingKind::DeadCode { .. } => "DC",
                FindingKind::LongParameterList { .. } => "LP",
                FindingKind::DataClump { .. } => "DK",
                FindingKind::MiddleMan { .. } => "MM",
                FindingKind::LazyClass { .. } => "LZ",
                FindingKind::RefusedBequest { .. } => "RB",
                FindingKind::SpeculativeGenerality { .. } => "SPG",
                FindingKind::InappropriateIntimacy { .. } => "II",
                FindingKind::DeepNesting { .. } => "DN",
                FindingKind::DetectedVisitor { .. } => "VIS",
                FindingKind::DetectedIterator { .. } => "ITR",
                FindingKind::DetectedState { .. } => "STA",
                FindingKind::DetectedComposite { .. } => "CMP",
                FindingKind::DetectedRepository { .. } => "R",
                FindingKind::DetectedPrototype { .. } => "PRT",
                FindingKind::HubModule { .. } => "HM",
                FindingKind::OrphanModule { .. } => "OM",
                FindingKind::DivergentChange { .. } => "DV",
                FindingKind::ParallelInheritance { .. } => "PI",
                FindingKind::PrimitiveObsession { .. } => "PO",
                FindingKind::LargeClass { .. } => "LCL",
                FindingKind::UnstableDependency { .. } => "UD",
                FindingKind::DetectedFlyweight { .. } => "FLY",
                FindingKind::DetectedEventEmitter { .. } => "EE",
                FindingKind::DetectedMemento { .. } => "MEM",
                FindingKind::DetectedFluentBuilder { .. } => "FB",
                FindingKind::DetectedNullObject { .. } => "NO",
                FindingKind::InconsistentNaming { .. } => "IN",
                FindingKind::CircularPackageDependency { .. } => "CPD",
                FindingKind::SuggestSumType { .. } => "SST",
                FindingKind::SuggestEnumFromHierarchy { .. } => "HTE",
                FindingKind::BooleanBlindness { .. } => "BB",
                FindingKind::SuggestNewtype { .. } => "SN",
                FindingKind::SuggestSealedType { .. } => "SEL",
                FindingKind::LargeProductType { .. } => "LPT",
                FindingKind::AnemicDomainModel { .. } => "AM",
                FindingKind::MagicNumber { .. } => "MN",
                FindingKind::MutableGlobalState { .. } => "MG",
                FindingKind::EmptyCatch { .. } => "EC",
                FindingKind::CallbackHell { .. } => "CH",
                FindingKind::ApiInconsistency { .. } => "AI",
                FindingKind::LackOfCohesion { .. } => "LC",
                FindingKind::HighCoupling { .. } => "HC",
                FindingKind::ModuleInstability { .. } => "UM",
                FindingKind::HighCognitiveComplexity { .. } => "CC",
                FindingKind::HighRiskFunction { .. } => "HRF",
                FindingKind::HighRiskFile { .. } => "HRL",
                FindingKind::UntestedPublicFunction { .. } => "UP",
                FindingKind::LowTestRatio { .. } => "LTR",
                FindingKind::IntegrationTestSmell { .. } => "IS",
                FindingKind::HighBlastRadius { .. } => "HBR",
                FindingKind::MisplacedFunction { .. } => "MF",
                FindingKind::ImplicitModule { .. } => "IM",
                FindingKind::UnstablePublicApi { .. } => "UPA",
                FindingKind::UndocumentedPublicApi { .. } => "UA",
                FindingKind::LeakyAbstraction { .. } => "LA",
                FindingKind::FfiBoundary { .. } => "FFI",
                FindingKind::SubprocessCall { .. } => "SUB",
                FindingKind::IpcBoundary { .. } => "IPC",
                FindingKind::EnvVarUsage { .. } => "EV",
                FindingKind::HardcodedEndpoint { .. } => "HE",
                FindingKind::FeatureFlag { .. } => "FF",
                FindingKind::ConfigFileUsage { .. } => "CF",
                FindingKind::VecUsedAsSet { .. } => "VAS",
                FindingKind::VecUsedAsMap { .. } => "VAM",
                FindingKind::LinearSearchInLoop { .. } => "LSIL",
                FindingKind::StringConcatInLoop { .. } => "SCIL",
                FindingKind::SortedVecForLookup { .. } => "SVL",
                FindingKind::NestedLoopLookup { .. } => "NLL",
                FindingKind::HashMapWithSequentialKeys { .. } => "HSK",
                FindingKind::ExcessiveCollectIterate { .. } => "CI",
                FindingKind::UnusedImport { .. } => "UI",
                FindingKind::InconsistentErrorHandling { .. } => "IEH",
                FindingKind::TechDebtComment { .. } => "TD",
                FindingKind::CloneInLoop { .. } => "CIL",
                FindingKind::RedundantCollectIterate { .. } => "RCI",
                FindingKind::RepeatedMapLookup { .. } => "RML",
                FindingKind::VecNoPresize { .. } => "VNP",
                FindingKind::SortThenFind { .. } => "STF",
                FindingKind::ListConcatInLoop { .. } => "LCO",
                FindingKind::UnboundedRecursion { .. } => "URB",
                FindingKind::SuggestVectorize { .. } => "VEC",
                FindingKind::SuggestPolars { .. } => "POL",
                FindingKind::RegexRecompileInLoop { .. } => "RRC",
                FindingKind::MemoizationCandidate { .. } => "MCM",
                FindingKind::ExceptionForControlFlow { .. } => "EFC",
                FindingKind::NPlusOneQuery { .. } => "N1Q",
                FindingKind::SyncAsyncConflict { .. } => "SAC",
                FindingKind::RepeatedFormatInLoop { .. } => "RFI",
                FindingKind::SleepInLoop { .. } => "SLA",
                FindingKind::GeneratorOverList { .. } => "GEN",
                FindingKind::UnnecessaryChain { .. } => "UCH",
                FindingKind::LargeListIn { .. } => "LLI",
                FindingKind::DictKeysIter { .. } => "DLK",
                FindingKind::UnclosedResource { .. } => "UCM",
                FindingKind::EnumerateVsRangeLen { .. } => "ELV",
                FindingKind::YieldFrom { .. } => "YLD",
                FindingKind::AppendInLoopExtend { .. } => "APD",
                FindingKind::DoubleWithStatement { .. } => "DWS",
                FindingKind::ImportInFunction { .. } => "IIF",
                FindingKind::ConstantCondition { .. } => "CST",
                FindingKind::RedundantNegation { .. } => "RNE",
                FindingKind::DefaultDictPattern { .. } => "DFC",
                FindingKind::EmptyStringCheck { .. } => "ESE",
            };

            let tier_flag = match finding.tier {
                Tier::Critical => "C",
                Tier::High => "H",
                Tier::Medium => "M",
                Tier::Low => "L",
            };
            text.push_str(&format!(
                "[{tier_flag}][{tag}] {}\n",
                finding.description
            ));

            if include_source {
                for &ni in &finding.node_indices {
                    let node_idx = petgraph::graph::NodeIndex::new(ni);
                    if let Some(node) = graph.get_node(node_idx) {
                        let loc = node.location();
                        let path_str = loc.0;
                        let loc_str = if path_str.is_empty() {
                            "".to_string()
                        } else if loc.1 > 0 {
                            format!(" ({}:{})", path_str, loc.1)
                        } else {
                            format!(" ({})", path_str)
                        };
                        text.push_str(&format!("  {} [{}]{loc_str}\n", node.name(), node.short_label()));

                        if let Some(src) = node.source_snippet() {
                            for line in src.lines().take(5) {
                                text.push_str(&format!("    │ {line}\n"));
                            }
                            let total = src.lines().count();
                            if total > 5 {
                                text.push_str(&format!("    │ ... ({} more lines)\n", total - 5));
                            }
                        }
                    }
                }
            } else {
                let mut nodes_info = Vec::new();
                for &ni in &finding.node_indices {
                    let node_idx = petgraph::graph::NodeIndex::new(ni);
                    if let Some(node) = graph.get_node(node_idx) {
                        let loc = node.location();
                        let path_str = loc.0;
                        let loc_str = if path_str.is_empty() {
                            "".to_string()
                        } else if loc.1 > 0 {
                            format!("({}:{})", path_str, loc.1)
                        } else {
                            format!("({})", path_str)
                        };
                        
                        nodes_info.push(format!("{}({}){}", node.name(), node.short_label(), loc_str));
                    }
                }
                if !nodes_info.is_empty() {
                    text.push_str(&format!("  └─ {}\n", nodes_info.join(", ")));
                }
            }
            text.push('\n');
        }

        ToolResult {
            content: vec![ToolContent::text(text)],
            is_error: None,
        }
    })
}

fn handle_save_graph(state: &SharedState, args: &serde_json::Value) -> ToolResult {
    let save_path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => std::path::PathBuf::from(p),
        None => {
            return ToolResult {
                content: vec![ToolContent::text("Missing required parameter: path".into())],
                is_error: Some(true),
            }
        }
    };

    with_any_graph(state, |graph| match graph.save(&save_path) {
        Ok(()) => ToolResult {
            content: vec![ToolContent::text(format!(
                "Graph saved to {}.\nReload next session with load_graph.",
                save_path.display()
            ))],
            is_error: None,
        },
        Err(e) => ToolResult {
            content: vec![ToolContent::text(format!("Failed to save graph: {e}"))],
            is_error: Some(true),
        },
    })
}

fn handle_load_graph(state: &SharedState, args: &serde_json::Value) -> ToolResult {
    let load_path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => std::path::PathBuf::from(p),
        None => {
            return ToolResult {
                content: vec![ToolContent::text("Missing required parameter: path".into())],
                is_error: Some(true),
            }
        }
    };

    match crate::graph::CodeGraph::load(&load_path) {
        Ok(graph) => {
            let node_count = graph.node_count();
            let edge_count = graph.edge_count();
            let annotated = graph.has_annotations();

            // Key the loaded graph by the load path itself (no re-indexing needed).
            let key = load_path.canonicalize().unwrap_or(load_path.clone());
            let mut s = state.lock().unwrap();
            s.graphs.insert(key.clone(), graph);

            ToolResult {
                content: vec![ToolContent::text(format!(
                    "Loaded graph from {}.\nGraph: {} nodes, {} edges{}.",
                    key.display(),
                    node_count,
                    edge_count,
                    if annotated {
                        ", annotated (source snippets available)"
                    } else {
                        ""
                    },
                ))],
                is_error: None,
            }
        }
        Err(e) => ToolResult {
            content: vec![ToolContent::text(format!("Failed to load graph: {e}"))],
            is_error: Some(true),
        },
    }
}

fn handle_get_source(state: &SharedState, args: &serde_json::Value) -> ToolResult {
    let name = match args.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => {
            return ToolResult {
                content: vec![ToolContent::text("Missing required parameter: name".into())],
                is_error: Some(true),
            }
        }
    };
    let kind_filter = args.get("kind").and_then(|v| v.as_str());
    let repo = args.get("repository").and_then(|v| v.as_str());

    with_graph(state, repo, |graph| {
        let results = graph.search_by_name(name);
        let filtered: Vec<_> = results
            .into_iter()
            .filter(|(_, node)| {
                if let Some(kind) = kind_filter {
                    node.label() == kind
                } else {
                    true
                }
            })
            .collect();

        if filtered.is_empty() {
            return ToolResult {
                content: vec![ToolContent::text(format!(
                    "No symbol found matching '{name}'"
                ))],
                is_error: None,
            };
        }

        let mut text = String::new();
        for (_, node) in filtered.iter().take(5) {
            text.push_str(&format_node(node));
            text.push('\n');
            match node.source_snippet() {
                Some(src) => {
                    text.push_str("```\n");
                    text.push_str(src);
                    if !src.ends_with('\n') {
                        text.push('\n');
                    }
                    text.push_str("```\n");
                }
                None => {
                    text.push_str(
                        "  (no source available — re-index with annotate=true to enable)\n",
                    );
                }
            }
            text.push('\n');
        }
        if filtered.len() > 5 {
            text.push_str(&format!("... and {} more matches\n", filtered.len() - 5));
        }

        ToolResult {
            content: vec![ToolContent::text(text)],
            is_error: None,
        }
    })
}

fn handle_get_context_for_symbol(state: &SharedState, args: &serde_json::Value) -> ToolResult {
    let name = match args.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => {
            return ToolResult {
                content: vec![ToolContent::text("Missing required parameter: name".into())],
                is_error: Some(true),
            }
        }
    };
    let kind_filter = args.get("kind").and_then(|v| v.as_str());
    let repo = args.get("repository").and_then(|v| v.as_str());

    with_graph(state, repo, |graph| {
        let results = graph.search_by_name(name);
        let filtered: Vec<_> = results
            .into_iter()
            .filter(|(_, node)| kind_filter.is_none_or(|k| node.label() == k))
            .collect();

        if filtered.is_empty() {
            return ToolResult {
                content: vec![ToolContent::text(format!(
                    "No symbol found matching '{name}'"
                ))],
                is_error: None,
            };
        }

        let (idx, node) = &filtered[0];
        let mut text = format!("Context for {} '{name}':\n\n", node.short_label());

        // ── Source ──────────────────────────────────────────────────────
        text.push_str("── Definition ──\n");
        text.push_str(&format_node(node));
        text.push('\n');
        if let Some(src) = node.source_snippet() {
            text.push_str("```\n");
            text.push_str(src);
            if !src.ends_with('\n') {
                text.push('\n');
            }
            text.push_str("```\n");
        } else {
            text.push_str("  (re-index with annotate=true to include source)\n");
        }
        text.push('\n');

        // ── Callers ──────────────────────────────────────────────────────
        let callers = graph.get_callers_of(*idx);
        text.push_str(&format!("── Callers ({}) ──\n", callers.len()));
        if callers.is_empty() {
            text.push_str("  (none — may be an entry point or dead code)\n");
        } else {
            let list: Vec<_> = callers.iter().take(20).map(|(_, n)| format_node_brief(n)).collect();
            text.push_str(&format!("  {}\n", list.join(", ")));
            if callers.len() > 20 {
                text.push_str(&format!("  ... and {} more\n", callers.len() - 20));
            }
        }
        text.push('\n');

        // ── Callees ──────────────────────────────────────────────────────
        let callees = graph.get_callees_of(*idx);
        text.push_str(&format!("── Calls ({}) ──\n", callees.len()));
        if callees.is_empty() {
            text.push_str("  (none)\n");
        } else {
            let list: Vec<_> = callees.iter().take(20).map(|(_, n)| format_node_brief(n)).collect();
            text.push_str(&format!("  {}\n", list.join(", ")));
            if callees.len() > 20 {
                text.push_str(&format!("  ... and {} more\n", callees.len() - 20));
            }
        }
        text.push('\n');

        // ── Similar nodes ─────────────────────────────────────────────────
        if graph.has_annotations() {
            let groups = graph.find_similar_nodes(Some(node.label()), 3);
            let my_group = groups.iter().find(|g| g.iter().any(|(i, _)| i == idx));
            if let Some(group) = my_group {
                let others: Vec<_> = group.iter().filter(|(i, _)| i != idx).collect();
                if !others.is_empty() {
                    text.push_str(&format!(
                        "── Similar code ({} match(es)) ──\n",
                        others.len()
                    ));
                    let list: Vec<_> = others.iter().take(5).map(|(_, n)| format_node_brief(n)).collect();
                    text.push_str(&format!("  {}\n", list.join(", ")));
                    text.push('\n');
                }
            }
        }

        if filtered.len() > 1 {
            text.push_str(&format!(
                "Note: {} other symbols named '{name}' exist — use kind= to narrow down.\n",
                filtered.len() - 1
            ));
        }

        ToolResult {
            content: vec![ToolContent::text(text)],
            is_error: None,
        }
    })
}

fn handle_find_references(state: &SharedState, args: &serde_json::Value) -> ToolResult {
    let name = match args.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => {
            return ToolResult {
                content: vec![ToolContent::text("Missing required parameter: name".into())],
                is_error: Some(true),
            }
        }
    };
    let repo = args.get("repository").and_then(|v| v.as_str());

    with_graph(state, repo, |graph| {
        let results = graph.search_by_name(name);
        if results.is_empty() {
            return ToolResult {
                content: vec![ToolContent::text(format!(
                    "No symbol found matching '{name}'"
                ))],
                is_error: None,
            };
        }

        let (idx, node) = &results[0];
        let mut text = format!("References to {} '{name}':\n\n", node.short_label());

        // CALLS edges (reverse) — who calls this
        let callers = graph.get_callers_of(*idx);
        text.push_str(&format!("── Called by ({}) ──\n", callers.len()));
        let list: Vec<_> = callers.iter().take(30).map(|(_, n)| format_node_brief(n)).collect();
        if !list.is_empty() {
            text.push_str(&format!("  {}\n", list.join(", ")));
        }
        if callers.len() > 30 {
            text.push_str(&format!("  ... and {} more\n", callers.len() - 30));
        }
        text.push('\n');

        // INHERITS edges (reverse) — who inherits from this
        let inheritors = graph
            .incoming_edges(*idx)
            .into_iter()
            .filter(|(_, k)| matches!(k, EdgeKind::Inherits))
            .filter_map(|(src, _)| graph.get_node(src))
            .collect::<Vec<_>>();
        if !inheritors.is_empty() {
            text.push_str(&format!("── Inherited by ({}) ──\n", inheritors.len()));
            let list: Vec<_> = inheritors.iter().map(|n| format_node_brief(n)).collect();
            text.push_str(&format!("  {}\n", list.join(", ")));
            text.push('\n');
        }

        // IMPLEMENTS edges (reverse) — who implements this
        let implementors = graph.get_implementors(*idx);
        if !implementors.is_empty() {
            text.push_str(&format!("── Implemented by ({}) ──\n", implementors.len()));
            let list: Vec<_> = implementors.iter().map(|(_, n)| format_node_brief(n)).collect();
            text.push_str(&format!("  {}\n", list.join(", ")));
            text.push('\n');
        }

        // IMPORTS edges (reverse) — which files import this symbol
        let importers = graph
            .incoming_edges(*idx)
            .into_iter()
            .filter(|(_, k)| matches!(k, EdgeKind::Imports { .. }))
            .filter_map(|(src, _)| graph.get_node(src))
            .collect::<Vec<_>>();
        if !importers.is_empty() {
            text.push_str(&format!("── Imported by ({}) ──\n", importers.len()));
            let list: Vec<_> = importers.iter().map(|n| format_node_brief(n)).collect();
            text.push_str(&format!("  {}\n", list.join(", ")));
            text.push('\n');
        }

        // TESTS edges (reverse) — test functions that test this
        let testers = graph
            .incoming_edges(*idx)
            .into_iter()
            .filter(|(_, k)| matches!(k, EdgeKind::Tests))
            .filter_map(|(src, _)| graph.get_node(src))
            .collect::<Vec<_>>();
        if !testers.is_empty() {
            text.push_str(&format!("── Tested by ({}) ──\n", testers.len()));
            let list: Vec<_> = testers.iter().map(|n| format_node_brief(n)).collect();
            text.push_str(&format!("  {}\n", list.join(", ")));
            text.push('\n');
        }

        let total =
            callers.len() + inheritors.len() + implementors.len() + importers.len() + testers.len();
        if total == 0 {
            text.push_str("No references found — symbol may be unused or an entry point.\n");
        }

        if results.len() > 1 {
            text.push_str(&format!(
                "\nNote: {} other symbols named '{name}' exist. Showing references for the first match ({}).\n",
                results.len() - 1,
                format_node_brief(node),
            ));
        }

        ToolResult {
            content: vec![ToolContent::text(text)],
            is_error: None,
        }
    })
}

fn handle_get_module_overview(state: &SharedState, args: &serde_json::Value) -> ToolResult {
    let dir_path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => {
            return ToolResult {
                content: vec![ToolContent::text("Missing required parameter: path".into())],
                is_error: Some(true),
            }
        }
    };
    let repo = args.get("repository").and_then(|v| v.as_str());

    with_graph(state, repo, |graph| {
        let needle = std::path::Path::new(dir_path);

        // Collect all File nodes whose path contains the needle directory.
        let mut files: Vec<(petgraph::graph::NodeIndex, &GraphNode)> = graph
            .graph
            .node_indices()
            .filter_map(|idx| {
                let node = &graph.graph[idx];
                if let GraphNode::File(f) = node {
                    if f.path.ends_with(needle)
                        || f.path.ancestors().any(|a| a.ends_with(needle))
                        || needle.ends_with(&f.path)
                        || needle.ancestors().any(|a| a.ends_with(&f.path))
                        || f.path.to_string_lossy().contains(dir_path)
                    {
                        return Some((idx, node));
                    }
                }
                None
            })
            .collect();

        if files.is_empty() {
            return ToolResult {
                content: vec![ToolContent::text(format!(
                    "No files found under '{dir_path}'. \
                     Check the path and make sure the directory is indexed."
                ))],
                is_error: None,
            };
        }

        // Sort files by path for consistent output.
        files.sort_by_key(|(_, n)| n.name().to_string());

        let file_paths: std::collections::HashSet<_> = files
            .iter()
            .filter_map(|(_, n)| {
                if let GraphNode::File(f) = n {
                    Some(f.path.clone())
                } else {
                    None
                }
            })
            .collect();

        let mut text = format!("Module overview: {} ({} files)\n\n", dir_path, files.len());

        // Per-file summary
        text.push_str("── Files ──\n");
        let mut total_lines = 0usize;
        let mut total_public = 0usize;
        for (_, node) in &files {
            if let GraphNode::File(f) = node {
                total_lines += f.total_lines;
                total_public += f.public_count;
                text.push_str(&format!(
                    "  {:40} {:4} lines  pub={:3} priv={:3}{}",
                    f.relative_path,
                    f.total_lines,
                    f.public_count,
                    f.private_count,
                    if f.is_test_file { "  [test]" } else { "" },
                ));
                text.push('\n');
            }
        }
        text.push_str(&format!(
            "  Total: {} lines, {} public symbols\n\n",
            total_lines, total_public
        ));

        // Cross-file call relationships within the module
        let mut internal_calls: Vec<(String, String)> = Vec::new();
        let mut external_deps: std::collections::HashSet<String> = std::collections::HashSet::new();

        for (file_idx, node) in &files {
            if let GraphNode::File(_) = node {
                // Walk all functions/methods in this file
                for (child_idx, _) in graph.get_children(*file_idx) {
                    for (callee_idx, _) in graph.get_callees_of(child_idx) {
                        // Find which file the callee belongs to
                        let callee_file = graph
                            .incoming_edges(callee_idx)
                            .into_iter()
                            .find(|(_, k)| matches!(k, EdgeKind::Contains))
                            .and_then(|(src, _)| graph.get_node(src));

                        if let Some(GraphNode::File(cf_data)) = callee_file {
                            if file_paths.contains(&cf_data.path) {
                                let caller_name = node.name().to_string();
                                let callee_name = cf_data.name.clone();
                                if caller_name != callee_name {
                                    let pair = (caller_name, callee_name);
                                    if !internal_calls.contains(&pair) {
                                        internal_calls.push(pair);
                                    }
                                }
                            } else {
                                external_deps.insert(cf_data.name.clone());
                            }
                        }
                    }
                }
            }
        }

        if !internal_calls.is_empty() {
            text.push_str("── Internal dependencies ──\n");
            for (from, to) in &internal_calls {
                text.push_str(&format!("  {from} → {to}\n"));
            }
            text.push('\n');
        }

        if !external_deps.is_empty() {
            let mut sorted_deps: Vec<_> = external_deps.into_iter().collect();
            sorted_deps.sort();
            text.push_str("── External dependencies ──\n");
            for dep in &sorted_deps {
                text.push_str(&format!("  {dep}\n"));
            }
            text.push('\n');
        }

        ToolResult {
            content: vec![ToolContent::text(text)],
            is_error: None,
        }
    })
}

/// Returns true if the cache is newer than all source files in `root` (i.e. nothing has changed).
fn cache_is_fresh(root: &std::path::Path, cache_path: &std::path::Path) -> bool {
    let cache_mtime = match std::fs::metadata(cache_path).and_then(|m| m.modified()) {
        Ok(t) => t,
        Err(_) => return false,
    };
    !any_source_newer(root, cache_path, cache_mtime)
}

fn any_source_newer(
    dir: &std::path::Path,
    cache_path: &std::path::Path,
    cache_mtime: std::time::SystemTime,
) -> bool {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return false,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path == cache_path {
            continue;
        }
        if path.is_dir() {
            // Skip hidden dirs (e.g. .git, node_modules)
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.') || name == "node_modules" || name == "target" {
                continue;
            }
            if any_source_newer(&path, cache_path, cache_mtime) {
                return true;
            }
        } else if is_source_file(&path) {
            if let Ok(mtime) = std::fs::metadata(&path).and_then(|m| m.modified()) {
                if mtime > cache_mtime {
                    return true;
                }
            }
        }
    }
    false
}

/// Returns true if the path has a recognised source file extension.
fn is_source_file(path: &std::path::Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()).unwrap_or(""),
        "rs" | "py"
            | "js"
            | "ts"
            | "tsx"
            | "jsx"
            | "go"
            | "java"
            | "c"
            | "h"
            | "cpp"
            | "hpp"
            | "cc"
            | "cxx"
            | "cs"
            | "rb"
            | "php"
            | "phtml"
    )
}

/// Append `.ast_context_cache.json` to the project's `.gitignore` if it isn't already there.
/// Best-effort — silently does nothing if the file can't be read or written.
fn ensure_gitignore(root: &std::path::Path) {
    let gitignore = root.join(".gitignore");
    const ENTRY: &str = ".ast_context_cache.json";

    let existing = std::fs::read_to_string(&gitignore).unwrap_or_default();
    if existing.lines().any(|l| l.trim() == ENTRY) {
        return;
    }

    let addition = if existing.ends_with('\n') || existing.is_empty() {
        format!("{ENTRY}\n")
    } else {
        format!("\n{ENTRY}\n")
    };

    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&gitignore)
        .and_then(|mut f| {
            use std::io::Write;
            f.write_all(addition.as_bytes())
        });
}

// ── formatting helpers ───────────────────────────────────────────────────

fn format_node(node: &GraphNode) -> String {
    let sl = node.short_label();
    match node {
        GraphNode::Function(f) => {
            format!(
                "  [{sl}] {} ({}:{}–{}, cc={})",
                f.name, f.path.display(), f.span.start_line, f.span.end_line, f.cyclomatic_complexity,
            )
        }
        GraphNode::Class(c) => {
            let bases = if c.bases.is_empty() {
                String::new()
            } else {
                format!(" < {}", c.bases.join(", "))
            };
            format!("  [{sl}] {}{} ({}:{}–{})", c.name, bases, c.path.display(), c.span.start_line, c.span.end_line)
        }
        GraphNode::Struct(s) => {
            format!("  [{sl}] {} ({}:{}–{})", s.name, s.path.display(), s.span.start_line, s.span.end_line)
        }
        GraphNode::Trait(t) => {
            format!("  [{sl}] {} ({}:{}–{})", t.name, t.path.display(), t.span.start_line, t.span.end_line)
        }
        GraphNode::Interface(i) => {
            format!("  [{sl}] {} ({}:{}–{})", i.name, i.path.display(), i.span.start_line, i.span.end_line)
        }
        GraphNode::Enum(e) => {
            format!("  [{sl}] {} [{}] ({}:{}–{})", e.name, e.variants.join(", "), e.path.display(), e.span.start_line, e.span.end_line)
        }
        GraphNode::Variable(v) => {
            format!("  [{sl}] {} ({}:{})", v.name, v.path.display(), v.line_number)
        }
        GraphNode::Module(m) => {
            format!("  [{sl}] {}", m.name)
        }
        GraphNode::File(f) => {
            format!("  [{sl}] {} ({})", f.name, f.path.display())
        }
        _ => format!("  [{sl}] {}", node.name()),
    }
}

fn format_node_brief(node: &GraphNode) -> String {
    let sl = node.short_label();
    match node {
        GraphNode::Function(f) => format!("{}({sl})({}:{})", f.name, f.path.display(), f.span.start_line),
        GraphNode::Class(c) => format!("{}({sl})({}:{})", c.name, c.path.display(), c.span.start_line),
        GraphNode::Struct(s) => format!("{}({sl})({}:{})", s.name, s.path.display(), s.span.start_line),
        GraphNode::Trait(t) => format!("{}({sl})({}:{})", t.name, t.path.display(), t.span.start_line),
        GraphNode::Interface(i) => format!("{}({sl})({}:{})", i.name, i.path.display(), i.span.start_line),
        GraphNode::Enum(e) => format!("{}({sl})({}:{})", e.name, e.path.display(), e.span.start_line),
        GraphNode::Variable(v) => format!("{}({sl})({}:{})", v.name, v.path.display(), v.line_number),
        _ => format!("{}({sl})", node.name()),
    }
}
