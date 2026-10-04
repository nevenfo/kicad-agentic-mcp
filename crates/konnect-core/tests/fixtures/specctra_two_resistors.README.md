# Specctra two-resistor board fixture

`specctra_two_resistors.kicad_pcb` is a deliberately small two-layer board used
to test the first fail-closed Specctra export profile. It was derived from the
repository's existing PCB integration fixture, assigned stable test UUIDs and a
closed rectangular outline, then opened and re-saved by KiCad 10.0.5 with:

```text
kicad-cli pcb upgrade --force specctra_two_resistors.kicad_pcb
```

That final KiCad-authored serialization is intentional. In particular, it
captures KiCad 10's direct `(net "NAME")` pad syntax rather than relying on a
hand-written approximation of the board format.

`specctra_two_resistors.native-kicad-10.dsn` was exported from that board by
KiCad 10.0.5's real `pcbnew.ExportSpecctraDSN(board, path)` binding. Only the
environment-specific absolute output path in the root `(pcb ...)` identifier
was normalized to `board.dsn`; structure, placement, library, network, rules,
and wiring content remain KiCad-authored. It is the differential fixture for
the optional KiCad 10 ActionPlugin bridge.
