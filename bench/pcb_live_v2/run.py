"""PCB live benchmark V2: same intention, two servers, KiCad as referee.

Each run starts pcbnew on a fresh copy of a fixture, lets the server under test
carry out one intention by whatever calls it needs, then judges the *final
KiCad state* — live read-back over `kipy`, saved file through `kicad-cli` —
never the server's JSON. A `success` the referee does not confirm is counted as
a false success.

Every oracle ships with controls: the oracle side injects the correct result
(must pass) and the known-wrong result (must fail). A scenario whose controls
do not separate is reported invalid rather than scored.

Run with the bench venv (kicad-python 0.8.0):
    ../_bench-venv/Scripts/python.exe bench/pcb_live_v2/run.py --scenario B --runs 3
"""

from __future__ import annotations

import argparse
import json
import platform
import shutil
import subprocess
import sys
import tempfile
import time
from datetime import datetime, timezone
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(REPO / "bench"))

import kicad_session as ks  # noqa: E402
from mcp_client import McpStdioClient  # noqa: E402

FIXTURES = HERE / "fixtures"
UPSTREAM_TAG = "v0.13.0"
IMPLS = {
    "fork": REPO / "target" / "release" / "konnect.exe",
    "upstream": REPO.parent / "_upstream-target" / "release" / "konnect.exe",
}
SOURCES = {"fork": REPO, "upstream": REPO.parent / "_upstream-v0.13.0"}


def git_sha(path: Path) -> str:
    return subprocess.run(["git", "-C", str(path), "rev-parse", "HEAD"],
                          capture_output=True, text=True).stdout.strip()


# ── MCP side ─────────────────────────────────────────────────────────────

class Server:
    """One MCP server process, with the counts the metrics need."""

    def __init__(self, impl: str):
        env = dict(__import__("os").environ, KICAD_API_SOCKET=ks.SOCKET)
        self.client = McpStdioClient([str(IMPLS[impl])], env=env)

    def __enter__(self) -> "Server":
        self.client.__enter__()
        self.client.initialize()
        return self

    def __exit__(self, *exc: object) -> None:
        self.client.__exit__(*exc)

    def call(self, name: str, args: dict) -> dict:
        c = self.client.tools_call(name, args)
        res = c.result or {}
        text = "".join(p.get("text", "") for p in res.get("content", []) if p.get("type") == "text")
        try:
            body = json.loads(text) if text else None
        except json.JSONDecodeError:
            body = None
        ok = c.error is None and not res.get("isError", False)
        return {"tool": name, "ok": ok, "ms": round(c.duration_ms, 1),
                "body": body if body is not None else text[:800], "rpc_error": c.error}

    @property
    def tool_calls(self) -> int:
        return self.client.session.mcp_calls

    @property
    def response_bytes(self) -> int:
        return self.client.session.total_response_bytes


# ── geometry helpers for oracles ─────────────────────────────────────────

def _near(a: tuple[float, float], b: tuple[float, float], tol: float) -> bool:
    return abs(a[0] - b[0]) <= tol and abs(a[1] - b[1]) <= tol


def copper_joins(tracks: list[dict], net: str, a: tuple[float, float], b: tuple[float, float],
                 tol: float = 0.05) -> bool:
    """True when the net's track endpoints form a chain from point a to point b."""
    segs = [(t["start"], t["end"]) for t in tracks if t["net"] == net]
    reached = [a]
    frontier = [a]
    while frontier:
        p = frontier.pop()
        for s, e in segs:
            for x, y in ((s, e), (e, s)):
                if _near(x, p, tol) and not any(_near(y, r, tol) for r in reached):
                    reached.append(y)
                    frontier.append(y)
    return any(_near(r, b, tol) for r in reached)


# ── scenario B — live ≠ saved ────────────────────────────────────────────
#
# R2 is moved in the live board (by the oracle side, standing in for a GUI
# user) and NOT saved. The intention is "route GND from R1 pad 2 to R2 pad 1".
# Correct: copper ends on the pad where KiCad holds it now. Wrong: copper ends
# where the saved file still says it is.

B_MOVE = (110.0, 60.0)


def b_prepare(s: ks.PcbnewSession) -> dict:
    s.move_footprint_live("R2", *B_MOVE)
    p1, p2 = s.live_pad("R1", "2"), s.live_pad("R2", "1")
    return {"from": (p1.x, p1.y), "to": (p2.x, p2.y), "stale_to": (109.5, 50.0)}


def b_act(srv: Server, board: Path) -> list[dict]:
    return [
        srv.call("load_toolset", {"name": "pcb_routing"}),
        srv.call("route_pad_to_pad", {"board": str(board), "net_name": "GND",
                                      "ref1": "R1", "pad1": "2", "ref2": "R2", "pad2": "1",
                                      "layer": "F.Cu", "width": 0.25}),
    ]


