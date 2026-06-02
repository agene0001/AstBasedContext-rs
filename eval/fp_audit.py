"""Precision audit: sample redundancy findings and have an LLM judge each as a
TRUE or FALSE positive, producing a per-check precision number.

This complements the *free* structural audits already baked into the engine (the
dead-code reference guard, the untested-public bar) by covering the
pattern-matched checks — CloneInLoop, VecNoPresize, and the other optimization
heuristics — that need semantic judgment a static rule can't give.

Usage (from eval/, after `cargo build --release`):
    export GEMINI_API_KEY=...
    ./run.sh                      # not needed; or run directly via uv:
    uv run fp_audit.py [category] # default category: optimization

Env:
    EVAL_FP_N      findings to sample (default 25)
    GEMINI_MODEL   judge model (default gemini-3.1-pro-preview)

It is NOT run by CI — it calls the Gemini API and costs quota.
"""

from __future__ import annotations

import os
import re
import sys
from collections import defaultdict
from pathlib import Path

from google.genai import types

import agent
from mcp_client import McpClient
from run_eval import find_binary

REPO_ROOT = Path(__file__).resolve().parent.parent

HEADER_RE = re.compile(r"^\[.\]\[([A-Z]+)\]\s*(.*)")
# matches `name(kind)(path:line)` in the "└─ ..." member line
SYMBOL_RE = re.compile(r"([A-Za-z_][A-Za-z0-9_]*)\([a-z]+\)\(")

JUDGE_SYS = (
    "You audit static-analysis findings about a multi-language codebase. For each "
    "finding, given its description and (when available) the relevant source, decide "
    "if it is a TRUE positive (a real, actionable issue a competent developer would "
    "reasonably act on) or a FALSE positive (incorrect, not actionable, or noise). "
    "Be strict and concise."
)


def parse_findings(text: str) -> list[tuple[str, str, str | None]]:
    """Return [(tag, description, first_symbol_name | None)]."""
    out: list[tuple[str, str, str | None]] = []
    lines = text.splitlines()
    for i, line in enumerate(lines):
        m = HEADER_RE.match(line)
        if not m:
            continue
        tag, desc = m.group(1), m.group(2).strip()
        sym = None
        for j in range(i + 1, min(i + 5, len(lines))):
            sm = SYMBOL_RE.search(lines[j])
            if sm:
                sym = sm.group(1)
                break
        out.append((tag, desc, sym))
    return out


def fetch_source(mcp: McpClient, sym: str | None) -> str:
    """Best-effort: pull the symbol's source block via get_context."""
    if not sym:
        return ""
    ctx = mcp.call_tool("get_context_for_symbol", {"name": sym})
    if "```" in ctx:
        return ctx.split("```", 2)[1][:1500]
    return ctx[:600]


def judge_batch(client, batch) -> tuple[list[bool], str]:
    """batch: [(idx, tag, desc, src)] -> ([is_true_positive], raw_verdict)."""
    prompt = (
        "Judge each finding. Reply with exactly one line per finding in the form "
        "`<n> TP` or `<n> FP - short reason`.\n\n"
    )
    for n, (_, tag, desc, src) in enumerate(batch):
        prompt += f"[{n}] ({tag}) {desc}\n"
        if src:
            prompt += f"SOURCE:\n{src}\n"
        prompt += "\n"
    config = types.GenerateContentConfig(system_instruction=JUDGE_SYS, temperature=0.0)
    contents = [types.Content(role="user", parts=[types.Part.from_text(text=prompt)])]
    resp = agent._generate_with_retry(client, contents, config)
    raw = agent._response_text(resp)

    results = [True] * len(batch)  # default TP if a line can't be parsed (conservative)
    for line in raw.splitlines():
        vm = re.match(r"\s*\[?(\d+)\]?[).:\s]+(TP|FP)", line, re.IGNORECASE)
        if vm:
            n = int(vm.group(1))
            if 0 <= n < len(batch):
                results[n] = vm.group(2).upper() == "TP"
    return results, raw


def main() -> None:
    category = sys.argv[1] if len(sys.argv) > 1 else os.environ.get("EVAL_FP_CATEGORY", "optimization")
    n = int(os.environ.get("EVAL_FP_N", "25"))

    client = agent.make_client()
    if agent.MODEL not in agent.available_models(client):
        raise SystemExit(f"Model '{agent.MODEL}' unavailable; set GEMINI_MODEL.")
    print(f"Model: {agent.MODEL}  |  category: {category}  |  sample: {n}\n")

    mcp = McpClient(find_binary())
    # Two parts of THIS repo make it a poor precision target when auditing itself:
    #   - src/redundancy/** DEFINES the textual patterns the checks search for
    #     ("except KeyError", ".collect::<Vec", …) as string literals, so a self-scan
    #     flags the check functions against their own patterns;
    #   - test_project/** is intentional-anti-pattern FIXTURES (they exist to trigger
    #     the checks), so they're "false positives" by construction.
    # Both are dogfooding artifacts, not real-codebase behavior — exclude them. For
    # a meaningful number on language-specific checks (the Python ones can't fire on
    # Rust at all), point this at a real repo of that language via EVAL_FP_REPO.
    repo = os.environ.get("EVAL_FP_REPO", str(REPO_ROOT))
    mcp.call_tool(
        "index_directory",
        {"path": repo, "annotate": True, "force_reindex": True,
         "exclude": ["grammars/**", "target/**", "eval/**", "src/redundancy/**", "test_project/**"]},
    )
    text = mcp.call_tool(
        "analyze_redundancy", {"category": category, "limit": n, "limit_per_type": 0}
    )
    findings = parse_findings(text)[:n]
    if not findings:
        mcp.close()
        raise SystemExit(f"No findings parsed for category '{category}'.")

    items = [(i, tag, desc, fetch_source(mcp, sym)) for i, (tag, desc, sym) in enumerate(findings)]
    mcp.close()

    per_tag: dict[str, list[int]] = defaultdict(lambda: [0, 0])  # tag -> [tp, total]
    fps: list[str] = []
    judge_usage = agent.Usage()
    BATCH = 8
    for s in range(0, len(items), BATCH):
        batch = items[s:s + BATCH]
        verdicts, _ = judge_batch(client, batch)
        for (idx, tag, desc, _src), tp in zip(batch, verdicts):
            per_tag[tag][0] += int(tp)
            per_tag[tag][1] += 1
            if not tp:
                fps.append(f"[{tag}] {desc[:90]}")

    total_tp = sum(v[0] for v in per_tag.values())
    total = sum(v[1] for v in per_tag.values())
    print(f"{'check':<8}{'precision':>11}{'tp/total':>11}")
    print("-" * 30)
    for tag, (tp, tot) in sorted(per_tag.items(), key=lambda kv: kv[1][0] / kv[1][1]):
        print(f"{tag:<8}{100 * tp / tot:>10.0f}%{f'{tp}/{tot}':>11}")
    print(f"\nOVERALL precision: {100 * total_tp / total:.0f}%  ({total_tp}/{total})")
    if fps:
        print("\nFlagged as false positives:")
        for f in fps[:80]:
            print(f"  - {f}")


if __name__ == "__main__":
    try:
        main()
    except agent.QuotaExhausted:
        print("\nHit the Gemini daily quota — retry tomorrow or lower EVAL_FP_N.")
        raise SystemExit(1)
