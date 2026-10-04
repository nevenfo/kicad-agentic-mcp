"""Summarise a `run.py` result file as a Markdown matrix, one row per cell.

    python bench/pcb_live_v2/report.py bench/results/pcb_live_v2-<...>.json
"""

from __future__ import annotations

import json
import statistics
import sys
from collections import defaultdict
from pathlib import Path


def main() -> int:
    data = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
    env = data["environment"]
    print(f"KiCad {env['kicad']} · fork `{env['fork_sha'][:8]}` · upstream {env['upstream_tag']} "
          f"`{env['upstream_sha'][:8]}` · {env['started']}\n")
    cells: dict[tuple[str, str], list[dict]] = defaultdict(list)
    for r in data["results"]:
        cells[(r["scenario"], r["subject"])].append(r)
    print("| scénario | sujet | runs | fonctionnel | succès MCP | faux succès | refus | absent "
          "| restart éditeur | erreur harness | appels MCP | s (médiane) |")
    print("|---|---|---|---|---|---|---|---|---|---|---|---|")
    for (scn, subj), rs in sorted(cells.items()):
        n = len(rs)

        def count(key: str) -> str:
            return f"{sum(1 for r in rs if r.get(key))}/{n}"
        calls = sorted({r.get("mcp_tool_calls") for r in rs if r.get("mcp_tool_calls") is not None})
        secs = statistics.median(r["seconds"] for r in rs)
        print(f"| {scn} | {subj} | {n} | {count('functional')} | {count('mcp_success')} "
              f"| {count('false_success')} | {count('refused')} | {count('capability_absent')} "
              f"| {count('gui_intervention')} | {count('harness_error')} "
              f"| {','.join(map(str, calls)) or '-'} | {secs:.1f} |")
    return 0


if __name__ == "__main__":
    sys.exit(main())
