# Ready to post

Prepared on 2026-08-27 for manual publication after the validated `v1.1.1`
release. Nothing in this file has been posted or submitted.

## 1. KiCad forum

**Recommended title**

```text
KiCad Agentic MCP — an AI assistant that edits your board through KiCad's own API, and lets KiCad decide whether the result is good
```

**Final text**

```markdown
This audience is wary of a language model writing to their files, and it is right to be. So the first thing worth saying about this plugin is what it does *not* do: it does not rewrite your `.kicad_pcb` behind KiCad's back.

**KiCad Agentic MCP** is a native KiCad 10 plugin — one Rust binary — that exposes 202 tools over the [Model Context Protocol](https://modelcontextprotocol.io), so Claude, or another MCP client, can work your project directly:

- place and wire schematic parts, by pin name;
- place, move, rotate and route footprints in the running PCB editor, over KiCad's IPC API, so **Ctrl+Z walks the changes back** the way it does for anything you did by hand;
- run ERC and DRC, decoupling and power-rail audits, connectivity checks;
- produce Gerbers, drill, BOM, pick-and-place, 3D models and PDF.

**Verification is KiCad's, not the model's.** When the assistant says a board passes, that verdict came from `kicad-cli`. A check that could not run is reported as an error, never as a clean board — that rule is a test in the suite, not a good intention.

## What one prompt actually does

There is a [demo committed in the repository](https://github.com/nevenfo/kicad-agentic-mcp/tree/agentic/main/examples/demo) — the starting board, the exact prompt and the setup steps — so you can run the same thing rather than take a screenshot's word for it. A regulator and two capacitors arrive from the schematic sitting in a heap, with five missing connections. One prompt asks for the capacitors placed within 5 mm of the regulator and the three nets routed.

Run twice from the same starting state, on two different days: both times KiCad reported **5 unconnected items before, 0 after**, eleven track segments, and no errors — and both times the coordinates were different, because a model does not place a capacitor twice in the same spot. Silkscreen warnings remained, and the run explained why rather than hiding them.

Two honest numbers about speed: the board changes themselves — both placements, eleven traces, the saves — take **under a second**. The prompt around them takes **six to seven minutes**, because that is how long the model takes to look, decide, and route one segment per turn. The product operation is fast; the conversation is not.

## What it costs to install

Download the `konnect-pcm-v1.1.1-*` zip for your OS from the [v1.1.1 release](https://github.com/nevenfo/kicad-agentic-mcp/releases/tag/v1.1.1) and install it through KiCad's Plugin and Content Manager with *Install from File* — it is not in KiCad's official addon repository, so there is no repository URL to add. Then enable the KiCad API in *Preferences → Plugins*; it ships off, and every PCB tool needs it. The README's Quick start is five steps, and it was walked end to end on a machine that had never had this plugin installed, with every friction written down.

## What I am not claiming

- **Not a success rate for your boards.** The benchmark in the repository is six golden tasks with three repeats on one machine — 18/18 — and it is described there in full. It says nothing about your parts or your prompt. The token benchmark was measured on v1.0.0, not v1.1.1.
- **PCB tools need KiCad running**, with the API enabled and the board open. pcbnew has no headless path, so neither does this.
- **Windows is where live use has been tested most.** macOS binaries are unsigned and unnotarised. Linux compiles and passes CI, with no QA against a running KiCad.
- **Parts are placed, not authored.** It searches and uses library symbols and footprints; creating new ones is on the roadmap.
- It is AGPL-3.0, and a fork of [mixelpixx/Konnect](https://github.com/mixelpixx/Konnect) v0.2.2 under the same licence.

If you try it and the install does not work, that is the thing worth telling me. There is a [six-question first-run report](https://github.com/nevenfo/kicad-agentic-mcp/issues/new?template=first-run.yml), most of it one click, because nothing about this has yet been measured from an external first run.

Repository: https://github.com/nevenfo/kicad-agentic-mcp
```

**Image / demo:** insert `resources/images/demo-before.png` and
`resources/images/demo-after.png` side by side. Link the reproducible demo in
`examples/demo/`.

## 2. Reddit r/KiCad

**Recommended title**

```text
One prompt, and KiCad says 0 unconnected items — an MCP plugin that edits the running board editor
```

**Final text**

