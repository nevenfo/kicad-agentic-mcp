# Issue 474 live KiCad IPC captures

These protobuf payloads are real responses from KiCad 10 on 2026-09-14. They
were captured through `KiCadIpcClient::get_items_in`; they are not hand-built
approximations of KiCad objects.

- `issue_474_r1.ipc.bin` is `R1` from
  `specctra_two_resistors.kicad_pcb`. The preservation test varies only the
  symbol path and value needed to exercise a safe schematic-backed update.
- `issue_474_copper_zone_0.ipc.bin` is the filled GND copper zone from
  `crates/konnect-sexp/tests/fixtures/ecc83-pp.kicad_pcb` after KiCad 10 opened
  and upgraded the KiCad 9 board in memory.
- `issue_474_zone_0.ipc.bin` is a rule-area/keep-out from
  `crates/konnect-ipc/tests/fixtures/live_ipc.kicad_pcb`.

To regenerate, open a disposable copy of each named board in pcbnew, discover
that board with `find_open_board`, request `KotPcbFootprint` or `KotPcbZone`
through `get_items_in`, and write the returned `Any.value` bytes unchanged.
Do not substitute `build_footprint_item` or `build_zone`: this fixture exists
to preserve the distinctions present in KiCad's actual IPC messages.
