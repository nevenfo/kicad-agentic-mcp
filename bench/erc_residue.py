"""Replay a model-fit run's applied plans and name the ERC violations it left.

`model_fit.py` records `erc_errors` as a count, because the assertion it comes
from (`erc_max_errors`) is a budget check and a budget only needs a number. A
count cannot tell a deterministic gap in the operation library from a design
mistake the model made, and that distinction is what decides whether a failure
belongs to the NO_LLM tier or to the model (H.6.1 / H.6.2, D37).

This replays every attempt the run recorded as *applied* with at least one ERC
error — the compiled plan verbatim, against the same built server — and prints
each violation's own description, then a histogram of the classes.

The replay is read-only with respect to the repository and to the original run:
the plan's `create` path is rewritten to a fresh temporary directory before the
plan is applied, and nothing is written back into the results file.

    python bench/erc_residue.py --server .\\target\\release\\konnect.exe \\
        --results bench/results/model-fit-gpt-oss-20b-medium-e27.json \\
        --out bench/results/erc-residue-e27.json

Every attempt must reproduce the count the run recorded. A row where
`expected != got` means the replay is not measuring the same thing the run
graded, and nothing below it should be believed.
"""
from __future__ import annotations

import argparse
import collections
import json
import os
import re
import sys
import tempfile
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).parent))

from mcp_client import McpStdioClient  # noqa: E402
from runner import GatewayClient  # noqa: E402

DEFAULT_CONFIG = str(Path(__file__).parent / "konnect.bench.toml")

# The prefixes KiCad's own ERC uses. A description that matches none of them is
# kept whole rather than bucketed into an "other" that hides what it was.
CLASSES = (
    "Pin not connected",
    "Input Power pin not driven",
    "Label not connected",
    "Wire not connected",
    "Symbol pin or wire end off connection grid",
    "Different net names between connected",
)


def text_of(result: dict[str, Any] | None) -> str:
    if not result:
        return ""
    return "".join(c.get("text", "") for c in result.get("content", []))


def classify(description: str) -> str:
    for prefix in CLASSES:
        if description.startswith(prefix):
            return prefix
    return description.split(":")[0]


def replay(server: str, config: str, plan: dict[str, Any], work: str) -> dict[str, Any]:
    """Apply one recorded plan into `work` and return its ERC violations."""
    old_path = plan["ops"][0]["with"].get("path")
    if old_path:
        plan = json.loads(json.dumps(plan).replace(old_path, work.replace("\\", "/")))

    proc_env = dict(os.environ)
    proc_env.setdefault("RUST_LOG", "warn")
    with McpStdioClient([server, "--config", config], env=proc_env) as raw:
        raw.initialize()
        client = GatewayClient(raw)

        applied = client.tools_call("apply_plan", {"plan": plan})
        body = text_of(applied.result)
        if applied.error or (applied.result or {}).get("isError"):
            return {"apply_error": body[:300], "violations": []}

        match = re.search(r'"([^"]*\.kicad_sch)"', body)
        schematic = match.group(1).replace("\\\\", "\\") if match else None
        if schematic is None:
            found = list(Path(work).glob("*.kicad_sch"))
            schematic = str(found[0]) if found else None
        if schematic is None:
            return {"apply_error": "applied but wrote no schematic", "violations": []}

        erc = client.tools_call("run_erc", {"schematic": schematic, "severity": "error"})
        try:
            payload = json.loads(text_of(erc.result))
        except (json.JSONDecodeError, TypeError):
            return {"apply_error": text_of(erc.result)[:300], "violations": []}
        return {"violations": payload.get("violations", [])}


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--server", required=True, help="path to the built konnect server binary")
    ap.add_argument("--config", default=DEFAULT_CONFIG)
    ap.add_argument("--results", required=True, help="a model_fit.py results JSON")
    ap.add_argument("--out", default=None, help="where to write the per-attempt replay")
    args = ap.parse_args()

    # Resolved before any temp directory becomes the plan's target: a relative
    # server path would otherwise be read against a cwd this script never set.
    server = str(Path(args.server).resolve())

    run = json.loads(Path(args.results).read_text(encoding="utf-8"))
    rows: list[dict[str, Any]] = []
    mismatches = 0

    for task, levels in run["tasks"].items():
        for hint, attempts in levels.items():
            for index, attempt in enumerate(attempts):
                if not attempt.get("applies") or not (attempt.get("erc_errors") or 0):
                    continue
                work = tempfile.mkdtemp(prefix="erc-residue-")
                out = replay(server, args.config, json.loads(attempt["compiled_plan"]), work)
                got = len(out["violations"])
                expected = attempt["erc_errors"]
                mismatches += got != expected
                rows.append(
                    {
                        "task": task,
                        "hint": hint,
                        "index": index,
                        "grade": attempt["grade"],
                        "failure_kind": (attempt.get("failure") or {}).get("kind"),
                        "expected_erc": expected,
                        "got_erc": got,
                        "apply_error": out.get("apply_error"),
                        "violations": [v.get("description") for v in out["violations"]],
                    }
                )
                flag = "" if got == expected else "  ** MISMATCH **"
                print(f"{task:24s} {hint:8s} #{index} expected={expected} got={got}{flag}")
                for violation in out["violations"]:
                    print(f"       {violation.get('description')}")

    if args.out:
        Path(args.out).write_text(json.dumps(rows, indent=1), encoding="utf-8")

    per_violation: collections.Counter[str] = collections.Counter()
    per_attempt: collections.Counter[tuple[str, ...]] = collections.Counter()
    blocking: collections.Counter[tuple[str, ...]] = collections.Counter()
    for row in rows:
        classes = tuple(sorted({classify(v) for v in row["violations"]}))
        per_attempt[classes] += 1
        if row["failure_kind"] == "erc_budget":
            blocking[classes] += 1
        for violation in row["violations"]:
            per_violation[classify(violation)] += 1

    total = sum(per_violation.values())
    print(f"\n== violations ({total} over {len(rows)} applied attempts) ==")
    for name, count in per_violation.most_common():
        print(f"{count:4d}  {100 * count / total:5.1f}%  {name}")

    print("\n== attempts by violation-class set ==")
    for classes, count in per_attempt.most_common():
        print(f"{count:4d}  {' + '.join(classes)}")

    print("\n== the same, restricted to attempts the budget actually failed ==")
    for classes, count in blocking.most_common():
        print(f"{count:4d}  {' + '.join(classes)}")

    if mismatches:
        print(f"\n{mismatches} attempt(s) did not reproduce their recorded count; the replay is not "
              "measuring what the run graded.")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
