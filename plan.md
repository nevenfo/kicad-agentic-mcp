# PLAN — KiCad Agentic MCP

Strategic source of truth: final target, invariants, full roadmap, dependencies
and validation criteria. Operational state lives in `progress.md`; measurements
live in `docs/benchmark.md`, `docs/local-agents.md`, `docs/capability-matrix.md`
and `bench/results/`; history lives in Git.

**Identifier convention.** Phase letters (`A`…`M`) are the historical phase
names. Lots and tasks are dotted — `D.4`, `H.5.3` — so they never collide with
this project's decision records (`D31`) and defect records (`E24`), which are a
separate namespace referenced from code comments, commit messages and `docs/`.
Look a task up with `rg "H\.5\.3" plan.md`; do not read this file whole.

## Objectif final

Turn the `mixelpixx/Konnect` fork into a KiCad **agentic control layer**: a large
internal capability surface, a small external MCP surface, local LLM agents that
absorb operational work, a deterministic engine for everything that does not need
generative reasoning, task state and evidence held outside any model's context,
and verification that comes from KiCad rather than from an agent's own opinion.

```
WORKSPACE  C:\Users\FlowUP\kicad-agentic-mcp\konnect-agentic   branch agentic/main
BASE       mixelpixx/Konnect @ 5cd6454 (v0.2.2), AGPL-3.0-only, workspace-wide
KICAD      10.0.3, C:\Users\FlowUP\AppData\Local\Programs\KiCad\10.0\bin\kicad-cli.exe
HARDWARE   RTX 5080 16 303 MiB VRAM · Ryzen 7 9800X3D · 32 GiB RAM · LM Studio
```

16 GB VRAM is the hard budget for any local model: it must hold the model, the
KV cache and whatever KiCad's GUI is using.

## Invariants

Rules that survive every phase. Breaking one is a defect, not a trade-off.

- **INV1 — the verdict is KiCad's.** A design is declared sound by `kicad-cli`
  ERC/DRC, never by Konnect's own analysis and never by a model. A validator that
  could not run is a failure, never zero findings.
- **INV2 — generic subsystems stay re-licensable.** New `kam-*` crates are
  clean-room, `MIT OR Apache-2.0`, and depend on no `konnect-*` type. The KiCAD
  half of each lives in `konnect-core::<same name>`. AGPL never flows into them;
  absorbed MIT code travels with its notice.
- **INV3 — no mutation without an audit record**, and every change carries its
  own proof (semantic diff on by default, evidence handles, call log).
- **INV4 — refuse before the first mutation.** Compile-time reference checking,
  `base_revisions`, unknown validator names and unrecognised `verify` values are
  all rejected before anything is written. A caller who believes a check ran when
  it did not is the worst failure mode this project has shipped.
- **INV5 — one grid, one place.** Every electrically meaningful `(at x y)` goes
  through the single snapping helper over `SCHEMATIC_GRID_MM` (1.27 mm). A
  guarantee that holds *usually* is the bug it is meant to prevent.
- **INV6 — a target that is missed is recorded as missed**, never moved to match
  the result, and no win is netted off against it. A number that cannot be
  reproduced from the repository is not a result.
- **INV7 — advisory analysis says so at the call site**, in the tool description
  an agent actually reads, not only in a generated document.
- **INV8 — accept what a model writes only when it has exactly one meaning.**
  Genuine ambiguity stays refused; a widened acceptance must never turn a
  previously compiling input into a failure.
- **INV9 — the local inference backend is loopback-only** unless a named
  constructor says otherwise. Exposing it to the network must be something
  somebody typed.
- **INV10 — the KiCad access strategy is fixed by KiCad 10, not by preference:**
  PCB over IPC, schematic over the S-expression engine, validation and export
  over `kicad-cli`. Do not fork KiCad; re-evaluate at KiCad 11 (Phase I).
- **INV11 — a checkbox in this file means proof**, not intention: targeted tests,
  integration, gate, or a benchmark run whose artefact is committed.

## Plateforme — KiCad 10 ground truth

Verified against KiCad sources, 2026-08-10. These are constraints, not opinions.

| Fact | Consequence |
|---|---|
| IPC = NNG REQ/REP + protobuf envelope, `KICAD_API_SOCKET` / `KICAD_API_TOKEN`, API disabled by default | `doctor` must check and say so |
| No protocol version, only `GetVersion` | capability probing is behavioural |
| **No async events, no pub/sub** | the event journal is ours: revisions + targeted diffing + file watching. Never advertise push notifications |
| Server is single-threaded on the UI thread | serialise IPC, own timeout/retry, expect `AS_BUSY` |
| `BeginCommit` / `EndCommit` exist on PCB **and** SCH handlers | the real transaction primitive for the IPC path |
| PCB coverage complete over IPC | PCB path = IPC |
| **Schematic IPC is empty on 10.0** (`schematic_commands.proto` has no commands, `getItemFromDocument()` returns `nullopt`) | schematic path = S-expression engine |
| `kicad-python` 0.8.0 + `kicad-cli api-server` target **KiCad 11** | headless schematic IPC is upstream work; do not fork KiCad 10 |
| S-expr versions on 10.0: board `20260206`, schematic `20260306`, symbol lib `20251024` | parser/writer compat matrix |

## Architecture cible

```
harness (Claude Code / Codex / AGY)
        │  EXECUTION PATH: delegate            AUDIT PATH: query / verify / evidence
        ▼
┌──────────────────────────┐
│ MCP GATEWAY (small)      │  kicad_describe + kicad_invoke                SHIPPED
├──────────────────────────┤
│ TASK STATE MANAGER       │  objective / constraints / facts / failures   SHIPPED
├──────────────────────────┤
│ CONTEXT + ATTENTION MGR  │  anchor · budgets · compaction · retrieval       SHIPPED
├──────────────────────────┤
│ AGENT ROUTER             │  NO_LLM | LOCAL | ESCALATE (fitted, H.6.2)        SHIPPED
├──────────────────────────┤
│ LOCAL AGENT RUNTIME      │  supervisor / schematic / pcb / verification      SHIPPED
├──────────────────────────┤
│ PLAN COMPILER + PLAN IR  │  typed, reference-checked, batched            SHIPPED
├──────────────────────────┤
│ DETERMINISTIC ENGINE     │  ~190 capabilities + revisions + transactions SHIPPED
├──────────────────────────┤
│ KiCad: IPC (PCB) · S-expr (SCH) · kicad-cli (validate/export)
├──────────────────────────┤
│ VALIDATION + EVIDENCE    │  ERC/DRC, semantic diff, evidence packs       SHIPPED
└──────────────────────────┘
```

Crates (new ones obey INV2):

