# PROGRESS

## Phase actuelle

K — multi-harness. K.1.2 is done; K.1.1 is the only thing left in K, and it is
the last dependency of phase M (final benchmark, which also needs H.6 and H.7 —
both DONE).

## Tâche actuelle

K.1.1 — run the golden suite through Claude Code, Codex and AGY. All three CLIs
are installed on this machine (`claude`, `codex`, `agy` on PATH), so it is
technically autonomous, but every run spends the user's own LLM budget on their
accounts — that is the user's call, not the agent's. Awaiting that decision.

## Dernière tâche validée

K.1.2 — the golden suite can now fail for a reason other than a wrong result.
Tasks declare `expected_tools`, `allowed_tools`, `forbidden_tools`, `safety`
and `max_calls`; the runner audits the executed path against them and reports
`SAFETY_VIOLATIONS`, `UNNECESSARY_CALL_RATE` and `INSTABILITY_RATE`.

Validation :
- `bench/runner.py --load-mode gateway --repeat 3 --enforce` → 7 tasks / 21
  runs, 21/21, four thresholds PASS, exit 0. Same with `--load-mode tools
  --repeat 2` and `--load-mode toolsets --repeat 2`
- `cargo test -p konnect-core` green; clippy `--all-targets -D warnings` and
  `cargo fmt --check` clean; the matrix drift test passes without
  `KAM_UPDATE_MATRIX`
- negative controls, all on the executed path: a write step in the read-only
  task → `safety` + `disk_mutation`; the same step with `capabilities.is_write`
  monkeypatched to `False` → `disk_mutation` *alone* (the disk check does not
  depend on the registry); `forbidden_tools` / `allowed_tools` /
  `expected_tools` each fire, with an identical verdict in `gateway` and
  `toolsets` mode; removing an exception from `TOOL_EFFECTS` makes the Rust
  exhaustiveness test fail naming the tool

## Décisions actives

- D56 — `safety: read_only` is checked twice and the second check does not
  trust the first. The declarative half reads the `effect` column of
  `docs/capability-matrix.md` (generated from `capability::tool_effect`); the
  other half fingerprints every byte of `$WORK` before the first step and after
  the last. If the Rust classification lies, the read-only task fails anyway.
  Verified with `is_write` forced to `False`, not asserted.
- D57 — the bench audits the *executed* path, never `task["steps"]`. Judging
  the YAML against the YAML would make `forbidden_tools` and `missing_expected`
  unfalsifiable. In `gateway` mode the names come from `kicad_invoke`'s
  per-entry `tool` field — the server's account of what it ran.
- D58 — `capability::tool_effect` classifies by verb plus six named exceptions,
  each decided by reading the handler, and falls back to `Write` for anything
  unknown. Calling a reader a writer costs a visible refusal; calling a writer a
  reader lets a mutation through a context that believed itself safe. A test
  forbids the fallback from ever being the answer for a MANIFEST tool.
- D53 — a foreign writer landing mid-batch is caught by the per-write
  compare-and-swap, never by `base_revisions`. `base_revisions` guards the
  batch's *start*; `write_atomic_if_unchanged` guards each write. Rollback
  (D12) is deliberately not extended to reach another application's edits.
- D55 — the idempotency ledger (`kam_state::IdempotencyLedger`) is per-process
  and in memory: it protects a client retrying a call it just made, seconds not
  restarts. Cross-process safety is content-keyed instead (D53).
  `OperationInFlight` is therefore only reachable over HTTP.
- D54 — a held-handle rename failure is separated from an ACL denial by
  re-opening the target for `DELETE` and looking for `ERROR_SHARING_VIOLATION`.
  Only that case is relabelled `ResourceBusy`; `permission_denied` as a whole
  keeps `TransientClass::None`, because telling a recovery loop to wait out an
  ACL is a hang.
- D51 — the symbol index fingerprint includes each library entry's own mtime,
  in milliseconds, read through `std::fs::metadata` (on Windows the
  enumeration's timestamps come from the parent's lazily-updated index entry).
  Measured 3.7 ms for 223 libraries.
