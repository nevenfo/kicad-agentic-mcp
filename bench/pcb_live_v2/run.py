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


def b_act(srv: Server, board: Path, s: ks.PcbnewSession, ctx: dict) -> list[dict]:
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


def b_control(s: ks.PcbnewSession, ctx: dict, kind: str, board: Path) -> None:
    (x1, y1) = ctx["from"]
    (x2, y2) = ctx["to"] if kind == "good" else ctx["stale_to"]
    s.add_track_live("GND", x1, y1, x2, y1)
    if (x2, y2) != (x2, y1):
        s.add_track_live("GND", x2, y1, x2, y2)


# ── scenario E — negative schematic-parity oracle ────────────────────────
#
# The board diverges from its schematic (R2's value, edited live and saved, the
# way a GUI user would). Intention: "does the board match the schematic?",
# asked twice — diverged, then restored. The referee is `kicad-cli pcb drc
# --schematic-parity` run by the oracle side. A server passes only if it
# reports a non-zero parity count while diverged and a *measured* zero once
# restored. A zero that is not backed by a real parity run is the false green
# the Hi-Fi stress test met; `null` is honest but not functional.


def find_key(obj, key):
    if isinstance(obj, dict):
        if key in obj:
            return obj[key]
        for v in obj.values():
            r = find_key(v, key)
            if r is not None:
                return r
    elif isinstance(obj, list):
        for v in obj:
            r = find_key(v, key)
            if r is not None:
                return r
    return None


def reported_parity(body) -> int | None:
    v = find_key(body, "schematic_parity") if isinstance(body, (dict, list)) else None
    return len(v) if isinstance(v, list) else v if isinstance(v, int) else None


def e_prepare(s: ks.PcbnewSession) -> dict:
    return {"phases": []}


def e_cycle(s: ks.PcbnewSession, ctx: dict, board: Path, ask) -> list[dict]:
    calls = []
    for phase, value in (("diverged", "4.7k"), ("restored", "10k")):
        s.set_value_live("R2", value)
        s.save()
        arb = ks.drc(board, board.parent / f"arb-{phase}.json", parity=True)
        c = ask()
        calls.append(c)
        ctx["phases"].append({"phase": phase, "arbiter_parity": arb["parity"],
                              "reported_parity": reported_parity(c["body"])})
    return calls


def e_act(srv: Server, board: Path, s: ks.PcbnewSession, ctx: dict) -> list[dict]:
    load = srv.call("load_toolset", {"name": "verification"})
    return [load] + e_cycle(s, ctx, board, lambda: srv.call("run_drc", {"board": str(board), "severity": "all"}))


def e_control(s: ks.PcbnewSession, ctx: dict, kind: str, board: Path) -> None:
    """good: a reporter that echoes a real parity run; bad: one that always says 0."""
    def ask():
        if kind == "bad":
            return {"tool": "control", "ok": True, "body": {"by_category": {"schematic_parity": 0}}}
        r = ks.drc(board, board.parent / "ctl.json", parity=True)
        return {"tool": "control", "ok": True, "body": {"by_category": {"schematic_parity": r["parity"]}}}
    e_cycle(s, ctx, board, ask)


def e_judge(s: ks.PcbnewSession, ctx: dict, board: Path, work: Path) -> dict:
    ph = {p["phase"]: p for p in ctx["phases"]}
    d, r = ph.get("diverged", {}), ph.get("restored", {})
    arbiter_ok = (d.get("arbiter_parity") or 0) > 0 and r.get("arbiter_parity") == 0
    caught = (d.get("reported_parity") or 0) > 0
    cleared = r.get("reported_parity") == 0
    return {"pass": arbiter_ok and caught and cleared, "arbiter_separates": arbiter_ok,
            "divergence_reported": caught, "restore_reported_clean": cleared,
            "false_green": d.get("reported_parity") == 0, "phases": ctx["phases"]}