```
crates/konnect-*      existing, AGPL, refactored in place
crates/kam-state      revisions, idempotency, snapshots, Task State + anchor      SHIPPED
crates/kam-evidence   semantic diff over an abstract ItemSet, handle store,
                      findings with stable ids                                    SHIPPED
crates/kam-plan       Plan IR, compiler, reference resolution, execution FSM      SHIPPED
crates/kam-graph      indexed store + query/neighbour language                    SHIPPED
crates/kam-llm        local provider abstraction (+ router, not built)            PARTIAL
crates/kam-context    budgets, compaction, retrieval                              SHIPPED
crates/kam-bench      benchmark runner + metrics schema                           NOT BUILT (Python harness in bench/ does the job today)
```

## Critères globaux de réussite (V1)

Current values: `docs/benchmark.md`. Targets are never moved (INV6).

- [x] `SUCCESS_RATE` ≥ baseline — 18/18 golden, held across every phase
- [x] median `MCP_CALLS` per task ≤ 5 — **4**
- [x] `WALL_CLOCK_P50` ≤ baseline — 65 ms against 70 ms
- [x] silent corruption / silent stale-state write = **0** — refused by `base_revisions`
- [x] mutations without an audit record = **0**
- [ ] external tokens/task ≤ 2 000 — **~2 185**, missed by ~185 in deliberate
      trades (diff on by default, task filing, verification); recorded as missed
- [ ] `tools/list` at startup ≤ ~1 000 — **2 034**, missed; only reachable by
      retiring the toolset-loading path, which would break every shipped skill
- [ ] retrieval precision @8 ≥ 60 % — **22.4 %** (recall @8 100 %) — see F.5
- [ ] `LLM_CALLS_PER_SUCCESSFUL_TASK` materially below baseline — measured
      **15 → 5.5** inside the model-fit harness, but **no baseline for this
      metric was ever measured**, so the criterion is not claimed met
- [x] `CAPABILITY_COVERAGE` > baseline — **72.6 % against the baseline's
      22.6 %**, on a frozen denominator of 186, with no regression. Defined in
      J.2.1 and rendered in `docs/capability-matrix.md` under
      *V1 comparison target*: the 187 tools the baseline registers at `5cd6454`
      (this fork still registers all of them), minus what KiCAD gives no API
      for, scored on both sides by the same scanner. The headline number is
      the whole-surface number and is deliberately not the criterion — its
      denominator grows with every tool added; it stands at **72.6 %** too after
      J.2.3. Nothing this fork adds can enter the frozen denominator, so that
      percentage moves only when a test that runs starts proving a tool

---

# Phase A — Bootstrap — DONE

## A.1 — Base selection (Gate 0)

### Objectif
Choose the base repository on measured evidence and record the licence posture.

### Dépendances
None.

### Tâches
- [x] A.1.1 Compare `Konnect` / `kicad-mcp-pro` / legacy `KiCAD-MCP-Server`
- [x] A.1.2 Verify licences and choose officially → Konnect, AGPL-3.0-only
- [x] A.1.3 Clone into a clean workspace, branch `agentic/main`

### Validation
`cargo test --workspace --lib --tests` → 469 passed on the untouched fork.
Full comparison and rationale: `git show HEAD:plan.md` (Gate 0 sections, before
this file was migrated) and decisions D1/D2.

**Why not `kicad-mcp-pro` as the base** (still the reason not to migrate): its
schematic writer depends on `kicad-sch-api`, which drops `global_label` nodes on
save, so its own mitigation refuses the write; profiles are boot-time only; three
build systems; 251 Python modules force deferred registration. Its *ideas* were
adopted clean-room and are tracked as tasks (D.6, D.7, D.8, D.9, J.5, M.1).

## A.2 — Build, tests, real MCP session — DONE

### Tâches
- [x] A.2.1 Build and test the fork on this machine (`protoc` provisioned, E1)
- [x] A.2.2 Run the server against a real harness over stdio
- [x] A.2.3 `gate.ps1` mirrors the upstream CI gate

### Validation
`gate.ps1` green; per-user KiCad installs discovered (E3).

---

# Phase B — Cartography — DONE

## B.1 — Map the base

### Tâches
- [x] B.1.1 Map transport, registry, IPC, S-expression engine, validation, errors
- [x] B.1.2 Verify the KiCad 10 ground truth against KiCad's own sources

### Validation
The ground-truth table above; every claim traced to a KiCad source file.

---

# Phase C — Baseline benchmark — DONE

## C.1 — Golden suite and metrics

### Objectif
Measure the untouched base before any refactor, so every later claim has a floor.

### Dépendances
A.2.

### Tâches
- [x] C.1.1 Golden projects + task set (6 tasks × 3 load modes = 18)
- [x] C.1.2 Metrics schema: tokens, calls, latency, success
- [x] C.1.3 Baseline recorded

### Validation
`docs/benchmark.md`, `bench/results/*baseline*.json`: 12 373 external tokens/task,
11 MCP calls, 18/18.

---

# Phase F — Compact MCP surface — DONE except F.5

## F.1 — Capability index and tool-granular loading

### Tâches
- [x] F.1.1 `find_capabilities` + `load_tools` beside `list_toolboxes` / `load_toolset`
- [x] F.1.2 Keep both loading paths (D5: removing one breaks every shipped skill)

### Validation
Retrieval recall @8 = 100 %; measured in `docs/benchmark.md`.

## F.2 — Schema compression and starter kit

### Tâches
- [x] F.2.1 Compress heavy inlined schemas — no `$defs`/`$ref` (D7: client chain
      mangles them; several upstream issues recommend inlining instead)
- [x] F.2.2 Shrink the starter kit — `config` leaves, two tools re-admitted
      individually (D8), checked against `find_capabilities` on their own intents

### Validation
3 698 → 3 197 tk/task, startup 1 958 → 1 454 at the time.

## F.3 — The gateway

### Objectif
A catalogue that never has to be refreshed: `CATALOG_TOKENS` → 0.

### Tâches
- [x] F.3.1 `kicad_describe` + `kicad_invoke` — **two** verbs, not seven (D9)
- [x] F.3.2 Batch semantics: `stop_on_error`, and `atomic` follows it unless set
      explicitly (D10 — found by the benchmark, not by review)

### Validation
1 995 tk/task, 4 MCP calls, `CATALOG_TOKENS` 0, 18/18.

## F.4 — Capability matrix

### Tâches
- [x] F.4.1 `docs/capability-matrix.md` generated from `konnect-core::capability`
- [x] F.4.2 `SUPPORTED` is discovered from tests and golden tasks, never declared
      (D26); `#[ignore]`d tests read `gated`; what KiCad has no API for leaves the
      denominator
- [x] F.4.3 Three tests: manifest names every registered tool, names no tool that
      does not exist, committed markdown equals what the code renders

