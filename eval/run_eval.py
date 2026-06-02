"""Run the two-arm eval and report tokens-per-correct-answer.

For each task, the same Gemini agent answers twice:
  - "files"       — only read_file / grep / list_dir
  - "ast_context" — the real MCP code-graph tools (server auto-indexes the repo)

We then compare correctness (vs. substring oracles) and total tokens consumed.
The headline number is tokens-per-correct-answer: lower is better, and it only
counts as a win if the tool arm stays at least as accurate as the files arm.
"""

from __future__ import annotations

import json
import os
import sys
from pathlib import Path

import agent
from mcp_client import McpClient

REPO_ROOT = Path(__file__).resolve().parent.parent  # the Rust crate root


def find_binary() -> str:
    if os.environ.get("AST_CONTEXT_BIN"):
        return os.environ["AST_CONTEXT_BIN"]
    for rel in ("target/release/ast_context", "target/debug/ast_context"):
        p = REPO_ROOT / rel
        if p.exists():
            return str(p)
    return "ast_context"  # assume on PATH


def grade(answer: str, task: dict) -> bool:
    a = answer.lower()
    if not all(s.lower() in a for s in task.get("expect_all", [])):
        return False
    any_list = task.get("expect_any", [])
    if any_list and not any(s.lower() in a for s in any_list):
        return False
    return True


def select_tasks(tasks: list[dict]) -> list[dict]:
    """Optionally run a subset, by id, so you can iterate on just the expensive
    tasks without paying for the ones already at/below grep. Ids come from argv
    or EVAL_TASKS (comma-separated), e.g.:
        ./run.sh enum-opportunity passthrough-walker
        EVAL_TASKS=enum-opportunity,passthrough-walker ./run.sh
    """
    wanted = list(sys.argv[1:])
    if os.environ.get("EVAL_TASKS"):
        wanted += [w.strip() for w in os.environ["EVAL_TASKS"].split(",")]
    wanted = {w for w in wanted if w}
    if not wanted:
        return tasks
    chosen = [t for t in tasks if t["id"] in wanted]
    if not chosen:
        raise SystemExit(
            f"No tasks matched {sorted(wanted)}.\nAvailable: {[t['id'] for t in tasks]}"
        )
    missing = wanted - {t["id"] for t in chosen}
    if missing:
        print(f"(ignoring unknown task ids: {sorted(missing)})")
    return chosen


