//! Token-usage reduction benchmark.
//!
//! Quantifies the tool's value proposition: an agent that reads a compact,
//! AST-derived *map* of a codebase instead of the raw source uses far fewer
//! tokens. Run it against any directory:
//!
//! ```text
//! cargo run --release --example token_ratio -- /path/to/project
//! ```
//!
//! It reports two things:
//!   1. Whole-repo overview: tokens of a per-file symbol map vs. reading every
//!      source file (the "understand the codebase" baseline).
//!   2. Targeted retrieval: for a sample of symbols, tokens of a
//!      definition + callers + callees context vs. reading the whole file(s).
//!
//! Token counts are approximated as `chars / 4`. That's deliberately
//! tokenizer-agnostic: we report *ratios*, which are stable across tokenizers.
//! Swap in a real tokenizer (e.g. `tiktoken-rs`) if you want absolute counts.

use std::collections::HashSet;
use std::path::PathBuf;

use ast_context::graph::CodeGraph;
use ast_context::types::node::GraphNode;
use ast_context::{walker, GraphBuilder};

/// Rough token estimate. Ratios are what matter, not the absolute number.
fn approx_tokens(s: &str) -> usize {
    s.chars().count().div_ceil(4)
}

fn main() {
    let root = match std::env::args().nth(1) {
        Some(p) => PathBuf::from(p),
        None => {
            eprintln!("usage: cargo run --example token_ratio -- <path>");
            std::process::exit(2);
        }
    };
    if !root.exists() {
        eprintln!("path does not exist: {}", root.display());
        std::process::exit(1);
    }

    // ── Baseline: tokens to read every source file ──────────────────────
    let files = walker::walk_source_files_full(&root, &[], false);
    let mut raw_tokens = 0usize;
    for f in &files {
        if let Ok(bytes) = std::fs::read(f) {
            raw_tokens += bytes.len().div_ceil(4);
        }
    }

    // ── Build the graph (annotated, so snippets are available) ──────────
    let graph = GraphBuilder::build_full_with_options(&root, true, &[], None, false)
        .expect("graph build should succeed");

    // ── Whole-repo map: one line per file + its public symbols ──────────
    let map = render_repo_map(&graph);
    let map_tokens = approx_tokens(&map);

    println!("== Whole-repo overview ==");
    println!("  source files:        {}", files.len());
    println!("  raw source tokens:   ~{raw_tokens}");
    println!("  structural map tokens: ~{map_tokens}");
    if map_tokens > 0 {
        println!(
            "  compression:         {:.1}x  (map is {:.1}% of raw)",
            raw_tokens as f64 / map_tokens as f64,
            100.0 * map_tokens as f64 / raw_tokens as f64,
        );
    }

    // ── Targeted retrieval: symbol context vs. whole file ───────────────
    // Node paths are stored relative to the canonicalized root; join to read.
    let base = root.canonicalize().unwrap_or(root.clone());
    let (ctx_tokens, file_tokens, n) = sample_symbol_context(&graph, &base);
    if n > 0 {
        println!("\n== Targeted retrieval (sample of {n} functions) ==");
        println!("  avg context tokens:  ~{}", ctx_tokens / n);
        println!("  avg whole-file tokens: ~{}", file_tokens / n);
        if ctx_tokens > 0 {
            println!(
                "  compression:         {:.1}x",
                file_tokens as f64 / ctx_tokens.max(1) as f64,
            );
        }
    }
}

/// One line per file: relative path, line count, and its top-level symbol
/// signatures — the density `get_module_overview` / `get_file_summary` deliver.
fn render_repo_map(graph: &CodeGraph) -> String {
    let mut out = String::new();
    for (idx, node) in graph.nodes_by_label("File") {
        let GraphNode::File(f) = node else { continue };
        out.push_str(&format!("{} ({}L)\n", f.relative_path, f.total_lines));
        for (_, child) in graph.get_children(idx) {
            match child {
                GraphNode::Function(fd) => {
                    out.push_str(&format!("  fn {}({})\n", fd.name, fd.args.join(", ")));
                }
                GraphNode::Class(c) => out.push_str(&format!("  class {}\n", c.name)),
                GraphNode::Struct(s) => out.push_str(&format!("  struct {}\n", s.name)),
                GraphNode::Trait(t) => out.push_str(&format!("  trait {}\n", t.name)),
                GraphNode::Enum(e) => out.push_str(&format!("  enum {}\n", e.name)),
                _ => {}
            }
        }
    }
    out
}

/// For a sample of functions, compare the tokens of a focused context
/// (definition source + caller/callee names) against reading the whole file(s)
/// that contain the function and its neighbours.
fn sample_symbol_context(graph: &CodeGraph, base: &std::path::Path) -> (usize, usize, usize) {
    const SAMPLE: usize = 40;
    let mut ctx_tokens = 0usize;
    let mut file_tokens = 0usize;
    let mut n = 0usize;

    for (idx, node) in graph.nodes_by_label("Function") {
        if n >= SAMPLE {
            break;
        }
        let Some(src) = node.source_snippet() else { continue };

        // Focused context: the definition + one-line caller/callee summaries.
        let callers = graph.get_callers_of(idx);
        let callees = graph.get_callees_of(idx);
        let mut ctx = String::from(src);
        for (_, n) in callers.iter().chain(callees.iter()) {
            ctx.push_str(n.name());
            ctx.push(' ');
        }
        ctx_tokens += approx_tokens(&ctx);

        // Naive alternative: read the full file(s) of this symbol + neighbours.
        let mut paths: HashSet<String> = HashSet::new();
        paths.insert(node.location().0);
        for (_, neighbour) in callers.iter().chain(callees.iter()) {
            paths.insert(neighbour.location().0);
        }
        for p in &paths {
            if let Ok(bytes) = std::fs::read(base.join(p)) {
                file_tokens += bytes.len().div_ceil(4);
            }
        }
        n += 1;
    }
    (ctx_tokens, file_tokens, n)
}
