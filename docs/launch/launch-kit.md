# Launch kit

Everything needed to announce this project, drafted here so that publishing is a
single decision rather than a writing session. **Nothing in this kit has been
posted.** The repository metadata was applied on 2026-08-27; every external
post and submission remains the user's call, including which account posts.

Every factual claim in every draft traces to `docs/benchmark.md`,
`RELEASE_NOTES.md`, or a measurement made in R.1 (`first-run-walk.md`) or R.3
(`demo-run-2.md`, `demo-run-3.md`). Where a number belongs to a version other
than the current one, the draft says so.

## R.4.1 — Repository metadata, applied

Applied and verified on GitHub on 2026-08-27: the description, homepage and 12
topics below are public; the repository has 0 stars and 0 forks, and the latest
release is `v1.1.1`.

**Description** (350 char limit; this is 195):

> AI-assisted PCB design for KiCad 10. A native KiCad plugin — one Rust binary —
> that gives Claude and other MCP clients 202 tools for schematics, layout,
> routing, ERC/DRC and manufacturing output.

Why this rather than what is there now: the current description leads with
*agentic MCP coprocessor … deterministic planning, validation, rollback and
optional local LLMs*, which describes the architecture to someone who already
knows what the thing is. A stranger searching for KiCad automation reads the
first eight words and stops. The architecture is the second sentence's job, and
the README's.

**Homepage:** `https://github.com/nevenfo/kicad-agentic-mcp/releases/latest` —
the release page, because that is where a first-time user's path starts (R.1),
and the project has no site to point at instead.

**Topics** (GitHub allows 20; these are 12, all existing topics with real search
traffic):

```
kicad  kicad-plugin  pcb  pcb-design  eda  electronics
mcp  mcp-server  model-context-protocol  claude  llm  rust
```

The first six find the KiCad and hardware audience, the next five find the MCP
audience, and `rust` is what it is built in. `agent-skills` and `kicad-schematics`
from the upstream repository are dropped: the first has almost no reach, and the
second is covered by `kicad`.

**Not proposed here:** renaming the repository, changing the licence badge, or
touching the upstream repository in any way.

## R.4.2 — The pitches

**One sentence** (for a directory entry, a topic list, a tweet):

> A native KiCad 10 plugin that lets Claude and other MCP clients edit
> schematics and PCBs directly — with every result checked by KiCad itself, not
> by the model.

**One paragraph** (for a forum post's opening, a directory's long field):

