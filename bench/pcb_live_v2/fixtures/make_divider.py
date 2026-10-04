"""Regenerate the `divider/` project fixture from KiCad's own libraries.

A two-resistor divider whose board matches its schematic exactly (parity 0 under
`kicad-cli pcb drc --schematic-parity`), with real library footprints so DRC
reports no `lib_footprint_mismatch`, one routed net (/VOUT), and two board-only
mounting holes (H1, H2) that have no symbol and must survive any sync.

`--zone` adds a GND copper zone on F.Cu (scenario C's variant).
`--c-final` (with `--zone`) writes scenario C's expected end state: supply
renamed +5V by `edit_schematic_for_c`, R1 pad 1 on +5V, zone and copper kept.
`--a-final` writes scenario A's expected end state: the schematic edited by
`edit_schematic_for_a` and a board that already carries it (R2 4.7k, R3 placed).

    python make_divider.py OUTDIR [--zone]
then `kicad-cli pcb upgrade OUTDIR/divider.kicad_pcb` so the checked-in board is
in KiCad 10.0.6's native format.
"""

from __future__ import annotations

import os
import re
import shutil
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
SRC = HERE.parent.parent / "fixtures"  # bench/fixtures/divider.*
LIB = Path(os.environ["LOCALAPPDATA"]) / "Programs/KiCad/10.0/share/kicad/footprints"
R_LIB = ("Resistor_SMD", "R_0402_1005Metric")
H_LIB = ("MountingHole", "MountingHole_3.2mm_M3")
SYM = {"R1": "2d81fd01-7a39-4590-bdac-d49e4d779c8c", "R2": "c3e7961b-01d4-4476-9983-7c006349375b",
       "R3": "3a3a3a3a-0000-4000-8000-000000000003"}


def edit_schematic_for_a(sch: str) -> str:
    """Scenario A's schematic change: R2 10k -> 4.7k, and a new unconnected R3.

    Idempotent, so it can be applied to a fixture that already carries it.
    """
    if SYM["R3"] in sch:
        return sch
    start = sch.index(f'(uuid "{SYM["R2"]}")')
    start = sch.rindex("  (symbol\n", 0, start)
    end = sch.index("\n  (symbol\n", start + 1)
    r2 = sch[start:end]
    r3 = (r2.replace(SYM["R2"], SYM["R3"]).replace('"R2"', '"R3"')
          .replace("100.33 9", "120.33 9"))
    r2_new = r2.replace('(property "Value" "10k"', '(property "Value" "4.7k"')
    assert r2_new != r2 and r3 != r2
    return sch[:start] + r2_new + "\n" + r3 + sch[end:]


def edit_schematic_for_c(sch: str) -> str:
    """Scenario C's change: the supply symbol becomes +5V, so R1 pad 1 — an
    unrouted pad — changes net. Idempotent."""
    i = sch.index('(lib_id "power:+3V3")')
    old = '(property "Value" "+3V3"'
    j = sch.find(old, i)
    if j < 0 or j > sch.find("\n  (symbol\n", i):
        return sch
    return sch[:j] + '(property "Value" "+5V"' + sch[j + len(old):]
NETS = ["", "+3V3", "/VOUT", "GND"]


def footprint(lib: tuple[str, str], ref: str, value: str, at: tuple[float, float], uid: int,
              pads: dict[str, str], board_only: bool = False) -> str:
    text = (LIB / f"{lib[0]}.pretty" / f"{lib[1]}.kicad_mod").read_text(encoding="utf-8")
    text = re.sub(r'^\(footprint "[^"]+"', f'(footprint "{lib[0]}:{lib[1]}"', text)
    text = re.sub(r'\n\t\(version \d+\)\n\t\(generator "[^"]*"\)', "", text)
    head = f'\n\t(uuid "00000000-0000-0000-0000-{uid:012d}")\n\t(at {at[0]} {at[1]})'
    text = text.replace('\n\t(layer "F.Cu")', '\n\t(layer "F.Cu")' + head, 1)
    text = re.sub(r'(\(property "Reference" )"[^"]*"', rf'\1"{ref}"', text)
    text = re.sub(r'(\(property "Value" )"[^"]*"', rf'\1"{value}"', text)
    if ref in SYM:
        text = text.replace("\n\t(attr", f'\n\t(path "/{SYM[ref]}")\n\t(sheetname "/")'
                                          f'\n\t(sheetfile "divider.kicad_sch")\n\t(attr', 1)
    if board_only:
        text = re.sub(r"\(attr ([^)]*)\)", lambda m: f"(attr {m.group(1)} board_only)", text, count=1)
    for num, net in pads.items():
        text = re.sub(rf'(\(pad "{num}" [^\n]*\n(?:\t\t[^\n]*\n)*?\t\t\(layers [^\n]*\n)',
                      lambda m: m.group(1) + f'\t\t(net {NETS.index(net)} "{net}")\n', text, count=1)
    return "\n".join("\t" + l if l else l for l in text.rstrip().splitlines())


