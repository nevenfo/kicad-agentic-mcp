# Konnect — Tool Directory

Canonical reference for every MCP tool exposed by Konnect. Generated from the Rust source (not from hand-maintained prose), so it reflects what the binary actually serves.

**Source of truth**
- Toolset metadata + declared counts: `crates/konnect-core/src/router/registry.rs` (`ALL_TOOLSETS`)
- Meta-tool definitions: `crates/konnect-core/src/router/meta_tools.rs` (`meta_tool_descriptions()`)
- Per-tool names + descriptions: `crates/konnect-core/src/tools/<toolset>.rs` (each `tool!(…)` in the `tools()` vec)

## Overview

- **22 toolsets** organized into 13 categories
- **203 registered tools** + **13 always-visible meta-tools** = **216 total**
- **Four ways to reach a tool**, cheapest last:
  1. **Toolset loading** — `list_toolboxes` → `load_toolset(name)` exposes a whole domain in `tools/list`; `unload_toolset(name)` prunes it. Coarse, and every load makes the client re-fetch the entire catalogue.
  2. **Tool loading** — `find_capabilities(intent)` → `load_tools([names])` exposes exactly the tools named. Same refresh, far less of it.
  3. **The gateway** — `kicad_describe([names])` → `kicad_invoke([calls])` calls tools *without* exposing them at all. The catalogue never changes, so no refresh happens, and a whole sequence fits in one round trip. Measured on the golden suite at Phase F: 2 171 external tokens per task and 4 MCP calls, against 3 770 / 10 for path 2 and 10 912 / 11 for path 1 — the three routes of this same server, which is what that comparison isolates. The gateway route has since drifted to **2 249** as tool descriptions and MCP annotations grew (M.1, 2026-08-24); `docs/benchmark.md` records where the +78 went.
  4. **A plan** — `kicad_invoke([{tool: "apply_plan", …}])` describes the change once instead of enumerating it, and one operation may expand to many calls. Measured on the same divider: 1 124 external tokens against the batch's 2 180; on a four-capacitor decoupling bank, 882 against 2 265. See `docs/benchmark.md`, "Phase G".
- **Startup surface**: the server pre-loads only the **starter kit** (`project`) plus two individually admitted config read tools, so baseline `tools/list` costs ~2.8K tokens (21 tools) instead of ~33.2K for the whole catalogue (2 831 and 33 183 tokens, measured 2026-08-24 on `bench/results/m1-surface.json`, when the catalogue was 215 tools; it is 216 today and the token figure has not been re-measured). If the LLM calls an unloaded tool by name, the error names the owning toolset so recovery is a single `load_toolset` hop.
- **Observability**: every `tools/call` is recorded — ring buffer of the last 100 calls + per-tool counters + JSONL at `<konnect dir>/logs/calls.jsonl`. Calls made inside a `kicad_invoke` batch are recorded individually under their own tool names. The LLM self-diagnoses via `get_recent_calls` and `server_stats`.

## Meta-tools (always visible)

Thirteen tools, grouped into *gateway*, *discovery/routing* and *observability*.

### Gateway

| Tool | Purpose |
|------|---------|
| `kicad_describe` | Return the full input schema of named tools. The schemas arrive as a result the caller asked for instead of a catalogue refresh it cannot decline. |
| `kicad_invoke` | Run one or more tools by name, in order, loaded or not. Nothing is added to `tools/list`. Stops at the first failure unless `stop_on_error: false`; reports `failed_at` and `not_run` so the caller knows what to retry. **It takes a batch**: `{"calls": [{"tool": "…", "args": {…}}]}` — not the `{"name", "arguments"}` shape a single MCP tool call uses. |
| `kicad_agent` | Explicit Agent gateway. Runs one supervisor turn from durable TaskState using the measured decision NO_LLM, LOCAL, or ESCALATE. Direct `kicad_describe`/`kicad_invoke` never enter this gateway or start an LLM. |
| `kicad_agent_verify` | Explicit Agent verification turn. Its PASS/FAIL verdict and `TaskState.verified_facts` come only from cached or freshly run kicad-cli ERC/DRC; model assertions are never verified facts. |

### Discovery / routing

| Tool | Purpose |
|------|---------|
| `find_capabilities` | Search all 203 tools by plain-language intent; returns name + toolset + one-line summary. |
| `load_tools` | Expose specific tools by name without loading their toolset. |
| `list_toolboxes` | List all 22 toolsets with category, tool count, and whether each is currently loaded. |
| `load_toolset` | Load a toolset by name to expose its tools in `tools/list`. Returns the list of tools added. |
| `unload_toolset` | Unload a toolset to prune its tools from `tools/list`. Use when switching tasks to keep context small. |
| `get_active_toolsets` | Return the currently loaded toolsets and how many tools each provides. |

### Observability

