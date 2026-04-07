# agents.md — AI Agent Instructions for AstBasedContext-rs

This file describes how AI agents (Claude, etc.) should use the AstBasedContext-rs tools to analyze codebases, find redundancies, and suggest improvements.

## Overview

AstBasedContext-rs builds a code graph from AST/CST analysis and exposes it via an MCP server. Agents interact with it through MCP tools — no direct file access or shell commands are needed.

## Setup

### MCP Configuration (Claude Desktop)

```json
{
  "mcpServers": {
    "ast-context": {
      "command": "ast_context",
      "args": ["mcp"]
    }
  }
}
```

### CLI (for scripts or non-MCP agents)

```
# Build the graph once
ast_context index /path/to/project --annotate --save graph.json

# Then query it
ast_context search --graph graph.json "my_function"
ast_context similar --graph graph.json --kind Function
```

## Workflows

### 1. Understand a new codebase

**Goal**: Get a high-level map of the project before diving into code.

```
1. index_directory { path: "/project", annotate: true, exclude: ["vendor/**", "*.generated.go"], skip_tests: true }
2. get_stats {}
3. find_code { query: "main", kind: "Function" }
4. find_complex_functions { limit: 10 }
```

**Tip**: Use the `exclude` parameter, `.astcontextignore` (or `.astcontextignore.local`) files, and `skip_tests: true` to skip vendored, generated, or test fixture code that would add noise to the analysis.

**What to look for**:
- The stats tell you the scale and language mix
- Complex functions are the highest-risk areas
- Entry points (`main`, `run`, `start`, `handle`) reveal the architecture

### 2. Find redundant/duplicate code (tiered analysis)

**Goal**: Identify redundancy across an entire codebase, ranked by confidence.

```
1. index_directory { path: "/project", annotate: true }
2. analyze_redundancy { min_tier: "high" }
```

This returns findings in 4 tiers, each with a specific finding type:

| Tier | What it catches | Action |
|------|----------------|--------|
| **Critical** | Passthrough wrappers (function just calls another with same args), 95%+ identical code | Almost certainly safe to consolidate. Ask user for confirmation. |
| **High** | Near-duplicate code (80-95% similar), short delegation wrappers, structs/enums with 70%+ overlap, facade (many external callers into internals), factory (scattered sibling constructors), builder (8+ params on constructor), circular dependencies (2-3 files), god class (2x threshold) | Very likely improvable. Show the user the evidence and ask. |
| **Medium** | Structural similarity (40-80% token overlap), merge/split candidates, structs/enums with 50-70% overlap, trait extraction, facade/factory (borderline), builder (6+ params), strategy, template method, observer (high fan-in), god class (at threshold), circular deps (4+ files), feature envy, shotgun surgery (8+ modules) | Needs human judgement. Present as suggestions, not directives. |
| **Low** | Moderate structural overlap, borderline split candidates, parameter struct/enum dispatch suggestions, observer (borderline), decorator (noisy), mediator (noisy), shotgun surgery (5-7 modules) | Informational. Mention if the user is actively refactoring. |

**Finding types explained**:

- **PASSTHROUGH**: `fn foo(x) { bar(x) }` — function A just calls function B with the same parameters. Could be a facade pattern (intentional) or dead indirection (redundant). Check if A is a public API boundary.
- **NEAR-DUPLICATE**: Two functions with nearly identical source after normalization. Typically copy-paste with minor edits.
- **SIMILAR**: Shared structure/tokens but meaningful differences. May benefit from a shared abstraction.
- **MERGE**: Two functions share a common core (~50-80% of lines) but diverge in specific sections. Could be unified with a parameter or enum to select behavior. Example: `process_csv()` and `process_json()` that share validation/output logic but differ in parsing.
- **SPLIT**: Single function that's too large (60+ lines or 15+ cyclomatic complexity) with multiple distinct sections. Each section could be its own function.
- **STRUCT-OVERLAP**: Two or more structs share a high percentage of field names. Could indicate they should share a base type or use composition.
- **ENUM-OVERLAP**: Two or more enums share a high percentage of variant names. May benefit from a shared enum or trait.
- **SUGGEST-STRUCT**: Multiple functions take the same 4+ parameters. A parameter struct would reduce duplication and improve API ergonomics.
- **SUGGEST-ENUM**: A function takes a boolean/flag parameter (`is_`, `use_`, `_mode`, `_kind`) that controls branching. An enum would be more expressive and extensible.
- **SUGGEST-TRAIT**: Multiple classes/structs share 3+ method names. A trait/interface would formalize the shared behavior.

