# PROGRESS

## Phase actuelle

**K — multi-harness.** Phases D, F and L are closed — L's every task was
already checked and `.\gate.ps1` is re-verified green at `6e298e1`. Phase I
stays gated by hardware (KiCad 10.0 here, not the KiCad 11 /
`kicad-cli api-server` it needs). K now holds two lots: K.1, whose claude half
waits on a budget decision, and **K.2**, opened this session — Konnect declares
no MCP tool annotations, and that is what blocks the codex half.

## Tâche actuelle

**K.2.1** — `McpToolDescription` gains an optional `annotations` object, filled
by both producers. Nothing is half-written yet.

## Dernière tâche validée

**K.1.7 and K.1.8** — two findings from three real codex runs on the day the
account's usage limit expired.

K.1.7: `codex exec --ignore-user-config` skips only `$CODEX_HOME/config.toml`;
`AGENTS.md`, `skills/`, `plugins/` and `.rules` load anyway, and the first run
spent its whole budget trying the operator's private `rtk` toolchain.
`CodexHomeGuard` gives the campaign a temp `CODEX_HOME` holding a copy of
`auth.json` and nothing else.

K.1.8: codex 0.147 cancels an MCP tool call whose tool carries no annotations —
an approval request with no responder in non-interactive `exec`. That, not the
prompt and not the wiring, is why codex called Konnect zero times.

Validation :
- second real codex run: the `rtk` attempts are gone, konnect still uncalled
- a four-tool stand-in MCP server, one `tools/list`, all four called in one run:
  `readOnlyHint: true` **ran**, no annotations **cancelled**,
  `readOnlyHint: false + destructiveHint: false` **ran**,
  `destructiveHint: true` **cancelled**
- ruled out first, each by its own run: `approval_policy="never"`,
  `mcp_servers.<name>.default_tools_approval_mode="auto"`, project
  `trust_level`, and a wiring fault (`codex mcp list` shows the server declared,
  and codex did emit a real `mcp_tool_call` for `konnect.find_capabilities`)

## Décisions actives

- D93 — MCP `annotations` are part of the shipping surface, not a nicety: a
  client that gates on them refuses an unannotated server outright, with no
  human in the loop to override it. And `destructiveHint` means *irreversible*,
  not "writes" — codex cancels a destructive tool exactly as readily as an
  unannotated one, so spending that flag on a routine write removes the tool
  from every headless client. See K.2.
- D92 — a headless harness measures its own home unless the home is replaced.
  A CLI flag that promises to ignore user configuration ignores one file;
  instructions, skills and plugins arrive regardless. The bench's answer is a
  throwaway `CODEX_HOME` carrying only credentials, not a longer flag list.
  What comes from the *account* rather than the machine — codex's own remote
  plugins — cannot be removed client-side and is recorded rather than fixed.
- D91 — an agentic audit judges what went *through* the gateway, and a refused
  call is not contamination. Two halves of one measurement bug, both found by
  spending $0.32 on one task before spending on a campaign. `bench/runner.py`
  has said since K.1.2 that `kicad_invoke` is a door and judging it marks every
  gateway run as a write; the harness runner read names off `tool_use` blocks
  and never unwrapped, so an agent that used the gateway — which Opus 5 does by
  default — scored a false `safety` violation and zero `expected_tools`. The
  names come from the reply's per-entry `tool` field, never the request, so
  what is audited is the server's own answer about what it ran. The mirror
  half: `--tools ""` genuinely removes a built-in, but the model can still emit
  a call for one and get "No such tool available" back — the isolation working,
  not a breach. Corollary that kept the fix honest: only `parse_stream` unwraps,
  because only its shape was read against a real transcript; `parse_codex_jsonl`
  gets a `WARN` when the audited path still names `kicad_invoke` instead of a
  guessed unwrap asserted as a measurement.
