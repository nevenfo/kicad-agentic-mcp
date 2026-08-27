# Draft — forum.kicad.info, *Software Tools*

**Not posted.** Requirements and the go/no-go list are in
[`launch-kit.md`](launch-kit.md).

**Title:** KiCad Agentic MCP — an AI assistant that edits your board through
KiCad's own API, and lets KiCad decide whether the result is good

---

This audience is wary of a language model writing to their files, and it is
right to be. So the first thing worth saying about this plugin is what it does
*not* do: it does not rewrite your `.kicad_pcb` behind KiCad's back.

**KiCad Agentic MCP** is a native KiCad 10 plugin — one Rust binary — that
exposes 202 tools over the [Model Context Protocol](https://modelcontextprotocol.io),
so Claude, or another MCP client, can work your project directly:

- place and wire schematic parts, by pin name;
- place, move, rotate and route footprints in the running PCB editor, over
  KiCad's IPC API, so **Ctrl+Z walks the changes back** the way it does for
  anything you did by hand;
- run ERC and DRC, decoupling and power-rail audits, connectivity checks;
- produce Gerbers, drill, BOM, pick-and-place, 3D models and PDF.

**Verification is KiCad's, not the model's.** When the assistant says a board
passes, that verdict came from `kicad-cli`. A check that could not run is
reported as an error, never as a clean board — that rule is a test in the suite,
not a good intention.

## What one prompt actually does

There is a demo committed in the repository — the starting board, the exact
prompt, the setup steps — so you can run the same thing rather than take a
screenshot's word for it. A regulator and two capacitors arrive from the
schematic sitting in a heap, with five missing connections. One prompt asks for
the capacitors placed within 5 mm of the regulator and the three nets routed.

Run twice from the same starting state, on two different days: both times KiCad
reported **5 unconnected items before, 0 after**, eleven track segments, and no
errors — and both times the coordinates were different, because a model does not
place a capacitor twice in the same spot. Silkscreen warnings remained, and the
run explained why rather than hiding them.

Two honest numbers about speed: the board changes themselves — both placements,
eleven traces, the saves — take **under a second**. The prompt around them takes
**six to seven minutes**, because that is how long the model takes to look,
decide, and route one segment per turn. The product is fast; the conversation
is not.

## What it costs to install

Download the `konnect-pcm-v1.1.1-*` zip for your OS from the release page and install
it through KiCad's Plugin and Content Manager with *Install from File* — it is
not in KiCad's official addon repository, so there is no repository URL to add.
Then enable the KiCad API in *Preferences → Plugins*; it ships off, and every
PCB tool needs it. The README's Quick start is five steps, and it was walked end
to end on a machine that had never had this plugin installed, with every
friction written down — including the ones that made it worse.

## What I am not claiming

- **Not a success rate for your boards.** The benchmark in the repository is six
  golden tasks with three repeats on one machine — 18/18 — and it is described
  there in full. It says nothing about your parts or your prompt.
- **PCB tools need KiCad running**, with the API enabled and the board open.
  pcbnew has no headless path, so neither does this.
- **Windows is where it has been tested.** macOS binaries are unsigned and
  unnotarised (there is an `xattr` line in the README). Linux compiles and
  passes CI, with no QA against a running KiCad.
- **Parts are placed, not authored.** It searches and uses library symbols and
  footprints; creating new ones is on the roadmap.
- It is AGPL-3.0, and a fork of
  [mixelpixx/Konnect](https://github.com/mixelpixx/Konnect) v0.2.2 under the
  same licence.

## What would help

If you try it and the install does not work, that is the thing worth telling me.
There is a first-run report form in the repository's issues — six questions,
most of them one click — and it exists because nothing about this has been
measured on a machine that is not mine.

Repository, releases and the demo: `https://github.com/nevenfo/kicad-agentic-mcp`
