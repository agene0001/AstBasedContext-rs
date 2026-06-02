//! Calibrate `structural_confirm_threshold` against a labeled corpus.
//!
//! Sweeps the threshold, runs the full redundancy analysis at each value, and
//! reports precision / recall / F1 of the near-duplicate + structurally-similar
//! findings against ground-truth labels. Labels are derived from function-name
//! prefixes so the corpus is self-describing:
//!
//!   `dup<G>_<n>`  → every function sharing group `<G>` is a true-similar cluster
//!                   (each within-group pair is a true positive that *should* be flagged)
//!   `trap_*` / `uniq_*` → belong to no cluster; any flagged pair touching them,
//!                          or any cross-group pair, is a false positive
//!
//! Traps are crafted to share vocabulary with a `dup` group but differ in AST
//! shape — they're the lexical false positives the structural gate should drop.
//!
//! Usage: `cargo run --release --example calibrate_threshold -- [corpus_dir]`
//! (defaults to `tests/fixtures/redundancy/calibration`).

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

use ast_context::redundancy::{self, AnalysisConfig, FindingKind};
use ast_context::types::node::GraphNode;
use ast_context::{CodeGraph, GraphBuilder};

/// Ground-truth cluster for a function name, or `None` if it belongs to none.
fn cluster_of(name: &str) -> Option<String> {
    let rest = name.strip_prefix("dup")?;
    let group = rest.split('_').next()?;
    (!group.is_empty()).then(|| group.to_string())
}

/// All within-cluster pairs that *should* be flagged, from the graph's functions.
fn true_pairs(graph: &CodeGraph) -> BTreeSet<(String, String)> {
    let mut by_cluster: HashMap<String, Vec<String>> = HashMap::new();
    for (_, node) in graph.nodes_by_label("Function") {
        if let GraphNode::Function(f) = node {
            if let Some(c) = cluster_of(&f.name) {
                by_cluster.entry(c).or_default().push(f.name.clone());
            }
        }
    }
    let mut pairs = BTreeSet::new();
    for names in by_cluster.values() {
        for i in 0..names.len() {
            for j in (i + 1)..names.len() {
                pairs.insert(ordered(&names[i], &names[j]));
            }
        }
    }
    pairs
}

fn ordered(a: &str, b: &str) -> (String, String) {
    if a <= b {
        (a.to_string(), b.to_string())
    } else {
        (b.to_string(), a.to_string())
    }
}

/// Pairs flagged by the near-duplicate + structurally-similar checks at `threshold`.
fn flagged_pairs(graph: &CodeGraph, threshold: f64) -> BTreeSet<(String, String)> {
    let config = AnalysisConfig {
        // Only the two lexical-similarity checks matter here; keep their
        // candidate windows at defaults and vary just the structural gate.
        structural_confirm_threshold: threshold,
        min_lines: 2,
        ..Default::default()
    };
    let mut pairs = BTreeSet::new();
    for finding in redundancy::analyze(graph, &config) {
        let names = match &finding.kind {
            FindingKind::NearDuplicate { names, .. }
            | FindingKind::StructurallySimilar { names, .. } => names,
            _ => continue,
        };
        for i in 0..names.len() {
            for j in (i + 1)..names.len() {
                pairs.insert(ordered(&names[i], &names[j]));
            }
        }
    }
    pairs
}

fn main() {
    let dir = std::env::args().nth(1).map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/redundancy/calibration")
    });
    if !dir.exists() {
        eprintln!("corpus dir does not exist: {}", dir.display());
        std::process::exit(1);
    }

    let graph = GraphBuilder::build_full_with_options(&dir, true, &[], None, false)
        .expect("graph build should succeed");

    let truth = true_pairs(&graph);
    if truth.is_empty() {
        eprintln!("no `dup<G>_<n>` functions found in corpus — nothing to calibrate against");
        std::process::exit(1);
    }
    println!("Corpus: {} true-similar pairs to recover.\n", truth.len());
    println!(
        "{:>9}  {:>3}  {:>3}  {:>9}  {:>6}  {:>6}",
        "threshold", "TP", "FP", "precision", "recall", "F1"
    );

    let mut best = (f64::NAN, -1.0f64); // (threshold, f1)
    let mut t = 0.0;
    while t <= 0.901 {
        let flagged = flagged_pairs(&graph, t);
        let tp = flagged.intersection(&truth).count();
        let fp = flagged.len() - tp;
        let precision = if flagged.is_empty() {
            1.0
        } else {
            tp as f64 / flagged.len() as f64
        };
        let recall = tp as f64 / truth.len() as f64;
        let f1 = if precision + recall > 0.0 {
            2.0 * precision * recall / (precision + recall)
        } else {
            0.0
        };
        println!(
            "{t:>9.2}  {tp:>3}  {fp:>3}  {precision:>9.2}  {recall:>6.2}  {f1:>6.2}"
        );
        // Prefer the highest F1; on ties, the lower threshold (keeps recall).
        if f1 > best.1 + 1e-9 {
            best = (t, f1);
        }
        t += 0.05;
    }

    println!(
        "\nRecommended structural_confirm_threshold: {:.2} (F1 = {:.2})",
        best.0, best.1
    );
}
