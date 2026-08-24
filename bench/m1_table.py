"""M.1 — the three modes side by side, from committed artefacts only.

Baseline, Direct and Agent are three different ways to get the same design
built, and they are *not* three settings of one harness:

* **Baseline** — upstream Konnect, driven by `bench/runner.py`'s scripted
  oracle. Every tool call is written in the task file.
* **Direct** — this fork through `kicad_describe` / `kicad_invoke`, same
  oracle, same task files. The comparison with Baseline therefore isolates the
  server surface and nothing else.
* **Agent** — this fork's local runtime (H.7): the caller states an objective,
  a local model writes the Plan IR, the server compiles, applies and verifies
  it, and `kicad-cli` returns the verdict (INV1). No oracle, no external model.

This script runs nothing and spends nothing. It reads the result files that are
committed under `bench/results/` and prints the tables `docs/benchmark.md`
carries, so the document can be regenerated from evidence rather than retyped.

Usage:
    python bench/m1_table.py
"""

from __future__ import annotations

import argparse
import collections
import json
import statistics
from pathlib import Path
from typing import Any

RESULTS = Path(__file__).parent / "results"

# Which artefact speaks for which mode. Named files, never globs: a table that
# silently picks up tomorrow's re-run is not reproducible.
# All three columns are measured on the same machine on the same day: comparing
# a baseline taken two weeks ago against a fork taken today would let machine
# state into a table that claims to be about servers. The Phase F artefacts
# (`baseline-tasks.json`, `fork-gateway-tasks.json`) stay where they are and
# still back the surface sections above.
BASELINE = "m1-baseline-r5.json"
DIRECT = "m1-gateway-r5.json"
AGENT = {
    "sch_divider": "agent-e2e-gpt-oss-20b-medium-m1-divider.json",
    "sch_ldo": "agent-e2e-gpt-oss-20b-medium-m1-ldo.json",
}

# The Agent runtime is exercised by `bench/model_tasks/`, whose files state an
# objective; the oracle suite is `bench/tasks/`, whose files script tool calls.
# They are the same two designs, written for two harnesses that cannot share a
# task file — the same split K.1.3 made for the external-agent harness.
AGENT_TASK_OF = {"sch_divider": "model_divider", "sch_ldo": "model_ldo"}

# A fourth way to reach the same server, measured in K.1 and kept in its own
# table: an *external* agent — a commercial CLI with its own model — driving
# Konnect over MCP. It answers a different question from the three modes above
# (what a frontier model does with this surface, not what the surface costs),
# so folding it into their table would compare a scripted oracle with a model's
# own judgement and read the difference as a server property.
EXTERNAL = {
    "codex-cli (codex)": "k11-codex.json",
    "claude-sonnet-5": "k11-claude-sonnet5.json",
    "claude-opus-5 (anchor, 1 task)": "k11-claude-opus5-anchor-r3.json",
}


def load(name: str) -> dict[str, Any]:
    return json.loads((RESULTS / name).read_text(encoding="utf-8"))


def oracle_rows(name: str) -> dict[str, list[dict[str, Any]]]:
    by_task: dict[str, list[dict[str, Any]]] = collections.defaultdict(list)
    for run in load(name)["runs"]:
        by_task[run["task_id"]].append(run)
    return by_task


def median(values: list[float]) -> float:
    return statistics.median(values)


def oracle_cell(runs: list[dict[str, Any]], key: str) -> float:
    """Median across runs — the convention `bench/runner.py` prints and the one
    `docs/benchmark.md` has always quoted. Mean would let one slow spawn on
    `manufacturing_exports` speak for a whole column."""
    return median([r[key] for r in runs])


def agent_summary(name: str) -> dict[str, Any]:
    data = load(name)
    surface = data["surface"]
    attempts = data["attempts"]
    last = attempts[-1]
    verification = last.get("verification") or {}
    # Summed over attempts, not read off the last one: a design that took four
    # tries cost four prompts, and the caller's machine ran all of them.
    def total(field: str) -> int | None:
        values = [(a.get("usage") or {}).get(field) for a in attempts]
        return sum(v for v in values if v is not None) if any(v is not None for v in values) else None

    return {
        "task": data["task"],
        "model": data["model"],
        "attempts": len(attempts),
        "status": last.get("status"),
        "verdict": verification.get("verdict"),
        "verdict_source": verification.get("source"),
        "erc_errors": verification.get("errors"),
        "erc_warnings": verification.get("warnings"),
        "mcp_calls": surface["mcp_calls"],
        "response_tokens": surface["response_tokens"],
        "catalog_tokens": surface["catalog_tokens"],
        "external_tokens": surface["external_tokens"],
        "setup_tokens": surface["setup_tokens"],
        "local_calls": data["local_calls"],
        "external_calls": data["external_calls"],
        "prompt_tokens": total("prompt_tokens"),
        "completion_tokens": total("completion_tokens"),
        "reasoning_tokens": total("reasoning_tokens"),
        "wall_clock_ms": sum(a.get("wall_clock_ms") or 0.0 for a in attempts) or None,
    }