- D90 — a tool description earns its retrieval by naming the *goal*, and
  naming the actions is how it overpays. F.5.5's lever — say the thing in the
  domain's own words — does not generalise to a tool that *composes* other
  tools: `apply_plan`'s operations are place, power, label, wire, connect,
  decouple, and a description saying so ranks it against
  `batch_place_components` and `add_power_symbol` on tasks that want one edit,
  not a plan. Measured, and the legible cost is not the 1.7 precision points
  but the +140 catalog tokens loaded on 3 of 7 tasks for a schema none of them
  calls. One goal sentence buys the same reachability for zero. Corollary
  bounding the claim: the three goal queries still missing are the long ones,
  because clause splitting decides per clause and `apply_plan` is the best
  answer to none of "supply rails", "a wire between them", "a labelled output"
  — D68 stands, with an edge one query wide instead of zero.
- D89 — a mode restricts by *what a write leaves behind*, not by whether it
  writes. A gerber lands on disk exactly as a schematic edit does, so `Effect`
  could never separate them; `WriteTarget { DesignDocument, Derived }` is the
  second axis, and it is orthogonal rather than a third `Effect` variant because
  `Effect` answers D56's question only and the matrix's `effect` column is
  parsed by `bench/capabilities.py`, which keeps just the exact strings `read`
  and `write` — widening it would have silently emptied the bench's table. The
  fail-safe is `DesignDocument`, mirroring D58: a tool added tomorrow is refused
  under `MANUFACTURING` rather than allowed by accident, and both `Write`
  meta-tools take it with no named exception, `kicad_agent` included. The paired
  half: `EXPERIMENTAL` is given **no** rule, because no use case for one exists
  anywhere in the repo and inventing one to justify a name is INV4 read
  backwards — it is a documented alias of `WRITE`, pinned by a test that runs
  under it the very tool `MANUFACTURING` refuses.
- D88 — atomicity has two remedies, and a commit is only the second one. A site
  needs neither unless it sends more than one *mutation*: a single mutating
  command is already atomic in KiCad, and wrapping it in `BeginCommit`/
  `EndCommit` would buy nothing but two round trips. Where several mutations are
  of the same nature, merging them into one `CreateItems` beats a commit — it is
  atomic by construction and costs less. `run_commit` is for mutations of
  *different* natures, which cannot be merged: `replace_track`'s delete followed
  by a create is the whole of that category on this path. Corollary that decided
  the scope: `add_track`, `place_footprint`, `refill_zones` and the four board
  outlines each send exactly one mutation and were deliberately left alone.
- D87 — a serialised queue adds **no retry and no timeout**, and both refusals
  are the point. A job is an `FnOnce` whose effects are not observable from
  outside, so replaying it after a partial failure is precisely the double-apply
  D.9 exists to prevent; the only safe retry is the caller's own, with a key,
  which `IdempotencyLedger` already serves. A timeout is the mirror image: every
  command is already bounded by `send_command` (5 s send / 30 s recv) and a job
  sends a finite number of them, so a queue-level deadline would only turn "this
  is slow" into "I no longer know whether it applied". Two corollaries: the key
  is the IPC *address*, because it names the KiCad instance being serialised
  against and because it is what lets tests be independent without a shared
  environment variable (D67); and submission is synchronous, because an
  `async fn` would have made queue order depend on poll order.
- D86 — a revision names a *position in a timeline*, and the two ways an entry
  can name it mean opposite inclusion. An entry whose `before == since` is
  itself the change away from `since` and is the first one to report; an entry
  whose `after == since` has already arrived there, so only what follows counts.
  The design said "the last entry naming `since`, then everything after it" and
  was wrong for the common case — a caller asking right after a batch — which is
  why the rule is written here rather than left implicit in a `rposition`.
- D85 — the delta path is **pull**, and the file watcher D.7.2 named is
  deliberately not built. A watcher is a daemon that would have to survive a
  restart to be worth anything, and the one question it would answer — has this
  document moved since `rev` — is already answered on demand by D.1's
  content-addressed revision, with no state to keep in sync. D.7.3 is the other
  half: a watcher whose findings may never be pushed has no consumer but the
  poll that exists anyway. Recorded under D.7's Validation in plan.md rather
  than dropped silently.
