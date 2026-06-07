//! Tiered redundancy and architecture analysis for code graphs.
//!
//! Produces a ranked list of findings from Critical → Low across 132 checks spanning
//! function redundancy, struct/enum overlap, design patterns, anti-patterns, type system,
//! metrics, risk scores, test coverage, blast radius, API surface, cross-language boundaries,
//! configuration detection, data structure usage, and optimization suggestions.
//!
//! Requires `--annotate` for source-level checks.

mod context;
mod helpers;
mod types;

// Flat (cross-cutting) categories that don't group under a single theme.
mod api_surface;
mod blast_radius;
mod code_quality;
mod config_detection;
mod cross_language;
mod metrics;
mod risk;
mod structural;
mod testing;
mod type_system;

// Themed check groups. The submodules are re-imported below so the orchestrator
// call sites (`function_checks::…`, `anti_patterns::…`, `data_structures::…`)
// stay unchanged.
mod optimization;
mod patterns;
mod redundancy;

use optimization::data_structures;
use patterns::{anti_patterns, design_patterns, pattern_detection};
use redundancy::{function_checks, struct_enum, type_suggestions};

pub use types::{AnalysisConfig, Finding, FindingKind, Tier};

use crate::graph::CodeGraph;
use context::AnalysisContext;

/// Run the full tiered redundancy analysis on a code graph.
///
/// Returns findings sorted by tier (Critical first, Low last).
pub fn analyze(graph: &CodeGraph, config: &AnalysisConfig) -> Vec<Finding> {
    let mut findings = Vec::new();
    let ctx = AnalysisContext::build(graph, config);

    let skip = |name: &str| config.skip_checks.iter().any(|c| c == name);

    // When a single category is requested, skip whole check groups up front
    // rather than running every check and filtering the results afterwards.
    // Each module alias used in the guards below maps 1:1 to a finding category:
    // `function_checks` emits the "redundancy" category; every other module's
    // alias *is* its category. (The post-pass `retain` below is kept as a
    // safety net in case a check ever emits a finding outside its category.)
    let cat_ok = |alias: &str| {
        config.category.as_deref().is_none_or(|want| {
            want == if alias == "function_checks" {
                "redundancy"
            } else {
                alias
            }
        })
    };

    // ── Check 1: Passthrough wrappers (Critical / High) ─────────────────
    if cat_ok("function_checks") && !skip("function_checks") && !skip("find_passthroughs") {
        function_checks::find_passthroughs(&ctx, &mut findings);
    }

    // ── Check 2: Near-duplicates (Critical / High) ──────────────────────
    if cat_ok("function_checks") && !skip("function_checks") && !skip("find_near_duplicates") {
        function_checks::find_near_duplicates(&ctx, &mut findings);
    }

    // ── Check 3: Structural similarity (Medium) ─────────────────────────
    if cat_ok("function_checks") && !skip("function_checks") && !skip("find_structural_similar") {
        function_checks::find_structural_similar(&ctx, &mut findings);
    }

    // ── Check 4: Merge candidates (Medium / Low) ────────────────────────
    if cat_ok("function_checks") && !skip("function_checks") && !skip("find_merge_candidates") {
        function_checks::find_merge_candidates(&ctx, &mut findings);
    }

    // ── Check 5: Split candidates (Medium / Low) ────────────────────────
    if cat_ok("function_checks") && !skip("function_checks") && !skip("find_split_candidates") {
        function_checks::find_split_candidates(&ctx, &mut findings);
    }

    // ── Check 6: Overlapping structs (High / Medium) ────────────────────
    if cat_ok("struct_enum") && !skip("struct_enum") && !skip("find_overlapping_structs") {
        struct_enum::find_overlapping_structs(&ctx, &mut findings);
    }

    // ── Check 7: Overlapping enums (High / Medium) ──────────────────────
    if cat_ok("struct_enum") && !skip("struct_enum") && !skip("find_overlapping_enums") {
        struct_enum::find_overlapping_enums(&ctx, &mut findings);
    }

    // ── Check 8: Suggest parameter structs (Medium / Low) ───────────────
    if cat_ok("type_suggestions") && !skip("type_suggestions") && !skip("suggest_parameter_structs") {
        type_suggestions::suggest_parameter_structs(&ctx, &mut findings);
    }

    // ── Check 9: Suggest enum dispatch (Low) ────────────────────────────
    if cat_ok("type_suggestions") && !skip("type_suggestions") && !skip("suggest_enum_dispatch") {
        type_suggestions::suggest_enum_dispatch(&ctx, &mut findings);
    }

    // ── Check 10: Suggest trait extraction (Medium / Low) ───────────────
    if cat_ok("type_suggestions") && !skip("type_suggestions") && !skip("suggest_trait_extraction") {
        type_suggestions::suggest_trait_extraction(&ctx, &mut findings);
    }

    // ── Architecture pattern suggestions ─────────────────────────────────

    // ── Check 11: Suggest facade (High / Medium) ─────────────────────────
    if cat_ok("design_patterns") && !skip("design_patterns") && !skip("suggest_facade") {
        design_patterns::suggest_facade(&ctx, &mut findings);
    }

    // ── Check 12: Suggest factory (High / Medium) ────────────────────────
    if cat_ok("design_patterns") && !skip("design_patterns") && !skip("suggest_factory") {
        design_patterns::suggest_factory(&ctx, &mut findings);
    }

    // ── Check 13: Suggest builder (High / Medium) ────────────────────────
    if cat_ok("design_patterns") && !skip("design_patterns") && !skip("suggest_builder") {
        design_patterns::suggest_builder(&ctx, &mut findings);
    }

    // ── Check 14: Suggest strategy (Medium) ──────────────────────────────
    if cat_ok("design_patterns") && !skip("design_patterns") && !skip("suggest_strategy") {
        design_patterns::suggest_strategy(&ctx, &mut findings);
    }

    // ── Check 15: Suggest template method (Medium) ───────────────────────
    if cat_ok("design_patterns") && !skip("design_patterns") && !skip("suggest_template_method") {
        design_patterns::suggest_template_method(&ctx, &mut findings);
    }

    // ── Check 16: Suggest observer (Medium / Low) ────────────────────────
    if cat_ok("design_patterns") && !skip("design_patterns") && !skip("suggest_observer") {
        design_patterns::suggest_observer(&ctx, &mut findings);
    }

    // ── Check 17: Suggest decorator (Low) ────────────────────────────────
    if cat_ok("design_patterns") && !skip("design_patterns") && !skip("suggest_decorator") {
        design_patterns::suggest_decorator(&ctx, &mut findings);
    }

    // ── Check 18: Suggest mediator (Low) ─────────────────────────────────
    if cat_ok("design_patterns") && !skip("design_patterns") && !skip("suggest_mediator") {
        design_patterns::suggest_mediator(&ctx, &mut findings);
    }

    // ── Anti-pattern detection ───────────────────────────────────────────

    // ── Check 19: God class/module (High / Medium) ───────────────────────
    if cat_ok("anti_patterns") && !skip("anti_patterns") && !skip("detect_god_class") {
        anti_patterns::detect_god_class(&ctx, &mut findings);
    }

    // ── Check 20: Circular dependencies (High) ──────────────────────────
    if cat_ok("anti_patterns") && !skip("anti_patterns") && !skip("detect_circular_dependencies") {
        anti_patterns::detect_circular_dependencies(&ctx, &mut findings);
    }

    // ── Check 21: Feature envy (Medium) ──────────────────────────────────
    if cat_ok("anti_patterns") && !skip("anti_patterns") && !skip("detect_feature_envy") {
        anti_patterns::detect_feature_envy(&ctx, &mut findings);
    }

    // ── Check 22: Shotgun surgery (Medium / Low) ─────────────────────────
    if cat_ok("anti_patterns") && !skip("anti_patterns") && !skip("detect_shotgun_surgery") {
        anti_patterns::detect_shotgun_surgery(&ctx, &mut findings);
    }

    // ── Pattern detection (type/visibility enrichment) ───────────────────

    // ── Check 23: Singleton (Medium) ─────────────────────────────────────
    if cat_ok("pattern_detection") && !skip("pattern_detection") && !skip("detect_singleton") {
        pattern_detection::detect_singleton(&ctx, &mut findings);
    }

    // ── Check 24: Adapter (Medium) ───────────────────────────────────────
    if cat_ok("pattern_detection") && !skip("pattern_detection") && !skip("detect_adapter") {
        pattern_detection::detect_adapter(&ctx, &mut findings);
    }

    // ── Check 25: Proxy (Medium) ─────────────────────────────────────────
    if cat_ok("pattern_detection") && !skip("pattern_detection") && !skip("detect_proxy") {
        pattern_detection::detect_proxy(&ctx, &mut findings);
    }

    // ── Check 26: Command (Medium) ───────────────────────────────────────
    if cat_ok("pattern_detection") && !skip("pattern_detection") && !skip("detect_command") {
        pattern_detection::detect_command(&ctx, &mut findings);
    }

    // ── Check 27: Chain of Responsibility (Medium) ───────────────────────
    if cat_ok("pattern_detection") && !skip("pattern_detection") && !skip("detect_chain_of_responsibility") {
        pattern_detection::detect_chain_of_responsibility(&ctx, &mut findings);
    }

    // ── Check 28: Dependency Injection (Medium / Low) ────────────────────
    if cat_ok("pattern_detection") && !skip("pattern_detection") && !skip("detect_dependency_injection") {
        pattern_detection::detect_dependency_injection(&ctx, &mut findings);
    }

    // ── Additional anti-patterns ─────────────────────────────────────────

    // ── Check 29: Dead code (Critical) ────────────────────────────────────
    if cat_ok("anti_patterns") && !skip("anti_patterns") && !skip("detect_dead_code") {
        anti_patterns::detect_dead_code(&ctx, &mut findings);
    }

    // ── Check 30: Long parameter list (High) ──────────────────────────────
    if cat_ok("anti_patterns") && !skip("anti_patterns") && !skip("detect_long_parameter_list") {
        anti_patterns::detect_long_parameter_list(&ctx, &mut findings);
    }

    // ── Check 31: Data clumps (High) ──────────────────────────────────────
    if cat_ok("anti_patterns") && !skip("anti_patterns") && !skip("detect_data_clumps") {
        anti_patterns::detect_data_clumps(&ctx, &mut findings);
    }

    // ── Check 32: Middle man (Medium) ─────────────────────────────────────
    if cat_ok("anti_patterns") && !skip("anti_patterns") && !skip("detect_middle_man") {
        anti_patterns::detect_middle_man(&ctx, &mut findings);
    }

    // ── Check 33: Lazy class (Medium) ─────────────────────────────────────
    if cat_ok("anti_patterns") && !skip("anti_patterns") && !skip("detect_lazy_class") {
        anti_patterns::detect_lazy_class(&ctx, &mut findings);
    }

    // ── Check 34: Refused bequest (Medium) ────────────────────────────────
    if cat_ok("anti_patterns") && !skip("anti_patterns") && !skip("detect_refused_bequest") {
        anti_patterns::detect_refused_bequest(&ctx, &mut findings);
    }

    // ── Check 35: Speculative generality (Medium) ─────────────────────────
    if cat_ok("anti_patterns") && !skip("anti_patterns") && !skip("detect_speculative_generality") {
        anti_patterns::detect_speculative_generality(&ctx, &mut findings);
    }

    // ── Check 36: Inappropriate intimacy (Low) ────────────────────────────
    if cat_ok("anti_patterns") && !skip("anti_patterns") && !skip("detect_inappropriate_intimacy") {
        anti_patterns::detect_inappropriate_intimacy(&ctx, &mut findings);
    }

    // ── Check 37: Deep nesting (Medium) ───────────────────────────────────
    if cat_ok("anti_patterns") && !skip("anti_patterns") && !skip("detect_deep_nesting") {
        anti_patterns::detect_deep_nesting(&ctx, &mut findings);
    }

    // ── Additional pattern detection ─────────────────────────────────────

    // ── Check 38: Visitor pattern (Medium) ────────────────────────────────
    if cat_ok("pattern_detection") && !skip("pattern_detection") && !skip("detect_visitor") {
        pattern_detection::detect_visitor(&ctx, &mut findings);
    }

    // ── Check 39: Iterator pattern (Medium) ───────────────────────────────
    if cat_ok("pattern_detection") && !skip("pattern_detection") && !skip("detect_iterator") {
        pattern_detection::detect_iterator(&ctx, &mut findings);
    }

    // ── Check 40: State pattern (Medium) ──────────────────────────────────
    if cat_ok("pattern_detection") && !skip("pattern_detection") && !skip("detect_state") {
        pattern_detection::detect_state(&ctx, &mut findings);
    }

    // ── Check 41: Composite pattern (Medium) ──────────────────────────────
    if cat_ok("pattern_detection") && !skip("pattern_detection") && !skip("detect_composite") {
        pattern_detection::detect_composite(&ctx, &mut findings);
    }

    // ── Check 42: Repository pattern (Medium) ─────────────────────────────
    if cat_ok("pattern_detection") && !skip("pattern_detection") && !skip("detect_repository") {
        pattern_detection::detect_repository(&ctx, &mut findings);
    }

    // ── Check 43: Prototype pattern (Medium) ──────────────────────────────
    if cat_ok("pattern_detection") && !skip("pattern_detection") && !skip("detect_prototype") {
        pattern_detection::detect_prototype(&ctx, &mut findings);
    }

    // ── Structural / architecture quality ────────────────────────────────

    // ── Check 44: Hub module (Medium) ─────────────────────────────────────
    if cat_ok("structural") && !skip("structural") && !skip("detect_hub_module") {
        structural::detect_hub_module(&ctx, &mut findings);
    }

    // ── Check 45: Orphan module (Low) ─────────────────────────────────────
    if cat_ok("structural") && !skip("structural") && !skip("detect_orphan_module") {
        structural::detect_orphan_module(&ctx, &mut findings);
    }

    // ── Additional anti-patterns (batch 2) ───────────────────────────────

    // ── Check 46: Divergent change (Medium) ───────────────────────────────
    if cat_ok("anti_patterns") && !skip("anti_patterns") && !skip("detect_divergent_change") {
        anti_patterns::detect_divergent_change(&ctx, &mut findings);
    }

    // ── Check 47: Parallel inheritance (Low) ──────────────────────────────
    if cat_ok("anti_patterns") && !skip("anti_patterns") && !skip("detect_parallel_inheritance") {
        anti_patterns::detect_parallel_inheritance(&ctx, &mut findings);
    }

    // ── Check 48: Primitive obsession (Medium) ────────────────────────────
    if cat_ok("anti_patterns") && !skip("anti_patterns") && !skip("detect_primitive_obsession") {
        anti_patterns::detect_primitive_obsession(&ctx, &mut findings);
    }

    // ── Check 49: Large class (High) ──────────────────────────────────────
    if cat_ok("anti_patterns") && !skip("anti_patterns") && !skip("detect_large_class") {
        anti_patterns::detect_large_class(&ctx, &mut findings);
    }

    // ── Check 50: Unstable dependency (Low) ───────────────────────────────
    if cat_ok("anti_patterns") && !skip("anti_patterns") && !skip("detect_unstable_dependency") {
        anti_patterns::detect_unstable_dependency(&ctx, &mut findings);
    }

    // ── Additional pattern detection (batch 2) ───────────────────────────

    // ── Check 51: Flyweight (Medium) ──────────────────────────────────────
    if cat_ok("pattern_detection") && !skip("pattern_detection") && !skip("detect_flyweight") {
        pattern_detection::detect_flyweight(&ctx, &mut findings);
    }

    // ── Check 52: Event emitter / observer (Medium) ───────────────────────
    if cat_ok("pattern_detection") && !skip("pattern_detection") && !skip("detect_event_emitter") {
        pattern_detection::detect_event_emitter(&ctx, &mut findings);
    }

    // ── Check 53: Memento (Medium) ────────────────────────────────────────
    if cat_ok("pattern_detection") && !skip("pattern_detection") && !skip("detect_memento") {
        pattern_detection::detect_memento(&ctx, &mut findings);
    }

    // ── Check 54: Fluent builder (Medium) ─────────────────────────────────
    if cat_ok("pattern_detection") && !skip("pattern_detection") && !skip("detect_fluent_builder") {
        pattern_detection::detect_fluent_builder(&ctx, &mut findings);
    }

    // ── Check 55: Null object (Medium) ────────────────────────────────────
    if cat_ok("pattern_detection") && !skip("pattern_detection") && !skip("detect_null_object") {
        pattern_detection::detect_null_object(&ctx, &mut findings);
    }

    // ── Structural quality (batch 2) ─────────────────────────────────────

    // ── Check 56: Inconsistent naming (Low) ───────────────────────────────
    if cat_ok("structural") && !skip("structural") && !skip("detect_inconsistent_naming") {
        structural::detect_inconsistent_naming(&ctx, &mut findings);
    }

    // ── Check 57: Circular package dependency (High) ──────────────────────
    if cat_ok("structural") && !skip("structural") && !skip("detect_circular_package_dependency") {
        structural::detect_circular_package_dependency(&ctx, &mut findings);
    }

    // ── Type system suggestions ──────────────────────────────────────────

    // ── Check 58: Tagged union / suggest sum type (High) ──────────────────
    if cat_ok("type_system") && !skip("type_system") && !skip("detect_tagged_union") {
        type_system::detect_tagged_union(&ctx, &mut findings);
    }

    // ── Check 59: Class hierarchy → enum (Medium) ─────────────────────────
    if cat_ok("type_system") && !skip("type_system") && !skip("detect_hierarchy_to_enum") {
        type_system::detect_hierarchy_to_enum(&ctx, &mut findings);
    }

    // ── Check 60: Boolean blindness (Medium) ──────────────────────────────
    if cat_ok("type_system") && !skip("type_system") && !skip("detect_boolean_blindness") {
        type_system::detect_boolean_blindness(&ctx, &mut findings);
    }

    // ── Check 61: Suggest newtype (Low) ───────────────────────────────────
    if cat_ok("type_system") && !skip("type_system") && !skip("detect_suggest_newtype") {
        type_system::detect_suggest_newtype(&ctx, &mut findings);
    }

    // ── Check 62: Suggest sealed type (Medium) ────────────────────────────
    if cat_ok("type_system") && !skip("type_system") && !skip("detect_suggest_sealed_type") {
        type_system::detect_suggest_sealed_type(&ctx, &mut findings);
    }

    // ── Check 63: Large product type (High) ───────────────────────────────
    if cat_ok("type_system") && !skip("type_system") && !skip("detect_large_product_type") {
        type_system::detect_large_product_type(&ctx, &mut findings);
    }

    // ── Additional anti-patterns (batch 3) ───────────────────────────────

    // ── Check 64: Anemic domain model (Medium) ───────────────────────────
    if cat_ok("anti_patterns") && !skip("anti_patterns") && !skip("detect_anemic_domain_model") {
        anti_patterns::detect_anemic_domain_model(&ctx, &mut findings);
    }

    // ── Check 65: Magic numbers/strings (Low) ────────────────────────────
    if cat_ok("anti_patterns") && !skip("anti_patterns") && !skip("detect_magic_numbers") {
        anti_patterns::detect_magic_numbers(&ctx, &mut findings);
    }

    // ── Check 66: Mutable global state (High) ────────────────────────────
    if cat_ok("anti_patterns") && !skip("anti_patterns") && !skip("detect_mutable_global_state") {
        anti_patterns::detect_mutable_global_state(&ctx, &mut findings);
    }

    // ── Check 67: Empty catch (High) ─────────────────────────────────────
    if cat_ok("anti_patterns") && !skip("anti_patterns") && !skip("detect_empty_catch") {
        anti_patterns::detect_empty_catch(&ctx, &mut findings);
    }

    // ── Check 68: Callback hell (Medium) ─────────────────────────────────
    if cat_ok("anti_patterns") && !skip("anti_patterns") && !skip("detect_callback_hell") {
        anti_patterns::detect_callback_hell(&ctx, &mut findings);
    }

    // ── Check 69: API inconsistency (Low) ────────────────────────────────
    if cat_ok("anti_patterns") && !skip("anti_patterns") && !skip("detect_api_inconsistency") {
        anti_patterns::detect_api_inconsistency(&ctx, &mut findings);
    }

    // ── Metrics ──────────────────────────────────────────────────────────

    // ── Check 70: Lack of cohesion (Medium) ──────────────────────────────
    if cat_ok("metrics") && !skip("metrics") && !skip("detect_lack_of_cohesion") {
        metrics::detect_lack_of_cohesion(&ctx, &mut findings);
    }

    // ── Check 71: High coupling (Medium) ─────────────────────────────────
    if cat_ok("metrics") && !skip("metrics") && !skip("detect_high_coupling") {
        metrics::detect_high_coupling(&ctx, &mut findings);
    }

    // ── Check 72: Module instability (Low) ───────────────────────────────
    if cat_ok("metrics") && !skip("metrics") && !skip("detect_module_instability") {
        metrics::detect_module_instability(&ctx, &mut findings);
    }

    // ── Check 73: Cognitive complexity (Medium) ──────────────────────────
    if cat_ok("metrics") && !skip("metrics") && !skip("detect_cognitive_complexity") {
        metrics::detect_cognitive_complexity(&ctx, &mut findings);
    }

    // ── Composite Risk Score ─────────────────────────────────────────

    // ── Check 74: High risk function (High) ──────────────────────────
    if cat_ok("risk") && !skip("risk") && !skip("detect_high_risk_functions") {
        risk::detect_high_risk_functions(&ctx, &mut findings);
    }

    // ── Check 75: High risk file (Medium) ────────────────────────────
    if cat_ok("risk") && !skip("risk") && !skip("detect_high_risk_files") {
        risk::detect_high_risk_files(&ctx, &mut findings);
    }

    // ── Test Coverage Gaps ───────────────────────────────────────────

    // ── Check 76: Untested public function (High) ────────────────────
    if cat_ok("testing") && !skip("testing") && !skip("detect_untested_public_functions") {
        testing::detect_untested_public_functions(&ctx, &mut findings);
    }

    // ── Check 77: Low test ratio (Medium) ────────────────────────────
    if cat_ok("testing") && !skip("testing") && !skip("detect_low_test_ratio") {
        testing::detect_low_test_ratio(&ctx, &mut findings);
    }

    // ── Check 78: Integration test smell (Low) ───────────────────────
    if cat_ok("testing") && !skip("testing") && !skip("detect_integration_test_smells") {
        testing::detect_integration_test_smells(&ctx, &mut findings);
    }

    // ── Change Blast Radius ──────────────────────────────────────────

    // ── Check 79: High blast radius (High) ───────────────────────────
    if cat_ok("blast_radius") && !skip("blast_radius") && !skip("detect_high_blast_radius") {
        blast_radius::detect_high_blast_radius(&ctx, &mut findings);
    }

    // ── Semantic Clustering ──────────────────────────────────────────

    // ── Check 80: Misplaced function (Medium) ────────────────────────
    if cat_ok("blast_radius") && !skip("blast_radius") && !skip("detect_misplaced_functions") {
        blast_radius::detect_misplaced_functions(&ctx, &mut findings);
    }

    // ── Check 81: Implicit module (Low) ──────────────────────────────
    if cat_ok("blast_radius") && !skip("blast_radius") && !skip("detect_implicit_modules") {
        blast_radius::detect_implicit_modules(&ctx, &mut findings);
    }

    // ── API Surface Analysis ─────────────────────────────────────────

    // ── Check 82: Unstable public API (High) ─────────────────────────
    if cat_ok("api_surface") && !skip("api_surface") && !skip("detect_unstable_public_api") {
        api_surface::detect_unstable_public_api(&ctx, &mut findings);
    }

    // ── Check 83: Undocumented public API (Medium) ───────────────────
    if cat_ok("api_surface") && !skip("api_surface") && !skip("detect_undocumented_public_api") {
        api_surface::detect_undocumented_public_api(&ctx, &mut findings);
    }

    // ── Check 84: Leaky abstraction (High) ───────────────────────────
    if cat_ok("api_surface") && !skip("api_surface") && !skip("detect_leaky_abstraction") {
        api_surface::detect_leaky_abstraction(&ctx, &mut findings);
    }

    // ── Cross-Language Boundaries ────────────────────────────────────

    // ── Check 85: FFI boundary (Medium) ──────────────────────────────
    if cat_ok("cross_language") && !skip("cross_language") && !skip("detect_ffi_boundary") {
        cross_language::detect_ffi_boundary(&ctx, &mut findings);
    }

    // ── Check 86: Subprocess call (Medium) ───────────────────────────
    if cat_ok("cross_language") && !skip("cross_language") && !skip("detect_subprocess_calls") {
        cross_language::detect_subprocess_calls(&ctx, &mut findings);
    }

    // ── Check 87: IPC/RPC boundary (Medium) ──────────────────────────
    if cat_ok("cross_language") && !skip("cross_language") && !skip("detect_ipc_boundary") {
        cross_language::detect_ipc_boundary(&ctx, &mut findings);
    }

    // ── Configuration Detection ──────────────────────────────────────

    // ── Check 88: Environment variable usage (Low) ───────────────────
    if cat_ok("config_detection") && !skip("config_detection") && !skip("detect_env_var_usage") {
        config_detection::detect_env_var_usage(&ctx, &mut findings);
    }

    // ── Check 89: Hardcoded endpoint (Medium) ────────────────────────
    if cat_ok("config_detection") && !skip("config_detection") && !skip("detect_hardcoded_endpoints") {
        config_detection::detect_hardcoded_endpoints(&ctx, &mut findings);
    }

    // ── Check 90: Feature flag (Low) ─────────────────────────────────
    if cat_ok("config_detection") && !skip("config_detection") && !skip("detect_feature_flags") {
        config_detection::detect_feature_flags(&ctx, &mut findings);
    }

    // ── Check 91: Config file usage (Low) ────────────────────────────
    if cat_ok("config_detection") && !skip("config_detection") && !skip("detect_config_file_usage") {
        config_detection::detect_config_file_usage(&ctx, &mut findings);
    }

    // ── Data Structure Usage Suggestions ──────────────────────────────

    // ── Check 92: Vec used as set (Medium) ────────────────────────────
    if cat_ok("data_structures") && !skip("data_structures") && !skip("detect_vec_used_as_set") {
        data_structures::detect_vec_used_as_set(&ctx, &mut findings);
    }

    // ── Check 93: Vec used as map (Medium) ────────────────────────────
    if cat_ok("data_structures") && !skip("data_structures") && !skip("detect_vec_used_as_map") {
        data_structures::detect_vec_used_as_map(&ctx, &mut findings);
    }

    // ── Check 94: Linear search in loop (High) ───────────────────────
    if cat_ok("data_structures") && !skip("data_structures") && !skip("detect_linear_search_in_loop") {
        data_structures::detect_linear_search_in_loop(&ctx, &mut findings);
    }

    // ── Check 95: String concat in loop (Medium) ─────────────────────
    if cat_ok("data_structures") && !skip("data_structures") && !skip("detect_string_concat_in_loop") {
        data_structures::detect_string_concat_in_loop(&ctx, &mut findings);
    }

    // ── Check 96: Sorted Vec for lookup (Low) ────────────────────────
    if cat_ok("data_structures") && !skip("data_structures") && !skip("detect_sorted_vec_for_lookup") {
        data_structures::detect_sorted_vec_for_lookup(&ctx, &mut findings);
    }

    // ── Check 97: Nested loop lookup (High) ──────────────────────────
    if cat_ok("data_structures") && !skip("data_structures") && !skip("detect_nested_loop_lookup") {
        data_structures::detect_nested_loop_lookup(&ctx, &mut findings);
    }

    // ── Check 98: HashMap with sequential keys (Low) ─────────────────
    if cat_ok("data_structures") && !skip("data_structures") && !skip("detect_hashmap_sequential_keys") {
        data_structures::detect_hashmap_sequential_keys(&ctx, &mut findings);
    }

    // ── Check 99: Excessive collect-iterate (High) ───────────────────
    if cat_ok("data_structures") && !skip("data_structures") && !skip("detect_excessive_collect_iterate") {
        data_structures::detect_excessive_collect_iterate(&ctx, &mut findings);
    }

    // ── Code Quality ──────────────────────────────────────────────────

    // ── Check 100: Unused imports (Low) ────────────────────────────────
    if cat_ok("code_quality") && !skip("code_quality") && !skip("detect_unused_imports") {
        code_quality::detect_unused_imports(&ctx, &mut findings);
    }

    // ── Check 101: Inconsistent error handling (Low) ───────────────────
    if cat_ok("code_quality") && !skip("code_quality") && !skip("detect_inconsistent_error_handling") {
        code_quality::detect_inconsistent_error_handling(&ctx, &mut findings);
    }

    // ── Check 102: Tech debt comments (Medium / Low) ───────────────────
    if cat_ok("code_quality") && !skip("code_quality") && !skip("detect_tech_debt_comments") {
        code_quality::detect_tech_debt_comments(&ctx, &mut findings);
    }

    // ── Repeated fully-qualified path → suggest `use` alias (Low) ───────
    if cat_ok("code_quality") && !skip("code_quality") && !skip("detect_repeated_qualified_paths") {
        code_quality::detect_repeated_qualified_paths(&ctx, &mut findings);
    }

    // ── Struct layout / padding (computed, high-precision) ─────────────
    if cat_ok("memory_layout") && !skip("memory_layout") && !skip("detect_struct_layout") {
        optimization::detect_struct_layout(&ctx, &mut findings);
    }

    // ── Optimization Suggestions ──────────────────────────────────────

    // ── Check 103: Clone/allocation in loop (Medium) ──────────────────
    if cat_ok("optimization") && !skip("optimization") && !skip("detect_clone_in_loop") {
        optimization::detect_clone_in_loop(&ctx, &mut findings);
    }

    // ── Check 104: Redundant collect then iterate (Medium) ────────────
    if cat_ok("optimization") && !skip("optimization") && !skip("detect_redundant_collect_iterate") {
        optimization::detect_redundant_collect_iterate(&ctx, &mut findings);
    }

    // ── Check 105: Repeated map lookup (Low) ──────────────────────────
    if cat_ok("optimization") && !skip("optimization") && !skip("detect_repeated_map_lookup") {
        optimization::detect_repeated_map_lookup(&ctx, &mut findings);
    }

    // ── Check 106: Vec/list without pre-sizing (Low) ──────────────────
    if cat_ok("optimization") && !skip("optimization") && !skip("detect_vec_no_presize") {
        optimization::detect_vec_no_presize(&ctx, &mut findings);
    }

    // ── Check 107: Sort then linear find (Medium) ─────────────────────
    if cat_ok("optimization") && !skip("optimization") && !skip("detect_sort_then_find") {
        optimization::detect_sort_then_find(&ctx, &mut findings);
    }

    // ── Check 108: List concat in loop (Medium) ───────────────────────
    if cat_ok("optimization") && !skip("optimization") && !skip("detect_list_concat_in_loop") {
        optimization::detect_list_concat_in_loop(&ctx, &mut findings);
    }

    // ── Check 109: Unbounded recursion (Low) ──────────────────────────
    if cat_ok("optimization") && !skip("optimization") && !skip("detect_unbounded_recursion") {
        optimization::detect_unbounded_recursion(&ctx, &mut findings);
    }

    // ── Check 110: SIMD / vectorization candidate (Low) ──────────────
    if cat_ok("optimization") && !skip("optimization") && !skip("detect_vectorization_candidate") {
        optimization::detect_vectorization_candidate(&ctx, &mut findings);
    }

    // ── Check 111: Suggest Polars over Pandas (Medium / Low) ─────────
    if cat_ok("optimization") && !skip("optimization") && !skip("detect_suggest_polars") {
        optimization::detect_suggest_polars(&ctx, &mut findings);
    }

    // ── Check 112: Regex recompile in loop (Medium) ───────────────────
    if cat_ok("optimization") && !skip("optimization") && !skip("detect_regex_recompile_in_loop") {
        optimization::detect_regex_recompile_in_loop(&ctx, &mut findings);
    }

    // ── Check 113: Memoization candidate (Low) ──────────────────────
    if cat_ok("optimization") && !skip("optimization") && !skip("detect_memoization_candidate") {
        optimization::detect_memoization_candidate(&ctx, &mut findings);
    }

    // ── Check 114: Exception for control flow (Medium) ──────────────
    if cat_ok("optimization") && !skip("optimization") && !skip("detect_exception_for_control_flow") {
        optimization::detect_exception_for_control_flow(&ctx, &mut findings);
    }

    // ── Check 115: N+1 query (High) ─────────────────────────────────
    if cat_ok("optimization") && !skip("optimization") && !skip("detect_n_plus_one_query") {
        optimization::detect_n_plus_one_query(&ctx, &mut findings);
    }

    // ── Check 116: Sync/async conflict (High) ───────────────────────
    if cat_ok("optimization") && !skip("optimization") && !skip("detect_sync_async_conflict") {
        optimization::detect_sync_async_conflict(&ctx, &mut findings);
    }

    // ── Check 117: Repeated format in loop (Low) ────────────────────
    if cat_ok("optimization") && !skip("optimization") && !skip("detect_repeated_format_in_loop") {
        optimization::detect_repeated_format_in_loop(&ctx, &mut findings);
    }

    // ── Check 118: Sleep in loop (Medium) ─────────────────────────────
    if cat_ok("optimization") && !skip("optimization") && !skip("detect_sleep_in_loop") {
        optimization::detect_sleep_in_loop(&ctx, &mut findings);
    }

    // ── Check 119: Generator over list (Low) ────────────────────────
    if cat_ok("optimization") && !skip("optimization") && !skip("detect_generator_over_list") {
        optimization::detect_generator_over_list(&ctx, &mut findings);
    }

    // ── Check 120: Unnecessary iterator chain (Low) ─────────────────
    if cat_ok("optimization") && !skip("optimization") && !skip("detect_unnecessary_chain") {
        optimization::detect_unnecessary_chain(&ctx, &mut findings);
    }

    // ── Check 121: Large list membership test (Medium) ──────────────
    if cat_ok("optimization") && !skip("optimization") && !skip("detect_large_list_in") {
        optimization::detect_large_list_in(&ctx, &mut findings);
    }

    // ── Check 122: Dict keys iteration (Low) ────────────────────────
    if cat_ok("optimization") && !skip("optimization") && !skip("detect_dict_keys_iter") {
        optimization::detect_dict_keys_iter(&ctx, &mut findings);
    }

    // ── Check 123: Unclosed resource (High) ─────────────────────────
    if cat_ok("optimization") && !skip("optimization") && !skip("detect_unclosed_resource") {
        optimization::detect_unclosed_resource(&ctx, &mut findings);
    }

    // ── Check 124: Enumerate vs range(len()) (Low) ───────────────────
    if cat_ok("optimization") && !skip("optimization") && !skip("detect_enumerate_vs_range_len") {
        optimization::detect_enumerate_vs_range_len(&ctx, &mut findings);
    }

    // ── Check 125: yield from (Low) ─────────────────────────────────
    if cat_ok("optimization") && !skip("optimization") && !skip("detect_yield_from") {
        optimization::detect_yield_from(&ctx, &mut findings);
    }

    // ── Check 126: Append in loop → extend (Low) ────────────────────
    if cat_ok("optimization") && !skip("optimization") && !skip("detect_append_in_loop_extend") {
        optimization::detect_append_in_loop_extend(&ctx, &mut findings);
    }

    // ── Check 127: Double with statement (Low) ──────────────────────
    if cat_ok("optimization") && !skip("optimization") && !skip("detect_double_with_statement") {
        optimization::detect_double_with_statement(&ctx, &mut findings);
    }

    // ── Check 128: Import in function (Low) ─────────────────────────
    if cat_ok("optimization") && !skip("optimization") && !skip("detect_import_in_function") {
        optimization::detect_import_in_function(&ctx, &mut findings);
    }

    // ── Check 129: Constant condition (Medium) ──────────────────────
    if cat_ok("optimization") && !skip("optimization") && !skip("detect_constant_condition") {
        optimization::detect_constant_condition(&ctx, &mut findings);
    }

    // ── Check 130: Redundant negation (Low) ─────────────────────────
    if cat_ok("optimization") && !skip("optimization") && !skip("detect_redundant_negation") {
        optimization::detect_redundant_negation(&ctx, &mut findings);
    }

    // ── Check 131: Default dict pattern (Low) ─────────────────────────
    if cat_ok("optimization") && !skip("optimization") && !skip("detect_default_dict_pattern") {
        optimization::detect_default_dict_pattern(&ctx, &mut findings);
    }

    // ── Check 132: Empty string check (Low) ─────────────────────────
    if cat_ok("optimization") && !skip("optimization") && !skip("detect_empty_string_check") {
        optimization::detect_empty_string_check(&ctx, &mut findings);
    }

    // Filter by category if specified
    if let Some(ref cat) = config.category {
        findings.retain(|f| f.kind.category() == cat.as_str());
    }

    // Sort: Critical first, then High, Medium, Low
    findings.sort_by_key(|f| f.tier);
    findings
}
