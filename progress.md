# PROGRESS

## Phase actuelle

J — scope expansion. J.1, J.2, J.3 and J.4 are all closed, the J.2.4 residue and
the CI half of J.3 included. Nothing in phase J is open.

## Tâche actuelle

None. Phase K is unnamed: the user asked for a priority other than the PCB
benchmark coverage but has not said which.

## Dernière tâche validée

J.3.4 — the `live-ipc` job runs on a GitHub runner and passes. `windows-latest`
does give pcbnew a usable window station, which answers J.3.3's open question.

Validation :
- run 32026731031, `live-ipc`: 3/3 live tests, exit 0; `e2e` green in the same run
- `scripts/live-pcb-e2e.ps1` locally, fresh profile (`APPDATA` at an empty
  directory): 3/3, exit 0 — and unchanged against the normal profile
- J.2.4.3 + J.2.4.4 before it: `cargo test --workspace` 960 passed / 0 failed,
  `cargo clippy -p konnect-core --lib -- -D warnings` PASS, matrix regenerated
  (sourcing 100 %, the JLCPCB `GAP` gone, KiCAD domains 73.2 %)

## Décisions actives

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

Phase I remains gated: this machine has KiCad 10.0, not the KiCad 11 /
`kicad-cli api-server` it needs.

## Fichiers / zones utiles

- `scripts/live-pcb-e2e.ps1` — the live PCB harness: prepares the profile,
  starts pcbnew, runs both live suites, stops pcbnew. Run it with no arguments.
  Its window-enumeration diagnostics are the tool for any future "KiCad is
  reachable and not answering": they print the modal's text
- `.github/workflows/e2e-kicad.yml` — `e2e` + `live-ipc`, weekly and on demand.
  Dispatch it as `gh workflow run e2e-kicad.yml -R nevenfo/kicad-agentic-mcp
  --ref agentic/main`; a bare `gh` resolves to `upstream` and 403s. `origin` has
  no `main` — its default branch *is* `agentic/main`
- `crates/konnect-core/src/tools/integration.rs` — the JLCPCB tools; the
  published-database constants (`JLCPCB_LIBRARIES`, `JLCPCB_PART_COLUMNS`) and
  both test modules live here. The coverage scanner counts `#[cfg(test)]` blocks
  under `crates/konnect-core/src/tools` as proof, which is why the hermetic
  download tests sit in the source file rather than in `tests/`
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

Ask the user which priority phase K is, then build the phase around it — objective,
dependencies, tasks, validations — before writing any code. The one candidate
already on the table and declined is the PCB benchmark coverage J.3 unblocked.