| Tool | Purpose |
|------|---------|
| `get_recent_calls` | Last N tool calls (newest first) — `call_id`, tool, toolset, duration, status (ok/error/not_found), `error_kind`. The LLM's debug log. Default limit 20, max 100. |
| `server_stats` | Uptime, total/error call counts, per-tool totals + errors, and the JSONL log path. |
| `changes_since` | Report what happened to a document after the revision `since` (a token `kicad_invoke` already gave in its `revisions` field). Returns the current revision, whether it matches `since`, and the run-journal entries that touched the document. |

---

## Project

### `project` · 6 tools
**Purpose:** Create, open, save, snapshot KiCAD projects, and launch the live schematic viewer.
**Source:** [`crates/konnect-core/src/tools/project.rs`](crates/konnect-core/src/tools/project.rs)

| Tool | Description |
|------|-------------|
| `create_project` | Create a new KiCAD project at the given path. Creates the directory, a blank `.kicad_pro`, empty `.kicad_sch`, and blank `.kicad_pcb`. |
| `open_project` | Check whether a KiCAD project is currently open in the running KiCAD UI. Returns the active project path and whether KiCAD IPC is available. |
| `save_project` | Save the currently open PCB board file via KiCAD IPC. Requires KiCAD to be running with IPC enabled. |
| `get_project_info` | Read project metadata from a `.kicad_pro` file. Returns name, schematic/PCB paths, last-modified times. |
| `snapshot_project` | Export the schematic and PCB to PDF as a timestamped snapshot/checkpoint. Useful before major edits. |
| `open_schematic_viewer` | Launch the live schematic viewer (SVG with auto-refresh on file change). Use after placing components so the user can see changes in real time. |

---

## Schematic

### `sch_components` · 17 tools
**Purpose:** Add, edit, move, rotate, and delete schematic symbols.
**Source:** [`crates/konnect-core/src/tools/sch_components.rs`](crates/konnect-core/src/tools/sch_components.rs)

| Tool | Description |
|------|-------------|
| `create_schematic` | Create a new blank `.kicad_sch` schematic file. |
| `add_schematic_component` | Add a symbol from a KiCAD library to the schematic. Snaps to the 1.27mm grid. |
| `delete_schematic_component` | Remove a symbol instance from the schematic by reference designator or UUID. |
| `edit_schematic_component` | Update fields (Reference, Value, Footprint, custom properties) of a symbol instance. |
| `get_schematic_component` | Get all properties, position, and pin locations for a symbol instance. |
| `list_schematic_components` | List all symbol instances with positions, values, footprints, uuids, and pin locations. A symbol's uuid is what addresses it in every editing tool here, alongside its reference. |
| `move_schematic_component` | Move a symbol to a new position. Does NOT adjust connected wires. |
| `rotate_schematic_component` | Rotate a symbol by setting its absolute rotation angle (0/90/180/270). |
| `move_connected` | Move a symbol and stretch/shrink connected wire stubs to preserve connections. |
| `move_region` | Move all symbols within a bounding box by a given offset. |
| `annotate_schematic` | Run kicad-cli to auto-assign reference designators (`R?` → `R1`, `U?` → `U1`, etc.). |
| `get_schematic_pin_locations` | Get exact (X,Y) coordinates of every pin on a symbol, accounting for rotation/mirroring. |
| `batch_get_schematic_pin_locations` | Get pin locations for multiple components in a single file read. |
| `add_component_annotation` | Add a custom property (annotation) to a symbol instance. |
| `group_components` | Add a group property to multiple components in the schematic. |
| `replace_component` | Replace a component's `lib_id` with a new library symbol (swap the component type). |
| `get_schematic_view` | Render the schematic to a PNG image (base64-encoded) via kicad-cli. |

### `sch_wiring` · 19 tools
**Purpose:** Wires, net labels, power symbols, junctions, no-connects, pin-to-pin connections.
**Source:** [`crates/konnect-core/src/tools/sch_wiring.rs`](crates/konnect-core/src/tools/sch_wiring.rs)

