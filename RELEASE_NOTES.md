# KiCad Agentic MCP v1.1.0

Correctness release. No new tool and no architecture change: the surface is
still **202 tools across 22 toolsets**. What moved is fidelity — what this
server writes into a KiCad file, and what it reports back about one — plus the
release gate that now proves it on CI's machine rather than on a developer's.

The measured figures further down were taken on 2026-08-24 for v1.0.0, on the
machine named at the top of [docs/benchmark.md](docs/benchmark.md), from
artefacts committed under `bench/results/`. This release did not re-run the
benchmark, so those numbers describe v1.0.0 and are reproduced unchanged. Where
a target was missed, it says so and the target is not moved.

## What changed in v1.1.0

Four fixes change a behaviour you can observe from a client:

- **`create_netclass` and `assign_net_to_class` write the project file, not the
  board.** They now edit the sibling `.kicad_pro` (`net_settings`,
  `netclass_patterns`). At v1.0.0 they inserted a `(netclass …)` node into the
  `.kicad_pcb`, which makes `kicad-cli` exit 3 — the board became unreadable to
  KiCad's own tooling.
- **`run_drc` no longer reports a clean board that is not routed.** It reads
  `unconnected_items` and `schematic_parity` alongside `violations`, and it
  reads each violation's position where KiCad actually writes it. A board with
  unrouted copper is now refused by the evidence gate that used to approve it.
- **Power symbols are part of the schematic net graph.** Netlists that
  previously reported a supply pin as disconnected now resolve it.
- **`register_footprint_library` and `register_symbol_library` answer what they
  did**: `result` is `inserted`, `unchanged` or `updated`, and a nickname
  already registered against a different URI is refused unless
  `replace_existing` is set.

Fidelity fixes, none of which change an API:

- `(paper …)` keeps every argument KiCad wrote after the page-size name, so a
  custom page size and the portrait flag survive a load/write cycle.
- `(lib_name …)` is preserved, and a derived symbol resolves its pins the way
  KiCad resolves them.
- A tool that writes a **connection point** — wire, junction, no-connect, stub —
  leaves a placed pin's coordinate untouched instead of snapping it to the grid;
  7.6 % of the pins in KiCad's own demo corpus do not survive that snap.
- Line endings, indentation and coordinate precision are properties of the file
  being edited rather than of this writer.
- The parser reports failure on a document it could not consume. It previously
  returned success on a truncated file.
- `add_layer` allocates a layer id from the canonical name under the board's own
  numbering, rather than an id that need not match it.

Infrastructure:

- **A red real-KiCad E2E now has no path to a published release** — the suite is
  a required gate of the release workflow instead of a signal running beside it.
- The test suite proves itself on a machine that has never had KiCad installed,
  which is what CI is. Three of its assertions were measuring this developer's
  machine and passed for that reason.

## What this is

An MCP server that lets Claude and other AI assistants design KiCad 10
schematics and PCBs — and, on top of that, an **agentic control layer**: a large
internal capability surface behind a small external one, a deterministic engine
for everything that does not need generative reasoning, task state and evidence
held outside any model's context, and a verdict that comes from KiCad rather
than from an agent's opinion.

