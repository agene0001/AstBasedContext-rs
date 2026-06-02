"""Shared agentic loop + the two tool surfaces being compared (Gemini backend).

Arm A ("files"): plain `read_file` / `grep` / `list_dir` over the repo — the
baseline an agent uses today.

Arm B ("ast_context"): the real MCP tool surface, proxied to a running
`ast_context mcp` server.

Both arms run the same manual function-calling loop and the same token
accounting, so the only variable is the tool surface.
"""

from __future__ import annotations

import os
import subprocess
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Callable

import httpx
from google import genai
from google.genai import errors, types

# Default to a model that exists on the free tier. Override with GEMINI_MODEL.
# NOTE: ids like "gemini-3.5-pro"/"gemini-3.5-flash" do not exist and 404 —
# run_eval does a preflight that lists the models your key can actually use.
MODEL = os.environ.get("GEMINI_MODEL", "gemini-3.1-pro-preview")


@dataclass
class Usage:
    prompt_tokens: int = 0
    output_tokens: int = 0
    cached_tokens: int = 0
    thinking_tokens: int = 0
    turns: int = 0

    def add(self, meta: Any) -> None:
        if meta is None:
            return
        self.prompt_tokens += getattr(meta, "prompt_token_count", 0) or 0
        self.output_tokens += getattr(meta, "candidates_token_count", 0) or 0
        self.cached_tokens += getattr(meta, "cached_content_token_count", 0) or 0
        self.thinking_tokens += getattr(meta, "thoughts_token_count", 0) or 0

    @property
    def total(self) -> int:
        # All tokens this approach consumed to answer — the metric we compare.
        return self.prompt_tokens + self.output_tokens + self.thinking_tokens


# ── Arm A: file-reading tools (JSON-schema, provider-agnostic) ─────────────

FILE_TOOLS = [
    {
        "name": "read_file",
        "description": "Read a source file from the repository (path relative to repo root).",
        "input_schema": {
            "type": "object",
            "properties": {"path": {"type": "string"}},
            "required": ["path"],
        },
    },
    {
        "name": "grep",
        "description": "Search file contents with a regex (ripgrep). Optional glob to filter files.",
        "input_schema": {
            "type": "object",
            "properties": {
                "pattern": {"type": "string"},
                "glob": {"type": "string", "description": "e.g. '*.rs'"},
            },
            "required": ["pattern"],
        },
    },
    {
        "name": "list_dir",
        "description": "List entries in a directory (path relative to repo root).",
        "input_schema": {
            "type": "object",
            "properties": {"path": {"type": "string"}},
            "required": ["path"],
        },
    },
]


class FileToolExecutor:
    """Executes the file-reading tools, sandboxed to `root`."""

    def __init__(self, root: Path) -> None:
        self.root = root.resolve()

    def _safe(self, rel: str) -> Path | None:
        p = (self.root / rel).resolve()
        return p if str(p).startswith(str(self.root)) else None

    def __call__(self, name: str, args: dict[str, Any]) -> str:
        if name == "read_file":
            p = self._safe(args.get("path", ""))
            if not p or not p.is_file():
                return f"error: no such file: {args.get('path')}"
            return p.read_text(errors="replace")[:60_000]
        if name == "list_dir":
            p = self._safe(args.get("path", "."))
            if not p or not p.is_dir():
                return f"error: no such directory: {args.get('path')}"
            return "\n".join(sorted(e.name for e in p.iterdir()))
        if name == "grep":
            cmd = ["rg", "-n", "--no-heading", args["pattern"]]
            if args.get("glob"):
                cmd += ["--glob", args["glob"]]
            out = subprocess.run(cmd, cwd=self.root, capture_output=True, text=True)
            return (out.stdout or "(no matches)")[:30_000]
        return f"error: unknown tool {name}"


# ── JSON-schema → Gemini Schema conversion ─────────────────────────────────