| Tool | Description |
|------|-------------|
| `add_wire` | Add a wire segment (H or V) between two points. T-junctions are auto-detected and junction dots inserted. |
| `batch_add_wire` | Add multiple wire segments in a single file read/write cycle. |
| `delete_schematic_wire` | Delete a wire segment by UUID or by matching start/end coordinates. |
| `batch_delete_schematic_wire` | Delete multiple wire segments in a single file read/write cycle. |
| `split_wire_at_point` | Split a wire at a given point, creating two segments and a junction. Takes the wire's UUID when several cross the point. |
| `add_schematic_net_label` | Add a net label (`net_label`, `global_label`, or `hierarchical_label`). |
| `delete_schematic_net_label` | Delete a net label by its UUID, or by net name and position. |
| `rotate_schematic_label` | Rotate a net label, addressed by UUID or by net name and position, and update its justify direction. |
| `move_labels_by_offset` | Move all labels matching a net name by a given X/Y offset. |
| `batch_rotate_labels` | Rotate multiple labels by net name or by label UUID in a single file read/write cycle. |
| `add_power_symbol` | Add a power symbol (VCC, GND, etc.). Auto-numbers the internal `#PWR` reference. |
| `add_no_connect` | Add a no-connect flag (X marker) to an unconnected pin endpoint. Reports the flag's uuid, which is what `delete_no_connect` takes to remove that one. |
| `delete_no_connect` | Remove a no-connect flag by its UUID or at a given position. |
| `batch_delete_no_connect` | Delete multiple no-connect flags in a single file read/write cycle. |
| `add_junction` | Add a junction dot at a point where wires cross or T-intersect. |
| `batch_add_junction` | Add multiple junction dots in a single file read/write cycle. |
| `connect_to_net` | Connect a pin endpoint to a named net by adding a short wire stub + net label. |
| `connect_pins` | Connect two component pins by reference+pin number. Looks up pin coordinates and routes a wire. |
| `add_schematic_connection` | Connect two schematic points directly with a wire (auto H+V routing). Use `connect_pins` if you have references instead of coordinates. |

### `sch_analysis` · 15 tools
**Purpose:** Net connectivity, pin queries, trace paths, overlap/orphan detection.
**Source:** [`crates/konnect-core/src/tools/sch_analysis.rs`](crates/konnect-core/src/tools/sch_analysis.rs)

| Tool | Description |
|------|-------------|
| `list_schematic_wires` | List all wire segments with start/end coordinates and UUIDs. |
| `list_schematic_nets` | List all distinct net names from net labels, global labels, and power symbols. |
| `list_schematic_labels` | List all label instances (net/global/hierarchical) with positions, names, types, and uuids — a label's uuid is what addresses it in `delete_schematic_net_label` and `rotate_schematic_label`. |
| `get_net_connections` | Get all pins and labels connected to a named net. |
| `get_net_connectivity` | Build the full connectivity graph for a net using union-find. Returns wires, labels, and T-junction locations. |
| `get_pin_connections` | Get the net connected to a specific pin by tracing wires from the pin endpoint. |
| `get_pin_net_name` | Return just the net name for a specific pin on a component. |
| `get_component_nets` | Get all nets connected to every pin of a component. |
| `get_net_components` | Get all components (and their pins) connected to a named net. |
| `trace_from_point` | Trace connectivity from any (X,Y) point — returns what is at that point and the net it belongs to. |
| `find_orphan_items` | Find dangling wire ends, floating labels, and unconnected pin endpoints (0.05mm tolerance). |
| `find_shorted_nets` | Detect accidentally merged nets — pairs of distinct net names sharing a wire path. |
| `find_single_pin_nets` | Find nets with only one label/connection — often indicates a missing counterpart. |
| `get_connected_items` | Get all wires, labels, and components connected to a given component by tracing each of its pins. |
| `check_schematic_overlaps` | Find overlapping symbols or labels that may indicate placement errors. |

### `sch_batch` · 12 tools
**Purpose:** Bulk add, edit, delete, and move schematic elements in one call.
**Source:** [`crates/konnect-core/src/tools/sch_batch.rs`](crates/konnect-core/src/tools/sch_batch.rs)

| Tool | Description |
|------|-------------|
| `batch_connect_to_net` | Connect many pins to a named net by adding labels at each endpoint. Single read → all labels inserted → single write. |
| `batch_delete` | Delete multiple schematic items (wires, labels, junctions, components) by UUID or reference — single file write. |
| `bulk_move_schematic_components` | Move multiple components by a uniform dx/dy offset in a single atomic write. |
| `batch_edit_schematic_components` | Apply field updates (Value, Footprint, custom properties) to multiple components in a single atomic write. |
| `batch_delete_schematic_components` | Delete multiple components by reference designator or UUID in a single atomic write. |
| `connect_passthrough` | Add a wire stub and matching net label at a point to route a signal through a region without drawing a full path. |
| `add_schematic_text` | Add a text annotation (non-net label) to the schematic at a given position. |
| `get_schematic_layout` | Return a compact spatial summary of the schematic: component positions, bounding box, optionally wires, labels, junction dots and no-connect flags — each with the uuid that addresses it. |
| `validate_wire_connections` | Check all wire endpoints for floating ends not connected to a pin, label, or another wire. |
| `validate_component_connections` | Check that every non-passive pin has at least one wire or label connected. Reports unconnected pins. |
| `batch_place_components` | Place multiple symbols from KiCAD libraries in a single file read/write cycle. Pass explicit references -- there is no auto-numbering; an omitted reference becomes '?' like an eeschema-unannotated symbol, same as `add_schematic_component`. |
| `batch_connect_pins` | Connect multiple component pin pairs by reference and pin number, in a single file read/write cycle. |

### `sch_export` · 7 tools
**Purpose:** Export schematic to SVG/PDF/netlist/BOM, run ERC.
**Source:** [`crates/konnect-core/src/tools/sch_export.rs`](crates/konnect-core/src/tools/sch_export.rs)

