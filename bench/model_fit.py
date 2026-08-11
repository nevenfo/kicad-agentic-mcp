"""Does a small local model write a *valid* KiCAD Plan IR, and how often, at
what local-token cost and latency?

The oracle is never the model. Every attempt is graded on a 0-3 ladder by
running it through the real, built server — `preview_plan` then `apply_plan`
via `kicad_invoke`, `verify` handled by this harness's own invariant checks —
exactly the path `bench/runner.py` already proves against real `kicad-cli`.
Nothing here reimplements the compiler or ERC parsing; `check_assertion` and
`GatewayClient` are imported from `runner.py`, not rewritten.

    0 = the reply is not schema-valid JSON
    1 = valid JSON, but `preview_plan` refuses it (compile failure)
    2 = it compiles and `apply_plan` runs, but a task invariant or the ERC
        budget fails
    3 = it applies, every invariant holds, and ERC is within budget

The backend is OpenAI-compatible HTTP (`POST {base_url}/chat/completions`),
the same contract `crates/kam-llm/src/openai_compat.rs` implements: streamed
SSE, `stream_options.include_usage`, `response_format: {type: json_schema}`.
Loopback only by convention (no host is contacted but the one given).

The prompt is four blocks, in a fixed order, so the first three are
prefix-cache friendly and byte-identical across every task and every model:

    [IMMUTABLE SYSTEM RULES] [PLAN IR SCHEMA] [OPERATION LIBRARY]
    [DYNAMIC TASK] [ACTIVE TASK ANCHOR]

The schema and operation-library blocks are not hand-typed: they come from
`kicad_describe(["apply_plan"])` against the real server, the same call
`bench/dump_catalog.py` and `bench/plan_cost.py` already make. A hand-copied
schema would drift from `kam-plan` and the model would be refused for a
reason this harness invented.

Usage:
    python bench/model_fit.py --selftest --server .\\target\\release\\konnect.exe
    python bench/model_fit.py --server .\\target\\release\\konnect.exe \\
        --model qwen3.5-9b --base-url http://127.0.0.1:1234/v1
"""

from __future__ import annotations

import argparse
import json
import os
import re
import statistics
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).parent))

import tiktoken  # noqa: E402
import yaml  # noqa: E402

from mcp_client import McpStdioClient  # noqa: E402
from runner import GatewayClient, check_assertion  # noqa: E402 - the real oracle path

ENC = tiktoken.get_encoding("o200k_base")
TASK_DIR = Path(__file__).parent / "model_tasks"
RESULTS_DIR = Path(__file__).parent / "results"


def tokens(text: str) -> int:
    return len(ENC.encode(text))


def text_of(result: Any) -> str:
    if not isinstance(result, dict):
        return ""
    return "\n".join(p.get("text", "") for p in result.get("content", []) if p.get("type") == "text")


# ── the four stable-prefix blocks ───────────────────────────────────────────

IMMUTABLE_SYSTEM_RULES = """You are a deterministic KiCAD schematic planner. You do not call tools \
directly: you write one Plan IR document and nothing else.

Output ONLY a single JSON object — the plan document itself. No markdown \
fences, no prose before or after it, no explanation.

Every coordinate you write inside a place/power/label/wire/decouple \
operation's arguments is snapped to the 1.27mm schematic grid by the server \
before it is used. Do not hand-round coordinates; write the number the task \
gives you.

A coordinate field must be a literal number, never a ${...} reference — the \
server refuses one, because snapping happens before any step has run. Use \
the 'call' operation, which does no arithmetic, for anything that genuinely \
needs a previous step's raw output.

Use ${op_id.field} to read a non-coordinate field — a file path, a \
reference designator, anything a tool actually returned — from an earlier \
operation named by its 'id'. The id is whatever you gave that operation; if \
you gave none, it is 'op1', 'op2', ... by position.

If you are unsure the design is electrically correct, write the plan \
anyway. The server verifies it against real KiCAD ERC; you are not the \
verifier."""


