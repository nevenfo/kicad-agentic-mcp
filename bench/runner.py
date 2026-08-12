"""Golden-task benchmark runner.

Each task is a *scripted oracle path*: the exact call sequence a perfect agent
would make. Running it measures what the server costs when the reasoning is
free — MCP round trips, wall clock, and the tokens the results push back into
the caller's context. That is the floor every agent mode is judged against, and
it is fully deterministic, so a refactor that regresses it is caught without an
LLM in the loop.

Assertions are checked against KiCad's own output (kicad-cli ERC/DRC) or
against the server's read tools — never against the text a model produced.

Usage:
    python bench/runner.py --server <binary> --label baseline
    python bench/runner.py --server <binary> --task sch_divider --repeat 5
"""

from __future__ import annotations

import argparse
import collections
import json
import os
import shutil
import statistics
import sys
import tempfile
import time
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).parent))

import tiktoken  # noqa: E402
import yaml  # noqa: E402

from mcp_client import Call, McpStdioClient  # noqa: E402

ENC = tiktoken.get_encoding("o200k_base")
TASK_DIR = Path(__file__).parent / "tasks"


def tokens(text: str) -> int:
    return len(ENC.encode(text))


# ── assertions ───────────────────────────────────────────────────────────────


@dataclass
class AssertResult:
    kind: str
    ok: bool
    detail: str


def _text_of(call_result: Any) -> str:
    if not call_result:
        return ""
    parts = call_result.get("content") or []
    return "\n".join(p.get("text", "") for p in parts if p.get("type") == "text")


def _json_of(call_result: Any) -> Any:
    raw = _text_of(call_result)
    try:
        return json.loads(raw)
    except (json.JSONDecodeError, TypeError):
        return None


def _unwrap_invoke(call: Call, index: int) -> Call:
    """Present one entry of a `kicad_invoke` batch as if it were its own call.

    The synthetic Call is deliberately **not** appended to the session: the
    round trip that really happened is the `kicad_invoke` one, and counting both
    would inflate MCP_CALLS with calls that were never on the wire.
    """
    payload = _json_of(call.result) or {}
    entry = next((r for r in payload.get("results", []) if r.get("index") == index), None)
    if entry is None:
        text = json.dumps({"error": "no result at index", "index": index, "batch": payload})
        ok = False
    elif entry.get("ok"):
        inner = entry.get("result")
        text = inner if isinstance(inner, str) else json.dumps(inner)
        ok = True
    else:
        inner = entry.get("result", {"error": entry.get("error"), "kind": entry.get("error_kind")})
        text = inner if isinstance(inner, str) else json.dumps(inner)
        ok = False
    return Call(
        method="tools/call",
        params={"name": entry.get("tool") if entry else "?", "arguments": {}},
        request_bytes=0,
        response_bytes=0,
        duration_ms=0.0,
        result={"content": [{"type": "text", "text": text}], "isError": not ok},
        error=call.error,
    )


class GatewayClient:
    """Route every tool call through `kicad_invoke`.

    The gateway's claim is that a caller can drive the whole server through a
    catalogue that never changes. Assertions have to go through the same door as
    the steps or the measurement would quietly cheat: a `run_erc` called
    directly would be a tool the gateway never had to expose.
    """

    PASSTHROUGH = {"kicad_invoke", "kicad_describe", "find_capabilities", "list_toolboxes"}

    def __init__(self, inner: McpStdioClient):
        self.inner = inner

    @property
    def session(self):  # noqa: ANN201 - mirrors McpStdioClient
        return self.inner.session

    def tools_call(self, name: str, arguments: dict | None = None, timeout: float = 300.0) -> Call:
        if name in self.PASSTHROUGH:
            return self.inner.tools_call(name, arguments, timeout=timeout)
        call = self.inner.tools_call(
            "kicad_invoke",
            {"calls": [{"tool": name, "args": arguments or {}}]},
            timeout=timeout,
        )
        return _unwrap_invoke(call, 0)


def _call_failed(call: Call) -> bool:
    """Same test as the step-error check above (line ~357): a JSON-RPC error or
    a tool result with `isError` is a failed call, never an empty result (E4).
    """
    return bool(call.error) or bool((call.result or {}).get("isError"))


