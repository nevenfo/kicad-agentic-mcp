# Developer Guide — Konnect

Internal reference for developing and maintaining the Rust port.

Repository-wide naming, public API, branch, and pull-request rules live in
[docs/NAMING_CONVENTIONS.md](docs/NAMING_CONVENTIONS.md).

## Quick Start

```bash
# protoc is required for protobuf code generation. If PROTOC is unset, the
# build falls back to `protoc` on PATH (see Build Requirements below).
set PROTOC=C:\path\to\protoc.exe   # or install via `choco install protoc`

cargo check                          # verify everything compiles (~15s)
cargo test --workspace --lib --tests # all tests
cargo build --release -p konnect # build the MCP server binary

# Build the schematic viewer (separate crate)
cd crates/schematic-viewer
cargo build --release
```

Schematic-viewer build notes (Windows):

- If `cargo` is not recognized in a fresh shell, add it to the session PATH first:
  `set PATH=%PATH%;%USERPROFILE%\.cargo\bin`
- Close any running viewer window before rebuilding — Windows locks a running
  `.exe`, so the link step fails while the app is open.

## Architecture

```
Konnect/
├── crates/
│   ├── kam-context/           # Token budgets for one local-agent context (clean-room, no konnect-* dep)
│   │   └── src/
│   │       └── compaction.rs        # Compactor, CompactedContext, RetrievalBundle, TaskCore
│   │
│   ├── kam-evidence/          # Domain-level diffing for design documents (format-agnostic, item-set based)
│   │   └── src/
│   │       ├── diff.rs               # ItemSet comparison → human-readable change summary
│   │       ├── model.rs              # ItemSet, Item, key/attribute types
│   │       └── store.rs              # Item-by-item detail behind a handle, resolved on demand
│   │
│   ├── kam-graph/             # Queryable BTreeMap index over an ItemSet — the agent's "world model"
│   │   └── src/
│   │       ├── graph.rs              # Graph construction (indices built once)
│   │       └── query.rs              # Intersecting queries (net members, neighbors, stats)
│   │
│   ├── kam-llm/               # Backend-agnostic local tool-calling chat-completion abstraction
│   │   └── src/
│   │       ├── provider.rs           # Provider trait every backend implements
│   │       ├── openai_compat.rs      # LM Studio / llama.cpp server backend
│   │       ├── usage.rs              # What a call actually cost
│   │       └── hardware.rs           # What the machine offers
│   │
│   ├── kam-runtime/           # Explicit Agent gateway; router accepts only NO_LLM/LOCAL/ESCALATE decisions
│   │   └── src/lib.rs                # Gateway + task-state-driven local supervisor
│   │
│   ├── kam-plan/              # A change described once, checked, then executed without re-asking
│   │   └── src/
│   │       ├── compile.rs            # Intent → the tool calls that implement it
│   │       ├── execute.rs            # Runs a compiled plan
│   │       ├── ir.rs                 # Plan intermediate representation
│   │       ├── program.rs            # Compiled program type
│   │       └── refs.rs               # A later step naming an earlier step's output
│   │
│   ├── kam-state/             # Safety primitives for batched mutations of on-disk documents
│   │   └── src/
│   │       ├── revision.rs           # Content-addressed revisions (detect concurrent edits)
│   │       ├── ledger.rs             # Idempotency keys (retry after timeout returns first result)
│   │       ├── snapshot.rs           # Before-images, restored when a batch fails partway
│   │       └── task.rs               # Objective/constraints/facts/attempts held outside model context
│   │
│   ├── konnect/              # Main binary + cdylib entry points
│   │   └── src/
│   │       ├── main.rs              # CLI: --config, subcommands
│   │       ├── lib.rs               # cdylib re-exports ffi
│   │       ├── ffi.rs               # C ABI: kicad_plugin_init/version/shutdown
│   │       ├── config.rs            # TOML + JSON config, socket path auto-detection
│   │       └── transport/
│   │           ├── stdio.rs         # Line-by-line JSON-RPC over stdin/stdout (default)
│   │           └── http.rs          # Streamable HTTP: POST + GET (SSE) on /mcp (transport = "http" / "both")
│   │
│   ├── konnect-core/          # All tool logic (22 toolsets)
│   │   └── src/
│   │       ├── mcp/
│   │       │   ├── protocol.rs      # MCP JSON-RPC 2.0 types
│   │       │   ├── handler.rs       # Dispatch: initialize, tools/list (all tools static), tools/call
│   │       │   └── server.rs        # Session state machine
│   │       ├── router/
│   │       │   ├── mod.rs           # ToolRouter: load/unload toolsets
│   │       │   ├── registry.rs      # Static toolset metadata + tools_for() dispatcher
│   │       │   └── meta_tools.rs    # 13 always-visible meta-tools
│   │       └── tools/
│   │           ├── mod.rs            # ToolDef, ToolContext, tool! macro, helpers, kicad_config_dir(), resolve_lib_symbol()
│   │           ├── cli.rs            # kicad-cli v10 subprocess wrapper (verified against actual binary)
│   │           ├── svg_import.rs     # SVG parsing + Bezier flattening for import_svg_logo (usvg-backed)
│   │           ├── project.rs        # 6 tools (incl. open_schematic_viewer)
│   │           ├── sch_components.rs # 17 tools (component placement with lib_symbols embedding)
│   │           ├── sch_wiring.rs     # 19 tools (incl. connect_pins, power symbol embedding)
│   │           ├── sch_analysis.rs   # 15 tools (union-find net graph, connectivity)
│   │           ├── sch_batch.rs      # 12 tools (single-read/single-write atomic operations)
│   │           ├── sch_export.rs     # 7 tools (SVG/PDF/netlist/ERC)
│   │           ├── sch_buses.rs      # 5 tools (bus segments, bus entries, bus aliases, reading a bus name as the nets it stands for)
│   │           ├── sch_hierarchy.rs  # 12 tools (typed Sheet model, sheet CRUD + hierarchy/page queries + pin lifecycle)
│   │           ├── pcb_board.rs      # 11 tools (S-expr file editing, IPC fallback, SVG logo import)
│   │           ├── pcb_components.rs # 13 tools (IPC real-time via NNG+protobuf)
│   │           ├── pcb_routing.rs    # 12 tools (traces, vias, nets, netclasses)
│   │           ├── pcb_export.rs     # 13 tools (Gerber, PDF, 3D, DRC, DXF/GenCAD/IPC-2581/ODB++)
│   │           ├── library.rs        # 14 tools (symbol/footprint library management)
│   │           ├── integration.rs    # 9 tools (JLCPCB SQLite, Freerouting, datasheets)
│   │           ├── verification.rs   # 8 tools (DRC, design rules, KiCAD UI)
│   │           ├── config.rs         # 7 tools (user/project config, design rules)
│   │           ├── design_review.rs  # 6 tools (decoupling/connection/power/DFM audits)
│   │           ├── templates.rs      # 4 tools (6 built-in reference circuit templates)
│   │           ├── manufacturing.rs  # 3 tools (export package, validate, cost estimate)
│   │           ├── plan.rs           # 2 tools (preview_plan compiles+checks, run_plan executes — kam-plan bridge)
│   │           ├── task.rs           # 4 tools (objective/constraints/facts/attempts held outside model context — kam-state bridge)
│   │           └── graph.rs          # 3 tools (graph_query, graph_neighbors, graph_stats — kam-graph bridge)
│   │
│   ├── konnect-sexp/                  # S-expression engine (no KiCAD dependency)
│   │   └── src/
│   │       ├── parser.rs             # nom-based parser (handles empty strings)
│   │       ├── writer.rs             # SexpEdit + apply_edits + write_atomic
│   │       ├── schematic.rs          # SymbolInstance, LibPin, extract_*, pin_endpoint
│   │       └── geometry.rs           # PinTransform, transform_pin (CANONICAL pin math)
│   │
│   ├── konnect-ipc/                   # KiCAD 10 IPC API client
│   │   ├── proto/                    # Protobuf definitions (copied from KiCAD v10 source)
│   │   ├── build.rs                  # prost-build protobuf code generation
│   │   └── src/
│   │       ├── gen.rs                # Generated protobuf Rust types
│   │       ├── client.rs             # NNG req/rep client, all methods implemented
│   │       ├── builders.rs           # Protobuf message construction helpers (mm→nm conversion)
│   │       └── types.rs              # Public types (IpcFootprint, IpcTrack, etc.)
│   │
│   ├── konnect-schematic-editor/     # Typed schematic model with revision-aware create/replace (`Schematic`)
│   │   └── src/
│   │       ├── schematic/                  # Symbol, Sheet, Label, Bus, Wire, Junction/NoConnect/Text typed collections
│   │       ├── sexp/                        # Parser/writer used to load and persist the typed model
│   │       ├── library.rs            # Symbol/footprint library lookups for the typed model
│   │       └── kicad_paths.rs        # KiCAD install/data-dir discovery
│   │
│   └── schematic-viewer/            # Tauri desktop app (separate from workspace)
│       ├── tauri.conf.json
│       ├── capabilities/default.json # Tauri 2 ACL grant (core:default) — without it event.listen() is silently denied
│       ├── src/main.rs               # Multi-sheet watcher + snapshot-isolated incremental kicad-cli SVG rendering + Tauri commands, 20 unit tests
│       └── frontend/index.html       # Pan/zoom SVG viewer, sheet selector, auto-refresh
│
├── plugin/                           # Python thin launcher (runs inside KiCAD)
│   ├── __init__.py                   # pcbnew.ActionPlugin — settings dialog (PCB Editor only)
│   ├── settings_dialog.py            # wxPython settings UI (paths, server control)
│   └── plugin.json                   # KiCAD 10 IPC plugin manifest
│
├── packaging/
│   ├── build-pcm.ps1                 # Build the PCM zip (Windows)
│   ├── build-pcm.sh                  # Build the PCM zip (macOS/Linux)
│   ├── metadata.json                 # KiCAD PCM package manifest
│   ├── validate-pcm.py               # Validate metadata.json against the PCM schema
│   ├── schema/                       # PCM packages.v1 JSON schema
│   └── resources/                    # PCM package resources (icon.png)
│
└── .github/workflows/
    ├── ci.yml                        # Check + test + clippy on 3 platforms
    ├── e2e-kicad.yml                 # End-to-end tests against a real KiCAD install
    └── release.yml                   # Build binaries + GitHub Release on tag push
```

