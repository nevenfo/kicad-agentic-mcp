# Upstream correctness audit

**Baseline** `5cd6454` (fork point, `Merge pull request #132`) · **Upstream** `mixelpixx/Konnect`, remote `upstream`, ref `upstream/main` · **Date** 2026-08-24

## Scope

This is a differential audit of upstream's correctness and safety fixes, not a synchronisation.
Only changes with a direct effect on trust in the tool's output are considered: data corruption or
loss, wrong connectivity, wrong symbol or net resolution, false success, wrong ERC/DRC results,
KiCad incompatibility, wrong exports, and round-trip infidelity with a functional consequence.
Features, documentation, dependabot, packaging, refactors and agent tooling are out of scope and
are not listed.

PR #144 (`lib_name` / `find_lib_symbol` / `unmodelled_children` / `paper_args`) and PR #209
(user paper size) are being backported outside this audit and are deliberately not classified here.

## Method

For each candidate: read the upstream net diff (`git diff <merge>^1 <merge>`), identify the exact
faulty mechanism upstream fixed, then locate that mechanism in this fork with `rg` — the fork has
renamed and reimplemented parts of the tree, so no path is assumed — and conclude *still present* /
*already fixed here* / *code absent*. A `BACKPORT NOW` verdict requires high value, low-to-medium
complexity, and applicability proven by a `file:line` citation in this fork. Everything else is
`LATER` with a precise next action, or `NOT APPLICABLE` with the reason.

Two upstream fixes that are not merge commits surfaced while reading the same files and are
included because they dominate their neighbours in value: `e7eeeac` (DRC report categories) and
`9a56233` (netclasses written into the board). Appendix A lists the rest of upstream's
direct-to-main fix commits, which this bounded audit did not triage.

## Summary

| PR / commit | Subject | Category | Verdict |
|---|---|---|---|
| `e7eeeac` | DRC report drops `unconnected_items` and `schematic_parity` | false success | **BACKPORT NOW** |
| `9a56233` + #220 | `create_netclass` writes into the board, not the project | corruption | **BACKPORT NOW** |
| #174 | s-expression escapes decoded in several passes | corruption | **BACKPORT NOW** |
| #262 | power symbols absent from the schematic net graph | connectivity | **BACKPORT NOW** |
| #297 + #298 | only the first item of an ERC/DRC violation is kept | wrong ERC/DRC | **BACKPORT NOW** |
| #153 (partial) | `add_layer` writes an unopenable board | corruption | **BACKPORT NOW** |
| #142 | KiCad 10 net nodes read by fixed index | net resolution | **BACKPORT NOW** |
| #139 | BOM ignores `exclude_dnp` and `format` | false success / wrong export | **BACKPORT NOW** |
| #266 | PCB plot passes `--layers` per layer, no `--mode-single` | wrong export | **BACKPORT NOW** |
| #263 | `run_erc` on a sub-sheet reports invocation artefacts | wrong ERC | **BACKPORT NOW** |
| #212 | one junction dot emitted per wire, not per T | corruption | **BACKPORT NOW** |
| #213 | `#PWR` numbered by count, duplicating a live designator | wrong resolution | **BACKPORT NOW** |
| #214 | deleted wires leave junction dots behind | connectivity | **BACKPORT NOW** |
| #274 | footprint pad count read from source text | wrong result | **BACKPORT NOW** |
| #140 | `validate_for_manufacturing` counts by substring | wrong result | **BACKPORT NOW** |
| #271 | `find_orphan_items` consults no pins | wrong result | LATER |
| #179 | edits and pin lookups hit only the first unit | connectivity | LATER |
| #185 | design review approves on partial coverage | false success | LATER |
| #148 | net-label stub driven into the symbol body | round-trip | LATER |
| #186 | instance fields placed at fixed offsets | round-trip | LATER |
| #138 (residual) | drill `--output` without a trailing separator | wrong export | LATER |
| #162 | `query_traces` returns no UUID | — | LATER |
| #136 | symbols resolved through `sym-lib-table` | symbol resolution | NOT APPLICABLE |
| #149 | symbol body sized to fit pin names | — | NOT APPLICABLE |

---

## BACKPORT NOW

### `e7eeeac` — the DRC report drops two of its three categories

