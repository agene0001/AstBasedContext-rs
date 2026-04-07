//! Tiered redundancy and architecture analysis for code graphs.
//!
//! Produces a ranked list of findings from Critical → Low across 102 checks spanning
//! function redundancy, struct/enum overlap, design patterns, anti-patterns, type system,
//! metrics, risk scores, test coverage, blast radius, API surface, cross-language boundaries,
//! configuration detection, and data structure usage suggestions.
//!
//! Requires `--annotate` for source-level checks.

mod context;
mod helpers;
mod types;

mod anti_patterns;
mod api_surface;
mod code_quality;
mod data_structures;
mod blast_radius;
mod config_detection;
mod cross_language;
mod design_patterns;
mod function_checks;
mod metrics;
mod pattern_detection;
mod risk;
mod struct_enum;
mod structural;
mod testing;
mod type_suggestions;
mod type_system;

pub use types::{AnalysisConfig, Finding, FindingKind, Tier};

use context::AnalysisContext;
use crate::graph::CodeGraph;

/// Run the full tiered redundancy analysis on a code graph.
///
/// Returns findings sorted by tier (Critical first, Low last).
pub fn analyze(graph: &CodeGraph, config: &AnalysisConfig) -> Vec<Finding> {
    let mut findings = Vec::new();
    let ctx = AnalysisContext::build(graph, config);

    // ── Check 1: Passthrough wrappers (Critical / High) ─────────────────
    function_checks::find_passthroughs(&ctx, &mut findings);

    // ── Check 2: Near-duplicates (Critical / High) ──────────────────────
    function_checks::find_near_duplicates(&ctx, &mut findings);

    // ── Check 3: Structural similarity (Medium) ─────────────────────────
    function_checks::find_structural_similar(&ctx, &mut findings);

    // ── Check 4: Merge candidates (Medium / Low) ────────────────────────
    function_checks::find_merge_candidates(&ctx, &mut findings);

    // ── Check 5: Split candidates (Medium / Low) ────────────────────────
    function_checks::find_split_candidates(&ctx, &mut findings);

    // ── Check 6: Overlapping structs (High / Medium) ────────────────────
    struct_enum::find_overlapping_structs(&ctx, &mut findings);

    // ── Check 7: Overlapping enums (High / Medium) ──────────────────────
    struct_enum::find_overlapping_enums(&ctx, &mut findings);

    // ── Check 8: Suggest parameter structs (Medium / Low) ───────────────
    type_suggestions::suggest_parameter_structs(&ctx, &mut findings);

    // ── Check 9: Suggest enum dispatch (Low) ────────────────────────────
    type_suggestions::suggest_enum_dispatch(&ctx, &mut findings);

    // ── Check 10: Suggest trait extraction (Medium / Low) ───────────────
    type_suggestions::suggest_trait_extraction(&ctx, &mut findings);

    // ── Architecture pattern suggestions ─────────────────────────────────

    // ── Check 11: Suggest facade (High / Medium) ─────────────────────────
    design_patterns::suggest_facade(&ctx, &mut findings);

    // ── Check 12: Suggest factory (High / Medium) ────────────────────────
    design_patterns::suggest_factory(&ctx, &mut findings);

    // ── Check 13: Suggest builder (High / Medium) ────────────────────────
    design_patterns::suggest_builder(&ctx, &mut findings);

    // ── Check 14: Suggest strategy (Medium) ──────────────────────────────
    design_patterns::suggest_strategy(&ctx, &mut findings);

    // ── Check 15: Suggest template method (Medium) ───────────────────────
    design_patterns::suggest_template_method(&ctx, &mut findings);

    // ── Check 16: Suggest observer (Medium / Low) ────────────────────────
    design_patterns::suggest_observer(&ctx, &mut findings);

    // ── Check 17: Suggest decorator (Low) ────────────────────────────────
    design_patterns::suggest_decorator(&ctx, &mut findings);

    // ── Check 18: Suggest mediator (Low) ─────────────────────────────────
    design_patterns::suggest_mediator(&ctx, &mut findings);

    // ── Anti-pattern detection ───────────────────────────────────────────

    // ── Check 19: God class/module (High / Medium) ───────────────────────
    anti_patterns::detect_god_class(&ctx, &mut findings);

    // ── Check 20: Circular dependencies (High) ──────────────────────────
    anti_patterns::detect_circular_dependencies(&ctx, &mut findings);

    // ── Check 21: Feature envy (Medium) ──────────────────────────────────
    anti_patterns::detect_feature_envy(&ctx, &mut findings);

    // ── Check 22: Shotgun surgery (Medium / Low) ─────────────────────────
    anti_patterns::detect_shotgun_surgery(&ctx, &mut findings);

    // ── Pattern detection (type/visibility enrichment) ───────────────────

    // ── Check 23: Singleton (Medium) ─────────────────────────────────────
    pattern_detection::detect_singleton(&ctx, &mut findings);

    // ── Check 24: Adapter (Medium) ───────────────────────────────────────
    pattern_detection::detect_adapter(&ctx, &mut findings);

    // ── Check 25: Proxy (Medium) ─────────────────────────────────────────
    pattern_detection::detect_proxy(&ctx, &mut findings);

    // ── Check 26: Command (Medium) ───────────────────────────────────────
    pattern_detection::detect_command(&ctx, &mut findings);

    // ── Check 27: Chain of Responsibility (Medium) ───────────────────────
    pattern_detection::detect_chain_of_responsibility(&ctx, &mut findings);

    // ── Check 28: Dependency Injection (Medium / Low) ────────────────────
    pattern_detection::detect_dependency_injection(&ctx, &mut findings);

    // ── Additional anti-patterns ─────────────────────────────────────────

    // ── Check 29: Dead code (Critical) ────────────────────────────────────
    anti_patterns::detect_dead_code(&ctx, &mut findings);

    // ── Check 30: Long parameter list (High) ──────────────────────────────
    anti_patterns::detect_long_parameter_list(&ctx, &mut findings);

    // ── Check 31: Data clumps (High) ──────────────────────────────────────
    anti_patterns::detect_data_clumps(&ctx, &mut findings);

    // ── Check 32: Middle man (Medium) ─────────────────────────────────────
    anti_patterns::detect_middle_man(&ctx, &mut findings);

    // ── Check 33: Lazy class (Medium) ─────────────────────────────────────
    anti_patterns::detect_lazy_class(&ctx, &mut findings);

    // ── Check 34: Refused bequest (Medium) ────────────────────────────────
    anti_patterns::detect_refused_bequest(&ctx, &mut findings);

    // ── Check 35: Speculative generality (Medium) ─────────────────────────
    anti_patterns::detect_speculative_generality(&ctx, &mut findings);

    // ── Check 36: Inappropriate intimacy (Low) ────────────────────────────
    anti_patterns::detect_inappropriate_intimacy(&ctx, &mut findings);

    // ── Check 37: Deep nesting (Medium) ───────────────────────────────────
    anti_patterns::detect_deep_nesting(&ctx, &mut findings);

    // ── Additional pattern detection ─────────────────────────────────────

    // ── Check 38: Visitor pattern (Medium) ────────────────────────────────
    pattern_detection::detect_visitor(&ctx, &mut findings);

    // ── Check 39: Iterator pattern (Medium) ───────────────────────────────
    pattern_detection::detect_iterator(&ctx, &mut findings);

    // ── Check 40: State pattern (Medium) ──────────────────────────────────
    pattern_detection::detect_state(&ctx, &mut findings);

    // ── Check 41: Composite pattern (Medium) ──────────────────────────────
    pattern_detection::detect_composite(&ctx, &mut findings);

    // ── Check 42: Repository pattern (Medium) ─────────────────────────────
    pattern_detection::detect_repository(&ctx, &mut findings);

    // ── Check 43: Prototype pattern (Medium) ──────────────────────────────
    pattern_detection::detect_prototype(&ctx, &mut findings);

    // ── Structural / architecture quality ────────────────────────────────

    // ── Check 44: Hub module (Medium) ─────────────────────────────────────
    structural::detect_hub_module(&ctx, &mut findings);

    // ── Check 45: Orphan module (Low) ─────────────────────────────────────
    structural::detect_orphan_module(&ctx, &mut findings);

    // ── Additional anti-patterns (batch 2) ───────────────────────────────

    // ── Check 46: Divergent change (Medium) ───────────────────────────────
    anti_patterns::detect_divergent_change(&ctx, &mut findings);

    // ── Check 47: Parallel inheritance (Low) ──────────────────────────────
    anti_patterns::detect_parallel_inheritance(&ctx, &mut findings);

    // ── Check 48: Primitive obsession (Medium) ────────────────────────────
    anti_patterns::detect_primitive_obsession(&ctx, &mut findings);

    // ── Check 49: Large class (High) ──────────────────────────────────────
    anti_patterns::detect_large_class(&ctx, &mut findings);

    // ── Check 50: Unstable dependency (Low) ───────────────────────────────
    anti_patterns::detect_unstable_dependency(&ctx, &mut findings);

    // ── Additional pattern detection (batch 2) ───────────────────────────

    // ── Check 51: Flyweight (Medium) ──────────────────────────────────────
    pattern_detection::detect_flyweight(&ctx, &mut findings);

    // ── Check 52: Event emitter / observer (Medium) ───────────────────────
    pattern_detection::detect_event_emitter(&ctx, &mut findings);

    // ── Check 53: Memento (Medium) ────────────────────────────────────────
    pattern_detection::detect_memento(&ctx, &mut findings);

    // ── Check 54: Fluent builder (Medium) ─────────────────────────────────
    pattern_detection::detect_fluent_builder(&ctx, &mut findings);

    // ── Check 55: Null object (Medium) ────────────────────────────────────
    pattern_detection::detect_null_object(&ctx, &mut findings);

    // ── Structural quality (batch 2) ─────────────────────────────────────

    // ── Check 56: Inconsistent naming (Low) ───────────────────────────────
    structural::detect_inconsistent_naming(&ctx, &mut findings);

    // ── Check 57: Circular package dependency (High) ──────────────────────
    structural::detect_circular_package_dependency(&ctx, &mut findings);

    // ── Type system suggestions ──────────────────────────────────────────

    // ── Check 58: Tagged union / suggest sum type (High) ──────────────────
    type_system::detect_tagged_union(&ctx, &mut findings);

    // ── Check 59: Class hierarchy → enum (Medium) ─────────────────────────
    type_system::detect_hierarchy_to_enum(&ctx, &mut findings);

    // ── Check 60: Boolean blindness (Medium) ──────────────────────────────
    type_system::detect_boolean_blindness(&ctx, &mut findings);

    // ── Check 61: Suggest newtype (Low) ───────────────────────────────────
    type_system::detect_suggest_newtype(&ctx, &mut findings);

    // ── Check 62: Suggest sealed type (Medium) ────────────────────────────
    type_system::detect_suggest_sealed_type(&ctx, &mut findings);

    // ── Check 63: Large product type (High) ───────────────────────────────
    type_system::detect_large_product_type(&ctx, &mut findings);

    // ── Additional anti-patterns (batch 3) ───────────────────────────────

    // ── Check 64: Anemic domain model (Medium) ───────────────────────────
    anti_patterns::detect_anemic_domain_model(&ctx, &mut findings);

    // ── Check 65: Magic numbers/strings (Low) ────────────────────────────
    anti_patterns::detect_magic_numbers(&ctx, &mut findings);

    // ── Check 66: Mutable global state (High) ────────────────────────────
    anti_patterns::detect_mutable_global_state(&ctx, &mut findings);

    // ── Check 67: Empty catch (High) ─────────────────────────────────────
    anti_patterns::detect_empty_catch(&ctx, &mut findings);

    // ── Check 68: Callback hell (Medium) ─────────────────────────────────
    anti_patterns::detect_callback_hell(&ctx, &mut findings);

    // ── Check 69: API inconsistency (Low) ────────────────────────────────
    anti_patterns::detect_api_inconsistency(&ctx, &mut findings);

    // ── Metrics ──────────────────────────────────────────────────────────

    // ── Check 70: Lack of cohesion (Medium) ──────────────────────────────
    metrics::detect_lack_of_cohesion(&ctx, &mut findings);

    // ── Check 71: High coupling (Medium) ─────────────────────────────────
    metrics::detect_high_coupling(&ctx, &mut findings);

    // ── Check 72: Module instability (Low) ───────────────────────────────
    metrics::detect_module_instability(&ctx, &mut findings);

    // ── Check 73: Cognitive complexity (Medium) ──────────────────────────
    metrics::detect_cognitive_complexity(&ctx, &mut findings);

    // ── Composite Risk Score ─────────────────────────────────────────

    // ── Check 74: High risk function (High) ──────────────────────────
    risk::detect_high_risk_functions(&ctx, &mut findings);

    // ── Check 75: High risk file (Medium) ────────────────────────────
    risk::detect_high_risk_files(&ctx, &mut findings);

    // ── Test Coverage Gaps ───────────────────────────────────────────

    // ── Check 76: Untested public function (High) ────────────────────
    testing::detect_untested_public_functions(&ctx, &mut findings);

    // ── Check 77: Low test ratio (Medium) ────────────────────────────
    testing::detect_low_test_ratio(&ctx, &mut findings);

    // ── Check 78: Integration test smell (Low) ───────────────────────
    testing::detect_integration_test_smells(&ctx, &mut findings);

    // ── Change Blast Radius ──────────────────────────────────────────

    // ── Check 79: High blast radius (High) ───────────────────────────
    blast_radius::detect_high_blast_radius(&ctx, &mut findings);

    // ── Semantic Clustering ──────────────────────────────────────────

    // ── Check 80: Misplaced function (Medium) ────────────────────────
    blast_radius::detect_misplaced_functions(&ctx, &mut findings);

    // ── Check 81: Implicit module (Low) ──────────────────────────────
    blast_radius::detect_implicit_modules(&ctx, &mut findings);

    // ── API Surface Analysis ─────────────────────────────────────────

    // ── Check 82: Unstable public API (High) ─────────────────────────
    api_surface::detect_unstable_public_api(&ctx, &mut findings);

    // ── Check 83: Undocumented public API (Medium) ───────────────────
    api_surface::detect_undocumented_public_api(&ctx, &mut findings);

    // ── Check 84: Leaky abstraction (High) ───────────────────────────
    api_surface::detect_leaky_abstraction(&ctx, &mut findings);

    // ── Cross-Language Boundaries ────────────────────────────────────

    // ── Check 85: FFI boundary (Medium) ──────────────────────────────
    cross_language::detect_ffi_boundary(&ctx, &mut findings);

    // ── Check 86: Subprocess call (Medium) ───────────────────────────
    cross_language::detect_subprocess_calls(&ctx, &mut findings);

    // ── Check 87: IPC/RPC boundary (Medium) ──────────────────────────
    cross_language::detect_ipc_boundary(&ctx, &mut findings);

    // ── Configuration Detection ──────────────────────────────────────

    // ── Check 88: Environment variable usage (Low) ───────────────────
    config_detection::detect_env_var_usage(&ctx, &mut findings);

    // ── Check 89: Hardcoded endpoint (Medium) ────────────────────────
    config_detection::detect_hardcoded_endpoints(&ctx, &mut findings);

    // ── Check 90: Feature flag (Low) ─────────────────────────────────
    config_detection::detect_feature_flags(&ctx, &mut findings);

    // ── Check 91: Config file usage (Low) ────────────────────────────
    config_detection::detect_config_file_usage(&ctx, &mut findings);

    // ── Data Structure Usage Suggestions ──────────────────────────────

    // ── Check 92: Vec used as set (Medium) ────────────────────────────
    data_structures::detect_vec_used_as_set(&ctx, &mut findings);

    // ── Check 93: Vec used as map (Medium) ────────────────────────────
    data_structures::detect_vec_used_as_map(&ctx, &mut findings);

    // ── Check 94: Linear search in loop (High) ───────────────────────
    data_structures::detect_linear_search_in_loop(&ctx, &mut findings);

    // ── Check 95: String concat in loop (Medium) ─────────────────────
    data_structures::detect_string_concat_in_loop(&ctx, &mut findings);

    // ── Check 96: Sorted Vec for lookup (Low) ────────────────────────
    data_structures::detect_sorted_vec_for_lookup(&ctx, &mut findings);

    // ── Check 97: Nested loop lookup (High) ──────────────────────────
    data_structures::detect_nested_loop_lookup(&ctx, &mut findings);

    // ── Check 98: HashMap with sequential keys (Low) ─────────────────
    data_structures::detect_hashmap_sequential_keys(&ctx, &mut findings);

    // ── Check 99: Excessive collect-iterate (High) ───────────────────
    data_structures::detect_excessive_collect_iterate(&ctx, &mut findings);

    // ── Code Quality ──────────────────────────────────────────────────

    // ── Check 100: Unused imports (Low) ────────────────────────────────
    code_quality::detect_unused_imports(&ctx, &mut findings);

    // ── Check 101: Inconsistent error handling (Low) ───────────────────
    code_quality::detect_inconsistent_error_handling(&ctx, &mut findings);

    // ── Check 102: Tech debt comments (Medium / Low) ───────────────────
    code_quality::detect_tech_debt_comments(&ctx, &mut findings);

    // Sort: Critical first, then High, Medium, Low
    findings.sort_by_key(|f| f.tier);
    findings
}
