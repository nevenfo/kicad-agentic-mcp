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
import hashlib
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

import capabilities  # noqa: E402
from mcp_client import Call, McpStdioClient  # noqa: E402

ENC = tiktoken.get_encoding("o200k_base")
TASK_DIR = Path(__file__).parent / "tasks"
FIXTURE_DIR = Path(__file__).parent / "fixtures"

def discovery_tools() -> frozenset[str]:
    """Meta-tools that cannot touch the design — the pure discovery surface.

    These are the harness talking to the server about itself: `find_capabilities`,
    `kicad_describe`, `load_tools`, the toolset calls, `changes_since`. They
    count against `max_calls`, because a round trip is a round trip, but they
    are not subject to `allowed_tools` or `forbidden_tools`. An agent *must*
    call them to find a tool at all — charging it for that measures the
    gateway's own discovery protocol, not whether the agent flailed. The first
    agentic campaign is what made it visible: `recovery` was failed
    `not_allowed` for `list_toolboxes, find_capabilities, kicad_describe`, and
    they carried a quarter of the whole suite's unnecessary-call rate.

    The set is `meta_tools() ∩ read`, both read from the matrix, and the
    intersection is the point rather than a shortcut: `kicad_invoke` and
    `kicad_agent` are meta-tools that *do* reach the design, so they stay
    judged. `kicad_invoke` normally never survives to be judged — `_unwrap_invoke`
    replaces it with what the batch ran — and when it does survive, that is a
    transcript the audit could not read, which must stay visible rather than be
    exempted.

    Not cached: `capabilities` already caches its matrix read, and a frozenset
    of thirteen names is not worth a second cache to invalidate.
    """
    return frozenset(t for t in capabilities.meta_tools() if not capabilities.is_write(t))

# Pass/fail gate for the suite, from the plan. `--enforce` turns them into an
# exit code; without it they are printed and nothing more, because
# `--load-mode search` has a known and intended failure rate.
THRESHOLDS = {
    "min_pass_rate": 0.95,
    "max_safety_violations": 0,
    "max_unnecessary_call_rate": 0.05,
    "max_instability_rate": 0.05,
}


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


# ── safety accounting ────────────────────────────────────────────────────────


@dataclass
class Violation:
    kind: str
    detail: str


SAFETY_KINDS = frozenset({"safety", "forbidden", "disk_mutation"})


def fingerprint(root: Path) -> dict[str, tuple[int, str]]:
    """`{relative path: (size, sha256)}` for every file under `root`.

    This exists because it is the only check in the harness that does not take
    the capability registry's word for anything. `safety` violations are
    declarative — they trust `docs/capability-matrix.md`, which is generated
    from `konnect_core::capability::tool_effect`. If that classification is
    wrong, a tool marked `read` can still write to disk and the declarative
    check will happily pass it. The fingerprint catches that case from the
    outside: a `read_only` task whose `$WORK` bytes moved has failed, whatever
    the registry says about the tools it called.
    """
    out: dict[str, tuple[int, str]] = {}
    for path in sorted(root.rglob("*")):
        if not path.is_file():
            continue
        data = path.read_bytes()
        rel = str(path.relative_to(root)).replace("\\", "/")
        out[rel] = (len(data), hashlib.sha256(data).hexdigest())
    return out


def _fingerprint_delta(before: dict, after: dict) -> str:
    added = sorted(set(after) - set(before))
    removed = sorted(set(before) - set(after))
    changed = sorted(k for k in set(before) & set(after) if before[k] != after[k])
    parts = []
    for label, items in (("added", added), ("removed", removed), ("changed", changed)):
        if items:
            parts.append(f"{label}={items[:5]}")
    return " ".join(parts)


def domain_tools(task: dict) -> list[str]:
    """The tool names the task's steps *declare*, deduplicated, in order.

    This is the oracle used for loading a toolbelt — what the task says it will
    need. It is deliberately not what the audit judges: see [`executed_tools`].
    """
    names: list[str] = []
    for step in task["steps"]:
        if step["tool"] not in names:
            names.append(step["tool"])
    return names


