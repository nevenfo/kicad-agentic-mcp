# PROGRESS

## Phase actuelle

J — scope expansion. J.1, J.2 and J.4 are closed, the J.2.4 residue included.
J.3's question is answered; J.3.3's CI half is open again now that the job
actually runs on a runner and fails there.

## Tâche actuelle

J.3.3 — make the `live-ipc` job pass on a GitHub runner, or establish that it
cannot. It runs there now; it fails there (blocage 1).

## Dernière tâche validée

J.2.4.3 + J.2.4.4 — the JLCPCB parts database. The old URL 404'd because
upstream publishes chunked archives, not a single `.db`; and the query tools were
reading a schema no published database has ever had.

Validation :
- `cargo test --workspace`: 960 passed, 0 failed
- `cargo test -p konnect-core --lib jlcpcb`: 15 passed (download path + schema,
  loopback-served archive, no third party)
- `cargo test -p konnect-core --test sourcing_and_manufacturing -- --ignored
  the_published_database`: PASS against the real host
- `cargo clippy -p konnect-core --lib -- -D warnings`: PASS
- `docs/capability-matrix.md` regenerated: sourcing 100 %, the `GAP` is gone,
  KiCAD domains 72.6 % → 73.2 %, fork-vs-baseline 72.0 % → 72.6 %

## Décisions actives

- D48 — `download_jlcpcb_database` defaults to the `basic-preferred` library
  (~2 MB) rather than upstream's own default `current-parts` (~780 MB inflated):
  a caller who asks for "the database" should not get a 175 MB download it cannot
  see the progress of. `library` opts into `current-parts` or `all-parts`, and
  the fetched library is recorded in the file so an empty search result can say
  which one it searched.
- D47 — driving the PCB path needs a *desktop session* but no human. KiCad
  never hands `KICAD_API_SOCKET` to a process it did not spawn; the client
  constructs the deterministic `%LOCALAPPDATA%\Temp\kicad\api.sock` instead,
  with an empty `KICAD_API_TOKEN`. `api.enable_server` must be true before
  KiCad starts. The pipe appears *before* KiCad will answer on it, so every
  live test polls for an open document itself. Full detail: DEV.md, "Driving
  the PCB path unattended".
- D26 stands: the live suites keep their `#[ignore]` and stay `gated` in the
  matrix. The matrix scores what the default suite proves, and the default
  suite has no KiCad. The `live-ipc` CI job is where they actually run.
- D44 — `CAPABILITY_COVERAGE`'s comparison target is frozen: the 187 tools the
  baseline registers at `5cd6454` (this fork registers all 187, so no name
  mapping), minus what KiCAD gives no API for → denominator 186. Both numerators
  come from the same scanner pointed at each tree. Met only when strictly ahead
  *and* no tool the baseline proved is unproved here.
- D45 — an `ipc` tool is never "proved" by its own "KiCAD must be running"
  error, and a `cli` tool is never proved by failing to spawn.
- D46 — a third party is not a test dependency. The JLCPCB/LCSC/Freerouting
  tools are tested for what they do when it is absent; where a real payload is
  needed, it is served from a loopback server built from the published DDL.
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

1. Phase I remains gated: this machine has KiCad 10.0, not the KiCad 11 /
   `kicad-cli api-server` it needs.
2. **The `live-ipc` job fails on `windows-latest`** (J.3.3). Run 32020437428,
   2026-08-17: pcbnew started, stayed alive — the harness distinguishes an exit —
   and `\\.\pipe\...\kicad\api.sock` never appeared within 90 s. The `e2e` job in
   the same run passed, so the runner and the KiCad install are fine; it is the
   GUI-process API server that does not come up. Locally the same script is 3/3.
   Excluded: a missing workflow (see below), a pcbnew crash, a bad socket path
   (the harness logs the one it waits on and it is the deterministic one).
   Next attempt is running: the harness now prints pcbnew's window handle, title,
   responsiveness and CPU time when it gives up, and the CI step waits 180 s, so
   the outcome separates "slow on a cold runner" from "stuck before the API
   server". Run 32021813623.

   Settled on the way (do not re-derive): `origin` has **no** `main` branch — its
   default branch *is* `agentic/main`. The earlier dispatch 404 was that Actions
   had never registered the workflows, because no push had ever touched
   `.github/workflows/`; one push fixed it. `gh` resolves a bare `-R`-less
   invocation to `upstream` (mixelpixx/Konnect), where a dispatch is a 403 and
   whose green "E2E (real KiCAD)" runs are not this fork's. Always
   `-R nevenfo/kicad-agentic-mcp --ref agentic/main`.

## Fichiers / zones utiles

- `crates/konnect-core/src/tools/integration.rs` — the JLCPCB tools; the
  published-database constants (`JLCPCB_LIBRARIES`, `JLCPCB_PART_COLUMNS`) and
  both test modules live here. The scanner counts `#[cfg(test)]` blocks under
  `crates/konnect-core/src/tools` as proof, which is why the hermetic download
  tests sit in the source file rather than in `tests/`
- `scripts/live-pcb-e2e.ps1` — the live PCB harness: starts pcbnew, runs both
  live suites, stops pcbnew. Run it with no arguments.
- `crates/konnect-ipc/tests/live_kicad_test.rs`,
  `crates/konnect/tests/live_kicad_tools.rs` — the live suites
- `crates/konnect-core/tests/harness/mod.rs` — shared rig: calls a tool through
  `ToolRouter` by name; `BLANK_BOARD`, `TWO_RESISTORS`, `pins::*`
- `crates/konnect-core/src/capability/` — `baseline.rs` (frozen V1 target),
  `mod.rs` (`MANIFEST`, `MISSING`); regenerate the matrix with
  `KAM_UPDATE_MATRIX=1 cargo test -p konnect-core --test capability_matrix`
- Run the non-IPC live probes with
  `KICAD_CLI=<path> cargo test -p konnect-core -- --ignored`; the path is in
  `bench/konnect.bench.toml`
- Pre-existing H.6.1–H.6.5 changes are still uncommitted in `bench/`,
  `sch_components.rs`, `sch_wiring.rs`, `library.rs`, `docs/benchmark.md`,
  `docs/local-agents.md`. `sch_components.rs` also holds committed J.2.3/J.2.4
  hunks: stage from that file by filtered patch, never whole-file
- `cargo fmt` is not a gate: the tree has pre-existing drift in several crates,
  so format only the files a task touches (`rustfmt --edition 2021 <file>` pulls
  in `mod` siblings — revert those)
- `cargo clippy --tests` fails inside the uncommitted H.6 changes
  (`await_holding_lock`), which is why the project's clippy gate is `--lib`

## NEXT ACTION

Read run 32021813623's `live-ipc` diagnostics (`gh run view <id>
-R nevenfo/kicad-agentic-mcp --log-failed`) and decide from the process state
whether the API pipe is slow or unreachable on `windows-latest`. If it passes at
180 s, J.3.3 closes; if pcbnew is alive with no window handle, record that a
GitHub runner gives it no usable window station and mark the job `gated` rather
than chasing it further.

Phase K is still unnamed: the user asked for a priority other than the PCB
benchmark coverage but has not said which.
