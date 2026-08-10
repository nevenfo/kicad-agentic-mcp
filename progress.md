# progress.md — KiCad Agentic MCP

Append-only working log. Error history is never deleted.

---

## GOAL

Turn `mixelpixx/Konnect` into a KiCad **agentic control layer**: large internal
capability surface, small external MCP surface, local LLM agents that absorb
operational work, a deterministic engine for everything that does not need
generative reasoning, task state and evidence outside the LLM context, and
verification that comes from KiCad rather than from the agent's own opinion.

## CURRENT PHASE

**E** — world model / task state / evidence. A (bootstrap), B (cartography),
C (baseline benchmark) and F (compact surface) are done; D shipped revisions,
idempotency, transactional batches and the error catalog. Semantic diff is the
first Phase E item and it is in. Evidence handles, Task State and ProjectGraph
are not.

## CURRENT TASK

A batch now says what it changed in the vocabulary of the design
(`crates/kam-evidence`, `crates/konnect-core/src/evidence/`):

* `kam-evidence` — clean-room, MIT OR Apache-2.0, knows nothing about KiCAD.
  Documents reduce to an `ItemSet` (kind, stable key, label, attributes); the
  diff matches by key and reports attribute differences, so re-serialisation
  noise is removed structurally rather than filtered afterwards.
* `konnect-core::evidence` — the KiCAD half: `.kicad_sch` and `.kicad_pcb` to
  items, keyed on KiCAD's own UUIDs. Symbols, wires, buses, labels, junctions,
  sheets; footprints, tracks, vias, zones and nets, with pads counted rather
  than listed so a net reports `connections: +2`.
* `kicad_invoke` grew a `diff` argument — `none` / `summary` (default) /
  `changes`. The snapshot it already took for rollback is the before-image.

**2 158 external tokens/task (+6.1 % vs Phase D), 4 MCP calls, 18/18, 567
tests.** Startup surface 1 912 → 1 952, once per session.

Next in Phase E: evidence handles (`kicad://evidence/N`, `kicad://diff/N`), so
the detail lives outside the reply and the harness fetches it only to
challenge. Then the Task State Manager, then Plan IR.

---

## TODO

```
[x] Étape 0 — progress.md and plan.md
[x] Gate 0 — compare Konnect / kicad-mcp-pro / legacy KiCAD-MCP-Server
[x] Verify licences, choose the base officially            -> Konnect, AGPL-3.0-only
[x] Clone the base into a clean workspace                  -> konnect-agentic, branch agentic/main
[x] Read architecture / CI / tests / manifests
[x] Run the base's build + tests                           -> 469 green
[x] Build the initial benchmark                            -> bench/, 6 golden tasks, 3 load modes
[x] Measure baseline tokens / calls / latency / success    -> docs/benchmark.md
[x] Reduce the external MCP surface (first two levers)     -> -70.1 % external tokens
[x] Optimise progressive disclosure / tool retrieval       -> done + measured; retrieval is the weak link
[x] Compress heavy tool schemas + shrink the starter kit    -> 3 698 -> 3 197 tk/task, startup 1 958 -> 1 454
[x] Compact gateway (kicad_describe + kicad_invoke)         -> 1 995 tk/task, 4 calls, CATALOG_TOKENS = 0
[ ] Map the scope gaps (capability matrix)
[x] Revisions + optimistic concurrency (base_revisions)     -> content-addressed, kam-state
[x] Transactions / rollback / idempotency at the MCP layer  -> kicad_invoke, 2 033 tk/task, 18/18
[x] Error catalog (TransientClass, stable io codes)         -> E9 closed, E11 stays fixed
[ ] Stable IDs (UUID-addressed items, not path+coordinates)
[ ] Snapshots as first-class handles (kicad://snapshot/N)
[x] Semantic diff                                           -> kam-evidence + konnect-core::evidence, 2 158 tk/task
[ ] ProjectGraph / World Model
[ ] Task State Manager
[ ] Context Manager + Attention Manager (ACTIVE TASK anchor)
[ ] Handles / resources / evidence packs
[ ] Plan IR + deterministic executor + batching
[ ] Direct mode / Agent mode split
[ ] Local model provider, hardware probe, model benchmark, router
[ ] Independent verification, error catalog, retries, recovery
[ ] Event journal / deltas
[ ] Custom KiCad gate  (default: NO — see D3)
[ ] Multi-harness tests (Claude Code / Codex / AGY)
[ ] Hardening + failure injection
[ ] Final benchmark + comparison table
```