### Validation
`KAM_UPDATE_MATRIX=1` regenerates; the equality test fails if it drifts. First
render 27.3 %, now 28.6 %. Note: `bench/probes` counts as evidence as well as
`bench/tasks`, so adding a probe can move the number.

## F.5 — Retrieval precision — OPEN

### Objectif
22.4 % precision @8 at the recall needed to succeed. The gateway made each wrong
guess cheap; it did not make the guess right.

### Dépendances
None. Plural stemming was implemented, measured and **rejected** (D6): recall
100 % → 98.2 % at 8 results. Do not re-attempt it.

### Tâches
- [ ] F.5.1 Find a retrieval change that raises precision without costing recall
- [ ] F.5.2 Answer whether a compiled plan moves precision at all — the golden
      suite is a scripted oracle and can never show it (it never searches)

### Validation
Precision @8 ≥ 60 % with recall @8 ≥ 98 %, measured by the existing retrieval
probe, before/after on the same build.

---

# Phase D — Domain stabilisation — PARTIAL

## D.1 — Revisions and optimistic concurrency — DONE

### Tâches
- [x] D.1.1 Content-addressed revisions in `kam-state`
- [x] D.1.2 `base_revisions` refuses a batch whose document moved

### Validation
Silent stale-state write: possible → **refused**, asserted by test.

## D.2 — Transactions, rollback, idempotency — DONE

### Tâches
- [x] D.2.1 `kicad_invoke` batches: snapshot, rollback, idempotency key
- [x] D.2.2 Rollback is file-level, not KiCad's undo stack (D12) — complete for
      the S-expression path, **not** an undo for anything applied over IPC to a
      running KiCad; the IPC path needs `BeginCommit`/`EndCommit` (see D.9)

### Validation
2 033 tk/task, 18/18, partial batch on failure: yes → **rolled back**.

## D.3 — Error catalog — DONE (first pass)

### Tâches
- [x] D.3.1 `TransientClass` (`none`/`network`/`timeout`/`lock`/`state`) +
      `retry_after_ms` instead of a bare `retryable` flag
- [x] D.3.2 Stable io codes, locale-independent messages (E9)
- [x] D.3.3 Structured errors are serialised, not `Debug`-formatted (E11)

### Validation
Error-shape tests; E9 and E11 stay closed.

## D.4 — Stable IDs — TODO

### Objectif
Address items by UUID rather than by path + coordinates, so a reference survives
a move and two agents cannot mean different things by the same address.

### Dépendances
D.1 (revisions). The graph already keys on KiCad's own UUIDs, so the extraction
side exists (`konnect-core::graph`).

### Tâches
- [ ] D.4.1 UUID-addressed item handles across the schematic tools
- [ ] D.4.2 Keep the existing path+coordinate forms accepted (INV8)

### Validation
A tool call that names a UUID still resolves after the item moved; targeted tests
plus one probe on a real project.

## D.5 — Snapshots as first-class handles — TODO

### Objectif
`kicad://snapshot/N` beside `kicad://diff/N` and `kicad://evidence/N`.

### Dépendances
D.2 (snapshots exist internally), E.2 (the handle store and its resource route).

### Tâches
- [ ] D.5.1 Issue a handle per snapshot, resolvable over MCP `resources/read`
- [ ] D.5.2 An expired handle is not an unknown one (D16), same discrimination as
      the evidence store

### Validation
Round-trip test over `resources/read`; eviction returns the expired shape.

## D.6 — Error-catalog completeness, retries, recovery policy — TODO

### Dépendances
D.3.

### Tâches
- [ ] D.6.1 Cover the remaining error paths with catalogued codes
- [ ] D.6.2 Retry policy driven by `TransientClass` (`state` means reconcile
      first — a blind retry is useless)
- [ ] D.6.3 `FailureMode` on verdicts (`design` / `environment` / `configuration`
      / `manual_review`) + `MANUAL_STEP_REQUIRED` naming the exact GUI step — a
      broken environment and a broken design must drive opposite agent loops

### Validation
Failure-injection cases resolve to the right class and the right agent loop.

## D.7 — Event journal / deltas — TODO

### Objectif
`changes_since(rev)`. KiCad has no pub/sub, so this is ours to build.

### Dépendances
D.1 (revisions), E.1 (semantic diff).

### Tâches
- [ ] D.7.1 Append-only JSONL run journal with `pre_snapshot_path`,
      `post_snapshot_path`, `rollback_token` per entry
- [ ] D.7.2 `changes_since(rev)` from targeted diffing + file watching
- [ ] D.7.3 Never advertise push notifications over MCP

### Validation
A journal replay reconstructs the same semantic diff the batch reported.

## D.8 — Operating mode, orthogonal to discovery — TODO

### Objectif
Profile controls *discovery*; mode (`READONLY` / `WRITE` / `MANUFACTURING` /
`EXPERIMENTAL`) controls *execution risk*. Loading a toolset must not grant
permission to mutate.

### Dépendances
F.3 (gateway), `kam-state`.

### Tâches
- [ ] D.8.1 Mode held in `kam-state`, enforced at the gateway
- [ ] D.8.2 A `read_only` context refuses *any* write tool, by capability class
      rather than by a listed set

### Validation
A write tool called under `READONLY` is refused before the first mutation (INV4).

## D.9 — Serialised IPC command queue — TODO

### Objectif
KiCad's API server is single-threaded on the UI thread. The lock matters less
than the guarantee that a retry never double-applies.

### Dépendances
D.3 (idempotency keys already exist), PCB path only.