**Upstream mechanism.** `kicad-cli pcb drc --format json` emits three sibling arrays:
`violations`, `unconnected_items` and `schematic_parity`. The parser read `violations` only, so an
unrouted net — reported under `unconnected_items` at severity `error` — was invisible to every
caller. The fix introduces a `DrcReport` with `all()`, `error_count()` and `missing_categories()`,
the last so a gate can tell "no findings" from "the category was never reported".

**State in this fork.** Still present. `crates/konnect-core/src/tools/cli.rs:176` declares
`run_drc(...) -> Result<Vec<DrcViolation>>` and lines 198–214 map `raw.get("violations")` alone.
Consumers: `crates/konnect-core/src/evidence/validators.rs:148`,
`crates/konnect-core/src/tools/pcb_export.rs:662`,
`crates/konnect-core/src/tools/verification.rs:192`.

**Impact.** A board with unrouted copper passes `run_drc`, `get_drc_violations`, the manufacturing
export gate and — worst — the evidence validator, which is this fork's own approval mechanism. This
is the purest false success in the set: the tool reports a clean board and the fab receives an open
net. Frequency is high; every partially routed board hits it.

**Cost / risk.** ~120 lines in `cli.rs` plus three call sites. The signature change from
`Vec<DrcViolation>` to `DrcReport` is compiler-enforced, so no silent breakage. Low regression risk.

### `9a56233` + #220 `1677f69` — `create_netclass` writes into the board file

**Upstream mechanism.** `create_netclass` inserted a `(netclass …)` node into the `.kicad_pcb`. On any
modern board the reachable branch placed it as a direct child of `(kicad_pcb`, a token pcbnew
rejects, so the board no longer loaded. It was wrong twice over: KiCad has kept net classes in the
project's `.kicad_pro` (`net_settings`) since v7, so even a syntactically valid insert went where
nothing reads. Upstream moved both `create_netclass` and `assign_net_to_class` to the sibling
`.kicad_pro`, refusing a board with no project file. #220 then stopped the update path folding
creation defaults in — naming only `trace_width` on an existing class used to reset the clearance,
drill and via size the caller had tuned.

**State in this fork.** Still present, in the pre-`9a56233` form.
`crates/konnect-core/src/tools/pcb_routing.rs:643` builds `netclass_sexp` and inserts it at
`content.rfind(')')` when no `(net_classes` block is found (lines 657–678). The fork never touches
`.kicad_pro`, so `save_project_settings` does not exist here either.

**Impact.** Corruption: a call that reports success leaves a board KiCad refuses to open. On the
branch that does find `(net_classes`, the class is written where KiCad never reads it — a silent
no-op dressed as success. Both are user-visible on the first call.

**Cost / risk.** ~250 lines: rewrite two handlers against the project JSON, add the "no sibling
`.kicad_pro`" refusal, then apply #220's `FIELDS` table on top. Self-contained in `pcb_routing.rs`.
Medium complexity, low regression risk — the board file stops being written at all.

### #174 `28833f4` — string escapes decoded in several passes

**Upstream mechanism.** `unescape` chained `.replace()` calls, so a backslash unescaped by an earlier
pass was re-read as the introducer of a later escape. Upstream replaced it with a single
left-to-right scan.

