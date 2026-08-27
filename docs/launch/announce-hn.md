# Draft — Hacker News, Show HN

**Not posted.** Requirements and the go/no-go list are in
[`launch-kit.md`](launch-kit.md). Show HN expects the author in the thread for
the first hours; that is a go/no-go line, not a detail.

**Title:** Show HN: KiCad Agentic MCP – an MCP server whose verdicts come from
KiCad, not the model

**URL:** `https://github.com/nevenfo/kicad-agentic-mcp`

---

**Body (plain text; HN renders no images):**

I write PCB tooling, and the thing that kept bothering me about LLM-driven CAD
is not that the model gets things wrong. It is that it reports success it cannot
know it had. So this MCP server does not decide whether a board is good. KiCad
does: `kicad-cli` runs the ERC or the DRC, and a check that could not run comes
back as an error rather than as a clean board. That rule has a test behind it,
and it is the reason the project exists in its current shape.

It is a native KiCad 10 plugin — one Rust binary — exposing 202 tools across 22
toolsets over MCP: schematic capture, PCB layout and routing through KiCad's IPC
API into the running editor (so undo works), ERC/DRC and audits, and the
manufacturing export pipeline.

Two measurements I would rather show than assert.

**Tool-surface cost.** Loading a 200-tool catalogue into every request is the
obvious way to build this and it is expensive. Routing through two meta-tools
took external tokens per task from 12 373 to 1 995 (-83.9 %) with the success
rate unchanged at 18/18 and median MCP calls from 11 to 4. That was measured on
v1.0.0, on one machine, from artefacts committed in the repository; neither
v1.1.0 nor v1.1.1 re-ran it, and the release notes say so.

**A demo you can run.** The starting board, the prompt and the setup are
committed. One prompt places two capacitors within 5 mm of a regulator and
routes three nets; KiCad reports 5 unconnected items before and 0 after, eleven
segments, no errors. I ran it twice from the same starting state and got the
same circuit at different coordinates. Timing, both halves: the board changes
take under a second of server time; the prompt around them takes six to seven
minutes, because the model routes one segment per turn. There is no batch
"route this net" call yet, and that is the honest reason for the six minutes.

What I am not claiming: no success rate beyond those six golden tasks; no
platform claim beyond Windows (macOS binaries are unsigned, Linux compiles and
passes CI with no QA against a running KiCad); parts are placed, not authored;
PCB tools need KiCad running with its API enabled, because pcbnew has no
headless path.

AGPL-3.0. It is a fork of mixelpixx/Konnect v0.2.2 under the same licence, and
what the fork adds — a gateway, a plan IR with a deterministic executor,
evidence handles, task state — is spelled out in the README rather than blurred.

If you try it and the install fails, that is the most useful thing you can tell
me; there is a six-question first-run form in the issues, and nothing here has
been measured yet on a machine that is not mine.