### Tâches
- [ ] D.9.1 `mpsc` + worker task serialising IPC access, own timeout/retry policy
- [ ] D.9.2 `BeginCommit`/`EndCommit` for atomicity on the IPC path (D12's gap)

### Validation
Concurrent callers cannot interleave; a replayed idempotency key applies once.
Blocked in CI by the same GUI-session question as J.3.

---

# Phase E — World model, task state, evidence — DONE

## E.1 — Semantic diff — DONE

### Tâches
- [x] E.1.1 `kam-evidence::diff` matches items by stable key, format-agnostic on
      purpose (D15) — a second document format costs an extractor, not an engine
- [x] E.1.2 KiCAD extractors in `konnect-core::evidence`, keyed on KiCad's UUIDs
- [x] E.1.3 A document is itself an item, so a creation reads `document +3`
      instead of "no design change" (D14)

### Validation
`bench/probes/semantic_diff.yaml` on a real project; 2 158 tk/task.

**Recorded limit:** the diff reports objects, not connectivity. A schematic has
no netlist in the document, and deriving one would re-implement the connectivity
that E7 already shows disagreeing with `kicad-cli`.

## E.2 — Handles, resources, evidence packs — DONE

### Tâches
- [x] E.2.1 Bounded handle store; `kicad://diff/N` and `kicad://evidence/N`
- [x] E.2.2 Resolve over MCP `resources/read`
- [x] E.2.3 An expired handle is distinguished from an unknown one (D16)

### Validation
Round-trip over `resources/read`; +14 tk/task, +0 startup.

## E.3 — Independent verification — DONE

### Tâches
- [x] E.3.1 `kicad_invoke(verify:)` runs `kicad-cli` ERC/DRC (INV1)
- [x] E.3.2 Findings identified by `validator + rule + location` hashed to a
      12-hex id, never by prose (D19); identical findings get an ordinal
- [x] E.3.3 Verdicts cached against the content revision they describe (D18); the
      first verification of a session reports `baseline: "unknown"` rather than
      implying zero
- [x] E.3.4 `verify` is opt-in and a typo is refused (D17) — 7 ms against
      ~1 100 ms per batch is why it is opt-in, and silence is why a misspelling
      cannot be tolerated

### Validation
`bench/probes/validators.yaml`: batch 1 `errors: 4, baseline: unknown`, batch 2
`errors: 2, fixed: 2`.

**Recorded limit:** `verify` only checks documents the batch changed. A read-only
batch gets no verdict; a bare check is still `run_erc`.

## E.4 — Task State Manager — DONE

### Tâches
- [x] E.4.1 `kam-state::task`: objective, constraints, verified facts, failed
      attempts, evidence handles, held outside any model's context
- [x] E.4.2 The ACTIVE TASK anchor is **rendered** from the record on every read,
      never stored (D21) — a cached anchor could disagree with what it describes
- [x] E.4.3 Hard constraints are refused at the bound rather than evicted
- [x] E.4.4 Four tools as a toolset, not gateway verbs (D20) — **0** startup
      tokens, asserted by a stdio test on both halves
- [x] E.4.5 `kicad_invoke(task_id=…)` files revisions, evidence and failures by
      itself; an unknown `task_id` does not fail an already-applied batch

### Validation
2 175 tk/task, 18/18; startup +36, all of it the `task_id` property.

## E.5 — ProjectGraph — DONE

### Tâches
- [x] E.5.1 `kam-graph`: indexed store + filter/neighbour/count query language
- [x] E.5.2 `konnect-core::graph`: KiCAD extractors + a cache keyed on the
      content revision, so a query on an unmoved document rebuilds nothing
- [x] E.5.3 `graph_query` / `graph_neighbors` / `graph_stats` as a toolset — 0
      startup tokens, asserted
- [x] E.5.4 `fields` projection (`compact` default) — unfiltered 525 → 340 tk

### Validation
18/18 at 2 174 tk/task, startup unchanged, 752 tests at the time.

**Recorded limit (D30):** an unfiltered `graph_query` still costs 340 tk against
310 for the plain dump. The graph wins on filtering (109 tk against 310) and on
adjacency, not on serialisation, and its description says so. Shortening the key
was rejected: the key is the address `graph_neighbors` takes.

## E.6 — Context Manager — DONE

### Objectif
`crates/kam-context`: budgets, compaction, retrieval. The attention half already
exists as the anchor (E.4.2); the budget half does not exist at all.

### Dépendances
E.4, and the local-token accounting from H.2 (`Usage`, reasoning split).

### Tâches
- [x] E.6.1 Token budgets per context, measured against real local runs
- [x] E.6.2 Compaction that preserves the objective (already true of the anchor)
      and the verified facts
- [x] E.6.3 Retrieval into the context, budget-aware

### Validation
A compaction cycle on a real session loses no hard constraint and no verified
fact; measured against a local run rather than asserted.

---

# Phase G — Plan IR and deterministic executor — DONE

## G.1 — The IR and the compiler

### Tâches
- [x] G.1.1 `kam-plan`: `ir`, `refs` (`${op.field}`), `compile`, `execute` as a
      state machine the async host drives (no runtime in the crate)
- [x] G.1.2 A plan is refused at compile time, never discovered at step 4 (D22);
      unknown and forward references are separate errors because one is fixed by
      renaming and the other by reordering
- [x] G.1.3 What the compiler deliberately does not check: whether a referenced
      *field* will exist at run time — that stays a step failure with the
      reference named

### Validation
46 tests in `kam-plan`.

## G.2 — The KiCAD operation library

### Tâches
- [x] G.2.1 `call`, `place`, `power`, `label`, `wire`, `connect`, `decouple`
- [x] G.2.2 Every emitted coordinate is snapped before it reaches a tool, which
      makes E6 unreachable inside a plan
- [x] G.2.3 A coordinate must be a number, never a `${ref}` (D23) — the snap
      happens at compile time and the reference resolves at run time
- [x] G.2.4 `decouple` places and wires; it has no opinion on whether the design
      is right, and says so in its own documentation (D24)

### Validation
E2E against real `kicad-cli`: the off-grid input that produced six ERC errors
produces none.

## G.3 — The plan toolset

### Tâches
- [x] G.3.1 `preview_plan` (compile, list, change nothing) and `apply_plan`
- [x] G.3.2 Runs **inside** `kicad_invoke` (D25), inheriting snapshot, rollback,
      `base_revisions`, diff, `verify` and task filing; each inner step is written
      to the call log by hand, since `apply_plan` cannot use the `tool!` macro

### Validation
`bench/plan_cost.py`, same design built both ways, void unless the semantic diff
and the ERC verdict match: divider 2 180 → **1 124** tk (−48.4 %), decoupling bank
2 265 → **882** tk (−61.1 %). Golden suite unchanged, 0 startup tokens.

## G.4 — Plan-owned postconditions

### Tâches
- [x] G.4.1 `erc` / `drc` = no new finding by stable id; `erc_clean` / `drc_clean`
      = zero errors, absolutely (D29 — one name would have been ambiguous)
- [x] G.4.2 An unrecognised validator name is refused in `build()` (INV4)
- [x] G.4.3 A failed postcondition returns `is_error`, so the atomic
      `kicad_invoke` rolls the plan back (D28); the plan never chooses what clean
      means — the verdict is `kicad-cli`'s
- [x] G.4.4 `Postcondition` lives in `konnect-core::evidence::validators`, keeping
      `kam-plan` ignorant of KiCAD (INV2)

### Validation
Measured on the divider: no validators 48 ms, `erc_clean` 1 114 ms, `erc`
2 182 ms; the reply is byte-identical, so a passing postcondition costs no tokens.
E2E run against real `kicad-cli`, not merely written (E12's rule).

---

# Phase H — Local AI runtime — DONE

## H.1 — Backend and shortlist — DONE

### Objectif
Answer the backend question from primary sources and refuse to pick a model by
reputation.

### Tâches
- [x] H.1.1 Backend = OpenAI-compatible HTTP (D31). `vLLM` has no native Windows
      support; `llama.cpp` needs a source build for Blackwell `sm_120`; LM Studio
      wraps `llama.cpp` and exposes tools + `response_format: json_schema`
- [x] H.1.2 Shortlist `Qwen3.5-9B` and `openai/gpt-oss-20b`, both Apache-2.0 with
      documented tool calling; `Qwen3.5-27B` ruled out at ~16.5 GB before KV cache
- [x] H.1.3 Recorded so it is not re-asked: **there is no EDA-specialised
      open-weight model.** The electronics competence stays in the deterministic
      engine and the validators

### Validation
`docs/local-agents.md`; every claim traced to a primary source.

## H.2 — The seam, `crates/kam-llm` — DONE

### Tâches
- [x] H.2.1 `provider::Provider` — one `async fn complete`, object-safe so the
      router can hold `Box<dyn Provider>` and a backend swap is a config change
- [x] H.2.2 Vocabulary shaped like MCP's own tool definitions, so a tool catalogue
      crosses untranslated
- [x] H.2.3 `openai_compat` refuses a non-loopback base URL in `new`; the override
      is a separate named constructor (INV9)
- [x] H.2.4 `usage::Usage` so the local-token KPIs are a field at the call site;
      a backend reporting no counts leaves them at 0 rather than estimating
- [x] H.2.5 `hardware::probe` never panics and never guesses — `nvidia-smi` first,
      a Windows fallback that reports names and **not** VRAM
      (`Win32_VideoController.AdapterRAM` misreports modern cards), a backend
      probe that checks `PATH` and opens no socket
- [x] H.2.6 `ReasoningEffort` on `CompletionRequest`, unset = absent field

### Validation
19 tests in the crate; it ranks, chooses and routes nothing.

## H.3 — The oracle, `bench/model_fit.py` — DONE

### Objectif
Grade a local model's plan by compiling it through the real server, never by
reading it.

### Dépendances
G.3 (`apply_plan`), C.1 (`runner.py`).

### Tâches
- [x] H.3.1 0–3 ladder: 0 not schema-valid JSON, 1 `preview_plan` refuses,
      2 applies but breaks an invariant or the ERC budget, 3 applies clean
- [x] H.3.2 `check_assertion` and `GatewayClient` **imported** from
      `bench/runner.py`, never reimplemented — a harness with its own compiler
      would refuse a plan for a reason it invented
- [x] H.3.3 Prompt = four blocks in fixed order; the schema and operation-library
      blocks are pulled from `kicad_describe(["apply_plan"])` against the running
      server, so a copied schema cannot drift
- [x] H.3.4 Four tasks: `01_divider`, `02_ldo`, `03_decoupling_bank`,
      `04_reference_heavy`
- [x] H.3.5 `--repair N`: a repair round gets its own previous plan and the
      server's **verbatim** refusal, nothing else (D34); the work directory is
      emptied between rounds and the paths stay the same
- [x] H.3.6 `select_best_round` — a repair that lowers the grade is discarded, not
      recorded; ties keep the earlier round; tokens and `llm_calls` stay summed
      over every round performed

### Validation
Selftest, 8 rungs, three of them proving round selection with no model involved.
The stable prefix is byte-identical across all tasks and hint levels — the
property a prefix cache needs.

## H.4 — Make the measurement measure the model, not the harness — DONE

### Objectif
Every run in this phase first measured one of our own defects. Walk the ladder
from the bottom until the residue is the model's.

### Dépendances
H.3.

### Tâches
- [x] H.4.1 Item shapes documented in the operation library (E14)
- [x] H.4.2 A failed plan must not report success, and the oracle must not read a
      failed check as a passing one (E15)
- [x] H.4.3 One placeholder notation, not two a character apart (E16)
- [x] H.4.4 Every scalar carries a type; `create{path,name}` is an operation
      because three independent failure shapes asked for it (E17, D32)
- [x] H.4.5 Accept the reference spellings that have exactly one meaning
      (E18, E19 — the source of INV8)
- [x] H.4.6 A symbol error names candidates (E21)
- [x] H.4.7 `finish_reason` + `reasoning_tokens` recorded; `truncated` is a sixth
      outcome beside the ladder, never a renumbered grade (E20)
- [x] H.4.8 `reasoning_effort` recorded on both sides — a setting the benchmark
      can select and the runtime cannot send makes the measurement unusable (E22)
- [x] H.4.9 `loaded_context_length` recorded per run, after a `high` run graded
      0/60 inside an 8 192-token window it overran 51 times (E23)
- [x] H.4.10 Optional `rollback_policy` spellings that name the absence of a
      choice, and a `schematic` inferred when exactly one candidate exists (E24)
- [x] H.4.11 Cache the symbol library index — a failed lookup cost 7.2 s and
      nothing cached it (E26)

### Validation
Compile rate 6/60 → 54/60 across the run; E24 alone 34/60 → 54/60 at Fisher exact
**p = 0.0001**, `LLM_CALLS_PER_SUCCESSFUL_TASK` 10.0 → 5.5. E26: failed lookup
7.2 s → 43 ms warm / 1 642 ms cold, suite `WALL_CLOCK_P95` back to 890 ms.
`gate.ps1 -Bench`: 815 tests, 18/18, startup unchanged.

**The general result is D37, and it governs the rest of the phase: before
attributing a number to a model, check that the failures are not ours.** The
cheapest place to look is the failure histogram — one refusal string repeated is
ours, a spread across many is the model's.

## H.5 — Measurement runs — DONE

### Objectif
Two models with valid numbers, measured on the same build in the same declared
window, before any threshold is fitted to either.

### Dépendances
H.4 (a run before its defects are fixed measures us, not the model).

### Tâches
- [x] H.5.1 `qwen3.5-9b` measured across the E14–E21 ladder
- [x] H.5.2 `gpt-oss-20b` measured at effort unset / `medium` / `high`, contexts
      8k and 32k, and again on the E24 build
- [x] H.5.3 Re-measure `qwen3.5-9b` on the current build in a **declared 32 768
      window**. Its E24-build run is confounded — its baseline ran in a window
      nothing recorded, and the model's behaviour moved with the window (output
      tokens 122 600 → 460 054). The two are not a before/after pair. Done on the
      E26 build, and `gpt-oss-20b` `medium` re-run beside it so the pair sits on
      one build rather than straddling E24 and E26
- [x] H.5.4 Re-run the `--strict-json` comparison, now that `finish_reason` can
      state the mechanism instead of leaving it inferred (D33 decided the setting
      on outcome counts alone; it stays **off** until a run says otherwise). Run
      on the chosen model on the E26 build: no difference anywhere it could
      matter, `invalid_json` 0/60 both ways, `finish_reason: stop` on all 120
      attempts. It stays off

### Validation
A committed `bench/results/model-fit-*.json` per run carrying
`loaded_context_length` and `reasoning_effort`, compared on the compile rate and
on grade 3 with Fisher exact. Headline table: `docs/benchmark.md` § Model fit.

**Standing results** (detail in `docs/benchmark.md`, do not restate elsewhere):
the chosen local model is `gpt-oss-20b`, `medium`, ctx 32 768, one-shot — 16/60 at
grade 3, 53/60 compiling, 3.75 LLM calls per success on the E27 build (12/60,
54/60 and 5.0 on the E26 build the model choice was made on). `high` is
dominated by `medium` at 2.8× the cost. Deliberation is 92–97 % of local output
tokens, so any budget reading only the answer is wrong by more than 10×. Against
`qwen3.5-9b` measured on the same build in the same window, the 20B reaches grade
3 four times as often (12/60 vs 3/60, p = 0.0246) at half the output tokens; the
compile-rate difference is not significant (54/60 vs 49/60, p = 0.295). The
earlier verdict — no success difference, 9B compiling better — came from a pair
that straddled two builds.

## H.6 — The router — DONE

### Objectif
`NO_LLM | SMALL | MEDIUM | LARGE | ESCALATE`, with thresholds fitted to measured
data rather than to n = 60 noise. The fitting was done (H.6.2) and the data
answered with three tiers: `NO_LLM | LOCAL | ESCALATE`. The middle rungs are not
postponed, they are unmeasurable — no second local model is cheaper per success
and no self-repair round converts a failure.

### Dépendances
H.5.3 — done, and it removes one of the two tiers: on one build the 9B costs more
per success than the 20B (D38), so there is no cheap tier to route *to*. What is
left is D37's point: routing between *no LLM* and an LLM buys the whole call.

### Tâches
- [x] H.6.1 **NO_LLM first** — extend the deterministic operation library wherever
      a measurement shows an LLM call can be removed entirely. The candidate came
      from E26's own residue as required (E25 had already exhausted the refusal
      strings): 16 of 60 attempts failed to apply on a `lib_id` that named
      exactly one installed symbol through a library that does not exist.
      `canonical_lib_id` rewrites those two shapes — invented library, power
      polarity sign — and only when the installed index admits exactly one
      answer. E27: `not_applied` 16/60 → 5/60 (p = 0.0148),
      `LLM_CALLS_PER_SUCCESSFUL_TASK` 5.0 → 3.75