def fetch_apply_plan_schema(server: str, config: str) -> dict[str, Any]:
    """`kicad_describe(["apply_plan"])` against the real server — the same
    call `bench/plan_cost.py` and `bench/dump_catalog.py` make. Never
    hand-copied: a schema this harness retyped would drift from `kam-plan`.
    """
    env = dict(os.environ)
    env.setdefault("RUST_LOG", "warn")
    with McpStdioClient([server, "--config", config], env=env) as c:
        c.initialize()
        result = c.tools_call("kicad_describe", {"names": ["apply_plan"]})
        payload = json.loads(text_of(result.result))
    tools = payload.get("tools", [])
    if not tools:
        raise SystemExit(
            f"kicad_describe(['apply_plan']) found nothing; not_found={payload.get('not_found')}"
        )
    return tools[0]


def build_stable_blocks(tool_def: dict[str, Any]) -> tuple[str, str]:
    """PLAN IR SCHEMA and OPERATION LIBRARY, both derived from the fetched
    `apply_plan` tool definition — never retyped.
    """
    schema = tool_def["input_schema"]
    schema_block = (
        "PLAN IR SCHEMA (apply_plan's input_schema, fetched from the running server via "
        "kicad_describe, not hand-copied):\n" + json.dumps(schema, indent=2, sort_keys=True)
    )
    op_library_text = schema.get("properties", {}).get("plan", {}).get("description", "")
    op_library_block = "OPERATION LIBRARY (the schema's own 'plan' field description):\n" + op_library_text
    return schema_block, op_library_block


def stable_prefix(schema_block: str, op_library_block: str) -> str:
    return "\n\n".join([IMMUTABLE_SYSTEM_RULES, schema_block, op_library_block])


HINT_LEVELS = ("full", "minimal", "none")


def task_hint(task: dict[str, Any], hint_level: str) -> str:
    """The `notes` block at a given hint level. `full` is the geometry-complete
    block (coordinates, pin offsets, PWR_FLAG); `minimal` is only what is not
    electronics and not guessable (the grid, the ${...}-in-a-coordinate
    refusal); `none` is empty — the objective is all the model gets.
    """
    return (task.get("hints") or {}).get(hint_level, "")


def build_dynamic_task(task: dict[str, Any], env: dict[str, str], hint_level: str) -> str:
    lines = [f"TASK: {task['title']}", "", task["objective"].strip()]
    hint_text = task_hint(task, hint_level)
    if hint_text:
        lines += ["", "NOTES:", hint_text.strip()]
    lines += [
        "",
        f"$WORK = {env['WORK']}",
        f"$NAME = {env['NAME']}",
        f"$SCH  = {env['SCH']}",
    ]
    return "\n".join(lines)


def build_anchor(task: dict[str, Any]) -> str:
    inv = task.get("invariants", {})
    parts = [f"ACTIVE TASK ANCHOR — {task['title']}."]
    if inv.get("components"):
        parts.append(f"Required components: {', '.join(inv['components'])}.")
    if inv.get("nets"):
        parts.append(f"Required nets: {', '.join(inv['nets'])}.")
    parts.append(f"ERC (severity=error) must report at most {inv.get('erc_max_errors', 0)}.")
    parts.append("Respond with the plan JSON only.")
    return " ".join(parts)


# ── the OpenAI-compatible backend call ──────────────────────────────────────

# A minimal, truthful envelope: only what kam_plan::ir::Plan::from_json
# actually requires ('ops', each needing 'op'). Not a hand-copied per-operation
# schema — 'with' is deliberately opaque, the same way the compiler treats it.
PLAN_RESPONSE_SCHEMA = {
    "type": "object",
    "properties": {
        "plan_id": {"type": "string"},
        "documents": {"type": "array", "items": {"type": "string"}},
        "ops": {
            "type": "array",
            "items": {
                "type": "object",
                "properties": {
                    "id": {"type": "string"},
                    "op": {"type": "string"},
                    "with": {"type": "object"},
                },
                "required": ["op"],
            },
        },
        "constraints": {"type": "array", "items": {"type": "string"}},
        "validators": {"type": "array", "items": {"type": "string"}},
        "rollback_policy": {"type": "string"},
    },
    "required": ["ops"],
}


