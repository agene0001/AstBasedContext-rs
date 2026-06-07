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
use std::collections::HashSet;
use std::path::Path;
use petgraph::graph::NodeIndex;
use crate::types::node::FieldDecl;

/// Shared server state.
pub struct ServerState {
    /// Indexed graphs keyed by root path.
    pub graphs: HashMap<PathBuf, CodeGraph>,

    /// Warm language servers keyed by root path, started lazily on the first
    /// `semantic=true` request and kept alive for the rest of the session so we
    /// never pay rust-analyzer's indexing cost more than once per repo. `None`
    /// caches a failed start so we don't retry the multi-second spawn each call.
    #[cfg(feature = "lsp")]
    providers: HashMap<PathBuf, Option<Arc<crate::analysis::LspProvider>>>,
}

impl ServerState {
    pub fn new() -> Self {
        Self {
            graphs: HashMap::new(),
            #[cfg(feature = "lsp")]
            providers: HashMap::new(),
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
            description: "BROWSE/discover code elements by NAME (functions, classes, structs, enums, \
                constants, fields): pass an identifier or partial identifier (not a phrase or value), \
                or omit query + give kind to LIST all of a kind. Use it to find what exists or what's \
                named. If you already know (even roughly) the symbol you want to READ, skip this and \
                call get_context_for_symbol directly — it also takes a partial name and won't need a \
                find_code first.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "A symbol name or partial name (an identifier, NOT free text or a value). Omit it (with a `kind`) to list ALL symbols of that kind."
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
                }
            }),
        },
        ToolDefinition {
            name: "get_overview".to_string(),
            description: "Overview of a path. Pass a FILE path (e.g. 'src/parser/python.rs') to list \
                its symbols, or a DIRECTORY path (e.g. 'src/parser') for a module summary — files, \
                symbol counts, lines, most complex functions, and cross-file dependencies.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "A file path (lists its symbols) or directory path (module summary)"
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
            name: "get_repo_map".to_string(),
            description: "Map the whole codebase: the most important symbols (PageRank-ranked by \
                call/inherit/implement edges) as signature skeletons grouped by file, up to a token \
                budget. Use ONLY for broad 'what is this codebase / where do I start / what are the \
                core pieces' questions. Do NOT use it to find or understand a specific symbol (use \
                find_code or get_context_for_symbol) or to inspect one file (use get_overview) — for \
                anything narrower than the whole repo it is wasted tokens.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "budget": {
                        "type": "integer",
                        "description": "Approx token budget for the map (default 1500)"
                    },
                    "repository": {
                        "type": "string",
                        "description": "Path of the indexed repository to query (optional)"
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
            name: "analyze_redundancy".to_string(),
            description: "Tiered redundancy + code-health report: passthrough wrappers, near/structural \
                duplicates, merge/split candidates, dead code, and anti-patterns, ranked Critical>High>Medium>Low. \
                For dead code pass category='anti_patterns'. Scans the WHOLE repo by default — pass \
                path='src/foo.rs' (or a dir) to scope it to one file cheaply. Tags: tiers [C]/[H]/[M]/[L]; \
                types e.g. [PT]=passthrough [ND]=near-dup [DC]=dead-code. Needs annotate=true on index.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Scope findings to this file or directory (e.g. 'src/walker.rs') — avoids the whole-repo report"
                    },
                    "category": {
                        "type": "string",
                        "description": "Restrict to one category of findings",
                        "enum": ["redundancy", "struct_enum", "type_suggestions", "design_patterns",
                                 "anti_patterns", "pattern_detection", "structural", "type_system",
                                 "metrics", "risk", "testing", "blast_radius", "api_surface",
                                 "cross_language", "config_detection", "data_structures",
                                 "code_quality", "optimization", "memory_layout"]
                    },
                    "min_tier": {
                        "type": "string",
                        "description": "Lowest tier to report (default low)",
                        "enum": ["critical", "high", "medium", "low"]
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max findings (default 40; full report ~25k tokens; 0=all)"
                    },
                    "include_source": {
                        "type": "boolean",
                        "description": "Rarely needed: each finding already names its symbol and file:line. Only set true to read the actual duplicated code; much larger output (default false)"
                    },
                    "repository": {
                        "type": "string",
                        "description": "Indexed repo to query (optional)"
                    },
                    "semantic": {
                        "type": "boolean",
                        "description": "Confirm findings with a language server (rust-analyzer) instead of syntactic heuristics — e.g. dead code is verified against real references (resolving macros, fn-pointers, trait dispatch). SLOWER: the server loads/indexes the project (which must build) on first use this session, then stays warm. Rust only; requires rust-analyzer on PATH. If the server can't start, the call returns an error rather than falling back to noisy heuristics. Default false."
                    }
                }
            }),
        },
        ToolDefinition {
            name: "get_context_for_symbol".to_string(),
            description: "One-call deep dive on a named symbol: source (or value/default for a \
                const/field), callers, callees, references (inherits/implements/imports/tests), and \
                similar code — instead of chaining separate lookups. Accepts an EXACT or APPROXIMATE \
                name and resolves the best match (listing any alternatives), so call it DIRECTLY with \
                the name you want — no find_code lookup first. To inspect SEVERAL symbols at once, pass \
                a comma-separated list (e.g. 'CodeGraph, GraphNode, build') — one call instead of many. \
                Resolves a struct field or enum variant to its owning type.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Symbol name (or a struct field / enum variant). Comma-separate several names to inspect them all in one call."
                    },
                    "kind": {
                        "type": "string",
                        "description": "Type filter — only needed to disambiguate when several symbols share the name (the result lists them). Usually omit it.",
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
        "get_overview" => handle_get_overview(state, args),
        "get_repo_map" => handle_get_repo_map(state, args),
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

/// Resolve a `repository` argument to the root key under which a graph is
/// indexed, mirroring [`with_graph`]'s exact-then-suffix matching. Used to key
/// the language-server cache by the same root the graph lives under.
#[cfg(feature = "lsp")]
fn resolve_root_key(s: &ServerState, repository: Option<&str>) -> Option<PathBuf> {
    if s.graphs.is_empty() {
        return None;
    }
    match repository {
        Some(repo) => {
            let target = PathBuf::from(repo);
            if s.graphs.contains_key(&target) {
                return Some(target);
            }
            s.graphs.keys().find(|k| k.ends_with(&target)).cloned()
        }
        None => s.graphs.keys().next().cloned(),
    }
}

/// Outcome of resolving the optional language server for a request.
enum Semantic {
    /// `semantic` was not requested — run the AST path.
    Off,
    /// A warm provider is ready.
    #[cfg(feature = "lsp")]
    Ready(Arc<dyn crate::analysis::SemanticProvider>),
    /// `semantic` was requested but no provider could be supplied; carries a
    /// caller-facing explanation. We surface this instead of silently running
    /// the noisy heuristic path — semantic mode is a contract for confirmed
    /// results.
    Unavailable(String),
}

/// Get (or lazily start, once per session) a warm language server for the repo.
/// Resolved *before* the `with_graph` lock so the first call can block on
/// indexing without holding the state mutex across the closure.
fn resolve_semantic(
    state: &SharedState,
    repository: Option<&str>,
    semantic: bool,
) -> Semantic {
    if !semantic {
        return Semantic::Off;
    }
    #[cfg(feature = "lsp")]
    {
        let mut s = state.lock().unwrap();
        let Some(root) = resolve_root_key(&s, repository) else {
            return Semantic::Unavailable(
                "semantic=true requested, but no indexed repository is available to root a \
                 language server. Index a directory first."
                    .into(),
            );
        };
        let started = match s.providers.get(&root) {
            Some(cached) => cached.clone(),
            None => {
                // First request for this repo: start rust-analyzer (blocks on indexing).
                let started = crate::analysis::LspProvider::start(&root).map(Arc::new);
                s.providers.insert(root.clone(), started.clone());
                started
            }
        };
        match started {
            Some(p) => Semantic::Ready(p),
            None => Semantic::Unavailable(format!(
                "semantic=true requested, but rust-analyzer could not start for {}. \
                 Is it on PATH (or AST_CONTEXT_RUST_ANALYZER set)? Does the project build? \
                 Re-run with semantic=false for heuristic results — not returning unconfirmed \
                 heuristic findings.",
                root.display()
            )),
        }
    }
    #[cfg(not(feature = "lsp"))]
    {
        let _ = (state, repository);
        Semantic::Unavailable(
            "semantic=true requested, but this server was built without the `lsp` feature. \
             Re-run with semantic=false for heuristic results."
                .into(),
        )
    }
}

/// True for paths that hold test fixtures, examples, or vendored grammars rather
/// than the project's own production code — used to rank real symbols ahead of
/// look-alikes when a bare name matches several.
fn is_secondary_path(path: &str) -> bool {
    let p = path.to_lowercase();
    // Match a directory *segment* so root-relative paths (e.g. "tests/foo.rs",
    // "examples/bar.rs") are caught, not just nested "/tests/" ones.
    p.split('/').any(|seg| {
        matches!(seg, "tests" | "test" | "examples" | "fixtures" | "grammars")
            || seg.starts_with("test_project")
    }) || p.ends_with("_test.rs")
        || p.contains("fixture")
}

/// Normalize user-supplied `kind` synonyms to the graph's canonical node labels,
/// so a reasonable guess (e.g. kind="Constant") still returns results instead of
/// an empty turn. Consts/statics are stored as Variable; methods as Function.
fn canonical_kind(k: &str) -> &str {
    match k {
        "Constant" | "Const" | "Static" => "Variable",
        "Method" => "Function",
        other => other,
    }
}

fn handle_find_code(state: &SharedState, args: &serde_json::Value) -> ToolResult {
    // query is optional: omitting it (with a `kind`) lists all symbols of that kind.
    let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
    let kind_filter = args.get("kind").and_then(|v| v.as_str()).map(canonical_kind);
    // Empty query is allowed only as a "list everything of this kind" browse —
    // otherwise it's meaningless (and would match every field).
    if query.trim().is_empty() && kind_filter.is_none() {
        return ToolResult {
            content: vec![ToolContent::text(
                "Provide a search query (a name or partial name), or a `kind` to list all symbols of that type.".into(),
            )],
            is_error: Some(true),
        };
    }
    let repo = args.get("repository").and_then(|v| v.as_str());

    with_graph(state, repo, |graph| {
        let q = query.to_lowercase();
        // List mode (empty query + kind): enumerate all nodes of that kind.
        let filtered: Vec<_> = if q.is_empty() {
            graph
                .nodes_by_label(kind_filter.unwrap_or(""))
                .into_iter()
                .take(50)
                .collect()
        } else {
            graph
                .search_by_name(query)
                .into_iter()
                .filter(|(_, node)| kind_filter.is_none_or(|k| node.label() == k))
                .take(50)
                .collect()
        };
        let seen: HashSet<_> = filtered.iter().map(|(idx, _)| *idx).collect();

        // Struct/class field names and enum variant names aren't node names, so a
        // plain name search misses them. Match them too and return the owning type.
        let field_str = |f: &FieldDecl| {
            let ty = f.type_annotation.as_deref().map(|t| format!(": {t}")).unwrap_or_default();
            let dv = f.default_value.as_deref().map(|v| format!(" = {v}")).unwrap_or_default();
            format!("field {}{ty}{dv}", f.name)
        };
        // Only match fields/variants for queries of real length — a 0–1 char
        // query would otherwise match every field of every type.
        let mut member_hits: Vec<(String, String)> = Vec::new();
        for idx in graph.graph.node_indices() {
            if q.len() < 2 || seen.contains(&idx) {
                continue;
            }
            let node = &graph.graph[idx];
            if kind_filter.is_some_and(|k| node.label() != k) {
                continue;
            }
            let matched = match node {
                GraphNode::Struct(s) => s.fields.iter().find(|f| f.name.to_lowercase().contains(&q)).map(field_str),
                GraphNode::Class(c) => c.fields.iter().find(|f| f.name.to_lowercase().contains(&q)).map(field_str),
                GraphNode::Enum(e) => e
                    .variants
                    .iter()
                    .find(|v| v.to_lowercase().contains(&q))
                    .map(|v| format!("variant {v}")),
                _ => None,
            };
            if let Some(m) = matched {
                member_hits.push((format_node(node), m));
                if member_hits.len() >= 50 {
                    break;
                }
            }
        }

        if filtered.is_empty() && member_hits.is_empty() {
            // Steer the model in THIS turn instead of forcing a retry: split the
            // query into word parts and surface the closest-named symbols.
            let mut sugg: Vec<String> = Vec::new();
            let mut seen_s = HashSet::new();
            // Longer word-parts are usually the rarer, more specific ones (e.g.
            // "version" beats "graph") — match them first so the best hint leads.
            let mut parts: Vec<&str> =
                query.split(|c: char| !c.is_alphanumeric()).filter(|p| p.len() >= 3).collect();
            parts.sort_by_key(|p| std::cmp::Reverse(p.len()));
            for part in parts {
                for (idx, node) in graph.search_by_name(part) {
                    if kind_filter.is_none_or(|k| node.label() == k) && seen_s.insert(idx) {
                        sugg.push(format_node_brief(node));
                        if sugg.len() >= 5 {
                            break;
                        }
                    }
                }
                if sugg.len() >= 5 {
                    break;
                }
            }
            let msg = if sugg.is_empty() {
                format!("No results found for '{query}'")
            } else {
                format!("No exact match for '{query}'. Closest symbols:\n\n  {}", sugg.join("\n  "))
            };
            return ToolResult {
                content: vec![ToolContent::text(msg)],
                is_error: None,
            };
        }

        let mut text = String::new();
        if !filtered.is_empty() {
            if q.is_empty() {
                text.push_str(&format!(
                    "All {} (showing {}):\n\n",
                    kind_filter.unwrap_or("symbols"),
                    filtered.len()
                ));
            } else {
                text.push_str(&format!("Found {} results for '{query}':\n\n", filtered.len()));
            }
            for (_, node) in &filtered {
                // Compact one-liner when listing a whole kind; full node otherwise.
                if q.is_empty() {
                    text.push_str(&format!("  {}\n", format_node_brief(node)));
                } else {
                    text.push_str(&format_node(node));
                    text.push('\n');
                }
            }
        }
        if !member_hits.is_empty() {
            text.push_str(&format!(
                "\n{} type(s) with a matching field/variant for '{query}':\n\n",
                member_hits.len()
            ));
            for (owner, member) in &member_hits {
                text.push_str(&format!("{}  ⟶ {member}\n", owner.trim_start()));
            }
        }

        ToolResult {
            content: vec![ToolContent::text(text)],
            is_error: None,
        }
    })
}

/// Unified overview: a file path (has an extension) → its symbols; a directory
/// path → the module overview. Folds get_file_summary + get_module_overview.
fn handle_get_overview(state: &SharedState, args: &serde_json::Value) -> ToolResult {
    let is_file = args
        .get("path")
        .and_then(|v| v.as_str())
        .is_some_and(|p| Path::new(p).extension().is_some());
    if is_file {
        handle_get_file_summary(state, args)
    } else {
        handle_get_module_overview(state, args)
    }
}

/// Rank nodes by importance via PageRank over reference-like edges (Calls /
/// Inherits / Implements): rank flows from a user to the thing it uses, so
/// heavily-called functions and widely-implemented traits accumulate weight.
/// This is the ranking behind the repo map (à la Aider's PageRank repo map).
fn pagerank(graph: &CodeGraph) -> HashMap<NodeIndex, f64> {
    use petgraph::visit::EdgeRef;
    let g = &graph.graph;
    let edge_ok =
        |e: &EdgeKind| matches!(e, EdgeKind::Calls { .. } | EdgeKind::Inherits | EdgeKind::Implements);
    let nodes: Vec<_> = g.node_indices().collect();
    let n = nodes.len().max(1) as f64;
    let mut rank: HashMap<_, f64> =
        nodes.iter().map(|&i| (i, 1.0 / n)).collect();
    let out_deg: HashMap<_, usize> = nodes
        .iter()
        .map(|&i| (i, g.edges(i).filter(|e| edge_ok(e.weight())).count()))
        .collect();

    const DAMPING: f64 = 0.85;
    for _ in 0..20 {
        let mut next: HashMap<_, f64> =
            nodes.iter().map(|&i| (i, (1.0 - DAMPING) / n)).collect();
        for &i in &nodes {
            let d = out_deg[&i];
            if d == 0 {
                continue;
            }
            let share = DAMPING * rank[&i] / d as f64;
            for e in g.edges(i).filter(|e| edge_ok(e.weight())) {
                *next.get_mut(&e.target()).unwrap() += share;
            }
        }
        rank = next;
    }
    rank
}

/// Flagship token-saver: a graph-ranked "repo map" — the most important symbols
/// across the whole codebase, rendered as signature skeletons (bodies elided) up
/// to a token budget. Lets an agent grasp a codebase's shape in ~1-2k tokens
/// instead of reading files. Modeled on Aider's PageRank repo map.
fn handle_get_repo_map(state: &SharedState, args: &serde_json::Value) -> ToolResult {
    let budget_tokens = args.get("budget").and_then(|v| v.as_u64()).unwrap_or(1500) as usize;
    let repo = args.get("repository").and_then(|v| v.as_str());

    with_graph(state, repo, |graph| {
        let rank = pagerank(graph);
        // Definition nodes only, and only the project's own code (skip fixtures).
        let mut ranked: Vec<(&GraphNode, f64)> = graph
            .graph
            .node_indices()
            .filter_map(|i| {
                let node = &graph.graph[i];
                let is_def = matches!(
                    node,
                    GraphNode::Function(_)
                        | GraphNode::Class(_)
                        | GraphNode::Struct(_)
                        | GraphNode::Trait(_)
                        | GraphNode::Interface(_)
                        | GraphNode::Enum(_)
                );
                if is_def && !is_secondary_path(&node.location().0) {
                    Some((node, rank.get(&i).copied().unwrap_or(0.0)))
                } else {
                    None
                }
            })
            .collect();

        if ranked.is_empty() {
            return ToolResult {
                content: vec![ToolContent::text(
                    "No symbols to map — index a repository first (with annotate=true).".into(),
                )],
                is_error: None,
            };
        }

        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Greedily take top-ranked symbols until the char budget (≈ tokens×4) is
        // spent, then regroup by file for a readable map.
        let char_budget = budget_tokens.saturating_mul(4);
        let mut used = 0usize;
        let mut by_file: std::collections::BTreeMap<String, Vec<String>> = Default::default();
        let mut shown = 0usize;
        let total = ranked.len();
        for (node, _) in &ranked {
            let (file, line, _) = node.location();
            let line_str = match node {
                GraphNode::Function(f) => format!("  {}  (:{})", fn_signature(f), f.span.start_line),
                other => format!("  {} {}  (:{line})", other.short_label(), other.name()),
            };
            let header_cost = if by_file.contains_key(&file) { 0 } else { file.len() + 1 };
            if used + line_str.len() + 1 + header_cost > char_budget && shown > 0 {
                break;
            }
            used += line_str.len() + 1 + header_cost;
            by_file.entry(file).or_default().push(line_str);
            shown += 1;
        }

        let mut text = format!(
            "Repo map — {shown} most important symbols of {total} (PageRank-ranked, budget ≈{budget_tokens} tokens).\n\
             Signatures only; use get_overview <file> or get_context_for_symbol <name> to drill in.\n\n"
        );
        for (file, lines) in &by_file {
            text.push_str(file);
            text.push('\n');
            for l in lines {
                text.push_str(l);
                text.push('\n');
            }
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
        let needle = Path::new(file_path);
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
                    // Module-level vars/consts are skeleton; function-local vars
                    // (context = enclosing fn) are body — drop them from the outline.
                    GraphNode::Variable(v) if v.context.is_none() => Some(v.path.as_path()),
                    GraphNode::Variable(_) => None,
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

        let mut counts: HashMap<&str, usize> = HashMap::new();
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
    use crate::analysis::{self, AnalysisConfig, Tier};

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
    let structural_confirm = args.get("structural_confirm").and_then(|v| v.as_f64());
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
    // Cap total findings by default — the full report can be ~25k tokens, which
    // is unusable inside an agent loop. Callers can raise it or pass 0 for all.
    // (Source snippets are capped separately below so a low limit doesn't, via
    // the shuffle, randomly drop the specific finding the caller is after.)
    let limit = match args.get("limit").and_then(|v| v.as_u64()) {
        Some(0) => None,
        Some(v) => Some(v as usize),
        None => Some(40),
    };
    let category = args
        .get("category")
        .and_then(|v| v.as_str())
        .map(String::from);
    let path_filter = args.get("path").and_then(|v| v.as_str());
    let repo = args.get("repository").and_then(|v| v.as_str());
    let semantic = args.get("semantic").and_then(|v| v.as_bool()).unwrap_or(false);

    // Warm (or reuse) a language server before taking the graph lock, so its
    // first-use indexing cost isn't paid while holding the state mutex. If
    // semantic was requested but can't be satisfied, say so plainly rather than
    // fall back to the noisy heuristic path.
    let provider: Option<Arc<dyn analysis::SemanticProvider>> =
        match resolve_semantic(state, repo, semantic) {
        Semantic::Off => None,
        #[cfg(feature = "lsp")]
        Semantic::Ready(p) => Some(p),
        Semantic::Unavailable(msg) => {
            return ToolResult {
                content: vec![ToolContent::text(msg)],
                is_error: Some(true),
            };
        }
    };

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
        if let Some(v) = structural_confirm {
            config.structural_confirm_threshold = v;
        }

        let findings = match provider.as_deref() {
            Some(p) => analysis::analyze_with(graph, &config, p),
            None => analysis::analyze(graph, &config),
        };
        let mut filtered: Vec<_> = findings
            .into_iter()
            .filter(|f| f.tier <= min_tier)
            .collect();

        // Scope to a file/dir if requested: keep only findings involving a node in
        // that path. Lets an agent ask "redundancy in THIS file" cheaply instead of
        // getting the whole-repo report.
        if let Some(pf) = path_filter {
            filtered.retain(|f| {
                f.node_indices.iter().any(|&ni| {
                    graph
                        .get_node(petgraph::graph::NodeIndex::new(ni))
                        .is_some_and(|n| n.location().0.contains(pf))
                })
            });
        }

        // Normalize each finding's member order: several checks collect node
        // indices from a HashSet, whose iteration order Rust randomizes per
        // process. Sorting here makes the rendered "└─" list deterministic for
        // every finding kind in one place. (Passthrough's wrapper-vs-target order
        // doesn't matter here — its body render locates the wrapper by name.)
        for f in &mut filtered {
            f.node_indices.sort_unstable();
        }

        // Order by tier (Critical first), and within a tier keep findings in the
        // project's own code above ones that live entirely in test fixtures /
        // examples — those are usually noise when hunting real redundancy. Runs
        // BEFORE the per-type cap so production findings win the limited slots.
        // Output is DETERMINISTIC: the old rand shuffle existed only to rotate
        // past a flood of false positives (mostly the now-clustered pairwise
        // SuggestParameterStruct findings); with that fixed, stable output is
        // preferable (reproducible, cache-friendly, no re-examining churn).
        let is_secondary_finding = |f: &analysis::Finding| {
            !f.node_indices.is_empty()
                && f.node_indices.iter().all(|&ni| {
                    graph
                        .get_node(NodeIndex::new(ni))
                        .map(|n| is_secondary_path(&n.location().0))
                        .unwrap_or(false)
                })
        };
        // Fully deterministic order: tier, then production-before-fixtures, then a
        // stable tiebreak on the finding's (sorted) node indices. Without the
        // tiebreak, ties fall back to checks' HashMap iteration order, which Rust
        // randomizes per process — making output differ run to run.
        filtered.sort_by_cached_key(|f| {
            let mut idx = f.node_indices.clone();
            idx.sort_unstable();
            // description is the final tiebreak for findings with no node_indices
            // (e.g. data clumps); their descriptions are built deterministically.
            (f.tier, is_secondary_finding(f), idx, f.description.clone())
        });

        if limit_per_type > 0 {
            let mut counts = HashMap::new();
            filtered.retain(|f| {
                let count = counts.entry(std::mem::discriminant(&f.kind)).or_insert(0);
                *count += 1;
                *count <= limit_per_type
            });
        }

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
                    "(Showing top {} of {} findings; pass limit=0 for all, or narrow with \
                     category=… / min_tier=high)\n\n",
                    l,
                    filtered.len()
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
            let used_tags: BTreeSet<&str> = display_filtered.iter().map(|f| f.kind.legend_entry()).collect();
            text.push_str("Tiers: C=critical H=high M=medium L=low\nCodes: ");
            text.push_str(&used_tags.into_iter().collect::<Vec<_>>().join(" "));
            text.push_str("\n\n");
        }

        // Attach source snippets to only the first few findings — descriptions
        // already carry the key info, and full source for dozens of findings is
        // huge. This keeps every finding's description while bounding tokens.
        const SOURCE_FINDING_CAP: usize = 3;
        let mut source_shown = 0usize;
        for finding in &display_filtered {
            let tag = finding.kind.short_code();

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

            if include_source && source_shown < SOURCE_FINDING_CAP {
                source_shown += 1;
                for &ni in &finding.node_indices {
                    let node_idx = NodeIndex::new(ni);
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
                // A passthrough's wrapper body IS the evidence for the claim and
                // is tiny by definition (cc=1, a few lines). Inline it so the model
                // doesn't spend a heavy get_context call just to confirm the
                // delegation it was already told about.
                if let analysis::FindingKind::Passthrough { wrapper_name, .. } = &finding.kind {
                    // Locate the wrapper by name (node_indices is now sorted, so
                    // position is no longer meaningful).
                    if let Some(node) = finding
                        .node_indices
                        .iter()
                        .filter_map(|&ni| graph.get_node(NodeIndex::new(ni)))
                        .find(|n| n.name() == wrapper_name)
                    {
                        if let Some(src) = node.source_snippet() {
                            for line in src.lines().take(4) {
                                text.push_str(&format!("    │ {line}\n"));
                            }
                            if src.lines().count() > 4 {
                                text.push_str("    │ ...\n");
                            }
                        }
                    }
                }

                let mut nodes_info = Vec::new();
                for &ni in &finding.node_indices {
                    let node_idx = NodeIndex::new(ni);
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
        let filtered: Vec<_> = graph
            .search_by_name(name)
            .into_iter()
            .filter(|(_, node)| kind_filter.is_none_or(|k| node.label() == k))
            .collect();

        if !filtered.is_empty() {
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
                    // Constants/variables carry their value in the header line above,
                    // so don't nag about missing source for them.
                    None if !matches!(node, GraphNode::Variable(_)) => {
                        text.push_str(
                            "  (no source available — re-index with annotate=true to enable)\n",
                        );
                    }
                    None => {}
                }
                text.push('\n');
            }
            if filtered.len() > 5 {
                text.push_str(&format!("... and {} more matches\n", filtered.len() - 5));
            }
            return ToolResult {
                content: vec![ToolContent::text(text)],
                is_error: None,
            };
        }

        // No node is named `name` — it may be a struct/enum field or variant.
        let q = name.to_lowercase();
        let fmt_field = |f: &FieldDecl| {
            let ty = f.type_annotation.as_deref().map(|t| format!(": {t}")).unwrap_or_default();
            let dv = f.default_value.as_deref().map(|v| format!(" = {v}")).unwrap_or_default();
            format!("field {}{ty}{dv}", f.name)
        };
        let mut hits = String::new();
        for idx in graph.graph.node_indices() {
            if q.len() < 2 {
                break; // too short — would match every field
            }
            let node = &graph.graph[idx];
            let m = match node {
                GraphNode::Struct(s) => s.fields.iter().find(|f| f.name.to_lowercase().contains(&q)).map(fmt_field),
                GraphNode::Class(c) => c.fields.iter().find(|f| f.name.to_lowercase().contains(&q)).map(fmt_field),
                GraphNode::Enum(e) => e
                    .variants
                    .iter()
                    .find(|v| v.to_lowercase().contains(&q))
                    .map(|v| format!("variant {v}")),
                _ => None,
            };
            if let Some(m) = m {
                let (path, line, _) = node.location();
                hits.push_str(&format!("  [{}] {} ({path}:{line})  ⟶ {m}\n", node.short_label(), node.name()));
            }
        }

        let text = if hits.is_empty() {
            format!("No symbol found matching '{name}'")
        } else {
            format!("'{name}' is a field/variant (not a standalone symbol). Found on:\n\n{hits}")
        };
        ToolResult {
            content: vec![ToolContent::text(text)],
            is_error: None,
        }
    })
}

/// "field name: Type = default" describing a struct/class field.
fn describe_field(f: &FieldDecl) -> String {
    let ty = f.type_annotation.as_deref().map(|t| format!(": {t}")).unwrap_or_default();
    let dv = f.default_value.as_deref().map(|v| format!(" = {v}")).unwrap_or_default();
    format!("field {}{ty}{dv}", f.name)
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

    // Batch: a comma-separated `name` inspects several symbols in ONE call,
    // collapsing the common "drill into A, then B, then C" chain (each its own
    // turn that re-sends the whole transcript) into a single response.
    const MAX_BATCH: usize = 12;
    let names: Vec<&str> = name
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .take(MAX_BATCH)
        .collect();
    if names.is_empty() {
        return ToolResult {
            content: vec![ToolContent::text("Provide a symbol name.".into())],
            is_error: Some(true),
        };
    }

    with_graph(state, repo, |graph| {
        let render_one = |name: &str| -> String {
        let mut filtered: Vec<(NodeIndex, &GraphNode)> = graph
            .search_by_name(name)
            .into_iter()
            .filter(|(_, node)| kind_filter.is_none_or(|k| node.label() == k))
            .collect();
        // search_by_name is substring; for get_context the caller almost always
        // means an exact symbol. If any match the name exactly (case-insensitive),
        // keep only those — so "process" doesn't drag in "preprocess"/"process_file".
        if filtered.iter().any(|(_, n)| n.name().eq_ignore_ascii_case(name)) {
            filtered.retain(|(_, n)| n.name().eq_ignore_ascii_case(name));
        }
        // Rank the best match first so filtered[0] is the one the caller meant —
        // this avoids a get_context→get_context(kind=) retry. Order by: real
        // definitions before import/Module nodes (a `use Foo` creates a Module
        // node named Foo that would otherwise shadow `struct Foo`), then
        // production code before test fixtures/examples.
        filtered.sort_by_key(|(_, n)| (n.label() == "Module", is_secondary_path(&n.location().0)));

        // If `name` is a struct field / enum variant (not a node itself), fall
        // back to the type that owns it — so "what's the default of X" is answered
        // in this single call rather than forcing a separate field lookup.
        let mut prefix = String::new();
        if filtered.is_empty() && name.len() >= 2 {
            let q = name.to_lowercase();
            let owner = graph.graph.node_indices().find_map(|i| {
                let n = &graph.graph[i];
                let desc = match n {
                    GraphNode::Struct(s) => s.fields.iter().find(|f| f.name.to_lowercase().contains(&q)).map(describe_field),
                    GraphNode::Class(c) => c.fields.iter().find(|f| f.name.to_lowercase().contains(&q)).map(describe_field),
                    GraphNode::Enum(e) => e.variants.iter().find(|v| v.to_lowercase().contains(&q)).map(|v| format!("variant {v}")),
                    _ => None,
                };
                desc.map(|d| (i, n, d))
            });
            if let Some((i, n, d)) = owner {
                prefix = format!("'{name}' is a {d} of {} — showing that type:\n\n", n.name());
                filtered.push((i, n));
            }
        }

        if filtered.is_empty() {
            return format!("No symbol found matching '{name}'");
        }

        let (idx, node) = &filtered[0];
        let mut text = prefix;
        text.push_str(&format!("Context for {} '{}':\n\n", node.short_label(), node.name()));

        // ── Source ──────────────────────────────────────────────────────
        text.push_str("── Definition ──\n");
        text.push_str(&format_node(node));
        text.push('\n');
        if let Some(src) = node.source_snippet() {
            // The body is already bounded by the annotate layer (MAX_SNIPPET_BYTES,
            // 4 KB), so no further cap is needed — return it whole.
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

        // ── Other references (inherited/implemented/imported/tested by) ──
        let mut refs: Vec<String> = Vec::new();
        for (src, kind) in graph.incoming_edges(*idx) {
            let label = match kind {
                EdgeKind::Inherits => "inherited by",
                EdgeKind::Implements => "implemented by",
                EdgeKind::Imports { .. } => "imported by",
                EdgeKind::Tests => "tested by",
                _ => continue,
            };
            if let Some(n) = graph.get_node(src) {
                refs.push(format!("{label} {}", format_node_brief(n)));
            }
        }
        if !refs.is_empty() {
            text.push_str(&format!("── Other references ({}) ──\n", refs.len()));
            text.push_str(&format!(
                "  {}\n\n",
                refs.iter().take(20).cloned().collect::<Vec<_>>().join(", ")
            ));
        }

        // ── Similar nodes ─────────────────────────────────────────────────
        // Supplementary, not the core of "what is X" — show the count + top 5.
        if graph.has_annotations() {
            let groups = graph.find_similar_nodes(Some(node.label()), 3);
            let my_group = groups.iter().find(|g| g.iter().any(|(i, _)| i == idx));
            if let Some(group) = my_group {
                let others: Vec<_> = group.iter().filter(|(i, _)| i != idx).collect();
                if !others.is_empty() {
                    text.push_str(&format!("── Similar code ({} match(es)) ──\n", others.len()));
                    let list: Vec<_> = others.iter().take(5).map(|(_, n)| format_node_brief(n)).collect();
                    text.push_str(&format!("  {}\n", list.join(", ")));
                    text.push('\n');
                }
            }
        }

        if filtered.len() > 1 {
            // List the alternatives (with kind + file) so the model can confirm it
            // already has the right one, or pick another in a single follow-up —
            // instead of guessing kind= and re-calling just to see the list.
            text.push_str(&format!(
                "── Other symbols also named '{name}' ({}) ──\n",
                filtered.len() - 1
            ));
            for (_, n) in filtered.iter().skip(1).take(8) {
                text.push_str(&format!("  {}\n", format_node_brief(n)));
            }
            if filtered.len() - 1 > 8 {
                text.push_str(&format!("  ... and {} more\n", filtered.len() - 1 - 8));
            }
            text.push_str("  (full context shown above is the first match; re-call with kind= to pick another)\n");
        }

        text
        };

        let body = names
            .iter()
            .map(|n| render_one(n))
            .collect::<Vec<_>>()
            .join("\n\n────────────────────\n\n");
        ToolResult {
            content: vec![ToolContent::text(body)],
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
        let needle = Path::new(dir_path);

        // Collect all File nodes whose path contains the needle directory.
        let mut files: Vec<(NodeIndex, &GraphNode)> = graph
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

        let file_paths: HashSet<_> = files
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

        // Most complex functions in this module (folded from find_complex_functions).
        let mut complex: Vec<(&str, u32, String, usize)> = Vec::new();
        for (file_idx, _) in &files {
            for (_, child) in graph.get_children(*file_idx) {
                if let GraphNode::Function(f) = child {
                    complex.push((
                        f.name.as_str(),
                        f.cyclomatic_complexity,
                        f.path.display().to_string(),
                        f.span.start_line as usize,
                    ));
                }
            }
        }
        complex.sort_by(|a, b| b.1.cmp(&a.1));
        if !complex.is_empty() {
            text.push_str("── Most complex functions ──\n");
            for (name, cc, path, line) in complex.iter().take(8) {
                text.push_str(&format!("  cc={cc:<3} {name} ({path}:{line})\n"));
            }
            text.push('\n');
        }

        // Cross-file call relationships within the module
        let mut internal_calls: Vec<(String, String)> = Vec::new();
        let mut external_deps: HashSet<String> = HashSet::new();

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
fn cache_is_fresh(root: &Path, cache_path: &Path) -> bool {
    let cache_mtime = match std::fs::metadata(cache_path).and_then(|m| m.modified()) {
        Ok(t) => t,
        Err(_) => return false,
    };
    !any_source_newer(root, cache_path, cache_mtime)
}

fn any_source_newer(
    dir: &Path,
    cache_path: &Path,
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
fn is_source_file(path: &Path) -> bool {
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
fn ensure_gitignore(root: &Path) {
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

/// A compact signature skeleton for a function: `[async] name(arg: type, …) -> ret`.
/// This is the high-value "signatures, bodies elided" primitive — built from data
/// we already parse (args/arg_types/return_type), shown wherever a function is
/// rendered so the outline answers "what's the shape of X" without the body.
fn fn_signature(f: &crate::types::node::FunctionData) -> String {
    let params: Vec<String> = f
        .args
        .iter()
        .enumerate()
        .map(|(i, a)| match f.arg_types.get(i).and_then(|t| t.as_deref()) {
            Some(ty) => format!("{a}: {ty}"),
            None => a.clone(),
        })
        .collect();
    let ret = f.return_type.as_deref().map(|r| format!(" -> {r}")).unwrap_or_default();
    let asyncp = if f.is_async { "async " } else { "" };
    format!("{asyncp}{}({}){ret}", f.name, params.join(", "))
}

fn format_node(node: &GraphNode) -> String {
    let sl = node.short_label();
    match node {
        GraphNode::Function(f) => {
            format!(
                "  [{sl}] {} ({}:{}–{}, cc={})",
                fn_signature(f), f.path.display(), f.span.start_line, f.span.end_line, f.cyclomatic_complexity,
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
            // Cap variant display — some generated enums have hundreds of variants.
            const MAX_VARIANTS: usize = 16;
            let vars = if e.variants.len() > MAX_VARIANTS {
                format!("{}, …(+{} more)", e.variants[..MAX_VARIANTS].join(", "), e.variants.len() - MAX_VARIANTS)
            } else {
                e.variants.join(", ")
            };
            format!("  [{sl}] {} [{}] ({}:{}–{})", e.name, vars, e.path.display(), e.span.start_line, e.span.end_line)
        }
        GraphNode::Variable(v) => {
            let ty = v.type_annotation.as_deref().map(|t| format!(": {t}")).unwrap_or_default();
            // Collapse multi-line values to one line and elide long ones — a const's
            // full array/literal is body, not skeleton (e.g. a 18-element list).
            let val = v
                .value
                .as_deref()
                .map(|x| {
                    let collapsed = x.split_whitespace().collect::<Vec<_>>().join(" ");
                    let shown: String = collapsed.chars().take(60).collect();
                    if collapsed.chars().count() > 60 {
                        format!(" = {shown}…")
                    } else {
                        format!(" = {shown}")
                    }
                })
                .unwrap_or_default();
            format!("  [{sl}] {}{}{} ({}:{})", v.name, ty, val, v.path.display(), v.line_number)
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
