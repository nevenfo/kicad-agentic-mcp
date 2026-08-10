"""Ad-hoc probe: run an arbitrary call sequence against the server and print the
raw results. Used to discover real tool behaviour before freezing it into a
benchmark task, so task specs are written from measured reality rather than
from a guess at the schema's intent.

Usage:
    python bench/probe.py --server <binary> --script bench/probes/<name>.yaml
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

import yaml  # noqa: E402

from mcp_client import McpStdioClient  # noqa: E402


def text_of(result: object) -> str:
    if not isinstance(result, dict):
        return ""
    return "\n".join(p.get("text", "") for p in result.get("content", []) if p.get("type") == "text")


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--server", required=True)
    ap.add_argument("--config", default=str(Path(__file__).parent / "konnect.bench.toml"))
    ap.add_argument("--script", required=True)
    ap.add_argument("--max-chars", type=int, default=1500)
    args = ap.parse_args()

    script = yaml.safe_load(Path(args.script).read_text(encoding="utf-8"))
    work = Path(tempfile.mkdtemp(prefix="kam-probe-"))
    name = script.get("project_name", "probe")
    # `create_project` writes <path>/<name>.kicad_* directly — it does not
    # create a <name>/ subdirectory. Verified, not assumed.
    posix_work = str(work).replace("\\", "/")
    env_vars = {
        "WORK": posix_work,
        "NAME": name,
        "SCH": f"{posix_work}/{name}.kicad_sch",
        "PCB": f"{posix_work}/{name}.kicad_pcb",
    }

    def sub(v: object) -> object:
        if isinstance(v, str):
            for k, val in env_vars.items():
                v = v.replace(f"${k}", val)
            return v
        if isinstance(v, list):
            return [sub(x) for x in v]
        if isinstance(v, dict):
            return {k: sub(x) for k, x in v.items()}
        return v

    proc_env = dict(os.environ)
    proc_env.setdefault("RUST_LOG", "warn")

    print(f"work dir: {work}\n")
    with McpStdioClient([args.server, "--config", args.config], env=proc_env) as c:
        c.initialize()
        c.tools_call("load_toolset", {"name": script["toolsets"]})
        for i, step in enumerate(script["steps"]):
            call = c.tools_call(step["tool"], sub(step.get("args", {})))
            body = text_of(call.result)
            flag = "ERR " if (call.error or (call.result or {}).get("isError")) else "ok  "
            print(f"[{i:>2}] {flag}{step['tool']}  ({call.duration_ms:.0f} ms)")
            print("      " + body[: args.max_chars].replace("\n", "\n      "))
            print()

    print(f"\nartifacts kept at: {work}")


if __name__ == "__main__":
    main()