**State in this fork.** Still present. `crates/konnect-sexp/src/parser.rs:160` runs
`\\"` → `\n` → `\t` → `\\` in sequence: `C:\\new` decodes to `C:\` followed by a newline.
`crates/konnect-schematic-editor/src/sexp/parser.rs:67-82` already does the single correct pass, so
only `konnect-sexp` is affected.

**Impact.** Any Windows path, regex or datasheet URL carrying `\\n` or `\\t` in a property value is
silently mangled on read and written back mangled. Corruption on round-trip, invisible until the file
is reopened.

**Cost / risk.** ~30 lines in one function, with tests. Trivial.

### #262 `6d394a4` — power symbols name nets, and the graph does not know it

**Upstream mechanism.** `build_net_graph` was fed `extract_labels` only. A `power:GND` symbol names
the net it touches exactly as a label does — KiCad takes the name from the placed symbol's `Value` —
so every power rail came back unnamed and unconnected. Upstream added `extract_power_symbol_labels`
(only `power_in` pins name a net, which keeps `PWR_FLAG` from renaming the rail it flags) and
`extract_all_net_labels`, routed every net-graph consumer through it, and deleted the `sch_bridge`
conversion layer. `find_orphan_items` was deliberately left on `extract_labels`.

**State in this fork.** Still present, and the mechanism is intact.
`crates/konnect-sexp/src/schematic.rs:161-166` declares `LabelKind::PowerSymbol`, but nothing in the
tree ever constructs it — the variant is dead. `LibPin`
(`crates/konnect-sexp/src/schematic.rs:324-332`) carries no `electrical_type`, and `parse_lib_pin`
(`crates/konnect-sexp/src/schematic.rs:399-425`) never reads `(pin <type> …)`.
`crates/konnect-core/src/tools/sch_bridge.rs` is still wired in at
`crates/konnect-core/src/tools/mod.rs:20` and used by `sch_analysis.rs` at lines 365, 389, 574, 602
and 632; `handle_list_nets` (`crates/konnect-core/src/tools/sch_analysis.rs:314-332`) still builds its
list from `cse::Schematic`'s three label vectors.

**Impact.** Every net-connectivity answer about a power rail is wrong: `list_nets` omits `GND` and
`+3V3`, `get_net_connections` reports zero connected points, `find_single_pin_nets` and
`find_shorted_nets` reason over a graph missing the most connected nets on the sheet. This is the
largest connectivity defect in the set, and it fires on essentially every real design.

**Cost / risk.** ~250 lines: `electrical_type` on `LibPin`, the two extractors in
`konnect-sexp/src/schematic.rs` (the fork already has `find_lib_symbol`, `extract_lib_pins_for_unit`,
`pin_transform` and `pin_endpoint`, so no new primitives are needed), then the call-site swap in
`sch_analysis.rs`. Deleting `sch_bridge.rs` is optional and should be a separate step. Medium
complexity. Regression risk is confined to tools that will start reporting more nets than before.

### #297 `f142942` + #298 `3d9085c` — only the first item of a violation survives

**Upstream mechanism.** Both reports attach the position and the offender to each *item* of a
violation, and a violation regularly names two. The parser kept `items[0]` and discarded the rest, so
a `pin_to_pin` conflict lost the pin that explains it and two `unconnected_items` violations sharing a
rule, a description and a first position became indistinguishable. Upstream promoted `items` to a
`Vec<ReportItem>` on both `ErcViolation` and `DrcViolation` and shared one item decoder.

**State in this fork.** Still present. `crates/konnect-core/src/tools/cli.rs:26-53` defines
`ErcViolation` and `DrcViolation` with a single `pos` and no `items`; `parse_erc_json`
(`crates/konnect-core/src/tools/cli.rs:140-166`) binds `first_item` and reads only that. The fork
already carries `rule`, as `Option<String>` rather than upstream's `String` — a divergence the
backport must preserve or reconcile deliberately.

**Additional defect found here, not in upstream's diff.** The fork's DRC parser
(`crates/konnect-core/src/tools/cli.rs:203-213`) reads `v.get("pos")` at the *violation* level. KiCad
writes no such field — the position lives on each item — so every DRC violation this fork reports has
`pos: null`. Upstream's pre-#298 code already read the first item's position; the fork either
regressed or never had it. Folding this into the same patch is free.

**Impact.** ERC and DRC output that is technically true and practically unusable: the caller is sent
back to `kicad-cli` by hand to find the second pin, and DRC findings carry no coordinates at all.

**Cost / risk.** ~140 lines across `cli.rs`, `sch_export.rs:335` and `verification.rs:222`. Low risk;
`pos` can be kept as a derived convenience field.

### #153 `c79d9a9` (partial) — `add_layer` writes a board KiCad cannot open

**Upstream mechanism.** Two defects. Layers were read with `find_all("")`, which returns nothing,
because the layer ordinal is the *head* of the list — so every field sat one index earlier than the
accessors assumed. And the insertion point was found by searching for the literal `"\n  )"`; a
tab-indented KiCad 10 file never contains it, so the fallback found the first `)` in the block — the
close of the *first layer entry* — and the new layer was written inside it. Upstream added
`konnect_sexp::layers`, a paren-balanced `close_of_block`, and a canonical-name guard, since a board
carrying a name KiCad does not define does not open at all.

**State in this fork.** Half fixed, independently. `crates/konnect-core/src/tools/pcb_board.rs:497`
defines a `board_layers` helper that reads by shape, used by `handle_get_layer_list`
(`crates/konnect-core/src/tools/pcb_board.rs:537`) and by the id allocator in `handle_add_layer`
(`:577-579`). The write side was not fixed:
`crates/konnect-core/src/tools/pcb_board.rs:582-591` still probes for `"\n  )"` with a first-`)`
fallback, and there is no canonical-name validation. Separately,
`crates/konnect-core/src/tools/pcb_board.rs:404-406` still computes `layer_count` with
`find("layers").map(|n| n.find_all("").len())`, which reports `0` on every board.

**Impact.** Corruption on `add_layer` against any tab-indented (KiCad 10-authored) board: the tool
returns success and the board no longer loads. `get_board_info` reports zero layers.

**Cost / risk.** ~50 lines: port `close_of_block` and `entry_indent`, fix the `get_board_info`
counter. The canonical-name guard needs `is_canonical_name`, which can be a short local table rather
than the whole `konnect_sexp::layers` module. Low risk.

### #142 `e0014ed` — KiCad 10 net nodes read by fixed index

**Upstream mechanism.** KiCad ≤ 9 writes `(net 1 "GND")`; KiCad 10 dropped the top-level table and
writes `(net "GND")` on every item. Code indexing the name at position 2 reads nothing on a KiCad 10
board. Upstream added `konnect_sexp::net` with shape-aware accessors, a `count_distinct_nets`, and a
refusal in `add_net` for KiCad 10 boards, where a file-level insert is meaningless.

**State in this fork.** Still present, at three sites.
`crates/konnect-core/src/tools/pcb_components.rs:1397-1400` reads a pad's net with
`.find("net").and_then(|n| n.get(2))`, defaulting to `""` — so on a KiCad 10 board *every pad reports
no net*. `crates/konnect-core/src/tools/pcb_board.rs:414` computes `net_count` as
`tree.find_all("net").len() - 1`, which on KiCad 10 counts every per-item reference and on KiCad 9
counts declarations plus references. `crates/konnect-core/src/tools/pcb_routing.rs:276` derives the
next net id from `content.matches("(net ").count()`, which collides with existing ids immediately.

**Impact.** Wrong net resolution on the board side: `get_component_pads` reports a fully netted board
as fully unconnected, which is exactly the input an agent uses to decide what to route. `add_net`
writes a colliding id. High value.

**Cost / risk.** ~120 lines. The three read sites can be fixed with a small shared accessor without
porting the whole `konnect-sexp/src/net.rs` module; the `add_net` refusal needs `board_is_kicad_10`
(~30 lines). Low risk.

### #139 `f20813d` — the BOM ignores `exclude_dnp`

**Upstream mechanism.** `export_bom` passed neither `--fields`, `--labels`, `--group-by` nor
`--exclude-dnp`, and took a `_format` argument it discarded. Upstream introduced `BomOptions` and
`bom_args`.

**State in this fork.** Still present, and worse than upstream's starting point because the tool
schema advertises the option. `crates/konnect-core/src/tools/sch_export.rs:141-146` declares
`exclude_dnp` with `"default": true`; `handle_export_bom`
(`crates/konnect-core/src/tools/sch_export.rs:157-174`) never reads it, and `cli::export_bom`
(`crates/konnect-core/src/tools/cli.rs:325-336`) takes `_format` and passes nothing but `--output`.

**Impact.** False success on a manufacturing artefact: the caller asks for a BOM without DNP parts,
is told `"success": true`, and receives a BOM containing them. The fab orders parts the design says
not to place. `format` is a dead parameter on top.

**Cost / risk.** ~80 lines in `cli.rs` and `sch_export.rs`, plus the one other caller at
`crates/konnect-core/src/tools/manufacturing.rs:187`. Low risk.

### #266 `d55cbf1` — PCB plot arguments KiCad 10 rejects

**Upstream mechanism.** `pdf` and `svg` repeated `--layers` once per layer; KiCad 10 takes one
comma-separated value and rejects the duplicate. `--mode-single` was never passed, so a single-file
plot was not requested. Upstream factored `single_file_pcb_export_args` and added
`cli_failure_diagnostics`, because kicad-cli writes the "Duplicate argument" message to stdout, which
the error path discarded.

**State in this fork.** Still present. `crates/konnect-core/src/tools/cli.rs:494-503` (`export_pdf`)
and `crates/konnect-core/src/tools/cli.rs:505-515` (`export_svg_pcb`) both push `--layers` in a loop
and never pass `--mode-single`. Callers: `crates/konnect-core/src/tools/pcb_export.rs:348` and `:377`.

**Impact.** Every layer-filtered PDF or SVG export of a board fails, and the failure message is empty
because the diagnostic went to stdout. Wrong export, high frequency.

**Cost / risk.** ~50 lines in one file. Trivial, and the `cli_failure_diagnostics` half is worth
having on its own.

### #263 `291136d` — `run_erc` on a sub-sheet reports the invocation, not the design

**Upstream mechanism.** `kicad-cli` treats whatever file it is handed as the root of the hierarchy and
looks for a `.kicad_pro` beside it. A sub-sheet has none, so the project's `sym-lib-table` is never
read and every symbol from a project library is reported as an unknown library. Upstream detects the
case — sheet reachable from the single project root in the directory — and returns a structured
`invalid_argument` on `schematic` naming the root to retry against.

**State in this fork.** Still present. `crates/konnect-core/src/tools/sch_export.rs:308-315`
(`handle_run_erc`) hands `sch_path` straight to `cli::run_erc` with no root check. The helpers the fix
needs exist but are private: `project_root_for` in `crates/konnect-core/src/tools/library.rs` and
`MAX_HIERARCHY_DEPTH` at `crates/konnect-core/src/tools/sch_hierarchy.rs:249`.

**Impact.** Wrong ERC: a caller running ERC on a child sheet gets a wall of "unknown library"
violations that describe nothing about the design, and the obvious remedy — registering the library
again — is the wrong one. An agent acting on that output makes the project worse.

**Cost / risk.** ~250 lines, all new self-contained functions plus one guard and two visibility
changes. Medium complexity, low regression risk: the only behaviour change is a refusal on a path
that previously produced garbage.

### #212 `f9daa7d` — a junction dot per wire instead of per T

**Upstream mechanism.** `find_t_junctions` reports every T on the sheet, not only the ones the new
wire created, so an unguarded add loop re-emits a dot at every existing T on every call — quadratic
across a batch. Upstream extracted `add_missing_junctions`, which checks for a coincident dot first.

**State in this fork.** Still present at three sites, one more than upstream had:
`crates/konnect-core/src/tools/sch_wiring.rs:546-548` (`handle_add_wire`),
`crates/konnect-core/src/tools/sch_wiring.rs:603-605` (`handle_batch_add_wire`) and
`crates/konnect-core/src/tools/sch_wiring.rs:2007-2009` (`handle_connect_to_net`). Each is immediately
followed by a correctly guarded mid-segment-pin loop, so the guard already exists a few lines below
in every case.

**Impact.** Duplicate junction nodes accumulate in the file on every wiring call. KiCad tolerates them
on load but the sheet no longer round-trips, diffs become unreadable, and a batch of *n* wires over a
sheet with *m* existing Ts writes *n·m* dots.

**Cost / risk.** ~30 lines: one helper, three call sites. Trivial.

### #213 `30e80ae` — `#PWR` numbered by count re-issues a live designator

