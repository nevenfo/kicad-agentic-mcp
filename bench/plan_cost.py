"""What a plan costs against the batch it replaces, on the same design.

The golden suite measures the server; this measures one specific claim — that
describing a change once is cheaper than enumerating it — and it measures it on
the divider from `bench/tasks/01_sch_divider.yaml`, built twice by a fresh
server each time and checked against KiCAD's own ERC both times.

Both shapes are given the *same pre-snapped coordinates*, so the difference
reported here is structural and nothing else. The plan would also accept the
round numbers a person would type — 100.0, 80.0 — and snap them; the batch
would write them verbatim and fail ERC six times (E6). That is a real advantage
and it is deliberately kept out of the numbers below, because it would make the
comparison flatter to the plan than the structure alone deserves.

Two scenarios, because they answer different halves of the claim. The divider
compresses the *shape* of a change; every coordinate in it is data the caller
chose, so the request barely moves and the saving is in schemas and reply. The
capacitor bank is where a macro earns its keep: one operation instead of nine
calls, and eight power symbol positions the caller never writes down.

The bank is a fragment, not a finished design: a rail with no source is not
driven, so both shapes report ERC 2. That is the point — they report the *same*
2, on the same twelve symbols, which is what makes the token comparison
like for like. A mismatch between the two would void the run and says so.

Usage:
    python bench/plan_cost.py --server .\\target\\release\\konnect.exe
"""

from __future__ import annotations

import argparse
import json
import os
import statistics
import sys
import tempfile
from pathlib import Path
from typing import Any

sys.path.insert(0, str(Path(__file__).parent))

import tiktoken  # noqa: E402

from mcp_client import McpStdioClient  # noqa: E402

ENC = tiktoken.get_encoding("o200k_base")


def tokens(text: str) -> int:
    return len(ENC.encode(text))


def text_of(result: Any) -> str:
    if not isinstance(result, dict):
        return ""
    return "\n".join(p.get("text", "") for p in result.get("content", []) if p.get("type") == "text")


# ── the same divider, described two ways ─────────────────────────────────────


def batch_calls(work: str, sch: str) -> list[dict[str, Any]]:
    """The golden task's own call sequence, coordinates already on the grid."""
    return [
        {"tool": "create_project", "args": {"path": work, "name": "pc"}},
        {
            "tool": "batch_place_components",
            "args": {
                "schematic": sch,
                "components": [
                    {"lib_id": "Device:R", "reference": "R1", "value": "10k", "x": 100.33, "y": 80.01},
                    {"lib_id": "Device:R", "reference": "R2", "value": "10k", "x": 100.33, "y": 95.25},
                    {"lib_id": "power:PWR_FLAG", "reference": "#FLG01", "x": 100.33, "y": 76.2},
                    {"lib_id": "power:PWR_FLAG", "reference": "#FLG02", "x": 100.33, "y": 99.06},
                ],
            },
        },
        {"tool": "add_power_symbol", "args": {"schematic": sch, "power_net": "+3V3", "x": 100.33, "y": 76.2}},
        {"tool": "add_power_symbol", "args": {"schematic": sch, "power_net": "GND", "x": 100.33, "y": 99.06}},
        {"tool": "connect_pins", "args": {"schematic": sch, "ref1": "R1", "pin1": "2", "ref2": "R2", "pin2": "1"}},
        {"tool": "add_schematic_net_label", "args": {"schematic": sch, "net": "VOUT", "x": 100.33, "y": 87.63}},
    ]


def plan_document(work: str, sch: str) -> dict[str, Any]:
    return {
        "plan_id": "divider",
        "ops": [
            {"op": "call", "with": {"tool": "create_project", "args": {"path": work, "name": "pc"}}},
            {
                "op": "place",
                "with": {
                    "schematic": sch,
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
                    "schematic": sch,
                    "symbols": [
                        {"net": "+3V3", "x": 100.33, "y": 76.2},
                        {"net": "GND", "x": 100.33, "y": 99.06},
                    ],
                },
            },
            {"op": "connect", "with": {"schematic": sch, "connections": [{"from": "R1.2", "to": "R2.1"}]}},
            {"op": "label", "with": {"schematic": sch, "labels": [{"net": "VOUT", "x": 100.33, "y": 87.63}]}},
        ],
    }


# ── scenario 2: a decoupling bank, where one operation replaces 3N calls ─────
#
# The divider compresses the *shape* of a change and barely touches the request
# payload, because every coordinate in it is data the caller chose. A macro is
# where the request shrinks: the caller says which capacitors go on which rail
# and the operation works out eight power symbols and their positions.

CAPS = [
    {"reference": f"C{10 + i}", "value": "100nF"} for i in range(4)
]
BANK_AT = {"x": 120.65, "y": 80.01}
BANK_PITCH = 7.62
BANK_RAIL = "+3V3"


def bank_batch_calls(work: str, sch: str) -> list[dict[str, Any]]:
    """The same bank, enumerated: one placement call and two symbols per cap."""
    placed = []
    symbols = []
    for i, cap in enumerate(CAPS):
        x = round(BANK_AT["x"] + BANK_PITCH * i, 2)
        placed.append({"lib_id": "Device:C", "reference": cap["reference"], "value": cap["value"], "x": x, "y": BANK_AT["y"]})
        symbols.append({"schematic": sch, "power_net": BANK_RAIL, "x": x, "y": round(BANK_AT["y"] - 3.81, 2)})
        symbols.append({"schematic": sch, "power_net": "GND", "x": x, "y": round(BANK_AT["y"] + 3.81, 2)})
    return [
        {"tool": "create_project", "args": {"path": work, "name": "pc"}},
        {"tool": "batch_place_components", "args": {"schematic": sch, "components": placed}},
        *({"tool": "add_power_symbol", "args": s} for s in symbols),
    ]


