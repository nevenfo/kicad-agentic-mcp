# PROGRESS

## Phase actuelle

K — multi-harness. K.1.1 is the only thing left in K, and it is the last
dependency of phase M (final benchmark, which also needs H.6 and H.7 — both
DONE).

## Tâche actuelle

K.1.1 — run the golden suite through Claude Code, Codex and AGY. The runner
(K.1.3) is written and proven; what is missing is the measurement, blocked
externally until 2026-08-20, not by any code.

## Dernière tâche validée

K.1.5 — the twelve gateway meta-tools now carry a declared effect, so the
`read_only` bench tier no longer rejects `find_capabilities` and `load_tools`.
Also landed this session: K.1.3 (`bench/harness_runner.py`, the agentic runner)
and L.2.6 (the GUI stand-in in `concurrent_gui_edit.rs` could tear the file it
raced — found by CI, fixed with an atomic rename).

Validation :
- `gate.ps1` green; matrix drift test passes without `KAM_UPDATE_MATRIX`; the
  meta-tool exhaustiveness test exercised red by removing `server_stats`
- CI green on `agentic/main` (run 32060085312) — ubuntu, macos, windows
- one real scored Claude Code run through `bench/harness_runner.py`: 9 calls,
  0 off-server, $0.0615 with haiku on `sch_inspection`
- `bench/capabilities.py` reads `read` for the discovery tools; an invented
  name still classifies `write`

## Décisions actives

- D62 — a harness prompt is passed on **stdin**, never in argv. `claude` and
  `codex` are `.CMD` shims on Windows, so `cmd.exe` re-parses the command line
  and cuts the argument at the first newline — measured: the agent got one line
  of the prompt and every flag after it was lost, including `--mcp-config`.
  `agy.exe` is a native binary and is safe in argv, which is why it differs.
