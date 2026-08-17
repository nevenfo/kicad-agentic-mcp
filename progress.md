# PROGRESS

## Phase actuelle

J — scope expansion. J.1, J.2 and J.4 are closed; J.2.4 has one item left that
needs an external lookup; J.3 is the only untouched lot.

## Tâche actuelle

J.3.1 — determine by experiment whether KiCad 10.0.3 on Windows exposes
`KICAD_API_SOCKET` reliably enough for unattended PCB E2E, or whether the PCB
path needs a live GUI session. **Needs a decision first: the experiment launches
the KiCad GUI on this machine.**

## Dernière tâche validée

J.2.4 — fix the two defects the coverage work surfaced (J.2.4.1, J.2.4.2) and
record the third (J.2.4.3).

Validation :
- `cargo test --workspace`: 949 PASS, 26 ignored
- ignored suite with `KICAD_CLI` set: 15 PASS against KiCad 10.0.3 and the
  network
- `cargo clippy -p konnect-core --lib -- -D warnings`: PASS
- matrix regenerated; V1 comparison 22.6 % (baseline) against 72.6 % (135 of
  the frozen 186), 0 regressions

## Décisions actives

- D44 — `CAPABILITY_COVERAGE`'s comparison target is frozen: the 187 tools the
  baseline registers at `5cd6454` (this fork registers all 187, so no name
  mapping), minus what KiCAD gives no API for → denominator 186. Both numerators
  come from the same scanner pointed at each tree. Met only when strictly ahead
  *and* no tool the baseline proved is unproved here.
- D45 — an `ipc` tool is never "proved" by its own "KiCAD must be running"
  error, and a `cli` tool is never proved by failing to spawn. What a running
  test may claim is what the server decides before it calls out; the rest is an
  `#[ignore]`d live probe that reads `gated` and claims nothing.
- D46 — a third party is not a test dependency. The JLCPCB/LCSC/Freerouting
  tools are tested for what they do when it is absent, which is the state every
  fresh install is in.
- D43 — Direct/Agent is an explicit gateway entry-point choice; `ESCALATE`
  returns structured failure to the caller.
- D42 — Agent retrieval must combine task-specific electrical and Plan IR
  constraints with geometry.
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

1. **`git push` cannot authenticate here** — "Unable to persist credentials with
   the 'wincredman' credential store", and no tty for a prompt. Eleven commits
   from `c17138f` to `dd2be6b` are local only on `agentic/main`. Run `git push`
   from an interactive shell.
2. **J.2.4.3 needs an external lookup** this session could not make: the JLCPCB
   parts database moved and the old URL 404s. The tool is declared a `GAP`
   meanwhile.
3. Phase I remains gated: this machine has KiCad 10.0, not the KiCad 11 /
   `kicad-cli api-server` it needs.

## Fichiers / zones utiles

- `crates/konnect-core/tests/harness/mod.rs` — shared rig: calls a tool through
  `ToolRouter` by name; `BLANK_BOARD`, `TWO_RESISTORS`, `pins::*`
- `crates/konnect-core/tests/` — the J.2.3 lots: `nets_and_wires.rs`,
  `symbols_and_schematic.rs`, `config_and_rules.rs`, `design_review.rs`,
  `libraries_and_footprints.rs`, `board_and_labels.rs`, `cli_tools.rs`,
  `sourcing_and_manufacturing.rs`; live probes in `bus_live.rs`,
  `drill_export_live.rs`
- `crates/konnect-core/src/capability/baseline.rs` — frozen V1 target
- `crates/konnect-core/src/capability/mod.rs` — `MANIFEST`, `MISSING`
- `crates/konnect-core/tests/capability_matrix.rs` — regenerate with
  `KAM_UPDATE_MATRIX=1 cargo test -p konnect-core --test capability_matrix`
- `crates/konnect-core/src/tools/sch_buses.rs` — J.2.2.2 toolset
- Run every live probe with
  `KICAD_CLI=<path> cargo test -p konnect-core -- --ignored`; the path is in
  `bench/konnect.bench.toml`
- Pre-existing H.6.1–H.6.5 changes are still uncommitted in `bench/`,
  `sch_components.rs`, `sch_wiring.rs`, `library.rs`, `docs/benchmark.md`,
  `docs/local-agents.md`. `sch_components.rs` also holds committed J.2.3/J.2.4
  hunks: stage from that file by filtered patch, never whole-file
- `cargo clippy --tests` fails inside those uncommitted changes
  (`await_holding_lock`), which is why the project's clippy gate is `--lib`

## NEXT ACTION

Ask whether J.3.1 may launch the KiCad GUI on this machine; if yes, run the
experiment and record either an unattended PCB E2E in the gate or a written
platform constraint with evidence (J.3.2). Twenty-six tools — the `ipc` and
`process` ones — stay `NOT_TESTED` until that question is answered.