- [x] H.6.2 Tiers and escalation thresholds, each traceable to a measured number.
      The answer the measurements give is **three tiers, not five**:
      `NO_LLM` (H.6.1: `not_applied` 16/60 → 5/60, p = 0.0148),
      one local model (`gpt-oss-20b` at `medium`, D38: 12/60 vs the 9B's 3/60,
      p = 0.0246, at half the output tokens), then `ESCALATE` on the first
      failure of any kind (D35: one repair round converted 0 of 58 and pushed 11
      down the ladder, so no rung sits between the local model and escalation).
      `SMALL`/`MEDIUM`/`LARGE` collapse because no second local model is cheaper
      per success. E27's residue was replayed violation by violation
      (`bench/erc_residue.py`, 39 attempts, all reproducing their graded count)
      and holds no further `NO_LLM` candidate: 139 violations in three classes,
      largest single-rule ceiling 2/60 (p ≈ 0.5)
- [x] H.6.3 Direct mode / Agent mode split at the gateway is an explicit caller
      choice, not a heuristic or server-wide mode. Direct is the existing
      `kicad_describe` / `kicad_invoke` path and never starts an LLM. H.7.1 owns
      a distinct Agent entry point; its `ESCALATE` result returns failure and
      evidence to the caller and never silently contacts an external model