_TYPE_MAP = {
    "string": "STRING",
    "integer": "INTEGER",
    "number": "NUMBER",
    "boolean": "BOOLEAN",
    "array": "ARRAY",
    "object": "OBJECT",
}


def _to_schema(js: dict[str, Any]) -> types.Schema:
    """Convert a (subset of) JSON Schema into a Gemini types.Schema.

    Only the fields Gemini's function-calling supports are carried over;
    anything else (e.g. `additionalProperties`, `$schema`) is dropped.
    """
    kwargs: dict[str, Any] = {"type": _TYPE_MAP.get(js.get("type", "string"), "STRING")}
    if "description" in js:
        kwargs["description"] = js["description"]
    if "enum" in js:
        kwargs["enum"] = [str(e) for e in js["enum"]]
    if js.get("type") == "object":
        props = js.get("properties", {})
        kwargs["properties"] = {k: _to_schema(v) for k, v in props.items()}
        if js.get("required"):
            kwargs["required"] = list(js["required"])
    if js.get("type") == "array" and "items" in js:
        kwargs["items"] = _to_schema(js["items"])
    return types.Schema(**kwargs)


def _to_tool(tools: list[dict[str, Any]]) -> types.Tool:
    decls = []
    for t in tools:
        schema = t.get("input_schema") or {"type": "object", "properties": {}}
        params = _to_schema(schema) if schema.get("properties") else None
        decls.append(
            types.FunctionDeclaration(
                name=t["name"], description=t.get("description", ""), parameters=params
            )
        )
    return types.Tool(function_declarations=decls)


# ── The agentic loop (shared) ──────────────────────────────────────────────

SYSTEM = (
    "You are a code-comprehension agent answering a question about a Rust "
    "codebase. Use the available tools to gather just enough evidence, then give "
    "a short, direct final answer. Be economical: don't read more than you need. "
    "When you have the answer, state it plainly in your final message."
)


class QuotaExhausted(RuntimeError):
    """A per-day quota cap was hit — retrying won't help until it resets."""


def _generate_with_retry(client: genai.Client, contents, config, *, attempts: int = 5):
    """Call generate_content, retrying *transient* failures with backoff.

    Retries 503 (overload), per-minute 429s, and transient network errors
    (read/connect timeouts, dropped connections). Does NOT retry a per-day quota
    429 — that resets at ~midnight Pacific, so retrying only burns more of the
    already-spent allotment; it raises `QuotaExhausted` for a clean exit.
    """
    delay = 4.0
    for i in range(attempts):
        try:
            return client.models.generate_content(model=MODEL, contents=contents, config=config)
        except errors.ServerError:  # 503 UNAVAILABLE — model overloaded ("high demand")
            if i == attempts - 1:
                raise
            print(f"  [retry {i + 1}/{attempts - 1}] {MODEL} overloaded; waiting {delay:.0f}s...")
            time.sleep(delay)
            delay *= 2
        except httpx.TransportError as e:  # read/connect timeout, dropped conn — don't kill the run
            if i == attempts - 1:
                raise
            print(f"  [retry {i + 1}/{attempts - 1}] network error ({type(e).__name__}); waiting {delay:.0f}s...")
            time.sleep(delay)
            delay *= 2
        except errors.ClientError as e:  # 429 rate limit; 404 (bad model) etc. are not retryable
            if getattr(e, "code", None) != 429:
                raise
            if "PerDay" in str(e):  # daily free-tier cap — retrying is futile
                raise QuotaExhausted(str(e)) from e
            if i == attempts - 1:
                raise
            print(f"  [retry {i + 1}/{attempts - 1}] rate limited; waiting {delay:.0f}s...")
            time.sleep(delay)
            delay *= 2


def available_models(client: genai.Client) -> list[str]:
    """Model ids on this key that support generateContent (used by the preflight)."""
    out = []
    for m in client.models.list():
        if "generateContent" in (getattr(m, "supported_actions", None) or []):
            out.append(m.name.removeprefix("models/"))
    return sorted(out)