**Upstream mechanism.** The next reference was `format!("#PWR{:03}", count + 1)`. Drop `#PWR028` from a
sheet of 29 and the count is 28, so the next symbol is handed `#PWR029` — still in use. Upstream
issues the lowest number no symbol on the sheet is using.

**State in this fork.** Still present verbatim at
`crates/konnect-core/src/tools/sch_wiring.rs:1581-1590`, including the `// Auto-number the #PWR
reference by counting existing power symbols` comment.

**Impact.** Two power symbols share a reference. KiCad's annotation treats them as one component,
which corrupts the netlist and the schematic-to-board sync. Triggered by the ordinary
add-delete-add sequence an agent performs constantly.

**Cost / risk.** ~20 lines, one helper and one call site. Trivial.

### #214 `816dccf` — deleting a wire leaves its junction dot behind

**Upstream mechanism.** `delete_schematic_wire` removed the wire and nothing else. The dot the wire had
justified stayed on the sheet, where KiCad reads it as a connection between whatever still crosses
there. Upstream added `prune_orphaned_junctions`, folded the wire lookup into
`locate_wire_for_delete`, and reports `junctions_pruned_count` from the batch path.

**State in this fork.** Still present. `crates/konnect-core/src/tools/sch_wiring.rs:623`
(`handle_delete_wire`) deletes the block and writes; there is no `prune_orphaned_junctions` anywhere
in the crate.

