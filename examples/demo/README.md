# The canonical demo

One task, under 40 seconds, and the result appears **in KiCad's own canvas** —
not in a terminal. The AI arranges a subcircuit on the open board and routes it;
pcbnew redraws as it happens, and the changes land in KiCad's undo stack like any
other edit.

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

The setup is not part of the 40 seconds. The demo is.

**On v1.1.0, add two lines of configuration.** That release does not yet derive
either address, so name them yourself in your config file — the paths KiCad's
own preferences page shows:

```json
{
  "kicad_cli": "C:\\Users\\<YOU>\\AppData\\Local\\Programs\\KiCad\\10.0\\bin\\kicad-cli.exe",
  "ipc_address": "ipc://C:\\Users\\<YOU>\\AppData\\Local\\Temp\\kicad\\api.sock"
}
```

Both become unnecessary in the next release, which finds them itself.

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

## If you rebuild the pre-state yourself

The board was produced by KiCad's own *Update PCB from Schematic*, which is
available **only when the PCB editor was launched from the KiCad project
manager**. Started standalone, pcbnew refuses with *cannot update the PCB because
the PCB editor is open in standalone mode*. Konnect has no equivalent tool, which
is why this step is setup rather than part of the demo.