**Architecture pattern suggestions** (Checks 11-18):

- **SUGGEST-FACADE**: External modules call 4+ internal functions of a module directly. A facade would simplify the public API and reduce coupling.
- **SUGGEST-FACTORY**: Constructor calls to 3+ sibling classes (sharing a base) are scattered across the codebase. A factory method would centralize creation logic.
- **SUGGEST-BUILDER**: A constructor or function takes 6+ parameters. A builder pattern improves ergonomics and allows optional params with defaults.
- **SUGGEST-STRATEGY**: A trait/interface has 3+ implementors and callers branch to choose which one. Formalizing with the strategy pattern makes the selection explicit.
- **SUGGEST-TEMPLATE**: A base class has methods that all subclasses override. The non-overridden methods form a template algorithm, overridden ones are hooks.
- **SUGGEST-OBSERVER**: A function is called by 6+ callers from 4+ different modules. An event/observer pattern would decouple the callers.
- **SUGGEST-DECORATOR** (noisy): A function wraps a single call with before/after logic. If this pattern repeats, a decorator or middleware approach may help.
- **SUGGEST-MEDIATOR** (noisy): A module has both high fan-in (4+ modules call it) and high fan-out (calls 4+ modules). A mediator could manage the coordination.

**Anti-pattern detection** (Checks 19-22):