def call_backend(
    base_url: str, model: str, system_text: str, user_text: str, temperature: float, timeout: float
) -> dict[str, Any]:
    """One `/chat/completions` call, streamed exactly like
    `crates/kam-llm/src/openai_compat.rs::wire_request` builds it.

    Never raises: every failure mode (unreachable, HTTP error, timeout,
    malformed stream) comes back as `{"error": ..., "backend_unreachable": bool}`
    so the caller can tell "the backend is down" from "the model failed".
    """
    url = base_url.rstrip("/") + "/chat/completions"
    body = {
        "model": model,
        "messages": [
            {"role": "system", "content": system_text},
            {"role": "user", "content": user_text},
        ],
        "stream": True,
        "stream_options": {"include_usage": True},
        "response_format": {
            "type": "json_schema",
            "json_schema": {"name": "kicad_plan", "schema": PLAN_RESPONSE_SCHEMA, "strict": False},
        },
        "temperature": temperature,
    }
    req = urllib.request.Request(
        url,
        data=json.dumps(body).encode("utf-8"),
        headers={"Content-Type": "application/json"},
        method="POST",
    )

    t0 = time.perf_counter()
    ttft_ms: float | None = None
    parts: list[str] = []
    usage = {"prompt_tokens": 0, "completion_tokens": 0}

    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            for raw_line in resp:
                line = raw_line.decode("utf-8", "replace").strip()
                if not line.startswith("data:"):
                    continue
                data = line[len("data:") :].strip()
                if not data or data == "[DONE]":
                    continue
                if ttft_ms is None:
                    ttft_ms = (time.perf_counter() - t0) * 1000.0
                chunk = json.loads(data)
                if chunk.get("usage"):
                    usage["prompt_tokens"] = chunk["usage"].get("prompt_tokens", 0) or usage["prompt_tokens"]
                    usage["completion_tokens"] = (
                        chunk["usage"].get("completion_tokens", 0) or usage["completion_tokens"]
                    )
                for choice in chunk.get("choices") or []:
                    delta = choice.get("delta") or {}
                    if delta.get("content"):
                        parts.append(delta["content"])
    except urllib.error.HTTPError as e:
        detail = e.read().decode("utf-8", "replace")[:300]
        return {"error": f"backend HTTP {e.code} at {url}: {detail}", "backend_unreachable": False}
    except urllib.error.URLError as e:
        return {"error": f"backend unreachable at {url}: {e.reason}", "backend_unreachable": True}
    except TimeoutError:
        return {"error": f"backend timed out after {timeout}s at {url}", "backend_unreachable": False}
    except (json.JSONDecodeError, OSError, ValueError) as e:
        return {"error": f"malformed response from {url}: {e}", "backend_unreachable": False}

    wall_ms = (time.perf_counter() - t0) * 1000.0
    tps = None
    if usage["completion_tokens"] and ttft_ms is not None:
        generation_s = max((wall_ms - ttft_ms) / 1000.0, 1e-9)
        tps = usage["completion_tokens"] / generation_s

    return {
        "content": "".join(parts),
        "local_input_tokens": usage["prompt_tokens"],
        "local_output_tokens": usage["completion_tokens"],
        "ttft_ms": ttft_ms,
        "wall_clock_ms": wall_ms,
        "tokens_per_second": tps,
        "error": None,
        "backend_unreachable": False,
    }


def _sample_vram(stop: threading.Event, samples: list[int | None]) -> None:
    while not stop.is_set():
        try:
            out = subprocess.run(
                ["nvidia-smi", "--query-gpu=memory.used", "--format=csv,noheader,nounits"],
                capture_output=True,
                text=True,
                timeout=2,
            )
            if out.returncode == 0:
                vals = [int(x.strip()) for x in out.stdout.splitlines() if x.strip()]
                if vals:
                    samples.append(max(vals))
        except (FileNotFoundError, subprocess.TimeoutExpired, ValueError, OSError):
            # nvidia-smi absent or broken: record nothing and stop polling —
            # never crash the harness over an optional metric.
            return
        stop.wait(0.2)


def call_backend_with_vram(*args: Any, **kwargs: Any) -> dict[str, Any]:
    samples: list[int | None] = []
    stop = threading.Event()
    thread = threading.Thread(target=_sample_vram, args=(stop, samples), daemon=True)
    thread.start()
    result = call_backend(*args, **kwargs)
    stop.set()
    thread.join(timeout=1.0)
    valid = [s for s in samples if s is not None]
    result["vram_peak_mib"] = max(valid) if valid else None
    return result


# ── grading: the oracle path, never reimplemented ───────────────────────────


def build_assert_specs(task: dict[str, Any]) -> list[dict[str, Any]]:
    inv = task.get("invariants", {})
    specs = []
    if inv.get("components"):
        specs.append({"kind": "components_present", "value": inv["components"]})
    if inv.get("nets"):
        specs.append({"kind": "nets_present", "value": inv["nets"]})
    specs.append({"kind": "erc_max_errors", "value": inv.get("erc_max_errors", 0)})
    return specs


