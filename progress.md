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

**F** — compact MCP surface. Phases A (bootstrap), B (cartography) and C
(baseline benchmark) are done; D/E/G/H not started.

## CURRENT TASK

Phase F has landed three reductions, all measured (`docs/benchmark.md`):
`load_toolset` stopped echoing descriptions, tool-granular loading, and now
schema compression + a smaller starter kit. `tools` mode is at **3 197 external
tokens/task (−74.2 % vs baseline)** and startup is **1 454 tokens**, below
upstream's own 1 680.

Remaining Phase F lever is the compact gateway (~7 stable verbs, catalogue never
changes, `CATALOG_TOKENS` → 0). At 3 197 tk/task, 2 281 of them are still
catalogue churn, so the gateway is worth roughly twice everything Phase F has
achieved so far.

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
[ ] Compact gateway (~7 stable verbs, CATALOG_TOKENS -> 0)
[ ] Map the scope gaps (capability matrix)
[ ] Stable IDs / revisions / snapshots / optimistic concurrency
[ ] Transactions / rollback / idempotency at the MCP layer
[ ] Semantic diff
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

---

## BENCHMARKS

Full detail and method: **`docs/benchmark.md`**. Headline:

| Metric | Konnect baseline | Fork `tools` mode | Δ |
|---|---|---|---|
| SUCCESS_RATE | 18/18 | 18/18 | = |
| EXTERNAL_TOKENS/task | 12 373 | **3 197** | **−74.2 %** |
| RESPONSE_TOKENS/task | 3 984 | 964 | −75.8 % |
| CATALOG_TOKENS/task | 8 389 | 2 281 | −72.8 % |
| MCP_CALLS median/task | 11 | 10 | −1 |
| WALL_CLOCK_P50 | 70 ms | 64 ms | −6 ms |
| WALL_CLOCK_P95 | 888 ms | 1 183 ms | +295 (`kicad-cli` spawn variance) |
| `tools/list` at startup | 1 680 tk | **1 454 tk** | −13.5 % |

The startup regression introduced in step 1 (+278 tk for the two new meta-tools)
is now repaid: startup is 504 tokens below that peak and 226 below upstream, with
the meta-tools still present.

Build/test baseline: `cargo build --release -p konnect` 81 s cold;
`cargo test --workspace --lib --tests` 469 → **484 passed, 0 failed, 5 ignored**
on the fork. `cargo fmt --check` and `cargo clippy --workspace --locked -D
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

### E9 — Error messages leak the OS locale (2026-08-10) — OPEN

```
{"error":{"kind":"handler_error","reason":"IO error: Le fichier spécifié est introuvable. (os error 2)"}}
```

`std::io::Error` is formatted straight into the agent-facing payload, so the
same failure has different text on a French and an English machine. Error
matching, dedup, and any stable-finding-id scheme break on that. The error
catalog must carry a stable code and keep the localized string as a detail
field.

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

Per-tool schema compression has reached diminishing returns: the remaining
catalogue is flat (median 101 tk/tool) and the golden suite never loads the fat
tools anyway. The `tools` column is now 3 197 tk/task of which **2 281 is still
catalogue churn** — the client re-fetching `tools/list` because loading tools
changes it.

That answers open question 3: the compact gateway has to land. Design it so the
external catalogue is a fixed set of verbs that never changes, no
`notifications/tools/list_changed` is ever emitted during a task, and
`CATALOG_TOKENS` goes to zero — which is worth more than everything Phase F has
saved so far. That is also the natural seam for Phase G's Plan IR: a gateway
verb takes an objective, not a tool name.
