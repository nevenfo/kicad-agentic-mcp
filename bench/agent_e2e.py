"""Bounded live H.7.3 Agent gateway harness.

It reuses model_fit's prompt/schema construction and the real MCP gateway; it
does not grade, compile, execute, or validate Plan IR itself.
"""
from __future__ import annotations

import argparse
import json
import os
import shutil
import sys
import tempfile
import time
from pathlib import Path
from typing import Any

import yaml

sys.path.insert(0, str(Path(__file__).parent))

from mcp_client import McpStdioClient  # noqa: E402
from model_fit import (  # noqa: E402
    ENC,
    IMMUTABLE_SYSTEM_RULES,
    RESULTS_DIR,
    TASK_DIR,
    build_dynamic_task,
    build_stable_blocks,
    fetch_apply_plan_schema,
    tokens,
)


def text_of(result: Any) -> str:
    return "\n".join(part.get("text", "") for part in (result or {}).get("content", []) if part.get("type") == "text")


def payload(call: Any) -> dict[str, Any]:
    return json.loads(text_of(call.result))


def fresh_env(task: dict[str, Any]) -> tuple[Path, dict[str, str]]:
    work = Path(tempfile.mkdtemp(prefix=f"kam-agent-{task['id']}-"))
    name = task.get("project_name", task["id"])
    root = str(work).replace("\\", "/")
    return work, {"WORK": root, "NAME": name, "SCH": f"{root}/{name}.kicad_sch", "PCB": f"{root}/{name}.kicad_pcb"}


def atomic_bundle(task: dict[str, Any], env: dict[str, str], schema: dict[str, Any]) -> tuple[dict[str, Any], int, int]:
    schema_block, op_library = build_stable_blocks(schema)
    electrical = build_dynamic_task(task, env, "full")
    geometry = (task.get("hints") or {}).get("geometry", "")
    plan_ir = "\n\n".join((IMMUTABLE_SYSTEM_RULES, schema_block, op_library))
    bundle = {"electrical_constraints": electrical, "plan_ir": plan_ir, "geometry": geometry}
    measured = tokens(json.dumps(bundle, sort_keys=True))
    bundle["measured_tokens"] = measured
    return bundle, tokens(electrical), tokens(plan_ir)