It is a fork of [mixelpixx/Konnect](https://github.com/mixelpixx/Konnect) v0.2.2
(commit `5cd6454`), under the same AGPL-3.0-only licence. The binary is still
called `konnect`, so an existing MCP client configuration keeps working.

## What changed against base Konnect

Base Konnect is a router over a tool catalogue: the client loads toolsets and
calls tools by name. That still works, unchanged. What this fork adds is a
second way in, and the machinery behind it:

- **An MCP gateway** — `kicad_describe` / `kicad_invoke`. Tools are called
  without ever appearing in `tools/list`, so the catalogue refresh a router
  forces on the client (`notifications/tools/list_changed`) disappears from the
  bill entirely.
- **A Plan IR with a deterministic executor** — a typed, reference-checked plan
  is compiled and refused before the first mutation, then applied as one batch
  with rollback (`preview_plan` / `apply_plan`).
- **Evidence and handles** — a semantic diff is on by default, snapshots and
  diffs are addressable MCP resources (`kicad://snapshot/N`), and no mutation
  lands without an audit record.
- **Task state outside the context** — objective, constraints, established
  facts and failures are filed server-side and survive a compaction.
- **A world model** — an indexed project graph with a query and neighbour
  language, cheaper than dumping the design.
- **State safety primitives** — revisions with `base_revisions` optimistic
  concurrency, an idempotency ledger, transactions and a rollback journal.
- **A local-model runtime** — the caller states an objective, a local model on
  loopback writes the Plan IR, and the server compiles, applies and verifies it.
  The verdict is `kicad-cli`'s.
- **Tool annotations and capability metadata** — every tool declares its
  read/write character, and advisory analysis says so where a model reads it.

The tool surface itself grew to **202 tools across 22 toolsets** plus 13
meta-tools — 215 served by the catalogue.

## Measured results

Baseline (upstream v0.2.2 at `5cd6454`) and this fork ran back to back on
2026-08-24, seven golden tasks × 5 repeats, 35 runs each.

| | Baseline | This fork |
|---|---|---|
| success | 35/35 | 35/35 |
| MCP calls per task (median) | 11 | 4 |
| external tokens per task (median) | 14 337 | **2 249** (−84.3 %) |
| wall clock p50 | 77 ms | 86 ms |
| capability coverage (frozen 186-tool denominator) | 22.6 % | **72.6 %** |

- **Startup surface**: 21 tools / 2 831 tokens, against a full catalogue of 215
  tools / 33 183 tokens. Through the gateway, `tools/list` never changes at all.
- **Agent mode**: 2 MCP round trips per attempt — `start_task` and
  `kicad_agent`. The plan is compiled, applied and verified server-side; the
  caller sees no intermediate round trip.
- **Retrieval**: 62.0 % precision @8 with 100 % recall @8.
- **Binary**: 23.7 MB on Windows, unstripped — measured on the binary this
  release actually published, up 1.9 MB from v1.0.0's 21.8 MB. There is no
  `[profile.release]`,
  deliberately — adding `strip`/`lto` would change the code generation under
  every artefact the gate and the benchmarks were measured on, to improve a
  number nothing is gated on.
- Success is never judged from a model's prose. Assertions run KiCad's own ERC
  through `kicad-cli` or read the design back through the query tools.

## Trade-offs and missed criteria

Three V1 criteria were missed and one is not claimed. None of them was moved to
match the result, and no win is netted off against them.

| Criterion | Target | Measured | Verdict |
|---|---|---|---|
| `WALL_CLOCK_P50` ≤ baseline | ≤ 77 ms | 86 ms | **missed by 9 ms** |
| external tokens per task | ≤ 2 000 | 2 249 | **missed by 249** |
| `tools/list` at startup | ≤ ~1 000 | 2 831 | **missed** |
| `LLM_CALLS_PER_SUCCESSFUL_TASK` | materially below baseline | 15 → 5.5 inside the model-fit harness | **not claimed** — no baseline for this metric was ever measured |

- The fork is **slower where it guarantees something**: `recovery`, the task
  built to exercise the transaction journal, the snapshot manifest and the
  evidence store, costs +109 ms; `sch_inspection`, where there is nothing to
  guarantee, goes 14 → 6 ms. The direction is stable across samples, the
  magnitude is not.
- The 249 tokens over budget are deliberate trades: the semantic diff on by
  default, task filing, verification, and the snapshot handle at +18.
- The startup surface is only reachable by retiring the toolset-loading path,
  which would break every shipped skill. The cheaper shape was measured and
  rejected: dropping `openWorldHint` from read tools saves 78 of 342 tokens and
  would assert the MCP *open world* default about every read tool to save 2.8 %.
- **Success rate is equal, not ahead.** A scripted route succeeds by
  construction on both servers; the fork's margin is in what the route costs and
  in what happens when nobody scripts it.

## Known limitations

- **PCB tools need KiCad running** with the IPC API enabled and the board open —
  pcbnew has no headless mode, so a desktop session is required (a human is
  not). The benchmark's golden suite therefore covers the schematic and export
  paths only; the live PCB path is gated by the `live-ipc` job of the separate
  `E2E (real KiCAD)` workflow, and locally by `scripts/live-pcb-e2e.ps1`.
- **Windows is the most-tested platform.** macOS works from the release binaries
  or a source build and is not code-signed or notarised. Linux compiles and
  passes CI but has had no per-platform QA against a running KiCad.
- **The agent-mode success rate is not claimed from this release's runs** — two
  designs, one run each. The rate for the local model lives in the model-fit
  section of `docs/benchmark.md`, where it was measured with 60 attempts per arm.
- The evidence store holds 64 entries. Deepening it is deliberately deferred
  until a real session needs it (plan item D.5.3); no measured workload has
  wanted more than 32 batches of history.
- Not measured: KV-cache peak broken out of VRAM, backend prefix-cache hit
  rates, and a clean before/after pair for `qwen3.5-9b`.

## KiCad 10 status

KiCad 10.0.3 is the ground truth this release is built against, and the access
strategy is fixed by what KiCad 10 actually offers, not by preference:

- **PCB over IPC** (NNG + protobuf) — coverage is complete there.
- **Schematic over the S-expression engine** — schematic IPC is empty on 10.0:
  `schematic_commands.proto` declares no commands and `getItemFromDocument()`
  returns `nullopt`. This is not a workaround, it is the only path that exists.
- **Validation and export over `kicad-cli`** — ERC, DRC, Gerber, drill, BOM,
  pick-and-place, PDF, 3D.
- KiCad's IPC API is **disabled by default** and has no protocol version, no
  async events and no pub/sub; the server is single-threaded on the UI thread.
  The event journal is therefore this project's own (revisions + targeted
  diffing + file watching), and no push notification is advertised.