def grade_plan_text(server: str, config: str, task: dict[str, Any], env: dict[str, str], raw_text: str) -> dict[str, Any]:
    """Grade one model reply against the real server. Never touches the
    compiler or ERC directly — `preview_plan`/`apply_plan` do the compiling,
    `check_assertion` (imported from `runner.py`) does the invariant reading.
    """
    result: dict[str, Any] = {
        "grade": None,
        "valid_json": False,
        "compiles": False,
        "applies": False,
        "invariants_pass": False,
        "erc_errors": None,
        "error": None,
    }

    try:
        parsed = json.loads(raw_text)
    except (json.JSONDecodeError, TypeError):
        result.update(grade=0, error="reply is not valid JSON")
        return result
    if not isinstance(parsed, dict):
        result.update(grade=0, error="reply JSON is not an object")
        return result
    result["valid_json"] = True

    proc_env = dict(os.environ)
    proc_env.setdefault("RUST_LOG", "warn")
    with McpStdioClient([server, "--config", config], env=proc_env) as raw_client:
        raw_client.initialize()
        client = GatewayClient(raw_client)

        preview = client.tools_call("preview_plan", {"plan": parsed})
        preview_failed = bool(preview.error) or bool((preview.result or {}).get("isError"))
        if preview_failed:
            result.update(grade=1, error=(text_of(preview.result) or str(preview.error))[:400])
            return result
        result["compiles"] = True

        apply = client.tools_call("apply_plan", {"plan": parsed})
        apply_failed = bool(apply.error) or bool((apply.result or {}).get("isError"))
        if apply_failed:
            result.update(grade=2, error=(text_of(apply.result) or str(apply.error))[:400])
            return result
        result["applies"] = True

        step_errors: list[str] = []
        assertions = [check_assertion(spec, client, env, step_errors) for spec in build_assert_specs(task)]

    erc = next((a for a in assertions if a.kind == "erc_max_errors"), None)
    if erc is not None:
        m = re.search(r"errors=(\d+)", erc.detail)
        result["erc_errors"] = int(m.group(1)) if m else None

    ok = all(a.ok for a in assertions)
    result["invariants_pass"] = ok
    result["grade"] = 3 if ok else 2
    if not ok:
        result["error"] = "; ".join(a.detail for a in assertions if not a.ok)
    return result


# ── task loading ─────────────────────────────────────────────────────────────


def load_model_tasks(only: str | None) -> list[dict[str, Any]]:
    wanted = set(only.split(",")) if only else None
    tasks = []
    for path in sorted(TASK_DIR.glob("*.yaml")):
        task = yaml.safe_load(path.read_text(encoding="utf-8"))
        if wanted and task["id"] not in wanted:
            continue
        tasks.append(task)
    return tasks


def fresh_env(task: dict[str, Any]) -> dict[str, str]:
    work = Path(tempfile.mkdtemp(prefix=f"kam-modelfit-{task.get('id', 'task')}-"))
    posix = str(work).replace("\\", "/")
    name = task.get("project_name", task.get("id", "proj"))
    return {
        "WORK": posix,
        "NAME": name,
        "SCH": f"{posix}/{name}.kicad_sch",
        "PCB": f"{posix}/{name}.kicad_pcb",
    }


# ── selftest: prove the grading path before any model is measured ──────────

SELFTEST_TASK = {
    "id": "selftest_divider",
    "title": "selftest divider",
    "invariants": {"components": ["R1", "R2"], "erc_max_errors": 0},
}


