# PROGRESS

## Phase actuelle

L — hardening. L.1 (known debt) is closed and proven by a green CI run.
L.2 (failure injection and concurrency) is in progress.

## Tâche actuelle

L.2.1 — fuzz the S-expression parser/writer round trip. Delegated to a worker.
The gap is not the parser (already covered by
`crates/konnect-sexp/tests/proptest_parser.rs`) but the *write* round trip:
this crate has no tree serialiser, so every write is "locate a block by byte
offset, replace that text, rewrite the file". Nothing fuzzes that path, and the
existing file explicitly confines itself to ASCII — while `apply_edits` uses
`String::replace_range`, which panics on a non-char-boundary offset, and real
KiCAD files carry UTF-8.

## Dernière tâche validée

L.1.6 — the frozen-baseline test re-derives its measurement from the tree at
`BASELINE_COMMIT`, which a depth-1 checkout does not contain; the `check` job
now fetches full history.

Validation :
- CI run 32031309526 on `agentic/main`: **all 7 jobs green** — `Format`,
  `Clippy`, `Check & Test` on ubuntu/macos/windows, `Schematic viewer`,
  `PCM packaging`. This is the first fully green run this fork has ever had.
- L.1.4 was still `[ ]` in `plan.md` despite being committed at `439015a`; that
  same run is its proof, so it is now checked.

## Décisions actives

- D51 — the symbol index fingerprint includes each library entry's own mtime,
  in milliseconds, read through `std::fs::metadata` rather than the `DirEntry`
  (on Windows the enumeration's timestamps come from the parent's index entry,
  which NTFS updates lazily). L.1.3's old "a stale index can only cost a
  suggestion" reasoning died with H.6.1: `canonical_lib_id` reads that index
  and rewrites a `lib_id` on a unique owner, so a stale index turns a
  documented refusal into a silent pick. Measured 3.7 ms for 223 libraries.
- D52 — the test locks that guard process-global env vars are
  `tokio::sync::Mutex`, taken with `.lock().await`. A `std::sync` guard held
  across an `.await` is not `Send`, which is what E10 was; five plain `#[test]`s
  that share those locks became `#[tokio::test]` so each lock has one door.
- D49 — a KiCad profile this project *creates* is configured for a machine that
  is nobody's: software rendering (`graphics.canvas_type` 2), the three library
  tables written, and `do_not_show_again.update_check_prompt` /
  `.data_collection_prompt` answered. Each of those is a modal dialog KiCad would
  otherwise serve *before* its API, which is indistinguishable from a hung KiCad.
  A profile that already exists is a real user's and is never touched.
- D50 — the API pipe is matched by shape (`*\kicad\api.sock`), never by equality
  with a computed path, and the name that exists is what gets exported. KiCad may
  spell it in 8.3 (`RUNNER~1` on a runner); a pipe name is a literal in a
  namespace with no path resolution, so the difference is a failed connection.
- D48 — `download_jlcpcb_database` defaults to the `basic-preferred` library
  (~2 MB) rather than upstream's own default `current-parts` (~780 MB inflated).
- D47 — driving the PCB path needs a *desktop session* but no human. Full
  detail: DEV.md, "Driving the PCB path unattended".
- D26 stands: the live suites keep their `#[ignore]` and stay `gated` in the
  matrix. The `live-ipc` CI job is where they actually run.
- D44 — `CAPABILITY_COVERAGE`'s comparison target is frozen: the 187 tools the
  baseline registers at `5cd6454`, minus what KiCAD gives no API for →
  denominator 186.
- D45 — an `ipc` tool is never "proved" by its own "KiCAD must be running"
  error, and a `cli` tool is never proved by failing to spawn.
- D46 — a third party is not a test dependency.
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
  returns `PASS | FAIL | COULD_NOT_RUN`.
- J.1 — `find_single_pin_nets` is pin-aware and stays advisory/`PARTIAL`.

## Blocage actif

Phase I remains gated: this machine has KiCad 10.0, not the KiCad 11 /
`kicad-cli api-server` it needs.

## Fichiers / zones utiles

- `crates/konnect-sexp/src/writer.rs` — the whole write model: `apply_edits`,
  and the block finders (`find_balanced_block`, `find_block_starts`,
  `find_direct_child_blocks`, `find_enclosing_block`) whose byte offsets feed it
- `crates/konnect-sexp/tests/proptest_parser.rs` — the existing parser
  properties; L.2.1 adds a writer-side sibling rather than editing this one
- `.github/workflows/ci.yml` — triggers on `agentic/main` as well as `main`;
  clippy is `--all-targets` and the `check` job fetches full history. Dispatch
  the KiCad E2E one as `gh workflow run e2e-kicad.yml -R
  nevenfo/kicad-agentic-mcp --ref agentic/main`; a bare `gh` resolves to
  `upstream` and 403s
- `gate.ps1` — the local mirror of CI; every step of it is real now
- `crates/konnect-core/src/plan/ops.rs` — the operation library; its `tests`
  module parses the `*_SIGNATURE` DSL, and `minimal_examples()` must stay in
  `OP_LIBRARY` order
- `crates/konnect-schematic-editor/src/library.rs` — `canonical_lib_id` (H.6.1)
  and the on-disk symbol index (`probe_dir`, `DirFingerprint`, cache magic V2)
- `crates/konnect/tests/protocol_stdio.rs` — `stub_symbol_library()` and
  `spawn_with_symbols()`: how a stdio test gets a resolvable `lib_id` with no
  KiCAD installed
- `scripts/live-pcb-e2e.ps1` — the live PCB harness. Run it with no arguments
- `crates/konnect-core/tests/harness/mod.rs` — shared rig: calls a tool through
  `ToolRouter` by name; `BLANK_BOARD`, `TWO_RESISTORS`, `pins::*`
- `crates/konnect-core/src/capability/` — regenerate the matrix with
  `KAM_UPDATE_MATRIX=1 cargo test -p konnect-core --test capability_matrix`

## NEXT ACTION

Review the L.2.1 worker's result, run `cargo test -p konnect-sexp` and
`cargo clippy -p konnect-sexp --locked --all-targets -- -D warnings` as the
final check, then continue with L.2.2 (inject failures per `TransientClass`
and assert the recovery policy).