def fmt(value: Any) -> str:
    if isinstance(value, float):
        value = round(value)
    if isinstance(value, int):
        return f"{value:,}".replace(",", " ")
    return "—" if value is None else str(value)


def three_modes(base: dict, direct: dict, agents: dict[str, dict]) -> list[str]:
    tasks = [t for t in AGENT if t in base and t in direct]
    out = [
        "| Metric | Baseline (oracle) | Direct (oracle) | Agent (local model) |",
        "|---|---|---|---|",
    ]

    def row(label: str, b: Any, d: Any, a: Any) -> None:
        out.append(f"| {label} | {fmt(b)} | {fmt(d)} | {fmt(a)} |")

    b_runs = [r for t in tasks for r in base[t]]
    d_runs = [r for t in tasks for r in direct[t]]
    a_runs = [agents[t] for t in tasks]

    row("designs covered", len(tasks), len(tasks), len(tasks))
    row(
        "runs / attempts",
        len(b_runs),
        len(d_runs),
        sum(a["attempts"] for a in a_runs),
    )
    row("who writes the calls", "task file", "task file", "local model")
    row(
        "success",
        f"{sum(1 for r in b_runs if r['success'])}/{len(b_runs)}",
        f"{sum(1 for r in d_runs if r['success'])}/{len(d_runs)}",
        f"{sum(1 for a in a_runs if a['status'] == 'SUCCESS')}/{len(a_runs)}",
    )
    row(
        "verdict source",
        "assertions + kicad-cli",
        "assertions + kicad-cli",
        "kicad-cli (INV1)",
    )
    row(
        "MCP calls / design",
        oracle_cell(b_runs, "mcp_calls"),
        oracle_cell(d_runs, "mcp_calls"),
        median([a["mcp_calls"] for a in a_runs]),
    )
    # An oracle run has exactly one attempt, so its per-attempt cost *is* its
    # per-design cost. Agent mode retries, and a retry is paid in full: keeping
    # both rows is what stops a 4-attempt design from reading as an expensive
    # round trip rather than as three extra ones.
    row(
        "MCP calls / attempt",
        oracle_cell(b_runs, "mcp_calls"),
        oracle_cell(d_runs, "mcp_calls"),
        median([a["mcp_calls"] / a["attempts"] for a in a_runs]),
    )
    row(
        "RESPONSE_TOKENS / design",
        oracle_cell(b_runs, "response_tokens"),
        oracle_cell(d_runs, "response_tokens"),
        median([a["response_tokens"] for a in a_runs]),
    )
    row(
        "CATALOG_TOKENS / design",
        oracle_cell(b_runs, "catalog_tokens"),
        oracle_cell(d_runs, "catalog_tokens"),
        median([a["catalog_tokens"] for a in a_runs]),
    )
    row(
        "**EXTERNAL_TOKENS / design**",
        oracle_cell(b_runs, "response_tokens") + oracle_cell(b_runs, "catalog_tokens"),
        oracle_cell(d_runs, "response_tokens") + oracle_cell(d_runs, "catalog_tokens"),
        median([a["external_tokens"] for a in a_runs]),
    )
    row(
        "EXTERNAL_TOKENS / attempt",
        oracle_cell(b_runs, "response_tokens") + oracle_cell(b_runs, "catalog_tokens"),
        oracle_cell(d_runs, "response_tokens") + oracle_cell(d_runs, "catalog_tokens"),
        median([a["external_tokens"] / a["attempts"] for a in a_runs]),
    )
    # Agent mode's clock is not the other two's: it contains a 20B model
    # thinking. Reported in its own unit rather than folded into a millisecond
    # column that would then be read as server latency.
    row(
        "wall clock, median",
        f"{oracle_cell(b_runs, 'wall_clock_ms'):.0f} ms",
        f"{oracle_cell(d_runs, 'wall_clock_ms'):.0f} ms",
        f"{median([a['wall_clock_ms'] / 1000.0 for a in a_runs]):.0f} s (local inference)",
    )
    row("external model calls", 0, 0, sum(a["external_calls"] for a in a_runs))
    row("local model calls", 0, 0, sum(a["local_calls"] for a in a_runs))
    return out