| Tool | Description |
|------|-------------|
| `export_schematic_svg` | Export a schematic sheet to an SVG file using kicad-cli. |
| `export_schematic_pdf` | Export a schematic sheet to a PDF file using kicad-cli. |
| `generate_netlist` | Generate a KiCAD netlist file from the schematic using kicad-cli. |
| `export_netlist_summary` | Return a human-readable JSON netlist summary (components, nets, pin counts). Does not require kicad-cli. |
| `run_erc` | Run the Electrical Rules Check via kicad-cli and return violations filtered by severity. |
| `fix_connectivity` | Scan for near-miss wire endpoints within `snap_tolerance` of a pin/label and snap them into place. Supports `dry_run`. |
| `export_bom` | Generate a Bill of Materials (BOM) CSV from the schematic's component data. |

### `sch_buses` · 5 tools
**Purpose:** Buses: bus segments, bus entries, bus aliases, and expanding a bus name into the nets it carries.
**Source:** [`crates/konnect-core/src/tools/sch_buses.rs`](crates/konnect-core/src/tools/sch_buses.rs)

| Tool | Description |
|------|-------------|
| `add_bus` | Draw a bus segment on a schematic. A bus is not a thick wire: a wire connects to it only through a bus entry (`add_bus_entry`), and the bus takes its name from a label placed on it. |
| `add_bus_entry` | Add a bus entry — the short diagonal stub that taps one member out of a bus. Place it at the point on the bus; `dx`/`dy` are a delta, not a corner. |
| `add_bus_alias` | Declare a bus alias: one name standing for an explicit list of member nets, so a bus of unrelated signals can be labelled with a single name. |
| `list_buses` | List the bus segments, bus entries, and bus aliases on a schematic, with the labels that name each bus and the members those names expand to. |
| `expand_bus` | Expand a bus name into the member nets it stands for, the way KiCAD's connectivity does: `DATA[0..7]` is a vector, `{SDA SCL}` (optionally prefixed) is an explicit list. |

### `sch_hierarchy` · 12 tools
**Purpose:** Hierarchical sheets: add/edit/move/delete/duplicate a sheet, hierarchy and page-numbering queries, import/add/edit/delete sheet pins, pin/label sync validation.
**Source:** [`crates/konnect-core/src/tools/sch_hierarchy.rs`](crates/konnect-core/src/tools/sch_hierarchy.rs)

| Tool | Description |
|------|-------------|
| `add_hierarchical_sheet` | Insert a hierarchical sheet into a parent schematic, linking it to a child `.kicad_sch` file. Creates the child file if it doesn't exist, or links to an existing one (multi-instance reuse). Patches existing symbols' instance paths if the linked file already has components. |
| `edit_sheet` | Rename, resize, reposition, or repoint (`Sheetfile`) an existing sheet. |
| `move_sheet` | Reposition a sheet on the parent canvas without touching any other field. |
| `delete_sheet` | Remove a sheet reference from the parent schematic. Does not delete the child file. |
| `duplicate_sheet` | Copy an existing sheet and its child file under a new name/file, offset from the source, with an independent internal UUID. |
| `get_sheet_hierarchy` | Recursively walk the sheet tree, returning nested JSON with each sheet's name/file/uuid/position/size/page/pins and its own children. |
| `renumber_sheet_pages` | Walk the whole sheet tree and reassign sequential page numbers in depth-first order, fixing gaps left by delete/duplicate. |
| `import_sheet_pins` | Scan the child sheet's hierarchical_labels and auto-generate matching pins on the parent sheet block, skipping names that already have a pin — the primary way pins get created. |
| `add_sheet_pin` | Manually add a single pin to an existing sheet block. |
| `edit_sheet_pin` | Rename a pin, change its electrical type, or reposition it along the sheet border. |
| `delete_sheet_pin` | Remove a single pin without touching the rest of the sheet. |
| `validate_sheet_pins` | Read-only. Walk the sheet tree and report hierarchical_labels with no matching parent pin, and pins with no matching child label. |

---

## PCB

### `pcb_board` · 11 tools
**Purpose:** Board outline, layers, zones, mounting holes, board text, SVG logo import.
**Source:** [`crates/konnect-core/src/tools/pcb_board.rs`](crates/konnect-core/src/tools/pcb_board.rs)

| Tool | Description |
|------|-------------|
| `set_board_size` | Set the PCB board outline to a rectangle on the Edge.Cuts layer. |
| `get_board_info` | Return metadata about the PCB: title, revision, company, layer count, paper size. |
| `get_board_extents` | Return the bounding box of all objects on the board (IPC, falls back to file parse). |
| `get_layer_list` | Return all layers defined in the board with names and types. |
| `add_layer` | Add a new inner copper or technical layer to the board stack. |
| `set_active_layer` | Set the active layer recorded in the board file's setup section. |
| `add_board_outline` | Add a rectangular board outline on the Edge.Cuts layer at specified coordinates. |
| `add_mounting_hole` | Add an NPTH mounting hole footprint at the specified position. |
| `add_board_text` | Add a silkscreen or fabrication text string to the board. |
| `add_zone` | Add a copper fill zone polygon on a specified layer and net. |
| `import_svg_logo` | Import an SVG file as filled silkscreen/copper artwork (curves flattened to polygons). |