- D52 — the test locks guarding process-global env vars are
  `tokio::sync::Mutex`, taken with `.lock().await` (E10).
- D49 — a KiCad profile this project *creates* is configured for a machine that
  is nobody's: software rendering, the three library tables written, and the
  update-check / data-collection prompts answered. A profile that already
  exists is a real user's and is never touched.
- D50 — the API pipe is matched by shape (`*\kicad\api.sock`), never by
  equality with a computed path.
- D48 — `download_jlcpcb_database` defaults to `basic-preferred` (~2 MB).
- D47 — driving the PCB path needs a *desktop session* but no human (DEV.md).
- D26 — the live suites keep their `#[ignore]` and stay `gated` in the matrix;
  the `live-ipc` CI job is where they run.
- D44 — `CAPABILITY_COVERAGE`'s target is frozen at the 187 tools the baseline
  registers at `5cd6454`, minus what KiCAD gives no API for → denominator 186.
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

GitHub CI is still red on `agentic/main` for an external reason, not the code:
`codeload.github.com` returns 429 to the runners, so `dtolnay/rust-toolchain`
and `arduino/setup-protoc` fail to download and every job needing an action
download dies at the setup step, before any cargo command. Confirmed again on
run `32042930726`. `gate.ps1` covers the same ground locally and passes. Next
attempt: `gh run rerun <id> --failed -R nevenfo/kicad-agentic-mcp` once the
rate limit clears — do not restructure the workflows in response to a 429.

## Fichiers / zones utiles

- `bench/runner.py` — `audit()` (typed violations), `executed_tools()` (why the
  audit does not read the task file), `fingerprint()` (the check that does not
  trust the registry), `--enforce`. Run it with `py -3.11`: it is the only
  interpreter here with `yaml` and `tiktoken`
- `bench/capabilities.py` — reads the `effect` column out of
  `docs/capability-matrix.md`; unknown tool ⇒ `write`
- `bench/fixtures/divider.kicad_sch` — a real server output with `lib_symbols`
  embedded, so a read-only task has a subject it did not have to author
- `crates/konnect-core/src/capability/mod.rs` — `MANIFEST`, `Effect`,
  `VERB_EFFECTS` / `TOOL_EFFECTS` / `tool_effect`, and the tests that keep the
  fail-safe from being load-bearing. Regenerate the matrix with
  `KAM_UPDATE_MATRIX=1 cargo test -p konnect-core --test capability_matrix`
- `crates/konnect-sexp/src/writer.rs` — the whole write model: `apply_edits`
  and the block finders whose byte offsets feed it
- `crates/konnect-core/src/mcp/error.rs` — `TransientClass`, `retry_after_ms()`,
  `ToolErrorKind::transient_class()`; `from_anyhow` downcasts specific error
  types *before* its `io::Error` fallback
- `crates/konnect-core/tests/concurrent_gui_edit.rs` and `tests/lock_recovery.rs`
  — how to race a foreign writer, and two `kicad_invoke` calls, deterministically
- `.github/workflows/ci.yml` — triggers on `agentic/main` as well as `main`;
  dispatch the KiCad E2E one as `gh workflow run e2e-kicad.yml -R
  nevenfo/kicad-agentic-mcp --ref agentic/main` (a bare `gh` resolves to
  `upstream` and 403s)
- `gate.ps1` — the local mirror of CI; every step of it is real
- `crates/konnect-core/tests/harness/mod.rs` — shared rig: calls a tool through
  `ToolRouter` by name; `BLANK_BOARD`, `TWO_RESISTORS`, `pins::*`
- `scripts/live-pcb-e2e.ps1` — the live PCB harness. Run it with no arguments

## NEXT ACTION

K.1.1 — decide with the user whether to spend their LLM budget driving the
golden suite through `claude`, `codex` and `agy` headless (all three are on
PATH). If yes, build the harness runner under `bench/` and score each harness
on the same tasks and the same thresholds as `--enforce`. If no, K.1.1 stays
open and phase M starts without it, recording the gap under INV6.
