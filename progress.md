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

Phase F landed its first two reductions and they are measured
(`docs/benchmark.md`). Next lever is schema compression, starting with
`create_symbol` (1 448 tokens, 74 % of the whole startup budget).

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
[ ] Compress heavy tool schemas (create_symbol first)
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

---

## BENCHMARKS

Full detail and method: **`docs/benchmark.md`**. Headline:

| Metric | Konnect baseline | Fork `tools` mode | Δ |
|---|---|---|---|
| SUCCESS_RATE | 18/18 | 18/18 | = |
| EXTERNAL_TOKENS/task | 12 373 | **3 698** | **−70.1 %** |
| RESPONSE_TOKENS/task | 3 984 | 963 | −75.8 % |
| CATALOG_TOKENS/task | 8 389 | 2 785 | −66.8 % |
| MCP_CALLS median/task | 11 | 10 | −1 |
| WALL_CLOCK_P50 | 70 ms | 72 ms | ≈ |
| `tools/list` at startup | 1 680 tk | 1 958 tk | **+278 (regression)** |

The startup regression is the two new meta-tools. It is stated, not hidden.

Build/test baseline: `cargo build --release -p konnect` 81 s cold;
`cargo test --workspace --lib --tests` 469 → **473 passed, 0 failed** on the fork
(4 new `kicad_paths` tests, 9 new `capability_search` tests, minus reshaped ones).

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

Compress the heaviest tool schemas — `create_symbol` (1 448 tk),
`create_footprint` (530 tk), `add_hierarchical_sheet` (318 tk) — then re-run
`bench/surface.py` and `bench/runner.py --load-mode tools` to confirm the gain
lands on `CATALOG_TOKENS` rather than just on the startup number.