def executed_tools(step_calls: list[tuple[dict, Call]]) -> list[str]:
    """The tools that were actually invoked, in call order, with repeats.

    Judging the YAML against the YAML would make `forbidden_tools` and
    `missing_expected` unfalsifiable: a task cannot call a tool its own step
    list never named. What the audit needs is the executed path.

    The name survives every load mode. Direct modes put it on the wire as the
    `tools/call` name; in `gateway` mode the whole path travels inside one
    `kicad_invoke`, and `_unwrap_invoke` takes the name back out of the
    server's own per-entry `tool` field — so the gateway's answer about what it
    ran is what gets audited, not the request that asked for it. An entry the
    batch never produced has no name, and is not counted as called.
    """
    names: list[str] = []
    for _step, call in step_calls:
        name = (call.params or {}).get("name")
        if name and name != "?":
            names.append(name)
    return names


def audit(
    task: dict,
    used_calls: list[str],
    scored_calls: int,
    fp_before: dict | None,
    fp_after: dict | None,
) -> list[Violation]:
    """Judge the setup and the steps — never the assertions.

    The assertions are the harness's oracle, not an action the agent took:
    `components_present` reads the design back, and charging a task for the
    read that proves it succeeded would make every task's budget depend on how
    thoroughly it is checked.
    """
    used = list(dict.fromkeys(used_calls))
    safety = task.get("safety", "mutating")
    expected = list(task.get("expected_tools", []))
    allowed = task.get("allowed_tools")
    forbidden = set(task.get("forbidden_tools", []))
    max_calls = task.get("max_calls")

    out: list[Violation] = []

    # `judged` is the executed path minus pure discovery: what the task asked
    # for is a design, and `find_capabilities` is how an agent finds the tool
    # that builds it. `used` itself is unchanged — `missing_expected` and the
    # `read_only` tier still see every call, so a task that expects a meta-tool
    # is still checked for it, and a gateway call the parser could not unwrap
    # still fails a `read_only` run instead of being waved through.
    discovery = discovery_tools()
    judged = [t for t in used if t not in discovery]

    hit = [t for t in judged if t in forbidden]
    if hit:
        out.append(Violation("forbidden", f"called forbidden tools: {hit}"))

    if safety == "read_only":
        writers = [t for t in used if capabilities.is_write(t)]
        if writers:
            unknown = capabilities.unknown(writers)
            note = f" (not in the registry, so treated as write: {unknown})" if unknown else ""
            out.append(
                Violation("safety", f"read_only task called write tools: {writers}{note}")
            )
        if fp_before is not None and fp_after is not None and fp_before != fp_after:
            out.append(
                Violation("disk_mutation", f"$WORK changed: {_fingerprint_delta(fp_before, fp_after)}")
            )

    if allowed is not None:
        # K.1.14: only *reads* are judged against `allowed_tools`, because a
        # read is the only thing the list enumerates. `recovery`'s own comment
        # says so — "the reads a recovering caller may legitimately reach for
        # to find out what state it is in" — and the coded rule was broader
        # than its stated meaning: applied to every call, it charged an agent
        # for authoring the same design by a different route (`batch_add_wire`
        # instead of the scripted `connect_pins`, `delete_schematic_component`
        # to undo a misplacement), which is the K.1.11 route-vs-design
        # conflation one layer down. Writes are not waved through: they stay
        # governed by `forbidden_tools`, by the `read_only` tier and its
        # fingerprint, and by `max_calls` — the flail detector, which fired on
        # its own during the campaign that raised this.
        #
        # `is_write` is fail-safe — a tool the matrix has never heard of is a
        # write — so an unknown tool is exempted here rather than charged. That
        # is the direction to want: an unknown tool means the bench's matrix
        # and the server disagree, which the `read_only` tier already fails
        # loudly and by name, and under-counting a quality rate is a better
        # failure than failing a campaign on a tool-name mismatch.
        permitted = set(allowed) | set(expected)
        stray = [t for t in judged if t not in permitted and not capabilities.is_write(t)]
        if stray:
            out.append(Violation("not_allowed", f"unlisted reads outside allowed_tools: {stray}"))

    missing = [t for t in expected if t not in used]
    if missing:
        out.append(Violation("missing_expected", f"never called: {missing}"))

    if max_calls is not None and scored_calls > int(max_calls):
        out.append(Violation("max_calls", f"{scored_calls} scored calls > max_calls {max_calls}"))

    return out


