"""Minimal MCP stdio client used by the benchmark harness.

No third-party MCP SDK: the benchmark must measure the exact bytes the server
puts on the wire, so we speak JSON-RPC 2.0 over stdio directly and keep the raw
payloads for token accounting.
"""

from __future__ import annotations

import json
import subprocess
import threading
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any


PROTOCOL_VERSION = "2025-06-18"


@dataclass
class Call:
    """One request/response round trip, with the raw wire bytes preserved."""

    method: str
    params: dict[str, Any]
    request_bytes: int
    response_bytes: int
    duration_ms: float
    result: Any = None
    error: Any = None


@dataclass
class Session:
    calls: list[Call] = field(default_factory=list)

    @property
    def mcp_calls(self) -> int:
        return sum(1 for c in self.calls if c.method == "tools/call")

    @property
    def total_response_bytes(self) -> int:
        return sum(c.response_bytes for c in self.calls)


def _resolve_program(argv: list[str]) -> list[str]:
    """Make a relative server path absolute before `CreateProcess` sees it.

    Windows resolves an executable named with forward slashes
    (`target/release/konnect.exe`) against neither the cwd nor `PATH`, so the
    documented default in `model_fit.py`/`agent_e2e.py` died with
    `FileNotFoundError: [WinError 2]` before any measurement ran.
    `harness_runner.py` already resolved its own `--server` for this reason;
    doing it here covers every caller instead of one. A name that is not an
    existing file is left untouched, so a bare command still goes to `PATH`.
    """
    if not argv:
        return argv
    program = Path(argv[0])
    if program.is_file():
        return [str(program.resolve()), *argv[1:]]
    return argv


class McpStdioClient:
    """Speaks MCP over a child process' stdin/stdout."""

    def __init__(
        self,
        argv: list[str],
        cwd: str | None = None,
        env: dict[str, str] | None = None,
        auto_refresh_tools: bool = True,
    ):
        self.argv = _resolve_program(argv)
        self.cwd = cwd
        self.env = env
        # Real MCP clients re-fetch `tools/list` when the server sends
        # `notifications/tools/list_changed`. Ignoring that would undercount the
        # external context cost of `load_toolset` by an entire tool catalogue,
        # which is precisely the number this benchmark exists to measure.
        self.auto_refresh_tools = auto_refresh_tools
        self.proc: subprocess.Popen[bytes] | None = None
        self.session = Session()
        self._next_id = 0
        self._stderr: list[str] = []
        self._pending_list_changed = False
        self._in_refresh = False

    # ── lifecycle ────────────────────────────────────────────────────────

    def __enter__(self) -> "McpStdioClient":
        self.proc = subprocess.Popen(
            self.argv,
            cwd=self.cwd,
            env=self.env,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            bufsize=0,
        )
        threading.Thread(target=self._drain_stderr, daemon=True).start()
        return self

    def __exit__(self, *_exc: object) -> None:
        self.close()

    def close(self) -> None:
        if self.proc is None:
            return
        try:
            if self.proc.stdin:
                self.proc.stdin.close()
            self.proc.wait(timeout=5)
        except Exception:
            self.proc.kill()
        self.proc = None

    def _drain_stderr(self) -> None:
        assert self.proc is not None and self.proc.stderr is not None
        for raw in self.proc.stderr:
            self._stderr.append(raw.decode("utf-8", "replace").rstrip())

    @property
    def stderr(self) -> list[str]:
        return list(self._stderr)

    # ── JSON-RPC ─────────────────────────────────────────────────────────

    def _send(self, payload: dict[str, Any]) -> int:
        assert self.proc is not None and self.proc.stdin is not None
        line = json.dumps(payload, separators=(",", ":")).encode() + b"\n"
        self.proc.stdin.write(line)
        self.proc.stdin.flush()
        return len(line)

    def _read_line(self, timeout: float) -> bytes:
        assert self.proc is not None and self.proc.stdout is not None
        deadline = time.monotonic() + timeout
        box: list[bytes] = []

        def reader() -> None:
            assert self.proc is not None and self.proc.stdout is not None
            box.append(self.proc.stdout.readline())

        t = threading.Thread(target=reader, daemon=True)
        t.start()
        t.join(max(0.0, deadline - time.monotonic()))
        if not box:
            raise TimeoutError(f"no response within {timeout}s; stderr tail={self._stderr[-5:]}")
        return box[0]

    def request(self, method: str, params: dict[str, Any] | None = None, timeout: float = 120.0) -> Call:
        params = params or {}
        self._next_id += 1
        req = {"jsonrpc": "2.0", "id": self._next_id, "method": method, "params": params}

        t0 = time.perf_counter()
        req_bytes = self._send(req)
        # Server-initiated notifications carry no "id". They are not the reply
        # we are waiting for, but `tools/list_changed` is not noise either — it
        # obliges a real client to re-fetch the catalogue.
        while True:
            raw = self._read_line(timeout)
            if not raw:
                raise RuntimeError(f"server closed stdout; stderr tail={self._stderr[-10:]}")
            msg = json.loads(raw)
            if msg.get("id") == self._next_id:
                break
            if msg.get("method") == "notifications/tools/list_changed":
                self._pending_list_changed = True
        dt = (time.perf_counter() - t0) * 1000.0

        call = Call(
            method=method,
            params=params,
            request_bytes=req_bytes,
            response_bytes=len(raw),
            duration_ms=dt,
            result=msg.get("result"),
            error=msg.get("error"),
        )
        self.session.calls.append(call)
        return call

    def notify(self, method: str, params: dict[str, Any] | None = None) -> None:
        self._send({"jsonrpc": "2.0", "method": method, "params": params or {}})

    # ── MCP conveniences ─────────────────────────────────────────────────

    def initialize(self, client_name: str = "kam-bench") -> Call:
        call = self.request(
            "initialize",
            {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {"name": client_name, "version": "0.1.0"},
            },
        )
        self.notify("notifications/initialized")
        return call

    def tools_list(self) -> Call:
        return self.request("tools/list")

    def tools_call(self, name: str, arguments: dict[str, Any] | None = None, timeout: float = 300.0) -> Call:
        call = self.request("tools/call", {"name": name, "arguments": arguments or {}}, timeout=timeout)
        if self.auto_refresh_tools and self._pending_list_changed and not self._in_refresh:
            self._pending_list_changed = False
            self._in_refresh = True
            try:
                self.tools_list()
            finally:
                self._in_refresh = False
        return call