**The schematic IPC path is re-evaluated at KiCad 11, not before.**
`kicad-python` 0.8.0 and `kicad-cli api-server` target KiCad 11; that is
upstream work, and forking KiCad 10 to get there was rejected. The decision to
keep the S-expression engine or move the schematic path to IPC is plan item I.1
and stays open until KiCad 11 can be measured here.

## Getting started

- **Install**: KiCad 10 → Plugin and Content Manager → *Install from File* with
  `konnect-pcm-v1.1.0-<platform>.zip` from this release, or use the standalone
  server binary. Full steps, including the Claude Desktop and Claude Code
  configuration, are in [README.md](README.md).
- **macOS: the binaries are not signed or notarised.** Gatekeeper will refuse
  them on first launch, and the PCM package is not exempt. Clear the quarantine
  attribute after installing — `xattr -dr com.apple.quarantine <path>` on the
  extracted `konnect` binary, or on the plugin folder KiCad installed it into —
  or approve it under *System Settings → Privacy & Security*. This needs an
  Apple Developer account to fix properly and has not changed since v1.0.0.
- **Build from source**: `cargo build --release -p konnect` (needs `protoc` and
  `cmake`).
- **Reproduce the numbers**: `.\gate.ps1 -Bench`, or the individual runs listed
  under *Reproducing* in [docs/benchmark.md](docs/benchmark.md).
  `python bench/m1_table.py` regenerates every table in the M.1 section from the
  committed artefacts without running or spending anything.
- **Contribute or navigate the code**: [DEV.md](DEV.md) (architecture, the agent
  layer, build requirements), [tool-directory.md](tool-directory.md) (every
  tool), [CONTRIBUTING.md](CONTRIBUTING.md).
- **Tell us how far you got**:
  [file a first-run report](https://github.com/nevenfo/kicad-agentic-mcp/issues/new?template=first-run.yml)
  — six questions, about two minutes, and worth filing especially if you gave
  up. There is no telemetry in this binary and none is planned, so a report you
  write is the only thing that ever reaches us. The tally lives in
  [docs/adoption.md](docs/adoption.md).

## Licence

AGPL-3.0-only, workspace-wide — see [LICENSE](LICENSE). The generic `kam-*`
crates (state, evidence, plan, graph, context, llm, runtime) are clean-room and
`MIT OR Apache-2.0` in their own manifests. Commercial licensing:
[COMMERCIAL.md](COMMERCIAL.md).