def run(args: argparse.Namespace) -> dict[str, Any]:
    task = next(yaml.safe_load(path.read_text(encoding="utf-8")) for path in sorted(TASK_DIR.glob("*.yaml")) if yaml.safe_load(path.read_text(encoding="utf-8"))["id"] == args.task)
    schema = fetch_apply_plan_schema(args.server, args.config)
    proc_env = dict(os.environ, KONNECT_LOCAL_LLM_BASE_URL=args.base_url, KONNECT_LOCAL_LLM_MODEL=args.model, RUST_LOG="warn")
    attempts: list[dict[str, Any]] = []
    with McpStdioClient([args.server, "--config", args.config], env=proc_env) as client:
        client.initialize("h7-agent-e2e")
        for number in range(1, args.attempts + 1):
            work, env = fresh_env(task)
            try:
                bundle, core_tokens, prefix_tokens = atomic_bundle(task, env, schema)
                task_call = client.tools_call("kicad_invoke", {"calls": [{"tool": "start_task", "args": {"objective": build_dynamic_task(task, env, "full")}}]})
                task_payload = payload(task_call)
                inner = task_payload["results"][0]
                if not inner.get("ok"):
                    raise RuntimeError(f"start_task failed: {inner}")
                task_id = inner["result"]["task_id"]
                t0 = time.perf_counter()
                call = client.tools_call("kicad_agent", {
                    "task_id": task_id, "decision": "LOCAL", "execute": True, "document": env["SCH"],
                    "task_core_tokens": core_tokens, "fixed_prefix_tokens": prefix_tokens,
                    "retrieval_bundles": [bundle],
                }, timeout=args.timeout)
                wall_ms = (time.perf_counter() - t0) * 1000.0
                body = payload(call) if not call.error else {"status": "TRANSPORT_FAILED", "reason": str(call.error)}
                attempts.append({"attempt": number, "task_id": task_id, "work": env["WORK"], "status": body.get("status"), "preview": body.get("preview"), "application": body.get("application"), "verification": body.get("verification"), "evidence": (body.get("verification") or {}).get("evidence"), "usage": (body.get("supervisor") or {}).get("usage"), "reason": body.get("reason"), "local_calls": 1, "external_calls": 0, "wall_clock_ms": wall_ms})
                if body.get("status") == "SUCCESS":
                    break
            finally:
                if not args.keep:
                    shutil.rmtree(work, ignore_errors=True)
    # The same three numbers `bench/runner.py` reports for the oracle modes,
    # computed with the same `o200k_base` encoder and the same formulas, so the
    # Agent column of the M.1 table is not a different measurement wearing the
    # same names. `catalog_tokens` is expected to be 0: this path exposes no
    # tool, so no `notifications/tools/list_changed` ever fires.
    calls = client.session.calls
    tool_calls = [c for c in calls if c.method == "tools/call"]
    catalog_calls = [c for c in calls if c.method == "tools/list" and c.result]
    surface = {
        "mcp_calls": len(tool_calls),
        "request_tokens": sum(tokens(json.dumps(c.params, separators=(",", ":"))) for c in tool_calls),
        "response_tokens": sum(tokens(text_of(c.result)) for c in tool_calls if c.result),
        "catalog_tokens": sum(tokens(json.dumps(c.result.get("tools", []), separators=(",", ":"))) for c in catalog_calls),
        "catalog_refreshes": len(catalog_calls),
        # Paid once per session, not per task: the `kicad_describe(["apply_plan"])`
        # this harness fetches before the run so the prompt carries the real
        # schema rather than a retyped one.
        "setup_tokens": tokens(json.dumps(schema, separators=(",", ":"))),
    }
    surface["external_tokens"] = surface["response_tokens"] + surface["catalog_tokens"]
    return {"harness": "H.7.3", "task": task["id"], "hint": "full", "model": args.model, "base_url": args.base_url, "loopback": args.base_url.startswith("http://127.0.0.1"), "context_window_tokens": 32768, "reasoning_effort": "medium", "attempt_limit": args.attempts, "attempts": attempts, "local_calls": len(attempts), "external_calls": 0, "surface": surface}


def selftest() -> None:
    task = {"id": "t", "title": "t", "objective": "work $WORK $NAME $SCH", "hints": {"full": "electric", "geometry": "geo"}}
    _work, env = fresh_env(task)
    bundle, core, prefix = atomic_bundle(
        task,
        env,
        {"input_schema": {"properties": {"plan": {"description": "ops"}}}},
    )
    assert {"electrical_constraints", "plan_ir", "geometry", "measured_tokens"} <= bundle.keys()
    assert bundle["measured_tokens"] == tokens(json.dumps({k: v for k, v in bundle.items() if k != "measured_tokens"}, sort_keys=True))
    assert core > 0 and prefix > 0 and "$SCH" not in bundle["electrical_constraints"]
    shutil.rmtree(_work, ignore_errors=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--server", default="target/release/konnect.exe")
    parser.add_argument("--config", default=str(Path(__file__).parent / "konnect.bench.toml"))
    parser.add_argument("--base-url", default="http://127.0.0.1:1234/v1")
    parser.add_argument("--model", default="openai/gpt-oss-20b")
    parser.add_argument("--task", default="model_divider")
    parser.add_argument("--attempts", type=int, default=5)
    parser.add_argument("--timeout", type=float, default=600)
    parser.add_argument("--out", default=str(RESULTS_DIR / "agent-e2e-h7.3.json"))
    parser.add_argument("--keep", action="store_true")
    parser.add_argument("--selftest", action="store_true")
    args = parser.parse_args()
    selftest()
    if args.selftest:
        print("agent_e2e selftest: PASS")
    else:
        result = run(args)
        Path(args.out).write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
        print(json.dumps({"out": args.out, "attempts": len(result["attempts"]), "status": result["attempts"][-1]["status"] if result["attempts"] else "NONE"}))