def b_judge(s: ks.PcbnewSession, ctx: dict, board: Path, work: Path) -> dict:
    tracks = s.live_tracks()
    live_ok = copper_joins(tracks, "GND", ctx["from"], ctx["to"])
    stale_hit = copper_joins(tracks, "GND", ctx["from"], ctx["stale_to"])
    # Live: footprint still where the user left it (the server must not undo it).
    fp = s.footprint("R2")
    fp_kept = _near((ks.mm(fp.position.x), ks.mm(fp.position.y)), B_MOVE, 1e-3)
    # Saved: the user saves; KiCad's own checker then judges the file.
    s.save()
    d = ks.drc(board, work / "drc.json")
    saved_ok = d["unconnected"] == 1 and d["violations"].get("track_dangling", 0) == 0 \
        and d["violations"].get("shorting_items", 0) == 0
    # `stale_hit` is diagnostic only: an L-bend toward the live pad may pass
    # through the stale point and still be correct.
    return {"pass": live_ok and fp_kept and saved_ok,
            "live_copper_reaches_live_pad": live_ok, "copper_reaches_saved_pad": stale_hit,
            "moved_footprint_kept": fp_kept, "saved_drc": {k: d[k] for k in ("violations", "unconnected")},
            "live_tracks": tracks}


def b_control(s: ks.PcbnewSession, ctx: dict, kind: str) -> None:
    (x1, y1) = ctx["from"]
    (x2, y2) = ctx["to"] if kind == "good" else ctx["stale_to"]
    s.add_track_live("GND", x1, y1, x2, y1)
    if (x2, y2) != (x2, y1):
        s.add_track_live("GND", x2, y1, x2, y2)


SCENARIOS = {
    "B": {"fixture": "live_saved.kicad_pcb", "prepare": b_prepare, "act": b_act,
          "judge": b_judge, "control": b_control},
}


# ── one run ──────────────────────────────────────────────────────────────

def fresh_copy(fixture: str, root: Path) -> tuple[Path, Path]:
    work = Path(tempfile.mkdtemp(prefix="kam-v2-", dir=root))
    src = FIXTURES / fixture
    if src.is_dir():
        shutil.copytree(src, work / "p")
        board = next((work / "p").glob("*.kicad_pcb"))
    else:
        (work / "p").mkdir()
        board = work / "p" / fixture
        shutil.copy(src, board)
    # Same reason as live-pcb-e2e.ps1: an older format opens behind a modal dialog.
    ks.kicad_cli("pcb", "upgrade", str(board))
    return work, board


def run_once(scn: str, subject: str, root: Path) -> dict:
    """subject is an implementation name, or `control:good` / `control:bad`."""
    spec = SCENARIOS[scn]
    work, board = fresh_copy(spec["fixture"], root)
    fixture_sha = ks.sha256(board)
    rec: dict = {"scenario": scn, "subject": subject, "fixture_sha256": fixture_sha}
    t0 = time.perf_counter()
    try:
        with ks.PcbnewSession(board, work) as s:
            ctx = spec["prepare"](s)
            rec["context"] = ctx
            if subject.startswith("control:"):
                spec["control"](s, ctx, subject.split(":")[1])
                rec["calls"], rec["mcp_tool_calls"] = [], 0
            else:
                with Server(subject) as srv:
                    rec["calls"] = spec["act"](srv, board)
                    rec["mcp_tool_calls"] = srv.tool_calls
                    rec["response_bytes"] = srv.response_bytes
            rec["oracle"] = spec["judge"](s, ctx, board, work)
    except Exception as e:  # a crash is a result, not a reason to stop the matrix
        rec["harness_error"] = repr(e)
    rec["seconds"] = round(time.perf_counter() - t0, 1)
    calls = rec.get("calls", [])
    rec["mcp_success"] = bool(calls) and all(c["ok"] for c in calls)
    rec["functional"] = bool(rec.get("oracle", {}).get("pass"))
    rec["false_success"] = rec["mcp_success"] and not rec["functional"]
    rec["gui_intervention"] = False  # nothing in this harness touches the GUI
    return rec


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--scenario", nargs="+", default=list(SCENARIOS))
    ap.add_argument("--impl", nargs="+", default=list(IMPLS))
    ap.add_argument("--runs", type=int, default=3)
    ap.add_argument("--out", type=Path)
    args = ap.parse_args()

    root = Path(tempfile.mkdtemp(prefix="kam-pcb-live-v2-"))
    env = {"kicad": ks.kicad_version(), "os": platform.platform(),
           "fork_sha": git_sha(SOURCES["fork"]), "upstream_tag": UPSTREAM_TAG,
           "upstream_sha": git_sha(SOURCES["upstream"]),
           "binaries": {k: ks.sha256(v) for k, v in IMPLS.items()},
           "started": datetime.now(timezone.utc).isoformat(timespec="seconds")}
    results = []
    for scn in args.scenario:
        controls = [run_once(scn, f"control:{k}", root) for k in ("good", "bad")]
        valid = controls[0]["functional"] and not controls[1]["functional"]
        results += controls
        print(f"{scn} controls: good={controls[0]['functional']} bad={controls[1]['functional']}"
              f" -> oracle {'VALID' if valid else 'INVALID'}", flush=True)
        if not valid:
            continue
        for impl in args.impl:
            for i in range(args.runs):
                r = run_once(scn, impl, root)
                r["run"] = i + 1
                results.append(r)
                print(f"{scn} {impl} #{i + 1}: functional={r['functional']} mcp_success={r['mcp_success']}"
                      f" false_success={r['false_success']} calls={r.get('mcp_tool_calls')} {r['seconds']}s"
                      + (f" ERROR {r['harness_error']}" if "harness_error" in r else ""), flush=True)
    out = args.out or REPO / "bench" / "results" / f"pcb_live_v2-{datetime.now():%Y%m%d-%H%M%S}.json"
    out.write_text(json.dumps({"environment": env, "results": results}, indent=1, default=str),
                   encoding="utf-8")
    print(f"results: {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
