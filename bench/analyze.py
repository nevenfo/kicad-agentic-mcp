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


if __name__ == "__main__":
    main()