def track(x1, y1, x2, y2, uid) -> str:
    return (f'\t(segment (start {x1} {y1}) (end {x2} {y2}) (width 0.25) (layer "F.Cu") '
            f'(net {NETS.index("/VOUT")}) (uuid "00000000-0000-0000-0000-{uid:012d}"))')


def edge(x1, y1, x2, y2, uid) -> str:
    return (f'\t(gr_line (start {x1} {y1}) (end {x2} {y2}) (stroke (width 0.05) (type default)) '
            f'(layer "Edge.Cuts") (uuid "00000000-0000-0000-0000-{uid:012d}"))')


ZONE = f'''\t(zone (net {NETS.index("GND")}) (net_name "GND") (layer "F.Cu")
\t\t(uuid "00000000-0000-0000-0000-000000000900") (hatch edge 0.5)
\t\t(connect_pads (clearance 0.3)) (min_thickness 0.25)
\t\t(fill yes (thermal_gap 0.5) (thermal_bridge_width 0.5))
\t\t(polygon (pts (xy 82 32) (xy 128 32) (xy 128 68) (xy 82 68)))
\t)'''


def main() -> None:
    out = Path(sys.argv[1])
    zone = "--zone" in sys.argv
    a_final = "--a-final" in sys.argv
    c_final = "--c-final" in sys.argv
    if c_final:
        NETS[NETS.index("+3V3")] = "+5V"
    if a_final:  # what KiCad names an unconnected pin's net
        NETS.extend(["unconnected-(R3-Pad1)", "unconnected-(R3-Pad2)"])
    out.mkdir(parents=True, exist_ok=True)
    for ext in ("kicad_pro", "kicad_prl"):
        shutil.copy(SRC / f"divider.{ext}", out / f"divider.{ext}")
    sch = (SRC / "divider.kicad_sch").read_text(encoding="utf-8")
    fp = f"{R_LIB[0]}:{R_LIB[1]}"
    sch = sch.replace('(property "Footprint" ""\n      (at 100.33 80.01 0)',
                      f'(property "Footprint" "{fp}"\n      (at 100.33 80.01 0)')
    sch = sch.replace('(property "Footprint" ""\n      (at 100.33 95.25 0)',
                      f'(property "Footprint" "{fp}"\n      (at 100.33 95.25 0)')
    assert sch.count(fp) == 2, "schematic footprint fields not found"
    if a_final:
        sch = edit_schematic_for_a(sch)
    if c_final:
        sch = edit_schematic_for_c(sch)
    (out / "divider.kicad_sch").write_text(sch, encoding="utf-8")

    parts = [
        footprint(R_LIB, "R1", "10k", (100, 45), 101, {"1": NETS[1], "2": "/VOUT"}),
        footprint(R_LIB, "R2", "4.7k" if a_final else "10k", (100, 55), 201, {"1": "/VOUT", "2": "GND"}),
        *([footprint(R_LIB, "R3", "10k", (115, 50), 701,
                               {"1": "unconnected-(R3-Pad1)", "2": "unconnected-(R3-Pad2)"})] if a_final else []),
        footprint(H_LIB, "H1", "MountingHole", (85, 35), 301, {}, board_only=True),
        footprint(H_LIB, "H2", "MountingHole", (125, 65), 401, {}, board_only=True),
    ]
    tracks = [track(100.51, 45, 102, 45, 501), track(102, 45, 102, 50, 502),
              track(102, 50, 98, 50, 503), track(98, 50, 98, 55, 504), track(98, 55, 99.49, 55, 505)]
    edges = [edge(80, 30, 130, 30, 601), edge(130, 30, 130, 70, 602),
             edge(130, 70, 80, 70, 603), edge(80, 70, 80, 30, 604)]
    board = board_text(parts, tracks, edges, [ZONE] if zone else [])
    (out / "divider.kicad_pcb").write_text(board, encoding="utf-8")


def board_text(parts: list[str], tracks: list[str], edges: list[str], extra: list[str]) -> str:
    return "\n".join([
        "(kicad_pcb", "\t(version 20241229)", '\t(generator "kam-bench")', '\t(generator_version "10.0")',
        "\t(general (thickness 1.6))", '\t(paper "A4")',
        '\t(layers (0 "F.Cu" signal) (2 "B.Cu" signal) (9 "F.Adhes" user "F.Adhesive") '
        '(11 "B.Adhes" user "B.Adhesive") (13 "F.Paste" user) (15 "B.Paste" user) '
        '(5 "F.SilkS" user "F.Silkscreen") (7 "B.SilkS" user "B.Silkscreen") (1 "F.Mask" user) '
        '(3 "B.Mask" user) (17 "Dwgs.User" user "User.Drawings") (19 "Cmts.User" user "User.Comments") '
        '(25 "Edge.Cuts" user) (27 "Margin" user) (31 "F.CrtYd" user "F.Courtyard") '
        '(29 "B.CrtYd" user "B.Courtyard") (35 "F.Fab" user) (33 "B.Fab" user))',
        "\t(setup (pad_to_mask_clearance 0))",
        *[f'\t(net {i} "{n}")' for i, n in enumerate(NETS)],
        *parts, *tracks, *edges, *extra, ")", ""])


if __name__ == "__main__":
    main()
