# Draft — r/KiCad

**Not posted.** Requirements and the go/no-go list are in
[`launch-kit.md`](launch-kit.md).

**Title:** One prompt, and KiCad says 0 unconnected items — an MCP plugin that
edits the running board editor

**Post type:** self-post with the before/after image pair
(`resources/images/demo-before.png`, `demo-after.png`). The image goes first;
this subreddit scrolls.

---

**Before / after, from one prompt:** three footprints imported from the
schematic and sitting in a heap → the two capacitors placed either side of the
regulator and the three nets routed in copper.

KiCad's own verdict on that, from `kicad-cli` afterwards: **5 unconnected items
before, 0 after**, eleven track segments, no errors. The starting board, the
prompt and the setup steps are committed in the repository, so this is
reproducible rather than illustrative — I ran it twice and got the same circuit
with different coordinates, which is exactly what should happen.

**What it is:** KiCad Agentic MCP, a native KiCad 10 plugin — one Rust binary —
that gives Claude and other MCP clients 202 tools for schematic capture, layout,
routing, ERC/DRC and manufacturing output. PCB edits go through KiCad's IPC API
into the *running* editor, so you watch them happen and Ctrl+Z undoes them.

**The part I care about most:** when it says the board passes, that verdict came
from `kicad-cli`, not from the model. A check that could not run is reported as
an error, never as a clean board.

**Speed, both numbers, because only one of them flatters me:** the board changes
land in under a second; the prompt around them takes six to seven minutes,
because the model routes one segment per turn. The product is fast. The
conversation is not.

**What I am not claiming:** no success rate beyond the six golden tasks in the
repository's benchmark (18/18, one machine, three repeats, fully described).
PCB tools need KiCad running with the API on. Windows is where this has been
tested; macOS binaries are unsigned; Linux compiles and passes CI with no QA
against a running KiCad. Parts are placed, not authored. AGPL-3.0, forked from
mixelpixx/Konnect v0.2.2 under the same licence.

If you try it and the install fails, there is a first-run report form in the
issues — six questions, two minutes. It is the only way any of this gets
measured on a machine that is not mine.

`https://github.com/nevenfo/kicad-agentic-mcp`
