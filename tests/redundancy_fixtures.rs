//! Table-driven fixture tests for the redundancy analysis engine.
//!
//! Each case points at a directory under `tests/fixtures/redundancy/` and asserts
//! that `redundancy::analyze` does (positive) or does not (negative) report a
//! given finding kind. Tests run against `analyze()` directly, which is
//! deterministic — the random sampling lives only in the CLI/MCP presentation
//! layer, so it doesn't affect these assertions.
//!
//! To add coverage: drop a fixture dir with a `mod.<ext>` source file and add a
//! row to `CASES`. Negative cases are the important ones — they pin the
//! false-positive behavior so a future heuristic change can't silently regress
//! precision.

use std::collections::HashSet;
use std::path::PathBuf;

use ast_context::redundancy::{self, AnalysisConfig, Finding, FindingKind};
use ast_context::GraphBuilder;

/// Build an annotated graph for a fixture dir and run the full analysis.
fn analyze_fixture(rel_dir: &str) -> Vec<Finding> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/redundancy")
        .join(rel_dir);
    assert!(dir.exists(), "fixture dir missing: {}", dir.display());

    // annotate = true so source snippets and structural fingerprints exist.
    let graph = GraphBuilder::build_full_with_options(&dir, true, &[], None, false)
        .expect("graph build should succeed");
    redundancy::analyze(&graph, &AnalysisConfig::default())
}

fn count<F: Fn(&FindingKind) -> bool>(findings: &[Finding], pred: F) -> usize {
    findings.iter().filter(|f| pred(&f.kind)).count()
}

/// (fixture dir, predicate matching the finding kind, expect_present).
type Case = (&'static str, fn(&FindingKind) -> bool, bool);

fn is_near_duplicate(k: &FindingKind) -> bool {
    matches!(k, FindingKind::NearDuplicate { .. })
}
fn is_passthrough(k: &FindingKind) -> bool {
    matches!(k, FindingKind::Passthrough { .. })
}

const CASES: &[Case] = &[
    ("near_duplicate_positive", is_near_duplicate, true),
    ("near_duplicate_negative", is_near_duplicate, false),
    ("passthrough_positive", is_passthrough, true),
    ("passthrough_negative", is_passthrough, false),
];

#[test]
fn redundancy_fixture_cases() {
    let mut failures = Vec::new();

    for &(dir, pred, expect_present) in CASES {
        let findings = analyze_fixture(dir);
        let hits = count(&findings, pred);
        let ok = if expect_present { hits >= 1 } else { hits == 0 };
        if !ok {
            failures.push(format!(
                "  {dir}: expected finding {} but got {hits} hit(s)",
                if expect_present { "present" } else { "absent" },
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "redundancy fixture expectations failed:\n{}",
        failures.join("\n")
    );
}

/// Regression guard for the structural confirmation gate, locking in the
/// calibration result (see `examples/calibrate_threshold.rs`): at the default
/// threshold the genuine `dup*` clusters are flagged and every `trap_*` lexical
/// false positive is rejected.
#[test]
fn calibration_gate_keeps_precision_and_recall() {
    let findings = analyze_fixture("calibration");

    // Collect the name groups reported by the two lexical-similarity checks.
    let groups: Vec<&Vec<String>> = findings
        .iter()
        .filter_map(|f| match &f.kind {
            FindingKind::NearDuplicate { names, .. }
            | FindingKind::StructurallySimilar { names, .. } => Some(names),
            _ => None,
        })
        .collect();

    let flagged: HashSet<&str> = groups
        .iter()
        .flat_map(|g| g.iter().map(String::as_str))
        .collect();

    // Precision: no trap function may be flagged as similar to anything.
    let trapped: Vec<&str> = flagged.iter().copied().filter(|n| n.starts_with("trap_")).collect();
    assert!(
        trapped.is_empty(),
        "structural gate let through false positives: {trapped:?}"
    );

    // Recall: each genuine cluster must be caught (a group with ≥2 of its members).
    for cluster in ["dupA", "dupB", "dupC"] {
        let caught = groups
            .iter()
            .any(|g| g.iter().filter(|n| n.starts_with(cluster)).count() >= 2);
        assert!(caught, "genuine cluster {cluster} was not flagged");
    }
}
