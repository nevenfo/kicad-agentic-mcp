# PROGRESS

## Phase actuelle

K — multi-harness. L.2.8 is closed, so K.1.1 is again the only thing left that
is not externally blocked by a machine or an account, and it is the last
dependency of phase M (final benchmark; H.6 and H.7 are DONE).

## Tâche actuelle

K.1.1 — run the golden suite through Claude Code, Codex and AGY. The runner
(K.1.3) is written and proven; what is missing is the measurement, blocked
externally until 2026-08-20, not by any code.

## Dernière tâche validée

L.2.8 / L.2.8.1 (b03d7f3) — the flaky `board_design_rules_round_trip_through_
the_file`. The race was `HOME`: `redirected_user_config` repoints it for the
config tests, and the design-rules test resolved its document lock through
`dirs::data_local_dir()`, which on macOS sits in the same `konnect/` subtree —
so its lock file was born in another test's `TempDir` and deleted under it.
The harness now sets `KONNECT_STATE_DIR` once per test binary; the write and
lock paths now name the operation and file in every IO error.

Validation :
- CI run 32108806941 green on ubuntu, macos and windows, no job cancelled
- `gate.ps1` green end to end (fmt, clippy, test, doctest, release build), no
  warning
- locks observed under `target/tmp/konnect-state/locks/`, not in the profile
- the earlier reading of run 32103900156 as a macOS+Windows failure was wrong:
  Windows was cancelled by the matrix fail-fast, and that misreading is what
  sent the first diagnosis looking for a shared cause

## Décisions actives

- D67 — one test binary shares one environment, so a variable a test repoints
  is global state every other test reads without knowing it. The document lock
  path no longer reads `HOME` in tests: the harness sets `KONNECT_STATE_DIR`
  once per binary to `<CARGO_TARGET_TMPDIR>/konnect-state`. The second half of
  the lesson is that an IO error naming neither its operation nor its path
  makes a flake unattributable after the fact — `write_atomic` touches the
  document, the scratch file, the lock file and the lock directory, and one
  bare `Invalid argument` covered all four.
- D66 — the retrieval ceiling was *how many* results came back, not their
  order. Every query padded its answer to `limit`, so one task's union reached
  34 tools for the ~7 it needed. `search` now cuts each clause at 0.65 of that
  clause's own best score and caps each tool family at one, so a decided query
  deliberately returns fewer hits than asked for. `family_of` keeps a name's
  terms **in order**: `get_component_nets` and `get_net_components` are
  different tools and an order-insensitive key would let the cap delete one.
- D65 — a search configuration is only allowed into production once
  `bench/runner.py --load-mode search` reproduces it server-side. The offline
  probe asserts on startup that it matches production `search()`, and its all7
  numbers have matched the runner's to the decimal twice; that is what makes it
  usable for exploration, not a substitute for the run of record.
- D64 — a retrieval lever ships only if the probe shows it moving a number.
  Measured and **dropped**: the one-character noise filter (the `s` of
  "component's" pays +10.37 and moves no ratio anywhere). Measured and kept on
  narrow grounds: component <-> symbol costs nothing and buys nothing except at
  tau=0.75. Synonyms are checked against corpus document frequency before being
  added — `location` has df 0 in this registry and would be dead weight.
- D63 — the fix for a plural query against a singular corpus term is
  *asymmetric*. Stemming (D6) cut the "s" off both sides, which can turn a
  match into a non-match and did. Reverse prefix only ever adds a fallback
  +4/+1, and only when nothing stronger scored, so it cannot cost anyone a
  rank. The three-character floor is measured, not chosen: three-letter EDA
  terms (pin, net, pad) are the common case and are almost always typed plural.
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

K.1.1 stays blocked until **2026-08-20** for two external reasons recorded
under K.1.4: the Codex account is at its usage limit until that date, and
`agy`'s MCP wiring depends on `AgyMcpConfigGuard`, never yet exercised against
a real agy run (D61). The Claude Code path alone is unblocked; the user chose
to run the three harnesses as one campaign instead.

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
- `crates/konnect-core/src/router/capability_search.rs` — the whole retrieval
  pipeline in one file: `Idf`, `split_clauses`, `CLAUSE_SCORE_RATIO`,
  `per_clause_limit`, `family_of` / `MAX_PER_FAMILY`, `SYNONYMS`. Every
  constant carries the measurement that chose it. Its offline instrument is
  `crates/konnect-core/examples/retrieval_probe.rs` (axes D through I, feed it
  `bench/retrieval_intents.py`'s JSON dump); since the pipelines diverged its
  startup check asserts production *behaviour* — D6 rank 1, a composite intent
  returning a tool per clause, a decided query under the limit — instead of
  score-for-score equality. The run of record stays
  `bench/runner.py --load-mode search` (D65)
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

Actionable before that date if the user wants it: F.5.2 — whether a compiled
plan moves retrieval precision at all. It was unanswerable while the golden
suite never searched; `--load-mode search` now runs 7/7, so the comparison has
become possible. It needs a scope decision first, so do not start it silently.