---

## DECISIONS

### D1 — Base = Konnect (2026-08-10)

Recorded in `plan.md` § Gate 0 with measurements. Only Rust candidate, single
process, already had progressive disclosure, transactions, atomic writes,
observability and 469 green tests. `kicad-mcp-pro` is broader (380 tools,
2 852 tests, generated parity matrix) but its schematic writer depends on
`kicad-sch-api`, which drops `global_label` nodes on save; its profiles are
boot-time only; and it needs three build systems. The legacy server is the
TS→Python→SWIG chain Konnect exists to replace.

### D2 — Licence posture (2026-08-10)

AGPL-3.0-only, personal use, no distribution today → unblocked. Generic
subsystems go into new clean-room `kam-*` crates so a future re-licence does not
require rewriting them. MIT code from `kicad-mcp-pro` may be absorbed with its
notice; nothing flows the other way.

### D3 — No KiCad fork for schematic IPC (2026-08-10)

Verified against KiCad 10.0 sources: `schematic_commands.proto` declares no
commands, `eeschema/api/api_handler_sch.cpp` registers only `GetOpenDocuments`,
`getItemFromDocument()` returns `std::nullopt` (TODO). Schematic IPC is being
built upstream for **KiCad 11** (`kicad-python` 0.8.0, `kicad-cli api-server`).
Forking KiCad 10 would duplicate work against a moving upstream. Konnect's
S-expression engine stays the schematic path. Re-evaluate at KiCad 11.

### D4 — No async events exist in KiCad IPC (2026-08-10)

KiCad exposes only `KINNG_REQUEST_SERVER` (REQ/REP); there is no pub/sub. The
event journal and `changes_since(rev)` must be ours: internal revision counter,
targeted diffing, file watching. The MCP layer must not advertise push
notifications it cannot deliver.

### D5 — Keep both loading paths (2026-08-10)

`find_capabilities` + `load_tools` is the cheap path; `list_toolboxes` +
`load_toolset` is kept because sweeping a whole domain is sometimes right, and
because every existing skill and client uses it. Removing it would be a
breaking change for a benefit already obtained by adding the alternative.

### D6 — Plural stemming rejected on evidence (2026-08-10)

Implemented in `capability_search.rs`, measured, removed. It moved retrieval
recall at 8 results/query from 100 % to 98.2 % and helped at no limit. The
rejection is documented at the code site so it is not re-attempted.

### D7 — No `$defs` / `$ref` in `inputSchema` (2026-08-10)

`create_symbol` inlines the same pin-item object three times; a local `$ref`
would have removed ~400 tokens from it. Rejected after checking the whole chain,
not just the spec:

* **MCP 2026-07-28**: explicitly allows it. `inputSchema` is full JSON Schema
  2020-12, `$defs` is named in the composition-keyword section, and there is a
  dedicated `$ref` resolution section (local `$ref` fine; network `$ref` MUST
  NOT be auto-dereferenced).
* **Anthropic Messages API**: supported under `strict: true` with documented
  limits (no external `$ref`, no recursion, no `allOf` + `$ref`). **Undocumented
  for non-strict tools** — the mode Claude Code actually uses.
* **Client chain: not reliable.** `openai/codex` #3152 and #13746 (schema
  degraded to `{"request": string}` / arrays of strings), `gemini-cli` #13326
  (Gemini API rejects `$defs`), `mcpb` #174 (Claude Desktop on Windows fails to
  compile schemas with `$defs`), plus the same symptom in Kiro, mastra, n8n and
  autogen.

A tool schema the model receives mangled is worse than a fat one. Compression
therefore stays inside inlined schemas, which every client already handles. Note
that several of those issues recommend the opposite direction — servers should
*inline* their `$ref`s before exposing them — which is where this already is.

