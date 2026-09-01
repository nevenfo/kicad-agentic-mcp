# KiCad Agentic MCP v1.1.4

Capability release for three limitations the Hi-Fi benchmark demonstrated on
real work: a schematic KiCad had open could be written under it, a generated
courtyard could be smaller than the part it enclosed with no way to fix it from
the MCP, and `on_board` / `in_bom` / `dnp` were neither readable nor writable.
The architecture does not move — PCB over IPC, schematic over the controlled
S-expression engine, ERC/DRC and exports over `kicad-cli` — and the
`DocumentType` routing v1.1.3 fixed is untouched. One tool is added, so the
surface is now **203 tools across 22 toolsets**.

The benchmark and model-fit figures further down were taken on 2026-08-24 for
v1.0.0, on the machine named at the top of
[docs/benchmark.md](docs/benchmark.md), from artefacts committed under
`bench/results/`. This release did not re-run them, so those numbers describe
v1.0.0 and are reproduced unchanged. The separate Windows binary-size figure
was measured on v1.1.0 and is labelled as such. Where a target was missed, it
says so and the target is not moved.

## What changed in v1.1.4

- **A schematic KiCad owns is no longer written under it.** Opening
  `X.kicad_sch` in Eeschema creates a sibling `~X.kicad_sch.lck`; until v1.1.3
  a mutation went through anyway, and the editor's next save silently discarded
  it. Every schematic write path funnels through `konnect-sexp`'s writer or
  `commit_file_transaction`, so the guard sits there once, and is re-checked
  immediately before the `rename` that commits — an editor opened *during* the
  operation is refused too, not only one open when it started. A refusal leaves
  the file byte-identical, with no scratch file and no journal entry. It reaches
  the client as `error_kind: conflict`, naming the lock file so a human knows
  which editor to close.
- **The lock is never removed, moved, or judged stale.** KiCad's lock file
  holds exactly `{"hostname":…,"username":…}` — 50 bytes, no PID, no timestamp.
  Freshness is not decidable from it, so it is not decided: presence is refusal.
  The guard covers `.kicad_sch` only; the board goes through IPC into the
  running editor, where KiCad itself arbitrates.
- **Footprint graphics are editable.** The new `set_footprint_graphics` appends,
  replaces or deletes `fp_line`, `fp_arc`, `fp_rect`, `fp_circle` and `fp_poly`
  on one layer of a `.kicad_mod`, as a single revision-checked atomic
  replacement. It is an API typed by primitive, not a text editor: one layer per
  call, everything else carried through as it was. A primitive that a
  `(group …)` references is refused for replace and delete rather than silently
  dropped, which would leave KiCad a dangling reference. `get_footprint_info`
  now returns graphics in the shape `set_footprint_graphics` takes back.
- **A generated courtyard encloses the body, not just the pads.**
  `create_footprint` derived it from the pad envelope alone, so a part whose
  body overhangs its pads — `CF_Film_Box_P5.00mm_7.2x3.5mm` — got a courtyard
  smaller than itself. It is now the envelope of body **and** pads plus the
  clearance, aligned outward onto the KLC grid.
- **The pin-1 marker is declared, not guessed.** It is a client input, `true` by
  default, because the expensive mistake is a polarised part shipped without
  one. A non-polarised part is no longer given a marker it should not have, and
  the marker stays inside the courtyard —
  `Fuse_Schurter_UMT-H_5.3x16mm` had one outside it.
- **`on_board`, `in_bom` and `dnp` are read and written as the tags they are.**
  `get_schematic_component` and `list_schematic_components` always return the
  three: an absent tag is KiCad's default, not an undetermined field.
  `edit_schematic_component` and `batch_edit_schematic_components` take them as
  booleans and write them as symbol tags, never as custom properties — a
  `(property "dnp" "yes")` merely shows up in the field list and changes neither
  the netlist, nor the BOM, nor *Update PCB from schematic*. A missing tag is
  inserted where eeschema writes it, after `on_board` and before `uuid`, at the
  file's own indentation; a call addressed by reference reaches every unit of a
  multi-unit symbol; a non-boolean value is refused.
- **A field set to `null` removes the property.** There was no way to delete
  one, only to add or update. `fields: {"key": null}` now removes the whole
  `(property …)` block and its own lines with it — eight, for a property
  eeschema wrote — so the document reads as it did before the property existed.
  `Reference` and `Value` are refused, KiCad requires them, and deleting an
  absent property is reported rather than dressed up as a change.

Verified against real KiCad 10, not mocks. `gate.ps1` is green end to end (fmt,
`clippy -D warnings`, workspace tests, doctests, release build) over 1 318 new
lines of integration tests in `kicad_editor_lock.rs`, `footprint_graphics.rs`
and `symbol_attributes.rs`. Three live suites pass on a dedicated
`KICAD_CONFIG_HOME` — schematic `DocumentType` routing, PCB over IPC, and the
new lock refusal — and each Pareto fix was replayed on the Hi-Fi benchmark
through the MCP alone: both defective footprints repaired, and B2.8 lifted in a
single call. Hi-Fi ERC is unchanged either side of that edit, 0 errors and 15
warnings, the same as at gate C2.

This release adds one tool and removes no parameter. Existing calls keep their
meaning, with one deliberate exception: a `null` field value used to mean "a
value with no textual form" and now means deletion.

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

The tool surface itself grew to **203 tools across 22 toolsets** plus 13
meta-tools — 216 served by the catalogue.

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
  tools / 33 183 tokens — the surface as it stood on the measurement date, one
  tool short of today's. Through the gateway, `tools/list` never changes at all.
- **Agent mode**: 2 MCP round trips per attempt — `start_task` and
  `kicad_agent`. The plan is compiled, applied and verified server-side; the
  caller sees no intermediate round trip.
- **Retrieval**: 62.0 % precision @8 with 100 % recall @8.
- **Binary**: 23.7 MB on Windows, unstripped — measured on the v1.1.0 binary,
  up 1.9 MB from v1.0.0's 21.8 MB. v1.1.2 did not re-measure it. There is no
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
- **The agent-mode success rate is not claimed from v1.0.0's runs** — two
  designs, one run each. The rate for the local model lives in the model-fit
  section of `docs/benchmark.md`, where it was measured with 60 attempts per arm.
- The evidence store holds 64 entries. Deepening it is deliberately deferred
  until a real session needs it (plan item D.5.3); no measured workload has
  wanted more than 32 batches of history.
- Not measured: KV-cache peak broken out of VRAM, backend prefix-cache hit
  rates, and a clean before/after pair for `qwen3.5-9b`.

## KiCad 10 status

KiCad 10.0.3 is the ground truth this release is built against; the live
schematic and PCB suites in this release were run against 10.0.6 as well. The
access strategy is fixed by what KiCad 10 actually offers, not by preference:

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
  `konnect-pcm-v1.1.4-<platform>.zip` from this release, or use the standalone
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
