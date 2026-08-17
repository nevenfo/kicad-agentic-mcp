"""Where do the tokens and the milliseconds actually go?

Reads a runner result file and aggregates per tool. Aggregates at task level
hide the answer: the biggest single line item in the baseline is not a KiCad
operation at all, it is the discovery handshake.

Usage:
    python bench/analyze.py bench/results/baseline-tasks.json
"""

from __future__ import annotations

import collections
import json
import sys
from pathlib import Path


def main() -> None:
    path = Path(sys.argv[1])
    data = json.loads(path.read_text(encoding="utf-8"))
    runs = data["runs"]

    resp: collections.Counter[str] = collections.Counter()
    req: collections.Counter[str] = collections.Counter()
    calls: collections.Counter[str] = collections.Counter()
    ms: collections.Counter[str] = collections.Counter()

    for run in runs:
        for c in run["call_breakdown"]:
            tool = c["tool"] or "<unknown>"
            resp[tool] += c["resp_tokens"]
            req[tool] += c["req_tokens"]
            calls[tool] += 1
            ms[tool] += c["ms"]

    total_resp = sum(resp.values())
    n = len(runs)
    print(f"label: {data.get('label')}   runs: {n}")
    print(f"total response tokens: {total_resp}  ({total_resp / n:.0f} per run)\n")

    head = f"{'tool':<34}{'calls':>6}{'resp tk':>9}{'tk/call':>9}{'% resp':>8}{'ms/call':>9}"
    print(head)
    print("-" * len(head))
    for tool, value in resp.most_common(15):
        print(
            f"{tool:<34}{calls[tool]:>6}{value:>9}{value / calls[tool]:>9.0f}"
            f"{100 * value / total_resp:>7.1f}%{ms[tool] / calls[tool]:>9.0f}"
        )

    discovery = sum(resp[t] for t in ("list_toolboxes", "load_toolset", "unload_toolset"))
    print(
        f"\ndiscovery handshake (list_toolboxes + load_toolset): {discovery} tk "
        f"= {100 * discovery / total_resp:.1f}% of all response tokens "
        f"({discovery / n:.0f} tk per task)"
    )

    slowest = sorted(((ms[t] / calls[t], t) for t in calls), reverse=True)[:5]
    print("\nslowest tools (mean ms/call):")
    for avg, tool in slowest:
        print(f"  {avg:>8.0f}  {tool}")

    violations(runs)
    instability(runs)


def violations(runs: list[dict]) -> None:
    """Violations by kind, with the task each one came from.

    A count alone is not actionable: `safety` on one task and `max_calls` on
    another are different problems with different fixes, and a total hides
    which one moved.
    """
    by_kind: collections.Counter[str] = collections.Counter()
    by_kind_task: dict[str, collections.Counter[str]] = {}
    for run in runs:
        for v in run.get("violations", []):
            by_kind[v["kind"]] += 1
            by_kind_task.setdefault(v["kind"], collections.Counter())[run["task_id"]] += 1

    print("\nviolations by kind:")
    if not by_kind:
        print("  none")
        return
    for kind, n in by_kind.most_common():
        where = ", ".join(f"{t}×{c}" for t, c in by_kind_task[kind].most_common())
        print(f"  {kind:<18}{n:>4}   {where}")


def instability(runs: list[dict]) -> None:
    """Per-task disagreement between repeats of the same task.

    Signature = `(success, tuple(tools_used))`, the same one `bench/runner.py`
    scores on, so this reads the stored runs rather than recomputing a
    different definition of the same word.
    """
    by_task: dict[str, list[dict]] = {}
    for run in runs:
        by_task.setdefault(run["task_id"], []).append(run)

    measurable = {t: rs for t, rs in by_task.items() if len(rs) > 1}
    print("\ninstability per task:")
    if not measurable:
        print("  n/a — one run per task, nothing to disagree with")
        return
    for task_id, rs in measurable.items():
        sigs = collections.Counter(
            (r["success"], tuple(r.get("tools_used", []))) for r in rs
        )
        rate = 1.0 - sigs.most_common(1)[0][1] / len(rs)
        note = "" if len(sigs) == 1 else f"   {len(sigs)} distinct outcomes"
        print(f"  {task_id:<26}{rate:>6.0%}  ({len(rs)} runs){note}")


if __name__ == "__main__":
    main()