def main() -> None:
    tasks = select_tasks(json.loads((Path(__file__).parent / "tasks.json").read_text()))
    client = agent.make_client()

    # ── Preflight: make sure the chosen model actually exists for this key ──
    try:
        models = agent.available_models(client)
    except Exception as e:  # e.g. bad API key
        raise SystemExit(f"Could not list models (check GEMINI_API_KEY): {e}")
    if agent.MODEL not in models:
        print(f"Model '{agent.MODEL}' is not available on this API key.\n")
        print("Pick one of these with GEMINI_MODEL, e.g.:")
        print("  export GEMINI_MODEL=gemini-2.5-flash\n")
        print("Available models that support generateContent:")
        for m in models:
            print(f"  {m}")
        raise SystemExit(1)
    print(f"Model: {agent.MODEL}\n")

    # ── Arm B setup: start the MCP server and index the repo once ──────────
    mcp = McpClient(find_binary())
    print("Indexing repository into the code graph (forced fresh, no cache)...")
    # force_reindex so a stale .ast_context_cache.json can never confound results.
    idx = mcp.call_tool(
        "index_directory",
        {
            "path": str(REPO_ROOT),
            "annotate": True,
            "force_reindex": True,
            # grammars/ is vendored generated parsers (e.g. a 600-variant enum) —
            # not the project's own code, just noise for these questions.
            "exclude": ["grammars/**", "target/**", "eval/**"],
        },
    )
    print(f"  {idx.splitlines()[-1] if idx else '(no output)'}\n")
    # Hide session-management tools from the model — the harness already indexed
    # exactly one repo, so the agent never needs to list/index/save/load. Leaving
    # them in just lets the model waste turns (e.g. list_repositories) and inflates
    # the per-turn tool-definition overhead. This also keeps the comparison fair:
    # the files arm only has query tools too.
    SESSION_TOOLS = {"index_directory", "save_graph", "load_graph", "list_repositories"}
    mcp_tools = [t for t in mcp.list_tools() if t["name"] not in SESSION_TOOLS]

    file_exec = agent.FileToolExecutor(REPO_ROOT)
    files_total = agent.Usage()
    ast_total = agent.Usage()
    judge_total = agent.Usage()  # grading overhead, not attributed to either arm
    files_correct = ast_correct = 0

    def evaluate(answer: str) -> bool:
        if task.get("judge"):
            passed, verdict, u = agent.judge_answer(
                client, task["question"], answer, task.get("rubric", "")
            )
            _acc(judge_total, u)
            return passed
        return grade(answer, task)

    header = f"{'task':<18} {'arm':<12} {'ok':<3} {'tokens':>8} {'turns':>6}"
    print(header)
    print("-" * len(header))

    for task in tasks:
        # Arm A — file reading.
        a = agent.run_agent(
            client,
            agent.FILE_TOOLS,
            file_exec,
            task["question"],
            extra_system=" The repository root is a Rust crate; source lives under src/.",
        )
        ok_a = evaluate(a.answer)
        files_correct += ok_a
        _acc(files_total, a.usage)
        print(f"{task['id']:<18} {'files':<12} {('Y' if ok_a else 'n'):<3} {a.usage.total:>8} {a.usage.turns:>6}")

        # Arm B — ast_context code-graph tools.
        b = agent.run_agent(
            client,
            mcp_tools,
            lambda n, args: mcp.call_tool(n, args),
            task["question"],
            extra_system=(
                f" The repository at {REPO_ROOT} has already been indexed into a code "
                "graph. Prefer the code-graph query tools over reading whole files."
            ),
        )
        ok_b = evaluate(b.answer)
        ast_correct += ok_b
        _acc(ast_total, b.usage)
        print(f"{task['id']:<18} {'ast_context':<12} {('Y' if ok_b else 'n'):<3} {b.usage.total:>8} {b.usage.turns:>6}")

    mcp.close()

    n = len(tasks)
    print("\n== Summary ==")
    _report("files", files_total, files_correct, n)
    _report("ast_context", ast_total, ast_correct, n)
    if judge_total.total:
        print(f"  (LLM-judge grading overhead: {judge_total.total} tokens, not counted above)")

    if files_total.total and ast_total.total:
        print(
            f"\nast_context used {100 * ast_total.total / files_total.total:.0f}% of the "
            f"files arm's tokens, at {ast_correct}/{n} vs {files_correct}/{n} correct."
        )


def _acc(dst: agent.Usage, src: agent.Usage) -> None:
    dst.prompt_tokens += src.prompt_tokens
    dst.output_tokens += src.output_tokens
    dst.cached_tokens += src.cached_tokens
    dst.thinking_tokens += src.thinking_tokens
    dst.turns += src.turns


def _report(name: str, u: agent.Usage, correct: int, n: int) -> None:
    per_correct = u.total / correct if correct else float("inf")
    print(
        f"  {name:<12} correct={correct}/{n}  total_tokens={u.total}  "
        f"(prompt={u.prompt_tokens} output={u.output_tokens} thinking={u.thinking_tokens} "
        f"cached={u.cached_tokens})  tokens/correct={per_correct:.0f}"
    )


if __name__ == "__main__":
    try:
        main()
    except agent.QuotaExhausted:
        print(
            f"\nHit the Gemini free-tier DAILY request quota for '{agent.MODEL}'.\n"
            "Retrying won't help — it resets around midnight Pacific. Best options:\n"
            "  • Enable pay-as-you-go billing on the key (Google AI Studio → Billing).\n"
            "    A full run on flash costs well under a cent and removes the tiny\n"
            "    free daily cap + low per-minute limit that force all the retries.\n"
            "  • Or shrink the run: keep just 1-2 entries in tasks.json and run a\n"
            "    few per day until the quota resets.",
        )
        raise SystemExit(1)