## KiCAD 10 Integration

### IPC API (PCB Editor — real-time)
- Transport: **NNG** (nanomsg-next-gen) over IPC sockets (Windows named pipes)
- Protocol: **Protocol Buffers** (protobuf3) with ApiRequest/ApiResponse envelope
- Socket path: from `KICAD_API_SOCKET` environment variable (set by KiCAD when launching plugins)
- Scope: **PCB editor only** — full CRUD on all board items, layer management, design rules
- Schematic editor IPC: export-only (SVG, PDF, BOM, netlist) — NO item CRUD

### Driving the PCB path unattended

Measured on KiCad 10.0.3 / Windows 11 (J.3.1), because the answer decides
whether the PCB half of the suite can be a gate or only a manual ritual.
`scripts/live-pcb-e2e.ps1` is the executable form of everything below: it
starts pcbnew on a throwaway copy of the board fixture, runs both live suites,
and stops pcbnew, exiting non-zero if either fails.

- **A desktop session is required; a human is not.** pcbnew is a GUI binary
  with no headless mode, so something must own a window station — but nothing
  in the loop needs a person, and no window is ever clicked.
- **`KICAD_API_SOCKET` is not inherited by an external client.** KiCad sets it
  for plugins *it* launches. A separate process does not need it handed over:
  the server listens on a deterministic path,
  `%LOCALAPPDATA%\Temp\kicad\api.sock`, surfaced as the Windows named pipe
  `\\.\pipe\<that path>`. Construct it; do not read it out of the Preferences
  dialog.
