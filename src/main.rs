use std::path::{Path, PathBuf};
use std::process;

use clap::{Parser, Subcommand};

use ast_context::*;

mod mcp;
mod setup;

#[derive(Parser)]
#[command(
    name = "ast-context",
    version,
    about = "Build code graphs from AST/CST analysis"
)]
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
        /// Uses gitignore glob syntax. Also reads .astcontextignore and .astcontextignore.local files.
        #[arg(short, long = "exclude", value_delimiter = ',')]
        exclude: Vec<String>,

        /// Maximum file size in MB to index (default: 50). Files larger than this are skipped.
        #[arg(long, default_value = "50")]
        max_file_size: u64,

        /// Skip test files for a smaller, faster graph focused on production code.
        #[arg(long)]
        skip_tests: bool,
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

        /// Similarity threshold for near-duplicate detection (0.0-1.0)
        #[arg(long, default_value = "0.80")]
        near_dup_threshold: f64,

        /// Similarity threshold for structural similarity (0.0-1.0)
        #[arg(long, default_value = "0.50")]
        structural_threshold: f64,

        /// Shared line ratio for merge candidates (0.0-1.0)
        #[arg(long, default_value = "0.40")]
        merge_threshold: f64,

        /// Min structural (AST-shape) cosine to confirm a near-duplicate/similar
        /// finding (0.0-1.0). Higher = fewer false positives (annotated graphs).
        #[arg(long, default_value = "0.50")]
        structural_confirm: f64,

        /// List of checks or categories to skip (e.g. dead_code, anti_patterns). Comma-separated.
        #[arg(long = "skip-check", value_delimiter = ',')]
        skip_checks: Vec<String>,

        /// Filter to a specific category: redundancy, struct_enum, type_suggestions, design_patterns,
        /// anti_patterns, pattern_detection, structural, type_system, metrics, risk, testing,
        /// blast_radius, api_surface, cross_language, config_detection, data_structures,
        /// code_quality, optimization
        #[arg(long)]
        category: Option<String>,

        /// Include full source code snippets in output (increases context significantly)
        #[arg(long)]
        include_source: bool,

        /// Maximum number of findings to return per redundancy type (0 = all)
        #[arg(long, default_value = "10")]
        limit_per_type: usize,

        /// Confirm findings with a language server (rust-analyzer) instead of
        /// syntactic heuristics. Resolves real references/types, so e.g. dead
        /// code is verified against true usage. Slower: the server loads and
        /// indexes the project (which must build) — seconds, not milliseconds.
        /// Rooted at the current directory; requires rust-analyzer on PATH.
        #[arg(long)]
        semantic: bool,
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

    /// Configure the MCP server for Claude Desktop, Claude Code, Zed, Cursor, Windsurf, VS Code, and JetBrains
    Setup {
        /// Path to the ast_context binary (auto-detected from ~/.cargo/bin if omitted)
        #[arg(long)]
        mcp_path: Option<PathBuf>,

        /// Show what would be configured without making any changes
        #[arg(long)]
        dry_run: bool,
    },

    /// Start the MCP server (JSON-RPC 2.0 over stdin/stdout)
    Mcp,
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
            max_file_size,
            skip_tests,
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
            if skip_tests {
                eprintln!("Skipping test files.");
            }
            let graph = match GraphBuilder::build_full_with_options(
                &path,
                annotate,
                &exclude,
                Some(max_file_size * 1024 * 1024),
                skip_tests,
            ) {
                Ok(g) => g,
                Err(e) => {
                    eprintln!("Error building graph: {e}");
                    process::exit(1);
                }
            };

            match format.as_str() {
                "stats" => {
                    serialize::print_stats(&graph);
                }
                "json" => {
                    let out = output.unwrap_or_else(|| PathBuf::from("graph.json"));
                    if let Err(e) = serialize::export_json(&graph, &out) {
                        eprintln!("Error exporting JSON: {e}");
                        process::exit(1);
                    }
                    eprintln!("Exported to {}", out.display());
                }
                "jsonl" => {
                    let out = output.unwrap_or_else(|| PathBuf::from("output"));
                    if let Err(e) = serialize::export_jsonl(&graph, &out) {
                        eprintln!("Error exporting JSONL: {e}");
                        process::exit(1);
                    }
                    eprintln!(
                        "Exported to {}/nodes.jsonl and {}/edges.jsonl",
                        out.display(),
                        out.display()
                    );
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
                ("C#", &["cs"]),
                ("Ruby", &["rb"]),
                ("PHP", &["php", "phtml"]),
                ("Swift", &["swift"]),
                ("Dart", &["dart"]),
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

            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

            let parser = match parser::parser_for_extension(ext) {
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
                println!("  [{}] {}", node.short_label(), node.name());
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
                        println!("  {} [{}]", node.name(), node.short_label());
                    }
                }
                "callees" => {
                    let callees = g.get_callees_of(idx);
                    println!("Callees of '{}' ({} found):", name, callees.len());
                    for (_, node) in &callees {
                        println!("  {} [{}]", node.name(), node.short_label());
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
                        println!("  {}→ {} [{}]", "  ".repeat(*d), node.name(), node.short_label());
                    }
                }
                "implementors" => {
                    let impls = g.get_implementors(idx);
                    println!("Implementors of '{}' ({} found):", name, impls.len());
                    for (_, node) in &impls {
                        println!("  {} [{}]", node.name(), node.short_label());
                    }
                }
                "children" => {
                    let children = g.get_children(idx);
                    println!("Children of '{}' ({} found):", name, children.len());
                    for (_, node) in &children {
                        println!("  {} [{}]", node.name(), node.short_label());
                    }
                }
                _ => {
                    eprintln!("Unknown relationship: {relationship}");
                    eprintln!(
                        "Use: callers, callees, inheritance, call_chain, implementors, children"
                    );
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
                if let types::node::GraphNode::Function(f) = node {
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
                if let types::node::GraphNode::Function(f) = node {
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

            println!(
                "Found {} groups of potentially similar code:\n",
                groups.len()
            );
            for (i, group) in groups.iter().enumerate() {
                println!("── Group {} ({} nodes) ──", i + 1, group.len());
                for (_, node) in group {
                    match node {
                        types::node::GraphNode::Function(f) => {
                            println!(
                                "  [fn] {} ({}:{}–{}, cc={})",
                                f.name,
                                f.path.display(),
                                f.span.start_line,
                                f.span.end_line,
                                f.cyclomatic_complexity,
                            );
                        }
                        _ => {
                            println!("  [{}] {}", node.short_label(), node.name());
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
            near_dup_threshold,
            structural_threshold,
            merge_threshold,
            structural_confirm,
            skip_checks,
            category,
            include_source,
            limit_per_type,
            semantic,
        } => {
            let g = load_graph(&graph);

            let min_tier = match tier.to_lowercase().as_str() {
                "critical" => analysis::Tier::Critical,
                "high" => analysis::Tier::High,
                "medium" => analysis::Tier::Medium,
                "low" => analysis::Tier::Low,
                _ => {
                    eprintln!("Unknown tier: {tier}. Use: critical, high, medium, low");
                    process::exit(1);
                }
            };

            let config = analysis::AnalysisConfig {
                min_lines,
                split_complexity_threshold: split_complexity,
                split_line_threshold: split_lines,
                near_duplicate_threshold: near_dup_threshold,
                structural_threshold,
                merge_threshold,
                structural_confirm_threshold: structural_confirm,
                skip_checks,
                category,
                ..Default::default()
            };

            let findings = run_redundancy(&g, &config, semantic);
            let mut filtered: Vec<_> = findings
                .into_iter()
                .filter(|f| f.tier <= min_tier)
                .collect();

            // Deterministic output (no shuffle): the old randomization existed only
            // to rotate past a false-positive flood that clustering has since fixed.
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
                println!("No redundancy findings at tier {tier} or above.");
                println!("Make sure the graph was built with --annotate.");
                return;
            }

            // Emit compact legend
            {
                use std::collections::BTreeSet;
                let used: BTreeSet<&str> = filtered.iter().map(|f| f.kind.legend_entry()).collect();
                println!("Tiers: C=critical H=high M=medium L=low");
                println!("Codes: {}\n", used.into_iter().collect::<Vec<_>>().join(" "));
            }

            for finding in &filtered {
                let tag = finding.kind.short_code();

                let tier_flag = match finding.tier {
                    analysis::Tier::Critical => "C",
                    analysis::Tier::High => "H",
                    analysis::Tier::Medium => "M",
                    analysis::Tier::Low => "L",
                };

                println!("[{tier_flag}][{tag}] {}", finding.description);

                for &ni in &finding.node_indices {
                    let node_idx = petgraph::graph::NodeIndex::new(ni);
                    if let Some(node) = g.get_node(node_idx) {
                        let loc = node.location();
                        let path_str = std::env::current_dir()
                            .ok()
                            .and_then(|cwd| {
                                std::path::Path::new(&loc.0)
                                    .strip_prefix(&cwd)
                                    .ok()
                                    .map(|p| p.display().to_string())
                            })
                            .unwrap_or_else(|| loc.0.clone());
                        let loc_str = if path_str.is_empty() {
                            "".to_string()
                        } else if loc.1 > 0 {
                            format!(" ({}:{})", path_str, loc.1)
                        } else {
                            format!(" ({})", path_str)
                        };
                        println!("    {} [{}]{loc_str}", node.name(), node.short_label());

                        if include_source {
                            if let Some(src) = node.source_snippet() {
                                let preview: String = src
                                    .lines()
                                    .take(4)
                                    .map(|l| format!("      │ {l}"))
                                    .collect::<Vec<_>>()
                                    .join("\n");
                                let total = src.lines().count();
                                println!("{preview}");
                                if total > 4 {
                                    println!("      │ ... ({} more lines)", total - 4);
                                }
                            }
                        }
                    }
                }
                println!();
            }

            println!(
                "Total: {} findings ({} critical, {} high, {} medium, {} low)",
                filtered.len(),
                filtered
                    .iter()
                    .filter(|f| f.tier == analysis::Tier::Critical)
                    .count(),
                filtered
                    .iter()
                    .filter(|f| f.tier == analysis::Tier::High)
                    .count(),
                filtered
                    .iter()
                    .filter(|f| f.tier == analysis::Tier::Medium)
                    .count(),
                filtered
                    .iter()
                    .filter(|f| f.tier == analysis::Tier::Low)
                    .count(),
            );
        }

        Commands::Watch {
            path,
            debounce,
            exclude: _exclude,
        } => {
            if !path.exists() {
                eprintln!("Error: path does not exist: {}", path.display());
                process::exit(1);
            }

            eprintln!(
                "Watching {} for changes (debounce: {}ms)...",
                path.display(),
                debounce
            );
            eprintln!("Press Ctrl+C to stop.");

            let mut watcher = match FileWatcher::start(&path, Some(debounce)) {
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
                        watcher::WatchEvent::GraphRebuilt {
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
                        watcher::WatchEvent::Error(e) => {
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

        Commands::Setup { mcp_path, dry_run } => {
            setup::run(mcp_path, dry_run);
        }

        Commands::Mcp => {
            mcp::run_server();
        }
    }
}

fn load_graph(path: &Path) -> CodeGraph {
    match CodeGraph::load(path) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("Error loading graph from {}: {e}", path.display());
            process::exit(1);
        }
    }
}

/// Run the analysis, optionally backed by a language server. With `--semantic`,
/// findings are routed through rust-analyzer (started for the current
/// directory). If the server can't start, this does NOT fall back to the
/// heuristic-only path: semantic mode is a contract for confirmed results, so we
/// fail with a clear message rather than hand back noisy guesses.
#[cfg(feature = "lsp")]
fn run_redundancy(
    g: &CodeGraph,
    config: &analysis::AnalysisConfig,
    semantic: bool,
) -> Vec<analysis::Finding> {
    if !semantic {
        return analysis::analyze(g, config);
    }
    let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    match analysis::LspProvider::start(&root) {
        Some(provider) => {
            eprintln!("semantic mode: rust-analyzer ready ({})", root.display());
            analysis::analyze_with(g, config, &provider)
        }
        None => {
            eprintln!(
                "Error: --semantic was requested but rust-analyzer could not start for {}.\n\
                 Is rust-analyzer on PATH (or AST_CONTEXT_RUST_ANALYZER set)? Does the project build?\n\
                 Refusing to fall back to heuristic-only results, which are noisy — re-run without \
                 --semantic if you want them.",
                root.display()
            );
            process::exit(1);
        }
    }
}

#[cfg(not(feature = "lsp"))]
fn run_redundancy(
    g: &CodeGraph,
    config: &analysis::AnalysisConfig,
    semantic: bool,
) -> Vec<analysis::Finding> {
    if semantic {
        eprintln!(
            "Error: --semantic requires the `lsp` feature, which this binary was built without. \
             Rebuild with the default features, or re-run without --semantic."
        );
        process::exit(1);
    }
    analysis::analyze(g, config)
}

fn ctrlc_channel(tx: &std::sync::mpsc::Sender<()>) {
    #[cfg(unix)]
    {
        let tx2 = tx.clone();
        unsafe {
            libc_signal(2, move || {
                let _ = tx2.send(());
            });
        }
    }

    // On non-Unix platforms (Windows) rely on the OS to terminate the process on Ctrl+C.
    // The watcher loop will exit naturally when the process is killed.
    let _ = tx;
}

// Minimal Ctrl+C handling without external crate — Unix only.
#[cfg(unix)]
unsafe fn libc_signal<F: Fn() + Send + Sync + 'static>(sig: i32, handler: F) {
    use std::sync::OnceLock;
    static HANDLER: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();
    HANDLER.get_or_init(|| Box::new(handler));

    extern "C" fn signal_handler(_: i32) {
        if let Some(h) = HANDLER.get() {
            h();
        }
    }

    // SAFETY: registering a signal handler. `signal_handler` is an `extern "C"`
    // function with the correct `sighandler_t` signature; cast through a pointer
    // (not a direct fn-item→integer cast) per the compiler lint. The explicit
    // `unsafe` block is required under edition 2024.
    unsafe {
        libc::signal(sig, signal_handler as *const () as libc::sighandler_t);
    }
}