# ── scenario A — schematic → PCB sync ────────────────────────────────────
#
# The schematic changes (R2 10k → 4.7k, a new unconnected R3); intention:
# "bring the open board up to date with the schematic". Correct: the delta is
# applied (R2's value, R3 present) and nothing else moves — R1/R2 placement,
# the /VOUT copper and the board-only mounting holes H1/H2 are kept. Parity is
# then 0 under `kicad-cli`. Good control: a fixture already in the expected end
# state. Bad control: a naive sync that applies the value but drops H1.

sys.path.insert(0, str(FIXTURES))
from make_divider import edit_schematic_for_a  # noqa: E402

A_KEEP = ("R1", "R2", "H1", "H2")


def a_snapshot(s: ks.PcbnewSession) -> dict:
    fps = {}
    for fp in s.board_handle().get_footprints():
        fps[fp.reference_field.text.value] = {
            "pos": (ks.mm(fp.position.x), ks.mm(fp.position.y)),
            "layer": int(fp.layer), "value": fp.value_field.text.value}
    tracks = sorted((t["net"], t["start"], t["end"]) for t in s.live_tracks())
    return {"footprints": fps, "tracks": tracks}


def a_prepare(s: ks.PcbnewSession) -> dict:
    sch = s.board.with_suffix(".kicad_sch")
    sch.write_text(edit_schematic_for_a(sch.read_text(encoding="utf-8")), encoding="utf-8")
    return {"before": a_snapshot(s)}


def a_act(srv: Server, board: Path, s: ks.PcbnewSession, ctx: dict) -> list[dict]:
    sch = str(board.with_suffix(".kicad_sch"))
    calls = [srv.call("load_toolset", {"name": "sch_export"})]
    listed = {t["name"] for t in (srv.client.tools_list().result or {}).get("tools", [])}
    if "update_pcb_from_schematic" not in listed:
        ctx["capability_absent"] = True
        return calls
    dry = srv.call("update_pcb_from_schematic", {"schematic": sch, "board": str(board), "dry_run": True})
    calls.append(dry)
    rev = find_key(dry["body"], "plan_revision") if isinstance(dry["body"], dict) else None
    ctx["sync_status"] = find_key(dry["body"], "status") if isinstance(dry["body"], dict) else dry["body"]
    if ctx["sync_status"] not in (None, "ready"):
        ctx["sync_diagnostics"] = find_key(dry["body"], "diagnostics")
        ctx["refused"] = True
        return calls
    if rev:
        calls.append(srv.call("update_pcb_from_schematic", {
            "schematic": sch, "board": str(board), "dry_run": False, "expected_plan_revision": rev}))
    return calls


def a_control(s: ks.PcbnewSession, ctx: dict, kind: str, board: Path) -> None:
    if kind == "bad":
        s.set_value_live("R2", "4.7k")
        s.board_handle().remove_items(s.footprint("H1"))
    # good: the fixture already holds the expected end state (see SCENARIOS)


def a_judge(s: ks.PcbnewSession, ctx: dict, board: Path, work: Path) -> dict:
    before, after = ctx["before"], a_snapshot(s)
    bf, af = before["footprints"], after["footprints"]
    kept = {r: r in af and _near(af[r]["pos"], bf[r]["pos"], 1e-3) and af[r]["layer"] == bf[r]["layer"]
            for r in A_KEEP if r in bf}
    copper_kept = after["tracks"] == before["tracks"]
    delta = af.get("R2", {}).get("value") == "4.7k" and "R3" in af
    s.save()
    d = ks.drc(board, work / "drc.json", parity=True)
    return {"pass": all(kept.values()) and copper_kept and delta and d["parity"] == 0,
            "placement_kept": kept, "copper_kept": copper_kept, "delta_applied": delta,
            "parity_after": d["parity"], "capability_absent": ctx.get("capability_absent", False),
            "footprints_after": af}


# ── scenario C — copper zone + net change (class of upstream #779) ───────
#
# Same divider with a GND zone on F.Cu. The supply symbol becomes +5V, so R1
# pad 1 — whose net carries no copper at all — must change net. A zone on GND
# must not make every net look "routed" and block the change. Correct: R1.1 on
# +5V, zone and tracks unchanged, placement kept, parity 0. Good control: the
# expected end state; bad control: nothing applied (the #779 refusal).