### `pcb_components` · 13 tools
**Purpose:** Place, move, rotate, align, and duplicate PCB footprints.
**Source:** [`crates/konnect-core/src/tools/pcb_components.rs`](crates/konnect-core/src/tools/pcb_components.rs)

| Tool | Description |
|------|-------------|
| `place_component` | Place a footprint on the PCB at a given position and layer via KiCAD IPC. |
| `move_component` | Move a placed footprint to a new X/Y position via KiCAD IPC. |
| `rotate_component` | Set the rotation angle of a placed footprint via KiCAD IPC. |
| `delete_component` | Remove a footprint from the board via KiCAD IPC. |
| `edit_component` | Update the value or other properties of a placed footprint via KiCAD IPC. |
| `find_component` | Find a footprint by reference designator and return its position. |
| `get_component_pads` | Return pad positions and net assignments for a footprint. |
| `get_pad_position` | Return the schematic-space position of a specific pad number on a footprint. |
| `get_component_list` | List all footprints on the board with positions, layers, and values. |
| `place_component_array` | Place multiple copies of a footprint in a grid or line array via KiCAD IPC. |
| `align_components` | Align multiple footprints along a common X or Y axis via KiCAD IPC. |
| `duplicate_component` | Duplicate an existing footprint at a new position via KiCAD IPC. |
| `get_board_2d_view` | Render the PCB as a 2D image using kicad-cli; returns base64 PNG. |

### `pcb_routing` · 12 tools
**Purpose:** Traces, vias, copper pours, net classes, differential pairs.
**Source:** [`crates/konnect-core/src/tools/pcb_routing.rs`](crates/konnect-core/src/tools/pcb_routing.rs)

| Tool | Description |
|------|-------------|
| `add_net` | Add a new net entry to the PCB file (S-expression insert, no IPC required). |
| `route_trace` | Route a trace segment between two points on a copper layer via KiCAD IPC. |
| `route_pad_to_pad` | Route a direct trace between two pads of named components (L-bend routing) via IPC. |
| `add_via` | Add a through-hole via at a position and assign it to a net via IPC. |
| `add_copper_pour` | Add a copper fill zone polygon on a layer/net via S-expression insert. |
| `delete_trace` | Delete a trace segment identified by its UUID via KiCAD IPC. |
| `query_traces` | List trace segments on the board, optionally filtered by net and/or layer. |
| `get_nets_list` | Return all nets defined on the PCB via KiCAD IPC. |
| `modify_trace` | Modify a trace segment by deleting and re-adding it with new parameters. |
| `create_netclass` | Create or update a netclass in the project's design rules (sibling .kicad_pro net_settings; the board file is never touched). |
| `assign_net_to_class` | Assign a net to an existing netclass, as a netclass_patterns entry in the sibling .kicad_pro. |
| `route_differential_pair` | Route a differential pair (two parallel traces with a specified gap). |

### `pcb_export` · 13 tools
**Purpose:** Gerber, PDF, SVG, 3D model, pick-and-place, DRC, DXF/GenCAD/IPC-2581/ODB++.
**Source:** [`crates/konnect-core/src/tools/pcb_export.rs`](crates/konnect-core/src/tools/pcb_export.rs)

| Tool | Description |
|------|-------------|
| `export_gerber` | Export Gerber production files for all copper/mask layers using kicad-cli. |
| `export_pdf` | Export the PCB layout to a PDF file using kicad-cli. |
| `export_svg` | Export the PCB layout to an SVG file using kicad-cli. |
| `export_3d` | Export the PCB as a 3D model (STEP or VRML) using kicad-cli. |
| `export_netlist` | Export the PCB netlist in KiCAD or IPC-D-356 format. |
| `export_drill` | Export drill files on their own, with fabricator options: Excellon or Gerber, units, drill origin, separate plated/non-plated files, and a drill map. `export_gerber` and `export_manufacturing_package` emit drills with KiCAD's defaults; use this for anything else. |
| `export_position_file` | Generate a component placement (pick-and-place) position file for SMT assembly. |
| `export_dxf` | Export the PCB to DXF, one file per layer, using kicad-cli. For mechanical CAD interchange. |
| `export_gencad` | Export the PCB in GenCAD format using kicad-cli. |
| `export_ipc2581` | Export the PCB in IPC-2581 format using kicad-cli — a unified fab/assembly/test data format. |
| `export_odb` | Export the PCB in ODB++ format using kicad-cli — a unified fabrication data format. |
| `refill_zones` | Refill all copper pour zones using kicad-cli (`zone-fill`). |
| `get_drc_violations` | Run the Design Rule Check and return a list of violations. |

