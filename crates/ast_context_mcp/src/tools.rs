//! MCP tool definitions and handlers.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use ast_context_core::graph::CodeGraph;
use ast_context_core::types::node::GraphNode;
use ast_context_core::GraphBuilder;
use serde_json::json;

use crate::protocol::{ToolContent, ToolDefinition, ToolResult};

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
            description: "Index a directory and build its code graph".to_string(),
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
                    }
                },
                "required": ["query"]
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
                    }
                }
            }),
        },
        ToolDefinition {
            name: "get_stats".to_string(),
            description: "Get statistics about the indexed code graph".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {}
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
                    }
                }
            }),
        },
        ToolDefinition {
            name: "analyze_redundancy".to_string(),
            description: "Run tiered redundancy analysis: finds passthrough wrappers, near-duplicates, \
                structural similarity, merge candidates, and split candidates. Returns findings ranked \
                Critical > High > Medium > Low. Requires annotate=true on index.".to_string(),
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
                    }
                }
            }),
        },
    ]
}

/// Dispatch a tool call to its handler.
pub fn handle_tool(
    state: &SharedState,
    tool_name: &str,
    args: &serde_json::Value,
) -> ToolResult {
    match tool_name {
        "index_directory" => handle_index_directory(state, args),
        "find_code" => handle_find_code(state, args),
        "analyze_relationships" => handle_analyze_relationships(state, args),
        "find_dead_code" => handle_find_dead_code(state, args),
        "find_complex_functions" => handle_find_complex_functions(state, args),
        "get_stats" => handle_get_stats(state),
        "list_repositories" => handle_list_repositories(state),
        "find_similar" => handle_find_similar(state, args),
        "analyze_redundancy" => handle_analyze_redundancy(state, args),
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
            content: vec![ToolContent::text(format!("Path does not exist: {path_str}"))],
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

    match GraphBuilder::build_full(&path, annotate, &exclude) {
        Ok(graph) => {
            let node_count = graph.node_count();
            let edge_count = graph.edge_count();
            let canonical = path.canonicalize().unwrap_or(path);
            let mut s = state.lock().unwrap();
            s.graphs.insert(canonical.clone(), graph);

            ToolResult {
                content: vec![ToolContent::text(format!(
                    "Successfully indexed {}.\nGraph: {} nodes, {} edges.",
                    canonical.display(),
                    node_count,
                    edge_count
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

fn with_any_graph<F>(state: &SharedState, f: F) -> ToolResult
where
    F: FnOnce(&CodeGraph) -> ToolResult,
{
    let s = state.lock().unwrap();
    if let Some(graph) = s.graphs.values().next() {
        f(graph)
    } else {
        ToolResult {
            content: vec![ToolContent::text(
                "No repositories indexed. Use index_directory first.".into(),
            )],
            is_error: Some(true),
        }
    }
}

fn handle_find_code(state: &SharedState, args: &serde_json::Value) -> ToolResult {
    let query = match args.get("query").and_then(|v| v.as_str()) {
        Some(q) => q,
        None => {
            return ToolResult {
                content: vec![ToolContent::text("Missing required parameter: query".into())],
                is_error: Some(true),
            }
        }
    };
    let kind_filter = args.get("kind").and_then(|v| v.as_str());

    with_any_graph(state, |graph| {
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
    let max_depth = args
        .get("max_depth")
        .and_then(|v| v.as_u64())
        .unwrap_or(5) as usize;

    with_any_graph(state, |graph| {
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
                content: vec![ToolContent::text(format!("No node found with name '{name}'"))],
                is_error: None,
            };
        }

        let idx = indices[0];
        let mut text = String::new();

        match relationship {
            "callers" => {
                let callers = graph.get_callers_of(idx);
                text.push_str(&format!("Callers of '{name}' ({} found):\n", callers.len()));
                for (_, node) in &callers {
                    text.push_str(&format!("  - {}\n", format_node_brief(node)));
                }
            }
            "callees" => {
                let callees = graph.get_callees_of(idx);
                text.push_str(&format!(
                    "Functions called by '{name}' ({} found):\n",
                    callees.len()
                ));
                for (_, node) in &callees {
                    text.push_str(&format!("  - {}\n", format_node_brief(node)));
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
                for (_, node) in &impls {
                    text.push_str(&format!("  - {}\n", format_node_brief(node)));
                }
            }
            "children" => {
                let children = graph.get_children(idx);
                text.push_str(&format!(
                    "Children of '{name}' ({} found):\n",
                    children.len()
                ));
                for (_, node) in &children {
                    text.push_str(&format!("  - {}\n", format_node_brief(node)));
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
    let limit = args
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(50) as usize;

    with_any_graph(state, |graph| {
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
    let limit = args
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(20) as usize;

    with_any_graph(state, |graph| {
        let funcs = graph.most_complex_functions(limit);
        if funcs.is_empty() {
            return ToolResult {
                content: vec![ToolContent::text("No functions found.".into())],
                is_error: None,
            };
        }

        let mut text = format!("Most complex functions (top {}):\n\n", funcs.len());
        for (_, node, complexity) in &funcs {
            text.push_str(&format!("  complexity={complexity}  {}\n", format_node_brief(node)));
        }

        ToolResult {
            content: vec![ToolContent::text(text)],
            is_error: None,
        }
    })
}

fn handle_get_stats(state: &SharedState) -> ToolResult {
    let s = state.lock().unwrap();
    if s.graphs.is_empty() {
        return ToolResult {
            content: vec![ToolContent::text(
                "No repositories indexed. Use index_directory first.".into(),
            )],
            is_error: Some(true),
        };
    }

    let mut text = String::new();
    for (path, graph) in &s.graphs {
        text.push_str(&format!("Repository: {}\n", path.display()));
        text.push_str(&format!("  Nodes: {}\n", graph.node_count()));
        text.push_str(&format!("  Edges: {}\n", graph.edge_count()));

        // Count by type
        let mut counts: std::collections::HashMap<&str, usize> =
            std::collections::HashMap::new();
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
    let min_lines = args
        .get("min_lines")
        .and_then(|v| v.as_u64())
        .unwrap_or(5) as usize;

    with_any_graph(state, |graph| {
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
                text.push_str(&format!("  [{}] {}\n", node.label(), format_node_brief(node)));
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
    use ast_context_core::redundancy::{self, AnalysisConfig, FindingKind, Tier};

    let min_tier = match args.get("min_tier").and_then(|v| v.as_str()).unwrap_or("low") {
        "critical" => Tier::Critical,
        "high" => Tier::High,
        "medium" => Tier::Medium,
        _ => Tier::Low,
    };
    let min_lines = args
        .get("min_lines")
        .and_then(|v| v.as_u64())
        .unwrap_or(3) as usize;

    with_any_graph(state, |graph| {
        let config = AnalysisConfig {
            min_lines,
            ..Default::default()
        };

        let findings = redundancy::analyze(graph, &config);
        let filtered: Vec<_> = findings.iter().filter(|f| f.tier <= min_tier).collect();

        if filtered.is_empty() {
            return ToolResult {
                content: vec![ToolContent::text(
                    "No redundancy findings. Make sure the graph was indexed with annotate=true."
                        .into(),
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
            filtered.len(), critical, high, medium, low,
        ));

        let mut current_tier = None;
        for finding in &filtered {
            if current_tier != Some(finding.tier) {
                current_tier = Some(finding.tier);
                text.push_str(&format!("══ {} ══\n\n", finding.tier));
            }

            let tag = match &finding.kind {
                FindingKind::Passthrough { .. } => "PASSTHROUGH",
                FindingKind::NearDuplicate { .. } => "NEAR-DUPLICATE",
                FindingKind::StructurallySimilar { .. } => "SIMILAR",
                FindingKind::MergeCandidate { .. } => "MERGE",
                FindingKind::SplitCandidate { .. } => "SPLIT",
                FindingKind::OverlappingStructs { .. } => "STRUCT-OVERLAP",
                FindingKind::OverlappingEnums { .. } => "ENUM-OVERLAP",
                FindingKind::SuggestParameterStruct { .. } => "SUGGEST-STRUCT",
                FindingKind::SuggestEnumDispatch { .. } => "SUGGEST-ENUM",
                FindingKind::SuggestTraitExtraction { .. } => "SUGGEST-TRAIT",
                FindingKind::SuggestFacade { .. } => "SUGGEST-FACADE",
                FindingKind::SuggestFactory { .. } => "SUGGEST-FACTORY",
                FindingKind::SuggestBuilder { .. } => "SUGGEST-BUILDER",
                FindingKind::SuggestStrategy { .. } => "SUGGEST-STRATEGY",
                FindingKind::SuggestTemplateMethod { .. } => "SUGGEST-TEMPLATE",
                FindingKind::SuggestObserver { .. } => "SUGGEST-OBSERVER",
                FindingKind::SuggestDecorator { .. } => "SUGGEST-DECORATOR",
                FindingKind::SuggestMediator { .. } => "SUGGEST-MEDIATOR",
                FindingKind::GodClass { .. } => "GOD-CLASS",
                FindingKind::CircularDependency { .. } => "CIRCULAR-DEP",
                FindingKind::FeatureEnvy { .. } => "FEATURE-ENVY",
                FindingKind::ShotgunSurgery { .. } => "SHOTGUN-SURGERY",
                FindingKind::DetectedSingleton { .. } => "SINGLETON",
                FindingKind::DetectedAdapter { .. } => "ADAPTER",
                FindingKind::DetectedProxy { .. } => "PROXY",
                FindingKind::DetectedCommand { .. } => "COMMAND",
                FindingKind::DetectedChainOfResponsibility { .. } => "CHAIN-OF-RESP",
                FindingKind::DetectedDependencyInjection { .. } => "DI",
                FindingKind::DeadCode { .. } => "DEAD-CODE",
                FindingKind::LongParameterList { .. } => "LONG-PARAMS",
                FindingKind::DataClump { .. } => "DATA-CLUMP",
                FindingKind::MiddleMan { .. } => "MIDDLE-MAN",
                FindingKind::LazyClass { .. } => "LAZY-CLASS",
                FindingKind::RefusedBequest { .. } => "REFUSED-BEQUEST",
                FindingKind::SpeculativeGenerality { .. } => "SPECULATIVE-GENERALITY",
                FindingKind::InappropriateIntimacy { .. } => "INAPPROPRIATE-INTIMACY",
                FindingKind::DeepNesting { .. } => "DEEP-NESTING",
                FindingKind::DetectedVisitor { .. } => "VISITOR",
                FindingKind::DetectedIterator { .. } => "ITERATOR",
                FindingKind::DetectedState { .. } => "STATE",
                FindingKind::DetectedComposite { .. } => "COMPOSITE",
                FindingKind::DetectedRepository { .. } => "REPOSITORY",
                FindingKind::DetectedPrototype { .. } => "PROTOTYPE",
                FindingKind::HubModule { .. } => "HUB-MODULE",
                FindingKind::OrphanModule { .. } => "ORPHAN-MODULE",
                FindingKind::DivergentChange { .. } => "DIVERGENT-CHANGE",
                FindingKind::ParallelInheritance { .. } => "PARALLEL-INHERITANCE",
                FindingKind::PrimitiveObsession { .. } => "PRIMITIVE-OBSESSION",
                FindingKind::LargeClass { .. } => "LARGE-CLASS",
                FindingKind::UnstableDependency { .. } => "UNSTABLE-DEP",
                FindingKind::DetectedFlyweight { .. } => "FLYWEIGHT",
                FindingKind::DetectedEventEmitter { .. } => "EVENT-EMITTER",
                FindingKind::DetectedMemento { .. } => "MEMENTO",
                FindingKind::DetectedFluentBuilder { .. } => "FLUENT-BUILDER",
                FindingKind::DetectedNullObject { .. } => "NULL-OBJECT",
                FindingKind::InconsistentNaming { .. } => "INCONSISTENT-NAMING",
                FindingKind::CircularPackageDependency { .. } => "CIRCULAR-PKG-DEP",
                FindingKind::SuggestSumType { .. } => "SUGGEST-SUM-TYPE",
                FindingKind::SuggestEnumFromHierarchy { .. } => "HIERARCHY-TO-ENUM",
                FindingKind::BooleanBlindness { .. } => "BOOLEAN-BLINDNESS",
                FindingKind::SuggestNewtype { .. } => "SUGGEST-NEWTYPE",
                FindingKind::SuggestSealedType { .. } => "SUGGEST-SEALED",
                FindingKind::LargeProductType { .. } => "LARGE-PRODUCT-TYPE",
                FindingKind::AnemicDomainModel { .. } => "ANEMIC-MODEL",
                FindingKind::MagicNumber { .. } => "MAGIC-NUMBER",
                FindingKind::MutableGlobalState { .. } => "MUTABLE-GLOBAL",
                FindingKind::EmptyCatch { .. } => "EMPTY-CATCH",
                FindingKind::CallbackHell { .. } => "CALLBACK-HELL",
                FindingKind::ApiInconsistency { .. } => "API-INCONSISTENCY",
                FindingKind::LackOfCohesion { .. } => "LOW-COHESION",
                FindingKind::HighCoupling { .. } => "HIGH-COUPLING",
                FindingKind::ModuleInstability { .. } => "UNSTABLE-MODULE",
                FindingKind::HighCognitiveComplexity { .. } => "COGNITIVE-COMPLEXITY",
                FindingKind::HighRiskFunction { .. } => "HIGH-RISK-FUNC",
                FindingKind::HighRiskFile { .. } => "HIGH-RISK-FILE",
                FindingKind::UntestedPublicFunction { .. } => "UNTESTED-PUBLIC",
                FindingKind::LowTestRatio { .. } => "LOW-TEST-RATIO",
                FindingKind::IntegrationTestSmell { .. } => "INTEGRATION-SMELL",
                FindingKind::HighBlastRadius { .. } => "HIGH-BLAST-RADIUS",
                FindingKind::MisplacedFunction { .. } => "MISPLACED-FUNC",
                FindingKind::ImplicitModule { .. } => "IMPLICIT-MODULE",
                FindingKind::UnstablePublicApi { .. } => "UNSTABLE-API",
                FindingKind::UndocumentedPublicApi { .. } => "UNDOCUMENTED-API",
                FindingKind::LeakyAbstraction { .. } => "LEAKY-ABSTRACTION",
                FindingKind::FfiBoundary { .. } => "FFI-BOUNDARY",
                FindingKind::SubprocessCall { .. } => "SUBPROCESS",
                FindingKind::IpcBoundary { .. } => "IPC-BOUNDARY",
                FindingKind::EnvVarUsage { .. } => "ENV-VAR",
                FindingKind::HardcodedEndpoint { .. } => "HARDCODED-ENDPOINT",
                FindingKind::FeatureFlag { .. } => "FEATURE-FLAG",
                FindingKind::ConfigFileUsage { .. } => "CONFIG-FILE",
                FindingKind::VecUsedAsSet { .. } => "VEC-AS-SET",
                FindingKind::VecUsedAsMap { .. } => "VEC-AS-MAP",
                FindingKind::LinearSearchInLoop { .. } => "LINEAR-SEARCH-IN-LOOP",
                FindingKind::StringConcatInLoop { .. } => "STRING-CONCAT-IN-LOOP",
                FindingKind::SortedVecForLookup { .. } => "SORTED-VEC-LOOKUP",
                FindingKind::NestedLoopLookup { .. } => "NESTED-LOOP-LOOKUP",
                FindingKind::HashMapWithSequentialKeys { .. } => "HASHMAP-SEQ-KEYS",
                FindingKind::ExcessiveCollectIterate { .. } => "EXCESSIVE-COLLECT",
            };

            text.push_str(&format!("[{tag}] {}\n", finding.description));

            // Show brief source for involved nodes
            for &ni in &finding.node_indices {
                let node_idx = petgraph::graph::NodeIndex::new(ni);
                if let Some(node) = graph.get_node(node_idx) {
                    if let Some(src) = node.source_snippet() {
                        text.push_str(&format!("  {} [{}]:\n", node.name(), node.label()));
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
            text.push('\n');
        }

        ToolResult {
            content: vec![ToolContent::text(text)],
            is_error: None,
        }
    })
}

// ── formatting helpers ───────────────────────────────────────────────────

fn format_node(node: &GraphNode) -> String {
    match node {
        GraphNode::Function(f) => {
            format!(
                "  [Function] {} ({}:{}–{}, complexity={})",
                f.name,
                f.path.display(),
                f.span.start_line,
                f.span.end_line,
                f.cyclomatic_complexity,
            )
        }
        GraphNode::Class(c) => {
            let bases = if c.bases.is_empty() {
                String::new()
            } else {
                format!(" extends {}", c.bases.join(", "))
            };
            format!(
                "  [Class] {}{} ({}:{}–{})",
                c.name,
                bases,
                c.path.display(),
                c.span.start_line,
                c.span.end_line,
            )
        }
        GraphNode::Struct(s) => {
            format!(
                "  [Struct] {} ({}:{}–{})",
                s.name,
                s.path.display(),
                s.span.start_line,
                s.span.end_line,
            )
        }
        GraphNode::Trait(t) => {
            format!(
                "  [Trait] {} ({}:{}–{})",
                t.name,
                t.path.display(),
                t.span.start_line,
                t.span.end_line,
            )
        }
        GraphNode::Interface(i) => {
            format!(
                "  [Interface] {} ({}:{}–{})",
                i.name,
                i.path.display(),
                i.span.start_line,
                i.span.end_line,
            )
        }
        GraphNode::Enum(e) => {
            format!(
                "  [Enum] {} [{}] ({}:{}–{})",
                e.name,
                e.variants.join(", "),
                e.path.display(),
                e.span.start_line,
                e.span.end_line,
            )
        }
        GraphNode::Variable(v) => {
            format!(
                "  [Variable] {} ({}:{})",
                v.name,
                v.path.display(),
                v.line_number,
            )
        }
        GraphNode::Module(m) => {
            format!("  [Module] {}", m.name)
        }
        GraphNode::File(f) => {
            format!("  [File] {} ({})", f.name, f.path.display())
        }
        _ => format!("  [{}] {}", node.label(), node.name()),
    }
}

fn format_node_brief(node: &GraphNode) -> String {
    match node {
        GraphNode::Function(f) => {
            format!("{} ({}:{})", f.name, f.path.display(), f.span.start_line)
        }
        GraphNode::Class(c) => {
            format!("{} ({}:{})", c.name, c.path.display(), c.span.start_line)
        }
        _ => format!("{} [{}]", node.name(), node.label()),
    }
}