> **KiCad Agentic MCP** is a native KiCad 10 plugin — a single Rust binary —
> that exposes 202 tools over the Model Context Protocol, so Claude and other
> MCP clients can place and wire schematic parts, lay out and route boards, run
> ERC and DRC, and produce manufacturing output. Verification is KiCad's:
> `kicad-cli` decides whether a board passes, and a check that could not run is
> reported as an error rather than as a clean board. It is AGPL-3.0, it is a
> fork of [mixelpixx/Konnect](https://github.com/mixelpixx/Konnect) v0.2.2 under
> the same licence, and it is honest about its edges: PCB tools need a running
> KiCad with the API enabled and the board open, macOS binaries are unsigned,
> and Linux compiles and passes CI but has had no QA against a running KiCad.

Both state the limitation set. A launch that hides the caveats buys a first wave
of users who leave angry, and this project's whole feedback route (R.5) is built
on the first wave telling the truth about what happened.

## R.4.3 — The drafts

One per venue, adapted rather than pasted — a KiCad forum and an MCP directory
do not want the same first sentence:

| Draft | Venue | First sentence is about |
|---|---|---|
| [`announce-kicad-forum.md`](announce-kicad-forum.md) | forum.kicad.info | what it does to *your* board, and that KiCad checks it |
| [`announce-reddit-kicad.md`](announce-reddit-kicad.md) | r/KiCad | the demo, shown before it is described |
| [`announce-hn.md`](announce-hn.md) | Hacker News, Show HN | the measured token cost and the verification stance |
| [`announce-mcp-directory.md`](announce-mcp-directory.md) | MCP directories and lists | the tool surface and the install path |

## R.4.4 — Venues, and what each one demands

Requirements are as understood on 2026-08-26 and are the kind of thing that
changes without notice. **Re-read the venue's own rules immediately before
posting** — that is one line of the go/no-go list, not an optional courtesy.

| Venue | Format | Account | Licence statement | Image | Notes |
|---|---|---|---|---|---|
| **forum.kicad.info** | Discourse post, markdown, in *Software Tools* | forum account with enough trust level to post links | expected in-post; AGPL-3.0 is fine | one image welcome, not required | Read-the-room venue: KiCad users are wary of AI writing to their files. The draft leads with *KiCad verifies it*, and says plainly what is not claimed |
| **r/KiCad** | Reddit self-post, markdown | Reddit account, some subs require age/karma | in-post link to the licence | image or short clip is what carries the post | Self-promotion rules vary; a maintainer posting their own free tool is normally fine when it is disclosed |
| **Hacker News** | `Show HN: …` title, plain text body, one URL | HN account | not required, but state it | HN renders no images; the README's image does the work after the click | Show HN asks for something people can try, and for the author to be present in the thread for the first hours |
| **MCP directories** (`awesome-mcp-servers`-style lists, `mcp.so`, `glama.ai`, PulseMCP and similar) | usually a PR adding one line, or a web form | GitHub account for the PR ones | required by most: name the licence | logo/screenshot optional | Each list has its own ordering and category rules; the PR ones reject on format, not on merit |
| **KiCad's official Plugin & Content Manager repository** | PCM metadata PR against KiCad's addon repository | GitHub account | AGPL-3.0 acceptable | icon required at the sizes PCM specifies | **Out of R's scope.** It remains a named candidate for the R.6 gate. The `v1.1.1` package metadata names this fork's author and homepage while keeping the existing PCM identifier, but no submission to KiCad's addon repository has been made |

## R.4.5 — What is not claimed

The kit says these in the drafts themselves, not only here:

- **No success-rate claim beyond the golden suite.** `docs/benchmark.md` reports
  18/18 on six golden tasks with three repeats, on one machine, in a harness
  that is described there in full. That is not a claim about your board, your
  parts, or your prompt.
- **The token figures are v1.0.0's.** 12 373 → 1 995 external tokens per task
  (−83.9 %), MCP calls 11 → 4, measured on 2026-08-24 from artefacts committed
  under `bench/results/`. Neither v1.1.0 nor v1.1.1 re-ran the benchmark, and
  `RELEASE_NOTES.md` says so.
- **No platform claim beyond Windows.** Windows 11 with KiCad 10 is where the
  first-run walk (R.1) and all three demo runs happened. macOS binaries are
  unsigned and unnotarised. Linux compiles and passes CI and has had no QA
  against a running KiCad.
- **No claim that it is fast end to end.** The demo's board changes land in
  under a second of product time; the prompt around them takes six to seven
  minutes of model conversation. Both numbers are published, in that order, in
  `README.md` and `examples/demo/README.md`.
- **No claim of official KiCad endorsement.** It is not in KiCad's addon
  repository, and the PCM listing is a future decision, not a fact.
- **No claim about symbol or footprint authoring.** Parts are searched and
  placed, not created.

## R.4.6 — Go / no-go

The release and repository metadata are complete. Every remaining external
post or submission is the user's decision, and the account that posts is the
user's.

- [x] **Apply the repository metadata** (R.4.1: description, homepage, topics).
      Applied and verified on 2026-08-27
- [x] **Ship v1.1.1 first** (R.7.7). Published and validated on 2026-08-26
- [ ] **Re-read each venue's current rules**, immediately before posting there
- [ ] **Decide the order.** These drafts assume the KiCad forum and r/KiCad
      first, Show HN once at least one outside first-run report has arrived, and
      the MCP directories at any time
- [ ] **Decide who is available.** Show HN in particular expects the author in
      the thread for the first hours; posting it into a day with no time for
      that wastes the one shot the title gets
- [ ] **Decide whether the demo image is enough**, or whether a short screen
      capture is worth making first (R.3.6 produced the before/after pair; no
      recording exists)
