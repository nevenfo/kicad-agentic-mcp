"""Dump each golden task's plain-language intents and required tools as JSON.

Diagnostic-only companion to `--load-mode search` in `runner.py`: instead of
running the server, this just extracts what `required_tools()` already
computes from each task file, so a Rust probe can call `capability_search`
directly, off any MCP wiring or fixture cost.

Usage:
    python bench/retrieval_intents.py [--out PATH]

Output (stdout, or PATH if --out is given):
    [{"task": "01_sch_divider", "intents": [...], "needed": [...]}, ...]
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

import yaml

sys.path.insert(0, str(Path(__file__).parent))

from runner import required_tools  # noqa: E402

TASK_DIR = Path(__file__).parent / "tasks"


def load_tasks() -> list[dict]:
    out = []
    for path in sorted(TASK_DIR.glob("*.yaml")):
        task = yaml.safe_load(path.read_text(encoding="utf-8"))
        out.append(
            {
                "task": path.stem,
                "intents": task.get("intents", []),
                "needed": required_tools(task),
            }
        )
    return out


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, default=None, help="write JSON here instead of stdout")
    args = parser.parse_args()

    data = load_tasks()
    text = json.dumps(data, indent=2)
    if args.out:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(text, encoding="utf-8")
    else:
        print(text)


if __name__ == "__main__":
    main()
