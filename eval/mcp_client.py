"""Minimal JSON-RPC 2.0 client for the `ast_context mcp` server (stdio transport).

Spawns the binary as a subprocess and speaks the MCP protocol over stdin/stdout:
one JSON object per line. Just enough to `initialize`, `tools/list`, and
`tools/call` — which is all the eval needs to give the model the *real*
ast_context tool surface.
"""

from __future__ import annotations

import json
import subprocess
from typing import Any


class McpClient:
    def __init__(self, binary: str = "ast_context") -> None:
        self._proc = subprocess.Popen(
            [binary, "mcp"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            bufsize=1,
        )
        self._next_id = 0
        self._request("initialize", {})

    def _request(self, method: str, params: dict[str, Any]) -> dict[str, Any]:
        self._next_id += 1
        msg = {"jsonrpc": "2.0", "id": self._next_id, "method": method, "params": params}
        assert self._proc.stdin and self._proc.stdout
        self._proc.stdin.write(json.dumps(msg) + "\n")
        self._proc.stdin.flush()
        # Read until we get a line that parses as our response (skip any noise).
        while True:
            line = self._proc.stdout.readline()
            if not line:
                raise RuntimeError("MCP server closed the connection")
            line = line.strip()
            if not line:
                continue
            try:
                resp = json.loads(line)
            except json.JSONDecodeError:
                continue
            if resp.get("id") == self._next_id:
                if "error" in resp:
                    raise RuntimeError(f"MCP error: {resp['error']}")
                return resp.get("result", {})

    def list_tools(self) -> list[dict[str, Any]]:
        """Return tool definitions in Anthropic `tools` shape (name/description/input_schema)."""
        result = self._request("tools/list", {})
        tools = []
        for t in result.get("tools", []):
            tools.append(
                {
                    "name": t["name"],
                    "description": t.get("description", ""),
                    "input_schema": t.get("inputSchema", {"type": "object", "properties": {}}),
                }
            )
        return tools

    def call_tool(self, name: str, arguments: dict[str, Any]) -> str:
        """Call a tool and return its text content flattened to a string."""
        result = self._request("tools/call", {"name": name, "arguments": arguments})
        parts = []
        for block in result.get("content", []):
            if block.get("type") == "text":
                parts.append(block.get("text", ""))
        return "\n".join(parts) if parts else "(no output)"

    def close(self) -> None:
        try:
            if self._proc.stdin:
                self._proc.stdin.close()
            self._proc.terminate()
            self._proc.wait(timeout=5)
        except Exception:
            self._proc.kill()
