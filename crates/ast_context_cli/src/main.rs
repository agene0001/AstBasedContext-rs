use std::path::PathBuf;
use std::process;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "ast-context", version, about = "Build code graphs from AST/CST analysis")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Index a directory and build the code graph
    Index {
        /// Path to the directory to index
        path: PathBuf,

        /// Output format: json, jsonl, or stats
        #[arg(short, long, default_value = "stats")]
        format: String,

        /// Output path (for json/jsonl formats)
        #[arg(short = 'O', long)]
        output: Option<PathBuf>,

        /// Save the graph to a file for later loading
        #[arg(short, long)]
        save: Option<PathBuf>,

        /// Attach source snippets to each node for AI-driven analysis
        /// (redundancy detection, refactoring suggestions). Increases graph size.
        #[arg(short, long)]
        annotate: bool,

        /// Exclude directories/files matching glob patterns.
        /// Comma-separated or repeatable: --exclude "vendor/**,*.generated.go"
        /// Uses gitignore glob syntax. Also reads .astcontextignore files.
        #[arg(short, long = "exclude", value_delimiter = ',')]
        exclude: Vec<String>,
    },

    /// Show supported languages
    Languages,

    /// Parse a single file and print its structure
    Parse {
        /// Path to the file to parse
        path: PathBuf,
    },

    /// Search for code elements in an indexed graph
    Search {
        /// Path to a saved graph file
        #[arg(short, long)]
        graph: PathBuf,

        /// Search query (name or partial name)
        query: String,

        /// Filter by node type: Function, Class, Struct, Trait, Interface, Enum, Variable
        #[arg(short, long)]
        kind: Option<String>,
    },

    /// Analyze code relationships
    Analyze {
        /// Path to a saved graph file
        #[arg(short, long)]
        graph: PathBuf,

        /// Name of the function or class
        name: String,

        /// Relationship: callers, callees, inheritance, call_chain, implementors, children
        #[arg(short, long)]
        relationship: String,

        /// Max depth for call_chain (default: 5)
        #[arg(short, long, default_value = "5")]
        depth: usize,
    },

    /// Find dead code (uncalled functions)
    DeadCode {
        /// Path to a saved graph file
        #[arg(short, long)]
        graph: PathBuf,

        /// Maximum results
        #[arg(short, long, default_value = "50")]
        limit: usize,
    },

    /// Find most complex functions
    Complexity {
        /// Path to a saved graph file
        #[arg(short, long)]
        graph: PathBuf,

        /// Maximum results
        #[arg(short, long, default_value = "20")]
        limit: usize,
    },

    /// Find similar/redundant code (requires --annotate during index)
    Similar {
        /// Path to a saved graph file (must have been built with --annotate)
        #[arg(short, long)]
        graph: PathBuf,

        /// Filter by node type: Function, Class, Struct, Trait, Interface, Enum
        #[arg(short, long)]
        kind: Option<String>,

        /// Minimum lines for a node to be considered (skip trivial code)
        #[arg(short, long, default_value = "5")]
        min_lines: usize,
    },

    /// Run tiered redundancy analysis (requires --annotate during index)
    Redundancy {
        /// Path to a saved graph file (must have been built with --annotate)
        #[arg(short, long)]
        graph: PathBuf,

        /// Only show findings at or above this tier: critical, high, medium, low
        #[arg(short, long, default_value = "low")]
        tier: String,

        /// Minimum function lines to consider
        #[arg(short, long, default_value = "3")]
        min_lines: usize,

        /// Complexity threshold for split candidates
        #[arg(long, default_value = "15")]
        split_complexity: u32,

        /// Line threshold for split candidates
        #[arg(long, default_value = "60")]
        split_lines: usize,
    },

    /// Watch a directory for changes and rebuild the graph
    Watch {
        /// Path to the directory to watch
        path: PathBuf,

        /// Debounce interval in milliseconds
        #[arg(short, long, default_value = "2000")]
        debounce: u64,

        /// Exclude directories/files matching glob patterns.
        /// Comma-separated or repeatable: --exclude "vendor/**,build/**"
        #[arg(short, long = "exclude", value_delimiter = ',')]
        exclude: Vec<String>,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Index {
            path,
            format,
            output,
            save,
            annotate,
            exclude,
        } => {
            if !path.exists() {
                eprintln!("Error: path does not exist: {}", path.display());
                process::exit(1);
            }

            if annotate {
                eprintln!("Indexing {} with source annotations...", path.display());
            } else {
                eprintln!("Indexing {}...", path.display());
            }
            if !exclude.is_empty() {
                eprintln!("Excluding: {}", exclude.join(", "));
            }
            let graph = match ast_context_core::GraphBuilder::build_full(&path, annotate, &exclude) {
                Ok(g) => g,
                Err(e) => {
                    eprintln!("Error building graph: {e}");
                    process::exit(1);
                }
            };

            match format.as_str() {
                "stats" => {
                    ast_context_core::serialize::print_stats(&graph);
                }
                "json" => {
                    let out = output.unwrap_or_else(|| PathBuf::from("graph.json"));
                    if let Err(e) = ast_context_core::serialize::export_json(&graph, &out) {
                        eprintln!("Error exporting JSON: {e}");
                        process::exit(1);
                    }
                    eprintln!("Exported to {}", out.display());
                }
                "jsonl" => {
                    let out = output.unwrap_or_else(|| PathBuf::from("output"));
                    if let Err(e) = ast_context_core::serialize::export_jsonl(&graph, &out) {
                        eprintln!("Error exporting JSONL: {e}");
                        process::exit(1);
                    }
                    eprintln!("Exported to {}/nodes.jsonl and {}/edges.jsonl", out.display(), out.display());
                }
                _ => {
                    eprintln!("Unknown format: {format}. Use: stats, json, jsonl");
                    process::exit(1);
                }
            }

            if let Some(save_path) = save {
                if let Err(e) = graph.save(&save_path) {
                    eprintln!("Error saving graph: {e}");
                    process::exit(1);
                }
                eprintln!("Graph saved to {}", save_path.display());
            }
        }

        Commands::Languages => {
            println!("Supported languages:");
            let langs = [
                ("Python", &["py", "pyw"] as &[&str]),
                ("Rust", &["rs"]),
                ("TypeScript", &["ts", "tsx"]),
                ("JavaScript", &["js", "jsx", "mjs", "cjs"]),
                ("Go", &["go"]),
                ("Java", &["java"]),
                ("C", &["c"]),
                ("C++", &["cpp", "cc", "cxx", "hpp", "hh", "h"]),
            ];
            for (name, exts) in &langs {
                println!("  {name}: {}", exts.join(", "));
            }
        }

        Commands::Parse { path } => {
            if !path.exists() {
                eprintln!("Error: file does not exist: {}", path.display());
                process::exit(1);
            }

            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");

            let parser = match ast_context_core::parser::parser_for_extension(ext) {
                Some(p) => p,
                None => {
                    eprintln!("No parser for extension: .{ext}");
                    process::exit(1);
                }
            };

            let source = match std::fs::read(&path) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Error reading file: {e}");
                    process::exit(1);
                }
            };

            match parser.parse(&path, &source, false) {
                Ok(result) => {
                    println!("{}", serde_json::to_string_pretty(&result).unwrap());
                }
                Err(e) => {
                    eprintln!("Parse error: {e}");
                    process::exit(1);
                }
            }
        }

        Commands::Search { graph, query, kind } => {
            let g = load_graph(&graph);
            let results = g.search_by_name(&query);
            let filtered: Vec<_> = results
                .into_iter()
                .filter(|(_, node)| {
                    if let Some(ref k) = kind {
                        node.label() == k.as_str()
                    } else {
                        true
                    }
                })
                .take(50)
                .collect();

            if filtered.is_empty() {
                println!("No results found for '{query}'");
                return;
            }

            println!("Found {} results for '{query}':", filtered.len());
            for (_, node) in &filtered {
                println!("  [{}] {}", node.label(), node.name());
            }
        }

        Commands::Analyze {
            graph,
            name,
            relationship,
            depth,
        } => {
            let g = load_graph(&graph);

            let indices = g.find_functions(&name);
            let indices = if indices.is_empty() {
                g.find_classes(&name)
            } else {
                indices
            };

            if indices.is_empty() {
                eprintln!("No node found with name '{name}'");
                process::exit(1);
            }

            let idx = indices[0];
            match relationship.as_str() {
                "callers" => {
                    let callers = g.get_callers_of(idx);
                    println!("Callers of '{}' ({} found):", name, callers.len());
                    for (_, node) in &callers {
                        println!("  {} [{}]", node.name(), node.label());
                    }
                }
                "callees" => {
                    let callees = g.get_callees_of(idx);
                    println!("Callees of '{}' ({} found):", name, callees.len());
                    for (_, node) in &callees {
                        println!("  {} [{}]", node.name(), node.label());
                    }
                }
                "inheritance" => {
                    let chain = g.get_inheritance_chain(idx);
                    println!("Inheritance chain for '{name}':");
                    println!("  {name}");
                    for (i, (_, node)) in chain.iter().enumerate() {
                        println!("  {}↳ {}", "  ".repeat(i + 1), node.name());
                    }
                }
                "call_chain" => {
                    let chain = g.get_call_chain(idx, depth);
                    println!("Call chain from '{}' ({} nodes):", name, chain.len());
                    for (_, node, d) in &chain {
                        println!("  {}→ {} [{}]", "  ".repeat(*d), node.name(), node.label());
                    }
                }
                "implementors" => {
                    let impls = g.get_implementors(idx);
                    println!("Implementors of '{}' ({} found):", name, impls.len());
                    for (_, node) in &impls {
                        println!("  {} [{}]", node.name(), node.label());
                    }
                }
                "children" => {
                    let children = g.get_children(idx);
                    println!("Children of '{}' ({} found):", name, children.len());
                    for (_, node) in &children {
                        println!("  {} [{}]", node.name(), node.label());
                    }
                }
                _ => {
                    eprintln!("Unknown relationship: {relationship}");
                    eprintln!("Use: callers, callees, inheritance, call_chain, implementors, children");
                    process::exit(1);
                }
            }
        }

        Commands::DeadCode { graph, limit } => {
            let g = load_graph(&graph);
            let dead: Vec<_> = g.find_dead_code().into_iter().take(limit).collect();

            if dead.is_empty() {
                println!("No dead code candidates found.");
                return;
            }

            println!("Dead code candidates ({} found):", dead.len());
            for (_, node) in &dead {
                if let ast_context_core::types::node::GraphNode::Function(f) = node {
                    println!(
                        "  {} ({}:{}, complexity={})",
                        f.name,
                        f.path.display(),
                        f.span.start_line,
                        f.cyclomatic_complexity,
                    );
                }
            }
        }

        Commands::Complexity { graph, limit } => {
            let g = load_graph(&graph);
            let funcs = g.most_complex_functions(limit);

            if funcs.is_empty() {
                println!("No functions found.");
                return;
            }

            println!("Most complex functions (top {}):", funcs.len());
            for (_, node, complexity) in &funcs {
                if let ast_context_core::types::node::GraphNode::Function(f) = node {
                    println!(
                        "  complexity={complexity}  {} ({}:{})",
                        f.name,
                        f.path.display(),
                        f.span.start_line,
                    );
                }
            }
        }

        Commands::Similar {
            graph,
            kind,
            min_lines,
        } => {
            let g = load_graph(&graph);
            let groups = g.find_similar_nodes(kind.as_deref(), min_lines);

            if groups.is_empty() {
                println!("No similar code groups found.");
                println!("Make sure the graph was built with --annotate.");
                return;
            }

            println!("Found {} groups of potentially similar code:\n", groups.len());
            for (i, group) in groups.iter().enumerate() {
                println!("── Group {} ({} nodes) ──", i + 1, group.len());
                for (_, node) in group {
                    match node {
                        ast_context_core::types::node::GraphNode::Function(f) => {
                            println!(
                                "  [Function] {} ({}:{}–{}, complexity={})",
                                f.name,
                                f.path.display(),
                                f.span.start_line,
                                f.span.end_line,
                                f.cyclomatic_complexity,
                            );
                        }
                        _ => {
                            println!("  [{}] {}", node.label(), node.name());
                        }
                    }
                    if let Some(src) = node.source_snippet() {
                        // Show first 5 lines of source as preview
                        let preview: String = src
                            .lines()
                            .take(5)
                            .map(|l| format!("    │ {l}"))
                            .collect::<Vec<_>>()
                            .join("\n");
                        let total_lines = src.lines().count();
                        println!("{preview}");
                        if total_lines > 5 {
                            println!("    │ ... ({} more lines)", total_lines - 5);
                        }
                    }
                    println!();
                }
            }
        }

        Commands::Redundancy {
            graph,
            tier,
            min_lines,
            split_complexity,
            split_lines,
        } => {
            let g = load_graph(&graph);

            let min_tier = match tier.to_lowercase().as_str() {
                "critical" => ast_context_core::redundancy::Tier::Critical,
                "high" => ast_context_core::redundancy::Tier::High,
                "medium" => ast_context_core::redundancy::Tier::Medium,
                "low" => ast_context_core::redundancy::Tier::Low,
                _ => {
                    eprintln!("Unknown tier: {tier}. Use: critical, high, medium, low");
                    process::exit(1);
                }
            };

            let config = ast_context_core::redundancy::AnalysisConfig {
                min_lines,
                split_complexity_threshold: split_complexity,
                split_line_threshold: split_lines,
                ..Default::default()
            };

            let findings = ast_context_core::redundancy::analyze(&g, &config);
            let filtered: Vec<_> = findings
                .iter()
                .filter(|f| f.tier <= min_tier)
                .collect();

            if filtered.is_empty() {
                println!("No redundancy findings at tier {tier} or above.");
                println!("Make sure the graph was built with --annotate.");
                return;
            }

            let mut current_tier = None;
            for finding in &filtered {
                if current_tier != Some(finding.tier) {
                    current_tier = Some(finding.tier);
                    println!("\n══ {} ══\n", finding.tier);
                }

                // Print kind tag
                use ast_context_core::redundancy::FindingKind;
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

                println!("  [{tag}] {}", finding.description);

                // Print source previews for involved nodes
                for &ni in &finding.node_indices {
                    let node_idx = petgraph::graph::NodeIndex::new(ni);
                    if let Some(node) = g.get_node(node_idx) {
                        if let Some(src) = node.source_snippet() {
                            let preview: String = src
                                .lines()
                                .take(4)
                                .map(|l| format!("      │ {l}"))
                                .collect::<Vec<_>>()
                                .join("\n");
                            let total = src.lines().count();
                            println!("    {} [{}]:", node.name(), node.label());
                            println!("{preview}");
                            if total > 4 {
                                println!("      │ ... ({} more lines)", total - 4);
                            }
                        }
                    }
                }
                println!();
            }

            println!(
                "Total: {} findings ({} critical, {} high, {} medium, {} low)",
                filtered.len(),
                filtered.iter().filter(|f| f.tier == ast_context_core::redundancy::Tier::Critical).count(),
                filtered.iter().filter(|f| f.tier == ast_context_core::redundancy::Tier::High).count(),
                filtered.iter().filter(|f| f.tier == ast_context_core::redundancy::Tier::Medium).count(),
                filtered.iter().filter(|f| f.tier == ast_context_core::redundancy::Tier::Low).count(),
            );
        }

        Commands::Watch { path, debounce, exclude: _exclude } => {
            if !path.exists() {
                eprintln!("Error: path does not exist: {}", path.display());
                process::exit(1);
            }

            eprintln!("Watching {} for changes (debounce: {}ms)...", path.display(), debounce);
            eprintln!("Press Ctrl+C to stop.");

            let mut watcher = match ast_context_core::FileWatcher::start(&path, Some(debounce)) {
                Ok(w) => w,
                Err(e) => {
                    eprintln!("Error starting watcher: {e}");
                    process::exit(1);
                }
            };

            // Handle Ctrl+C
            let (tx, rx) = std::sync::mpsc::channel();
            ctrlc_channel(&tx);

            loop {
                // Check for watcher events
                if let Ok(event) = watcher.events.try_recv() {
                    match event {
                        ast_context_core::watcher::WatchEvent::GraphRebuilt {
                            changed_files,
                            node_count,
                            edge_count,
                        } => {
                            eprintln!(
                                "Graph rebuilt: {} changed files, {} nodes, {} edges",
                                changed_files.len(),
                                node_count,
                                edge_count,
                            );
                        }
                        ast_context_core::watcher::WatchEvent::Error(e) => {
                            eprintln!("Watch error: {e}");
                        }
                    }
                }

                // Check for Ctrl+C
                if rx.try_recv().is_ok() {
                    eprintln!("Stopping watcher...");
                    watcher.stop();
                    break;
                }

                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    }
}