def _call_error_detail(call: Call) -> str:
    return call.error if call.error else _text_of(call.result)[:300]


def check_assertion(spec: dict, client: McpStdioClient, env: dict[str, str], step_errors: list[str]) -> AssertResult:
    kind = spec["kind"]

    if kind == "all_steps_ok":
        return AssertResult(kind, not step_errors, "; ".join(step_errors) or "all steps returned ok")

    if kind == "erc_max_errors":
        limit = int(spec["value"])
        call = client.tools_call("run_erc", {"schematic": env["SCH"], "severity": "error"})
        if _call_failed(call):
            return AssertResult(kind, False, f"run_erc call failed: {_call_error_detail(call)}")
        payload = _json_of(call.result)
        if payload is None:
            return AssertResult(kind, False, f"run_erc returned non-JSON: {_text_of(call.result)[:200]}")
        count = payload.get("error_count")
        if count is None:
            violations = payload.get("violations", [])
            count = sum(1 for v in violations if v.get("severity") == "error")
        return AssertResult(kind, count <= limit, f"erc errors={count} limit={limit}")

    if kind == "components_present":
        expected = set(spec["value"])
        call = client.tools_call("list_schematic_components", {"schematic": env["SCH"]})
        if _call_failed(call):
            return AssertResult(kind, False, f"list_schematic_components call failed: {_call_error_detail(call)}")
        payload = _json_of(call.result) or {}
        found = {c.get("reference") for c in payload.get("components", [])}
        missing = sorted(expected - found)
        return AssertResult(kind, not missing, f"missing={missing} found={sorted(found)}")

    if kind == "nets_present":
        expected = set(spec["value"])
        call = client.tools_call("list_schematic_nets", {"schematic": env["SCH"]})
        if _call_failed(call):
            return AssertResult(kind, False, f"list_schematic_nets call failed: {_call_error_detail(call)}")
        payload = _json_of(call.result) or {}
        raw_nets = payload.get("nets", [])
        found = {n if isinstance(n, str) else n.get("name") for n in raw_nets}
        missing = sorted(expected - found)
        return AssertResult(kind, not missing, f"missing={missing} found={sorted(x for x in found if x)}")

    if kind == "no_single_pin_nets":
        call = client.tools_call("find_single_pin_nets", {"schematic": env["SCH"]})
        if _call_failed(call):
            return AssertResult(kind, False, f"find_single_pin_nets call failed: {_call_error_detail(call)}")
        payload = _json_of(call.result) or {}
        offenders = payload.get("single_pin_nets", payload.get("nets", []))
        allowed = set(spec.get("allow", []))
        bad = [n for n in offenders if (n if isinstance(n, str) else n.get("net")) not in allowed]
        return AssertResult(kind, not bad, f"single-pin nets={bad}")

    if kind == "file_exists":
        target = Path(os.path.expandvars(spec["value"].replace("$WORK", env["WORK"])))
        return AssertResult(kind, target.exists(), str(target))

    return AssertResult(kind, False, f"unknown assertion kind '{kind}'")


# ── task execution ───────────────────────────────────────────────────────────


@dataclass
class TaskRun:
    task_id: str
    success: bool
    mcp_calls: int
    setup_calls: int
    wall_clock_ms: float
    request_tokens: int
    response_tokens: int
    # Tokens the client pays for `tools/list` refreshes triggered by
    # `notifications/tools/list_changed`. Separate line because it is context
    # the caller never asked for and cannot decline.
    catalog_tokens: int = 0
    catalog_refreshes: int = 0
    step_errors: list[str] = field(default_factory=list)
    assertions: list[dict] = field(default_factory=list)
    # Per-call breakdown. Aggregates hide where the tokens actually go: without
    # this, `load_toolset` echoing every loaded tool's description looks like
    # generic overhead instead of the single largest line item.
    call_breakdown: list[dict] = field(default_factory=list)
    # Only populated in `search` load mode: how well capability search did.
    retrieval: dict | None = None


def substitute(value: Any, env: dict[str, str]) -> Any:
    if isinstance(value, str):
        out = value
        for k, v in env.items():
            out = out.replace(f"${k}", v)
        return out
    if isinstance(value, list):
        return [substitute(v, env) for v in value]
    if isinstance(value, dict):
        return {k: substitute(v, env) for k, v in value.items()}
    return value