def fixture_grade3(env: dict[str, str]) -> dict[str, Any]:
    """A hand-written, correct divider plan — the same design
    `bench/plan_cost.py::plan_document` builds, pre-snapped."""
    return {
        "plan_id": "selftest-divider",
        "ops": [
            {"op": "call", "with": {"tool": "create_project", "args": {"path": env["WORK"], "name": env["NAME"]}}},
            {
                "op": "place",
                "with": {
                    "schematic": env["SCH"],
                    "components": [
                        {"lib_id": "Device:R", "reference": "R1", "value": "10k", "x": 100.33, "y": 80.01},
                        {"lib_id": "Device:R", "reference": "R2", "value": "10k", "x": 100.33, "y": 95.25},
                        {"lib_id": "power:PWR_FLAG", "reference": "#FLG01", "x": 100.33, "y": 76.2},
                        {"lib_id": "power:PWR_FLAG", "reference": "#FLG02", "x": 100.33, "y": 99.06},
                    ],
                },
            },
            {
                "op": "power",
                "with": {
                    "schematic": env["SCH"],
                    "symbols": [{"net": "+3V3", "x": 100.33, "y": 76.2}, {"net": "GND", "x": 100.33, "y": 99.06}],
                },
            },
            {"op": "connect", "with": {"schematic": env["SCH"], "connections": [{"from": "R1.2", "to": "R2.1"}]}},
            {"op": "label", "with": {"schematic": env["SCH"], "labels": [{"net": "VOUT", "x": 100.33, "y": 87.63}]}},
        ],
    }


def fixture_grade1(env: dict[str, str]) -> dict[str, Any]:
    """An unknown operation name — refused at compile time by `preview_plan`."""
    return {"ops": [{"op": "levitate", "with": {"schematic": env["SCH"]}}]}


def fixture_grade2(env: dict[str, str]) -> dict[str, Any]:
    """Compiles and applies — two resistors placed, nothing wired, nothing
    powered — so every pin floats. E12: a floating passive pin is an ERC
    *error* in KiCad 10, not a warning."""
    return {
        "plan_id": "selftest-floating",
        "ops": [
            {"op": "call", "with": {"tool": "create_project", "args": {"path": env["WORK"], "name": env["NAME"]}}},
            {
                "op": "place",
                "with": {
                    "schematic": env["SCH"],
                    "components": [
                        {"lib_id": "Device:R", "reference": "R1", "value": "10k", "x": 100.33, "y": 80.01},
                        {"lib_id": "Device:R", "reference": "R2", "value": "10k", "x": 100.33, "y": 95.25},
                    ],
                },
            },
        ],
    }


FIXTURE_GRADE0_TEXT = '{"ops": [ this is not valid json'


def run_selftest(args: argparse.Namespace) -> None:
    tool_def = fetch_apply_plan_schema(args.server, args.config)
    schema_block, op_block = build_stable_blocks(tool_def)
    prefix = stable_prefix(schema_block, op_block)

    # Point 4: the first three blocks must be byte-identical across every
    # task. Build the full system message for two different tasks and prove
    # it does not move; only the user turn (dynamic task + anchor) may.
    tasks = load_model_tasks(None)
    if len(tasks) < 2:
        raise SystemExit(f"expected at least 2 tasks in {TASK_DIR}, found {len(tasks)}")
    env_a, env_b = fresh_env(tasks[0]), fresh_env(tasks[1])
    system_a = stable_prefix(schema_block, op_block)
    system_b = stable_prefix(schema_block, op_block)
    user_a = build_dynamic_task(tasks[0], env_a, "full") + "\n\n" + build_anchor(tasks[0])
    user_b = build_dynamic_task(tasks[1], env_b, "full") + "\n\n" + build_anchor(tasks[1])
    assert system_a == system_b, "the stable prefix moved between two tasks"
    assert user_a != user_b, "two different tasks produced the same dynamic block"
    print(f"stable prefix identical across tasks: {tokens(prefix)} tk")
    print(f"  {tasks[0]['id']} dynamic+anchor: {tokens(user_a)} tk")
    print(f"  {tasks[1]['id']} dynamic+anchor: {tokens(user_b)} tk")

    print(f"\nprompt block token counts (o200k_base, same counting as bench/runner.py):")
    print(f"  IMMUTABLE_SYSTEM_RULES  {tokens(IMMUTABLE_SYSTEM_RULES):>6} tk")
    print(f"  PLAN IR SCHEMA          {tokens(schema_block):>6} tk")
    print(f"  OPERATION LIBRARY       {tokens(op_block):>6} tk")
    print(f"  stable prefix (total)   {tokens(prefix):>6} tk")

    # Same task, three hint levels. The prefix must not move: a hint belongs
    # to the dynamic part, never the stable one.
    task0 = tasks[0]
    envs_by_hint = {level: fresh_env(task0) for level in HINT_LEVELS}
    systems_by_hint = {level: stable_prefix(schema_block, op_block) for level in HINT_LEVELS}
    users_by_hint = {
        level: build_dynamic_task(task0, envs_by_hint[level], level) + "\n\n" + build_anchor(task0)
        for level in HINT_LEVELS
    }
    for level, system_msg in systems_by_hint.items():
        assert system_msg == prefix, f"stable prefix moved at hint level '{level}'"
    assert len({users_by_hint[level] for level in HINT_LEVELS}) == len(HINT_LEVELS), (
        "two hint levels produced the same dynamic block"
    )
    print(f"\nstable prefix identical across hint levels for {task0['id']}: {tokens(prefix)} tk")
    for level in HINT_LEVELS:
        print(f"  hint={level:<7} dynamic+anchor: {tokens(users_by_hint[level]):>4} tk")

    checks = [
        ("grade 3 (correct divider plan)", fixture_grade3, 3, False),
        ("grade 1 (unknown operation)", fixture_grade1, 1, False),
        ("grade 2 (floating pins, E12)", fixture_grade2, 2, False),
        ("grade 0 (malformed JSON)", None, 0, True),
    ]

    print("\nselftest — grading path only, no model involved:")
    failures = []
    for label, fixture_fn, expected, is_raw_text in checks:
        env = fresh_env(SELFTEST_TASK)
        raw = FIXTURE_GRADE0_TEXT if is_raw_text else json.dumps(fixture_fn(env))
        result = grade_plan_text(args.server, args.config, SELFTEST_TASK, env, raw)
        got = result["grade"]
        status = "OK" if got == expected else "FAIL"
        print(f"  [{status}] {label}: expected grade {expected}, got {got}  ({result.get('error') or 'no error'})")
        if got != expected:
            failures.append(label)

    if failures:
        raise SystemExit(f"\nSELFTEST FAILED: {failures}")
    print("\nSELFTEST PASSED — the oracle is proven before any model is measured.")