- [x] H.6.4 Evaluate `connect` naming a single-pin symbol while omitting `pin1`.
      Decision: do not build it from E27's 2/60 signal, which is at the noise
      floor. Reopen only if a later run raises it, or alongside a change already
      touching that path
- [x] H.6.5 Fit the geometry contract before adding an agent that composes
      prompts. E28 kept only pin offsets and derived coordinates: 3/20 grade 3,
      all on the decoupling macro, indistinguishable from E27's 7/40 without
      full hints (two-sided p = 1.0) and below `full`'s 9/20 in the pre-declared
      direction (one-sided p = 0.0412; two-sided p = 0.0824). The router must
      retrieve task-specific electrical and Plan IR constraints with geometry;
      the small sample does not identify one sentence as the mechanism

### Validation
`LLM_CALLS_PER_SUCCESSFUL_TASK` and success rate measured with the router on and
off, same tasks, same build, same declared window. For H.6.2 the on/off pair is
E26 vs E27 for the `NO_LLM` boundary; the other two tier decisions are settled by
D38 and D35, each a measured comparison rather than a router setting. H.6.5 is
E28's `geometry` arm against E27's same-model, same-window prompt arms.

## H.7 — Local agent runtime — DONE

### Objectif
Supervisor / schematic / pcb / verification agents over the router.

### Dépendances
H.6, E.6 (budgets), E.4 (task state).

### Tâches
- [x] H.7.1 Supervisor loop driven by task state, not by conversation
- [x] H.7.2 Verification agent whose verdict is `kicad-cli`'s (INV1)
- [x] H.7.3 End-to-end Agent task on the fitted local profile, with no external
      model call and deterministic verification evidence. The `model_divider`
      golden task completed on its first recorded attempt: 8/8 deterministic
      steps, ERC PASS with 0 errors/0 warnings, one local call and zero external
      calls (`agent-e2e-gpt-oss-20b-medium-h7.3b.json`)

### Validation
An end-to-end task completed with no external model call, measured against the
same golden tasks.

---

# Phase I — Custom KiCad gate — TODO (default: NO)

## I.1 — Re-evaluate at KiCad 11

### Objectif
The one blocker that would justify forking KiCad — live schematic IPC — is being
solved upstream for KiCad 11. Default position stays **do not fork** (D3).

### Dépendances
KiCad 11 / `kicad-python` 0.8.0 / `kicad-cli api-server` being available here.

### Tâches
- [ ] I.1.1 Re-run the ground-truth check against KiCad 11 sources
- [ ] I.1.2 Decide: keep the S-expression engine, or move the schematic path to
      IPC. Only a measured blocker that survives KiCad 11 reopens the fork option

### Validation
A written decision with the same evidence standard as D3, or the gate stays shut.

---

# Phase J — Scope expansion — TODO

## J.1 — Close E7 — DONE

### Objectif
Konnect's in-process connectivity analysis disagrees with `kicad-cli` ERC — it
has returned `single_pin_net_count: 0` while `kicad-cli` found six unconnected
pins. The **disclosure** is closed (fifteen tools carry the advisory suffix at the
call site, from the same manifest the matrix renders, INV7); the defect is not.

### Dépendances
None.