**Impact.** Wrong connectivity that survives the edit: after deleting a tap off a rail, the leftover
dot ties the crossing wires together. The user sees a clean-looking sheet with a short in it.

**Cost / risk.** ~160 lines. The fork's delete path has diverged — it resolves the UUID through
`crate::tools::find_schematic_item_by_uuid` and `find_schematic_item_block_for_delete`
(`crates/konnect-core/src/tools/sch_wiring.rs:631-635`) rather than upstream's
`wire_block_with_leading_whitespace` — so `locate_wire_for_delete` must be adapted rather than copied.
`prune_orphaned_junctions` and `wires_in_ranges` port unchanged. Medium complexity.

### #274 `cea103c` — footprint properties inferred from source text

**Upstream mechanism.** `get_footprint_info` counted pads with `content.matches("\n  (pad ")` and
probed for `"B.CrtYd"` / `"(model "` as substrings. KiCad controls indentation and line endings, so a
tab-indented or CRLF footprint counts zero pads, and a courtyard is "found" in any footprint whose
description mentions the layer. Upstream reads all three from the parsed tree.

**State in this fork.** Still present at `crates/konnect-core/src/tools/library.rs:2928-2932`,
identical to upstream's pre-fix lines. The file is already parsed nearby, so the fix has no new
dependency.

