# PROGRESS

## Phase actuelle

L — hardening — **closed**. L.1 (known debt) and L.2 (failure injection and
concurrency, L.2.1 through L.2.5) are all done. Next is M — final benchmark.

## Tâche actuelle

K.1.2 — the eval design in `bench/`. M.1 (phase M) depends on H.6, H.7 and
K.1; H.6/H.7 are DONE but K.1 is not, so K.1 comes first. K.1.1 (running the
golden suite through Claude Code / Codex / AGY) needs external harnesses and is
not autonomous; K.1.2 is: add `expected_tools`, `allowed_tools`,
`forbidden_tools`, a `safety` tier checked against the capability registry (a
`read_only` case must reject *any* write tool), `max_calls`, and an
instability rate across repeated runs.

## Dernière tâche validée

L.2.5 — a held handle and a denied ACL no longer look alike.
`refine_rename_failure` (konnect-sexp/src/writer.rs) re-opens the rename's
target asking only for `DELETE`: a sharing violation means a handle is held →
`ResourceBusy` → `Io { code: "resource_busy" }` → `TransientClass::Lock`;
a second access denial means an ACL → left as `permission_denied` → `None`.
This closes L.2, and with it phase L.

Validation :
- `cargo test -p konnect-sexp` 95 passed, `-p konnect-core` 412, `-p konnect`
  all green (33 in `protocol_stdio`)
- `cargo clippy --workspace --locked --all-targets -- -D warnings` clean;
  `cargo fmt --all -- --check` clean
- negative controls, both directions: forcing the probe to report a sharing
  violation makes the ACL test fail (`left: ResourceBusy`); forcing it to
  report access-denied makes the held-handle test fail (`left:
  PermissionDenied`). The probe does the discrimination, not the test setup
- earlier in phase L: L.2.4 (ledger boundary, control = `check_base_revisions`
  neutered → the batch applies instead of being refused) and L.2.3
  (compare-and-swap, control = both checks short-circuited → the file is
  *corrupted*, not merely misreported)
- phase-close check: `gate.ps1` → `GATE PASSED`, exit 0 (fmt, clippy, test,
  doctest, release build; the optional benchmark steps stay behind their flag)

## Décisions actives

- D53 — a foreign writer landing mid-batch is caught by the per-write
  compare-and-swap, never by `base_revisions`. `base_revisions` guards the
  batch's *start*; `write_atomic_if_unchanged` guards each write. Rollback
  (D12) is deliberately not extended to reach another application's edits —
  its only promise is that Konnect's own half-applied batch does not survive,
  and the coherence guarantee comes from the refusal, not the undo.
- D55 — the idempotency ledger (`kam_state::IdempotencyLedger`, in the
  `ToolContext`) is per-process and stays in memory. It protects a client
  retrying a call it just made — seconds, not restarts. Cross-process and
  cross-application safety is content-keyed instead, never caller-keyed:
  `base_revisions` and the per-write compare-and-swap (D53).
  `OperationInFlight` is therefore only reachable over HTTP.
- D54 — a held-handle rename failure *is* separated from an ACL denial, by
  re-opening the target for `DELETE` and looking for `ERROR_SHARING_VIOLATION`
  (no ACL denial produces it). Only that case is relabelled `ResourceBusy`;
  `permission_denied` as a whole keeps `TransientClass::None`, because telling
  a recovery loop to wait out an ACL is a hang. A probe that succeeds leaves
  the original error untouched — an unnamed refusal stays honest.
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

GitHub CI is red on `agentic/main` for an external reason, not the code:
`codeload.github.com` is returning 429 to the runners, so `Swatinem/rust-cache`
and `arduino/setup-protoc` fail to download (the latter surfaces as "unable to
get latest version"). Every job that needs an action download fails at the
setup step, before any cargo command; `Format`, `Schematic viewer` and `PCM
packaging validation` pass. `gate.ps1` covers the same ground locally and
passes. Next attempt: re-run the failed jobs (`gh run rerun <id> --failed -R
nevenfo/kicad-agentic-mcp`) once the rate limit clears — do not restructure the
workflows in response to a 429.

## Fichiers / zones utiles

- `crates/konnect-sexp/src/writer.rs` — the whole write model: `apply_edits`,
  and the block finders (`find_balanced_block`, `find_block_starts`,
  `find_direct_child_blocks`, `find_enclosing_block`) whose byte offsets feed it
- `crates/konnect-sexp/tests/proptest_parser.rs` (parser properties) and
  `tests/proptest_writer.rs` (L.2.1, writer properties). The writer generator
  carries its own ground truth — never assert one finder against another
- `crates/konnect-core/src/mcp/error.rs` — `TransientClass`,
  `retry_after_ms()` (Lock 250 ms, Network/Timeout 1 s, None/State none) and
  `ToolErrorKind::transient_class()`. `from_anyhow` downcasts specific error
  types *before* its `io::Error` fallback — anything without an `io::Error` in
  its chain decays to `HandlerError` unless matched there first
- `crates/konnect-core/tests/concurrent_gui_edit.rs` — how to race a foreign
  writer against a batch deterministically: a thread doing plain
  `std::fs::write` in a tight loop for the batch's whole duration, so no
  assertion depends on which call the conflict lands on
- `crates/konnect-core/tests/lock_recovery.rs` — how to race two
  `kicad_invoke` calls deterministically: two tokio tasks sharing one
  `ToolContext`, the loser fired only after the winner's writes are visible on
  disk. It cannot be done over stdio (see L.2.4)
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

Implement K.1.2 — extend the task schema in `bench/tasks/*.yaml` and the runner
(`bench/runner.py`, scored in `bench/analyze.py`) with `expected_tools`,
`allowed_tools`, `forbidden_tools`, `safety`, `max_calls` and a repeated-run
instability rate, checking the `safety` tier against
`crates/konnect-core/src/capability/`. Then run the suite and
`cargo clippy --workspace --locked --all-targets -- -D warnings`.