# Tools each assertion kind needs on top of whatever the steps call. Loading is
# part of the measured cost, so this has to be exact — an assertion that fails
# with `toolset_not_loaded` would look like a design regression.
ASSERT_TOOLS = {
    "erc_max_errors": ["run_erc"],
    "components_present": ["list_schematic_components"],
    "nets_present": ["list_schematic_nets"],
    "no_single_pin_nets": ["find_single_pin_nets"],
    "all_steps_ok": [],
    "file_exists": [],
}


def required_tools(task: dict) -> list[str]:
    """Exact tool names a task touches, in stable order."""
    names: list[str] = []
    for step in task["steps"]:
        if step["tool"] not in names:
            names.append(step["tool"])
    for spec in task.get("assert", []):
        for name in ASSERT_TOOLS.get(spec["kind"], []):
            if name not in names:
                names.append(name)
    return names


def run_task(
    task: dict, server: str, config: str, keep: bool, load_mode: str, search_limit: int | None = None
) -> TaskRun:
    work = Path(tempfile.mkdtemp(prefix=f"kam-bench-{task['id']}-"))
    name = task.get("project_name", task["id"])
    # `create_project` writes <path>/<name>.kicad_* directly; there is no
    # <name>/ subdirectory.
    posix_work = str(work).replace("\\", "/")
    env_vars = {
        "WORK": posix_work,
        "NAME": name,
        "SCH": f"{posix_work}/{name}.kicad_sch",
        "PCB": f"{posix_work}/{name}.kicad_pcb",
    }

    proc_env = dict(os.environ)
    proc_env.setdefault("RUST_LOG", "warn")

    step_errors: list[str] = []
    assertions: list[AssertResult] = []

    with McpStdioClient([server, "--config", config], env=proc_env) as raw_client:
        raw_client.initialize()
        # In gateway mode every domain call travels inside `kicad_invoke`;
        # `client` is what the task and its assertions see either way.
        client = GatewayClient(raw_client) if load_mode == "gateway" else raw_client

        # A real agent must discover and load its toolbelt. Count it honestly:
        # discovery calls, their responses, and the tools/list refresh they
        # trigger are all context the harness pays for.
        #
        # Both modes are given oracle knowledge of what the task needs — the
        # comparison is loading granularity, not search quality. Search quality
        # is measured separately, against the `intent` field.
        setup_before = len(client.session.calls)
        retrieval: dict | None = None
        if load_mode == "tools":
            client.tools_call("load_tools", {"names": required_tools(task)})
        elif load_mode == "gateway":
            # Same oracle as `tools` mode, so the comparison isolates *how* the
            # schemas arrive: as a result the caller asked for, or as a
            # catalogue refresh it cannot decline.
            client.tools_call("kicad_describe", {"names": required_tools(task)})
        elif load_mode == "search":
            # No oracle here. The task's plain-language `intents` are all the
            # agent gets; whatever the search returns is the whole toolbelt.
            # A tool the search misses shows up as a failed step, which is the
            # honest way to score retrieval.
            found: list[str] = []
            for intent in task.get("intents", []):
                call = client.tools_call(
                    "find_capabilities",
                    {"query": intent, "limit": search_limit or task.get("search_limit", 8)},
                )
                payload = _json_of(call.result) or {}
                for m in payload.get("matches", []):
                    if m["name"] not in found:
                        found.append(m["name"])
            if found:
                client.tools_call("load_tools", {"names": found})
            needed = required_tools(task)
            missed = [n for n in needed if n not in found]
            retrieval = {
                "queries": len(task.get("intents", [])),
                "returned": len(found),
                "needed": len(needed),
                "hits": len(needed) - len(missed),
                "missed": missed,
                "precision": round((len(needed) - len(missed)) / len(found), 3) if found else 0.0,
                "recall": round((len(needed) - len(missed)) / len(needed), 3) if needed else 1.0,
            }
        else:
            client.tools_call("list_toolboxes")
            if task.get("toolsets"):
                client.tools_call("load_toolset", {"name": task["toolsets"]})
        setup_calls = len(client.session.calls) - setup_before

        t0 = time.perf_counter()
        step_calls: list[tuple[dict, Call]] = []
        if load_mode == "gateway":
            # One round trip for the whole scripted path. `stop_on_error` is off
            # because recovery tasks deliberately contain failing steps and the
            # ones after them still have to run.
            batch = raw_client.tools_call(
                "kicad_invoke",
                {
                    "calls": [
                        {"tool": s["tool"], "args": substitute(s.get("args", {}), env_vars)}
                        for s in task["steps"]
                    ],
                    "stop_on_error": False,
                },
            )
            step_calls = [
                (step, _unwrap_invoke(batch, i)) for i, step in enumerate(task["steps"])
            ]
        else:
            step_calls = [
                (step, client.tools_call(step["tool"], substitute(step.get("args", {}), env_vars)))
                for step in task["steps"]
            ]

        for i, (step, call) in enumerate(step_calls):
            # `expect_error: true` inverts the check. Recovery tasks need to
            # assert that a bad call *fails* — a server that silently accepts
            # a nonexistent lib_id or a stale path is the failure mode we are
            # guarding against, and it would otherwise score as a pass.
            expect_error = bool(step.get("expect_error"))
            failed = bool(call.error) or bool((call.result or {}).get("isError"))
            if failed and not expect_error:
                detail = call.error if call.error else _text_of(call.result)[:300]
                step_errors.append(f"step[{i}] {step['tool']}: {detail}")
            elif expect_error and not failed:
                step_errors.append(
                    f"step[{i}] {step['tool']}: expected an error, got success — "
                    f"{_text_of(call.result)[:200]}"
                )
        wall = (time.perf_counter() - t0) * 1000.0

        for spec in task.get("assert", []):
            assertions.append(check_assertion(spec, client, env_vars, step_errors))

        calls = client.session.calls

    req_tokens = sum(tokens(json.dumps(c.params, separators=(",", ":"))) for c in calls if c.method == "tools/call")
    resp_tokens = sum(tokens(_text_of(c.result)) for c in calls if c.method == "tools/call" and c.result)
    catalog_calls = [c for c in calls if c.method == "tools/list" and c.result]
    catalog_tokens = sum(
        tokens(json.dumps(c.result.get("tools", []), separators=(",", ":"))) for c in catalog_calls
    )

    if not keep:
        shutil.rmtree(work, ignore_errors=True)

    return TaskRun(
        task_id=task["id"],
        success=not step_errors and all(a.ok for a in assertions),
        mcp_calls=sum(1 for c in calls if c.method == "tools/call"),
        setup_calls=setup_calls,
        wall_clock_ms=wall,
        request_tokens=req_tokens,
        response_tokens=resp_tokens,
        catalog_tokens=catalog_tokens,
        catalog_refreshes=len(catalog_calls),
        retrieval=retrieval,
        step_errors=step_errors,
        assertions=[asdict(a) for a in assertions],
        call_breakdown=[
            {
                "tool": c.params.get("name"),
                "req_tokens": tokens(json.dumps(c.params, separators=(",", ":"))),
                "resp_tokens": tokens(_text_of(c.result)),
                "ms": round(c.duration_ms, 1),
            }
            for c in calls
            if c.method == "tools/call"
        ],
    )