- **GOD-CLASS**: A class has 20+ methods or a file has 30+ functions. It's doing too much — split into smaller, focused modules with single responsibilities.
- **CIRCULAR-DEP**: Two or more files depend on each other (detected via call graph cycles using Tarjan's SCC). Makes code harder to test and refactor — extract shared logic into a new module.
- **FEATURE-ENVY**: A method calls more methods on another class than on its own. The method may belong in the other class.
- **SHOTGUN-SURGERY**: Changing one function would require updating callers across 5+ modules. High coupling — consider stabilizing the interface or adding an abstraction layer.

**Pattern detection** (Checks 23-28, requires enriched type data):

- **SINGLETON**: Class with 2+ signals: private constructor, static self-typed field, static accessor method.
- **ADAPTER**: Class that implements an interface but wraps a field of a different type.
- **PROXY**: Class that implements an interface and wraps a field of the same interface type.
- **COMMAND**: 3+ classes all implementing a single-method interface.
- **CHAIN-OF-RESP**: Class with a self-referencing field named `next`/`handler`/`successor`.
- **DI**: Constructor params typed as known interfaces/traits (dependency injection).

**Additional anti-patterns** (Checks 29-37):

- **DEAD-CODE**: Function/method with zero incoming CALLS edges that is not an entry point or test.
- **LONG-PARAMS**: Function with 6+ parameters — parameter struct would improve ergonomics.
- **DATA-CLUMP**: The same group of 3+ parameter names appears together across multiple functions.
- **MIDDLE-MAN**: Class where 80%+ of methods just delegate to another object.
- **LAZY-CLASS**: Class with only 1-2 trivial methods and no real logic.
- **REFUSED-BEQUEST**: Subclass that overrides none of the parent's methods.
- **SPEC-GENERALITY**: Interface/trait with exactly one implementor — over-engineered abstraction.
- **INAPP-INTIMACY**: Two classes with very high bidirectional call coupling.
- **DEEP-NESTING**: Function with high cyclomatic complexity and multiple nesting levels.

**Additional pattern detection** (Checks 38-55):

- **VISITOR**: `accept(visitor)` method + classes with `visit_X` methods.
- **ITERATOR**: Classes implementing `next`/`has_next` or `__iter__`/`__next__`.
- **STATE**: Strategy-like structure where the state object holds a reference to its own interface.
- **COMPOSITE**: Class containing a collection of its own type + shares an interface.
- **REPOSITORY**: Classes with CRUD-like methods (`find`, `save`, `delete`, `update`) on a single entity.
- **PROTOTYPE**: Classes with `clone`/`copy`/`deep_copy` factory methods.
- **FLYWEIGHT**: Factory/cache pattern — static map field + method returning cached instances.
- **EVENT-EMITTER**: Classes with `subscribe`/`unsubscribe`/`notify` or `addEventListener`/`removeEventListener`.
- **MEMENTO**: Classes with `save_state`/`restore_state` or `undo`/`redo` method pairs.
- **FLUENT-BUILDER**: Class where methods return `self`/`Self` (fluent/builder interface).
- **NULL-OBJECT**: Class implementing an interface where all methods are empty no-ops.

**Structural quality** (Checks 44-45, 56-57):

- **HUB-MODULE**: Single file imported by 10+ other files — bottleneck, changes ripple widely.
- **ORPHAN-MODULE**: File with no incoming calls or imports — potentially unused.
- **INCONSISTENT-NAMING**: Functions in the same class/module mix naming conventions.
- **CIRCULAR-PKG**: Directory-level circular dependency (package A ↔ package B).

**Type system suggestions** (Checks 58-63):

- **TAGGED-UNION**: Class with a `type`/`kind`/`tag` discriminator field + branching on it — should be an enum with variants.
- **HIERARCHY-TO-ENUM**: Leaf subclasses with no fields, only behavior differences — could be enum variants.
- **BOOL-BLINDNESS**: Function takes 2+ booleans, or a boolean that controls completely different behavior paths — use an enum.
- **SUGGEST-NEWTYPE**: Struct wrapping exactly one primitive field — consider a newtype for type safety.
- **SUGGEST-SEALED**: Interface with all implementors in the same file/module — effectively a closed sum type.
- **LARGE-PRODUCT**: Struct with 10+ fields, many optional — consider decomposing or using a builder.

**More anti-patterns** (Checks 46-50, 64-69):

- **DIVERGENT-CHANGE**: One file has functions called by many different modules — changes for many different reasons.
- **PARALLEL-INHERIT**: Creating a subclass of A always requires creating a subclass of B.
- **PRIM-OBSESSION**: Functions with 3+ params all typed as primitive types — no domain types used.
- **LARGE-CLASS**: Class/file with 500+ lines of source code.
- **UNSTABLE-DEP**: Module depends on something with very high fan-in (frequently changed).
- **ANEMIC-DOMAIN**: Class with only getters/setters, no real behavior methods.
- **MAGIC-NUMBER**: Literal constants (not 0/1/-1/""/true/false) hardcoded in function bodies.
- **MUTABLE-GLOBAL**: Module-level mutable variables that can cause hidden state bugs.
- **EMPTY-CATCH**: try/catch blocks with empty or no-op error handlers (swallowed errors).
- **CALLBACK-HELL**: Deeply nested callbacks/closures (3+ levels).
- **API-INCONSISTENCY**: Similar functions (by name prefix) with different param ordering or return types.

**Metrics** (Checks 70-73):

- **LOW-COHESION**: Methods in a class that don't share instance fields (LCOM) — class should be split.
- **HIGH-COUPLING**: Class depends on many other classes (CBO) — fragile, hard to test in isolation.
- **MOD-INSTABILITY**: Module has many more outgoing dependencies than incoming — likely to change.
- **COGNITIVE-COMPLEXITY**: Function with high cognitive complexity (penalizes nesting depth, not just branch count).

**How to use the tiers**:

```
# Start with just critical + high for quick wins
analyze_redundancy { min_tier: "high" }

# If refactoring deeply, include medium
analyze_redundancy { min_tier: "medium" }

# Full audit
analyze_redundancy { min_tier: "low" }
```

**After finding redundancy, validate with the call graph**:
```
analyze_relationships { name: "suspected_wrapper", relationship: "callers" }
analyze_relationships { name: "suspected_wrapper", relationship: "callees" }
```

If a passthrough wrapper has many callers, it may be a legitimate facade. If it has one caller, it's probably safe to inline.

**Recommendation format**:
```
[CRITICAL/PASSTHROUGH] `parse_config` → `read_settings`
  - parse_config just forwards (path, defaults) to read_settings
  - Callers: only main() calls parse_config
  - Suggestion: Replace calls to parse_config with direct calls to read_settings

[MEDIUM/MERGE] `process_csv` and `process_json`
  - Share 65% of lines (validation, output, error handling)
  - Differ in: parsing logic (lines 12-25 vs 12-30)
  - Suggestion: Extract shared logic into process_data(parser: impl Parser)
```

**Composite risk score** (Checks 74-75):

- **HIGH-RISK-FUNC**: Function with high composite risk score combining: cognitive complexity, line count, fan-in, TODO count, test coverage, and mutability. Use this as a "start here" signal for refactoring.
- **HIGH-RISK-FILE**: File-level risk score combining average function complexity, file size, documentation level, and test status.

**Test coverage gaps** (Checks 76-78):

- **UNTESTED-PUBLIC**: Public function with no test coverage (no TESTS edge, not called from test files). High risk for regressions.
- **LOW-TEST-RATIO**: File where fewer than 30% of functions have test coverage.
- **INTEGRATION-SMELL**: Test function that touches 4+ files transitively — may be an integration test disguised as a unit test.

**Change blast radius** (Check 79):

- **HIGH-BLAST-RADIUS**: Changing this function would transitively affect 5+ modules. Uses BFS on reverse call graph to compute full impact. Consider stabilizing the interface before refactoring.

**Semantic clustering** (Checks 80-81):

- **MISPLACED-FUNC**: Function that interacts more with another file than its own — it may belong in the other file.
- **IMPLICIT-MODULE**: Group of 5+ tightly coupled functions spanning multiple files that form a natural module boundary.

**API surface analysis** (Checks 82-84):

- **UNSTABLE-API**: Public function with 5+ callers and 4+ parameters — changing its signature has high blast radius.
- **UNDOCUMENTED-API**: Public function with callers but no docstring.
- **LEAKY-ABSTRACTION**: Public function exposing internal/private types (underscore-prefixed) in its signature.

**Cross-language boundaries** (Checks 85-87):

- **FFI-BOUNDARY**: Function or module using foreign function interface (extern C, ctypes, cffi, wasm_bindgen, PyO3, JNI, N-API, cgo). Changes affect both sides of the language boundary.
- **SUBPROCESS**: Function spawning an external process (subprocess.run, Command::new, child_process.exec, etc.). Cross-process boundary with different error/lifecycle semantics.
- **IPC-BOUNDARY**: Module or function using inter-process/remote communication (gRPC, protobuf, Kafka, Redis, ZeroMQ, WebSocket, REST endpoints). Data crosses process/network boundaries.

**Configuration detection** (Checks 88-91):

- **ENV-VAR**: Function reads environment variables (std::env::var, os.environ, process.env, etc.). Behavior depends on deployment configuration.
- **HARDCODED-ENDPOINT**: Function contains hardcoded URL or IP address that should be a configuration value.
- **FEATURE-FLAG**: Function uses feature flags or conditional compilation (#[cfg(feature)], #ifdef, LaunchDarkly, etc.). Behavior varies by configuration.
- **CONFIG-FILE**: Function references a config file (.env, config.yaml, settings.json, Cargo.toml, etc.). Depends on external configuration.

You can also use the simpler `find_similar` tool for quick structural grouping without the tiered analysis:
```
find_similar { kind: "Function", min_lines: 8 }
find_similar { kind: "Struct" }
```

### 3. Suggest module reorganization

**Goal**: Help split a large codebase into better-organized modules.

```
1. index_directory { path: "/project", annotate: true }
2. get_stats {}                                          # understand scale
3. find_similar { min_lines: 5 }                         # find code that belongs together
4. find_code { query: "", kind: "Module" }               # see current module structure
5. analyze_relationships { name: "BigModule", relationship: "children" }
```

**Strategy**:
- Modules with many children (>20 functions/classes) are candidates for splitting
- Groups of similar functions scattered across modules suggest a missing abstraction
- Functions that call each other heavily but live in different modules might belong together
- Dead code (`find_dead_code`) can be removed before reorganizing

### 4. Trace a bug or understand a feature

**Goal**: Follow the call chain from an entry point to understand how a feature works.

```
1. find_code { query: "handle_request" }
2. analyze_relationships { name: "handle_request", relationship: "call_chain", max_depth: 5 }
3. analyze_relationships { name: "handle_request", relationship: "callers" }
```

### 5. Assess impact of a change

**Goal**: Before modifying a function, understand who depends on it.

```
1. analyze_relationships { name: "target_function", relationship: "callers" }
2. analyze_relationships { name: "TargetClass", relationship: "implementors" }
3. analyze_relationships { name: "TargetClass", relationship: "inheritance" }
```

### 6. Find dead code to clean up

**Goal**: Identify safe deletion candidates.

```
1. find_dead_code { limit: 100 }
```

**Caveats**:
- Entry points (`main`, handlers, test functions) will show as "dead" because nothing in the codebase calls them — they're called externally
- Functions referenced via reflection, decorators, or dynamic dispatch may appear dead
- Check if a function is exported/public before recommending deletion

## Tool Reference (Quick)

| Tool | When to use |
|------|-------------|
| `index_directory` | First step — always index before querying. Use `annotate: true` for similarity analysis. |
| `find_code` | Search by name. Use `kind` to filter (Function, Class, Struct, Trait, Interface, Enum, Variable, Module). |
| `analyze_relationships` | Trace connections. Relationships: `callers`, `callees`, `inheritance`, `call_chain`, `implementors`, `children`. |
| `find_dead_code` | Functions with zero callers. Review before recommending deletion. |
| `find_complex_functions` | High cyclomatic complexity = high risk. Good refactoring targets. |
| `find_similar` | Groups of structurally similar code. Requires `annotate: true` on index. |
| `analyze_redundancy` | **Tiered** redundancy analysis: passthroughs, near-duplicates, merge/split candidates. Requires `annotate: true`. |
| `get_stats` | Quick overview of graph size and composition. |
| `list_repositories` | See what's already indexed (persists within a session). |

## Graph Concepts

**Nodes** represent code elements:
- `Repository` > `Directory` > `File` > `Function` / `Class` / `Struct` / etc.
- Each node has a name, file path, line span, and (if annotated) source snippet

**Edges** represent relationships:
- `CONTAINS` — structural hierarchy (file contains function)
- `CALLS` — function invocations (with line number and arguments)
- `IMPORTS` — module dependencies
- `INHERITS` — class inheritance
- `IMPLEMENTS` — interface/trait implementation
- `TESTS` — test function → the production function it verifies (auto-detected by naming convention)

**Enriched node data** (always available, no flag needed):
- Functions carry: `is_async`, `arg_types`, `return_type`, `visibility`, `is_static`, `is_abstract`, `todo_comments` (TODO/FIXME/HACK text found in the body), `raises` (exception types thrown), `has_error_handling`
- Classes and structs carry: `fields` (typed field declarations with visibility)
- Files carry: `public_count`, `private_count`, `comment_line_count`, `total_lines`, `is_test_file`

**Annotations** (`--annotate` / `annotate: true`):
- Attaches actual source code to each node
- Enables `find_similar` and lets you read function bodies directly from the graph
- Increases graph size by ~40% — only enable when needed

## Supported Languages

Python, Rust, TypeScript, JavaScript, Go, Java, C, C++, C#, Ruby, PHP, Swift, Dart

*(Note: Kotlin support is currently a TODO due to upstream parser dependencies).*

Each language has a dedicated tree-sitter parser that extracts language-specific constructs (e.g., Rust traits, Go interfaces, Python decorators, Java annotations).

## Tips for Agents

- **Always index with `annotate: true`** if you plan to do similarity analysis or need to read source code
- **Index once per session** — the graph persists in memory across tool calls
- **Combine tools** — use `find_similar` to find candidates, then `analyze_relationships` to validate them
- **Don't trust similarity blindly** — structurally similar code may be intentionally duplicated (e.g., platform-specific code, test fixtures, generated code)
- **Use `find_code` broadly first** — searching with a partial name casts a wide net, then narrow down with `kind` filter
- **The graph is static** — it reflects the code at index time. If you suggest changes, the user needs to re-index to see the updated graph