**Impact.** `pad_count: 0` for every KiCad 10 stock footprint — the number an agent uses to check a
footprint against a symbol before assigning it. Wrong result, silently plausible.

**Cost / risk.** ~12 lines. Trivial.

### #140 `349c074` — manufacturing validation counts by substring

**Upstream mechanism.** `validate_for_manufacturing` computed `net_count` from
`content.matches("\n  (net ")` and `track_count` from `matches("(segment ") + matches("(via ")`.
Indentation-dependent, blind to KiCad 10's per-item net shape, blind to `(arc …)` track segments, and
double-counting a KiCad 9 net through its declaration and its references. Upstream added
`count_nets_and_tracks` over the parsed tree.

**State in this fork.** Still present at `crates/konnect-core/src/tools/manufacturing.rs:325-326`,
identical.

**Impact.** The pre-manufacturing gate reasons over invented numbers: zero nets and zero tracks on a
KiCad 10 board, so a board that has never been routed and one that is fully routed look the same to
it. False success on the last check before fabrication.

**Cost / risk.** ~60 lines, one self-contained function in one file; the tree is already parsed at the
call site. Low risk.

---

## LATER

### #271 `2183267` — `find_orphan_items` consults no pins

`find_orphan_items` counted wire endpoints and label positions and nothing else, so a wire ending on a
component pin was reported dangling and an unconnected pin was never reported at all. Upstream
rewrote it around spatial indices (`PointIndex`, `WireIndex`), added `extract_no_connects` and
`extract_sheet_pins`, and reported the `unconnected_pin` findings the tool description had always
promised.

Still present in this fork at `crates/konnect-core/src/tools/sch_analysis.rs:596-624`, verbatim.
`placed_pins_by_reference`, `extract_no_connects` and `extract_sheet_pins` do not exist here.

High value — the tool currently produces both false positives and false negatives on any sheet with
components — but ~470 lines in `sch_analysis.rs` plus two new `konnect-sexp` extractors, and it needs
`LibPin::electrical_type`, which #262 introduces. **Next action:** land #262 first, then port
`PointIndex`/`WireIndex`, `extract_no_connects`/`extract_sheet_pins` and `placed_pins_by_reference`,
and replace the handler body wholesale.

### #179 `7bb6925` — units of a multi-unit symbol are not addressed individually

Two halves. The pin half — resolving a pin against the owning unit rather than superimposing every
unit's pins — is **already fixed here**: `extract_lib_pins_for_unit` exists at
`crates/konnect-sexp/src/schematic.rs:355`. The edit half is not. `find_all_symbol_instance_blocks`
and `field_value_ranges` do not exist; `crates/konnect-core/src/tools/mod.rs:651`
(`find_symbol_instance_block`) and `crates/konnect-core/src/tools/sch_batch.rs:321` / `:330` still
return the first match only.

Worse, fifteen call sites still use the unit-blind `extract_lib_pins`, including
`crates/konnect-core/src/tools/sch_batch.rs:404` and `:1175`,
`crates/konnect-core/src/tools/sch_export.rs:270` and `:386`,
`crates/konnect-core/src/tools/design_review.rs:166`/`264`/`762`/`804`/`918`, and
`crates/konnect-core/src/tools/sch_analysis.rs:444`/`494`/`530`/`868`/`904`. Combined with an
`instances.iter().find(|i| i.reference == reference)` first-match lookup
(`crates/konnect-core/src/tools/sch_batch.rs:390-399`), a `batch_connect_to_net` on unit 2 of an
op-amp computes unit 2's pin against unit 1's placement transform and drops the net label at the
wrong coordinate — wrong connectivity, not just a wrong report.