- D84 — the run journal records a *restore point*, never a capability, and a
  field is only worth having while it is true. `rollback_token` names the entry
  whose pre-image is on disk; nothing in MCP accepts it and nothing from the
  journal enters a reply, because publishing an address no tool accepts is D82
  read backwards. Its truth is re-established on the way **out** of `entries()`
  by looking at the directory, not on the way in: the line is append-only and
  eviction is a budget decision, not a correction to what happened. Two
  corollaries: only the files a batch *changed* are imaged, so the cost is
  proportional to the change and not to the project; and `root` is the one
  absolute path in a line — every document and image path hangs off it — because
  a journal that could not be addressed by document could not answer
  `changes_since`.

- D83 — an address resolves to a *position*, and the position is what the edit
  uses. Stated for sheets as D81 and now the rule everywhere: the units of a
  multi-unit symbol share a designator, so a handler that resolved a uuid and
  then redescended by name edited unit 1 in silence. Corollary about `cse`:
  `SymbolCollection` got `remove_at` and deliberately no `by_uuid` — a lookup
  by identity is the thing this rule says not to do a second time.
- D82 — an address a tool accepts must be an address some tool publishes,
  and the two ship together. `list_schematic_labels` returned positions and
  net names and no uuid, so accepting a label uuid alone would have been an
  address nobody could obtain. The rule has a live counterexample recorded as
  D.4.1.8: nothing lists junctions or no-connects, so `delete_no_connect`'s
  uuid form is reachable only right after `add_no_connect` returned it.
- D81 — a mutable lookup is reached by *position*, never by re-addressing what
  a name already resolved. `cse` reads a missing `(uuid …)` as the empty
  string, so translating a name-resolved sheet into its uuid would make two
  identity-less sheets answer to the same `""`. The layer below already
  refuses that document (`ItemId` rejects an empty uuid at commit time, so the
  failure is a refusal and the file is untouched — pinned by
  `a_sheet_without_a_uuid_is_refused_rather_than_edited`), which is why this
  is structural rather than a bug fixed: the resolver should not depend on a
  guarantee that belongs to another layer.
- D80 — `reference` wins when a call carries both addresses, and the resolved
  `reference` is *read out of the block*, never echoed from the request: a
  `uuid` call and a `reference` call then hand the same string to everything
  downstream. The `reference` branch reads nothing and keeps its own
  "not found" messages, so INV8 is preserved by construction rather than by
  test. Known gap, recorded as D.4.1.7 rather than papered over: the seven
  handlers that redescend by designator land on unit 1 of a multi-unit symbol
  even when the uuid named another unit.
- D79 — an item's identity is its *own* direct-child `(uuid …)`, and the shared
  resolver indexes only that. `batch_delete` is therefore deliberately left on
  its textual search: it has always accepted a UUID nested inside the item it
  deletes — a `(sheet …)`'s own `(pin …)` — and walked out to the enclosing
  top-level block, which the index answers `NotFound` for. The permissive input
  is now pinned by a test rather than by the absence of one, so migrating it
  later is a decision to drop an accepted address (INV8), not a refactor.
  Corollary for the two sites that did move: a UUID that resolves to the wrong
  *kind* is not `NotFound` — `delete_wire` sends it down the same "cannot
  locate a wire block" path an unresolved enclosing-tag search produced before.
- D78 — a third-party service is its own failure domain. `UpstreamFailed
  { service, code, detail }`: nothing in a failed JLCPCB download is the
  caller's fault, the filesystem's or KiCAD's, and `code` separates what prose
  cannot — `unreachable` / `server_error` are `Network` (waiting is the
  recovery), `client_error` / `unexpected_response` are `None`. 429 files with
  the 5xx: it is the one 4xx that says "later", and filing it with 404 would
  tell a client to give up on a rate limit. `service` is the host, never the
  URL — a field a client matches on must not change per chunk.