### Tâches
- [x] J.1.1 Reproduce the disagreement as a committed probe. Three isolated
      `Device:R` symbols produce 0 in-process single-pin nets versus 6
      `pin_not_connected` findings from KiCad 10.0.3 ERC; the ignored probe
      re-runs both sources when `KICAD_CLI` is set
- [x] J.1.2 Replace label-frequency counting with pin-aware analysis and narrow
      the public claim to pins with no wire/label and no explicit `no_connect`
- [x] J.1.3 Keep the advisory suffix: the reproduced case is now 6 == 6, but
      the implementation remains intentionally `PARTIAL`; the generated matrix
      and suffix/manifest equality test pass

### Validation
The probe's in-process answer equals `kicad-cli`'s on the case that fails today,
and the suffix/manifest equality test still passes.

## J.2 — Raise capability coverage — DONE

### Dépendances
F.4 (the matrix is the instrument).

### Tâches
- [x] J.2.1 Define the coverage comparison target the V1 criterion needs — the
      187 tools the baseline registers at `5cd6454`, frozen in
      `capability::baseline`, scored on both sides by the same scanner; met only
      when strictly ahead *and* nothing the baseline proved is lost. Measured
      **22.6 % → 29.6 %** on a denominator of 186, 0 regressions, re-derived
      from `git archive` in the default gate
- [x] J.2.2 Fill the highest-value gaps — `MISSING` names buses, a standalone
      drill export, IPC-D-356, and the stackup write KiCad 10 declares and does
      not implement
  - [x] J.2.2.1 The two `kicad-cli` gaps: `export_drill` exposes format, units,
        origin, separate PTH/NPTH and the map; `export_netlist(format: "ipc")`
        routes to `pcb export ipcd356` instead of sending an invalid
        `sch export netlist --format`. Both `MISSING` rows are gone. The live
        probe also fixed a real defect: `--output` is a directory, and the two
        existing callers passed `<dir>/drill.drl`, so KiCAD made a *directory*
        of that name — `export_manufacturing_package` reported a file path that
        was not a file
  - [x] J.2.2.2 Buses — `sch_buses` models bus segments, entries and aliases in
        the engine and exposes `add_bus`, `add_bus_entry`, `add_bus_alias`,
        `list_buses`, `expand_bus`. KiCAD's netlist confirms the vector
        expansion (`bus_live`). The probe also found a defect that predates
        buses: KiCAD 10 refuses a label whose `at` has no angle, and the engine
        wrote `(at x y)` — every label type was affected, and the tools only
        escaped it by calling `set_rotation` afterwards
  - [x] J.2.2.3 Stackup write — confirmed and pinned rather than written down:
        KiCad 10's vendored board protos mark `UpdateBoardStackup`
        '**not yet implemented**', and `konnect-ipc`'s
        `stackup_write_is_unimplemented` test fails if that ever changes. Stays
        `GUI_ONLY_NO_API` and out of the denominator
- [x] J.2.3 Prove the tools that have no test that runs. 126 of 202 at the last
      regeneration; 19 `ipc`, 3 `process` and 4 `ipc→sexpr` of those need a live
      KiCAD and stay `gated` until J.3 answers the GUI-session question, so the
      reachable target is the ~100 that run against files, `kicad-cli` or
      nothing. Ordered by size, largest first — each is one commit. **Done: 126
      untested became 26**, and the 26 are the `ipc`/`process` tools waiting on
      J.3 plus the handful whose only honest proof is `gated`
  - [x] J.2.3.1 `nets` and `wires` — `tests/nets_and_wires.rs` builds a circuit
        with the writers and questions every reader about it, through the
        router by name. `get_nets_list` stays `NOT_TESTED`: it is `ipc`, and
        proving it by its own "KiCAD must be running" error would be the claim
        the matrix exists to prevent. `tests/harness/` is the shared rig
  - [x] J.2.3.2 `symbols` and `schematic` — `tests/symbols_and_schematic.rs`.
        `annotate_schematic` and `get_schematic_view` are `cli` and wait for
        J.2.3.7. Three defects found by writing the tests, two fixed here:
        `edit_schematic_component` applied `new_reference` first, which made the
        symbol unfindable and failed every other field in the same call; and a
        rename never reached the `instances` block, so `kicad-cli` kept
        exporting the old designator while the tool reported success (live
        probe). The third, `move_connected` not stretching wires, is recorded
        below rather than hidden
  - [x] J.2.3.3 `config` and `rules` — `tests/config_and_rules.rs`. The
        user-scoped config is a real file in the user's profile, so `APPDATA` /
        `HOME` is redirected into a tempdir under a mutex; the tests assert the
        behaviour that matters at two scopes — the project scope wins the merge,
        and a project rule does not leak into another project
  - [x] J.2.3.4 `review` — `tests/design_review.rs`. Each audit is checked for
        the contrast that makes it worth anything: it finds what it exists to
        find, and stays quiet when there is nothing. The advice itself is never
        asserted. The coverage percentage does not move and should not: all six
        are `HEURISTIC`, so they read `PARTIAL` rather than `SUPPORTED` however
        well tested they are — 78 untested tools became 72
  - [x] J.2.3.5 `footprints` and `libraries` — `tests/libraries_and_footprints.rs`
        builds its own `.pretty` and `.kicad_sym` with `create_footprint` /
        `create_symbol` and then registers, lists, reads and edits *that*, so no
        installed KiCAD is needed. `search_footprints` searches the installed
        libraries by design and stays `#[ignore]`d/`gated` rather than faked
  - [x] J.2.3.6 `labels`, `stackup`, `zones`, `pcb` and `templates` —
        `tests/board_and_labels.rs`. The four `ipc→sexpr` board tools fall back
        to the file engine with no KiCAD listening, so they are testable; the
        `ipc` `refill_zones` is not and waits for J.3. Found and fixed:
        `get_layer_list` searched `find_all("")` for entries whose head is the
        layer id, so it returned an empty list on every board, and `add_layer`
        picked a free id from that same empty set
  - [x] J.2.3.7 the thirteen `cli` tools — `tests/cli_tools.rs` covers each
        twice: a test that runs everywhere pins what the server decides before
        spawning (required arguments, rejected formats, a closed severity set,
        the clamped render size), and an `#[ignore]`d live probe checks the file
        `kicad-cli` actually writes. The live probes use a blank board, not
        `test.kicad_pcb`: that fixture is a KiCad 8 file and KiCad 10 refuses to
        load it
  - [x] J.2.3.8 `sourcing`, `datasheet`, cost/DFM and the last file-engine
        strays — `tests/sourcing_and_manufacturing.rs`. A third party is not a
        test dependency, so what is asserted is what each tool does when it is
        absent: "no database" must never read as "nothing found". The download
        and the `kicad-cli` snapshot are `#[ignore]`d

### Validation
`docs/capability-matrix.md` regenerated; the percentage moves for the right
reason (a test that runs, not a denominator change).