---

## Library

### `library` · 15 tools
**Purpose:** Symbol libraries, footprint libraries, search and registration.
**Source:** [`crates/konnect-core/src/tools/library.rs`](crates/konnect-core/src/tools/library.rs)

| Tool | Description |
|------|-------------|
| `create_footprint` | Create a new footprint (`.kicad_mod`) file from a pad layout description. |
| `edit_footprint_pad` | Edit the size, shape, or position of a pad in an existing `.kicad_mod`. |
| `register_footprint_library` | Register a local footprint library directory in the KiCAD global or project library table. Answers `result`: `inserted`, `unchanged`, or `updated`. A nickname already registered against a different URI is refused unless `replace_existing` is set, which corrects the URI in place and keeps the entry's `options` and `descr`. |
| `list_footprint_libraries` | List all registered footprint libraries (global and/or project). |
| `create_symbol` | Create a new KiCAD schematic symbol and append it to a `.kicad_sym` library. |
| `delete_symbol` | Delete a symbol definition from a `.kicad_sym` library. |
| `list_symbols_in_library` | List all symbol names defined in a `.kicad_sym` library file. |
| `register_symbol_library` | Register a `.kicad_sym` library file in the KiCAD global or project symbol table. Same `result` vocabulary and `replace_existing` policy as `register_footprint_library`. |
| `list_symbol_libraries` | List all registered symbol libraries (global and/or project). |
| `search_symbols` | Search for symbols across all registered libraries by name or keyword. |
| `list_library_footprints` | List all footprints in a specific registered library (`.pretty` directory). |
| `get_footprint_info` | Return detailed information about a footprint: pad layout, courtyard, description, and its graphics in the shape `set_footprint_graphics` takes back. |
| `set_footprint_graphics` | Atomically append, replace, or delete line/arc/rect/circle/poly primitives on one layer of a `.kicad_mod`. |
| `search_footprints` | Search for footprints across all registered libraries by name or keyword. |
| `get_symbol_info` | Return detailed information about a schematic symbol: pins, properties, description. |

---

## Integration

### `integration` · 9 tools
**Purpose:** JLCPCB parts database, Freerouting autoroute, datasheet URLs.
**Source:** [`crates/konnect-core/src/tools/integration.rs`](crates/konnect-core/src/tools/integration.rs)

| Tool | Description |
|------|-------------|
| `download_jlcpcb_database` | Download or update the local JLCPCB parts database cache (SQLite). Fetches the chunked archive published by kicad-jlcpcb-tools; `library` picks which one (`basic-preferred` ~2 MB by default, up to `all-parts`). |
| `search_jlcpcb_parts` | Search the local JLCPCB database by keyword, value, or category. |
| `get_jlcpcb_part` | Retrieve full details for a single JLCPCB part by LCSC part number. |
| `suggest_jlcpcb_alternatives` | Suggest JLCPCB-stocked alternatives for a given component value and footprint. |
| `get_jlcpcb_database_stats` | Statistics about the local JLCPCB cache: part count, last updated, file size. |
| `enrich_datasheets` | Fetch and cache datasheet URLs for all components in a schematic (LCSC API). |
| `get_datasheet_url` | Retrieve the datasheet URL for a component by MPN or LCSC ID. |
| `autoroute` | Run Freerouting autorouter: export DSN → autoroute → import SES result. |
| `check_freerouting` | Verify that the Freerouting JAR is available and return its version. |

---

## Verification

### `verification` · 8 tools
**Purpose:** ERC, DRC, design rules, KiCAD UI control.
**Source:** [`crates/konnect-core/src/tools/verification.rs`](crates/konnect-core/src/tools/verification.rs)

| Tool | Description |
|------|-------------|
| `run_drc` | Run the Design Rule Check on the PCB and return structured violation results. |
| `set_design_rules` | Set board-level design rules (clearance, trace width, via size) in the PCB file. |
| `get_design_rules` | Return the current design rule constraints defined in the PCB file. |
| `check_kicad_ui` | Check whether the KiCAD GUI application is running and responsive. |
| `launch_kicad_ui` | Launch the KiCAD GUI application and optionally open a project file. |
| `copy_routing_pattern` | Copy a routing pattern (traces and vias) from one region of the board to another. |
| `set_layer_constraints` | Set per-layer design constraints (min trace width, clearance) in board setup. |
| `check_clearance` | Check the physical clearance (distance) between two components on the PCB. |

---

## Configuration

### `config` · 7 tools
**Purpose:** User preferences, project rules, design rules, fab constraints. **Call `load_user_config` at session start.**
**Source:** [`crates/konnect-core/src/tools/config.rs`](crates/konnect-core/src/tools/config.rs)