**Next action:** fix the pin half first, independently of upstream's diff — replace the fifteen
`extract_lib_pins` call sites with `extract_lib_pins_for_unit(sym, inst.unit)` and the first-match
instance lookup with #179's candidates loop. Port `find_all_symbol_instance_blocks` /
`field_value_ranges` afterwards as a separate change to `sch_batch.rs`.

### #185 `59d6330` — the design review approves on partial coverage

Upstream made `run_design_review` walk every reachable sheet, record per-audit
requested/completed/failed counts and coverage figures, and return an `INCOMPLETE` verdict instead of
approval when coverage is partial or an audit failed.

Still applicable. `crates/konnect-core/src/tools/design_review.rs:523-622` runs the four schematic
audits against the single `args["schematic"]` path — the sheet hierarchy is never walked — and
derives the verdict purely from finding counts, so `"LOOKS GOOD — no critical issues found"` is what a
caller gets when the audits found nothing *because they inspected one sheet of twelve*, or because a
symbol was unresolvable and the audit produced no findings at all.

False success, high value, but ~730 lines and it interacts with this fork's own evidence and gating
layers (`crates/konnect-core/src/evidence/`), which upstream does not have. **Next action:** decide
first whether the verdict belongs in `design_review` or in the fork's evidence validators; if the
former, port `AuditAggregate` and the two coverage structs and drive the sheet list through
`crate::tools::sch_hierarchy`. Pair it with upstream `977f0c5` (require DRC evidence before approving
a board).

### #148 `e15f9f1` — the net-label stub is driven into the symbol body

Upstream derives the stub direction from the pin's own orientation (`pin_outward_at`,
`stub_direction`, `resolve_stub_direction`) so a left-edge pin gets a leftward stub and a label that
reads away from the body.

Partly moot here: this fork independently implements the `justify` half —
`konnect_sexp::schematic::label_justify` and the rewrite at
`crates/konnect-core/src/tools/sch_wiring.rs:1379-1431` — so a label's text already reads the right
way for its rotation. What remains is the direction default:
`crates/konnect-core/src/tools/sch_wiring.rs:1975` takes `direction` from the caller and defaults to
`"right"`, so connecting a left-edge pin pushes the stub across the symbol body. Beyond the rendering
mess, a stub crossing the body passes over other pins, and the mid-segment-pin loop at
`crates/konnect-core/src/tools/sch_wiring.rs:2011-2022` then plants junction dots on them.

Medium value, ~1000 lines upstream. **Next action:** port `pin_outward_at` and `stub_direction` into
`crate::tools` and use them as the default when `direction` is absent, leaving an explicit `direction`
argument authoritative. That is a fraction of the upstream diff and captures the whole functional
effect.

### #186 `31a2e41` — instance fields placed at fixed offsets

Upstream reads the library symbol's own `Reference`/`Value` anchors and justifications
(`cse::library::field_anchors`) and transforms them with the placement, instead of hard-coding
±3.81 mm at rotation 0.

Still present: `crates/konnect-core/src/tools/sch_components.rs:596-609` places `Reference` at
`(x, y - 3.81)` and `Value` at `(x, y + 3.81)`, both at rotation 0, whatever the symbol and whatever
the placement rotation. `field_anchors`, `field_at` and `FALLBACK_REFERENCE_AT` do not exist here.

Category fit is marginal — the consequence is visual (fields landing over the body or over a
neighbour on a rotated part), with no electrical effect and no data loss — so this ranks below
everything else in this document. **Next action:** port `field_anchors` into
`crates/konnect-schematic-editor/src/library.rs` and the `field_at` transform helper, then rewrite
the four `positioned(...)` calls. ~580 lines; do it only after the connectivity work.

### #138 `d42420b` (residual) — drill `--output` without a trailing separator

Largely superseded here. This fork independently rebuilt the drill export with a `DrillOptions` struct
(`crates/konnect-core/src/tools/cli.rs:393-433`), `--excellon-separate-th` exposed as `separate_th`,
option validation via `one_of`, `create_dir_all` and a real directory listing of what KiCad wrote
(`crates/konnect-core/src/tools/pcb_export.rs:447-475`).

