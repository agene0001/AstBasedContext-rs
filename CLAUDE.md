# AstBasedContext-rs — Dev Guide for Claude

## Project Overview

Rust rewrite of CodeGraphContext. Parses source code via tree-sitter CSTs, builds a petgraph directed graph, and exposes it via CLI + MCP server.

## Build & Test

```
cargo build          # debug build
cargo build --release
cargo test           # runs all tests (29 currently)
cargo clippy         # lint
```

## Workspace Structure

```
src/
  main.rs              # CLI entry point + subcommand dispatch
  mcp/mod.rs           # MCP server loop (run_server())
  mcp/protocol.rs      # JSON-RPC 2.0 types
  mcp/tools.rs         # MCP tool definitions and handlers
  setup.rs             # `ast_context setup` — auto-configure editors
```

The single `ast_context` binary serves both purposes:
- `ast_context <cli-command>` — code analysis CLI
- `ast_context mcp` — starts the MCP server (editors configure this command)

## Key Modules (ast_context)

- `parser/mod.rs` — `LanguageParser` trait + `parser_for_extension()` dispatcher
- `parser/<lang>.rs` — one file per language (python, rust_lang, typescript, javascript, go, java, c_lang, cpp, csharp, ruby, php, swift, dart)
- `graph/builder.rs` — two-pass graph builder (Pass 1: nodes + contains/imports; Pass 2: resolve cross-file calls/inherits)
- `graph/code_graph.rs` — `CodeGraph` wrapping petgraph `DiGraph<GraphNode, EdgeKind>`
- `graph/query.rs` — query methods on `CodeGraph`
- `types/node.rs` — `GraphNode` enum with all node types
- `types/edge.rs` — `EdgeKind` enum
- `types/parse_result.rs` — `FileParseResult` (output of each parser)
- `walker.rs` — .gitignore-aware directory walker via `ignore` crate, supports `--exclude` patterns and `.astcontextignore` files
- `watcher.rs` — file watcher via `notify` crate with debounce
- `serialize.rs` — JSON/JSONL export

## Critical Patterns

### tree-sitter 0.24 — StreamingIterator (not Iterator)

`QueryMatches` and `QueryCaptures` implement `StreamingIterator`, NOT `Iterator`. You CANNOT use `for m in cursor.matches(...)`. Always use:

```rust
use streaming_iterator::StreamingIterator;
let mut matches = cursor.matches(&query, root_node, source);
while let Some(m) = { matches.advance(); matches.get() } {
    for cap in m.captures { ... }
}
```

### Two-pass graph building

- **Pass 1**: Walk all files, call `parser.parse()`, add nodes to graph, add CONTAINS and IMPORTS edges, build `imports_map: HashMap<String, Vec<String>>` (symbol name → file paths)
- **Pass 2**: Walk files again, resolve calls against `imports_map` to add CALLS edges cross-file

### Adding a new language parser

1. Create `src/parser/<lang>.rs` implementing `LanguageParser` trait
2. Add the `tree-sitter-<lang>` crate to `Cargo.toml`
3. Register it in `src/parser/mod.rs` → `parser_for_extension()`
4. Add extension mappings to `src/types/language.rs` → `Language::from_extension()`
5. Add pre-scan support in `src/graph/builder.rs` if the language uses imports

### FileParseResult structure

All parsers return `FileParseResult` with these fields populated:
- `functions: Vec<FunctionData>` — includes span, args, arg_types, return_type, visibility, is_static, is_abstract, decorators, cyclomatic_complexity
- `classes: Vec<ClassData>` — includes bases for inheritance, fields (typed field declarations)
- `imports: Vec<ImportData>` — module dependencies
- `calls: Vec<CallData>` — function calls with line number and args
- `variables: Vec<VariableData>` — module-level variables
- `traits: Vec<TraitData>`, `interfaces: Vec<InterfaceData>`, `structs: Vec<StructData>`, `enums: Vec<EnumData>`, `macros: Vec<MacroData>` — Phase 2 node types

### Source annotation (`--annotate`)

When `GraphBuilder::build_with_options(path, true)` is called, the `annotate` module (`src/annotate.rs`) extracts source snippets for every node using its span. This is a post-processing step after parsing — parsers don't need to know about it.