from make_divider import edit_schematic_for_c  # noqa: E402


def c_snapshot(s: ks.PcbnewSession) -> dict:
    snap = a_snapshot(s)
    snap["zones"] = sorted((z.net.name, int(z.layer) if hasattr(z, "layer") else 0)
                           for z in s.board_handle().get_zones())
    return snap


def c_prepare(s: ks.PcbnewSession) -> dict:
    sch = s.board.with_suffix(".kicad_sch")
    sch.write_text(edit_schematic_for_c(sch.read_text(encoding="utf-8")), encoding="utf-8")
    return {"before": c_snapshot(s)}


def c_control(s: ks.PcbnewSession, ctx: dict, kind: str, board: Path) -> None:
    pass  # good: end-state fixture; bad: nothing applied


def c_judge(s: ks.PcbnewSession, ctx: dict, board: Path, work: Path) -> dict:
    before, after = ctx["before"], c_snapshot(s)
    bf, af = before["footprints"], after["footprints"]
    kept = {r: r in af and _near(af[r]["pos"], bf[r]["pos"], 1e-3) for r in A_KEEP}
    pad_net = s.live_pad("R1", "1").net
    s.save()
    d = ks.drc(board, work / "drc.json", parity=True)
    ok = (all(kept.values()) and after["tracks"] == before["tracks"]
          and after["zones"] == before["zones"] and pad_net == "+5V" and d["parity"] == 0)
    return {"pass": ok, "placement_kept": kept, "copper_kept": after["tracks"] == before["tracks"],
            "zones_kept": after["zones"] == before["zones"], "zones_after": after["zones"],
            "r1_pad1_net": pad_net, "parity_after": d["parity"],
            "capability_absent": ctx.get("capability_absent", False),
            "sync_status": ctx.get("sync_status"), "sync_diagnostics": ctx.get("sync_diagnostics")}


# ── scenario D — flip + layer change through a via (Hi-Fi C310/C311) ─────
#
# Intention: put C310 and C311 on B.Cu where they stand, then join C310 pad 1
# to U1 pad 1 (PVDD) — B.Cu track to a via at D_VIA, F.Cu track on to U1.
# The scripted agent takes pad positions from the server's own read-back after
# the flip, so a stale read-back shows up as misplaced copper. Correct: both on
# B.Cu, same XY, pads mirrored, a PVDD copper chain pad → via → pad, and a
# saved file KiCad's DRC finds clean of dangling/short/clearance with exactly
# one connection fewer to make. Good control: KiCad's own FlipItems plus copper
# by `kipy`; bad control: the same, but copper starts at the pre-flip pad.

D_VIA = (100.0, 46.5)  # clear of both pads under either flip convention
D_REFS = ("C310", "C311")


def pad_xy(body, number: str) -> tuple[float, float] | None:
    """Find pad `number`'s board position in a server's free-form read-back."""
    if isinstance(body, dict):
        num = next((body[k] for k in ("number", "pad", "pad_number", "name") if k in body), None)
        if str(num) == number:
            pos = body.get("position") if isinstance(body.get("position"), dict) else body
            if "x" in pos and "y" in pos:
                return float(pos["x"]), float(pos["y"])
        for v in body.values():
            r = pad_xy(v, number)
            if r:
                return r
    elif isinstance(body, list):
        for v in body:
            r = pad_xy(v, number)
            if r:
                return r
    return None


def d_snapshot(s: ks.PcbnewSession) -> dict:
    out = {}
    for ref in (*D_REFS, "U1"):
        fp = s.footprint(ref)
        out[ref] = {"pos": (ks.mm(fp.position.x), ks.mm(fp.position.y)),
                    "layer": ks.LAYER_NAMES.get(fp.layer, int(fp.layer)),
                    "pads": {p.number: (p.x, p.y) for p in s.live_pads() if p.ref == ref}}
    return out


def d_prepare(s: ks.PcbnewSession) -> dict:
    return {"before": d_snapshot(s)}


