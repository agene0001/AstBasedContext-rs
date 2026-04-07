# AstBasedContext-rs

A Rust implementation of Ast based Context — builds a code graph from AST/CST analysis of your source code and exposes it to LLMs via an MCP server.

Supports **13 languages**: Python, Rust, TypeScript, JavaScript, Go, Java, C, C++, C#, Ruby, PHP, Swift, Dart. *(Note: Kotlin is currently a TODO due to upstream parser dependencies).*

## What it does

1. Walks your project directory (respecting `.gitignore`)
2. Parses every source file using [tree-sitter](https://tree-sitter.github.io/) CSTs
3. Extracts functions, classes, structs, traits, interfaces, enums, variables, imports, and call relationships
4. Builds a directed graph linking everything together
5. Exposes the graph via a CLI or an MCP server so LLMs can query it

## Installation

```
cargo install ast_context
```

This installs the `ast_context` binary which handles both CLI code analysis and the MCP server.

### Build from source

If you prefer to build from source instead:

```
git clone https://github.com/agene0001/AstBasedContext-rs.git
cd AstBasedContext-rs
cargo install --path .
```

This will compile the project and install the `ast_context` binary to your Cargo bin directory.

Once installed (either from crates.io or from source), run the setup command to automatically configure your editors with the MCP server:

```
ast_context setup
```

## CLI Usage

### Index a project

```
ast_context index <path> [--format stats|json|jsonl] [--save graph.json] [--annotate] [--exclude <pattern>...]
```

```
# Print summary stats
ast_context index ./my-project

# Save the graph to a file for later querying
ast_context index ./my-project --save graph.json

# Index with source annotations (enables similarity/redundancy detection)
ast_context index ./my-project --save graph.json --annotate

# Skip test files for a smaller, faster graph focused on production code
ast_context index ./my-project --skip-tests

# Exclude directories/files (repeatable, gitignore glob syntax)
ast_context index ./my-project --exclude "vendor/**" --exclude "*.generated.go"

# Set a custom file size limit in MB (default: 50MB — skips huge auto-generated files)
ast_context index ./my-project --annotate --save graph.json --max-file-size 20

# Export as JSON or JSONL
ast_context index ./my-project --format json --output graph.json
ast_context index ./my-project --format jsonl --output output_dir/
```

#### `.astcontextignore` and `.astcontextignore.local`

Place an `.astcontextignore` file in your project root (or any subdirectory) to permanently exclude paths. You can also use `.astcontextignore.local` for per-user exclusions that you don't want to commit to git. Both use the same syntax as `.gitignore`:

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
ast_context search --graph graph.json "parse"

# Search for functions only
ast_context search --graph graph.json "parse" --kind Function

# Analyze relationships
ast_context analyze --graph graph.json "my_function" --relationship callers
ast_context analyze --graph graph.json "my_function" --relationship callees
ast_context analyze --graph graph.json "MyClass"    --relationship inheritance
ast_context analyze --graph graph.json "my_fn"      --relationship call_chain --depth 5
ast_context analyze --graph graph.json "MyTrait"    --relationship implementors
ast_context analyze --graph graph.json "MyModule"   --relationship children

# Find dead code (functions never called)
ast_context dead-code --graph graph.json --limit 50

# Find most complex functions (by cyclomatic complexity)
ast_context complexity --graph graph.json --limit 20
```

### Find similar/redundant code

Requires `--annotate` during indexing. Finds groups of structurally similar nodes based on token overlap and line count similarity.

```
# Find similar functions (great for finding consolidation opportunities)
ast_context similar --graph graph.json --kind Function --min-lines 8

# Find similar structs/classes
ast_context similar --graph graph.json --kind Struct

# Find all similar nodes across all types
ast_context similar --graph graph.json
```

This is designed for AI-assisted code review: the source snippets give an LLM enough context to identify genuinely redundant code even when names differ completely. Use cases:
- **Redundancy detection**: Find functions/classes that do the same thing
- **Consolidation**: Identify modules/packages that could be merged
- **Refactoring**: Help split large codebases into better modules based on what each node actually does

### Tiered redundancy analysis

Full redundancy, architecture, anti-pattern, and code quality analysis with confidence tiers (Critical > High > Medium > Low). **102 checks** spanning:

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
ast_context redundancy --graph graph.json

# Only critical + high confidence
ast_context redundancy --graph graph.json --tier high

# Only critical
ast_context redundancy --graph graph.json --tier critical

# Tune thresholds and skip specific checks or entire categories
ast_context redundancy --graph graph.json \
  --split-complexity 20 --split-lines 80 \
  --near-dup-threshold 0.85 \
  --structural-threshold 0.55 \
  --merge-threshold 0.45 \
  --skip-check detect_dead_code,data_structures
```

### Watch for changes

```
ast_context watch ./my-project --debounce 2000
ast_context watch ./my-project --exclude "build/**"
```

Rebuilds the graph whenever files change. Useful during active development.

### Parse a single file

```
ast_context parse src/main.rs
```

Prints the raw parse result as JSON.

### List supported languages

```
ast_context languages
```

## MCP Server

The MCP server lets Claude (or any MCP-compatible LLM) query your code graph directly.

### Quick setup

After installing, run the setup command once to auto-configure every detected editor:

```
ast_context setup
```

This detects and configures:
- **Claude Desktop** — macOS & Windows
- **Claude Code** — via `claude mcp add`
- **Zed** — `~/.config/zed/settings.json`
- **Cursor** — `~/.cursor/mcp.json`
- **Windsurf** — `~/.codeium/windsurf/mcp_config.json`
- **VS Code** (GitHub Copilot, v1.99+) — user-level `mcp.json`
- **JetBrains IDEs** (IntelliJ, PyCharm, GoLand, WebStorm, …) — all detected installs

```
# Preview what would be changed without modifying anything
ast_context setup --dry-run

# Override the binary path if auto-detection fails
ast_context setup --mcp-path /custom/path/to/ast_context
```

Restart your editor after running setup. Then ask your AI assistant to index your project:

```
Index /path/to/my-project with annotations
```

### Manual configuration

If you prefer to configure manually or use an unsupported editor:

### Configure with Claude Desktop / Claude Code

Add to `~/Library/Application Support/Claude/claude_desktop_config.json` (macOS) or `%APPDATA%\Claude\claude_desktop_config.json` (Windows):

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

### Configure with Zed

Add to `~/.config/zed/settings.json`:

```json
{
  "context_servers": {
    "ast-context": {
      "command": {
        "path": "ast_context",
        "args": ["mcp"]
      }
    }
  }
}
```

After configuring, ask your AI assistant to index your project:

```
Index /path/to/my-project with annotations, excluding node_modules and vendor
```

### Available MCP Tools

| Tool | Description |
|------|-------------|
| `index_directory` | Index a directory and build its code graph. Auto-caches to `.ast_context_cache.json` — subsequent calls load from cache instantly if no source files have changed. Pass `force_reindex=true` to rebuild, or `skip_tests=true` to exclude tests. |
| `find_code` | Search for functions/classes/structs by name (partial match, case-insensitive) |
| `get_file_summary` | List all symbols defined in a specific file — great for understanding a file before editing it |
| `get_source` | Retrieve the source snippet for a named symbol (requires `annotate=true` on index) |
| `get_context_for_symbol` | All context needed before editing a symbol: source, callers, callees, and similar functions in one call |
| `find_references` | All usages of a symbol: callers, inheritors, implementors, importers, and test functions |
| `get_module_overview` | Directory-level summary: files, line counts, public symbols, and cross-file dependencies |
| `analyze_relationships` | Callers, callees, inheritance, call chains, implementors, children |
| `find_dead_code` | Find uncalled functions |
| `find_complex_functions` | Rank functions by cyclomatic complexity |
| `get_stats` | Node/edge counts by type |
| `list_repositories` | Show all indexed repositories |
| `find_similar` | Find groups of redundant/similar code (requires `annotate=true` on index) |
| `analyze_redundancy` | Tiered redundancy + architecture + anti-pattern + type system + risk + boundary analysis (102 checks across 4 tiers, requires `annotate=true`) |
| `save_graph` | Save the in-memory graph to a file for manual archiving or sharing |
| `load_graph` | Load a previously saved graph into the session |

All query tools accept an optional `repository` parameter to target a specific indexed directory when multiple repos are loaded.

### Session persistence

The first time you index a project the graph is saved to `{project}/.ast_context_cache.json` (automatically added to `.gitignore`). In subsequent sessions, calling `index_directory` on the same path will:

- Load from cache instantly if no source files have changed **and** the configuration (like `annotate`, `exclude`, or `skip_tests`) is identical
- Automatically re-index if any source file is newer than the cache, or if the indexing configuration fingerprint has changed
- Rebuild unconditionally if `force_reindex=true` is passed

This means you can safely call `index_directory` at the start of every session without worrying about performance.

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
├── src/
│   ├── parser/      # Language parsers (one file per language)
│   ├── graph/       # Graph data structure, builder, queries
│   ├── types/       # Node/edge types, language enum
│   ├── mcp/         # MCP server implementation
│   ├── redundancy/  # Redundancy analysis and tiered checks
│   ├── walker.rs    # Directory walker
│   ├── watcher.rs   # File watcher
│   ├── serialize.rs # JSON/JSONL export
│   └── main.rs      # Unified binary entry point (CLI + MCP)
└── Cargo.toml
```

## Running Tests

```
cargo test
```

29 tests covering the Python parser and graph builder. More language-specific tests are a good contribution target.

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