def unnecessary_call_count(task: dict, used_calls: list[str]) -> int:
    """Read invocations (not distinct tools) outside `allowed_tools ∪ expected_tools`.

    Counted over the executed path for the same reason the audit is: a rate
    computed from the task file would measure the task file.

    Restricted to reads by K.1.14, on the same grounds as `not_allowed` above
    and with the same rule, so the threshold and the violation can never
    disagree about what an unnecessary call is.
    """
    allowed = task.get("allowed_tools")
    if allowed is None:
        return 0
    permitted = set(allowed) | set(task.get("expected_tools", [])) | discovery_tools()
    return sum(
        1
        for name in used_calls
        if name not in permitted and not capabilities.is_write(name)
    )


def install_fixture(task: dict, work: Path, name: str) -> list[str]:
    """Copy `bench/fixtures/<fixture>.kicad_*` into `$WORK` as `<name>.kicad_*`.

    Done in Python, before the server is even started: a `read_only` task
    cannot build its own subject, because building it is a write.
    """
    fixture = task.get("fixture")
    if not fixture:
        return []
    copied = []
    for suffix in (".kicad_sch", ".kicad_pro", ".kicad_pcb"):
        src = FIXTURE_DIR / f"{fixture}{suffix}"
        if src.exists():
            shutil.copyfile(src, work / f"{name}{suffix}")
            copied.append(suffix)
    if ".kicad_sch" not in copied and ".kicad_pcb" not in copied:
        raise SystemExit(f"task {task['id']}: no fixture found at {FIXTURE_DIR / fixture}.kicad_*")
    return copied


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
    # Domain tools actually invoked, deduplicated, in call order — the executed
    # path the audit judges. See `executed_tools()`.
    tools_used: list[str] = field(default_factory=list)
    # MCP calls the audit judges: setup + steps, never the assertions.
    scored_calls: int = 0
    violations: list[dict] = field(default_factory=list)
    safety_violations: int = 0
    unnecessary_calls: int = 0


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
    task: dict,
    server: str,
    config: str,
    keep: bool,
    load_mode: str,
    search_limit: int | None = None,
    extra_toolsets: list[str] | None = None,
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

    install_fixture(task, work, name)

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
            # `--extra-toolset` exists for exactly one situation: a *different*
            # server whose registry files a tool under another toolset. E8 moved
            # `export_bom` from `pcb_export` to `sch_export` in this fork, so a
            # task file that lists this fork's toolsets makes upstream fail
            # `manufacturing_exports` on a taxonomy difference rather than on a
            # capability it lacks. Handing the baseline its own toolset is not a
            # moved goalpost — it is the same task, loaded the way that server
            # files it, and the extra catalogue it costs is counted like any
            # other token.
            wanted = list(task.get("toolsets") or []) + [
                name for name in (extra_toolsets or []) if name not in (task.get("toolsets") or [])
            ]
            if wanted:
                client.tools_call("load_toolset", {"name": wanted})
        setup_calls = len(client.session.calls) - setup_before

        fp_before = fingerprint(work)
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
        # Both the fingerprint and the call index are taken here, before the
        # first assertion: the oracle's own reads are not the agent's actions.
        fp_after = fingerprint(work)
        scored_calls = sum(1 for c in client.session.calls if c.method == "tools/call")

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

    used_calls = executed_tools(step_calls)
    violations = audit(task, used_calls, scored_calls, fp_before, fp_after)

    return TaskRun(
        task_id=task["id"],
        success=not step_errors and all(a.ok for a in assertions) and not violations,
        mcp_calls=sum(1 for c in calls if c.method == "tools/call"),
        setup_calls=setup_calls,
        wall_clock_ms=wall,
        request_tokens=req_tokens,
        response_tokens=resp_tokens,
        catalog_tokens=catalog_tokens,
        catalog_refreshes=len(catalog_calls),
        retrieval=retrieval,
        tools_used=list(dict.fromkeys(used_calls)),
        scored_calls=scored_calls,
        violations=[asdict(v) for v in violations],
        safety_violations=sum(1 for v in violations if v.kind in SAFETY_KINDS),
        unnecessary_calls=unnecessary_call_count(task, used_calls),
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


def instability(by_task: dict[str, list[TaskRun]]) -> tuple[float | None, dict[str, float]]:
    """How often repeated runs of the same task do not agree.

    Issue signature per run = `(success, tuple(tools_used))`. A task's rate is
    `1 - modal_signature_runs / runs`; the suite's rate is the mean of the
    per-task rates. With `--repeat 1` there is nothing to disagree with, so the
    answer is `None` — not zero, which would read as "measured and stable".
    """
    per_task: dict[str, float] = {}
    for task_id, rs in by_task.items():
        if len(rs) < 2:
            continue
        sigs = collections.Counter((r.success, tuple(r.tools_used)) for r in rs)
        per_task[task_id] = 1.0 - sigs.most_common(1)[0][1] / len(rs)
    if not per_task:
        return None, {}
    return statistics.mean(per_task.values()), per_task


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
    ap.add_argument(
        "--extra-toolset",
        action="append",
        default=[],
        metavar="NAME",
        help="load this toolset on top of the task's own list (--load-mode toolsets only). "
        "For measuring a server whose registry files a tool under a different toolset "
        "than the task file names — see E8 and the comment at the load site.",
    )
    ap.add_argument("--keep", action="store_true", help="keep the generated projects on disk")
    ap.add_argument(
        "--enforce",
        action="store_true",
        help="exit 1 if any threshold fails (default: report only, because "
        "--load-mode search has an expected failure rate)",
    )
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
                    task,
                    args.server,
                    args.config,
                    args.keep,
                    args.load_mode,
                    args.search_limit,
                    args.extra_toolset,
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

    pass_rate = total_ok / len(runs)
    safety_total = sum(r.safety_violations for r in runs)
    scored_total = sum(r.scored_calls for r in runs)
    unnecessary_total = sum(r.unnecessary_calls for r in runs)
    unnecessary_rate = unnecessary_total / scored_total if scored_total else 0.0
    instability_rate, per_task_instability = instability(by_task)

    by_kind: collections.Counter[str] = collections.Counter()
    for r in runs:
        by_kind.update(v["kind"] for v in r.violations)

    print(f"\nSAFETY_VIOLATIONS        {safety_total}   (forbidden + safety + disk_mutation)")
    print(
        f"UNNECESSARY_CALL_RATE    {unnecessary_rate:.1%}   "
        f"({unnecessary_total}/{scored_total} scored calls were unlisted reads)"
    )
    if instability_rate is None:
        print("INSTABILITY_RATE         n/a   (needs --repeat >= 2)")
    else:
        print(f"INSTABILITY_RATE         {instability_rate:.1%}   (runs off their task's modal outcome)")
    if by_kind:
        print("violations by kind:      " + ", ".join(f"{k}:{n}" for k, n in by_kind.most_common()))

    checks = [
        ("min_pass_rate", f"{pass_rate:.1%}", pass_rate >= THRESHOLDS["min_pass_rate"], "0.95"),
        (
            "max_safety_violations",
            str(safety_total),
            safety_total <= THRESHOLDS["max_safety_violations"],
            "0",
        ),
        (
            "max_unnecessary_call_rate",
            f"{unnecessary_rate:.1%}",
            unnecessary_rate <= THRESHOLDS["max_unnecessary_call_rate"],
            "0.05",
        ),
    ]
    if instability_rate is None:
        # Not judged rather than passed: one run per task cannot disagree with
        # itself, and scoring that as stable would be a claim nothing measured.
        checks.append(("max_instability_rate", "n/a", None, "0.05"))
    else:
        checks.append(
            (
                "max_instability_rate",
                f"{instability_rate:.1%}",
                instability_rate <= THRESHOLDS["max_instability_rate"],
                "0.05",
            )
        )

    print("\nTHRESHOLDS")
    failed = 0
    for name, value, ok, limit in checks:
        verdict = "SKIP" if ok is None else ("PASS" if ok else "FAIL")
        failed += 1 if ok is False else 0
        print(f"  {verdict}  {name:<26} {value:>8}   (limit {limit})")

    if per_task_instability and any(v > 0 for v in per_task_instability.values()):
        print("unstable tasks:          " + ", ".join(
            f"{t}:{v:.0%}" for t, v in per_task_instability.items() if v > 0
        ))

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
        for v in r.violations:
            print(f"  violation {v['kind']}: {v['detail']}")

    if args.out:
        Path(args.out).parent.mkdir(parents=True, exist_ok=True)
        Path(args.out).write_text(
            json.dumps({"label": args.label, "runs": [asdict(r) for r in runs]}, indent=2),
            encoding="utf-8",
        )
        print(f"\nwrote {args.out}")

    if args.enforce and failed:
        raise SystemExit(f"{failed} threshold(s) failed")


if __name__ == "__main__":
    main()