def d_act(srv: Server, board: Path, s: ks.PcbnewSession, ctx: dict) -> list[dict]:
    b = str(board)
    calls = [srv.call("load_toolset", {"name": "pcb_components"}),
             srv.call("load_toolset", {"name": "pcb_routing"})]
    first = srv.call("flip_component", {"board": b, "reference": D_REFS[0], "layer": "B.Cu"})
    calls.append(first)
    if first["ok"]:
        calls.append(srv.call("flip_component", {"board": b, "reference": D_REFS[1], "layer": "B.Cu"}))
    else:
        # The server's own documented route: board closed, flip, reopen.
        ctx["live_flip_refused"] = str(first["body"])[:300]
        ctx["session_restart"] = True

        def flip_closed():
            for ref in D_REFS:
                calls.append(srv.call("flip_component", {"board": b, "reference": ref, "layer": "B.Cu"}))
        s.restart(flip_closed)
    c_pads = srv.call("get_component_pads", {"board": b, "reference": "C310"})
    u_pads = srv.call("get_component_pads", {"board": b, "reference": "U1"})
    calls += [c_pads, u_pads]
    cp, up = pad_xy(c_pads["body"], "1"), pad_xy(u_pads["body"], "1")
    ctx["server_readback"] = {"C310.1": cp, "U1.1": up}
    if not cp or not up:
        ctx["readback_unusable"] = True
        return calls
    vx, vy = D_VIA
    calls += [
        srv.call("route_trace", {"board": b, "net_name": "PVDD", "layer": "B.Cu", "width": 0.25,
                                 "x1": cp[0], "y1": cp[1], "x2": vx, "y2": vy}),
        srv.call("add_via", {"board": b, "net_name": "PVDD", "x": vx, "y": vy}),
        srv.call("route_trace", {"board": b, "net_name": "PVDD", "layer": "F.Cu", "width": 0.25,
                                 "x1": vx, "y1": vy, "x2": up[0], "y2": up[1]}),
    ]
    return calls


def d_control(s: ks.PcbnewSession, ctx: dict, kind: str, board: Path) -> None:
    s.flip_live(*D_REFS)
    cp = s.live_pad("C310", "1") if kind == "good" else None
    start = (cp.x, cp.y) if cp else ctx["before"]["C310"]["pads"]["1"]
    up = s.live_pad("U1", "1")
    s.add_track_live("PVDD", *start, *D_VIA, layer="B.Cu")
    s.add_via_live("PVDD", *D_VIA)
    s.add_track_live("PVDD", *D_VIA, up.x, up.y, layer="F.Cu")


def d_judge(s: ks.PcbnewSession, ctx: dict, board: Path, work: Path) -> dict:
    before, after = ctx["before"], d_snapshot(s)
    flips = {}
    for ref in D_REFS:
        b, a = before[ref], after[ref]
        fx, fy = b["pos"]
        # KiCad flips top/bottom by default and left/right on request; both
        # are a correct flip, nothing else is.
        lr = all(_near(a["pads"][n], (2 * fx - b["pads"][n][0], b["pads"][n][1]), 0.01) for n in b["pads"])
        tb = all(_near(a["pads"][n], (b["pads"][n][0], 2 * fy - b["pads"][n][1]), 0.01) for n in b["pads"])
        flips[ref] = {"layer": a["layer"], "xy_kept": _near(a["pos"], b["pos"], 1e-3),
                      "pads_mirrored": "left_right" if lr else "top_bottom" if tb else False,
}
    tracks, vias = s.live_tracks(), s.live_vias()
    cpad, upad = after["C310"]["pads"]["1"], after["U1"]["pads"]["1"]
    chain = copper_joins(tracks, "PVDD", cpad, upad)
    b_side = any(t["layer"] == "B.Cu" and (_near(t["start"], cpad, 0.05) or _near(t["end"], cpad, 0.05))
                 for t in tracks if t["net"] == "PVDD")
    via_ok = any(v["net"] == "PVDD" and _near(v["pos"], D_VIA, 0.05) for v in vias)
    s.save()
    # Pad copper side, read from the file KiCad itself just wrote: the IPC
    # padstack reports the footprint-definition layer, flipped or not.
    saved = ks.saved_pad_layers(board)
    for ref in D_REFS:
        flips[ref]["saved_pad_layers"] = saved.get(ref)
        flips[ref]["pad_copper_on_B"] = bool(saved.get(ref)) and all(
            "B.Cu" in ls and "F.Cu" not in ls for ls in saved[ref].values())
    flips_ok = all(f["layer"] == "B.Cu" and f["xy_kept"] and f["pads_mirrored"] and f["pad_copper_on_B"]
                   for f in flips.values())
    d = ks.drc(board, work / "drc.json")
    bad = {k: d["violations"].get(k, 0) for k in ("track_dangling", "via_dangling", "shorting_items",
                                                  "clearance", "copper_edge_clearance")}
    saved_ok = d["unconnected"] == 3 and not any(bad.values())
    return {"pass": flips_ok and chain and b_side and via_ok and saved_ok, "flips": flips,
            "copper_chain": chain, "b_side_at_pad": b_side, "via_ok": via_ok,
            "saved_drc": {"unconnected": d["unconnected"], **bad, "all": d["violations"]},
            "server_readback": ctx.get("server_readback"), "live_pad_C310.1": cpad,
            "session_restart": ctx.get("session_restart", False)}


