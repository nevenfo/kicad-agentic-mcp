"""Regenerate `flip_vias.kicad_pcb`: the Hi-Fi C310/C311 → B.Cu case, reduced.

Two 0805 decoupling capacitors (C310, C311) on F.Cu next to a two-pad stand-in
for the amplifier's supply pins (U1: PVDD, GND). Real library footprints, so a
flip has real silk/fab/courtyard and pad geometry to mirror.

    python make_flip.py OUT.kicad_pcb
then `kicad-cli pcb upgrade OUT.kicad_pcb`. Scenario D's good control is made
by KiCad itself at run time (`FlipItems` + via + tracks through `kipy`).
"""

from __future__ import annotations

import sys
from pathlib import Path

import make_divider as md

C_LIB = ("Capacitor_SMD", "C_0805_2012Metric")
U_LIB = ("Resistor_SMD", "R_0805_2012Metric")


def main() -> None:
    out = Path(sys.argv[1])
    md.NETS[:] = ["", "PVDD", "GND"]
    parts = [
        md.footprint(C_LIB, "C310", "100n", (100, 50), 101, {"1": "PVDD", "2": "GND"}),
        md.footprint(C_LIB, "C311", "10u", (100, 54), 201, {"1": "PVDD", "2": "GND"}),
        md.footprint(U_LIB, "U1", "TPA3255", (110, 50), 301, {"1": "PVDD", "2": "GND"}),
    ]
    edges = [md.edge(80, 30, 130, 30, 601), md.edge(130, 30, 130, 70, 602),
             md.edge(130, 70, 80, 70, 603), md.edge(80, 70, 80, 30, 604)]
    text = md.board_text(parts, [], edges, [])
    out.write_text(text, encoding="utf-8")


if __name__ == "__main__":
    main()
