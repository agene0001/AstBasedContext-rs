//! Live-server validation for the two LSP-backed checks (visibility tightening
//! and `&mut`-never-mutated). These run the real `analyze_with` over a tiny
//! Cargo fixture project with a live rust-analyzer, so they're `#[ignore]` by
//! default (slow: rust-analyzer indexes the project on first query) and need the
//! server binary. Run with:
//!
//!   AST_CONTEXT_LSP_RA="$(rustc --print sysroot)/bin/rust-analyzer" \
//!     cargo test --features lsp -- --ignored lsp_semantic_checks
#![cfg(feature = "lsp")]

use std::path::PathBuf;

use ast_context::analysis::{self, AnalysisConfig, FindingKind, LspProvider};
use ast_context::GraphBuilder;

fn ra_program() -> String {
    std::env::var("AST_CONTEXT_LSP_RA")
        .or_else(|_| std::env::var("AST_CONTEXT_RUST_ANALYZER"))
        .unwrap_or_else(|_| "rust-analyzer".into())
}

#[test]
#[ignore]
fn lsp_semantic_checks() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/lsp_project");
    let graph =
        GraphBuilder::build_full_with_options(&dir, true, &[], None, false).expect("graph builds");
    let provider =
        LspProvider::start_with(&ra_program(), &dir).expect("rust-analyzer should start");

    let findings = analysis::analyze_with(&graph, &AnalysisConfig::default(), &provider);

    let visibility: Vec<&str> = findings
        .iter()
        .filter_map(|f| match &f.kind {
            FindingKind::VisibilityTooBroad { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    let mut_ref: Vec<(&str, &str)> = findings
        .iter()
        .filter_map(|f| match &f.kind {
            FindingKind::UnnecessaryMutRef { function_name, param_name } => {
                Some((function_name.as_str(), param_name.as_str()))
            }
            _ => None,
        })
        .collect();

    // ── Visibility tightening ──────────────────────────────────────────────
    assert!(
        visibility.contains(&"file_local_helper"),
        "a pub fn used only within its own file should be flagged; got {visibility:?}"
    );
    assert!(
        !visibility.contains(&"cross_file_helper"),
        "a pub fn used from another file must NOT be flagged; got {visibility:?}"
    );

    // ── `&mut` never mutated ───────────────────────────────────────────────
    assert!(
        mut_ref.iter().any(|&(f, p)| f == "count_items" && p == "v"),
        "a &mut param only read (.len() is &self) should be flagged; got {mut_ref:?}"
    );
    assert!(
        !mut_ref.iter().any(|&(f, _)| f == "add_item"),
        "a &mut param mutated via .push() (&mut self) must NOT be flagged; got {mut_ref:?}"
    );
    assert!(
        !mut_ref.iter().any(|&(f, _)| f == "reset_value"),
        "a &mut param mutated via `*p = 0` must NOT be flagged; got {mut_ref:?}"
    );
}