def agent_detail(agents: dict[str, dict]) -> list[str]:
    out = [
        "| Design | attempts | verdict | ERC err/warn | MCP calls | EXTERNAL_TOKENS | local prompt tk | local completion tk | reasoning tk | wall clock |",
        "|---|---|---|---|---|---|---|---|---|---|",
    ]
    for task, a in agents.items():
        wall = a["wall_clock_ms"]
        out.append(
            f"| `{AGENT_TASK_OF[task]}` | {a['attempts']} | {a['verdict']} ({a['verdict_source']}) | "
            f"{a['erc_errors']}/{a['erc_warnings']} | {a['mcp_calls']} | {fmt(a['external_tokens'])} | "
            f"{fmt(a['prompt_tokens'])} | {fmt(a['completion_tokens'])} | {fmt(a['reasoning_tokens'])} | "
            f"{fmt(wall / 1000.0 if wall else None)} s |"
        )
    return out


def oracle_suite(base: dict, direct: dict) -> list[str]:
    shared = sorted(set(base) & set(direct))
    out = [
        "| Task | Baseline calls | Direct calls | Baseline EXTERNAL_TOKENS | Direct EXTERNAL_TOKENS | Baseline ms | Direct ms |",
        "|---|---|---|---|---|---|---|",
    ]
    for task in shared:
        b, d = base[task], direct[task]
        out.append(
            f"| `{task}` | {fmt(oracle_cell(b, 'mcp_calls'))} | {fmt(oracle_cell(d, 'mcp_calls'))} | "
            f"{fmt(oracle_cell(b, 'response_tokens') + oracle_cell(b, 'catalog_tokens'))} | "
            f"{fmt(oracle_cell(d, 'response_tokens') + oracle_cell(d, 'catalog_tokens'))} | "
            f"{fmt(oracle_cell(b, 'wall_clock_ms'))} | {fmt(oracle_cell(d, 'wall_clock_ms'))} |"
        )
    b_runs = [r for t in shared for r in base[t]]
    d_runs = [r for t in shared for r in direct[t]]
    out.append(
        f"| **median** | **{fmt(oracle_cell(b_runs, 'mcp_calls'))}** | **{fmt(oracle_cell(d_runs, 'mcp_calls'))}** | "
        f"**{fmt(oracle_cell(b_runs, 'response_tokens') + oracle_cell(b_runs, 'catalog_tokens'))}** | "
        f"**{fmt(oracle_cell(d_runs, 'response_tokens') + oracle_cell(d_runs, 'catalog_tokens'))}** | "
        f"**{fmt(oracle_cell(b_runs, 'wall_clock_ms'))}** | **{fmt(oracle_cell(d_runs, 'wall_clock_ms'))}** |"
    )
    return out


def external_agents() -> list[str]:
    """K.1's campaigns. `aborted` is the harness's own void classification
    (K.1.13/K.1.18): a run it cut short is not a measurement, so it leaves
    every rate and is reported as a count of its own."""
    out = [
        "| Harness | runs | void | DESIGN_PASS | strict SUCCESS | off-server calls | never reached Konnect | safety | median calls | cost |",
        "|---|---|---|---|---|---|---|---|---|---|",
    ]
    for label, name in EXTERNAL.items():
        runs = load(name)["runs"]
        scored = [r for r in runs if not r.get("aborted")]
        cost = sum(r["cost_usd"] or 0.0 for r in scored)
        out.append(
            f"| {label} | {len(runs)} | {len(runs) - len(scored)} | "
            f"{sum(1 for r in scored if r['design_success'])}/{len(scored)} | "
            f"{sum(1 for r in scored if r['success'])}/{len(scored)} | "
            f"{sum(r['off_server_calls'] for r in scored)} | "
            f"{sum(1 for r in scored if not r['tool_call_sequence'])}/{len(scored)} | "
            f"{sum(r['safety_violations'] for r in scored)} | "
            f"{fmt(median([r['tool_calls'] for r in scored]))} | "
            + (f"${cost:.4f} |" if cost else "not reported |")
        )
    return out


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.parse_args()

    base = oracle_rows(BASELINE)
    direct = oracle_rows(DIRECT)
    agents = {task: agent_summary(name) for task, name in AGENT.items()}

    print("### The three modes, on the designs all three have built\n")
    print("\n".join(three_modes(base, direct, agents)))
    print("\n### Agent mode, per design\n")
    print("\n".join(agent_detail(agents)))
    print("\n### Baseline vs Direct, whole oracle suite\n")
    print("\n".join(oracle_suite(base, direct)))
    print("\n### External agents driving the same server (K.1)\n")
    print("\n".join(external_agents()))
    print(
        "\nCoverage: Agent mode is measured on "
        f"{len(agents)} of the {len(set(base) & set(direct))} designs the oracle suite covers. "
        "The rest are not claimed."
    )


if __name__ == "__main__":
    main()