- D61 — `agy` 1.1.13 ignores workspace MCP config (`.mcp.json` *and* the
  documented `.agents/mcp_config.json`; antigravity-cli#60). Its only working
  wiring is the user's global `~/.gemini/config/mcp_config.json`, so
  `AgyMcpConfigGuard` writes the entry for the run and restores the original
  bytes on every exit path, refusing to start if `konnect` is already declared
  or a backup from a previous run is present. The user authorised this.
- D60 — a meta-tool's `effect` answers D56's question only: can this call mutate
  the *project on disk*. Session state (which tools `tools/list` exposes) is not
  a disk mutation, so `load_tools` / `load_toolset` / `unload_toolset` are
  `read`. Exhaustiveness is structural: `define_meta_tools!` generates the
  dispatch `match` and `META_TOOL_NAMES` together.
- D59 — each harness declares an isolation level. `tools-off` (Claude Code,
  `--tools ""`) makes any off-server call contamination; `read-only-sandbox`
  (codex, agy) cannot remove built-ins. Hence two rates: `SUCCESS_RATE` (strict,
  comparable only at equal isolation) and `DESIGN_PASS_RATE` (ignores
  contamination, comparable across harnesses), always printed with the level.
- D56 — `safety: read_only` is checked twice and the second check does not trust
  the first: the `effect` column of `docs/capability-matrix.md`, and a byte
  fingerprint of `$WORK` before the first step and after the last.
- D57 — the bench audits the *executed* path, never `task["steps"]`. In gateway
  mode the names come from `kicad_invoke`'s per-entry `tool` field.
- D58 — `capability::tool_effect` classifies by verb plus six named exceptions,
  each decided by reading the handler, with a `Write` fail-safe a test forbids
  from ever being the answer for a MANIFEST tool.
- D53 — a foreign writer landing mid-batch is caught by the per-write
  compare-and-swap, never by `base_revisions`. Rollback (D12) is deliberately
  not extended to reach another application's edits.
- D55 — the idempotency ledger is per-process and in memory; cross-process
  safety is content-keyed instead (D53). `OperationInFlight` is HTTP-only.
- D54 — a held-handle rename failure is separated from an ACL denial by
  re-opening the target for `DELETE` and looking for `ERROR_SHARING_VIOLATION`.
- D51 — the symbol index fingerprint includes each library entry's own mtime, in
  milliseconds, via `std::fs::metadata`. Measured 3.7 ms for 223 libraries.
- D52 — the test locks guarding process-global env vars are `tokio::sync::Mutex`
  (E10).
- D49 — a KiCad profile this project *creates* is configured for a machine that
  is nobody's; a profile that already exists is a real user's and is never
  touched.
- D50 — the API pipe is matched by shape (`*\kicad\api.sock`), never by equality.
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
- E.6.1/E.6.2/E.6.3 — measured 5 120-token reserve; callers supply measured token
  costs; the durable task core is non-evictable; bundles are atomic.
- H.7.1/H.7.2/H.7.3 — `kicad_agent` is separate from Direct; verification returns
  `PASS | FAIL | COULD_NOT_RUN`.
- J.1 — `find_single_pin_nets` is pin-aware and stays advisory/`PARTIAL`.

## Blocage actif

K.1.1 cannot be measured before **2026-08-20**, for two external reasons, both
recorded under K.1.4: the Codex account is at its usage limit until that date,
and `agy`'s MCP wiring depends on `AgyMcpConfigGuard`, which has never been
exercised against a real agy run (D61). The Claude Code path is unblocked and
could be measured alone at any time — the user chose to wait and run the three
harnesses as one campaign instead.

Phase I remains gated: this machine has KiCad 10.0, not the KiCad 11 /
`kicad-cli api-server` it needs.

## Fichiers / zones utiles

- `bench/harness_runner.py` — the agentic runner. `HARNESSES` (argv builder +
  isolation + parser per harness), `AgyMcpConfigGuard`, `parse_agy_stream`
  (agy's schema is `event`/`step_update`, nothing like Claude's, and each tool
  call appears twice — ACTIVE then DONE — so it dedupes on `step_index`).
  `--dry-run` spends nothing and touches no config. Run it with `py -3.11`
- `bench/agent_prompts.yaml` — one plain-language prompt per golden task; no
  tool names, or the run would measure instruction-following
- `bench/runner.py` — `audit()`, `fingerprint()`, `THRESHOLDS`; the harness
  runner imports all of it rather than reimplementing, which is the only reason
  the two sets of numbers compare
- `crates/konnect-core/src/capability/mod.rs` — `MANIFEST`, `Effect`,
  `VERB_EFFECTS` / `TOOL_EFFECTS` / `META_TOOL_EFFECTS`. Regenerate the matrix
  with `KAM_UPDATE_MATRIX=1 cargo test -p konnect-core --test capability_matrix`
- `crates/konnect-core/src/router/meta_tools.rs` — `define_meta_tools!` is the
  single source for both the dispatch `match` and `META_TOOL_NAMES`
- `crates/konnect-sexp/src/writer.rs` — the whole write model: `apply_edits` and
  the block finders whose byte offsets feed it
- `crates/konnect-core/src/mcp/error.rs` — `TransientClass`, `retry_after_ms()`,
  `ToolErrorKind::transient_class()`
- `crates/konnect-core/tests/concurrent_gui_edit.rs`, `tests/lock_recovery.rs` —
  how to race a foreign writer deterministically
- `.github/workflows/ci.yml` — triggers on `agentic/main` as well as `main`;
  dispatch the KiCad E2E one as `gh workflow run e2e-kicad.yml -R
  nevenfo/kicad-agentic-mcp --ref agentic/main` (a bare `gh` resolves to
  `upstream` and 403s)
- `gate.ps1` — the local mirror of CI, but not a substitute for it: L.2.6 was
  green here and red on all three CI runners
- `scripts/live-pcb-e2e.ps1` — the live PCB harness. Run it with no arguments

## NEXT ACTION

On or after 2026-08-20, run K.1.1: `py -3.11 bench/harness_runner.py --server
target/release/konnect.exe --harness <claude|codex|agy> --repeat 2 --enforce
--log-dir <dir> --out <json>` for each harness, and check the agy run first for
`off_server_calls == 0` — if it is not zero, `AgyMcpConfigGuard` did not wire
the server and the numbers are not a Konnect measurement. Budget is the user's
call each time: a Claude Code run costs ~$0.06 on the lightest task with haiku,
and the six other golden tasks all author something.