| Tool | Description |
|------|-------------|
| `load_user_config` | Load the user's global Konnect preferences (manufacturers, fab constraints, default passives, design rules). Call at session start. |
| `save_user_config` | Update a user preference using dot-notation, e.g. `fab_constraints.fab_house`. |
| `load_project_config` | Load project-specific config from `<project_dir>/.konnect/project.json`. Project overrides user. |
| `save_project_config` | Save a project-specific rule or override (same dot-notation as `save_user_config`). |
| `get_effective_config` | Return the merged config (user defaults + project overrides). The config Claude should use for design decisions. |
| `add_design_rule` | Add a natural-language design rule Claude should follow. Examples: "Always use 100nF X7R for MCU decoupling within 3mm of power pin". |
| `list_design_rules` | List all active design rules (user-level + project-level). |

---

## Design Review

### `design_review` · 6 tools
**Purpose:** AI-powered design audits: decoupling, connections, power rails, DFM, BOM health.
**Source:** [`crates/konnect-core/src/tools/design_review.rs`](crates/konnect-core/src/tools/design_review.rs)

| Tool | Description |
|------|-------------|
| `audit_decoupling` | Check that all ICs have appropriate decoupling caps. Finds power pins without nearby caps and flags wrong values. |
| `audit_connections` | Check for common connection mistakes: missing pull-ups on I2C/reset, missing series resistors on LEDs, floating inputs, shorted outputs. |
| `audit_power_rails` | Check power rail integrity: missing bulk capacitance, no test points, missing regulator output caps. |
| `audit_manufacturing` | DFM checks for the configured fab house: component spacing, silkscreen overlap, via-in-pad, acid traps, board-outline issues. |
| `run_design_review` | Run all available audit checks and produce a consolidated report. Call this when the user asks "is my board ready?" |
| `check_bom_health` | Analyze the BOM for supply-chain risks: parts with no MPN, lifecycle warnings, low stock, unavailable from preferred distributors. |

---

## Templates

### `templates` · 4 tools
**Purpose:** Reference circuit library — USB-C, LDO, buck converter, STM32, I2C, LED — verified component values.
**Source:** [`crates/konnect-core/src/tools/templates.rs`](crates/konnect-core/src/tools/templates.rs)

| Tool | Description |
|------|-------------|
| `search_templates` | Search the reference circuit template library. Returns matches for common subcircuits; templates have verified component values. |
| `get_template` | Get full details for a template: components, connections, design notes. Use the template ID from `search_templates`. |
| `apply_template` | Instantiate a template into the current schematic. Places all components and wires per the connection map; `net_mappings` re-binds template nets to project nets. |
| `list_template_categories` | List all available template categories and the number of templates in each. |

**Built-in templates** (loaded by `load_all_templates` in `templates.rs`):
`usb_c_5v_sink`, `ldo_3v3`, `stm32_minimal`, `i2c_pullups`, `led_indicator`, `buck_converter`.

---

## Manufacturing

### `manufacturing` · 3 tools
**Purpose:** Design-to-fab pipeline: export Gerber+BOM+positions package, validate for fab house, estimate cost.
**Source:** [`crates/konnect-core/src/tools/manufacturing.rs`](crates/konnect-core/src/tools/manufacturing.rs)

| Tool | Description |
|------|-------------|
| `export_manufacturing_package` | Generate ALL files needed for PCB fab + assembly in one call: Gerbers, drill, fab-house BOM, pick-and-place. Targets JLCPCB, PCBWay, etc. |
| `validate_for_manufacturing` | Pre-flight check before ordering: verifies the design is ready for the target fab house (board outline, design rules, BOM completeness, assembly constraints). |
| `estimate_cost` | Estimate total manufacturing cost (PCB + components + assembly) with itemized breakdown. |

---

## Plan

### `plan` · 2 tools
**Purpose:** Compile and run a plan: one operation expands to many grid-snapped tool calls, a later one may use an earlier one's result, and a plan that cannot finish is refused before it starts.
**Source:** [`crates/konnect-core/src/tools/plan.rs`](crates/konnect-core/src/tools/plan.rs) · IR in [`crates/kam-plan`](crates/kam-plan) · operations in [`crates/konnect-core/src/plan/ops.rs`](crates/konnect-core/src/plan/ops.rs)

| Tool | Description |
|------|-------------|
| `preview_plan` | Compile a plan and return the exact tool calls it would run, changing nothing. The same compile that would execute produces the listing, so what is reviewed is what runs. |
| `apply_plan` | Compile a plan and run it. Call it through `kicad_invoke` so the plan inherits the batch's snapshot, rollback, semantic diff and `verify`. Every inner step is recorded in the call log under its own name. |

Operations: `call` (any tool, verbatim), `place`, `power`, `label`, `wire`, `connect`, `decouple`. Every coordinate an operation emits is snapped to the 1.27 mm grid before it reaches a tool, which is what makes E6 — a power symbol written 0.33 mm off its pin, with no tool error anywhere — unreachable inside a plan.

