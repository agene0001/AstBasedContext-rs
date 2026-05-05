use std::fmt;

use serde::{Deserialize, Serialize};

/// Confidence tier for a redundancy finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Tier {
    /// Almost certainly redundant — safe to act on.
    Critical = 0,
    /// Very likely redundant — worth investigating.
    High = 1,
    /// Possibly redundant — needs human judgement.
    Medium = 2,
    /// Might be worth consolidating, or might be intentional.
    Low = 3,
}

impl fmt::Display for Tier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Tier::Critical => write!(f, "CRITICAL"),
            Tier::High => write!(f, "HIGH"),
            Tier::Medium => write!(f, "MEDIUM"),
            Tier::Low => write!(f, "LOW"),
        }
    }
}

/// The kind of redundancy detected.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FindingKind {
    /// Function body is just a call to another function with same/subset params.
    Passthrough {
        wrapper_name: String,
        target_name: String,
        /// All wrapper params are forwarded to target.
        exact_forward: bool,
    },

    /// Near-identical source code after whitespace/name normalization.
    NearDuplicate { names: Vec<String>, similarity: f64 },

    /// Structurally similar (shared tokens, similar shape).
    StructurallySimilar { names: Vec<String>, similarity: f64 },

    /// Two functions share a common core but differ in specific sections —
    /// could be merged with a parameter/enum to select behavior.
    MergeCandidate {
        names: Vec<String>,
        shared_line_ratio: f64,
    },

    /// Function is too large / complex and should be split.
    SplitCandidate {
        name: String,
        line_count: usize,
        complexity: u32,
        /// Estimated number of distinct "sections" (blocks separated by blank lines
        /// or comments, each with its own control flow).
        estimated_sections: usize,
    },

    // ── Struct / Enum redundancy ─────────────────────────────────────────
    /// Structs with heavily overlapping field names.
    OverlappingStructs {
        names: Vec<String>,
        shared_fields: Vec<String>,
        overlap_ratio: f64,
    },

    /// Enums with overlapping variant names.
    OverlappingEnums {
        names: Vec<String>,
        shared_variants: Vec<String>,
        overlap_ratio: f64,
    },

    // ── Design improvement suggestions ───────────────────────────────────
    /// Multiple functions share 4+ parameters — suggest grouping into a struct.
    SuggestParameterStruct {
        function_names: Vec<String>,
        shared_params: Vec<String>,
    },

    /// Function takes a boolean or string flag that controls branching —
    /// suggest replacing with an enum.
    SuggestEnumDispatch {
        function_name: String,
        flag_params: Vec<String>,
    },

    /// Multiple classes/structs implement overlapping method sets —
    /// suggest extracting a trait or interface.
    SuggestTraitExtraction {
        type_names: Vec<String>,
        shared_methods: Vec<String>,
    },

    // ── Architecture pattern suggestions ──────────────────────────────────
    /// External modules call many internal functions of a module directly —
    /// a facade would simplify the interface.
    SuggestFacade {
        module_name: String,
        internal_functions_called: usize,
        external_caller_count: usize,
    },

    /// Constructor calls to sibling classes (sharing a base) are scattered
    /// across the codebase — a factory method would centralize creation.
    SuggestFactory {
        base_name: String,
        sibling_names: Vec<String>,
        call_site_count: usize,
    },

    /// A constructor or function takes too many parameters — a builder
    /// pattern would improve ergonomics.
    SuggestBuilder {
        function_name: String,
        param_count: usize,
    },

    /// A trait/interface has multiple implementors and callers branch to
    /// choose which one — a strategy pattern would formalize this.
    SuggestStrategy {
        trait_name: String,
        implementor_names: Vec<String>,
    },

    /// A base class has methods that all subclasses override — the
    /// non-overridden methods form a template, overridden ones are hooks.
    SuggestTemplateMethod {
        base_name: String,
        hook_methods: Vec<String>,
        subclass_count: usize,
    },

    /// A function is called by many unrelated modules — an event/observer
    /// pattern would decouple the callers.
    SuggestObserver {
        function_name: String,
        caller_module_count: usize,
        total_callers: usize,
    },

    /// A function wraps a single call with before/after logic — could be
    /// a decorator pattern candidate (noisy).
    SuggestDecorator {
        wrapper_name: String,
        wrapped_name: String,
    },

    /// A module has both high fan-in and high fan-out — could benefit from
    /// a mediator to manage the coordination (noisy).
    SuggestMediator {
        module_name: String,
        fan_in: usize,
        fan_out: usize,
    },

    // ── Anti-pattern detection ────────────────────────────────────────────
    /// A class or module has too many methods/functions — it's doing too much.
    GodClass {
        name: String,
        method_count: usize,
        node_type: String,
    },

    /// Circular dependency between modules (files/directories import each other).
    CircularDependency { cycle: Vec<String> },

    /// A function calls more methods on another class than on its own —
    /// it may belong in the other class.
    FeatureEnvy {
        function_name: String,
        own_class: String,
        envied_class: String,
        own_calls: usize,
        envied_calls: usize,
    },

    /// Changing one function would likely require touching many modules —
    /// callers are spread across many directories.
    ShotgunSurgery {
        function_name: String,
        affected_modules: usize,
        total_callers: usize,
    },

    // ── Pattern detection (enabled by type/visibility enrichment) ─────────
    /// Class has a private constructor, a static field of its own type,
    /// and a static accessor method — classic singleton.
    DetectedSingleton { class_name: String },

    /// Class implements an interface and wraps a different type via a field,
    /// delegating methods to it — adapter pattern.
    DetectedAdapter {
        adapter_name: String,
        adaptee_type: String,
        interface_name: String,
    },

    /// Class wraps another object of the same interface type, delegating
    /// all methods — proxy pattern.
    DetectedProxy {
        proxy_name: String,
        wrapped_type: String,
    },

    /// Multiple classes implement a single-method interface (execute/run/invoke) —
    /// suggests the command pattern.
    DetectedCommand {
        interface_name: String,
        command_names: Vec<String>,
        method_name: String,
    },

    /// Class has a field typed as its own type or interface (next/handler/successor)
    /// and methods that conditionally delegate — chain of responsibility.
    DetectedChainOfResponsibility {
        class_name: String,
        next_field: String,
    },

    /// Constructor parameters are typed as interfaces/traits rather than
    /// concrete classes — dependency injection.
    DetectedDependencyInjection {
        class_name: String,
        constructor_name: String,
        interface_params: Vec<(String, String)>, // (param_name, interface_type)
    },

    // ── Additional anti-patterns ─────────────────────────────────────────
    /// Function/method is never called by anything in the graph.
    DeadCode { name: String, file_path: String },

    /// Function takes too many parameters (6+).
    LongParameterList {
        function_name: String,
        param_count: usize,
    },

    /// Same group of 3+ parameters appears across multiple functions.
    DataClump {
        function_names: Vec<String>,
        clumped_params: Vec<String>,
    },

    /// Class where 80%+ methods just delegate to another class.
    MiddleMan {
        class_name: String,
        delegated_class: String,
        delegation_ratio: f64,
        total_methods: usize,
    },

    /// Class with only 1-2 trivial methods — may not justify its existence.
    LazyClass {
        class_name: String,
        method_count: usize,
    },

    /// Subclass that overrides/calls none of the parent's methods.
    RefusedBequest {
        child_name: String,
        parent_name: String,
    },

    /// Interface/trait with exactly one implementor — may be premature abstraction.
    SpeculativeGenerality {
        interface_name: String,
        sole_implementor: String,
    },

    /// Two classes with very high bidirectional coupling.
    InappropriateIntimacy {
        class_a: String,
        class_b: String,
        a_to_b_calls: usize,
        b_to_a_calls: usize,
    },

    /// Function with high cyclomatic complexity suggesting deeply nested logic.
    DeepNesting {
        function_name: String,
        complexity: u32,
        line_count: usize,
    },

    // ── Additional pattern detection ─────────────────────────────────────
    /// Classes with accept(visitor) + visitor classes with visit_X methods.
    DetectedVisitor {
        visitor_name: String,
        element_names: Vec<String>,
    },

    /// Class implementing next/has_next or __iter__/__next__.
    DetectedIterator { class_name: String },

    /// Like Strategy but state object holds a reference to its own interface type.
    DetectedState {
        state_interface: String,
        state_names: Vec<String>,
    },

    /// Class containing a collection of its own type (tree/composite structure).
    DetectedComposite {
        class_name: String,
        collection_field: String,
    },

    /// Class with CRUD-like methods (find/save/delete/update) on a single entity.
    DetectedRepository {
        class_name: String,
        entity_hint: String,
        crud_methods: Vec<String>,
    },

    /// Class with clone/copy/deep_copy factory methods.
    DetectedPrototype {
        class_name: String,
        clone_method: String,
    },

    // ── Structural / architecture quality ────────────────────────────────
    /// A file that imports from 10+ other files — potential bottleneck.
    HubModule {
        file_name: String,
        import_count: usize,
    },

    /// A file with no incoming CALLS or IMPORTS edges — potentially unused.
    OrphanModule {
        file_name: String,
        file_path: String,
    },

    // ── Additional anti-patterns (batch 2) ───────────────────────────────
    /// One file has functions called by many different unrelated modules —
    /// it changes for many different reasons.
    DivergentChange {
        file_name: String,
        caller_module_count: usize,
    },

    /// Two class hierarchies that always grow in parallel.
    ParallelInheritance {
        hierarchy_a: String,
        hierarchy_b: String,
        paired_count: usize,
    },

    /// Function params are all primitives (string/int/bool) with no domain types.
    PrimitiveObsession {
        function_name: String,
        primitive_params: Vec<String>,
    },

    /// Class with 500+ lines of source code.
    LargeClass {
        name: String,
        line_count: usize,
        node_type: String,
    },

    /// Module depends on an unstable (high fan-in) dependency.
    UnstableDependency {
        dependent_name: String,
        dependency_name: String,
        dependency_caller_count: usize,
    },

    // ── Additional pattern detection (batch 2) ───────────────────────────
    /// Static map/dict field + method returning cached instances — flyweight.
    DetectedFlyweight {
        class_name: String,
        cache_field: String,
    },

    /// Classes with subscribe/unsubscribe/notify or addEventListener methods.
    DetectedEventEmitter {
        class_name: String,
        event_methods: Vec<String>,
    },

    /// Classes with save_state/restore_state or undo/redo method pairs.
    DetectedMemento {
        class_name: String,
        method_pair: (String, String),
    },

    /// Fluent interface — methods that return self/Self (builder in use).
    DetectedFluentBuilder {
        class_name: String,
        fluent_methods: Vec<String>,
    },

    /// Class inheriting an interface where all methods are empty/no-op.
    DetectedNullObject {
        class_name: String,
        interface_name: String,
    },

    // ── Structural quality (batch 2) ─────────────────────────────────────
    /// Functions in the same class/module using different naming conventions.
    InconsistentNaming {
        scope_name: String,
        snake_case_names: Vec<String>,
        camel_case_names: Vec<String>,
    },

    /// Circular dependency at the directory/package level.
    CircularPackageDependency { cycle: Vec<String> },

    // ── Type system suggestions ──────────────────────────────────────────
    /// Class uses a string/int `type`/`kind`/`tag` field with branching —
    /// should be an enum / sum type with variants.
    SuggestSumType {
        class_name: String,
        tag_field: String,
    },

    /// Leaf subclasses that carry no data, only override behavior —
    /// could be variants of an enum / ADT.
    SuggestEnumFromHierarchy {
        base_name: String,
        leaf_names: Vec<String>,
    },

    /// Function takes multiple booleans or a boolean that controls entirely
    /// different code paths — use a descriptive enum instead.
    BooleanBlindness {
        function_name: String,
        bool_params: Vec<String>,
    },

    /// Struct/class wrapping exactly one primitive field — consider a newtype
    /// for type safety.
    SuggestNewtype {
        type_name: String,
        wrapped_field: String,
        wrapped_type: String,
    },

    /// Interface/trait where all implementors live in the same file/module —
    /// effectively a closed sum type, consider sealing it.
    SuggestSealedType {
        interface_name: String,
        implementor_names: Vec<String>,
        file_name: String,
    },

    /// Struct with 10+ fields, many optional — the product type is too wide.
    LargeProductType {
        type_name: String,
        field_count: usize,
        optional_count: usize,
    },

    // ── Additional anti-patterns (batch 3) ───────────────────────────────
    /// Class with only getters/setters, no real behavior.
    AnemicDomainModel {
        class_name: String,
        getter_setter_count: usize,
        total_methods: usize,
    },

    /// Hardcoded literal constants in function bodies.
    MagicNumber {
        function_name: String,
        literals: Vec<String>,
    },

    /// Module-level mutable global state.
    MutableGlobalState {
        variable_name: String,
        file_name: String,
    },

    /// try/catch with empty error handler.
    EmptyCatch { function_name: String },

    /// Deeply nested callbacks/closures (3+ levels).
    CallbackHell {
        function_name: String,
        nesting_depth: usize,
    },

    /// Similar functions with different parameter ordering or signatures.
    ApiInconsistency {
        function_names: Vec<String>,
        shared_prefix: String,
    },

    // ── Metrics ──────────────────────────────────────────────────────────
    /// Low cohesion — methods in a class don't share instance fields.
    LackOfCohesion {
        class_name: String,
        lcom_score: f64,
        method_count: usize,
    },

    /// High coupling between objects — class depends on too many others.
    HighCoupling {
        class_name: String,
        coupled_classes: usize,
    },

    /// Module instability — high efferent coupling relative to total.
    ModuleInstability {
        file_name: String,
        afferent: usize,
        efferent: usize,
        instability: f64,
    },

    /// High cognitive complexity — deeply nested, hard to understand.
    HighCognitiveComplexity { function_name: String, score: u32 },

    // ── Composite Risk Score ────────────────────────────────────────────
    /// Function with a high composite risk score across multiple dimensions.
    HighRiskFunction {
        name: String,
        risk_score: f64,
        factors: Vec<String>,
    },

    /// File with a high composite risk score.
    HighRiskFile {
        name: String,
        risk_score: f64,
        factors: Vec<String>,
    },

    // ── Test Coverage Gaps ──────────────────────────────────────────────
    /// Public function with no test coverage.
    UntestedPublicFunction {
        function_name: String,
        file_name: String,
        caller_count: usize,
    },

    /// File with low percentage of functions covered by tests.
    LowTestRatio {
        file_name: String,
        function_count: usize,
        tested_count: usize,
        ratio: f64,
    },

    /// Test function that touches many modules — may be an integration test disguised as unit test.
    IntegrationTestSmell {
        test_name: String,
        modules_touched: usize,
    },

    // ── Change Blast Radius ─────────────────────────────────────────────
    /// Changing this function would transitively affect many modules.
    HighBlastRadius {
        function_name: String,
        direct_callers: usize,
        transitive_callers: usize,
        modules_affected: usize,
    },

    // ── Semantic Clustering ─────────────────────────────────────────────
    /// Function that interacts more with another file than its own.
    MisplacedFunction {
        function_name: String,
        current_file: String,
        suggested_cluster: String,
    },

    /// Group of heavily connected functions spanning multiple files.
    ImplicitModule {
        cluster_functions: Vec<String>,
        spanning_files: Vec<String>,
    },

    // ── API Surface Analysis ────────────────────────────────────────────
    /// Public function with many callers and many params — fragile to change.
    UnstablePublicApi {
        function_name: String,
        caller_count: usize,
        param_count: usize,
    },

    /// Public function with no docstring.
    UndocumentedPublicApi {
        function_name: String,
        file_name: String,
    },

    /// Public function exposing internal/private types in its signature.
    LeakyAbstraction {
        function_name: String,
        internal_types_exposed: Vec<String>,
    },

    // ── Cross-Language Boundaries ───────────────────────────────────────
    /// FFI boundary — function uses foreign function interface.
    FfiBoundary {
        function_name: String,
        ffi_type: String, // "extern C", "ctypes", "cgo", "JNI", "napi", "wasm_bindgen", etc.
    },

    /// Subprocess/exec call — function spawns an external process.
    SubprocessCall {
        function_name: String,
        call_pattern: String,
    },

    /// IPC/RPC boundary — file uses inter-process or remote procedure call patterns.
    IpcBoundary {
        file_name: String,
        protocol: String, // "gRPC", "protobuf", "REST endpoint", "WebSocket", "message queue"
    },

    // ── Configuration Detection ─────────────────────────────────────────
    /// Environment variable read.
    EnvVarUsage {
        function_name: String,
        env_pattern: String,
    },

    /// Hardcoded URL or IP address in source.
    HardcodedEndpoint {
        function_name: String,
        endpoint: String,
    },

    /// Feature flag or conditional compilation.
    FeatureFlag { name: String, location: String },

    /// Config file reference — code reads from a config file.
    ConfigFileUsage {
        function_name: String,
        config_pattern: String,
    },

    // ── Suboptimal data structure usage (checks 92-99) ─────────────────
    /// Vec/list used with append + contains but no index access → use HashSet/set.
    VecUsedAsSet {
        function_name: String,
        variable_hint: String,
    },

    /// Vec of tuples searched with .iter().find(|(k,_)| ...) → use HashMap/dict.
    VecUsedAsMap {
        function_name: String,
        variable_hint: String,
    },

    /// .contains() / .find() inside a loop body → pre-compute a HashSet.
    LinearSearchInLoop {
        function_name: String,
        search_pattern: String,
    },

    /// String concatenation in a loop → use with_capacity / join / StringBuilder.
    StringConcatInLoop {
        function_name: String,
        concat_pattern: String,
    },

    /// .sort() + .binary_search() → consider BTreeSet/BTreeMap.
    SortedVecForLookup {
        function_name: String,
        variable_hint: String,
    },

    /// Nested loop with equality check (O(n²)) → use HashSet for inner collection.
    NestedLoopLookup {
        function_name: String,
        estimated_pattern: String,
    },

    /// HashMap/dict with sequential integer keys → use Vec/list.
    HashMapWithSequentialKeys {
        function_name: String,
        variable_hint: String,
    },

    /// .collect::<Vec<_>>() immediately iterated → remove intermediate collect.
    ExcessiveCollectIterate {
        function_name: String,
        collect_pattern: String,
    },

    /// Unused import — imported symbol not referenced in the file.
    UnusedImport {
        module_name: String,
        import_name: String,
    },

    /// Inconsistent error handling — mix of patterns in the same module.
    InconsistentErrorHandling {
        file_name: String,
        patterns_found: Vec<String>,
    },

    /// TODO/FIXME/HACK comment — tech debt marker found.
    TechDebtComment {
        function_name: String,
        marker: String,
        comment_text: String,
    },

    // ── Optimization suggestions (checks 103-109) ────────────────────────

    /// Expensive clone/allocation inside a loop body.
    CloneInLoop {
        function_name: String,
        pattern: String,
    },

    /// `.collect()` immediately followed by `.iter()` / `.into_iter()` — skip the allocation.
    RedundantCollectIterate {
        function_name: String,
        pattern: String,
    },

    /// Same map key looked up 2+ times — cache in a local variable.
    RepeatedMapLookup {
        function_name: String,
        key_hint: String,
        count: usize,
    },

    /// Vec/list created then pushed to in a loop without pre-sizing.
    VecNoPresize {
        function_name: String,
        variable_hint: String,
    },

    /// `.sort()` followed by `.iter().find()` — use `.binary_search()` or sorted data structure.
    SortThenFind {
        function_name: String,
        variable_hint: String,
    },

    /// Python `list += list` or `list.extend(list)` inside loop — O(n²) total; build once outside.
    ListConcatInLoop {
        function_name: String,
        variable_hint: String,
    },

    /// Recursive function with no depth/limit parameter — risk of stack overflow.
    UnboundedRecursion {
        function_name: String,
    },

    /// Loop with element-wise array arithmetic — candidate for SIMD or NumPy vectorization.
    SuggestVectorize {
        function_name: String,
        pattern: String,
        suggestion: String,
    },

    /// Pandas usage that could benefit from Polars for better performance.
    SuggestPolars {
        function_name: String,
        pattern: String,
    },

    // ── Optimization suggestions (checks 112-117) ────────────────────────

    /// Regex compiled inside a loop — compile once outside.
    RegexRecompileInLoop {
        function_name: String,
        pattern: String,
    },

    /// Pure-looking function called multiple times with identical arguments — memoize.
    MemoizationCandidate {
        function_name: String,
        callee: String,
        repeat_count: usize,
    },

    /// Exception/error used for normal control flow instead of conditional checks.
    ExceptionForControlFlow {
        function_name: String,
        pattern: String,
    },

    /// Database/API call inside a loop body — batch or prefetch.
    NPlusOneQuery {
        function_name: String,
        call_pattern: String,
    },

    /// Blocking/synchronous call inside an async function.
    SyncAsyncConflict {
        function_name: String,
        blocking_call: String,
    },

    /// Repeated string formatting with same template inside a loop.
    RepeatedFormatInLoop {
        function_name: String,
        pattern: String,
    },

    // ── Optimization suggestions (checks 118-122) ────────────────────────

    /// `sleep()` inside a loop body — busy-wait / polling pattern.
    SleepInLoop {
        function_name: String,
        pattern: String,
    },

    /// List comprehension passed to aggregate — use generator expression instead.
    GeneratorOverList {
        function_name: String,
        pattern: String,
    },

    /// Chainable iterator operations that can be fused (e.g. `.map().filter()` → `.filter_map()`).
    UnnecessaryChain {
        function_name: String,
        pattern: String,
        suggestion: String,
    },

    /// Membership test on a list literal — use a set literal for O(1) lookup.
    LargeListIn {
        function_name: String,
        pattern: String,
    },

    /// `for k in dict.keys()` — iterate the dict directly.
    DictKeysIter {
        function_name: String,
        pattern: String,
    },

    // ── Resource management (check 123) ──────────────────────────────────

    /// Resource opened without context manager / RAII guard.
    UnclosedResource {
        function_name: String,
        pattern: String,
        suggestion: String,
    },

    // ── Python idioms (checks 124-128) ───────────────────────────────────

    /// `for i in range(len(x))` → `for i, v in enumerate(x)`.
    EnumerateVsRangeLen {
        function_name: String,
    },

    /// `for x in iterable: yield x` → `yield from iterable`.
    YieldFrom {
        function_name: String,
    },

    /// `for x in items: result.append(x)` → `result.extend(items)`.
    AppendInLoopExtend {
        function_name: String,
        variable_hint: String,
    },

    /// Nested `with` blocks that can be combined into one `with a, b:`.
    DoubleWithStatement {
        function_name: String,
    },

    /// `import` statement inside function body — moves import cost into every call.
    ImportInFunction {
        function_name: String,
        module_name: String,
    },

    // ── Cross-language lint (checks 129-130) ─────────────────────────────

    /// Constant condition: `if True`, `while false`, `if 1 {` — dead or infinite branch.
    ConstantCondition {
        function_name: String,
        pattern: String,
    },

    /// Redundant negation: `if not x == y` → `if x != y`.
    RedundantNegation {
        function_name: String,
        pattern: String,
        suggestion: String,
    },

    // ── Checks 131-132 ──────────────────────────────────────────────────

    /// Repeated `if key not in d: d[key] = default` → use `defaultdict` or `.setdefault()`.
    DefaultDictPattern {
        function_name: String,
        pattern: String,
    },

    /// `if s == ""` / `if s != ""` → `if not s` / `if s` (Python) or `.is_empty()` (Rust).
    EmptyStringCheck {
        function_name: String,
        pattern: String,
        suggestion: String,
    },
}

