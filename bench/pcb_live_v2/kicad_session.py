"""A throwaway pcbnew session and an independent read-back of what it holds.

The oracle side of the PCB live V2 benchmark. Nothing here goes through either
MCP server under test: KiCad is started on a private copy of a fixture, with a
private profile, and read back through `kipy` (KiCad's official Python IPC
binding) and `kicad-cli`. The servers are the subjects; KiCad is the referee.

Profile handling mirrors `scripts/live-pcb-e2e.ps1` (copy of the user's profile,
API server on, every first-run prompt answered) so a run never edits the user's
settings and never stalls on a modal dialog.
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import time
from dataclasses import dataclass
from pathlib import Path

from kipy import KiCad
from kipy.geometry import Vector2

KICAD_BIN = Path(os.environ.get("LOCALAPPDATA", "")) / "Programs" / "KiCad" / "10.0" / "bin"
PCBNEW = KICAD_BIN / "pcbnew.exe"
KICAD_CLI = KICAD_BIN / "kicad-cli.exe"
SOCKET_FILE = Path(os.environ.get("LOCALAPPDATA", "")) / "Temp" / "kicad" / "api.sock"
SOCKET = f"ipc://{SOCKET_FILE}"

NM = 1_000_000  # kipy speaks nanometres

from kipy.proto.board.board_types_pb2 import BoardLayer  # noqa: E402

LAYERS = {"F.Cu": BoardLayer.BL_F_Cu, "B.Cu": BoardLayer.BL_B_Cu}
LAYER_NAMES = {v: k for k, v in LAYERS.items()}


def mm(v_nm: int) -> float:
    return v_nm / NM


def kicad_cli(*args: str, timeout: float = 120) -> subprocess.CompletedProcess[str]:
    return subprocess.run([str(KICAD_CLI), *args], capture_output=True, text=True,
                          encoding="utf-8", errors="replace", timeout=timeout)


def sha256(path: Path) -> str:
    import hashlib
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def kicad_version() -> str:
    return kicad_cli("version").stdout.strip()


def make_profile(work: Path) -> Path:
    home = work / "config"
    prof = home / "10.0"
    real = Path(os.environ["APPDATA"]) / "kicad" / "10.0"
    if real.exists():
        shutil.copytree(real, prof, dirs_exist_ok=True)
    prof.mkdir(parents=True, exist_ok=True)
    common = prof / "kicad_common.json"
    data = json.loads(common.read_text(encoding="utf-8")) if common.exists() else {}
    data.setdefault("api", {})["enable_server"] = True
    dnsa = data.setdefault("do_not_show_again", {})
    for k in ("data_collection_prompt", "update_check_prompt", "env_var_overwrite_warning",
              "migrate_wrl_prompt", "scaled_3d_models_warning", "zone_fill_warning"):
        dnsa[k] = True
    common.write_text(json.dumps(data, indent=2), encoding="utf-8")
    return home


def kicad_processes() -> list[str]:
    out = subprocess.run(["tasklist", "/FO", "CSV", "/NH"], capture_output=True, text=True).stdout
    names = {"pcbnew.exe", "eeschema.exe", "kicad.exe"}
    return [l.split(",")[0].strip('"') for l in out.splitlines()
            if l.split(",")[0].strip('"').lower() in names]


@dataclass
class Pad:
    ref: str
    number: str
    x: float
    y: float
    net: str
    layers: tuple = ()


class PcbnewSession:
    """pcbnew open on `board`, reachable over IPC, killed on exit.

    Killed rather than closed: an unsaved board answers WM_CLOSE with a modal
    "save changes?" prompt, and the throwaway directory makes the leftover lock
    harmless.
    """

    def __init__(self, board: Path, work: Path, timeout: float = 90):
        self.board = board
        self.work = work
        self.timeout = timeout
        self.proc: subprocess.Popen | None = None
        self.kicad: KiCad | None = None

    def __enter__(self) -> "PcbnewSession":
        busy = kicad_processes()
        if busy:
            raise RuntimeError(f"another KiCad holds the API socket: {busy}")
        env = dict(os.environ, KICAD_CONFIG_HOME=str(make_profile(self.work)))
        self.proc = subprocess.Popen([str(PCBNEW), str(self.board)], env=env)
        deadline = time.monotonic() + self.timeout
        last = None
        while time.monotonic() < deadline:
            try:
                k = KiCad(socket_path=SOCKET, client_name="kam-bench-oracle", timeout_ms=5000)
                b = k.get_board()
                if Path(b.name).name == self.board.name or b.name:
                    self.kicad = k
                    return self
            except Exception as e:  # pipe not up yet, or AS_NOT_READY
                last = e
            time.sleep(1.0)
        self.__exit__()
        raise TimeoutError(f"pcbnew never answered on {SOCKET}: {last!r}")

    def __exit__(self, *_exc: object) -> None:
        if self.proc is not None:
            self.proc.kill()
            self.proc.wait(timeout=30)
            self.proc = None
        # The pipe outlives the process for a moment; the next session must not
        # mistake it for its own.
        for _ in range(30):
            if not kicad_processes():
                break
            time.sleep(0.5)
        time.sleep(1.0)

    # ── independent live read-back ───────────────────────────────────────

    def board_handle(self):
        assert self.kicad is not None
        return self.kicad.get_board()

    def footprint(self, ref: str):
        for fp in self.board_handle().get_footprints():
            if fp.reference_field.text.value == ref:
                return fp
        raise KeyError(ref)

    def live_pads(self) -> list[Pad]:
        out = []
        for fp in self.board_handle().get_footprints():
            ref = fp.reference_field.text.value
            for p in fp.definition.pads:
                layers = tuple(LAYER_NAMES.get(c.layer, int(c.layer)) for c in p.padstack.copper_layers)
                out.append(Pad(ref, p.number, mm(p.position.x), mm(p.position.y), p.net.name, layers))
        return out

    def live_pad(self, ref: str, number: str) -> Pad:
        for p in self.live_pads():
            if p.ref == ref and p.number == number:
                return p
        raise KeyError(f"{ref}.{number}")

    def live_tracks(self) -> list[dict]:
        return [{"net": t.net.name, "layer": LAYER_NAMES.get(t.layer, int(t.layer)),
                 "start": (mm(t.start.x), mm(t.start.y)), "end": (mm(t.end.x), mm(t.end.y))}
                for t in self.board_handle().get_tracks()]

    def live_vias(self) -> list[dict]:
        return [{"net": v.net.name, "pos": (mm(v.position.x), mm(v.position.y))}
                for v in self.board_handle().get_vias()]

    # ── the "GUI user" actor: edits that are live but not saved ──────────

    def move_footprint_live(self, ref: str, x_mm: float, y_mm: float) -> None:
        board = self.board_handle()
        fp = self.footprint(ref)
        fp.position = Vector2.from_xy(round(x_mm * NM), round(y_mm * NM))
        board.update_items(fp)

    def add_track_live(self, net: str, x1: float, y1: float, x2: float, y2: float,
                       width_mm: float = 0.25, layer: str = "F.Cu") -> None:
        """Control injector: copper drawn by the oracle side itself."""
        from kipy.board_types import Track
        board = self.board_handle()
        nets = {n.name: n for n in board.get_nets()}
        t = Track()
        t.start = Vector2.from_xy(round(x1 * NM), round(y1 * NM))
        t.end = Vector2.from_xy(round(x2 * NM), round(y2 * NM))
        t.width = round(width_mm * NM)
        t.layer = LAYERS[layer]
        t.net = nets[net]
        board.create_items(t)

    def add_via_live(self, net: str, x: float, y: float, dia_mm: float = 0.6,
                     drill_mm: float = 0.3) -> None:
        from kipy.board_types import Via
        board = self.board_handle()
        v = Via()
        v.position = Vector2.from_xy(round(x * NM), round(y * NM))
        v.net = {n.name: n for n in board.get_nets()}[net]
        if len(v.padstack.copper_layers) == 0:
            v.padstack.proto.copper_layers.add().layer = LAYERS["F.Cu"]
        v.diameter = round(dia_mm * NM)
        v.drill_diameter = round(drill_mm * NM)
        board.create_items(v)

    def flip_live(self, *refs: str) -> None:
        board = self.board_handle()
        board.flip_items([self.footprint(r) for r in refs])

    def restart(self, while_closed=None) -> None:
        """Close pcbnew (board saved first), run `while_closed()`, reopen.

        The documented route for a mutation that refuses while KiCad holds the
        board: it is an editor session restart, which this benchmark counts as
        a manual intervention."""
        self.save()
        self.__exit__()
        if while_closed is not None:
            while_closed()
        self.__enter__()

    def set_value_live(self, ref: str, value: str) -> None:
        board = self.board_handle()
        fp = self.footprint(ref)
        fp.value_field.text.value = value
        board.update_items(fp)

    def save(self) -> None:
        self.board_handle().save()


def saved_pad_layers(board: Path) -> dict[str, dict[str, list[str]]]:
    """{reference: {pad: [layers]}} from a board file as KiCad wrote it.

    Deliberately crude — balanced-paren scan, no parser shared with either
    server — because its only job is to read KiCad's own output."""
    import re
    text = board.read_text(encoding="utf-8")
    out: dict[str, dict[str, list[str]]] = {}
    for m in re.finditer(r'\n\t\(footprint "', text):
        depth, i = 0, m.start() + 2
        start = i
        while True:
            c = text[i]
            depth += (c == "(") - (c == ")")
            i += 1
            if depth == 0:
                break
        block = text[start:i]
        ref = re.search(r'\(property "Reference" "([^"]+)"', block)
        if not ref:
            continue
        pads = {}
        for pm in re.finditer(r'\(pad "([^"]*)"[^\n]*\n(?:\t{3}[^\n]*\n)*?\t{3}\(layers ([^)]*)\)', block):
            pads[pm.group(1)] = re.findall(r'"([^"]+)"', pm.group(2))
        out[ref.group(1)] = pads
    return out


def drc(board: Path, out: Path, parity: bool = False, refill: bool = False) -> dict:
    """`kicad-cli pcb drc` on the saved file. The effect is the oracle, never the exit code."""
    extra = (["--schematic-parity"] if parity else []) + (["--refill-zones"] if refill else [])
    out.unlink(missing_ok=True)
    r = kicad_cli("pcb", "drc", "--format", "json", "--severity-all", *extra, "--output", str(out), str(board))
    if not out.exists():
        raise RuntimeError(f"kicad-cli drc produced nothing: {r.stdout}{r.stderr}")
    data = json.loads(out.read_text(encoding="utf-8"))
    counts: dict[str, int] = {}
    for v in data.get("violations", []):
        counts[v["type"]] = counts.get(v["type"], 0) + 1
    return {"violations": counts, "unconnected": len(data.get("unconnected_items", [])),
            "parity": len(data.get("schematic_parity", [])), "raw": data}
