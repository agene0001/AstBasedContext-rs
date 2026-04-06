# AstBasedContext-rs

A Rust rewrite of [CodeGraphContext](https://github.com/CodeGraphContext/CodeGraphContext-rs) — builds a code graph from AST/CST analysis of your source code and exposes it to LLMs via an MCP server.

Supports **8 languages**: Python, Rust, TypeScript, JavaScript, Go, Java, C, C++.

## What it does

1. Walks your project directory (respecting `.gitignore`)
2. Parses every source file using [tree-sitter](https://tree-sitter.github.io/) CSTs
3. Extracts functions, classes, structs, traits, interfaces, enums, variables, imports, and call relationships
4. Builds a directed graph linking everything together
5. Exposes the graph via a CLI or an MCP server so LLMs can query it

## Installation

```
cargo build --release
```

Binaries will be at `target/release/ast_context_cli` and `target/release/ast_context_mcp`.

## CLI Usage

### Index a project

```
ast-context index <path> [--format stats|json|jsonl] [--save graph.json] [--annotate] [--exclude <pattern>...]
```

```
# Print summary stats
ast-context index ./my-project

# Save the graph to a file for later querying
ast-context index ./my-project --save graph.json

# Index with source annotations (enables similarity/redundancy detection)
ast-context index ./my-project --save graph.json --annotate

# Exclude directories/files (repeatable, gitignore glob syntax)
ast-context index ./my-project --exclude "vendor/**" --exclude "*.generated.go"

# Export as JSON or JSONL
ast-context index ./my-project --format json --output graph.json
ast-context index ./my-project --format jsonl --output output_dir/
```

#### `.astcontextignore`

Place a `.astcontextignore` file in your project root (or any subdirectory) to permanently exclude paths. Uses the same syntax as `.gitignore`:

```
# Skip vendored code
vendor/
third_party/

# Skip generated files
*.generated.go
*.pb.go
*_generated.ts
```

This is read automatically — no CLI flags needed. You can combine it with `--exclude` for one-off exclusions.

### Query a saved graph

```
# Search by name (all types)
ast-context search --graph graph.json "parse"

# Search for functions only
ast-context search --graph graph.json "parse" --kind Function

# Analyze relationships
ast-context analyze --graph graph.json "my_function" --relationship callers
ast-context analyze --graph graph.json "my_function" --relationship callees
ast-context analyze --graph graph.json "MyClass"    --relationship inheritance
ast-context analyze --graph graph.json "my_fn"      --relationship call_chain --depth 5
ast-context analyze --graph graph.json "MyTrait"    --relationship implementors
ast-context analyze --graph graph.json "MyModule"   --relationship children

# Find dead code (functions never called)
ast-context dead-code --graph graph.json --limit 50

# Find most complex functions (by cyclomatic complexity)
ast-context complexity --graph graph.json --limit 20
```

### Find similar/redundant code

Requires `--annotate` during indexing. Finds groups of structurally similar nodes based on token overlap and line count similarity.

```
# Find similar functions (great for finding consolidation opportunities)
ast-context similar --graph graph.json --kind Function --min-lines 8

# Find similar structs/classes
ast-context similar --graph graph.json --kind Struct

# Find all similar nodes across all types
ast-context similar --graph graph.json
```

This is designed for AI-assisted code review: the source snippets give an LLM enough context to identify genuinely redundant code even when names differ completely. Use cases:
- **Redundancy detection**: Find functions/classes that do the same thing
- **Consolidation**: Identify modules/packages that could be merged
- **Refactoring**: Help split large codebases into better modules based on what each node actually does

### Tiered redundancy analysis

Full redundancy, architecture, anti-pattern, and code quality analysis with confidence tiers (Critical > High > Medium > Low). **99 checks** spanning:

- **Redundancy**: passthrough wrappers, near-duplicates, merge/split candidates, overlapping structs/enums
- **Type suggestions**: parameter structs, enum dispatch, trait extraction
- **Architecture patterns**: facade, factory, builder, strategy, template method, observer, decorator, mediator, visitor, iterator, state, composite, repository, prototype, flyweight, event emitter, memento, fluent builder, null object
- **Detected patterns**: singleton, adapter, proxy, command, chain of responsibility, dependency injection
- **Anti-patterns**: god class, circular dependencies, feature envy, shotgun surgery, dead code, long parameter list, data clumps, middle man, lazy class, refused bequest, speculative generality, inappropriate intimacy, deep nesting, anemic domain model, magic numbers, mutable global state, empty catch, callback hell, API inconsistency, divergent change, parallel inheritance, primitive obsession, large class, unstable dependency
- **Type system suggestions**: tagged union → sum type, class hierarchy → enum, boolean blindness, newtype wrapper, sealed type, large product type
- **Structural quality**: hub module, orphan module, inconsistent naming, circular package dependency
- **Metrics**: LCOM (lack of cohesion), CBO (coupling between objects), module instability, cognitive complexity
- **Composite risk scores**: per-function and per-file risk score combining complexity, test coverage, fan-in, TODOs, mutability
- **Test coverage gaps**: untested public functions, low test ratio per file, integration test smells
- **Change blast radius**: transitive caller analysis showing how many modules a change would affect
- **Semantic clustering**: misplaced functions, implicit modules (tightly coupled code spanning files)
- **API surface**: unstable public APIs (many callers + many params), undocumented public APIs, leaky abstractions
- **Cross-language boundaries**: FFI boundaries (extern C, ctypes, wasm_bindgen, PyO3, JNI, N-API), subprocess/exec calls, IPC/RPC protocols (gRPC, protobuf, Kafka, WebSocket, REST endpoints)
- **Configuration detection**: environment variable reads, hardcoded URLs/endpoints, feature flags, config file references
- **Data structure suggestions**: Vec used as set (→ HashSet), Vec used as map (→ HashMap), linear search in loop, string concatenation in loop, sorted Vec for lookup (→ HashMap), nested loop lookup (→ HashMap), HashMap with sequential integer keys (→ Vec), excessive collect-then-iterate chains

```
# Show all findings
ast-context redundancy --graph graph.json

# Only critical + high confidence
ast-context redundancy --graph graph.json --tier high

# Only critical
ast-context redundancy --graph graph.json --tier critical

# Tune thresholds
ast-context redundancy --graph graph.json --split-complexity 20 --split-lines 80
```

### Watch for changes

```
ast-context watch ./my-project --debounce 2000
ast-context watch ./my-project --exclude "build/**"
```

Rebuilds the graph whenever files change. Useful during active development.

### Parse a single file

```
ast-context parse src/main.rs
```

Prints the raw parse result as JSON.

### List supported languages

```
ast-context languages
```

## MCP Server

The MCP server lets Claude (or any MCP-compatible LLM) query your code graph directly.

### Configure with Claude Desktop

Add to `~/Library/Application Support/Claude/claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "ast-context": {
      "command": "/path/to/ast_context_mcp"
    }
  }
}
```

### Available MCP Tools

| Tool | Description |
|------|-------------|
| `index_directory` | Index a directory and build its code graph |
| `find_code` | Search for functions/classes by name |
| `analyze_relationships` | Callers, callees, inheritance, call chains |
| `find_dead_code` | Find uncalled functions |
| `find_complex_functions` | Rank functions by cyclomatic complexity |
| `get_stats` | Node/edge counts by type |
| `list_repositories` | Show all indexed repositories |
| `find_similar` | Find groups of redundant/similar code (requires `annotate=true` on index) |
| `analyze_redundancy` | Tiered redundancy + architecture + anti-pattern + type system + risk + boundary analysis (99 checks across 4 tiers) |

### MCP Protocol

The server implements [JSON-RPC 2.0](https://www.jsonrpc.org/) over `stdin`/`stdout`, following the [Model Context Protocol spec](https://modelcontextprotocol.io/).

Example session:
```json
// Client sends:
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"index_directory","arguments":{"path":"/my/project"}}}
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"find_code","arguments":{"query":"parse","kind":"Function"}}}

// Server responds:
{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","serverInfo":{"name":"ast-context-mcp","version":"0.1.0"},...}}
{"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"Successfully indexed /my/project.\nGraph: 1317 nodes, 1904 edges."}]}}
{"jsonrpc":"2.0","id":3,"result":{"content":[{"type":"text","text":"Found 18 results for 'parse':\n..."}]}}
```

## Graph Node Types

| Type | Description |
|------|-------------|
| `Repository` | Root of the graph |
| `Directory` | Subdirectory |
| `File` | Source file. Carries `public_count`, `private_count`, `comment_line_count`, `total_lines`, `is_test_file` |
| `Function` | Function or method. Carries `cyclomatic_complexity`, `arg_types`, `return_type`, `visibility`, `is_static`, `is_abstract`, `is_async`, `todo_comments`, `raises`, `has_error_handling` |
| `Class` | Class. Carries `bases` (parent classes) and `fields` (typed field declarations) |
| `Struct` | Struct. Carries `fields` (typed field declarations) |
| `Trait` | Rust trait or similar |
| `Interface` | Go/Java/TypeScript interface |
| `Enum` | Enum (includes variant names) |
| `Variable` | Module-level or top-level variable |
| `Module` | External module/package |

## Edge Types

| Type | Description |
|------|-------------|
| `CONTAINS` | Parent → child containment |
| `CALLS` | Function → function call (with line number and args) |
| `IMPORTS` | File → module dependency |
| `INHERITS` | Class → parent class |
| `IMPLEMENTS` | Class → interface/trait |
| `HAS_PARAMETER` | Function → parameter variable |
| `TESTS` | Test function → the production function it tests |

## Workspace Structure

```
AstBasedContext-rs/
├── crates/
│   ├── ast_context_core/   # Core library: parsing, graph building, querying
│   │   └── src/
│   │       ├── parser/     # Language parsers (one file per language)
│   │       ├── graph/      # Graph data structure, builder, queries
│   │       ├── types/      # Node/edge types, language enum
│   │       ├── walker.rs   # Directory walker
│   │       ├── watcher.rs  # File watcher
│   │       └── serialize.rs # JSON/JSONL export
│   ├── ast_context_cli/    # CLI binary
│   └── ast_context_mcp/    # MCP server binary
└── Cargo.toml
```

## Running Tests

```
cargo test
```

29 tests covering the Python parser and graph builder.

## Future Work

### Opt-in LSP Integration (`--analyze --lsp`)

The data structure checks (92-99) currently use source pattern matching (e.g., detecting `.push()` + `.contains()` to suggest HashSet). An opt-in LSP integration would confirm variable types before making suggestions, reducing false positives.

**Benefits:**
- Type-confirmed data structure suggestions (e.g., verify a variable is actually a `Vec` before suggesting `HashSet`)
- Unnecessary `.clone()` detection
- Parameter type suggestions
- Redundant type conversion detection
- More accurate unused import detection

**Approach:**
- Start with `rust-analyzer` only (best LSP support, project is Rust-focused)
- Query `textDocument/hover` for type-at-position to enrich existing findings
- Degrade gracefully with timeouts if the LSP is slow or unavailable
- Expand to other language servers one at a time, since each has different startup/protocol quirks
