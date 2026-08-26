# The canonical demo

One task, under 40 seconds, and the result appears **in KiCad's own canvas** —
not in a terminal. The AI places a subcircuit's footprints on the open board and
routes them; pcbnew redraws as it happens, and the changes land in KiCad's undo
stack like any other edit.

This is the demo the project's launch material shows. It is committed here so
anyone can repeat it from the same starting point and get the same end state.

## Starting point

`konnect-demo.kicad_pcb` — an **empty 60 × 45 mm board**: four `gr_line`
segments on `Edge.Cuts` and nothing else. 739 bytes. KiCad's own DRC reports
*0 violations, 0 unconnected items* on it.

Do not edit it in place. Copy the folder somewhere else and run the demo on the
copy, so the committed pre-state stays the pre-state.

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

> This board is empty. Add a 3.3 V LDO regulator with its input and output
> capacitors, place the capacitors within 5 mm of the regulator's pins, route
> VIN, VOUT and GND between them, and run DRC when you are done.

## What you should see

In the KiCad window, without touching it:

- three footprints appear on the board — a regulator and two capacitors —
  positioned, not dumped at the origin;
- copper traces connect them;
- KiCad's undo (Ctrl+Z) walks the changes back, because they arrived through
  KiCad's own API rather than by rewriting the file underneath it.

And in the reply: a DRC result that came from `kicad-cli`, not from the model.
A check that could not run is reported as an error, never as a clean board.

## Verifying it afterwards

From the folder you ran the demo in:

```bash
kicad-cli pcb drc --exit-code-violations --format json -o drc.json konnect-demo.kicad_pcb
```

KiCad, not Konnect and not the model, decides whether the board is sound.

## Reproducing

Start again from the committed `konnect-demo.kicad_pcb` and issue the same
prompt. The end state should match: same three parts, same nets connected. A
model will not place them at identical coordinates twice — what has to match is
the circuit, not the pixels.