- D77 — `MalformedDocument { path, detail }` is the gap between four kinds that
  each nearly fit: `Io` (the read succeeded), `FileNotFound` (the file is
  there), `InvalidArgument` (the call is well-formed — the caller named a
  document that exists) and `NotFound` (the addressed item is present; the
  document around it cannot be used). `TransientClass::None`, not `State`:
  `State` promises that reconciling and retrying is the recovery, and
  re-reading a board whose `(layers)` section is missing returns the same
  board. Added only once six sites across four files had converged on the
  shape — which, with D.6.1 zone 3 declining a kind for one site, is the bar a
  new kind has to clear.
- D76 — a typed boundary error carries the *reason*, and one place turns it
  into a `CallToolResult`. Two shapes proved this out: `IpcFailure` (the caller
  can only ask "may I edit the file behind KiCAD") and `FootprintPathError`
  (three of its four variants are `NotFound`, told apart by `item_kind`, which
  is what the caller acts on). Corollary, from the one site that kept its
  prose: `kind()` may return `Option<ToolErrorKind>`, and a `None` is a
  statement — a malformed `.kicad_mod` is not IO, not a missing item and not a
  malformed argument, and a kind invented for a single site would be worse than
  the prose.
- D75 — a catch-all error kind is debt, not a catalogue entry, and the debt
  scanner counts it as such. Converting plain text into
  `ToolErrorKind::HandlerError` lowers the count while telling the caller
  nothing new and asserting `TransientClass::None` — "do not retry" — on
  failures where starting KiCAD and retrying is exactly the fix. A false
  `transient` is worse than none, so a site whose cause cannot be told apart
  stays plain text with the reason written at the site. `from_anyhow` is not
  debt: it classifies from the error chain at runtime and reaches the catch-all
  only when the chain carries nothing better.
- D73 — a failure mode an agent reads decides which loop it runs, so
  `COULD_NOT_RUN` is barred from `design` structurally, not by convention: the
  private constructor for that path has no `Design` variant available. INV1
  already said a validator that could not run is a failure rather than zero
  findings; this is that rule where the compiler can enforce it. The paired
  rule: `MANUAL_STEP_REQUIRED` is a catalogued kind, never a prefix in prose,
  and its `step` text is read from the capability's `Limitation::GuiOnlyNoApi`
  reason so the error and `docs/capability-matrix.md` cannot drift.
- D74 — a test fixture must never name a real MANIFEST tool. The coverage
  scanner reads tool names out of test sources, so `tool: "autoroute"` in an
  error fixture credited `autoroute` with a proof pointing at `mcp/error.rs` —
  a tool proved by its own error, which is exactly what D45 forbids. Fixtures
  name something fictional.
- D72 — a snapshot handle carries a *manifest*, never the before-images: roots,
  file count, and per file its path relative to its root, its revision and its
  size. Two reasons, and the second is the load-bearing one: the bytes would
  blow the store's 4 MiB budget, and a handle that could restore would make an
  audit artefact into a capability, which D12 deliberately keeps internal.
  Paths are relative because a model reads this body and an absolute path would
  leak the caller's filesystem layout for no audit value. Second-order cost,
  measured rather than discovered later: a capturing batch stores two artefacts
  instead of one, so the store's 64 entries span half as many batches (D.5.3).
- D71 — the operating mode is fixed at startup and never elevable in-session:
  `ModeGuard`'s only public mutator restricts, and passing a less restrictive
  mode is a no-op rather than an elevation, so no meta-tool a model can reach
  widens what the process may do. It is `#[serde(skip)]` on `Config` for the
  mirror-image reason: a stale `read-only` in a saved settings file must not
  lock a server the operator meant to run writable. `MANUFACTURING` and
  `EXPERIMENTAL` parse and travel end to end but enforce nothing yet — a mode
  claiming a restriction it does not apply is exactly what INV4 forbids, so the
  rule and the MANIFEST classification that makes it observable land together
  in D.8.3 or not at all.
- D70 — **AGY is out of scope** (user, 2026-08-18). K.1.1 measures Claude Code
  and Codex only. The adapter, `AgyMcpConfigGuard` and `parse_agy_stream` stay
  in `bench/harness_runner.py` unused: they cost nothing there and deleting
  proven code buys nothing. What was learned about agy's MCP wiring is recorded
  in plan.md K.1.4 as a finding, not as work owed, and D61 is retired with it.