/// A single redundancy finding with tier, kind, involved nodes, and explanation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub tier: Tier,
    pub kind: FindingKind,
    pub node_indices: Vec<usize>, // stored as usize for serialization
    pub description: String,
}

/// Configuration for the redundancy analysis.
#[derive(Debug, Clone)]
pub struct AnalysisConfig {
    /// Minimum lines for a function to be checked (skip trivial getters).
    pub min_lines: usize,
    /// Complexity threshold for split candidates.
    pub split_complexity_threshold: u32,
    /// Line count threshold for split candidates.
    pub split_line_threshold: usize,
    /// Token similarity threshold for near-duplicates.
    pub near_duplicate_threshold: f64,
    /// Token similarity threshold for structural similarity.
    pub structural_threshold: f64,
    /// Shared line ratio threshold for merge candidates.
    pub merge_threshold: f64,
    /// Composite risk score threshold for flagging functions/files.
    pub risk_score_threshold: f64,
    /// Minimum transitive modules affected to flag blast radius.
    pub blast_radius_module_threshold: usize,
    /// Minimum test coverage ratio per file.
    pub test_ratio_threshold: f64,
    /// Minimum files touched by a test to flag integration test smell.
    pub integration_test_module_threshold: usize,
    /// List of check names (or category module names) to skip.
    pub skip_checks: Vec<String>,
    /// If set, only include findings from this category.
    pub category: Option<String>,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            min_lines: 3,
            split_complexity_threshold: 15,
            split_line_threshold: 60,
            near_duplicate_threshold: 0.80,
            structural_threshold: 0.40,
            merge_threshold: 0.50,
            risk_score_threshold: 0.6,
            blast_radius_module_threshold: 5,
            test_ratio_threshold: 0.30,
            integration_test_module_threshold: 4,
            skip_checks: Vec::new(),
            category: None,
        }
    }
}

