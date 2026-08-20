# Capability matrix

**Generated — do not edit by hand.** Rendered from `konnect_core::capability` by
`crates/konnect-core/tests/capability_matrix.rs`, which fails if this file has
drifted. Regenerate with:

```
KAM_UPDATE_MATRIX=1 cargo test -p konnect-core --test capability_matrix
```

Three rules decide what appears here, and they exist to stop the percentage
from becoming decoration:

* **`SUPPORTED` is discovered, not declared.** The status comes from scanning
  this repository's tests and golden benchmark tasks for something that
  exercises the tool. Code that looks finished and is exercised by nothing
  reads `NOT_TESTED`.
* **A test that does not run is not a proof.** `#[ignore]`d tests — the ones
  needing a live KiCAD GUI or the installed symbol libraries — appear as
  `gated` and never make a capability `SUPPORTED`.
* **What KiCAD has no API for leaves the denominator.** `GUI_ONLY_NO_API` and
  `REQUIRES_CUSTOM_KICAD` are not scored as our failures, so the coverage
  number measures what we chose not to build rather than what cannot be built.

The last section lists what no tool covers at all, because a matrix keyed on
the tools that exist is exactly how a document reports full coverage of a
partial feature set.

The scan recognises a tool by its name in quotes or by a call to
`handle_<tool>`. A tool whose handler is named differently — `route_differential_pair`
is handled by `handle_route_diff_pair` — is therefore under-reported unless a
test names it. That direction is deliberate: a scanner that guesses wide
inflates the number it exists to keep honest.

## Headline

| | entries | supported | partial | not tested | gap | KiCAD has no API | coverage |
|---|---|---|---|---|---|---|---|
| KiCAD domains | 169 | 120 | 19 | 23 | 2 | 5 | 73.2 % |
| server's own | 40 | 27 | 10 | 3 | 0 | 0 | 67.5 % |

Coverage is `(supported + external) / (entries − entries KiCAD has no API for)`. An entry is `supported` only when a test that actually runs, or a golden benchmark task, exercises it; the proof is named in the tables below.

## V1 comparison target

The headline above measures this fork's whole surface, which grows as tools are added — useful, and not a comparison. The V1 criterion `CAPABILITY_COVERAGE > baseline` is this table instead: the 187 tools mixelpixx/Konnect v0.2.2 registers at `5cd6454`, scored on both sides by the same scanner. This fork still registers every one of them, so the two sides compare name-for-name. The denominator drops only what KiCAD gives no API for and admits nothing this fork added, so the percentage can move only when a test that runs starts proving a tool.

| | inherited tools scored | proved | coverage |
|---|---|---|---|
| baseline `5cd6454` | 186 | 42 | 22.6 % |
| this fork | 186 | 135 | 72.6 % |

Criterion met: **yes** — ahead of the baseline requires being strictly ahead *and* losing nothing. No tool the baseline proved is unproved here.

## By domain