def _response_text(resp) -> str:
    cand = resp.candidates[0] if resp.candidates else None
    parts = cand.content.parts if cand and cand.content else []
    return "".join(p.text for p in parts if getattr(p, "text", None)).strip()


def judge_answer(client: genai.Client, question: str, answer: str, rubric: str) -> tuple[bool, str, "Usage"]:
    """LLM-as-judge for open-ended tasks where substring matching can't grade a
    different-but-valid answer. Returns (passed, verdict_text, judge_usage).

    Judge token cost is reported separately — it's grading overhead, not part of
    either arm's budget.
    """
    prompt = (
        "Grade an answer to a question about a Rust codebase.\n\n"
        f"QUESTION:\n{question}\n\n"
        f"PASS CRITERIA:\n{rubric}\n\n"
        f"ANSWER:\n{answer}\n\n"
        "Reply with ONE line beginning with PASS or FAIL, then a brief reason."
    )
    config = types.GenerateContentConfig(
        system_instruction="You are a strict but fair grader. Judge only against the PASS criteria.",
        temperature=0.0,
    )
    contents = [types.Content(role="user", parts=[types.Part.from_text(text=prompt)])]
    resp = _generate_with_retry(client, contents, config)
    usage = Usage()
    usage.add(resp.usage_metadata)
    usage.turns += 1
    verdict = _response_text(resp)
    return verdict.upper().startswith("PASS"), verdict, usage


@dataclass
class Result:
    answer: str
    usage: Usage = field(default_factory=Usage)


def run_agent(
    client: genai.Client,
    tools: list[dict[str, Any]],
    execute: Callable[[str, dict[str, Any]], str],
    question: str,
    *,
    extra_system: str = "",
    max_turns: int = 12,
) -> Result:
    """Run one task to completion, accumulating token usage across all turns."""
    config = types.GenerateContentConfig(
        system_instruction=SYSTEM + extra_system,
        tools=[_to_tool(tools)],
        temperature=0.0,
        automatic_function_calling=types.AutomaticFunctionCallingConfig(disable=True),
    )
    contents: list[types.Content] = [
        types.Content(role="user", parts=[types.Part.from_text(text=question)])
    ]
    usage = Usage()

    for _ in range(max_turns):
        resp = _generate_with_retry(client, contents, config)
        usage.add(resp.usage_metadata)
        usage.turns += 1

        candidate = resp.candidates[0] if resp.candidates else None
        parts = candidate.content.parts if candidate and candidate.content else []
        calls = [p.function_call for p in parts if getattr(p, "function_call", None)]

        if calls:
            contents.append(candidate.content)  # model turn (the function calls)
            fr_parts = []
            for fc in calls:
                args = dict(fc.args or {})
                if os.environ.get("EVAL_VERBOSE"):
                    preview = ", ".join(f"{k}={str(v)[:50]}" for k, v in args.items())
                    print(f"    · {fc.name}({preview})")
                try:
                    out = execute(fc.name, args)
                except Exception as e:  # never let a tool error kill the run
                    out = f"error executing {fc.name}: {e}"
                fr_parts.append(
                    types.Part.from_function_response(
                        name=fc.name, response={"result": out[:60_000]}
                    )
                )
            contents.append(types.Content(role="user", parts=fr_parts))
            continue

        text = "".join(p.text for p in parts if getattr(p, "text", None))
        return Result(answer=text.strip(), usage=usage)

    return Result(answer="(gave up: max turns reached)", usage=usage)


def make_client() -> genai.Client:
    if not (os.environ.get("GEMINI_API_KEY") or os.environ.get("GOOGLE_API_KEY")):
        raise SystemExit("Set GEMINI_API_KEY (from https://aistudio.google.com/apikey) to run the eval.")
    # Generous timeout (ms): the ast arm accumulates large contexts, and a slow
    # pro-model turn shouldn't trip the default read timeout. Transient timeouts
    # are still retried in _generate_with_retry as a backstop.
    return genai.Client(http_options=types.HttpOptions(timeout=120_000))