- All span-based node types get `source: Option<String>` (Function, Class, Struct, Trait, Interface, Enum, Macro)
- Snippets are truncated at 4KB per node
- `GraphNode::source_snippet()` accessor returns `Option<&str>`
- The `find_similar_nodes()` query uses Jaccard token similarity + line count ratio to group potentially redundant nodes
- `redundancy.rs` provides tiered analysis across 132 checks in 18 categories
- 132 check types across `FindingKind` enum, each assigned a `Tier` (Critical/High/Medium/Low)
- Checks 1-5: function-level redundancy (passthrough, near-duplicate, similar, merge, split)
- Checks 6-7: struct/enum overlap
- Checks 8-10: type suggestions (parameter struct, enum dispatch, trait extraction)
- Checks 11-18: architecture patterns (facade, factory, builder, strategy, template method, observer, decorator, mediator)
- Checks 19-22: anti-patterns (god class, circular deps, feature envy, shotgun surgery)
- Checks 23-28: pattern detection (singleton, adapter, proxy, command, chain of responsibility, DI)
- Checks 29-37: additional anti-patterns (dead code, long params, data clumps, middle man, lazy class, refused bequest, speculative generality, inappropriate intimacy, deep nesting)
- Checks 38-43: additional pattern detection (visitor, iterator, state, composite, repository, prototype)
- Checks 44-45: structural quality (hub module, orphan module)
- Checks 46-50: more anti-patterns (divergent change, parallel inheritance, primitive obsession, large class, unstable dependency)
- Checks 51-55: more pattern detection (flyweight, event emitter, memento, fluent builder, null object)
- Checks 56-57: more structural quality (inconsistent naming, circular package dependency)
- Checks 58-63: type system suggestions (tagged union→sum type, hierarchy→enum, boolean blindness, newtype, sealed type, large product type)
- Checks 64-69: more anti-patterns (anemic domain model, magic numbers, mutable global state, empty catch, callback hell, API inconsistency)
- Checks 70-73: metrics (lack of cohesion/LCOM, high coupling/CBO, module instability, cognitive complexity)
- Checks 74-75: composite risk score (high-risk function, high-risk file)
- Checks 76-78: test coverage gaps (untested public function, low test ratio, integration test smell)
- Check 79: change blast radius (transitive caller BFS)
- Checks 80-81: semantic clustering (misplaced function, implicit module)
- Checks 82-84: API surface analysis (unstable public API, undocumented public API, leaky abstraction)
- Checks 85-87: cross-language boundaries (FFI boundary, subprocess calls, IPC/RPC boundary)
- Checks 88-91: configuration detection (env var usage, hardcoded endpoints, feature flags, config file usage)
- Checks 92-99: data structure suggestions (vec-as-set, vec-as-map, linear search in loop, string concat in loop, sorted vec lookup, nested loop lookup, hashmap sequential keys, excessive collect-iterate)
- Checks 100-102: code quality (unused imports, inconsistent error handling, tech debt comments)
- Checks 103-132: optimization & resources (clone-in-loop, redundant collect, repeated lookup, vec-no-presize, sort-then-find, list-concat-loop, unbounded recursion, vectorization, polars suggestion, regex-recompile, memoization candidate, exception-for-control-flow, N+1 query, sync-async conflict, repeated-format-in-loop, sleep-in-loop, generator-over-list, unnecessary-chain, large-list-in, dict-keys-iter, unclosed-resource, enumerate-vs-range-len, yield-from, append-loop-extend, double-with, import-in-function, constant-condition, redundant-negation, default-dict-pattern, empty-string-check)
- This is opt-in because it increases graph size ~40%

### Graph save/load

```rust
graph.save(Path::new("graph.json"))?;
let graph = CodeGraph::load(Path::new("graph.json"))?;
// Indexes are automatically rebuilt on load
```

## MCP Server Protocol

JSON-RPC 2.0 over stdin/stdout. One JSON object per line.

Methods implemented:
- `initialize` → returns server info + capabilities
- `tools/list` → returns all tool definitions
- `tools/call` → dispatches to tool handler in `src/mcp/tools.rs`
- `ping` → empty response

## Dependencies of Note

| Crate | Version | Purpose |
|-------|---------|---------|
| `tree-sitter` | 0.24 | CST parsing |
| `tree-sitter-<lang>` | 0.23 | Language grammars |
| `streaming-iterator` | 0.1 | Required for tree-sitter 0.24 query API |
| `petgraph` | 0.7 + serde-1 | In-memory directed graph |
| `ignore` | 0.4 | .gitignore-aware dir walking |
| `notify` | 7 | File system events (watcher) |
| `notify-debouncer-full` | 0.4 | Debounced watcher events |
| `thiserror` | 2 | Error types |
| `clap` | 4 + derive | CLI argument parsing |

## Common Pitfalls

- `petgraph::visit::EdgeRef` must be in scope to call `.source()`/`.target()` on edge refs
- `path_index` and `name_index` in `CodeGraph` are `#[serde(skip)]` — always call `rebuild_indexes()` after deserialization
- tree-sitter `child_by_field_name()` only returns the **first** child with that field name; iterate children manually if multiple are expected (e.g. multiple `name` fields on an import statement)
- Decorators in Python CST are on the `decorated_definition` parent node, not on `function_definition`
- `typed_parameter` in Python has no `name` field — its first child IS the name identifier