| domain | entries | supported | partial | not tested | gap | no API | coverage |
|---|---|---|---|---|---|---|---|
| [`project`](#project) | 5 | 2 | 0 | 3 | 0 | 0 | 40.0 % |
| [`schematic`](#schematic) | 9 | 6 | 1 | 1 | 1 | 0 | 66.7 % |
| [`symbols`](#symbols) | 19 | 17 | 1 | 1 | 0 | 0 | 89.5 % |
| [`wires`](#wires) | 8 | 8 | 0 | 0 | 0 | 0 | 100.0 % |
| [`nets`](#nets) | 25 | 11 | 13 | 1 | 0 | 0 | 44.0 % |
| [`labels`](#labels) | 6 | 6 | 0 | 0 | 0 | 0 | 100.0 % |
| [`buses`](#buses) | 5 | 5 | 0 | 0 | 0 | 0 | 100.0 % |
| [`hierarchy`](#hierarchy) | 12 | 12 | 0 | 0 | 0 | 0 | 100.0 % |
| [`libraries`](#libraries) | 9 | 9 | 0 | 0 | 0 | 0 | 100.0 % |
| [`footprints`](#footprints) | 7 | 6 | 0 | 1 | 0 | 0 | 85.7 % |
| [`pcb`](#pcb) | 8 | 8 | 0 | 0 | 0 | 0 | 100.0 % |
| [`placement`](#placement) | 11 | 1 | 0 | 9 | 1 | 0 | 9.1 % |
| [`routing`](#routing) | 10 | 3 | 0 | 5 | 0 | 2 | 37.5 % |
| [`vias`](#vias) | 1 | 0 | 0 | 1 | 0 | 0 | 0.0 % |
| [`zones`](#zones) | 3 | 2 | 0 | 1 | 0 | 0 | 66.7 % |
| [`stackup`](#stackup) | 4 | 3 | 0 | 0 | 0 | 1 | 100.0 % |
| [`rules`](#rules) | 5 | 5 | 0 | 0 | 0 | 0 | 100.0 % |
| [`erc`](#erc) | 2 | 1 | 1 | 0 | 0 | 0 | 50.0 % |
| [`drc`](#drc) | 3 | 2 | 1 | 0 | 0 | 0 | 66.7 % |
| [`bom`](#bom) | 1 | 1 | 0 | 0 | 0 | 0 | 100.0 % |
| [`3d`](#3d) | 2 | 1 | 0 | 0 | 0 | 1 | 100.0 % |
| [`simulation`](#simulation) | 1 | 0 | 0 | 0 | 0 | 1 | — |
| [`manufacturing`](#manufacturing) | 3 | 1 | 2 | 0 | 0 | 0 | 33.3 % |
| [`gerber`](#gerber) | 1 | 1 | 0 | 0 | 0 | 0 | 100.0 % |
| [`drill`](#drill) | 1 | 1 | 0 | 0 | 0 | 0 | 100.0 % |
| [`pick_place`](#pick_place) | 1 | 1 | 0 | 0 | 0 | 0 | 100.0 % |
| [`datasheet`](#datasheet) | 2 | 2 | 0 | 0 | 0 | 0 | 100.0 % |
| [`sourcing`](#sourcing) | 5 | 5 | 0 | 0 | 0 | 0 | 100.0 % |
| [`export`](#export) | 11 | 10 | 1 | 0 | 0 | 0 | 90.9 % |
| [`review`](#review) | 6 | 0 | 6 | 0 | 0 | 0 | 0.0 % |
| [`config`](#config) | 7 | 7 | 0 | 0 | 0 | 0 | 100.0 % |
| [`templates`](#templates) | 4 | 4 | 0 | 0 | 0 | 0 | 100.0 % |
| [`task`](#task) | 4 | 4 | 0 | 0 | 0 | 0 | 100.0 % |
| [`plan`](#plan) | 2 | 2 | 0 | 0 | 0 | 0 | 100.0 % |
| [`ui`](#ui) | 3 | 0 | 0 | 3 | 0 | 0 | 0.0 % |
| [`graph`](#graph) | 3 | 0 | 3 | 0 | 0 | 0 | 0.0 % |

## Adapters

Which backend actually runs a call, and whether it needs KiCAD open. `ipc` has no file fallback: with no live KiCAD the call fails rather than editing the document, which is why unattended coverage of the PCB path is limited to what `kicad-cli` and the file engine can do.

| adapter | tools | needs a running KiCAD |
|---|---|---|
| `sexpr` | 124 | no |
| `cli` | 22 | no |
| `ipc` | 21 | yes |
| `ipc→sexpr` | 5 | no |
| `internal` | 19 | no |
| `external` | 8 | no |
| `process` | 3 | yes |

## Meta-tools

The always-visible gateway/discovery tools (`crates/konnect-core/src/router/meta_tools.rs`), classified separately from the domain tools above because none of their names carry a verb `tool_effect`'s table recognises — they would otherwise all fall back to `write`, which is the false positive that made the `read_only` bench tier reject `find_capabilities` and `load_tools`. `effect` means the same thing here as in the table above: whether the call can mutate the *project* on disk. A call that only changes this server's own session state — which tools `tools/list` currently exposes — is `read` by that measure, even though it does change something.

| tool | effect | why |
|---|---|---|
| `find_capabilities` | `read` | ranks tool names by relevance; writes nothing |
| `load_tools` | `read` | exposes tool names in tools/list; no project file touched |
| `kicad_describe` | `read` | hands out input schemas; no project file touched |
| `list_toolboxes` | `read` | lists toolset metadata and load state |
| `load_toolset` | `read` | exposes a toolset's tools in tools/list; session state only |
| `unload_toolset` | `read` | removes a toolset's tools from tools/list; session state only |
| `get_active_toolsets` | `read` | reads which toolsets are loaded |
| `get_recent_calls` | `read` | reads the shared call log |
| `server_stats` | `read` | reads uptime/call counters |
| `changes_since` | `read` | reads document revision state and the run journal; writes nothing |
| `kicad_invoke` | `write` | carries an arbitrary batch, including MANIFEST writers; D57: the audit keys on each inner call's own `tool` field, not on this name |
| `kicad_agent` | `write` | NO_LLM/ESCALATE/LOCAL touch only task state; `execute: true` applies a compiled Plan IR to `document` via agent_loop::execute — a real write |
| `kicad_agent_verify` | `read` | runs/reads cached kicad-cli ERC/DRC and records the verdict in task state; writes no project file |

## Detail

`effect` is `write` when a call can leave something behind — a project document, a file on disk (an export or a report counts), or the state of the loaded KiCAD — and `read` when it leaves nothing. It is derived from the tool's verb plus a short list of named exceptions, and a tool no rule covers is `write`: over-reporting a writer costs a refusal someone can see, while under-reporting one lets a mutation through a context that believed itself safe.

`write target` (only meaningful when `effect` is `write`) is `design_document` when the call can modify a source document of the design (`.kicad_sch`, `.kicad_pcb`, `.kicad_pro`, a project library) and `derived` when it writes only a fabrication artifact, a report, or this server's own state. `operating mode Manufacturing` (the design freeze) refuses `design_document` writes and allows `derived` ones; a tool no rule covers is `design_document` by the same fail-safe reasoning as `effect`.

### project

| tool | toolset | adapter | effect | write target | status | proof | evidence | note |
|---|---|---|---|---|---|---|---|---|
| `create_project` | `project` | `sexpr` | `write` | design_document | SUPPORTED | bench | `bench/probes/divider.yaml` |  |
| `open_project` | `project` | `ipc` | `write` | derived | NOT_TESTED | — | — |  |
| `save_project` | `project` | `ipc` | `write` | design_document | NOT_TESTED | — | — |  |
| `get_project_info` | `project` | `sexpr` | `read` | — | SUPPORTED | test | `crates/konnect-core/src/tools/project.rs` |  |
| `snapshot_project` | `project` | `internal` | `write` | derived | NOT_TESTED | gated | `crates/konnect-core/tests/sourcing_and_manufacturing.rs` |  |

### schematic

| tool | toolset | adapter | effect | write target | status | proof | evidence | note |
|---|---|---|---|---|---|---|---|---|
| `create_schematic` | `sch_components` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/src/tools/sch_components.rs` |  |
| `add_component_annotation` | `sch_components` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/src/tools/sch_components.rs` |  |
| `get_schematic_view` | `sch_components` | `cli` | `read` | — | NOT_TESTED | gated | `crates/konnect-core/tests/cli_tools.rs` |  |
| `find_orphan_items` | `sch_analysis` | `sexpr` | `read` | — | PARTIAL | test | `crates/konnect-core/tests/symbols_and_schematic.rs` | advisory: connectivity derived in-process, and it has disagreed with kicad-cli ERC (E7) — the verdict comes from run_erc / verify |
| `check_schematic_overlaps` | `sch_analysis` | `sexpr` | `read` | — | SUPPORTED | test | `crates/konnect-core/tests/symbols_and_schematic.rs` |  |
| `batch_delete` | `sch_batch` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/src/tools/sch_batch.rs` |  |
| `add_schematic_text` | `sch_batch` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/tests/symbols_and_schematic.rs` |  |
| `get_schematic_layout` | `sch_batch` | `sexpr` | `read` | — | SUPPORTED | test | `crates/konnect-core/tests/symbols_and_schematic.rs` |  |

Not covered by any tool:

| capability | status | why |
|---|---|---|
| edit a schematic that is open in the KiCad GUI | GAP | KiCad 10 registers only GetOpenDocuments on the schematic API (D3), so edits go to the file and the GUI must reload. Upstream lands this in KiCad 11 — the reason this fork does not patch KiCad |

### symbols

| tool | toolset | adapter | effect | write target | status | proof | evidence | note |
|---|---|---|---|---|---|---|---|---|
| `add_schematic_component` | `sch_components` | `sexpr` | `write` | design_document | SUPPORTED | bench | `bench/probes/symbol_lookup_cost.yaml` |  |
| `delete_schematic_component` | `sch_components` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/src/tools/sch_components.rs` |  |
| `edit_schematic_component` | `sch_components` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/src/tools/sch_components.rs` |  |
| `get_schematic_component` | `sch_components` | `sexpr` | `read` | — | SUPPORTED | bench | `bench/tasks/07_sch_inspection.yaml` |  |
| `list_schematic_components` | `sch_components` | `sexpr` | `read` | — | SUPPORTED | bench | `bench/probes/graph.yaml` |  |
| `move_schematic_component` | `sch_components` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/src/tools/sch_components.rs` |  |
| `rotate_schematic_component` | `sch_components` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/src/tools/sch_components.rs` |  |
| `move_connected` | `sch_components` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/tests/symbols_and_schematic.rs` |  |
| `move_region` | `sch_components` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/tests/symbols_and_schematic.rs` |  |
| `annotate_schematic` | `sch_components` | `cli` | `write` | design_document | NOT_TESTED | gated | `crates/konnect-core/tests/cli_tools.rs` |  |
| `get_schematic_pin_locations` | `sch_components` | `sexpr` | `read` | — | SUPPORTED | bench | `bench/tasks/07_sch_inspection.yaml` |  |
| `batch_get_schematic_pin_locations` | `sch_components` | `sexpr` | `read` | — | SUPPORTED | test | `crates/konnect-core/tests/symbols_and_schematic.rs` |  |
| `group_components` | `sch_components` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/tests/symbols_and_schematic.rs` |  |
| `replace_component` | `sch_components` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/src/tools/sch_components.rs` |  |
| `add_power_symbol` | `sch_wiring` | `sexpr` | `write` | design_document | PARTIAL | bench | `bench/probes/divider.yaml` | does not snap to the 1.27 mm grid (E6): a power symbol placed at a component's nominal coordinate lands 0.33 mm off the pin and ERC reports it unconnected. A plan's `power` operation snaps before calling it; the direct path does not |
| `batch_place_components` | `sch_batch` | `sexpr` | `write` | design_document | SUPPORTED | bench | `bench/probes/divider.yaml` |  |
| `bulk_move_schematic_components` | `sch_batch` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/tests/symbols_and_schematic.rs` |  |
| `batch_edit_schematic_components` | `sch_batch` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/tests/symbols_and_schematic.rs` |  |
| `batch_delete_schematic_components` | `sch_batch` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/tests/symbols_and_schematic.rs` |  |

### wires

| tool | toolset | adapter | effect | write target | status | proof | evidence | note |
|---|---|---|---|---|---|---|---|---|
| `add_wire` | `sch_wiring` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/src/tools/sch_wiring.rs` |  |
| `batch_add_wire` | `sch_wiring` | `sexpr` | `write` | design_document | SUPPORTED | bench | `bench/tasks/02_sch_ldo.yaml` |  |
| `delete_schematic_wire` | `sch_wiring` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/tests/nets_and_wires.rs` |  |
| `batch_delete_schematic_wire` | `sch_wiring` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/tests/nets_and_wires.rs` |  |
| `split_wire_at_point` | `sch_wiring` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/src/tools/sch_wiring.rs` |  |
| `add_junction` | `sch_wiring` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/tests/nets_and_wires.rs` |  |
| `batch_add_junction` | `sch_wiring` | `sexpr` | `write` | design_document | SUPPORTED | bench | `bench/tasks/02_sch_ldo.yaml` |  |
| `list_schematic_wires` | `sch_analysis` | `sexpr` | `read` | — | SUPPORTED | test | `crates/konnect-core/tests/nets_and_wires.rs` |  |

### nets

| tool | toolset | adapter | effect | write target | status | proof | evidence | note |
|---|---|---|---|---|---|---|---|---|
| `add_no_connect` | `sch_wiring` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/src/tools/sch_wiring.rs` |  |
| `delete_no_connect` | `sch_wiring` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/src/tools/sch_wiring.rs` |  |
| `batch_delete_no_connect` | `sch_wiring` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/src/tools/sch_wiring.rs` |  |
| `connect_to_net` | `sch_wiring` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/tests/nets_and_wires.rs` |  |
| `connect_pins` | `sch_wiring` | `sexpr` | `write` | design_document | SUPPORTED | bench | `bench/probes/divider.yaml` |  |
| `add_schematic_connection` | `sch_wiring` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/tests/nets_and_wires.rs` |  |
| `list_schematic_nets` | `sch_analysis` | `sexpr` | `read` | — | PARTIAL | bench | `bench/probes/divider.yaml` | advisory: connectivity derived in-process, and it has disagreed with kicad-cli ERC (E7) — the verdict comes from run_erc / verify |
| `get_net_connections` | `sch_analysis` | `sexpr` | `read` | — | PARTIAL | test | `crates/konnect-core/tests/nets_and_wires.rs` | advisory: connectivity derived in-process, and it has disagreed with kicad-cli ERC (E7) — the verdict comes from run_erc / verify |
| `get_net_connectivity` | `sch_analysis` | `sexpr` | `read` | — | PARTIAL | test | `crates/konnect-core/tests/nets_and_wires.rs` | advisory: connectivity derived in-process, and it has disagreed with kicad-cli ERC (E7) — the verdict comes from run_erc / verify |
| `get_pin_connections` | `sch_analysis` | `sexpr` | `read` | — | PARTIAL | test | `crates/konnect-core/tests/nets_and_wires.rs` | advisory: connectivity derived in-process, and it has disagreed with kicad-cli ERC (E7) — the verdict comes from run_erc / verify |
| `get_pin_net_name` | `sch_analysis` | `sexpr` | `read` | — | PARTIAL | test | `crates/konnect-core/tests/nets_and_wires.rs` | advisory: connectivity derived in-process, and it has disagreed with kicad-cli ERC (E7) — the verdict comes from run_erc / verify |
| `get_component_nets` | `sch_analysis` | `sexpr` | `read` | — | PARTIAL | test | `crates/konnect-core/tests/nets_and_wires.rs` | advisory: connectivity derived in-process, and it has disagreed with kicad-cli ERC (E7) — the verdict comes from run_erc / verify |
| `get_net_components` | `sch_analysis` | `sexpr` | `read` | — | PARTIAL | test | `crates/konnect-core/tests/nets_and_wires.rs` | advisory: connectivity derived in-process, and it has disagreed with kicad-cli ERC (E7) — the verdict comes from run_erc / verify |
| `trace_from_point` | `sch_analysis` | `sexpr` | `read` | — | PARTIAL | test | `crates/konnect-core/tests/nets_and_wires.rs` | advisory: connectivity derived in-process, and it has disagreed with kicad-cli ERC (E7) — the verdict comes from run_erc / verify |
| `find_shorted_nets` | `sch_analysis` | `sexpr` | `read` | — | PARTIAL | test | `crates/konnect-core/tests/nets_and_wires.rs` | advisory: connectivity derived in-process, and it has disagreed with kicad-cli ERC (E7) — the verdict comes from run_erc / verify |
| `find_single_pin_nets` | `sch_analysis` | `sexpr` | `read` | — | PARTIAL | bench | `bench/tasks/07_sch_inspection.yaml` | advisory: connectivity derived in-process, and it has disagreed with kicad-cli ERC (E7) — the verdict comes from run_erc / verify |
| `get_connected_items` | `sch_analysis` | `sexpr` | `read` | — | PARTIAL | test | `crates/konnect-core/src/tools/sch_analysis.rs` | advisory: connectivity derived in-process, and it has disagreed with kicad-cli ERC (E7) — the verdict comes from run_erc / verify |
| `batch_connect_to_net` | `sch_batch` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/src/plan/ops.rs` |  |
| `batch_connect_pins` | `sch_batch` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/src/tools/sch_batch.rs` |  |
| `connect_passthrough` | `sch_batch` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/tests/nets_and_wires.rs` |  |
| `validate_wire_connections` | `sch_batch` | `sexpr` | `read` | — | PARTIAL | test | `crates/konnect-core/tests/nets_and_wires.rs` | advisory: connectivity derived in-process, and it has disagreed with kicad-cli ERC (E7) — the verdict comes from run_erc / verify |
| `validate_component_connections` | `sch_batch` | `sexpr` | `read` | — | PARTIAL | test | `crates/konnect-core/src/tools/sch_batch.rs` | advisory: connectivity derived in-process, and it has disagreed with kicad-cli ERC (E7) — the verdict comes from run_erc / verify |
| `fix_connectivity` | `sch_export` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/tests/nets_and_wires.rs` |  |
| `add_net` | `pcb_routing` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/tests/config_and_rules.rs` |  |
| `get_nets_list` | `pcb_routing` | `ipc` | `read` | — | NOT_TESTED | — | — |  |

### labels

| tool | toolset | adapter | effect | write target | status | proof | evidence | note |
|---|---|---|---|---|---|---|---|---|
| `add_schematic_net_label` | `sch_wiring` | `sexpr` | `write` | design_document | SUPPORTED | bench | `bench/probes/divider.yaml` |  |
| `delete_schematic_net_label` | `sch_wiring` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/tests/board_and_labels.rs` |  |
| `rotate_schematic_label` | `sch_wiring` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/tests/board_and_labels.rs` |  |
| `move_labels_by_offset` | `sch_wiring` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/src/tools/sch_wiring.rs` |  |
| `batch_rotate_labels` | `sch_wiring` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/tests/board_and_labels.rs` |  |
| `list_schematic_labels` | `sch_analysis` | `sexpr` | `read` | — | SUPPORTED | test | `crates/konnect-core/src/tools/sch_wiring.rs` |  |

### buses

| tool | toolset | adapter | effect | write target | status | proof | evidence | note |
|---|---|---|---|---|---|---|---|---|
| `add_bus` | `sch_buses` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/src/tools/sch_buses.rs` |  |
| `add_bus_entry` | `sch_buses` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/src/tools/sch_buses.rs` |  |
| `add_bus_alias` | `sch_buses` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/src/tools/sch_buses.rs` |  |
| `list_buses` | `sch_buses` | `sexpr` | `read` | — | SUPPORTED | test | `crates/konnect-core/src/tools/sch_buses.rs` |  |
| `expand_bus` | `sch_buses` | `internal` | `read` | — | SUPPORTED | test | `crates/konnect-core/src/tools/sch_buses.rs` |  |

### hierarchy

| tool | toolset | adapter | effect | write target | status | proof | evidence | note |
|---|---|---|---|---|---|---|---|---|
| `add_hierarchical_sheet` | `sch_hierarchy` | `sexpr` | `write` | design_document | SUPPORTED | bench | `bench/tasks/04_sch_hierarchy.yaml` |  |
| `edit_sheet` | `sch_hierarchy` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/src/tools/sch_hierarchy.rs` |  |
| `move_sheet` | `sch_hierarchy` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/src/tools/sch_hierarchy.rs` |  |
| `delete_sheet` | `sch_hierarchy` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/src/tools/sch_hierarchy.rs` |  |
| `duplicate_sheet` | `sch_hierarchy` | `sexpr` | `write` | design_document | SUPPORTED | bench | `bench/tasks/04_sch_hierarchy.yaml` |  |
| `get_sheet_hierarchy` | `sch_hierarchy` | `sexpr` | `read` | — | SUPPORTED | bench | `bench/tasks/04_sch_hierarchy.yaml` |  |
| `renumber_sheet_pages` | `sch_hierarchy` | `sexpr` | `write` | design_document | SUPPORTED | bench | `bench/tasks/04_sch_hierarchy.yaml` |  |
| `import_sheet_pins` | `sch_hierarchy` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/src/tools/sch_hierarchy.rs` |  |
| `add_sheet_pin` | `sch_hierarchy` | `sexpr` | `write` | design_document | SUPPORTED | bench | `bench/tasks/04_sch_hierarchy.yaml` |  |
| `edit_sheet_pin` | `sch_hierarchy` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/src/tools/sch_hierarchy.rs` |  |
| `delete_sheet_pin` | `sch_hierarchy` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/src/tools/sch_hierarchy.rs` |  |
| `validate_sheet_pins` | `sch_hierarchy` | `sexpr` | `read` | — | SUPPORTED | bench | `bench/tasks/04_sch_hierarchy.yaml` |  |

### libraries

| tool | toolset | adapter | effect | write target | status | proof | evidence | note |
|---|---|---|---|---|---|---|---|---|
| `register_footprint_library` | `library` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/tests/libraries_and_footprints.rs` |  |
| `list_footprint_libraries` | `library` | `sexpr` | `read` | — | SUPPORTED | test | `crates/konnect-core/src/tools/library.rs` |  |
| `create_symbol` | `library` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/src/tools/library.rs` |  |
| `delete_symbol` | `library` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/tests/libraries_and_footprints.rs` |  |
| `list_symbols_in_library` | `library` | `sexpr` | `read` | — | SUPPORTED | test | `crates/konnect-core/src/tools/library.rs` |  |
| `register_symbol_library` | `library` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/tests/libraries_and_footprints.rs` |  |
| `list_symbol_libraries` | `library` | `sexpr` | `read` | — | SUPPORTED | test | `crates/konnect-core/tests/libraries_and_footprints.rs` |  |
| `search_symbols` | `library` | `sexpr` | `read` | — | SUPPORTED | bench | `bench/probes/discover.yaml` |  |
| `get_symbol_info` | `library` | `sexpr` | `read` | — | SUPPORTED | bench | `bench/probes/discover.yaml` |  |

### footprints

| tool | toolset | adapter | effect | write target | status | proof | evidence | note |
|---|---|---|---|---|---|---|---|---|
| `get_component_pads` | `pcb_components` | `sexpr` | `read` | — | SUPPORTED | test | `crates/konnect-core/tests/libraries_and_footprints.rs` |  |
| `get_pad_position` | `pcb_components` | `sexpr` | `read` | — | SUPPORTED | test | `crates/konnect-core/tests/libraries_and_footprints.rs` |  |
| `create_footprint` | `library` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/src/tools/library.rs` |  |
| `edit_footprint_pad` | `library` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/tests/libraries_and_footprints.rs` |  |
| `list_library_footprints` | `library` | `sexpr` | `read` | — | SUPPORTED | test | `crates/konnect-core/tests/libraries_and_footprints.rs` |  |
| `get_footprint_info` | `library` | `sexpr` | `read` | — | SUPPORTED | test | `crates/konnect-core/tests/libraries_and_footprints.rs` |  |
| `search_footprints` | `library` | `sexpr` | `read` | — | NOT_TESTED | gated | `crates/konnect-core/tests/libraries_and_footprints.rs` |  |

### pcb

| tool | toolset | adapter | effect | write target | status | proof | evidence | note |
|---|---|---|---|---|---|---|---|---|
| `set_board_size` | `pcb_board` | `ipc→sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/tests/board_and_labels.rs` |  |
| `get_board_info` | `pcb_board` | `sexpr` | `read` | — | SUPPORTED | test | `crates/konnect-core/tests/board_and_labels.rs` |  |
| `get_board_extents` | `pcb_board` | `ipc→sexpr` | `read` | — | SUPPORTED | test | `crates/konnect-core/tests/board_and_labels.rs` |  |
| `add_board_outline` | `pcb_board` | `ipc→sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/tests/board_and_labels.rs` |  |
| `add_mounting_hole` | `pcb_board` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/tests/board_and_labels.rs` |  |
| `add_board_text` | `pcb_board` | `ipc→sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/tests/board_and_labels.rs` |  |
| `import_svg_logo` | `pcb_board` | `ipc→sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/src/tools/pcb_board.rs` |  |
| `get_board_2d_view` | `pcb_components` | `cli` | `read` | — | SUPPORTED | test | `crates/konnect-core/tests/cli_tools.rs` |  |

### placement

| tool | toolset | adapter | effect | write target | status | proof | evidence | note |
|---|---|---|---|---|---|---|---|---|
| `place_component` | `pcb_components` | `ipc` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/src/tools/pcb_components.rs` |  |
| `move_component` | `pcb_components` | `ipc` | `write` | design_document | NOT_TESTED | gated | `crates/konnect/tests/live_kicad_tools.rs` |  |
| `rotate_component` | `pcb_components` | `ipc` | `write` | design_document | NOT_TESTED | — | — |  |
| `delete_component` | `pcb_components` | `ipc` | `write` | design_document | NOT_TESTED | — | — |  |
| `edit_component` | `pcb_components` | `ipc` | `write` | design_document | NOT_TESTED | gated | `crates/konnect/tests/live_kicad_tools.rs` |  |
| `find_component` | `pcb_components` | `ipc` | `read` | — | NOT_TESTED | — | — |  |
| `get_component_list` | `pcb_components` | `ipc` | `read` | — | NOT_TESTED | — | — |  |
| `place_component_array` | `pcb_components` | `ipc` | `write` | design_document | NOT_TESTED | gated | `crates/konnect/tests/live_kicad_tools.rs` |  |
| `align_components` | `pcb_components` | `ipc` | `write` | design_document | NOT_TESTED | gated | `crates/konnect/tests/live_kicad_tools.rs` |  |
| `duplicate_component` | `pcb_components` | `ipc` | `write` | design_document | NOT_TESTED | — | — |  |

Not covered by any tool:

| capability | status | why |
|---|---|---|
| automatic component placement | GAP | placement is caller-driven: every position is a coordinate someone chose. No autoplacer, and none in KiCad either |

### routing

| tool | toolset | adapter | effect | write target | status | proof | evidence | note |
|---|---|---|---|---|---|---|---|---|
| `route_trace` | `pcb_routing` | `ipc` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/tests/integration_test.rs` |  |
| `route_pad_to_pad` | `pcb_routing` | `ipc` | `write` | design_document | NOT_TESTED | — | — |  |
| `delete_trace` | `pcb_routing` | `ipc` | `write` | design_document | NOT_TESTED | — | — |  |
| `query_traces` | `pcb_routing` | `ipc` | `read` | — | NOT_TESTED | — | — |  |
| `modify_trace` | `pcb_routing` | `ipc` | `write` | design_document | NOT_TESTED | — | — |  |
| `route_differential_pair` | `pcb_routing` | `ipc` | `write` | design_document | NOT_TESTED | — | — | one straight segment per net, offset perpendicular by (gap + width) / 2: no length matching, no skew budget, no impedance target and no vias |
| `autoroute` | `integration` | `external` | `write` | design_document | GUI_ONLY_NO_API | test | `crates/konnect-core/src/tools/integration.rs` | kicad-cli 10 dropped Specctra DSN export and SES import, so the Freerouting round trip exists only in the PCB editor; the handler always fails and names the GUI steps |
| `check_freerouting` | `integration` | `external` | `read` | — | EXTERNAL_TOOL | test | `crates/konnect-core/tests/sourcing_and_manufacturing.rs` |  |
| `copy_routing_pattern` | `verification` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/tests/sourcing_and_manufacturing.rs` |  |

Not covered by any tool:

| capability | status | why |
|---|---|---|
| interactive push-and-shove routing | GUI_ONLY_NO_API | the router lives in the PCB editor; IPC creates track segments but does not drive the interactive router |

### vias

| tool | toolset | adapter | effect | write target | status | proof | evidence | note |
|---|---|---|---|---|---|---|---|---|
| `add_via` | `pcb_routing` | `ipc` | `write` | design_document | NOT_TESTED | — | — |  |

### zones

| tool | toolset | adapter | effect | write target | status | proof | evidence | note |
|---|---|---|---|---|---|---|---|---|
| `add_zone` | `pcb_board` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/src/mcp/error.rs` |  |
| `add_copper_pour` | `pcb_routing` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/tests/board_and_labels.rs` |  |
| `refill_zones` | `pcb_export` | `ipc` | `write` | design_document | NOT_TESTED | — | — |  |

### stackup

| tool | toolset | adapter | effect | write target | status | proof | evidence | note |
|---|---|---|---|---|---|---|---|---|
| `get_layer_list` | `pcb_board` | `sexpr` | `read` | — | SUPPORTED | test | `crates/konnect-core/tests/board_and_labels.rs` |  |
| `add_layer` | `pcb_board` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/tests/board_and_labels.rs` |  |
| `set_active_layer` | `pcb_board` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/tests/board_and_labels.rs` |  |

Not covered by any tool:

| capability | status | why |
|---|---|---|
| write the board stackup (material, thickness, dielectric) | GUI_ONLY_NO_API | `UpdateBoardStackup` is declared in KiCad 10's board protos and marked '**not yet implemented**' there (crates/konnect-ipc/proto/board/board_commands.proto, pinned by that crate's stackup_write_is_unimplemented test); the stackup is read-only over IPC and editable only in the GUI |

### rules

| tool | toolset | adapter | effect | write target | status | proof | evidence | note |
|---|---|---|---|---|---|---|---|---|
| `create_netclass` | `pcb_routing` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/tests/config_and_rules.rs` |  |
| `assign_net_to_class` | `pcb_routing` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/tests/config_and_rules.rs` |  |
| `set_design_rules` | `verification` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/tests/config_and_rules.rs` |  |
| `get_design_rules` | `verification` | `sexpr` | `read` | — | SUPPORTED | test | `crates/konnect-core/tests/config_and_rules.rs` |  |
| `set_layer_constraints` | `verification` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/tests/config_and_rules.rs` |  |

### erc

| tool | toolset | adapter | effect | write target | status | proof | evidence | note |
|---|---|---|---|---|---|---|---|---|
| `run_erc` | `sch_export` | `cli` | `write` | derived | SUPPORTED | bench | `bench/probes/divider.yaml` |  |

Not covered by any tool:

| capability | status | why |
|---|---|---|
| ERC on a schematic open in the GUI, or incremental ERC | PARTIAL | run_erc is a kicad-cli process over the file on disk: ~1.1 s per call and blind to unsaved GUI state |

### drc

| tool | toolset | adapter | effect | write target | status | proof | evidence | note |
|---|---|---|---|---|---|---|---|---|
| `get_drc_violations` | `pcb_export` | `cli` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/tests/cli_tools.rs` |  |
| `run_drc` | `verification` | `cli` | `write` | derived | SUPPORTED | test | `crates/konnect-core/tests/cli_tools.rs` |  |
| `check_clearance` | `verification` | `sexpr` | `read` | — | PARTIAL | test | `crates/konnect-core/tests/sourcing_and_manufacturing.rs` | geometric clearance computed in-process from the file, against no rule set — kicad-cli DRC is the verdict |

### bom

| tool | toolset | adapter | effect | write target | status | proof | evidence | note |
|---|---|---|---|---|---|---|---|---|
| `export_bom` | `sch_export` | `cli` | `write` | derived | SUPPORTED | bench | `bench/tasks/05_manufacturing_exports.yaml` |  |

### 3d

| tool | toolset | adapter | effect | write target | status | proof | evidence | note |
|---|---|---|---|---|---|---|---|---|
| `export_3d` | `pcb_export` | `cli` | `write` | derived | SUPPORTED | test | `crates/konnect-core/tests/cli_tools.rs` |  |

Not covered by any tool:

| capability | status | why |
|---|---|---|
| 3D viewer control and rendered board images | GUI_ONLY_NO_API | export_3d writes STEP/GLB/VRML geometry; rendering a picture of the board is the GUI's 3D viewer |

### simulation

Not covered by any tool:

| capability | status | why |
|---|---|---|
| run an ngspice simulation and read results | GUI_ONLY_NO_API | kicad-cli 10 has no simulation verb and the IPC protos expose none; only the GUI simulator runs one. `generate_netlist --format spice` produces the input file |

### manufacturing

| tool | toolset | adapter | effect | write target | status | proof | evidence | note |
|---|---|---|---|---|---|---|---|---|
| `export_manufacturing_package` | `manufacturing` | `cli` | `write` | derived | SUPPORTED | test | `crates/konnect-core/tests/cli_tools.rs` |  |
| `validate_for_manufacturing` | `manufacturing` | `sexpr` | `read` | — | PARTIAL | test | `crates/konnect-core/tests/sourcing_and_manufacturing.rs` | heuristic audit, not a validator — ERC/DRC decide whether a design is sound |
| `estimate_cost` | `manufacturing` | `internal` | `read` | — | PARTIAL | test | `crates/konnect-core/tests/sourcing_and_manufacturing.rs` | an order-of-magnitude estimate from stored per-fab-house rates, not a quote |

### gerber

| tool | toolset | adapter | effect | write target | status | proof | evidence | note |
|---|---|---|---|---|---|---|---|---|
| `export_gerber` | `pcb_export` | `cli` | `write` | derived | SUPPORTED | test | `crates/konnect-core/tests/cli_tools.rs` |  |

### drill

| tool | toolset | adapter | effect | write target | status | proof | evidence | note |
|---|---|---|---|---|---|---|---|---|
| `export_drill` | `pcb_export` | `cli` | `write` | derived | SUPPORTED | test | `crates/konnect-core/src/tools/pcb_export.rs` |  |

### pick_place

| tool | toolset | adapter | effect | write target | status | proof | evidence | note |
|---|---|---|---|---|---|---|---|---|
| `export_position_file` | `pcb_export` | `cli` | `write` | derived | SUPPORTED | test | `crates/konnect-core/tests/cli_tools.rs` |  |

### datasheet

| tool | toolset | adapter | effect | write target | status | proof | evidence | note |
|---|---|---|---|---|---|---|---|---|
| `enrich_datasheets` | `integration` | `sexpr` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/tests/sourcing_and_manufacturing.rs` |  |
| `get_datasheet_url` | `integration` | `external` | `read` | — | EXTERNAL_TOOL | test | `crates/konnect-core/tests/sourcing_and_manufacturing.rs` |  |

### sourcing

| tool | toolset | adapter | effect | write target | status | proof | evidence | note |
|---|---|---|---|---|---|---|---|---|
| `download_jlcpcb_database` | `integration` | `external` | `write` | design_document | EXTERNAL_TOOL | test | `crates/konnect-core/tests/sourcing_and_manufacturing.rs` |  |
| `search_jlcpcb_parts` | `integration` | `external` | `read` | — | EXTERNAL_TOOL | test | `crates/konnect-core/src/tools/integration.rs` |  |
| `get_jlcpcb_part` | `integration` | `external` | `read` | — | EXTERNAL_TOOL | test | `crates/konnect-core/src/tools/integration.rs` |  |
| `suggest_jlcpcb_alternatives` | `integration` | `external` | `read` | — | EXTERNAL_TOOL | test | `crates/konnect-core/tests/sourcing_and_manufacturing.rs` |  |
| `get_jlcpcb_database_stats` | `integration` | `external` | `read` | — | EXTERNAL_TOOL | test | `crates/konnect-core/tests/sourcing_and_manufacturing.rs` |  |

### export

| tool | toolset | adapter | effect | write target | status | proof | evidence | note |
|---|---|---|---|---|---|---|---|---|
| `export_schematic_svg` | `sch_export` | `cli` | `write` | derived | SUPPORTED | bench | `bench/tasks/05_manufacturing_exports.yaml` |  |
| `export_schematic_pdf` | `sch_export` | `cli` | `write` | derived | SUPPORTED | test | `crates/konnect-core/tests/cli_tools.rs` |  |
| `generate_netlist` | `sch_export` | `cli` | `write` | derived | SUPPORTED | bench | `bench/tasks/05_manufacturing_exports.yaml` |  |
| `export_netlist_summary` | `sch_export` | `sexpr` | `write` | derived | PARTIAL | test | `crates/konnect-core/tests/sourcing_and_manufacturing.rs` | advisory: connectivity derived in-process, and it has disagreed with kicad-cli ERC (E7) — the verdict comes from run_erc / verify |
| `export_pdf` | `pcb_export` | `cli` | `write` | derived | SUPPORTED | test | `crates/konnect-core/tests/cli_tools.rs` |  |
| `export_svg` | `pcb_export` | `cli` | `write` | derived | SUPPORTED | test | `crates/konnect-core/tests/cli_tools.rs` |  |
| `export_netlist` | `pcb_export` | `cli` | `write` | derived | SUPPORTED | test | `crates/konnect-core/tests/cli_tools.rs` |  |
| `export_dxf` | `pcb_export` | `cli` | `write` | derived | SUPPORTED | test | `crates/konnect-core/src/tools/pcb_export.rs` |  |
| `export_gencad` | `pcb_export` | `cli` | `write` | derived | SUPPORTED | test | `crates/konnect-core/src/tools/pcb_export.rs` |  |
| `export_ipc2581` | `pcb_export` | `cli` | `write` | derived | SUPPORTED | test | `crates/konnect-core/src/tools/pcb_export.rs` |  |
| `export_odb` | `pcb_export` | `cli` | `write` | derived | SUPPORTED | test | `crates/konnect-core/src/tools/pcb_export.rs` |  |

### review

| tool | toolset | adapter | effect | write target | status | proof | evidence | note |
|---|---|---|---|---|---|---|---|---|
| `audit_decoupling` | `design_review` | `sexpr` | `read` | — | PARTIAL | test | `crates/konnect-core/tests/design_review.rs` | heuristic audit, not a validator — ERC/DRC decide whether a design is sound |
| `audit_connections` | `design_review` | `sexpr` | `read` | — | PARTIAL | test | `crates/konnect-core/tests/design_review.rs` | heuristic audit, not a validator — ERC/DRC decide whether a design is sound |
| `audit_power_rails` | `design_review` | `sexpr` | `read` | — | PARTIAL | test | `crates/konnect-core/tests/design_review.rs` | heuristic audit, not a validator — ERC/DRC decide whether a design is sound |
| `audit_manufacturing` | `design_review` | `sexpr` | `read` | — | PARTIAL | test | `crates/konnect-core/tests/design_review.rs` | heuristic audit, not a validator — ERC/DRC decide whether a design is sound |
| `run_design_review` | `design_review` | `sexpr` | `read` | — | PARTIAL | test | `crates/konnect-core/tests/design_review.rs` | heuristic audit, not a validator — ERC/DRC decide whether a design is sound |
| `check_bom_health` | `design_review` | `sexpr` | `read` | — | PARTIAL | test | `crates/konnect-core/tests/design_review.rs` | heuristic audit, not a validator — ERC/DRC decide whether a design is sound |

### config

| tool | toolset | adapter | effect | write target | status | proof | evidence | note |
|---|---|---|---|---|---|---|---|---|
| `load_user_config` | `config` | `internal` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/tests/config_and_rules.rs` |  |
| `save_user_config` | `config` | `internal` | `write` | derived | SUPPORTED | test | `crates/konnect-core/tests/config_and_rules.rs` |  |
| `load_project_config` | `config` | `internal` | `read` | — | SUPPORTED | test | `crates/konnect-core/tests/config_and_rules.rs` |  |
| `save_project_config` | `config` | `internal` | `write` | derived | SUPPORTED | test | `crates/konnect-core/tests/config_and_rules.rs` |  |
| `get_effective_config` | `config` | `internal` | `read` | — | SUPPORTED | test | `crates/konnect-core/tests/config_and_rules.rs` |  |
| `add_design_rule` | `config` | `internal` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/tests/config_and_rules.rs` |  |
| `list_design_rules` | `config` | `internal` | `read` | — | SUPPORTED | test | `crates/konnect-core/tests/config_and_rules.rs` |  |

### templates

| tool | toolset | adapter | effect | write target | status | proof | evidence | note |
|---|---|---|---|---|---|---|---|---|
| `search_templates` | `templates` | `internal` | `read` | — | SUPPORTED | bench | `bench/probes/discover.yaml` |  |
| `get_template` | `templates` | `internal` | `read` | — | SUPPORTED | test | `crates/konnect-core/tests/board_and_labels.rs` |  |
| `apply_template` | `templates` | `sexpr` | `write` | design_document | SUPPORTED | bench | `bench/tasks/03_sch_template_stm32.yaml` |  |
| `list_template_categories` | `templates` | `internal` | `read` | — | SUPPORTED | bench | `bench/probes/discover.yaml` |  |

### task

| tool | toolset | adapter | effect | write target | status | proof | evidence | note |
|---|---|---|---|---|---|---|---|---|
| `start_task` | `task` | `internal` | `write` | derived | SUPPORTED | test | `crates/konnect-core/src/tools/plan.rs` |  |
| `update_task` | `task` | `internal` | `write` | derived | SUPPORTED | test | `crates/konnect-core/src/tools/task.rs` |  |
| `get_task` | `task` | `internal` | `read` | — | SUPPORTED | test | `crates/konnect-core/src/tools/task.rs` |  |
| `list_tasks` | `task` | `internal` | `read` | — | SUPPORTED | test | `crates/konnect/tests/protocol_stdio.rs` |  |

### plan

| tool | toolset | adapter | effect | write target | status | proof | evidence | note |
|---|---|---|---|---|---|---|---|---|
| `preview_plan` | `plan` | `internal` | `read` | — | SUPPORTED | test | `crates/konnect-core/src/tools/plan.rs` |  |
| `apply_plan` | `plan` | `internal` | `write` | design_document | SUPPORTED | test | `crates/konnect-core/src/tools/plan.rs` |  |

### ui

| tool | toolset | adapter | effect | write target | status | proof | evidence | note |
|---|---|---|---|---|---|---|---|---|
| `open_schematic_viewer` | `project` | `process` | `write` | derived | NOT_TESTED | — | — |  |
| `check_kicad_ui` | `verification` | `process` | `read` | — | NOT_TESTED | — | — |  |
| `launch_kicad_ui` | `verification` | `process` | `write` | design_document | NOT_TESTED | — | — |  |

### graph

| tool | toolset | adapter | effect | write target | status | proof | evidence | note |
|---|---|---|---|---|---|---|---|---|
| `graph_query` | `graph` | `sexpr` | `read` | — | PARTIAL | bench | `bench/probes/graph.yaml` | indexes only what the documents state — no .kicad_sch item ever carries a derived `net`; the connectivity verdict comes from run_erc, never from this tool (E7) |
| `graph_neighbors` | `graph` | `sexpr` | `read` | — | PARTIAL | test | `crates/konnect-core/src/tools/graph.rs` | indexes only what the documents state — no .kicad_sch item ever carries a derived `net`; the connectivity verdict comes from run_erc, never from this tool (E7) |
| `graph_stats` | `graph` | `sexpr` | `read` | — | PARTIAL | bench | `bench/probes/graph.yaml` | indexes only what the documents state — no .kicad_sch item ever carries a derived `net`; the connectivity verdict comes from run_erc, never from this tool (E7) |

## Not tested

26 of 202 registered tools have no proof that runs. `gated` means a test exists and is `#[ignore]`d — it needs a live KiCAD GUI, its IPC socket, or the installed libraries.

| tool | domain | adapter | proof found |
|---|---|---|---|
| `open_project` | `project` | `ipc` | none |
| `save_project` | `project` | `ipc` | none |
| `snapshot_project` | `project` | `internal` | gated — `crates/konnect-core/tests/sourcing_and_manufacturing.rs` |
| `open_schematic_viewer` | `ui` | `process` | none |
| `annotate_schematic` | `symbols` | `cli` | gated — `crates/konnect-core/tests/cli_tools.rs` |
| `get_schematic_view` | `schematic` | `cli` | gated — `crates/konnect-core/tests/cli_tools.rs` |
| `move_component` | `placement` | `ipc` | gated — `crates/konnect/tests/live_kicad_tools.rs` |
| `rotate_component` | `placement` | `ipc` | none |
| `delete_component` | `placement` | `ipc` | none |
| `edit_component` | `placement` | `ipc` | gated — `crates/konnect/tests/live_kicad_tools.rs` |
| `find_component` | `placement` | `ipc` | none |
| `get_component_list` | `placement` | `ipc` | none |
| `place_component_array` | `placement` | `ipc` | gated — `crates/konnect/tests/live_kicad_tools.rs` |
| `align_components` | `placement` | `ipc` | gated — `crates/konnect/tests/live_kicad_tools.rs` |
| `duplicate_component` | `placement` | `ipc` | none |
| `route_pad_to_pad` | `routing` | `ipc` | none |
| `add_via` | `vias` | `ipc` | none |
| `delete_trace` | `routing` | `ipc` | none |
| `query_traces` | `routing` | `ipc` | none |
| `get_nets_list` | `nets` | `ipc` | none |
| `modify_trace` | `routing` | `ipc` | none |
| `route_differential_pair` | `routing` | `ipc` | none |
| `refill_zones` | `zones` | `ipc` | none |
| `search_footprints` | `footprints` | `sexpr` | gated — `crates/konnect-core/tests/libraries_and_footprints.rs` |
| `check_kicad_ui` | `ui` | `process` | none |
| `launch_kicad_ui` | `ui` | `process` | none |

