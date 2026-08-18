"""Can retrieval find the plan path at all? (plan.md F.5.2)

F.5 raised precision @8 from 22.4 % to 62.0 % on the golden suite, where every
task is done the direct way: one intent per change, one tool per intent. F.5.2
asks the question that measurement could not answer — whether describing the
same change as a *compiled plan* moves precision at all. It could not be asked
while the suite was a scripted oracle that never searched; `--load-mode search`
made it askable.

The plan shape needs exactly one tool schema (`bench/plan_cost.py`'s
`SCHEMAS[(scenario, "plan")] == ["apply_plan"]`) where the direct shape needs
five. So the question splits in two, and this script measures both on one build
with the same methodology `bench/runner.py --load-mode search` uses — union of
the top-`limit` hits per query, precision = hits / |union|, recall = hits /
|needed|:

1. **Reachability.** Does a caller who states the *design goal* — the only
   thing an agent knows before it has decided how to do the work — ever get
   `apply_plan` back? Four phrasings of the same two designs, none naming the
   calling mechanism.
2. **Precision, if reached.** For a caller who already knows plans exist and
   says so, what do precision and recall become against |needed| = 1?

The direct shape of `01_sch_divider` runs as the control, from the task file
itself rather than a copy, so a drift in its intents cannot make the comparison
flattering to one side.

Costs nothing but a local process: `find_capabilities` is a read tool and no
model is involved.

Usage:
    python bench/plan_retrieval.py --server .\\target\\release\\konnect.exe
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).parent))

import yaml  # noqa: E402

from mcp_client import McpStdioClient  # noqa: E402
from runner import _json_of, required_tools  # noqa: E402

TASK_DIR = Path(__file__).parent / "tasks"

# The plan shape's whole toolbelt, as `bench/plan_cost.py` accounts for it: one
# operation library described inside one schema.
PLAN_TOOLS = ["apply_plan"]

# Queries that state what the caller wants *built*. Two of them describe
# `plan_cost.py`'s divider, two its decoupling bank — the only two designs this
# project has proven equivalent in both shapes, by the same ERC verdict on the
# same twelve symbols. No phrasing here names batching, planning or a call
# count, because a caller who knows to say that has already found the path.
GOAL_QUERIES = [
    "build a resistive voltage divider",
    "build a resistive voltage divider from two resistors on a new project, with "
    "supply rails, a wire between them and a labelled output",
    "add four decoupling capacitors on the 3V3 rail with their ground symbols",
    "place four capacitors and their power symbols in one step",
]

# Queries from a caller who already knows the mechanism exists and is looking
# for the tool that offers it. This is the generous case, not the realistic one.
MECHANISM_QUERIES = [
    "do the whole schematic change at once instead of one call at a time",
    "apply a batch of schematic edits as one operation",
    "compile a plan and run it",
]


def search(client: McpStdioClient, query: str, limit: int) -> list[str]:
    call = client.tools_call("find_capabilities", {"query": query, "limit": limit})
    payload = _json_of(call.result) or {}
    return [m["name"] for m in payload.get("matches", [])]


def measure(
    client: McpStdioClient, label: str, queries: list[str], needed: list[str], limit: int
) -> dict[str, Any]:
    """One shape's retrieval, scored exactly as `--load-mode search` scores it."""
    union: list[str] = []
    per_query = []
    for query in queries:
        names = search(client, query, limit)
        for name in names:
            if name not in union:
                union.append(name)
        per_query.append(
            {
                "query": query,
                "returned": len(names),
                # 1-based rank of each needed tool in *this* query's answer, or
                # null. The rank is the finding: a tool at rank 1 of a query
                # nobody would type is not reachable.
                "ranks": {n: (names.index(n) + 1 if n in names else None) for n in needed},
            }
        )
    hits = [n for n in needed if n in union]
    return {
        "shape": label,
        "queries": len(queries),
        "needed": needed,
        "union": len(union),
        "hits": len(hits),
        "missed": [n for n in needed if n not in union],
        "precision": round(len(hits) / len(union), 3) if union else 0.0,
        "recall": round(len(hits) / len(needed), 3) if needed else 1.0,
        "per_query": per_query,
    }


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--server", required=True)
    ap.add_argument("--config", default=str(Path(__file__).parent / "konnect.bench.toml"))
    ap.add_argument("--limit", type=int, default=8, help="top-k per query, as in --load-mode search")
    ap.add_argument(
        "--out", default=str(Path(__file__).parent / "results" / "latest-plan-retrieval.json")
    )
    args = ap.parse_args()

    task = yaml.safe_load((TASK_DIR / "01_sch_divider.yaml").read_text(encoding="utf-8"))

    env = dict(os.environ)
    env.setdefault("RUST_LOG", "warn")

    shapes = []
    with McpStdioClient([args.server, "--config", args.config], env=env) as client:
        client.initialize()
        shapes.append(
            measure(client, "direct", task["intents"], required_tools(task), args.limit)
        )
        shapes.append(measure(client, "plan-by-goal", GOAL_QUERIES, PLAN_TOOLS, args.limit))
        shapes.append(
            measure(client, "plan-by-mechanism", MECHANISM_QUERIES, PLAN_TOOLS, args.limit)
        )

    print(f"limit: {args.limit}\n")
    print(f"{'shape':<20} {'queries':>7} {'union':>6} {'needed':>7} {'prec':>7} {'recall':>7}")
    for s in shapes:
        print(
            f"{s['shape']:<20} {s['queries']:>7} {s['union']:>6} {len(s['needed']):>7} "
            f"{s['precision'] * 100:>6.1f}% {s['recall'] * 100:>6.1f}%"
        )

    for s in shapes:
        print(f"\n-- {s['shape']} --")
        for q in s["per_query"]:
            ranks = ", ".join(
                f"{name} rank {rank}" if rank else f"{name} NOT RETURNED"
                for name, rank in q["ranks"].items()
            )
            print(f"  {q['returned']:>2} back | {ranks}")
            print(f"     {q['query']!r}")
        if s["missed"]:
            print(f"  MISSED: {', '.join(s['missed'])}")

    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps({"limit": args.limit, "shapes": shapes}, indent=2), encoding="utf-8")
    print(f"\nwrote {out}")


if __name__ == "__main__":
    main()