- D68 — a tool is retrievable only through the vocabulary of the *change*, not
  of the machinery. `apply_plan` is invisible to every query stating a design
  goal, so the plan path is entered by prior knowledge (starter kit,
  `list_toolboxes`, system prompt) and never by search. Precision @8 is
  therefore the wrong instrument for it: the metric's denominator is the union
  search returns, so a shape that needs one tool scores badly however well it
  is served. The plan's measured win stays the schema tokens of G.3.
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
- D60 — a meta-tool's `effect` answers D56's question only: can this call mutate
  the *project on disk*. Session state (which tools `tools/list` exposes) is not
  a disk mutation, so `load_tools` / `load_toolset` / `unload_toolset` are
  `read`. Exhaustiveness is structural: `define_meta_tools!` generates the
  dispatch `match` and `META_TOOL_NAMES` together.
- D59 — each harness declares an isolation level. `tools-off` (Claude Code,
  `--tools ""`) makes any off-server call contamination; `read-only-sandbox`
  (codex) cannot remove built-ins. Hence two rates: `SUCCESS_RATE` (strict,
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

Aucun.

Phase I remains gated by hardware rather than by work: this machine has KiCad
10.0, not the KiCad 11 / `kicad-cli api-server` it needs.

## Fichiers / zones utiles

- `bench/harness_runner.py` — the agentic runner. `HARNESSES` (argv builder +
  isolation + parser per harness). The agy entries stay but are out of scope
  (D70). `--dry-run` spends nothing and touches no config. Run it with `py -3.11`.
  `HarnessResult` carries two paths and they answer different questions:
  `tool_calls` (round trips, what `max_calls` counts) and `audited_calls` (what
  `audit()` judges — `unwrap_gateway_batch` replaces each `kicad_invoke` with
  its reply's per-entry `tool` field). `--log-dir` is what makes a paid run
  re-scorable offline: feed the `.jsonl` back through `parse_stream` + `audit`
  instead of re-running the agent. `CodexHomeGuard` (built in `main`, never for
  `--dry-run`) is what keeps a codex run out of the operator's own
  `CODEX_HOME`; `codex mcp list -c <same overrides>` answers "is the server even
  declared" without spending a model call, and `codex exec --help` is the
  authority on what `--ignore-user-config` does and does not skip
- `bench/agent_prompts.yaml` — one plain-language prompt per golden task; no
  tool names, or the run would measure instruction-following
- `bench/runner.py` — `audit()`, `fingerprint()`, `THRESHOLDS`; the harness
  runner imports all of it rather than reimplementing, which is the only reason
  the two sets of numbers compare
- `bench/plan_retrieval.py` — F.5.2's instrument: the direct shape against the
  plan shape under `--load-mode search`'s own methodology, control read from
  the task file so it cannot drift in the plan's favour
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
- `crates/kam-state/src/journal.rs` — the whole run journal: `RunJournal`
  (`append` / `entries` / `image`), `JournalLimits`'s three independent budgets,
  and the eviction that leaves lines alone. Domain-free like the rest of
  `kam-state`: it is handed its directory, and never resolves one itself. The
  server's directory comes from `observability::journal_dir()`, which honours
  `KONNECT_STATE_DIR` only when absolute; the write site is
  `router::meta_tools::write_journal_entry`, called from `BatchGuard::finish`
  while the snapshot is still alive — the only moment `before()` is reachable.
  Its only reader is `handle_changes_since` in the same file, whose
  `paths_match` is what reconciles a caller's path with a journal line
- `crates/konnect-ipc/src/client.rs` — `run_commit` (the undo transaction, drops
  on any error), `add_tracks` / `replace_track` / `TrackSpec`, and the composite
  `place_footprint` whose four-command read-modify-write is why D.9.1 exists.
  Its mock lives in `crates/konnect-ipc/tests/mock_server_test.rs`: an NNG rep0
  server on `inproc://` that records the `type_url` sequence, which is how the
  commit behaviour is asserted without KiCAD
- `crates/konnect-core/src/tools/ipc_queue.rs` — the per-address FIFO every IPC
  call passes through, and the only place a `KiCadIpcClient` is built outside
  tests is still `ipc_boundary.rs::with_ipc`, which is now its only caller. The
  worker thread owns one client for the life of the process; a job that panics
  is caught so it cannot wedge the queue behind it
- `crates/konnect-core/src/mode_gate.rs` — the whole D.8 gate: `check()` /
  `refuse()`, consulted by `mcp/handler.rs::dispatch_tool` and by
  `handle_kicad_invoke`'s per-entry loop, never anywhere else. `kicad_invoke`
  stays exempt at the outer dispatch and is gated per entry, so a batch of
  exports runs under `MANUFACTURING` while a batch touching the design does not.
  The mode itself is `crates/kam-state/src/mode.rs` (`tier()` is where the
  ordering lives); the policy is `capability::mode_allows`, and the second axis
  is `capability::tool_write_target` / `meta_tool_write_target` with its
  `DERIVED_WRITES` list
- `crates/konnect-core/src/router/meta_tools.rs` — `define_meta_tools!` is the
  single source for both the dispatch `match` and `META_TOOL_NAMES`
- `crates/konnect-sexp/src/writer.rs` — the whole write model: `apply_edits` and
  the block finders whose byte offsets feed it
- `crates/konnect-core/src/tools/ipc_boundary.rs` — the only crossing point
  between a handler and KiCAD's IPC API: `with_ipc` (typed) and
  `ipc_error_result` (catalogued). No handler re-derives either
- `crates/konnect-core/tests/error_catalog_debt.rs` — the debt scanner and
  its ceiling (2). Both directions fail: it counts `CallToolResult::error(`
  call sites *and* literal `ToolErrorKind::HandlerError`, so the metric cannot
  be satisfied by moving prose into the catch-all
- `crates/konnect-core/src/mcp/error.rs` — `TransientClass`, `retry_after_ms()`,
  `ToolErrorKind::transient_class()`
- `crates/konnect-core/tests/concurrent_gui_edit.rs`, `tests/lock_recovery.rs` —
  how to race a foreign writer deterministically
- `.github/workflows/ci.yml` — triggers on `agentic/main` as well as `main`;
  dispatch the KiCad E2E one as `gh workflow run e2e-kicad.yml -R
  nevenfo/kicad-agentic-mcp --ref agentic/main` (a bare `gh` resolves to
  `upstream` and 403s)
- `gate.ps1` — the local mirror of CI, but not a substitute for it: L.2.6 was
  green here and red on all three CI runners. `-Bench` resolves its own Python
  (`py -3.11`, else `python`) by checking that it can import `tiktoken`: bare
  `python` on this machine is a 3.14 that cannot, which silently made the whole
  bench half of the gate unrunnable
- `scripts/live-pcb-e2e.ps1` — the live PCB harness. Run it with no arguments

## NEXT ACTION

Implement **K.2.1** — add the optional `annotations` object to
`McpToolDescription` (`crates/konnect-core/src/mcp/protocol.rs`) and fill it in
both producers: `meta_tools::meta_tool_descriptions()` and
`ToolDef::to_mcp_description`, the two sites `mcp/handler.rs`'s `tools/list`
arm concatenates. Then K.2.2 derives `readOnlyHint` from the existing effect
table and K.2.3 decides `destructiveHint` per tool. Validate with
`cargo test --workspace`, then the run K.2.4 names: one codex
`--task sch_inspection` whose `tools called:` is no longer empty.

Still open and still the user's to decide, unchanged by this session: the
**claude half of K.1.1** needs a budget *and* a model. `claude -p` with no
`--model` takes `claude-opus-5`; the one-task smoke run cost **$0.3172** on the
cheapest of the seven tasks, and six of the seven author something. `--model`
and `--max-budget-usd` are the two levers. The codex half costs no dollars
(ChatGPT subscription) and is blocked on K.2, not on money.