- **PowerShell's `Test-Path` reports `False` for that live pipe** — the
  FileSystem provider chokes on the embedded drive letter. Enumerate with
  `[System.IO.Directory]::GetFiles('\\.\pipe\')` instead.
- **`KICAD_API_TOKEN` may be empty.** KiCad issues a token to plugins it
  launches; it does not demand one from other clients.
- **`api.enable_server` must already be true** in
  `%APPDATA%\kicad\10.0\kicad_common.json`. It is off in a fresh profile and
  cannot be switched on over IPC — there is no server yet to ask. The script
  sets it before starting KiCad.
- **The pipe appears before KiCad will answer on it.** A client that connects
  the instant the pipe exists is told `AS_NOT_READY`. Every live test therefore
  polls for an open document itself rather than depending on a previous test
  having warmed KiCad up; with `--test-threads=1` the alphabetically first test
  is the one that pays.

### S-Expression File Editing (Schematic — offline)
- Direct read/write of `.kicad_sch` files
- Symbol definitions auto-embedded from KiCAD 10's `.kicad_symdir` format
- Power symbols (VCC, GND) embedded from `power.kicad_symdir`
- Existing-file edits use revision-checked atomic replacement: read the exact
  source, acquire a cooperative lock, reject any intervening KiCad or Konnect
  change, write a unique sibling scratch file, fsync, and rename.