## J.2.4 — Defects the coverage work surfaced

### Objectif
Writing the tests for J.2.3 turned up tools that work differently from what
they promise. The promises are corrected as they are found; these are the ones
whose fix is real work.

### Dépendances
None.

### Tâches
- [x] J.2.4.1 `edit_schematic_component` could not set a field the symbol has no
      property for — `footprint` on a symbol without one came back "'R2' has no
      'Footprint' property", which is the commonest edit after placement. A
      missing property is now created, in the same shape
      `add_component_annotation` writes, and setting it again edits it rather
      than adding a second
- [x] J.2.4.2 `move_connected` drags the wire ends that were on the moved pins,
      matched per pin number so a rotation still lands each wire on its own pin.
      Reports `wire_ends_dragged`; the `PARTIAL` limitation is retired and the
      description says what it now does
- [ ] J.2.4.3 `download_jlcpcb_database` cannot fetch anything: its source,
      `https://bouni.github.io/kicad-jlcpcb-tools/jlcpcb_parts.db`, returns HTTP
      404 (checked 2026-08-17) while the upstream project still exists, so the
      file moved. Declared a `GAP` and pinned by an `#[ignore]`d probe asserting
      the failure is reported and leaves no file behind. Fixing it needs the new
      URL, which is an external lookup this session could not make

### Validation
Each fix lands with the test that proved the defect, and the `PARTIAL` row it
retires disappears from the generated matrix.

## J.3 — PCB E2E without a GUI session — DONE

### Objectif
Open question, and it currently blocks PCB benchmark coverage entirely: does
KiCad 10.0.3 on Windows expose `KICAD_API_SOCKET` reliably enough for unattended
E2E, or does the PCB path need a live GUI session?

### Dépendances
None. Blocks D.9's validation and most of J.2's PCB half.

### Tâches
- [x] J.3.1 Determine it by experiment, not by reading — done, and the answer is
      *both*: an unattended PCB E2E is possible, and a desktop session is still
      required. `KICAD_API_SOCKET` is never handed to a process KiCad did not
      spawn, but it does not have to be: the server listens on the deterministic
      `%LOCALAPPDATA%\Temp\kicad\api.sock`, exposed as a named pipe, with an
      empty `KICAD_API_TOKEN` accepted. `scripts/live-pcb-e2e.ps1` is the
      experiment made repeatable — it starts pcbnew, runs both live suites and
      stops pcbnew, with no window ever touched. Three cold runs, 3/3 tests, exit 0.
- [x] J.3.2 If a GUI session is required, record it as a platform constraint and
      keep the `#[ignore]`d tests reading `gated` in the matrix (D26) — a
      *desktop session* is required (pcbnew has no headless mode) while a human
      is not. Recorded in DEV.md, "Driving the PCB path unattended", with the
      three findings that cost a run each: `api.enable_server` must be true
      before KiCad starts and cannot be set over IPC; PowerShell's `Test-Path`
      is blind to the live pipe; the pipe appears before KiCad will answer on
      it. The tests keep their `#[ignore]` and stay `gated` — D26 is unchanged,
      since the matrix scores what the default suite proves.
- [x] J.3.3 Give the answer a gate: `live-ipc` job in `.github/workflows/e2e-kicad.yml`,
      separate from `e2e` so an IPC failure and a kicad-cli failure stay
      distinguishable. Locally proven; its first CI run is still unobserved.

### Validation
Either an unattended PCB E2E in the gate, or a written constraint with evidence.
Both, as it turned out.

## J.4 — Adapter matrix — DONE

### Objectif
For each capability, which concrete backend actually runs (`ipc` / `cli` /
`sexpr-file`), so fallbacks are observable instead of implicit.

### Dépendances
F.4.

### Tâches
- [x] J.4.1 Generate it from the same manifest the capability matrix uses —
      it is *in* `docs/capability-matrix.md` rather than beside it: an `adapter`
      column on every tool row, an Adapters summary counting them and saying
      which need a running KiCAD, and the same equality test guarding both. A
      separate document would be a second thing to drift.
- [x] J.4.2 Make the fallback observable at run time too, not only in the
      document: an `ipc→sexpr` tool reports which backend answered
      (`"source": "file"` with no KiCAD listening), pinned by
      `board_and_labels.rs`

### Validation
Generated, committed, and equality-tested like the capability matrix.

---

# Phase K — Multi-harness — TODO

## K.1 — Claude Code, Codex, AGY

### Objectif
The handoff must be harness-agnostic: another agent, notably Codex, resumes from
`plan.md`, `progress.md`, Git and the tests without any Claude transcript.

### Dépendances
F.3 (the gateway is the whole external surface).

### Tâches
- [ ] K.1.1 Run the golden suite through each harness
- [ ] K.1.2 Adopt the eval design: `expected_tools`, `allowed_tools`,
      `forbidden_tools`, a `safety` tier checked against the capability registry
      (a `read_only` case rejects *any* write tool), `max_calls`, and an
      instability rate across repeated runs

### Validation
Thresholds: `min_pass_rate 0.95`, `max_safety_violations 0`,
`max_unnecessary_call_rate 0.05`, `max_instability_rate 0.05`.

---

# Phase L — Hardening — TODO

## L.1 — Known debt

### Tâches
- [ ] L.1.1 E10 — `MutexGuard` held across `await` in `sch_components.rs`. A real
      correctness smell, not a lint preference; upstream CI never linted test code
      so it never fired
- [ ] L.1.2 The operation-library anti-drift test checks examples rather than
      parsing signatures. Strengthen it so a signature change cannot pass
- [ ] L.1.3 The persistent symbol index is keyed on directory mtime and entry
      count: a symbol added inside an existing library directory without touching
      its mtime is not seen. Blast radius is a did-you-mean list, never a wrong
      resolution — revisit only if that changes

### Validation
`cargo clippy --workspace --locked --all-targets -- -D warnings` clean.

## L.2 — Failure injection and concurrency

### Tâches
- [ ] L.2.1 Fuzz the S-expression parser/writer round trip
- [ ] L.2.2 Inject failures per `TransientClass` and assert the recovery policy
- [ ] L.2.3 Concurrent user edits: a GUI holding the same file open is outside
      the file-level rollback (D12); prove `base_revisions` catches it

### Validation
Silent corruption stays 0 under injection; no partial batch survives a failure.

---

# Phase M — Final benchmark — TODO

## M.1 — Baseline vs direct mode vs agent mode

### Dépendances
H.6, H.7, K.1.

### Tâches
- [ ] M.1.1 Comparison table across the three modes on the same golden suite
- [ ] M.1.2 Every V1 criterion re-measured, missed ones recorded as missed (INV6)

### Validation
`docs/benchmark.md` final table, reproducible from committed artefacts.