### D8 — `config` is no longer a starter toolset (2026-08-10)

7 tools, 625 tokens, re-sent on every `tools/list` refresh, and **zero** calls
across the whole golden suite. Split: `load_user_config` and
`get_effective_config` (118 tk) stay, admitted individually via a new
`STARTER_TOOLS` list; the five write / design-rule tools leave. Before shipping,
`find_capabilities` was checked on their own intents and ranks each removed tool
**first** ("remember that I always use JLCPCB" → `save_user_config`, "add a
design rule for decoupling" → `add_design_rule`). The behavioural cost is one
discovery call for a rarely-used feature; the saving is 507 tokens on every
refresh in every load mode.

### D9 — The gateway is two verbs, not seven (2026-08-10)

`plan.md` sketched ~7 external verbs (`kicad_status`, `kicad_capabilities`,
`kicad_delegate`, `kicad_query`, `kicad_verify`, `kicad_evidence`,
`kicad_history`). Shipped: **two**, `kicad_describe` and `kicad_invoke`. The
other five all describe *what* is being called, not *how*, and the registry
already answers that — `kicad_invoke("run_erc")` is a verify, and
`kicad_invoke("list_schematic_nets")` is a query. Adding verbs whose only
content is a category would have re-created the schema cost the gateway exists
to remove. `kicad_status`, `kicad_evidence` and `kicad_history` earn their own
verbs when there is state behind them (Phases D/E); until then
`server_stats` / `get_recent_calls` cover it.

Both loading paths stay (D5 still holds): `load_toolset` and `load_tools` are
what every shipped skill and existing client uses, and removing them would break
them for a saving already obtained by adding a third path.

### D10 — `atomic` defaults to `stop_on_error`, not to `true` (2026-08-10)

The first Phase D build rolled back any batch containing a failure. The
benchmark scored the `recovery` task **0/3**: it deliberately fails five calls
mid-batch with `stop_on_error: false`, and the design built by the remaining
calls — which the assertions check — was being thrown away.

A caller who passes `stop_on_error: false` has declared the calls independent
and the survivors wanted; undoing them is the opposite of the request. So
`atomic` follows `stop_on_error` unless set explicitly. Found by measurement,
not by review, which is the argument for running the suite before believing a
safety feature is safe.

### D11 — `kam-state` is MIT OR Apache-2.0 inside an AGPL fork (2026-08-10)

Plan.md's licence mitigation said generic subsystems must stay re-licensable.
`kam-state` is the first one, so it carries its own permissive licence and
depends on no `konnect-*` type — the rule is now enforced by the crate's
manifest rather than by intention. It knows nothing about KiCAD beyond a list of
file suffixes.

### D12 — Rollback is file-level, not KiCad's undo stack (2026-08-10)

The snapshot restores bytes on disk. That is a complete undo for the
S-expression tools, and **not** an undo for anything applied over IPC to a
running KiCAD, nor for a GUI holding the same file open. `base_revisions` is the
detection half of that gap: it refuses a batch whose document moved. When the
PCB/IPC path gains mutations, it needs `BeginCommit`/`EndCommit` (plan.md, KiCad
10 ground truth) rather than this.

### D13 — the diff is on by default, and that costs 6.1 % (2026-08-10)

`diff` could have defaulted to `none` and kept the per-task number at 2 033.
It does not, because "every change carries its proof" is a project rule and a
reply of `ok: 3` is not reviewable. The measured price is **+40 startup tokens**
(the schema property) and **~85 per task** (one summary line per mutating
batch). The ≤ 2 000 target is now missed by 158 and is recorded as missed.

The counter-argument that decided it: the alternative to 85 tokens of summary
is a harness re-reading the documents to find out what happened, which costs an
order of magnitude more. `diff: "none"` exists for a caller who disagrees, and
`diff: "changes"` buys the per-item detail.

### D14 — documents are items, not a file count (2026-08-10)

The first build reported `create_project` as `no design change`: three files
appeared and their *contents* — an empty schematic, an empty board — differ in
no item. The batch that changed the most was described as the one that changed
nothing.

Fixed by making the document itself an item, so a creation reads `document +3`
through the same diff engine rather than through a special case. A file that is
modified but has no extractor (`.kicad_pro`) is still counted as
`undescribed_files`, because guessing at its contents would be worse than
admitting the gap.

Found by `bench/probes/semantic_diff.yaml` on a real project, not by review —
the unit tests all passed, because none of them created a project.

### D15 — the diff engine is format-agnostic on purpose (2026-08-10)

`kam-evidence` could have parsed KiCAD S-expressions directly and been half the
code. It does not, for two reasons that both cost something now and pay later:
the licence rule (D11) keeps generic subsystems free of AGPL-derived code, and
the split means a second document format costs an extractor rather than a
second diff engine. The extractor lives in `konnect-core::evidence` where the
KiCAD knowledge already is.

---

## BENCHMARKS

Full detail and method: **`docs/benchmark.md`**. Headline:

| Metric | Konnect baseline | Fork, Phase F | Fork, Phase D | Fork, Phase E | Δ vs baseline |
|---|---|---|---|---|---|
| SUCCESS_RATE | 18/18 | 18/18 | 18/18 | **18/18** | = |
| EXTERNAL_TOKENS/task | 12 373 | 1 995 | 2 033 | **2 158** | **−82.6 %** |
| CATALOG_TOKENS/task | 8 389 | 0 | 0 | **0** | −100 % |
| MCP_CALLS median/task | 11 | 4 | 4 | **4** | −7 |
| WALL_CLOCK_P50 | 70 ms | 72 ms | 67 ms | 64 ms | ≈ |
| WALL_CLOCK_P95 | 888 ms | 916 ms | 911 ms | 870 ms | ≈ |
| `tools/list` at startup | 1 680 tk | 1 725 tk | 1 912 tk | 1 952 tk | +272 (once per session) |

Phase D bought preconditions, idempotency and rollback for **+38 tokens/task**
and **+187 startup tokens**. Phase E bought the semantic diff for **+125
tokens/task** (+40 of it once per session). Both move the startup number
further from its ≤ ~1 000 target and the per-task number further from ≤ 2 000;
both targets are recorded as missed rather than moved, and neither win is
netted off against them.

Intermediate `tools` mode sits at 3 197 tk/task and is kept measured, because it
is what a client that does not use the gateway pays.

Startup is 45 tokens above upstream while carrying **four** extra meta-tools;
the +278 regression from step 1 was repaid by the starter-kit work and then
partly re-spent on the gateway verbs, which pay for themselves after the first
task of any session.

Build/test baseline: `cargo build --release -p konnect` 81 s cold;
`cargo test --workspace --lib --tests` 469 → 487 → 525 → **567 passed, 0
failed** on the fork. `cargo fmt --check` and `cargo clippy --workspace --locked -D
warnings` are clean.

---

## ERROR HISTORY

### E1 — `protoc` missing (2026-08-10) — RESOLVED

`konnect-ipc/build.rs` needs `protoc` on `PATH` or `$PROTOC`; neither existed.
Installed `Google.Protobuf` via winget → `libprotoc 35.1`. winget's PATH edit
does not reach an already-open shell, so every build command in this project
sets `PROTOC` explicitly, and `gate.ps1` discovers it automatically.

### E2 — Konnect auto-installed into the user's `~/.claude` (2026-08-10) — NOTED

Running the built binary triggers `install::needs_install()` →
`run_install_silent()`, which wrote six skills (`kicad-*`, `konnect`) and two
agents into `C:\Users\FlowUP\.claude\` at 14:18:36. Gated by a `.installed`
marker so it will not repeat, but it is a side effect upstream performs without
asking. **Open action:** add a `KAM_NO_INSTALL=1` opt-out before any automation
runs the binary in a loop.

### E3 — Per-user KiCad installs were invisible (2026-08-10) — FIXED

```
Library 'Device' not found in the installed KiCAD symbol libraries (lib_id 'Device:R').
```

Every symbol lookup failed on a stock machine. `winget install KiCad.KiCad`
produces "KiCad 10.0 (current user)" under `%LOCALAPPDATA%\Programs\KiCad\10.0`,
and the hard-coded search list only knew `C:\KiCad` and `C:\Program Files\KiCad`.
Worse, the list was written **twice** — `konnect-core::tools::kicad_share_roots`
(knew KiCad 8) and `library::find_symbol_dirs` (stopped at 9) — and the copies
had drifted.

Fix: one module, `konnect-schematic-editor::kicad_paths`, used by both. Adds
`%LOCALAPPDATA%\Programs`, `%APPDATA%`, and `%ProgramFiles%` read from the
environment instead of hard-coded. Major-outer/prefix-inner ordering so a stale
`C:\KiCad\8.0` can never shadow a current per-user 10.0. Three tests, one of
which asserts the winget prefix is present.

### E4 — `run_erc` reported "0 errors" while doing nothing (2026-08-10) — FIXED IN HARNESS

```
Tool error: Failed to spawn kicad-cli: kicad-cli.exe
```

`kicad-cli` was not on `PATH`. The failure was visible, but on an empty
schematic the *next* run still returned `{"errors":0,"total":0}` — a benchmark
that scores a no-op as a pass. `bench/konnect.bench.toml` now pins the absolute
`kicad_cli` path for every run.

### E5 — Benchmark undercounted external tokens by ~8 000/task (2026-08-10) — FIXED

The bench client skipped `notifications/tools/list_changed` instead of
re-fetching `tools/list`, so `load_toolset`'s real cost was invisible. A real
client has no such choice. `bench/mcp_client.py` now refreshes on notification
and `CATALOG_TOKENS` is reported as its own line.

### E6 — Grid snapping is inconsistent between placement tools (2026-08-10) — OPEN

`add_schematic_component` / `batch_place_components` snap to the 1.27 mm grid
(100, 80 → 100.33, 80.01). `add_power_symbol` does **not** — it writes the
coordinate verbatim. Placing a power symbol at the same nominal coordinate as a
resistor therefore leaves it 0.33 mm off the pin, and KiCad ERC reports
`Pin not connected` for both. No tool errors; the schematic is simply wrong.

Observed on the first divider probe: 6 ERC errors, all from this. Not yet fixed
— the fix is to snap in one place for every placement tool, which belongs with
the Phase D geometry work rather than as a spot patch.

### E7 — Konnect's own connectivity analysis disagrees with KiCad ERC (2026-08-10) — OPEN

On the same broken schematic, `find_single_pin_nets` returned
`{"single_pin_net_count": 0}` and `list_schematic_nets` returned `{"count": 0}`
while `kicad-cli sch erc` reported 6 unconnected-pin errors. The internal
analysis is not a substitute for the real validator. This is direct evidence for
the project's own rule: **never report OK from an internal check when a real
validator exists.** Feeds the independent-verification work.

### E8 — `export_bom` lives in the `pcb_export` toolset (2026-08-10) — OPEN

`export_bom(schematic)` reads only schematic data but is registered under
`pcb_export`. An agent that loaded every schematic toolset still gets
`toolset_not_loaded` and pays a failed call plus a `load_toolset` round trip.
Taxonomy defect; fix belongs with the capability matrix work.

### E9 — Error messages leak the OS locale (2026-08-10) — FIXED

```
{"error":{"kind":"handler_error","reason":"IO error: Le fichier spécifié est introuvable. (os error 2)"}}
```

`std::io::Error` is formatted straight into the agent-facing payload, so the
same failure has different text on a French and an English machine. Error
matching, dedup, and any stable-finding-id scheme break on that. The error
catalog must carry a stable code and keep the localized string as a detail
field.

Fixed with `ToolErrorKind::from_anyhow`, which walks the `anyhow` source chain
for a `std::io::Error` and emits `Io { code, detail }` — `code` from an explicit
`ErrorKind` mapping (never `Debug`, which is not a stability promise), `detail`
keeping the OS's own words for a human. Used at both places a handler error
reaches an agent: `handler.rs` dispatch and `kicad_invoke`'s inner loop. A test
asserts the French string classifies as `not_found`.

Still open in one respect: `SexpError::Io`'s own `Display` ("IO error: {0}")
remains localized. That is now a *detail* string rather than the matchable
value, which is the part that mattered.

### E11 — Structured errors were Debug-formatted into opaque strings (2026-08-10) — FIXED

Found by the gateway's first test. `get_symbol_info` with no `lib_id` returned:

```
Tool error: CallToolResult { content: [Text { text: "{\"error\":{\"field\":\"lib_id\",\"kind\":\"invalid_argument\",...
```

The cause is `require_str(args, "k").map_err(|e| anyhow::anyhow!("{:?}", e))?`
— eight sites across `library.rs` and `integration.rs`. The helper builds a
proper structured `invalid_argument` with a `field`, and the handler then
Debug-formats it into a string and re-wraps it as `handler_error`. Every
consumer that matches on `error.kind` — recovery loops, the observability
`error_kind` column, any future stable finding id — silently degrades to
"something went wrong" on exactly these calls.

This predates the fork and affects the normal path as much as the gateway; the
gateway only made it visible because its test asserted on the error *kind*
rather than on the presence of an error.

Fixed with a `try_arg!` macro that returns the structured result unchanged, and
the macro's doc comment states the anti-pattern so it does not come back. All
eight sites converted.

### E10 — Pre-existing clippy debt under `--all-targets` (2026-08-10) — DEFERRED

Upstream CI runs `cargo clippy --workspace --locked -- -D warnings` (no
`--all-targets`), so test code was never linted and lib-level
`await_holding_lock` warnings in `sch_components.rs` never fired. Under
`--all-targets` the baseline fails too, so this is inherited, not introduced.
The six literal-bool / length-comparison lints in
`konnect-schematic-editor/tests/integration.rs` are fixed. The
`MutexGuard`-held-across-await instances are real correctness smells and are
deferred to Phase L hardening; `gate.ps1` mirrors the upstream gate so it does
not silently accumulate more.

---

## OPEN QUESTIONS

1. Local model that fits 16 GB VRAM with reliable tool-calling and structured
   output. Needs a real benchmark (Phase H).
2. Can PCB E2E run unattended on Windows, or does `KICAD_API_SOCKET` require a
   live GUI session? Blocks PCB benchmark coverage.
3. Is tool-granular loading enough, or does the compact-gateway design (~7
   stable verbs, catalogue never changes, `CATALOG_TOKENS` → 0) need to land
   before Phase H? Current data says the gateway is the only way past 3 698.

---

## NEXT ACTION

Semantic diff is done and measured. Continue in Phase E, in this order:

1. **Evidence handles** — `kicad://evidence/N`, `kicad://diff/N` as MCP
   resources. The reply keeps its one-line summary and the full change list
   moves behind a handle, which is what makes `diff: "changes"` affordable at
   any size. The snapshot is the backing store; the diff is already
   `Serialize`, so the store is the missing half.
2. **Validators in the evidence pack.** The diff says what moved; it does not
   say whether the board still passes. `run_erc` / `run_drc` deltas belong in
   the same reply — `ERC 4 → 0` is the line that turns a diff into a proof, and
   **E7** says the internal analysis must not be the thing that answers it.
3. **Task State Manager** (`kam-state` grows a task module). Objective,
   constraints, verified facts, failed attempts — outside the LLM context.
4. **Plan IR** (`kam-plan`) on top of `kicad_invoke`, which has the three
   properties a plan executor needs: preconditions, atomicity, identity.

Still do not start the local model runtime (H). A batch can now be rolled back
*and* describe itself, but nothing yet verifies the description against KiCAD's
own validators, and an accelerated loop whose proof is self-reported is the
failure mode the whole design exists to avoid.

Two open defects to fold into that work rather than patch separately: **E6**
(power symbols do not snap to the 1.27 mm grid) belongs with the geometry pass,
and **E7** (internal connectivity analysis disagrees with `kicad-cli` ERC) is
exactly what step 2 above has to get right.

One limitation the diff inherits and does not hide: it reports **objects**, not
**connectivity**. `VDD3V3 connections: +2` is real on a `.kicad_pcb`, where
pads name their net in the file. A schematic has no netlist in the document, so
deriving one would mean re-implementing connectivity — the exact thing E7 shows
already disagrees with `kicad-cli`. Schematic nets stay absent from the diff
until they come from a validator.
