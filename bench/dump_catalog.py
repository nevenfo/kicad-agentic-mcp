"""Dump the full tool catalog (all toolsets loaded) to JSON on disk.

Kept out of the benchmark path on purpose: this is a developer aid for grepping
schemas without paying for them in an agent's context window.

Usage:
    python bench/dump_catalog.py --server <binary> --out bench/results/catalog.json
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

from mcp_client import McpStdioClient  # noqa: E402


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--server", required=True)
    ap.add_argument("--out", default="bench/results/catalog.json")
    args = ap.parse_args()

    env = dict(os.environ)
    env.setdefault("RUST_LOG", "warn")

    with McpStdioClient([args.server], env=env) as c:
        c.initialize()
        boxes = c.tools_call("list_toolboxes")
        names = [t["name"] for t in json.loads(boxes.result["content"][0]["text"])["toolsets"]]
        c.tools_call("load_toolset", {"name": names})
        tools = c.tools_list().result["tools"]

    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps({"count": len(tools), "tools": tools}, indent=2), encoding="utf-8")
    print(f"{len(tools)} tools -> {out}")


if __name__ == "__main__":
    main()
