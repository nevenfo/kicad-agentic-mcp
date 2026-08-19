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
│   ├── konnect-core/          # All tool logic (18 toolsets)
│   │   └── src/
│   │       ├── mcp/
│   │       │   ├── protocol.rs      # MCP JSON-RPC 2.0 types
│   │       │   ├── handler.rs       # Dispatch: initialize, tools/list (all tools static), tools/call
│   │       │   └── server.rs        # Session state machine
│   │       ├── router/
│   │       │   ├── mod.rs           # ToolRouter: load/unload toolsets
│   │       │   ├── registry.rs      # Static toolset metadata + tools_for() dispatcher
│   │       │   └── meta_tools.rs    # 6 always-visible meta-tools
│   │       └── tools/
│   │           ├── mod.rs            # ToolDef, ToolContext, tool! macro, helpers, kicad_config_dir(), resolve_lib_symbol()
│   │           ├── cli.rs            # kicad-cli v10 subprocess wrapper (verified against actual binary)
│   │           ├── svg_import.rs     # SVG parsing + Bezier flattening for import_svg_logo (usvg-backed)
│   │           ├── project.rs        # 6 tools (incl. open_schematic_viewer)
│   │           ├── sch_components.rs # 17 tools (component placement with lib_symbols embedding)
│   │           ├── sch_wiring.rs     # 19 tools (incl. connect_pins, power symbol embedding)
│   │           ├── sch_analysis.rs   # 15 tools (union-find net graph, connectivity)
│   │           ├── sch_batch.rs      # 12 tools (single-read/single-write atomic operations)
│   │           ├── sch_export.rs     # 6 tools (SVG/PDF/netlist/ERC)
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
│   │           └── manufacturing.rs  # 3 tools (export package, validate, cost estimate)
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

Known limitation: for a multi-unit symbol, the seven `sch_components` handlers
that go through `konnect_schematic_editor` redescend by designator, so a uuid
naming unit 2 lands on unit 1 (plan.md D.4.1.7).

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

The dispatch-level errors (not-loaded/unknown/handler-panic) are fully structured. So are **all missing-argument errors** across all 187 tools — `tools/mod.rs::require_str` / `require_f64` emit `ToolErrorKind::InvalidArgument { field, reason }` automatically. Most in-handler errors still use `CallToolResult::error("free text")` or bubble `anyhow::Error`; migrating them is incremental. `project.rs::handle_get_project_info` demonstrates the structured `FileNotFound` pattern.

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

The server does NOT expose all 187 tools (193 total with the 6 meta-tools) in `tools/list` by default — that would cost ~23K tokens of context on every listing. Instead:

- **Startup**: only `STARTER_KIT` toolsets are pre-loaded (see `router/registry.rs::STARTER_KIT`). Currently: `project`, `config`. Combined with the 6 meta-tools, baseline `tools/list` is ~19 tools ≈ 2K tokens.
- **On demand**: the LLM reads `list_toolboxes` → calls `load_toolset(name)` to expose a toolset's tools in subsequent `tools/list` responses. `unload_toolset(name)` prunes them when the task shifts.
- **`tools/list_changed` notification**: sent on every load/unload so MCP clients refresh their local tool cache.
- **Error recovery**: if the LLM calls an unloaded tool, `handler.rs` returns an actionable error naming the toolset that owns it (so the LLM can load it and retry in one hop — no extra `list_toolboxes` round-trip).
- **`auto_load_toolsets` (config key, default `false`)**: when set, a miss in `dispatch_tool` loads the owning toolset and executes the call in the same hop instead of returning `toolset_not_loaded` -- fewer round trips, at the cost of toolsets accumulating monotonically for the rest of the session (`unload_toolset` still prunes, but a tool call reloads its toolset right back). Off by default because the router's whole point is keeping `tools/list` small; turn it on only if your client would rather eat the context growth than handle one recoverable error per miss. Set via `konnect.toml`/`settings.json` (`auto_load_toolsets = true`) or the equivalent `ServerConfig` field when embedding.

The router is defined in `crates/konnect-core/src/router/mod.rs`.

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

- **18 toolsets, 187 tools** + 6 meta-tools (4 routing + 2 observability — see `tool-directory.md`)
- Baseline `tools/list`: ~19 tools / ~2K tokens (starter kit + meta-tools)
- Full-catalog `tools/list` (all loaded): 193 tools (187 registered + 6 meta) / ~25K tokens
- **0 IPC stubs** (all protobuf methods implemented)
- **0 unimplemented tools**
- **3 CLI commands removed in KiCAD v10** (specctra DSN/SES, pcb sync — return clear errors)