# ── main ─────────────────────────────────────────────────────────────────────


def load_tasks(only: str | None) -> list[dict]:
    tasks = []
    for path in sorted(TASK_DIR.glob("*.yaml")):
        task = yaml.safe_load(path.read_text(encoding="utf-8"))
        if only and task["id"] != only:
            continue
        tasks.append(task)
    return tasks


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--server", required=True)
    ap.add_argument("--config", default=str(Path(__file__).parent / "konnect.bench.toml"))
    ap.add_argument("--label", default="unlabeled")
    ap.add_argument("--task", default=None)
    ap.add_argument("--repeat", type=int, default=1)
    ap.add_argument(
        "--load-mode",
        choices=["toolsets", "tools", "search", "gateway"],
        default="toolsets",
        help="toolsets: list_toolboxes + load_toolset (baseline). "
        "tools: load_tools with the exact names the task needs (oracle). "
        "search: find_capabilities on the task's plain-language intents, no oracle. "
        "gateway: kicad_describe + a single batched kicad_invoke, catalogue never changes.",
    )
    ap.add_argument("--search-limit", type=int, default=None, help="override per-query result count")
    ap.add_argument("--keep", action="store_true", help="keep the generated projects on disk")
    ap.add_argument("--out", default=None)
    args = ap.parse_args()

    tasks = load_tasks(args.task)
    if not tasks:
        raise SystemExit(f"no tasks matched (dir={TASK_DIR})")

    runs: list[TaskRun] = []
    for task in tasks:
        for _ in range(args.repeat):
            runs.append(
                run_task(
                    task, args.server, args.config, args.keep, args.load_mode, args.search_limit
                )
            )

    by_task: dict[str, list[TaskRun]] = {}
    for r in runs:
        by_task.setdefault(r.task_id, []).append(r)

    print(f"label: {args.label}   tasks: {len(by_task)}   runs: {len(runs)}   load-mode: {args.load_mode}\n")
    header = (
        f"{'task':<24} {'ok':>6} {'calls':>6} {'p50 ms':>8} "
        f"{'req tk':>7} {'resp tk':>8} {'cat tk':>8} {'ext tk':>8}"
    )
    print(header)
    print("-" * len(header))
    for task_id, rs in by_task.items():
        ok = sum(1 for r in rs if r.success)
        p50 = statistics.median(r.wall_clock_ms for r in rs)
        r0 = rs[0]
        print(
            f"{task_id:<24} {ok}/{len(rs):>4} {r0.mcp_calls:>6} {p50:>8.0f} "
            f"{r0.request_tokens:>7} {r0.response_tokens:>8} {r0.catalog_tokens:>8} "
            f"{r0.response_tokens + r0.catalog_tokens:>8}"
        )

    total_ok = sum(1 for r in runs if r.success)
    ext = [r.response_tokens + r.catalog_tokens for r in runs]
    print(f"\nSUCCESS_RATE             {total_ok}/{len(runs)} = {total_ok / len(runs):.1%}")
    print(f"MCP_CALLS median/task    {statistics.median(r.mcp_calls for r in runs):.0f}")
    print(f"WALL_CLOCK_P50 (ms)      {statistics.median(r.wall_clock_ms for r in runs):.0f}")
    print(f"WALL_CLOCK_P95 (ms)      {sorted(r.wall_clock_ms for r in runs)[int(0.95 * (len(runs) - 1))]:.0f}")
    print(f"RESPONSE_TOKENS/task     {statistics.median(r.response_tokens for r in runs):.0f}")
    print(f"CATALOG_TOKENS/task      {statistics.median(r.catalog_tokens for r in runs):.0f}")
    print(f"EXTERNAL_TOKENS/task     {statistics.median(ext):.0f}   <- what the harness actually eats")

    retrievals = [r.retrieval for r in runs if r.retrieval]
    if retrievals:
        recall = statistics.mean(r["recall"] for r in retrievals)
        precision = statistics.mean(r["precision"] for r in retrievals)
        missed: collections.Counter[str] = collections.Counter()
        for r in retrievals:
            missed.update(r["missed"])
        print(f"\nRETRIEVAL_RECALL         {recall:.1%}   (needed tools the search surfaced)")
        print(f"RETRIEVAL_PRECISION      {precision:.1%}   (loaded tools that were needed)")
        if missed:
            print("misses (tool: times):    " + ", ".join(f"{t}:{n}" for t, n in missed.most_common(10)))

    for r in runs:
        if r.success:
            continue
        print(f"\nFAIL {r.task_id}")
        for e in r.step_errors:
            print(f"  step: {e}")
        for a in r.assertions:
            if not a["ok"]:
                print(f"  assert {a['kind']}: {a['detail']}")

    if args.out:
        Path(args.out).parent.mkdir(parents=True, exist_ok=True)
        Path(args.out).write_text(
            json.dumps({"label": args.label, "runs": [asdict(r) for r in runs]}, indent=2),
            encoding="utf-8",
        )
        print(f"\nwrote {args.out}")


if __name__ == "__main__":
    main()
