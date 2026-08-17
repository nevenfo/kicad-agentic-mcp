# PROGRESS

## Phase actuelle

J.2 — raise capability coverage. J.2.1 and J.2.2 are closed; J.2.3 is in
progress, two of its eight lots done.

## Tâche actuelle

J.2.3.3 — prove `config` (7 `internal`) and `rules` (5 `sexpr`).

## Dernière tâche validée

J.2.3.2 — prove the `symbols` and `schematic` tools.

Validation :
- `cargo test --workspace`: 904 PASS, 18 ignored
- live probes against KiCad 10.0.3: `bus_live` 2 PASS, `drill_export_live`
  4 PASS, the rename probe 1 PASS
- `cargo clippy -p konnect-core --lib -- -D warnings`: PASS
- matrix regenerated: KiCad-domain coverage 28.6 % → 47.0 %, V1 comparison
  22.6 % (baseline) against 43.0 % (80 of the frozen 186), 0 regressions

## Décisions actives

- D44 — `CAPABILITY_COVERAGE`'s comparison target is frozen: the 187 tools the
  baseline registers at `5cd6454` (this fork registers all 187, so no name
  mapping), minus what KiCAD gives no API for → denominator 186. Both numerators
  come from the same scanner pointed at each tree. Met only when strictly ahead
  *and* no tool the baseline proved is unproved here. The headline percentage is
  the whole-surface number and is not the criterion.
- D45 — an `ipc` tool is never "proved" by its own "KiCAD must be running"
  error. Those 19 tools stay `NOT_TESTED` until J.3 settles the GUI-session
  question.
- D43 — Direct/Agent is an explicit gateway entry-point choice. Direct never
  starts an LLM; `ESCALATE` returns structured failure to the caller.
- D42 — Agent retrieval must combine task-specific electrical and Plan IR
  constraints with geometry; generic pin offsets do not carry E27's effect.
- D40 — router tiers are `NO_LLM | LOCAL | ESCALATE`.
- D39 — uniquely resolvable installed-library names are canonicalized.
- D38 — local model is `gpt-oss-20b`, `medium`, ctx 32 768.
- D35/D33 — one repair round buys no success; `strict_json` stays off.
- E.6.1/E.6.2/E.6.3 — measured 5 120-token reserve; callers supply measured
  token costs; the durable task core is non-evictable; bundles are atomic.
- H.7.1/H.7.2/H.7.3 — `kicad_agent` is separate from Direct; verification
  returns `PASS | FAIL | COULD_NOT_RUN`; the local completion is constrained by
  the measured Plan IR schema at temperature 0.2.
- J.1 — `find_single_pin_nets` is pin-aware and stays advisory/`PARTIAL`.

## Blocage actif

`git push` cannot authenticate in this environment ("Unable to persist
credentials with the 'wincredman' credential store", no tty for a prompt). Five
commits from c17138f onward are local only on `agentic/main`. Everything else
proceeds; the user pushes when convenient.

Phase I remains gated: this machine has KiCad 10.0, not the KiCad 11 /
`kicad-cli api-server` it needs.

## Fichiers / zones utiles

- `crates/konnect-core/tests/harness/mod.rs` — shared rig: calls a tool through
  `ToolRouter` by name, no `kicad-cli`, no running KiCAD
- `crates/konnect-core/tests/fixtures/bus_two_resistors.kicad_sch` — two
  `Device:R` with the symbol embedded; pin coordinates are in `harness::pins`
- `crates/konnect-core/tests/nets_and_wires.rs`, `symbols_and_schematic.rs` —
  the J.2.3 lots done so far
- `crates/konnect-core/src/capability/baseline.rs` — frozen V1 target
- `crates/konnect-core/src/capability/mod.rs` — `MANIFEST`, `MISSING`
- `crates/konnect-core/tests/capability_matrix.rs` — matrix equality and the
  baseline re-derivation; regenerate with `KAM_UPDATE_MATRIX=1`
- `crates/konnect-core/src/tools/sch_buses.rs` — J.2.2.2 toolset
- Pre-existing H.6.1–H.6.5 changes are still uncommitted in `bench/`,
  `sch_components.rs`, `sch_wiring.rs`, `library.rs`, `docs/benchmark.md`,
  `docs/local-agents.md`. `sch_components.rs` now also holds committed J.2.3.2
  hunks: stage from that file by filtered patch, never whole-file
- `cargo clippy --tests` fails inside those uncommitted changes
  (`await_holding_lock`), which is why the project's clippy gate is `--lib`

## NEXT ACTION

Execute J.2.3.3 — prove the `config` and `rules` tools with a new
`crates/konnect-core/tests/` file using the shared harness, then regenerate the
matrix and confirm the V1 comparison still reports 0 regressions.
