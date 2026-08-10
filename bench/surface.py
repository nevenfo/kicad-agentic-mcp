"""Measure the MCP surface cost of a server: how many tokens `tools/list`
costs at startup and how many it costs with every toolset loaded.

This is the single cheapest baseline metric we have — it needs no KiCad, no
project, and no LLM, and it is fully deterministic.

Usage:
    python bench/surface.py --server <path-to-binary> [--label konnect-baseline]
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

import tiktoken  # noqa: E402

from mcp_client import McpStdioClient  # noqa: E402

ENC = tiktoken.get_encoding("o200k_base")


def tokens(obj: object) -> int:
    text = obj if isinstance(obj, str) else json.dumps(obj, separators=(",", ":"))
    return len(ENC.encode(text))


def measure(server: str, config: str | None) -> dict:
    argv = [server]
    if config:
        argv += ["--config", config]

    env = dict(os.environ)
    env.setdefault("RUST_LOG", "warn")

    with McpStdioClient(argv, env=env) as c:
        init = c.initialize()
        if init.error:
            raise SystemExit(f"initialize failed: {init.error}")

        baseline = c.tools_list()
        baseline_tools = baseline.result["tools"]

        # Discover every toolset and load them all: the "full catalog" cost.
        boxes = c.tools_call("list_toolboxes")
        payload = json.loads(boxes.result["content"][0]["text"])
        names = [t["name"] for t in payload["toolsets"]]
        c.tools_call("load_toolset", {"name": names})

        full = c.tools_list()
        full_tools = full.result["tools"]

        return {
            "server_info": init.result.get("serverInfo", {}),
            "protocol_version": init.result.get("protocolVersion"),
            "toolset_count": len(names),
            "toolsets": names,
            "baseline": {
                "tool_count": len(baseline_tools),
                "wire_bytes": baseline.response_bytes,
                "tokens": tokens(baseline_tools),
                "tool_names": sorted(t["name"] for t in baseline_tools),
            },
            "full_catalog": {
                "tool_count": len(full_tools),
                "wire_bytes": full.response_bytes,
                "tokens": tokens(full_tools),
            },
            "per_tool_tokens": sorted(
                ({"name": t["name"], "tokens": tokens(t)} for t in full_tools),
                key=lambda r: -r["tokens"],
            ),
        }


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--server", required=True)
    ap.add_argument("--config", default=None)
    ap.add_argument("--label", default="unlabeled")
    ap.add_argument("--out", default=None)
    args = ap.parse_args()

    result = measure(args.server, args.config)
    result["label"] = args.label

    b, f = result["baseline"], result["full_catalog"]
    print(f"label            : {args.label}")
    print(f"server           : {result['server_info']}")
    print(f"protocol         : {result['protocol_version']}")
    print(f"toolsets         : {result['toolset_count']}")
    print(f"baseline tools   : {b['tool_count']:>5}  tokens={b['tokens']:>7}  wire={b['wire_bytes']:>8}B")
    print(f"full catalog     : {f['tool_count']:>5}  tokens={f['tokens']:>7}  wire={f['wire_bytes']:>8}B")
    print(f"disclosure ratio : {b['tokens'] / f['tokens']:.3f}  (baseline / full)")
    print("top 10 heaviest tools:")
    for row in result["per_tool_tokens"][:10]:
        print(f"  {row['tokens']:>5}  {row['name']}")

    if args.out:
        Path(args.out).parent.mkdir(parents=True, exist_ok=True)
        Path(args.out).write_text(json.dumps(result, indent=2), encoding="utf-8")
        print(f"\nwrote {args.out}")


if __name__ == "__main__":
    main()