---

## Task

### `task` · 4 tools
**Purpose:** Track an objective across calls: constraints, success criteria, verified facts, failed attempts, evidence — held outside the conversation so a compaction cannot lose them.
**Source:** [`crates/konnect-core/src/tools/task.rs`](crates/konnect-core/src/tools/task.rs) · state in [`crates/kam-state`](crates/kam-state)

| Tool | Description |
|------|-------------|
| `start_task` | Open a task: objective, hard constraints, success criteria. Returns a `task_id` and the ACTIVE TASK anchor. |
| `update_task` | Append to the record — subgoal, progress, verified facts, assumptions, unresolved items, a failed attempt, an approach to stop retrying, a new status. |
| `get_task` | The full record plus the anchor. What to read after a compaction. |
| `list_tasks` | Id, truncated objective, status and progress for every task this session knows about. |

`kicad_invoke(task_id=…)` files a batch's revisions, evidence handles and failures under the task by itself; `update_task` is for what only the caller knows.

---

## Graph

### `graph` · 3 tools
**Purpose:** Query the indexed world model — filtered item lookups, spatial neighbors, and per-document/per-kind counts — instead of dumping a whole document.
**Source:** [`crates/konnect-core/src/tools/graph.rs`](crates/konnect-core/src/tools/graph.rs)

| Tool | Description |
|------|-------------|
| `graph_query` | Filter indexed items across a project's schematic and PCB documents by kind, label, document, indexed attribute, or spatial region — an intersection of indices instead of a document dump. |
| `graph_neighbors` | Items whose indexed position is near another item's, nearest first, distance included — never across documents, since two files index unrelated coordinate spaces. |
| `graph_stats` | Item counts per document and per kind for a project's graph — the cheap orientation query to run before `graph_query` or `graph_neighbors`, no items fetched. |

---

## Appendix: Structural observations

### Is the structure intelligent?

**Yes — the split holds up.** A few observations worth tracking as the tool surface grows:

1. **Categories mirror the KiCAD editor boundaries** — Schematic (`sch_*`), PCB (`pcb_*`), plus library/integration/verification/review/templates/manufacturing as cross-cutting concerns. A new tool's home is usually obvious. Three toolsets do **not** mirror an editor boundary and were added later for the agent runtime: `plan` (describe a change once instead of enumerating it), `task` (durable task state across calls) and `graph` (query the indexed project instead of dumping a document). They are organised by what the caller is doing, not by which editor owns the file, and a tool that belongs to one of them will not look obvious under the editor split — check what the caller is holding, not what the tool touches.

2. **Batch tools are split across two places**:
   - `sch_batch` holds top-level batch primitives (`batch_connect_to_net`, `batch_delete`, `bulk_move_schematic_components`, etc.) plus validation
   - `sch_wiring` / `sch_components` also contain `batch_*` tools (`batch_add_wire`, `batch_delete_no_connect`, `batch_rotate_labels`, `batch_get_schematic_pin_locations`, `batch_delete_schematic_components`, `batch_edit_schematic_components`)
   The split is defensible (tight-scope batches live with their domain; cross-domain batches live in `sch_batch`) but worth a one-paragraph convention note in DEV.md so future additions land consistently.

3. **Cross-toolset cleanups** (historical notes):
   - `search_footprints` and `get_symbol_info` were originally in `verification`; moved to `library` where they belong semantically. Users who were loading `verification` for these will be auto-redirected by the smart "tool not loaded" error.
   - `get_drc_violations` (`pcb_export`) and `run_drc` (`verification`) run the same kicad-cli check. Their tool descriptions now cross-reference each other and steer the LLM toward `run_drc` for interactive use (cleaner summary with error/warning counts) and `get_drc_violations` for bundling into a build package.

### Implementation notes

- The `tool!(name, description, input_schema, handler)` macro lives in `crates/konnect-core/src/tools/mod.rs` and produces a `ToolDef` inserted into each toolset's `tools()` vec.
- Dispatch: `router::registry::tools_for(name)` maps each toolset string to its `tools::<mod>::tools()` vec; `handler.rs` looks up `tools/call` in the currently-loaded toolsets. If the tool exists but its toolset isn't loaded, the error names the owning toolset for single-hop recovery (`router::ToolRouter::find_toolset_for_tool`).
- Some schematic handlers are mid-migration from raw `konnect-sexp` to the typed `konnect-schematic-editor` model (Phase 2 Waves 1–4, see commits `1314ed2`…`f92b3b1`). Tool names and semantics are unchanged; only the internal implementation is in flux.

### Regenerating this doc

The tool list is extracted mechanically from the `tool!(...)` invocations in `crates/konnect-core/src/tools/*.rs`. To regenerate after adding tools, re-run the same extraction and re-verify counts against `registry.rs::ALL_TOOLSETS` — the row count in each table here must equal each toolset's `tool_count`.
