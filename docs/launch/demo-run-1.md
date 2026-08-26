# Demo run 1 — the task chosen in R.3.1, run by a model, and what it hit

Run on 2026-08-26 against the **published** v1.1.0 binary (INV-R1), on the
committed pre-state of `examples/demo/`, with the committed prompt, verbatim.

This document is evidence, not marketing. The run **failed its own criterion**,
and the reason it failed is worth more than a passing run would have been.

## Setup

| | |
|---|---|
| Server | `…\3rdparty\plugins\com_github_mixelpixx_konnect\bin\konnect.exe` — the installed v1.1.0 asset, not a local build |
| Config | `kicad_cli` and `ipc_address` named by hand, as `examples/demo/README.md` documents for v1.1.0 |
| Board | a byte-identical copy of `examples/demo/konnect-demo.kicad_pcb` (739 B, md5 `309c8894…`), open in `pcbnew` |
| Client | `claude -p`, prompt verbatim, tools restricted to the MCP server plus `Glob`/`Read` — every write had to go through KiCad |
| Preflight | green, and free: `open_project` answered *KiCAD is running and IPC is available*; `find_capabilities` returned `place_component`, `route_pad_to_pad`, `route_trace` |

## Result

| | |
|---|---|
| Wall clock | **406 s** — the budget is 40 s |
| Turns | 41, stopped by `max_turns`, not by finishing |
| Tool calls | 55, of which **15 returned an error** |
| Cost | 2.44 USD |
| On the board afterwards | three footprints — `U1` SOT-223-3, `C1` and `C2` 0805 — and **zero track segments** |
| DRC (`kicad-cli`) | 0 errors, 0 unconnected, **5 warnings**: 3 × `lib_footprint_mismatch`, 2 × `silk_over_copper` |

The placement half worked, live, in KiCad's own canvas: three footprints
arrived through IPC and pcbnew redrew. The routing half never happened, and it
could not have.

## Why it could not have

**Routing needs a net, and nothing on this surface can create one.**

- `route_trace` is refused by KiCad: `Net 'VIN' not found on board`.
- `route_pad_to_pad` fails the same way — it addresses nets, not coordinates.
- `add_net` refuses the board's own format: *this board has no `(net <id> …)`
  table (KiCAD 20260206+ writes net names directly on items); a net cannot be
  added by inserting a table entry here*.
- No tool assigns a net to a pad. `find_capabilities` on
  *"assign a net name to a pad of a placed footprint"* returns
  `edit_footprint_pad` (edits a `.kicad_mod` on disk), `assign_net_to_class`
  (netclass membership, not net creation) and `get_pin_net_name` (a read).
- No tool does *Update PCB from Schematic*. `generate_netlist` and
  `export_netlist` move netlists **out**; nothing brings one **in**.

So on a board that carries no netlist, the product can place and it can measure,
but it cannot connect. A board only has nets if KiCad put them there from a
schematic. The demo of R.3.1 asked for a board with no netlist to be routed;
that is not a slow path, it is a closed one.

This is not a defect introduced by the demo. It is the shape of the PCB half of
the product, and R.3.1's measurement did not reach it: two `place_component`
calls in 176 ms proved the **transport**, and the transport is fine.

## Defects found, classified (INV-R3)

| ID | What | Class |
|---|---|---|
| **F-13** | A net cannot be created or assigned on a board without a netlist, so `route_trace` / `route_pad_to_pad` are unreachable there. `add_net` additionally targets a file format KiCad 10 no longer writes | **product** |
| **F-14** | `get_layer_list` fails `malformed_document: no (layers) section` on a minimal board. A board KiCad itself opens is reported as malformed | **product** |
| **F-15** | PCB reads and PCB writes disagree about where the board is: `place_component` goes to the running pcbnew, while `get_component_pads` and `get_pad_position` read the **file**, so a footprint just placed is invisible until something saves. The model lost several turns to this | **product** |
| **F-16** | `launch_kicad_ui` fails `program not found` — the same missing-discovery defect R.7 fixed for `kicad-cli`, on `kicad` this time (D149's chain is not applied to it) | **product** |
| **F-17** | Footprints placed through IPC raise `lib_footprint_mismatch` in DRC against the library they came from | **product, minor** |

None of these are fixed by this document. R.9 carried them, and the phase's
narrow exception decided each one:

| ID | Disposition |
|---|---|
| **F-13** | **Recorded, not fixed.** Creating or assigning nets is a capability, and R adds none. `examples/demo/` says so in the open: a PCB has nets only because a schematic gave it some, and *Update PCB from Schematic* is setup, not demo. Named candidate for the R.6 gate |
| **F-14** | **Fixed** (R.9.2). `get_layer_list` answers with KiCad's own default stackup, flagged `"declared": false`; `add_layer` still refuses, but says how to get a table instead of calling the board malformed |
| **F-15** | **Documented, not fixed** (R.9.3). Rerouting the read means moving the whole PCB read surface onto IPC; saving the user's board unasked is worse. Both pad reads now answer `"source": "file"`, `TROUBLESHOOTING` carries the symptom, and the rerouting is a named candidate for the R.6 gate |
| **F-16** | **Fixed** (R.9.1). D149's discovery chain now covers the `kicad` GUI binary, from a resolver both the server and the installer share |
| **F-17** | **Recorded, not fixed** (R.9.5). Run 2, whose footprints came from KiCad's own *Update PCB from Schematic* and were only moved over IPC, saw no mismatch — so the defect belongs to IPC *building* a footprint, not to IPC touching one. Named candidate for the R.6 gate |

## What the run also proved

- The published v1.1.0 binary, configured by hand as documented, is reached by a
  standalone MCP client and writes into a running KiCad. That is the Quick
  start's step 5 claim, and R.2 left it unproved. **It holds** — for placement.
- The tool allowlist held: the model tried `Bash`, `PowerShell` and `Grep` when
  the KiCad path resisted, and every one of them was refused. Nothing edited the
  board except KiCad.
- A model with 40 turns and no route forward does not stop; it searches. Nine of
  the 41 turns went to re-discovering capabilities after failures. A demo whose
  first failure is unrecoverable will always run long.

## Timing, for whoever narrows the task

406 s / 41 turns ≈ **10 s per turn**. A 40 s budget is therefore about **four
turns**: discover, load, one batched `kicad_invoke`, verify. Any demo that needs
the model to search a footprint library, recover from an error, or read the board
back before acting is already over budget.