def bank_plan(work: str, sch: str) -> dict[str, Any]:
    return {
        "plan_id": "bank",
        "ops": [
            {"op": "call", "with": {"tool": "create_project", "args": {"path": work, "name": "pc"}}},
            {
                "op": "decouple",
                "with": {
                    "schematic": sch,
                    "at": BANK_AT,
                    "pitch": BANK_PITCH,
                    "caps": [{**cap, "rail": BANK_RAIL} for cap in CAPS],
                },
            },
        ],
    }


# The schemas a caller has to hold to write each shape. This is the half of the
# cost that is easy to forget: a batch cannot be written without the argument
# schema of every tool in it.
SCHEMAS = {
    ("divider", "batch"): [
        "create_project",
        "batch_place_components",
        "add_power_symbol",
        "connect_pins",
        "add_schematic_net_label",
    ],
    ("divider", "plan"): ["apply_plan"],
    ("bank", "batch"): ["create_project", "batch_place_components", "add_power_symbol"],
    ("bank", "plan"): ["apply_plan"],
}


def run(server: str, config: str, scenario: str, shape: str) -> dict[str, Any]:
    work = Path(tempfile.mkdtemp(prefix=f"kam-plancost-{scenario}-{shape}-"))
    posix = str(work).replace("\\", "/")
    sch = f"{posix}/pc.kicad_sch"

    env = dict(os.environ)
    env.setdefault("RUST_LOG", "warn")

    with McpStdioClient([server, "--config", config], env=env) as c:
        c.initialize()
        c.tools_list()
        baseline = len(c.session.calls)

        c.tools_call("kicad_describe", {"names": SCHEMAS[(scenario, shape)]})

        document = plan_document if scenario == "divider" else bank_plan
        enumerated = batch_calls if scenario == "divider" else bank_batch_calls
        if shape == "plan":
            payload = {
                "verify": "auto",
                "calls": [{"tool": "apply_plan", "args": {"plan": document(posix, sch)}}],
            }
        else:
            payload = {"verify": "auto", "calls": enumerated(posix, sch)}

        applied = c.tools_call("kicad_invoke", payload)
        body = json.loads(text_of(applied.result))

        # Only what the caller pays for the change itself: the handshake and the
        # startup catalogue are identical in both shapes and would only dilute
        # the comparison.
        work_calls = c.session.calls[baseline:]
        request_tokens = sum(tokens(json.dumps(call.params, separators=(",", ":"))) for call in work_calls)
        response_tokens = sum(tokens(json.dumps(call.result, separators=(",", ":")) or "") for call in work_calls)
        wall = sum(call.duration_ms for call in work_calls)
        # Attributed, because "48% cheaper" is not a claim until it says which
        # half paid: the schemas the caller has to hold, or the reply it reads.
        breakdown = {
            call.params.get("name", call.method): {
                "request": tokens(json.dumps(call.params, separators=(",", ":"))),
                "response": tokens(json.dumps(call.result, separators=(",", ":")) or ""),
            }
            for call in work_calls
        }

    erc = next((v for v in body.get("validators", []) if v.get("check") == "erc"), {})
    return {
        "scenario": scenario,
        "shape": shape,
        "mcp_calls": sum(1 for call in work_calls if call.method == "tools/call"),
        "request_tokens": request_tokens,
        "response_tokens": response_tokens,
        "external_tokens": request_tokens + response_tokens,
        "wall_ms": wall,
        "breakdown": breakdown,
        "erc_errors": erc.get("errors"),
        "ok": body.get("ok"),
        "diff": (body.get("diff") or {}).get("summary"),
        "work": str(work),
    }


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--server", required=True)
    ap.add_argument("--config", default=str(Path(__file__).parent / "konnect.bench.toml"))
    ap.add_argument("--repeat", type=int, default=3)
    ap.add_argument("--out", default=str(Path(__file__).parent / "results" / "latest-plan-cost.json"))
    args = ap.parse_args()

    results: dict[str, dict[str, list[dict[str, Any]]]] = {}
    for scenario in ("divider", "bank"):
        results[scenario] = {
            shape: [run(args.server, args.config, scenario, shape) for _ in range(args.repeat)]
            for shape in ("batch", "plan")
        }

    for scenario, shapes in results.items():
        print(f"\n=== {scenario} ===")
        print(f"{'':<20}{'batch':>12}{'plan':>12}{'delta':>12}")
        print("-" * 56)
        for metric in ("mcp_calls", "request_tokens", "response_tokens", "external_tokens", "wall_ms"):
            a = statistics.median(r[metric] for r in shapes["batch"])
            b = statistics.median(r[metric] for r in shapes["plan"])
            pct = f"{(b - a) / a * 100:+.1f}%" if a else "—"
            print(f"{metric:<20}{a:>12.0f}{b:>12.0f}{pct:>12}")

        print("  where it goes (first run):")
        for shape in ("batch", "plan"):
            for name, cost in shapes[shape][0]["breakdown"].items():
                print(f"    {shape:<6} {name:<16} req {cost['request']:>5}  resp {cost['response']:>5}")

        for shape in ("batch", "plan"):
            first = shapes[shape][0]
            print(f"  {shape}: ERC errors={first['erc_errors']}  diff={first['diff']!r}")

        errs = {shape: shapes[shape][0]["erc_errors"] for shape in ("batch", "plan")}
        if errs["batch"] != errs["plan"]:
            print(f"  WARNING: the two shapes did not build the same design ({errs}); the comparison is void")

    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(results, indent=2), encoding="utf-8")
    print(f"\nwritten to {out}")


if __name__ == "__main__":
    main()
