# PROGRESS

## Phase actuelle

L — hardening. L.1 (known debt) is closed and proven by a green CI run.
L.2 (failure injection and concurrency) is in progress.

## Tâche actuelle

L.2.2 — inject failures per `TransientClass` and assert the recovery policy.
What exists today is the *classification*: `ToolErrorKind::transient_class()`
in `crates/konnect-core/src/mcp/error.rs` maps each error to
`None | Network | Timeout | Lock | State`, and its unit tests check that map.
What does not exist is the injection: nothing provokes a real held lock, a real
stale revision or a real timeout and then asserts the policy the class
advertises actually holds end to end — `Lock` retries as is and succeeds,
`State` stays failing until the caller re-reads, and no partial batch survives.

## Dernière tâche validée

L.2.1 — the writer-side round trip is fuzzed.

Validation :
- `cargo test -p konnect-sexp`: 93 + 8 + 7 unit/property tests + doctests, all
  pass; `cargo clippy -p konnect-sexp --locked --all-targets -- -D warnings`
  clean; `cargo fmt --all -- --check` clean
- negative control: removing string-awareness from `find_block_starts` fails
  the health property, shrunk to `(kicad_sch (symbol "(label 😀)"))`
- no production bug found, and `writer.rs` is unchanged. See L.2.1 in `plan.md`
  for why the UTF-8 boundary hazard this task targeted cannot fire

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
- `crates/konnect-sexp/tests/proptest_parser.rs` (parser properties) and
  `tests/proptest_writer.rs` (L.2.1, writer properties). The writer generator
  carries its own ground truth — never assert one finder against another
- `crates/konnect-core/src/mcp/error.rs` — `TransientClass`,
  `retry_after_ms()` (Lock 250 ms, Network/Timeout 1 s, None/State none) and
  `ToolErrorKind::transient_class()`. L.2.2's subject
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

Implement L.2.2 — provoke a real failure of each `TransientClass` and assert
the recovery policy end to end, then run `cargo test -p konnect-core` and
`cargo clippy -p konnect-core --locked --all-targets -- -D warnings`.