# ── aggregation and reporting ────────────────────────────────────────────────

NUMERIC_METRICS = [
    "local_input_tokens",
    "local_output_tokens",
    "ttft_ms",
    "tokens_per_second",
    "wall_clock_ms",
    "vram_peak_mib",
    "erc_errors",
]


def _p95(values: list[float]) -> float:
    s = sorted(values)
    return s[int(0.95 * (len(s) - 1))]


def aggregate_attempts(attempts: list[dict[str, Any]]) -> dict[str, Any]:
    graded = [a for a in attempts if a.get("grade") is not None]
    histogram = {g: sum(1 for a in graded if a["grade"] == g) for g in (0, 1, 2, 3)}
    out: dict[str, Any] = {
        "attempts": len(attempts),
        "graded": len(graded),
        "success_rate": (histogram[3] / len(graded)) if graded else None,
        "grade_histogram": histogram,
    }
    for metric in NUMERIC_METRICS:
        values = [a[metric] for a in attempts if a.get(metric) is not None]
        if values:
            out[metric] = {"median": statistics.median(values), "p95": _p95(values), "n": len(values)}
        else:
            out[metric] = None
    return out


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--server", required=True, help="path to the built konnect server binary")
    ap.add_argument("--config", default=str(Path(__file__).parent / "konnect.bench.toml"))
    ap.add_argument("--model", default=None, help="model id as the backend expects it")
    ap.add_argument("--base-url", default="http://127.0.0.1:1234/v1")
    ap.add_argument("--repeat", type=int, default=5)
    ap.add_argument("--temperature", type=float, default=0.2)
    ap.add_argument("--tasks", default=None, help="comma-separated task ids; default all of bench/model_tasks")
    ap.add_argument(
        "--hints",
        default=",".join(HINT_LEVELS),
        help="comma-separated hint levels to run, from full/minimal/none (default: all three)",
    )
    ap.add_argument("--timeout", type=float, default=120.0)
    ap.add_argument("--out", default=None)
    ap.add_argument("--selftest", action="store_true", help="prove the grading path; no model involved")
    args = ap.parse_args()

    if args.selftest:
        run_selftest(args)
        return

    if not args.model:
        raise SystemExit("--model is required unless --selftest")

    tool_def = fetch_apply_plan_schema(args.server, args.config)
    schema_block, op_block = build_stable_blocks(tool_def)
    prefix = stable_prefix(schema_block, op_block)
    print(
        f"stable prefix: {tokens(prefix)} tk "
        f"(rules {tokens(IMMUTABLE_SYSTEM_RULES)} + schema {tokens(schema_block)} + "
        f"ops {tokens(op_block)})"
    )

    tasks = load_model_tasks(args.tasks)
    if not tasks:
        raise SystemExit(f"no tasks matched in {TASK_DIR}")

    hint_levels = [h.strip() for h in args.hints.split(",") if h.strip()]
    unknown = [h for h in hint_levels if h not in HINT_LEVELS]
    if unknown:
        raise SystemExit(f"--hints named {unknown}, not in {HINT_LEVELS}")
    if not hint_levels:
        raise SystemExit("--hints named no levels")

    results: dict[str, Any] = {
        "model": args.model,
        "base_url": args.base_url,
        "temperature": args.temperature,
        "repeat": args.repeat,
        "hint_levels": hint_levels,
        "stable_prefix_tokens": tokens(prefix),
        # tasks[task_id][hint_level] -> list of per-attempt records. Never
        # collapsed into one series: a hint level changes what the model was
        # told, so mixing levels in one number would compare different tasks.
        "tasks": {},
    }

    backend_unreachable: str | None = None
    for task in tasks:
        results["tasks"][task["id"]] = {}
        for hint_level in hint_levels:
            attempts: list[dict[str, Any]] = []
            for i in range(args.repeat):
                env = fresh_env(task)
                user_text = build_dynamic_task(task, env, hint_level) + "\n\n" + build_anchor(task)
                backend = call_backend_with_vram(
                    args.base_url, args.model, prefix, user_text, args.temperature, args.timeout
                )
                if backend.get("backend_unreachable"):
                    backend_unreachable = backend["error"]
                    break

                attempt: dict[str, Any] = {
                    "llm_calls": 1,
                    "local_input_tokens": backend.get("local_input_tokens"),
                    "local_output_tokens": backend.get("local_output_tokens"),
                    "ttft_ms": backend.get("ttft_ms"),
                    "tokens_per_second": backend.get("tokens_per_second"),
                    "wall_clock_ms": backend.get("wall_clock_ms"),
                    "vram_peak_mib": backend.get("vram_peak_mib"),
                }
                if backend.get("error"):
                    attempt.update(
                        grade=None,
                        valid_json=False,
                        compiles=False,
                        applies=False,
                        invariants_pass=False,
                        erc_errors=None,
                        error=backend["error"],
                    )
                else:
                    attempt.update(grade_plan_text(args.server, args.config, task, env, backend["content"]))
                attempts.append(attempt)
                print(
                    f"  {task['id']} hint={hint_level} [{i + 1}/{args.repeat}] "
                    f"grade={attempt.get('grade')} error={attempt.get('error')}"
                )

            results["tasks"][task["id"]][hint_level] = attempts
            if backend_unreachable:
                break
        if backend_unreachable:
            break

    if backend_unreachable:
        results["backend_unreachable"] = backend_unreachable
        print(f"\nBACKEND UNREACHABLE — not a model failure: {backend_unreachable}")
    else:
        by_task_hint = {
            tid: {level: aggregate_attempts(attempts) for level, attempts in by_hint.items()}
            for tid, by_hint in results["tasks"].items()
        }
        # Per hint level, across every task — still never mixed with another
        # hint level.
        by_hint = {
            level: aggregate_attempts(
                [a for by_hint in results["tasks"].values() for a in by_hint.get(level, [])]
            )
            for level in hint_levels
        }
        results["aggregate"] = {"by_task_hint": by_task_hint, "by_hint": by_hint}

        print(f"\n=== {args.model} ===")
        for tid, by_hint_agg in by_task_hint.items():
            for level, agg in by_hint_agg.items():
                sr = agg["success_rate"]
                sr_s = f"{sr:.1%}" if sr is not None else "n/a"
                print(f"  {tid:<24} hint={level:<7} success={sr_s}  histogram={agg['grade_histogram']}")
        print()
        for level, agg in by_hint.items():
            sr = agg["success_rate"]
            sr_s = f"{sr:.1%}" if sr is not None else "n/a"
            print(f"  ACROSS TASKS hint={level:<7} success={sr_s}  histogram={agg['grade_histogram']}")

    slug = re.sub(r"[^a-zA-Z0-9._-]+", "-", args.model).strip("-")
    out = Path(args.out) if args.out else RESULTS_DIR / f"model-fit-{slug}.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(results, indent=2), encoding="utf-8")
    print(f"\nwritten to {out}")


if __name__ == "__main__":
    main()