Two residuals. Upstream appends `std::path::MAIN_SEPARATOR` to the `--output` value
(`drill_output_dir_arg`) because kicad-cli otherwise treats the last component as a file name; this
fork passes the path raw at `crates/konnect-core/src/tools/cli.rs:455`, while its own doc comment
above `export_drill` claims the directory form was verified against KiCAD 10.0. The two claims
conflict and cannot be resolved without a kicad-cli run. Second, `separate_th` defaults to `false`
here (`crates/konnect-core/src/tools/cli.rs:417`) whereas upstream always separates, on the grounds
that a single `MixedPlating` file distinguishes NPTH only by a comment most Excellon readers drop —
so the fab plates holes that must stay unplated.

**Next action:** run `kicad-cli pcb export drill --output <dir>` with and without a trailing separator
against KiCAD 10 and settle the doc comment; separately, decide whether `separate_plated` should
default to `true`.

### #162 `f95acce` — `query_traces` returns no UUID

`crates/konnect-core/src/tools/pcb_routing.rs:557-566` emits net, layer, width and endpoints;
`crates/konnect-ipc/src/types.rs:96-102` (`IpcTrack`) has no `uuid` field. Meanwhile
`handle_delete_trace` (`crates/konnect-core/src/tools/pcb_routing.rs:534-546`) *requires* a `uuid`, so
there is no path from listing a trace to deleting it.

Out of the retained categories — nothing is reported wrongly, only unreachably — but the patch is
twelve lines across three files. **Next action:** bundle it into whichever PCB change lands first
rather than scheduling it on its own.

---

## NOT APPLICABLE

**#136 `355e61a` — resolve symbols through `sym-lib-table`.** Already implemented here, independently
and more thoroughly than upstream's version: global and project tables
(`crates/konnect-core/src/tools/library.rs:844-846` and `:2605-2630`), `${KIPRJMOD}` and user path
variables (`crates/konnect-core/src/tools/library.rs:893-1000`), and structured errors distinguishing
"not in any table" from "registered but its uri does not resolve"
(`crates/konnect-core/src/tools/library.rs:2653-2695`).

**#149 `aa1c541` — size a symbol body to fit its pin names.** Out of the retained categories: the
change widens the body rectangle so two facing columns of long pin names stop overlapping. No
connectivity, export, ERC or file-validity consequence. This fork has its own `symbol_body_rect`
(`crates/konnect-core/src/tools/library.rs:1611`) with per-edge margin handling, so upstream's diff
would not apply as written in any case.

---

## Appendix A — untriaged upstream fixes

This audit covered the merge list plus `e7eeeac` and `9a56233`. Upstream also landed the following
non-merge fixes directly on `main` in the same range. Several are squarely inside the retained
categories and none of them were checked against this fork. They are listed so the gap is on the
record, not because they have been assessed.

| Commit | Subject | Apparent category |
|---|---|---|
| `f2372ca` | `fix(pcb)`: write zone net references in the board's own format, not net 0 | connectivity / corruption |
| `2904841` | `fix(sync)`: stop rewriting a footprint's graphics as phantom pads | corruption |
| `e7b0c54` | `fix(sch)`: key a child sheet's instances to the root, not to itself | hierarchy / corruption |
| `f8a8db0` | `fix(schematic)`: stop reformatting the whole sheet on every write | round-trip |
| `de70351` | `fix(sch)`: annotate properties in place, and carry field text on `bulk_move` | data loss |
| `8591707` | `fix(sch)`: make `edit_schematic_component` honour fields and rename properly | false success |
| `977f0c5` | `fix(review)`: require DRC evidence before approving a board | false success |
| `d5774b3` | `fix(review)`: count the pads that are actually there | wrong result |
| `6693681` | `fix(library)`: report what `register_symbol_library` actually did | false success |
| `ec705c3` | `fix(library)`: a table beside the file wins over an unrelated project above it | symbol resolution |
| `ff518c8` | `fix(ipc)`: stop sending KiCAD a layer it crashes on | KiCad compatibility |
| `59d0ead` | `fix(sync)`: only refuse the read-back mismatch that has no benign reading | false failure |
| `791f95b`, `4536d10`, `6ed6cac`, `c6a6407` | enforce a tool's required arguments before its handler runs | false success |

`ac71ebe` (`konnect init --help` running the installer), `2404b60` and `8d7da09` (docs and skill
examples) fall outside the retained categories.