SCENARIOS = {
    "B": {"fixture": "live_saved.kicad_pcb", "prepare": b_prepare, "act": b_act,
          "judge": b_judge, "control": b_control},
    "A": {"fixture": "divider", "control_fixture": {"good": "divider_a_final"},
          "prepare": a_prepare, "act": a_act, "judge": a_judge, "control": a_control},
    "C": {"fixture": "divider_zone", "control_fixture": {"good": "divider_c_final"},
          "prepare": c_prepare, "act": a_act, "judge": c_judge, "control": c_control},
    "D": {"fixture": "flip_vias.kicad_pcb", "prepare": d_prepare, "act": d_act,
          "judge": d_judge, "control": d_control},
    "E": {"fixture": "divider", "prepare": e_prepare, "act": e_act,
          "judge": e_judge, "control": e_control},
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
    fixture = spec["fixture"]
    if subject.startswith("control:"):
        fixture = spec.get("control_fixture", {}).get(subject.split(":")[1], fixture)
    work, board = fresh_copy(fixture, root)
    fixture_sha = ks.sha256(board)
    rec: dict = {"scenario": scn, "subject": subject, "fixture_sha256": fixture_sha}
    t0 = time.perf_counter()
    try:
        with ks.PcbnewSession(board, work) as s:
            ctx = spec["prepare"](s)
            rec["context"] = ctx
            if subject.startswith("control:"):
                spec["control"](s, ctx, subject.split(":")[1], board)
                rec["calls"], rec["mcp_tool_calls"] = [], 0
            else:
                with Server(subject) as srv:
                    rec["calls"] = spec["act"](srv, board, s, ctx)
                    rec["mcp_tool_calls"] = srv.tool_calls
                    rec["response_bytes"] = srv.response_bytes
            rec["oracle"] = spec["judge"](s, ctx, board, work)
    except Exception as e:  # a crash is a result, not a reason to stop the matrix
        rec["harness_error"] = repr(e)
    rec["seconds"] = round(time.perf_counter() - t0, 1)
    calls = rec.get("calls", [])
    rec["capability_absent"] = bool(rec.get("oracle", {}).get("capability_absent"))
    rec["refused"] = bool(rec.get("context", {}).get("refused"))
    # A server with no tool for the intention, or one that refused it, made no
    # success claim: neither can be a false success.
    rec["mcp_success"] = (bool(calls) and all(c["ok"] for c in calls)
                          and not rec["capability_absent"] and not rec["refused"])
    rec["functional"] = bool(rec.get("oracle", {}).get("pass"))
    rec["false_success"] = rec["mcp_success"] and not rec["functional"]
    # Nothing here clicks in the GUI; an editor restart forced by a server that
    # refuses while KiCad holds the board is the manual step a user would make.
    rec["gui_intervention"] = bool(rec.get("context", {}).get("session_restart"))
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