fn load_graph(path: &PathBuf) -> ast_context_core::CodeGraph {
    match ast_context_core::CodeGraph::load(path) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("Error loading graph from {}: {e}", path.display());
            process::exit(1);
        }
    }
}

fn ctrlc_channel(tx: &std::sync::mpsc::Sender<()>) {
    let tx = tx.clone();
    // Simple SIGINT handler
    let _ = std::thread::spawn(move || {
        // Use a simple approach: wait for EOF on a signal
        // In practice, the user will Ctrl+C and the process will exit
        loop {
            std::thread::sleep(std::time::Duration::from_secs(3600));
        }
    });
    // Set up Ctrl+C handler via signal
    let tx2 = tx.clone();
    unsafe {
        libc_signal(2, move || {
            let _ = tx2.send(());
        });
    }
}

// Minimal Ctrl+C handling without external crate
unsafe fn libc_signal<F: Fn() + Send + Sync + 'static>(sig: i32, handler: F) {
    use std::sync::OnceLock;
    static HANDLER: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();
    HANDLER.get_or_init(|| Box::new(handler));

    extern "C" fn signal_handler(_: i32) {
        if let Some(h) = HANDLER.get() {
            h();
        }
    }

    libc::signal(sig, signal_handler as libc::sighandler_t);
}