impl FindingKind {
    /// Return the category name for this finding kind.
    pub fn category(&self) -> &'static str {
        match self {
            FindingKind::Passthrough { .. }
            | FindingKind::NearDuplicate { .. }
            | FindingKind::StructurallySimilar { .. }
            | FindingKind::MergeCandidate { .. }
            | FindingKind::SplitCandidate { .. } => "redundancy",

            FindingKind::OverlappingStructs { .. }
            | FindingKind::OverlappingEnums { .. } => "struct_enum",

            FindingKind::SuggestParameterStruct { .. }
            | FindingKind::SuggestEnumDispatch { .. }
            | FindingKind::SuggestTraitExtraction { .. } => "type_suggestions",

            FindingKind::SuggestFacade { .. }
            | FindingKind::SuggestFactory { .. }
            | FindingKind::SuggestBuilder { .. }
            | FindingKind::SuggestStrategy { .. }
            | FindingKind::SuggestTemplateMethod { .. }
            | FindingKind::SuggestObserver { .. }
            | FindingKind::SuggestDecorator { .. }
            | FindingKind::SuggestMediator { .. } => "design_patterns",

            FindingKind::GodClass { .. }
            | FindingKind::CircularDependency { .. }
            | FindingKind::FeatureEnvy { .. }
            | FindingKind::ShotgunSurgery { .. }
            | FindingKind::DeadCode { .. }
            | FindingKind::LongParameterList { .. }
            | FindingKind::DataClump { .. }
            | FindingKind::MiddleMan { .. }
            | FindingKind::LazyClass { .. }
            | FindingKind::RefusedBequest { .. }
            | FindingKind::SpeculativeGenerality { .. }
            | FindingKind::InappropriateIntimacy { .. }
            | FindingKind::DeepNesting { .. }
            | FindingKind::DivergentChange { .. }
            | FindingKind::ParallelInheritance { .. }
            | FindingKind::PrimitiveObsession { .. }
            | FindingKind::LargeClass { .. }
            | FindingKind::UnstableDependency { .. }
            | FindingKind::AnemicDomainModel { .. }
            | FindingKind::MagicNumber { .. }
            | FindingKind::MutableGlobalState { .. }
            | FindingKind::EmptyCatch { .. }
            | FindingKind::CallbackHell { .. }
            | FindingKind::ApiInconsistency { .. } => "anti_patterns",

            FindingKind::DetectedSingleton { .. }
            | FindingKind::DetectedAdapter { .. }
            | FindingKind::DetectedProxy { .. }
            | FindingKind::DetectedCommand { .. }
            | FindingKind::DetectedChainOfResponsibility { .. }
            | FindingKind::DetectedDependencyInjection { .. }
            | FindingKind::DetectedVisitor { .. }
            | FindingKind::DetectedIterator { .. }
            | FindingKind::DetectedState { .. }
            | FindingKind::DetectedComposite { .. }
            | FindingKind::DetectedRepository { .. }
            | FindingKind::DetectedPrototype { .. }
            | FindingKind::DetectedFlyweight { .. }
            | FindingKind::DetectedEventEmitter { .. }
            | FindingKind::DetectedMemento { .. }
            | FindingKind::DetectedFluentBuilder { .. }
            | FindingKind::DetectedNullObject { .. } => "pattern_detection",

            FindingKind::HubModule { .. }
            | FindingKind::OrphanModule { .. }
            | FindingKind::InconsistentNaming { .. }
            | FindingKind::CircularPackageDependency { .. } => "structural",

            FindingKind::SuggestSumType { .. }
            | FindingKind::SuggestEnumFromHierarchy { .. }
            | FindingKind::BooleanBlindness { .. }
            | FindingKind::SuggestNewtype { .. }
            | FindingKind::SuggestSealedType { .. }
            | FindingKind::LargeProductType { .. } => "type_system",

            FindingKind::LackOfCohesion { .. }
            | FindingKind::HighCoupling { .. }
            | FindingKind::ModuleInstability { .. }
            | FindingKind::HighCognitiveComplexity { .. } => "metrics",

            FindingKind::HighRiskFunction { .. }
            | FindingKind::HighRiskFile { .. } => "risk",

            FindingKind::UntestedPublicFunction { .. }
            | FindingKind::LowTestRatio { .. }
            | FindingKind::IntegrationTestSmell { .. } => "testing",

            FindingKind::HighBlastRadius { .. }
            | FindingKind::MisplacedFunction { .. }
            | FindingKind::ImplicitModule { .. } => "blast_radius",

            FindingKind::UnstablePublicApi { .. }
            | FindingKind::UndocumentedPublicApi { .. }
            | FindingKind::LeakyAbstraction { .. } => "api_surface",

            FindingKind::FfiBoundary { .. }
            | FindingKind::SubprocessCall { .. }
            | FindingKind::IpcBoundary { .. } => "cross_language",

            FindingKind::EnvVarUsage { .. }
            | FindingKind::HardcodedEndpoint { .. }
            | FindingKind::FeatureFlag { .. }
            | FindingKind::ConfigFileUsage { .. } => "config_detection",

            FindingKind::VecUsedAsSet { .. }
            | FindingKind::VecUsedAsMap { .. }
            | FindingKind::LinearSearchInLoop { .. }
            | FindingKind::StringConcatInLoop { .. }
            | FindingKind::SortedVecForLookup { .. }
            | FindingKind::NestedLoopLookup { .. }
            | FindingKind::HashMapWithSequentialKeys { .. }
            | FindingKind::ExcessiveCollectIterate { .. } => "data_structures",

            FindingKind::UnusedImport { .. }
            | FindingKind::InconsistentErrorHandling { .. }
            | FindingKind::TechDebtComment { .. } => "code_quality",

            FindingKind::CloneInLoop { .. }
            | FindingKind::RedundantCollectIterate { .. }
            | FindingKind::RepeatedMapLookup { .. }
            | FindingKind::VecNoPresize { .. }
            | FindingKind::SortThenFind { .. }
            | FindingKind::ListConcatInLoop { .. }
            | FindingKind::UnboundedRecursion { .. }
            | FindingKind::SuggestVectorize { .. }
            | FindingKind::SuggestPolars { .. }
            | FindingKind::RegexRecompileInLoop { .. }
            | FindingKind::MemoizationCandidate { .. }
            | FindingKind::ExceptionForControlFlow { .. }
            | FindingKind::NPlusOneQuery { .. }
            | FindingKind::SyncAsyncConflict { .. }
            | FindingKind::RepeatedFormatInLoop { .. }
            | FindingKind::SleepInLoop { .. }
            | FindingKind::GeneratorOverList { .. }
            | FindingKind::UnnecessaryChain { .. }
            | FindingKind::LargeListIn { .. }
            | FindingKind::DictKeysIter { .. }
            | FindingKind::UnclosedResource { .. }
            | FindingKind::EnumerateVsRangeLen { .. }
            | FindingKind::YieldFrom { .. }
            | FindingKind::AppendInLoopExtend { .. }
            | FindingKind::DoubleWithStatement { .. }
            | FindingKind::ImportInFunction { .. }
            | FindingKind::ConstantCondition { .. }
            | FindingKind::RedundantNegation { .. }
            | FindingKind::DefaultDictPattern { .. }
            | FindingKind::EmptyStringCheck { .. } => "optimization",
        }
    }
}