```markdown
**Before / after, from one prompt:** three footprints imported from the schematic and sitting in a heap → the two capacitors placed either side of the regulator and the three nets routed in copper.

KiCad's own verdict on that, from `kicad-cli` afterwards: **5 unconnected items before, 0 after**, eleven track segments, no errors. The [starting board, prompt and setup steps](https://github.com/nevenfo/kicad-agentic-mcp/tree/agentic/main/examples/demo) are committed, so this is reproducible rather than illustrative — I ran it twice and got the same circuit with different coordinates, which is exactly what should happen.

**What it is:** KiCad Agentic MCP v1.1.1, a native KiCad 10 plugin — one Rust binary — that gives Claude and other MCP clients 202 tools for schematic capture, layout, routing, ERC/DRC and manufacturing output. PCB edits go through KiCad's IPC API into the *running* editor, so you watch them happen and Ctrl+Z undoes them.

**The part I care about most:** when it says the board passes, that verdict came from `kicad-cli`, not from the model. A check that could not run is reported as an error, never as a clean board.

**Speed, both numbers:** the board changes land in under a second; the prompt around them takes six to seven minutes, because the model routes one segment per turn. The product operation is fast. The conversation is not.

**What I am not claiming:** no success rate beyond the six golden tasks in the repository's benchmark (18/18, one machine, three repeats, fully described). Those token and success measurements are from v1.0.0, not v1.1.1. PCB tools need KiCad running with the API on and the board open. Windows is where live use has been tested most; macOS binaries are unsigned; Linux compiles and passes CI with no QA against a running KiCad. Parts are placed, not authored. It is AGPL-3.0 and forked from mixelpixx/Konnect v0.2.2 under the same licence.

If you try it and the install fails, there is a [six-question first-run report](https://github.com/nevenfo/kicad-agentic-mcp/issues/new?template=first-run.yml). It is the only way any of this gets measured on a machine that is not mine.

Repository and v1.1.1 release: https://github.com/nevenfo/kicad-agentic-mcp
```

**Image / demo:** make the before/after pair the first media. Prefer both PNGs
from `resources/images/`; if Reddit accepts only one lead image, use
`demo-after.png` and keep the demo link in the first paragraph.

## 3. Show HN

**Recommended title**

```text
Show HN: KiCad Agentic MCP – an MCP server whose verdicts come from KiCad, not the model
```

**Submission URL**

```text
https://github.com/nevenfo/kicad-agentic-mcp
```

**Final text / first comment**

```text
I write PCB tooling, and the thing that kept bothering me about LLM-driven CAD is not that the model gets things wrong. It is that it reports success it cannot know it had. So this MCP server does not decide whether a board is good. KiCad does: kicad-cli runs the ERC or DRC, and a check that could not run comes back as an error rather than as a clean board. That rule has a test behind it.

It is a native KiCad 10 plugin — one Rust binary — exposing 202 tools across 22 toolsets over MCP: schematic capture, PCB layout and routing through KiCad's IPC API into the running editor (so undo works), ERC/DRC and audits, and manufacturing exports.

Two measurements I would rather show than assert.

Tool-surface cost: routing through two meta-tools took external tokens per task from 12,373 to 1,995 (-83.9%), with the success rate unchanged at 18/18 and median MCP calls from 11 to 4. This was measured on v1.0.0, on one machine, from committed artefacts. Neither v1.1.0 nor v1.1.1 re-ran that benchmark.

Demo: the starting board, exact prompt and setup are committed. One prompt places two capacitors within 5 mm of a regulator and routes three nets; KiCad reports 5 unconnected items before and 0 after, eleven segments, no errors. I ran it twice from the same state and got the same circuit at different coordinates. The board changes take under a second of server time; the prompt around them takes six to seven minutes because the model routes one segment per turn.

What I am not claiming: no success rate beyond those six golden tasks; no claim that this covers arbitrary KiCad projects; no platform claim beyond Windows live validation. macOS binaries are unsigned, and Linux compiles and passes CI but has no QA against a running KiCad. Parts are placed, not authored. PCB tools need KiCad running with its API enabled and the board open because pcbnew has no headless path.

AGPL-3.0. It is a fork of mixelpixx/Konnect v0.2.2 under the same licence. The fork adds a gateway, a plan IR with a deterministic executor, evidence handles and task state.

If you try it and the install fails, that is the most useful thing you can tell me. There is a six-question first-run form in the repository, and no external first-run report has been recorded yet.
```

**Image / demo:** HN does not render an image in the post. The repository URL
opens on the README before/after pair; keep `examples/demo/` reproducible and
easy to find. Submit only when available to answer the thread for several hours.

## 4. MCP directory

**Recommended title / name**

```text
KiCad Agentic MCP
```

**Final short description**

