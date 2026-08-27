# The canonical demo

One task, and the result appears **in KiCad's own canvas** — not in a terminal.
The AI arranges a subcircuit on the open board and routes it; pcbnew redraws as
it happens, and the changes land in KiCad's undo stack like any manual edit.
The board changes take under a second of Konnect time; the model conversation
around them takes six to seven minutes.

This is the demo the project's launch material shows. It is committed here so
anyone can repeat it from the same starting point and get the same end state.

## Starting point

A real project, not a blank file:

| File | What is in it |
|---|---|
| `konnect-demo.kicad_sch` | `U1` AP1117-33 regulator, `C1` and `C2` 10 µF, footprints assigned, nets `VIN`, `VOUT`, `GND`, two `PWR_FLAG`s. **ERC: 0 errors, 0 warnings** |
| `konnect-demo.kicad_pcb` | a 60 × 45 mm board carrying the three footprints **imported from that schematic and left in a heap**, with their nets. **DRC: 5 missing connections**, 1 silkscreen-clearance warning |
| `konnect-demo.kicad_pro` | the project that ties them together |

The five missing connections are the point. They are what the demo closes.

**Why a schematic ships with a PCB demo:** a KiCad board has nets only because a
schematic gave it some. Routing addresses nets, so a board that never came from
a schematic cannot be routed — by Konnect or by anything else. This is not a
detail of the demo; it is how KiCad works, and pretending otherwise produces a
demo that cannot finish.

Do not edit these files in place. Copy the folder somewhere else and run the demo
on the copy, so the committed pre-state stays the pre-state.

## Setup, once, off the clock

1. Install Konnect and point your MCP client at it — the
   [Quick start](../../README.md#quick-start) has the five steps.
2. In KiCad: *Preferences → Plugins* → **Enable KiCad API**, then restart KiCad.
   PCB tools talk to a running KiCad through that socket; without it there is
   nothing to watch.
3. Open your copy of `konnect-demo.kicad_pro` in KiCad and open its **PCB
   editor**. Leave it on screen — this is what the demo shows.

The setup is outside the measured prompt time.

**On v1.1.1, a standard KiCad 10 install needs no Konnect settings file.**
Konnect discovers `kicad-cli`, the KiCad GUI binary and the IPC address. Use an
explicit override only for an unusual or portable installation; see
[Troubleshooting](../../docs/TROUBLESHOOTING.md).

## The prompt

Ask for it in one turn, exactly this:

> These three footprints were just imported from the schematic and are sitting
> in a heap. Place C1 and C2 within 5 mm of the regulator U1, route VIN, VOUT
> and GND, and run DRC when you are done.

## What you should see

In the KiCad window, without touching it:

- the two capacitors move out of the heap and settle beside the regulator;
- copper traces appear between them, on the nets the schematic named;
- KiCad's undo (Ctrl+Z) walks the changes back, because they arrived through
  KiCad's own API rather than by rewriting the file underneath it.

And in the reply: a DRC result that came from `kicad-cli`, not from the model.
A check that could not run is reported as an error, never as a clean board.

The committed pair — [`before`](../../resources/images/demo-before.png),
[`after`](../../resources/images/demo-after.png) — is what run 2 produced, and
both images come from the board files themselves, same frame and same zoom:

```bash
kicad-cli pcb render --side top --zoom 2.8 --pivot "0.2,0.25,0"   -w 1200 -h 900 --background opaque -o before.png konnect-demo.kicad_pcb
```

Run the same command on the board after the prompt for the other half. Your run
will not match pixel for pixel — a model does not place a capacitor at identical
coordinates twice — and it does not need to: what has to match is the circuit.

## Verifying it afterwards

From the folder you ran the demo in:

```bash
kicad-cli pcb drc --format json -o drc.json konnect-demo.kicad_pcb
```

The pre-state reports **5 missing connections**. A finished run reports **0**.
KiCad, not Konnect and not the model, decides that.

## Reproducing

Start again from the committed files and issue the same prompt. What has to match
is the circuit, not the pixels: same three parts, same three nets connected, same
DRC verdict. A model will not place a capacitor at identical coordinates twice.

It has been run twice this way — `docs/launch/demo-run-2.md` and
`demo-run-3.md`. Both closed the three nets with 11 segments and left KiCad
reporting 0 unconnected items and no errors; both placed the capacitors within
5 mm of `U1`, at coordinates 0.6 mm apart, and run 3 rotated `C2` where run 2
did not.

## How long it takes, and what that measures

Two numbers, because they measure two different things:

- **The board changes land in under a second.** Every call that alters the board
  — the placements, the rotation, the eleven traces, the saves — totalled
  **0.686 s** in run 2 and **0.773 s** in run 3, the slowest single write 0.07 s.
  Including `run_drc`, which is KiCad doing its own check, all Konnect calls came
  to 2.3 s and 4.7 s.
- **The prompt takes 6 to 7 minutes** end to end — 377 s over 47 turns, 424 s
  over 52. That is the model looking, deciding, and routing one segment per turn;
  there is no batch *route this net* call to collapse it into one.

The 40 s figure this demo was first written against measured neither: it was
chosen before anything had been run. It is published here as the two measured
numbers instead, because a demo whose claims survive checking is worth more than
a fast one.

## If you rebuild the pre-state yourself

The board was produced by KiCad's own *Update PCB from Schematic*, which is
available **only when the PCB editor was launched from the KiCad project
manager**. Started standalone, pcbnew refuses with *cannot update the PCB because
the PCB editor is open in standalone mode*. Konnect has no equivalent tool, which
is why this step is setup rather than part of the demo.
