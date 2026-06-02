# LLM-in-the-loop eval — tokens per correct answer

Measures the tool's actual value proposition: does an agent answer codebase
questions **more cheaply** with `ast_context`'s code-graph tools than by reading
raw files — *without losing accuracy*?

For each task, the same Gemini agent answers the question twice:

| Arm | Tools |
|-----|-------|
| `files` | `read_file`, `grep`, `list_dir` (the baseline) |
| `ast_context` | the real MCP code-graph tools (`find_code`, `get_context_for_symbol`, `get_overview`, `analyze_redundancy`), proxied to a live `ast_context mcp` server |

It records total tokens (from Gemini's `usage_metadata`) and grades each answer
against substring oracles, then reports **tokens-per-correct-answer** per arm.
A token saving only counts if the tool arm stays at least as accurate.

This is **not** run by CI — it calls the Gemini API and costs quota.

## Run it

```sh
# 1. Build the binary the MCP arm drives (release preferred):
cargo build --release            # from the crate root

# 2. Get a Gemini API key: https://aistudio.google.com/apikey
export GEMINI_API_KEY=...

# 3. Run via the launcher (puts the venv on /tmp — see note below):
cd eval
./run.sh
```

**Why `./run.sh` and not `uv run` directly:** this repo lives on an external
drive where uv can't hardlink and copy-installs instead, which corrupts namespace
packages like `google.genai` (`ImportError: cannot import name 'types' ...
unknown location`). The launcher forces the virtualenv onto the internal SSD
(`/tmp/ast-eval-venv`), which sidesteps that *and* keeps a large `.venv` out of
the project so your editor doesn't choke watching it. If you run `uv run`
directly, set `export UV_PROJECT_ENVIRONMENT=/tmp/ast-eval-venv` yourself first.

**Run a subset** (iterate on just the expensive tasks without paying for the
ones already at/below grep) by passing task ids:

```sh
./run.sh enum-opportunity passthrough-walker
# or: EVAL_TASKS=enum-opportunity,passthrough-walker ./run.sh
```

Optional environment overrides:

- `GEMINI_MODEL` — default `gemini-2.5-flash`; set `gemini-2.5-pro` for the stronger model.
- `AST_CONTEXT_BIN` — path to the `ast_context` binary (otherwise auto-detected
  under `target/release` or `target/debug`, then `PATH`).

## Adding tasks

Edit `tasks.json`. Each entry:

```json
{
  "id": "short-name",
  "question": "...",
  "expect_all": ["substring that MUST appear"],
  "expect_any": ["one", "of", "these", "must", "appear"]
}
```

Grading is case-insensitive substring matching — deliberately lenient.

**Open-ended design questions** ("where should this be an enum?", "find a facade")
can't be graded by substrings — the files arm may give a different-but-valid
answer. Mark those with `"judge": true` and a `rubric`; they're graded by an
LLM-as-judge (one extra model call per task, reported as separate overhead, not
charged to either arm):

```json
{
  "id": "enum-opportunity",
  "judge": true,
  "question": "Identify one place that would be clearer as an enum, and where.",
  "rubric": "PASS if the answer names a specific type/field and why an enum fits; FAIL if vague."
}
```

These design tasks are where ast_context's `analyze_redundancy` / `find_similar`
tools should show the biggest token win (one tool call vs. reading the repo), but
the interesting signal becomes answer *quality* — hence the judge. The *precision*
of the suggestions themselves is measured separately and for free (no API) by
`cargo run --example calibrate_threshold`.

## Precision audit (`fp_audit.py`)

A separate tool that measures the **false-positive rate** of redundancy findings.
The cheap, high-volume false positives (dead code via macro/dynamic-dispatch
calls, untested-public noise) are already handled structurally in the engine — but
the pattern-matched optimization checks (`CloneInLoop`, `VecNoPresize`, …) need
semantic judgment. This samples findings, fetches each one's source, and has an
LLM grade it TRUE/FALSE positive, reporting precision per check:

```sh
export GEMINI_API_KEY=...
uv run fp_audit.py            # default category: optimization
uv run fp_audit.py redundancy # any analyze_redundancy category
EVAL_FP_N=40 uv run fp_audit.py
```

Like the main eval, it's not run by CI (it costs API quota).

## Caveats

- Token counts come straight from the API response, so they're exact (not the
  `chars/4` estimate used by `examples/token_ratio.rs`).
- The MCP arm indexes the repo once up front; that indexing cost is *not* charged
  to either arm's token budget (it's a one-time setup an agent pays once per
  session, the same way the cache works in real use).
- Keep the task set small while iterating — every task is two full agent runs.