```text
Native KiCad 10 plugin — one Rust binary — giving MCP clients 202 tools for schematics, PCB layout, routing, ERC/DRC and manufacturing output. KiCad itself verifies the result.
```

**Final long description**

```markdown
**KiCad Agentic MCP** is a native KiCad 10 plugin that exposes 202 tools across 22 on-demand toolsets over the Model Context Protocol. An MCP client can place and wire schematic parts by pin name, place and route footprints in the running PCB editor through KiCad's IPC API so KiCad's own undo applies, run ERC, DRC, decoupling and power-rail audits, search a local JLCPCB parts catalogue, and produce Gerbers, drill files, BOM, pick-and-place, 3D models and PDF.

Verification is KiCad's: `kicad-cli` returns the verdict, and a check that could not run is reported as an error rather than as a clean board.

Routing through two meta-tools measured 1,995 external tokens per task against 12,373 for the equivalent flat surface (-83.9%), with the success rate unchanged on the repository's six-task golden suite. Those figures were measured on v1.0.0, on one machine, and were not re-run for v1.1.1.

Requirements: KiCad 10. PCB tools need KiCad running with its API enabled and the board open; pcbnew has no headless path. Windows is the most-tested platform. macOS binaries are unsigned and unnotarised. Linux compiles and passes CI but has had no QA against a running KiCad. Parts are placed, not authored. Licence: AGPL-3.0. Fork of [mixelpixx/Konnect](https://github.com/mixelpixx/Konnect) v0.2.2 under the same licence.
```

**One-line list entry**

```markdown
- [KiCad Agentic MCP](https://github.com/nevenfo/kicad-agentic-mcp) — Native KiCad 10 plugin exposing 202 MCP tools for schematic capture, PCB layout and routing, ERC/DRC and manufacturing output; verification comes from `kicad-cli` rather than the model. AGPL-3.0.
```

**Directory fields:** repository
`https://github.com/nevenfo/kicad-agentic-mcp`; release
`https://github.com/nevenfo/kicad-agentic-mcp/releases/tag/v1.1.1`; language
`Rust`; transport `stdio` (`HTTP` also available); scope `local`; licence
`AGPL-3.0`; requires `KiCad 10`, plus a running KiCad for PCB tools.

**Image / demo:** use `resources/images/demo-after.png` where a single image is
accepted; use the before/after pair where two images are supported. Check each
directory's current category, formatting and emoji rules before submitting.

## Links to insert

- Repository: https://github.com/nevenfo/kicad-agentic-mcp
- Latest release: https://github.com/nevenfo/kicad-agentic-mcp/releases/tag/v1.1.1
- Reproducible demo: https://github.com/nevenfo/kicad-agentic-mcp/tree/agentic/main/examples/demo
- Benchmark and its scope: https://github.com/nevenfo/kicad-agentic-mcp/blob/agentic/main/docs/benchmark.md
- First-run report: https://github.com/nevenfo/kicad-agentic-mcp/issues/new?template=first-run.yml
- Licence: https://github.com/nevenfo/kicad-agentic-mcp/blob/agentic/main/LICENSE
- Upstream attribution: https://github.com/mixelpixx/Konnect

## Caveats that must remain

- KiCad 10 is required; live validation used KiCad 10.0.3.
- PCB tools require a running KiCad, the API enabled and the board open.
- Windows is the most-tested live platform. macOS binaries are unsigned and
  unnotarised. Linux compiles and passes CI but has no QA against a running
  KiCad.
- The 12,373 → 1,995 token result, 18/18 golden result and 11 → 4 MCP-call
  result were measured on `v1.0.0`, not `v1.1.1`.
- The golden suite is six tasks, three repeats, on one machine. It is not a
  success-rate claim for arbitrary projects, parts or prompts.
- Parts and footprints are searched and placed, not authored.
- The project is not described as production-ready, universally compatible,
  officially endorsed by KiCad or the best KiCad MCP.
- It is AGPL-3.0 and a fork of `mixelpixx/Konnect` v0.2.2 under the same licence.
- Board changes take under a second of server time in the demo; the surrounding
  model conversation takes six to seven minutes.

## Recommended publication order

1. **KiCad forum.** Publish first to the audience best able to challenge the
   technical claims and installation path.
2. **r/KiCad.** Publish the before/after post after the forum post, ideally 12–24
   hours later so replies remain manageable.
3. **MCP directories.** Submit manually after the two audience posts; adapt the
   one-line entry to each directory's current format and licence fields.
4. **Show HN.** Publish last, preferably after at least one external first-run
   report exists, and only when available to answer comments for the first
   several hours.