- Cooperative lock files live under `KONNECT_STATE_DIR/locks` when that
  absolute override is set, otherwise under the platform local-data directory
  (`konnect/locks`). Reads never create files in the KiCad project.
- Multi-file schematic changes use project-local
  `.konnect-transaction-*.json` write-ahead journals. These journals contain
  complete before/after images and must be treated as sensitive project data.

`konnect_schematic_editor::Schematic` deliberately distinguishes creation from
replacement:

- `save(new_path)` is create-only and refuses to replace an existing path.
- `save(loaded_path)` and `overwrite()` replace only when the file still
  exactly matches the source loaded into the model. KiCad autosave therefore
  produces a conflict that callers must resolve by reloading and reapplying.
- Callers that intentionally replace an existing file must use the explicit
  revision-aware writer/command APIs; they must not delete the destination or
  weaken `save()` into an unconditional overwrite.

For journal diagnosis and recovery, use `konnect transaction status`,
`konnect transaction recover`, and the explicit force-gated `konnect
transaction abandon` escape hatch documented in
[Troubleshooting](docs/TROUBLESHOOTING.md#transaction-recovery-is-blocked-by-divergent-content).

### kicad-cli v10 (Subprocess)
- Verified commands: `sch erc`, `sch export svg/pdf/bom/netlist`, `pcb drc`, `pcb export gerbers/drill/pdf/svg/step/vrml/pos/ipcd356`, `pcb render`
- Removed in v10: `sch annotate` (reimplemented in Rust), `pcb sync`, `pcb export/import specctra`
- Version format: `20250610`

### Plugin Installation
- **PCM zip** is the correct install method
- KiCAD installs to: `C:\KiCad\10.0\share\kicad\scripting\plugins\konnect\`
- Both `__init__.py` (SWIG ActionPlugin for PCB editor settings dialog) and `plugin.json` (IPC exec plugin) are included

## Addressing an Item (plan.md D.4)

A schematic item is addressed either the way it always was — a reference
designator, a sheet name, a coordinate pair — or by its own KiCad `uuid`. Both
forms are accepted everywhere; the historical form never changed, and when a
call carries both, the historical one wins.

Why the second form exists: a designator survives a move but not a rename, a
sheet name is not unique, and a coordinate names whatever happens to be at that
point — two wires crossing there, or two labels stacked on one anchor. A uuid
is the identity KiCad itself writes, so it survives all of that.

**Which tools take one.** `sch_components`: the nine tools that address one
component. `sch_hierarchy`: the eight that address a sheet (`duplicate_sheet`
spells it `source_uuid`, beside `source_sheet_name`). `sch_wiring`:
`delete_schematic_wire`, `batch_delete_schematic_wire`, `split_wire_at_point`
(*which* wire is cut), `delete_schematic_net_label`, `rotate_schematic_label`,
`delete_no_connect`. `sch_buses` takes none — every tool there creates or
reads.

The plural tools take one the way their own address is already shaped
(plan.md D.4.1.6): a `uuids` array beside `references`
(`batch_get_schematic_pin_locations`, `group_components`,
`bulk_move_schematic_components`, `batch_delete_schematic_components`), or a
`uuid` field inside the entry objects they already read
(`batch_edit_schematic_components`, `batch_rotate_labels`,
`batch_delete_no_connect`). Both may be given at once and the batch is their
union, each item acted on once; a uuid that resolves to nothing joins the
per-entry errors that tool already collects, and the rest of the batch runs.
`move_labels_by_offset` keeps only `net`, which selects every label of a net
rather than addressing one item.

**Where an address comes from.** An address a tool accepts is an address some
tool publishes: `list_schematic_components`, `list_schematic_labels`,
`list_schematic_wires`, `get_sheet_hierarchy` and `get_schematic_component` all
return uuids, and `add_no_connect` reports the one it created. A junction or a
no-connect has no other identity to be found by, so `get_schematic_layout`
takes `include_junctions` / `include_no_connects` — off by default, since a
caller who does not need them should not pay for them.

**What a uuid means here.** An item's identity is its *own* direct-child
`(uuid …)`. A uuid nested inside another item — a sheet pin's, for instance —
does not address the item around it. The one exception is deliberate and
predates this model: `batch_delete` has always accepted a nested uuid and
deleted the enclosing item, and a test pins that so it is dropped by decision
rather than by refactor.

**When an address resolves to nothing**, the answer is `NotFound` carrying
`item_kind` and, on the uuid path, the uuids actually present as `candidates`.
A uuid that exists but names another kind of item is `NotFound` too — never an
edit to the wrong item.

**A multi-unit symbol** is several top-level `(symbol …)` blocks sharing one
designator, each with its own uuid and `(unit N)`. A uuid names one unit and
edits that unit: the `sch_components` handlers resolve an address to the
symbol's *position* — in the loaded schematic, among the parsed instances, or
as a byte range — and never redescend by designator afterwards (plan.md
D.4.1.7). A designator, having no unit in it, still means the first block
carrying it.

## Structured Errors

Tool-call failures are typed via the `ToolErrorKind` enum in `crates/konnect-core/src/mcp/error.rs`. MCP's `CallToolResult` spec has no top-level `data` field, so structured errors ride inside the text content as JSON:

```json
{
  "message": "Tool 'place_component' is in toolset 'pcb_components' — call load_toolset('pcb_components') first, then retry.",
  "error": {
    "kind": "toolset_not_loaded",
    "toolset": "pcb_components",
    "tool": "place_component"
  }
}
```

`is_error: true` on the result; plain clients show the `message` field, structured clients match on `kind`. The observer's `error_kind` column is populated via `extract_error_kind()` so JSONL logs use the same vocabulary regardless of where the error originated.

### Current kinds

| `kind` | When |
|--------|------|
| `toolset_not_loaded` | Tool exists but its toolset isn't loaded yet |
| `unknown_tool` | Tool name doesn't exist in any toolset |
| `invalid_argument` | Required argument missing/malformed |
| `file_not_found` | Referenced file doesn't exist |
| `handler_error` | Catch-all for unmigrated `anyhow::Error` returns |

### Producing structured errors in a handler

```rust
if !path.exists() {
    return Ok(CallToolResult::error_kind(
        ToolErrorKind::FileNotFound { path: path.display().to_string() },
        format!("Project file not found: {}", path.display()),
    ));
}
```

Adding a new kind: edit `mcp/error.rs`, add the variant, add the match arm in `short_code()`, use it from the handler. The `short_code_matches_serialized_kind_field` test will fail loudly if they drift.

The dispatch-level errors (not-loaded/unknown/handler-panic) are fully structured. So are **all missing-argument errors** across all 202 tools — `tools/mod.rs::require_str` / `require_f64` emit `ToolErrorKind::InvalidArgument { field, reason }` automatically. Most in-handler errors still use `CallToolResult::error("free text")` or bubble `anyhow::Error`; migrating them is incremental. `project.rs::handle_get_project_info` demonstrates the structured `FileNotFound` pattern.

## Observability

Every `tools/call` flows through `McpHandler::execute_tool`, which wraps the dispatch with:
- A **ring buffer** of the last 100 `CallRecord`s (surfaced via `get_recent_calls` meta-tool).
- **Per-tool counters** for totals, errors, cumulative duration, last-status, last-error (surfaced via `server_stats`).
- **JSONL append** to `<konnect dir>/logs/calls.jsonl` (one line per call). Paths:
  - Windows: `%APPDATA%\konnect\logs\calls.jsonl`
  - macOS: `~/Library/Application Support/konnect/logs/calls.jsonl`
  - Linux: `~/.konnect/logs/calls.jsonl`
- **Structured `tracing` events** (`tool_call_start` + `tool_call_end`) carrying `call_id`, `tool`, `toolset`, `status`, `dur_ms` — greppable in the stderr log.

Each `CallRecord` includes: `call_id`, `ts` (unix ms), `tool`, `toolset` (optional — `None` for meta-tools), `dur_ms`, `status` (`ok` / `error` / `not_found`), `error_kind`, `args_bytes`, `result_bytes`.

The observer is constructed once by `McpHandler::new` and stashed on both the handler and `ToolContext` so meta-tools can reach it. IO failures on the JSONL file never fail the tool call — they `tracing::warn!` and are silently dropped. Tests construct an in-memory-only observer via `ToolContext::new(...)` (no `log_path`).

Source: [`crates/konnect-core/src/observability.rs`](crates/konnect-core/src/observability.rs).

## Tool Routing (Starter Kit + On-Demand Loading)

The server does NOT expose all 202 tools (215 total with the 13 meta-tools) in `tools/list` by default — that would cost ~33K tokens of context on every listing. Instead:

- **Startup**: only `STARTER_KIT` toolsets are pre-loaded (see `router/registry.rs::STARTER_KIT`). Currently: `project` alone, plus the two `config` read tools admitted individually through `STARTER_TOOLS` (`load_user_config`, `get_effective_config`) — the five `config` write tools cost 507 tokens per refresh and the golden suite calls none of them. Combined with the 13 meta-tools, baseline `tools/list` is 21 tools / 2 831 tokens (measured, `bench/results/m1-surface.json`).
- **On demand**: the LLM reads `list_toolboxes` → calls `load_toolset(name)` to expose a toolset's tools in subsequent `tools/list` responses. `unload_toolset(name)` prunes them when the task shifts.
- **`tools/list_changed` notification**: sent on every load/unload so MCP clients refresh their local tool cache.
- **Error recovery**: if the LLM calls an unloaded tool, `handler.rs` returns an actionable error naming the toolset that owns it (so the LLM can load it and retry in one hop — no extra `list_toolboxes` round-trip).
- **`auto_load_toolsets` (config key, default `false`)**: when set, a miss in `dispatch_tool` loads the owning toolset and executes the call in the same hop instead of returning `toolset_not_loaded` -- fewer round trips, at the cost of toolsets accumulating monotonically for the rest of the session (`unload_toolset` still prunes, but a tool call reloads its toolset right back). Off by default because the router's whole point is keeping `tools/list` small; turn it on only if your client would rather eat the context growth than handle one recoverable error per miss. Set via `konnect.toml`/`settings.json` (`auto_load_toolsets = true`) or the equivalent `ServerConfig` field when embedding.

The router is defined in `crates/konnect-core/src/router/mod.rs`.

## The Agent Layer

The sections above describe the MCP server: a caller asks for a tool, a handler
runs it. The `kam-*` crates are the other half — what runs when the caller is a
model rather than a person, and what keeps that safe. They are clean-room by
construction (no `konnect-*` dependency, `MIT OR Apache-2.0` — `plan.md`'s
INV2), so each one has a KiCAD-side adapter inside `konnect-core` that supplies
the domain the crate refuses to know. Read this section for *how the pieces
fit*; `plan.md` and `decisions.md` own *why each was chosen*, and
`docs/benchmark.md` owns *what it measured*.

**The gateway** — `crates/kam-runtime/src/lib.rs`. Direct MCP tool calls never
enter it: a caller reaches it only through the two agent meta-tools,
`kicad_agent` and `kicad_agent_verify` (`router/meta_tools.rs`). Its router
accepts exactly three decisions — `NoLlm` (finish deterministically, no model
call), `Local` (the configured local model only), `Escalate` (refuse and return
structured evidence to the caller). `LocalModelProfile` pins what `Local` means:
`gpt-oss-20b`, medium reasoning effort, a 32 768-token window with 5 120 held
back for completion. A `SupervisorOutcome` always says which decision ran and
whether a provider was actually called; the model's text comes back as
`proposal`, never as a fact. `konnect-core/src/agent_loop.rs` is what turns a
proposal into a change: Plan IR compile, execute, then deterministic
verification in `verification_agent.rs`, whose only verdict source is
`kicad-cli` (or its exact-revision cache entry).

**The local provider** — `crates/kam-llm`. `provider.rs` is the trait every
backend implements; `openai_compat.rs` is the one concrete backend LM Studio and
`llama.cpp server` share; `usage.rs` is what a call cost and `hardware.rs` what
the machine offers. The crate chooses nothing — no model, no ranking, no
routing. `crates/kam-context` sits on top of it and holds the token budget for
one context: `BudgetLimits` (window, completion reserve) and `Compactor`, which
evicts by caller-ranked `RetrievalBundle`s while `TaskCore` stays. The backend's
`Usage` is authoritative, and reasoning tokens are already inside completion
tokens — kept as a split, never counted twice.

**Evidence and its handles** — `crates/kam-evidence`. `model.rs` defines the
`ItemSet` vocabulary, `diff.rs` matches items by stable key and reports
attribute differences (`U4 moved: (84,31) -> (82,29)`), `finding.rs` gives a
validator's findings stable ids so a fix is an id that disappeared rather than a
count that fell. `store.rs` is the second half of the same idea: the reply
carries the one-line summary, the item-by-item detail stays behind a handle. The
KiCAD side is `konnect-core/src/evidence/` — `schematic.rs` and `pcb.rs` extract
an `ItemSet` keyed by KiCad's own UUIDs (so a re-serialised file is still the
same items), `validators.rs` runs the `kicad-cli` checks. A handle is an MCP
resource: `resources/list` enumerates them and `resources/read` resolves one
(`mcp/handler.rs`), under the `kicad:` scheme, bounded at 64 entries — and a
handle that aged out reports differently from one that never existed, because an
agent deciding whether to re-run a check needs to know which it hit.

**The world model** — `crates/kam-graph`. `graph.rs` builds `BTreeMap` indices
once over an `ItemSet`; `query.rs` intersects them instead of re-scanning.
`BTreeMap` and not `HashMap` on purpose: a client caching by request prefix needs
the same query to answer in the same order every time. Every truncatable query
reports `total` separately from the `items` it returns, and the cap cannot be
raised — a caller who wants more asks a narrower question.
`konnect-core/src/graph.rs` discovers a project's `.kicad_sch` / `.kicad_pcb`
documents, extracts each with the same `evidence::extract` the diff already uses,
caches the built index, and serves the `graph` toolset.

**Plan IR** — `crates/kam-plan`. It replaces the call-read-decide loop, where
every arrow is an inference, with two moves: write the plan, run it. `ir.rs` is
the plan document, `compile.rs` expands one operation into many tool calls,
`refs.rs` resolves a later operation's reference to an earlier one's output
(`${place.reference}`), `program.rs` is the compiled step list and `execute.rs`
runs it. Both are settled *before* the first mutation: a plan whose reference
points at an operation that does not exist, or has not run yet, is refused and
never starts. The crate does not know what an operation means — that is
`konnect-core/src/plan/` (`ops.rs`), the single `OpLibrary` implementation, which
also does the arithmetic a model should not be asked to do: every coordinate it
emits is snapped to `konnect_sexp::geometry::SCHEMATIC_GRID_MM` before it reaches
a tool — the one grid helper of INV5, never a second literal.

**State safety** — `crates/kam-state`, four questions and nothing else.
`revision.rs`: is the document still what the plan was written against
(content-addressed, so an edit in another window is detected, not overwritten)?
`ledger.rs`: have I already run this (an idempotency key, so a retry after a
timeout returns the first result)? `snapshot.rs`: can I undo a half-applied batch
(before-images, written back on partial failure)? `task.rs`: what was I doing —
objective, constraints, verified facts, failed attempts, held outside any model's
context so a compaction or a model swap cannot lose them. `journal.rs` outlives
any one snapshot: an append-only record of run outcomes with bounded
before/after images. Orthogonal to all of them, `mode.rs` carries the
process-wide `OperatingMode` (`ReadOnly`, `Write` — the default, `Manufacturing`,
`Experimental`), enforced by `konnect-core/src/mode_gate.rs` at exactly two call
sites, always before a handler runs so a refusal has nothing to roll back (INV4).

**The bridge.** Three registry toolsets expose this layer to a caller, and they
are registry toolsets rather than gateway verbs so they cost nothing until used
(`plan.md` E.4.4, D20): `plan` (`preview_plan` compiles and returns the exact
calls, changing nothing; `apply_plan` compiles and runs them inside
`kicad_invoke`, inheriting its snapshot, rollback, semantic diff and verify
verdict, with every inner step logged under its own call id), `task`
(`start_task`, `update_task`, `get_task`, `list_tasks`) and `graph`
(`graph_query`, `graph_neighbors`, `graph_stats`). Two meta-tools sit outside the
toolsets and are always present: `kicad_agent` and `kicad_agent_verify`.
`changes_since` is the third always-present one that belongs here: it compares a
document's current revision against a token an earlier `kicad_invoke` published
and reads the journal for what happened in between — and answers with the
document's own state even when no journal is open, rather than refusing.

## Build Requirements

- Rust toolchain pinned by [`rust-toolchain.toml`](rust-toolchain.toml) (currently 1.96.0) —
  rustup picks it up automatically, and CI compiles with the same version. The pinned
  version IS the MSRV: bump it deliberately, in its own commit, after running the full
  local gate on the new version.
- `protoc` binary (for protobuf code generation in konnect-ipc crate)
  - Set `PROTOC` environment variable, or leave it unset and `konnect-ipc/build.rs`
    falls back to `protoc` found on PATH
  - Well-known-type includes are derived from `<PROTOC>/../../include` (i.e. a standard
    protoc release layout with `bin/protoc` next to `include/`) when that directory exists
  - Download: https://github.com/protocolbuffers/protobuf/releases
- For schematic-viewer (built separately from the workspace — see Quick Start):
  - Rust toolchain on PATH (Windows: `set PATH=%PATH%;%USERPROFILE%\.cargo\bin` if `cargo`
    isn't recognized in the shell)
  - Tauri 2 prerequisites: WebView2 runtime on Windows (usually pre-installed on Win 10/11)
  - At runtime it discovers `kicad-cli` from the standard KiCAD install paths, then PATH;
    override with `--kicad-cli <path>`
  - Rebuilds fail while a viewer window is open (Windows locks the running `.exe`) — close
    the app before `cargo build`

## Test Suite

Run all: `PROTOC=<path> cargo test --workspace --lib --tests`

| Location | What |
|----------|------|
| `konnect-sexp` unit tests | Parser, writer, geometry transforms |
| `konnect-core` unit tests | Router load/unload, starter-kit, registry invariants, observability, error taxonomy, arg helpers |
| `konnect-core` integration tests | Fixture files: parse, edit, write, observability, structured errors |
| `konnect-schematic-editor` tests | Typed schematic model + round-tripping |
| `scripts/live-pcb-e2e.ps1` | The `#[ignore]`d live-KiCad PCB suites, start to finish, against a pcbnew the script launches and stops itself (see "Driving the PCB path unattended") |

`schematic-viewer` is **excluded from the workspace** (`Cargo.toml`'s `[workspace] exclude`) since
it's a Tauri app built separately — `cargo test --workspace` never touches it, and neither does
CI (`.github/workflows/ci.yml` runs everything with `--workspace`). Run its tests explicitly:
`cd crates/schematic-viewer && cargo test`. Its 20 unit tests cover the pure sheet-tree-walking,
watch-directory, render-snapshot, event-debounce, and incremental-render-selection logic
(`walk_sheet_tree`, `compute_watch_dirs`, `snapshot_tree`, `drain_until_quiet`,
`files_needing_render`, `render_all`'s error handling) — the actual `kicad-cli` subprocess call
and Tauri command/event plumbing stay thin and untested, matching this codebase's existing
convention for other `kicad-cli`-calling code.

## Adding a New Tool

1. Add the `tool!(...)` definition to the appropriate toolset's `tools()` vec
2. Write the `async fn handle_*()` handler below the tools vec
3. Update `tool_count` in `router/registry.rs::ALL_TOOLSETS` — this is the declared count shown in `list_toolboxes`
4. If the new tool belongs in the default-available set, add its toolset to `registry.rs::STARTER_KIT`
5. Run `cargo check` and re-run the tool-directory extraction (see `tool-directory.md` header) to keep the docs in sync

## Current Stats

- **22 toolsets, 202 tools** + 13 meta-tools (2 gateway + 2 agent + 6 routing/discovery + 3 observability/state — see `tool-directory.md`)
- Baseline `tools/list`: 21 tools / 2 831 tokens (starter kit + meta-tools)
- Full-catalog `tools/list` (all loaded): 215 tools (202 registered + 13 meta) / 33 183 tokens

  Both surface figures are measured by `bench/surface.py` (tiktoken `o200k_base`) and
  committed as `bench/results/m1-surface.json`; `docs/benchmark.md` records what moved
  them.
- **0 IPC stubs** (all protobuf methods implemented)
- **0 unimplemented tools**
- **3 CLI commands removed in KiCAD v10** (specctra DSN/SES, pcb sync — return clear errors)
