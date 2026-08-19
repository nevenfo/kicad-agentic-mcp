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
harness (Claude Code / Codex)
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
- [ ] external tokens/task ≤ 2 000 — **~2 204**, missed by ~204 in deliberate
      trades (diff on by default, task filing, verification, and D.5's snapshot
      handle at +18); recorded as missed, never netted off against a win
- [ ] `tools/list` at startup ≤ ~1 000 — **2 034**, missed; only reachable by
      retiring the toolset-loading path, which would break every shipped skill
- [x] retrieval precision @8 ≥ 60 % — **62.0 %** (recall @8 **100 %**) — F.5
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

# Phase F — Compact MCP surface — DONE except F.5.7

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

## F.5 — Retrieval precision — MET (F.5.7 open)

### Objectif
22.4 % precision @8 at the recall needed to succeed. The gateway made each wrong
guess cheap; it did not make the guess right.

**Closed at 62.0 % precision / 100.0 % recall** (b3a1572), measured by
`bench/runner.py --load-mode search`. The suite's success rate in that mode
went 6/7 to 7/7 and external tokens per task 10 446 → 5 205.

### Dépendances
None. Plural stemming was implemented, measured and **rejected** (D6): recall
100 % → 98.2 % at 8 results. Do not re-attempt it.

### Tâches
- [x] F.5.1 Find a retrieval change that raises precision without costing recall
      — four of them, in F.5.4 through F.5.6: IDF weighting, clause splitting,
      a relative cutoff per clause, and one tool per family. Recall went up
      rather than down (97.1 % → 100 %).
- [x] F.5.2 Answer whether a compiled plan moves precision at all. **It does
      not**, and it fails in both directions at once, measured by
      `bench/plan_retrieval.py` at limit 8 on one build against the direct
      shape of `01_sch_divider` read from the task file: not one of four
      goal-stated queries returns `apply_plan`, at rank 8 or at rank 30 —
      "build a resistive voltage divider" returns `audit_power_rails` — because
      the description is about the mechanism of calling and shares no
      vocabulary with a query saying what to build; and for a caller who does
      name the mechanism, retrieval finds it at rank 1-2 while precision falls
      to 11.1 %, since the plan collapses |needed| from 5 to 1 without
      collapsing what search returns. The plan path is entered by prior
      knowledge, never by search, and precision @8 is the wrong instrument for
      it — its measured win is schema tokens (G.3: -48.4 % / -61.1 %)
- [x] F.5.3 Reverse prefix: the one-sided form of what D6 rejected. A query
      term that is the longer, plural side of a corpus term of at least three
      characters scores a fallback +4/+1, and only when nothing stronger
      already matched — purely additive, so D6's failure mode cannot recur,
      and its negative control is now a pinned test. Measured: recall on the
      seventh task 94.3 % → 97.1 %, hist6 recall unchanged at 100 %, precision
      22.5 % → 22.2 %. It buys recall, not precision; F.5.1 stays open.
- [x] F.5.4 The precision ceiling is *how many* results come back, not their
      order: every intent returns a full `limit`, so the union is ~33.8 tools
      per task against ~7.5 needed. A relative score threshold cuts the union
      to 12 and lifts precision to 65 %, but drops recall to 94 %, and every
      tool it drops is one whose intent is **composite** — a single intent
      asking for two or three tools, lexically dominated by one of them
      (`apply_template` behind `search_templates`, `generate_netlist` behind
      `export_netlist`). 8 of the 34 golden intents are composite. Measure
      clause splitting (search each clause of an intent separately, threshold
      per clause, merge by ratio-to-clause-best) against the threshold alone.
      **Measured**: precision 22.5 % → 58.7 % at unchanged 100 % recall on the
      historical perimeter, union 33.8 → 13.5. `apply_template` and
      `generate_netlist` both recovered. It does nothing for the two tools left
      in `07_sch_inspection`, whose intents are single-clause.
- [x] F.5.5 The two stragglers are a vocabulary problem, not a ranking one, and
      the fix is two levers that only work together: `where` → position /
      coordinates / locations recovers `get_schematic_pin_locations`, and a
      description that names the concept in the domain's own words —
      `get_schematic_component` says neither "component" nor "reference"
      anywhere but in its own name — recovers `get_schematic_component`. Two
      other levers were measured and are not shipping (D64). All seven tasks
      now reach 100 % recall, at 54.9 % precision.
- [x] F.5.6 Close the remaining ~5 precision points and port the winning
      configuration into `capability_search`. The cap is no longer
      `07_sch_inspection`: it is dilution on the two widest tasks, where a
      one-word clause spends three slots on near-ties. **The budget per clause
      was a dead end** — measured at zero, its apparent gain being a true
      positive lost rather than noise removed. What paid was one tool per
      family: three spellings of "place a component" spent three of the
      caller's eight slots, and capping the family removed 14 tools on the
      golden suite of which 14 were tools no task needed. Ported with a
      family key that keeps the terms of a name **in order**, because
      `get_component_nets` and `get_net_components` are different tools —
      pinned by a test, since the golden suite never asks for both.
- [ ] F.5.7 Decide whether `apply_plan` / `preview_plan` should name the design
      actions their operation library covers — place, power, label, wire,
      connect, decouple — which is exactly the lever that recovered
      `get_schematic_component` in F.5.5. Opened by F.5.2's finding, and
      deliberately not taken with it: a description that ranks for "place
      resistor symbols" puts `apply_plan` in competition with
      `batch_place_components` on every direct task, and the suite's 62.0 % is
      what would pay for it. Measure both sides on all seven tasks — plan
      reachability from a goal query, and precision on the direct suite —
      before shipping either answer

### Validation
Precision @8 ≥ 60 % with recall @8 ≥ 98 %, measured by the existing retrieval
probe, before/after on the same build. `bench/runner.py --load-mode search` is
the probe of record — `examples/retrieval_probe.rs` explores offline and
asserts it matches production `search()`, but a config only lands once the
server-side run confirms it.

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
  - [x] D.4.1.1 One resolver, not three. `konnect-sexp::command` already holds
        the real index (`ItemId`, `document_items`, both private); it exposes a
        lookup by UUID, and `tools/mod.rs` carries the handle layer beside
        `find_symbol_instance_block`. The three textual
        `content.find(r#"(uuid "…")"#)` sites — `delete_wire`,
        `batch_delete_wire`, `delete_schematic_items` — move onto it without
        changing what they accept or what they protect.
        Done for the two wire sites; the third (`batch_delete`) stays on the
        textual search *because* of the second clause — it has always accepted
        a UUID nested inside the item it deletes (a sheet pin's own), which the
        direct-child index answers `NotFound` for. That acceptance is now a
        test (`batch_delete_accepts_a_uuid_nested_inside_the_deleted_item`), so
        the next reader migrates it only by deciding to drop the input
  - [x] D.4.1.2 `sch_components`: `uuid` accepted wherever `reference` is —
        the nine tools that take one `reference`. Two resolvers, because the
        file has two worlds: `tools::resolve_component` (byte range, for the
        handlers holding the document text) and a thin
        `resolve_component_reference` that returns `reference` without reading
        anything and only pays for a read on the `uuid` path (INV8)
  - [ ] D.4.1.7 A `uuid` that names unit 2 of a multi-unit symbol still lands
        on unit 1 in the seven `cse`/tree handlers: the units share a
        designator, and those handlers redescend by it. Exact-unit addressing
        needs either `cse::SymbolCollection` lookups by uuid (it already has
        `remove_by_uuid`) or moving those handlers onto the byte range.
        `add_component_annotation` and `replace_component` are already exempt
  - [x] D.4.1.3 `sch_hierarchy`: `uuid` accepted wherever `sheet_name` is —
        eight tools, plus `source_uuid` beside `duplicate_sheet`'s
        `source_sheet_name`. Cleaner than D.4.1.2 because `cse` already
        addresses sheets by uuid (`by_uuid`, `by_uuid_mut`, `remove_by_uuid`),
        so no address is ever translated into a name — which matters, since
        sheet names are not unique and a test now pins that
  - [x] D.4.1.4 `sch_wiring` / `sch_buses`: `uuid` accepted wherever a point or
        a segment addresses an item; `extract_junctions` starts carrying the
        uuid it currently drops. Four tools take one
        (`split_wire_at_point`, `delete_schematic_net_label`,
        `rotate_schematic_label`, `delete_no_connect`); `sch_buses` has none to
        take one — every tool there creates or reads. The half that makes the
        address usable ships with it: `list_schematic_labels` publishes uuids,
        `add_no_connect` reports the one it created
  - [x] D.4.1.8 No reading tool listed junctions or no-connects, so a
        no-connect's uuid was only ever published by the call that *created*
        it. Resolved by extending a reader rather than adding a tool:
        `get_schematic_layout` takes `include_junctions` and
        `include_no_connects`, both defaulting to false so the summary costs
        what it always did, and reads both through `cse`, which already models
        them with their identity. Its labels now carry uuids too — wires
        already did, and the asymmetry was a trap
  - [x] D.4.1.5 D.4's own validation — the move test — and the docs.
        `crates/konnect-core/tests/uuid_addressing.rs` runs the loop a caller
        actually runs, against `bench/fixtures/divider.kicad_sch` rather than a
        hand-written fixture: list, edit through the published uuid, resolve it
        again. It covers the rename, which is where the two address forms part
        company, and that an edit rewrites no identity but its own.
        `list_schematic_components` had to start publishing uuids for that loop
        to exist at one call (D82). The model is written down in DEV.md,
        including both known gaps
  - [ ] D.4.1.6 The plural address forms, left out of D.4.1.2 to keep it one
        unit: `batch_get_schematic_pin_locations` and `group_components` take
        `references`, and a list that may mix the two spellings is its own
        decision (a `uuids` array beside it, or entries the resolver reads
        either way) — not a mechanical repeat of the singular case
- [x] D.4.2 Keep the existing path+coordinate forms accepted (INV8) — by
      construction rather than by promise: the historical form is evaluated
      first and reads nothing extra, and every migrated tool has a test that
      runs the same operation both ways and compares the documents

### Validation
A tool call that names a UUID still resolves after the item moved; targeted tests
plus one probe on a real project.

## D.5 — Snapshots as first-class handles — DONE

### Objectif
`kicad://snapshot/N` beside `kicad://diff/N` and `kicad://evidence/N`.

### Dépendances
D.2 (snapshots exist internally), E.2 (the handle store and its resource route).

### Tâches
- [x] D.5.1 Issue a handle per snapshot, resolvable over MCP `resources/read`.
      Emitted only when `Snapshot::capture` succeeds — a handle answering for no
      snapshot would be worse than none — and the batch reply gains
      `snapshot_evidence` without renaming a field, since the gateway tests and
      the bench read that shape. The body is a **manifest**, never the
      before-images: roots, file count, and per file its path relative to its
      root, its revision and its size. Rollback stays internal (D12); this is an
      audit artefact (INV3), not a capability, and a relative path keeps the
      caller's filesystem layout out of something a model reads
- [x] D.5.2 An expired handle is not an unknown one (D16), same discrimination as
      the evidence store — literally the same store: `Entry::uri` already builds
      `{scheme}://{kind}/{id}`, so `put("snapshot", …)` produces
      `kicad://snapshot/N` and `high_water` separates evicted from never-issued
      with no change to `kam-evidence` and none to the `resources/*` routes
- [ ] D.5.3 Reconsider the evidence store's 64-entry capacity if a session ever
      needs deeper history. A capturing batch now stores two artefacts instead
      of one, so those entries span half as many batches. Not a defect and not
      worth changing on speculation: the byte budget is nowhere near binding (a
      400-file snapshot is ~44 KiB of manifest against 4 MiB), and no measured
      workload has wanted more than 32 batches of history

### Validation
Round-trip over `resources/read` and presence in `resources/list`, proven
through the stdio protocol (`crates/konnect/tests/protocol_stdio.rs::
a_captured_snapshot_is_a_resolvable_handle`), not by calling the store directly.
Eviction returns the expired shape **on this kind**, not only on the store's own
`diff` fixture: `an_evicted_snapshot_handle_is_expired_not_unknown` fills a
capacity-2 store with three captures and asserts `evidence_expired` for the
first, `unknown_handle` for an id never issued. A failed capture emits nothing.
Gate green including the benchmark; the +18 tokens/task it costs are recorded in
`docs/benchmark.md` and in the V1 criterion above.

## D.6 — Error-catalog completeness, retries, recovery policy — DONE

### Dépendances
D.3.

### Tâches
- [x] D.6.1 Cover the remaining error paths with catalogued codes, by zone,
      lowering D.6.4's ceiling each time. A big-bang conversion of 150-odd
      hand-written messages would be unreviewable, so it went by zone, ranked
      by D.6.4 rather than estimated: **152 → 2**. The 2 left are not a to-do
      list — one condition (`TaskError::ListFull`) reached from two call sites,
      where no catalogued kind is true and inventing one would be a false
      classification. The ceiling comment says so, so a later reader does not
      read the floor as unfinished work.
      - [x] `sch_hierarchy.rs` — 28/28, ceiling 152 → 124. No new kind was
            needed: `InvalidArgument` ×14, `NotFound` ×12, `FileNotFound` ×5.
            That is the useful finding for the rest of D.6.1 — the work is
            classification, not catalogue design. Message text is never
            reworded, only classified: rewriting prose in bulk would drown the
            review and lose detail written by someone who knew the case
      - [x] `pcb_components.rs` — 20 of 25, ceiling 124 → 104, and the five it
            did **not** take are the point. They were first converted to
            `HandlerError`, the variant documented as the catch-all for what
            has not been migrated, which made the count fall while making the
            contract worse: `HandlerError` asserts `TransientClass::None`, and
            these sites include "KiCAD must be running", where starting KiCAD
            and retrying is precisely the fix. Plain text promised nothing;
            the catch-all promises something false. They are back to plain
            text with the obstacle written at each site — `with_ipc` folds an
            unreachable transport, a board mismatch and a business rejection
            into one `String`, so the type that would decide is already gone.
            The scanner now counts a literal `ToolErrorKind::HandlerError` as
            debt too, so the metric cannot be satisfied by moving text into the
            catch-all; `from_anyhow` is not counted, since it classifies from
            the error chain at runtime and reaches the catch-all only when the
            chain carries nothing better
      - [x] `library.rs` + `sch_wiring.rs` — 22 of 31, ceiling 104 → 82, no new
            kind. The 9 left in plain text all give the same reason, which is
            the finding: a helper stringified the error before the call site
            could classify it (`read_lib_table_checked`,
            `resolve_footprint_path`, `resolve_symbol_lib_path`, and the two
            batch paths whose `errors` string mixes "not found" with "not
            parseable"). One genuine exception: duplicate labels at one
            position in `sch_wiring.rs` — valid input, ambiguous world, neither
            an invalid argument nor a missing item, and a new kind for a single
            site was rightly not invented
      - [x] `meta_tools.rs` — 16 of 18, ceiling 71 → 55, no new kind.
            Fifteen are `InvalidArgument` behind one local helper, so the
            file's argument refusals cannot drift into as many shapes as it
            has call sites. Two of them were D.6.5's signature problem in
            miniature — `agent_u32` and `agent_retrieval` answered
            `Result<_, String>`, and now answer `Result<_, CallToolResult>`
            and classify where the field name is still in scope. The task
            errors classify from `kam_state::TaskError`, not from its
            `Display`. The 2 left are one condition reached twice:
            `TaskError::ListFull` is neither a missing item nor a malformed
            argument — the input is well-formed and the task's own state
            refuses it — so `task_error_kind` returns `Option` and that `None`
            is the statement (D76)
      - [x] `sch_components.rs` — 13/13, ceiling 55 → 41. Five of them were
            one sentence in two spellings, so the classification moved into one
            helper and the prose stayed at each site. The zone also paid a
            *catalogue* debt: **D77** adds `MalformedDocument { path, detail }`
            — the gap between four kinds that each nearly fit (`Io`: the read
            succeeded; `FileNotFound`: the file is there; `InvalidArgument`:
            the call is well-formed; `NotFound`: the addressed item is present
            and the document around it is not usable). Added once six sites
            across four files had converged on the shape, which is the bar
      - [x] `integration.rs` — 12/12, ceiling 41 → 29. Seven of them had no
            true kind either: **D78** adds `UpstreamFailed { service, code,
            detail }`. Nothing in a failed JLCPCB download is the caller's
            fault, the filesystem's or KiCAD's, and `code` separates the two
            failures prose cannot — `unreachable` / `server_error` are
            `Network` (waiting is the recovery), `client_error` /
            `unexpected_response` are `None`. 429 files with the 5xx: it is the
            one 4xx that says "later"
      - [x] the smaller files — `sch_batch.rs`, `pcb_board.rs`,
            `pcb_routing.rs`, `sch_wiring.rs`, `project.rs`, `templates.rs`,
            `sch_analysis.rs`, `config.rs`, `verification.rs`,
            `tools/mod.rs` — 27 sites, ceiling 29 → 2. Three things came out of
            it beyond the count: D77 reached the duplicate-label site zone 3
            had left in prose for want of a kind; the two batch paths D.6.5
            named and did not reach are now typed by *counting* their failure
            causes rather than by re-reading their own joined prose, and a
            batch that deleted nothing is classified by the worst failure it
            collected; and `NotFound` gained `candidates` (serialized only when
            non-empty) so `lib_symbol_not_found_error` could stop paying
            `HandlerError` for one structured field
      - [x] D.6.5 Stop throwing the type away at the boundary — done in two
            commits, ceiling 82 → 77 → 71.
            - The IPC half: four toolsets carried a byte-identical `with_ipc`
              returning `Result<T, String>`. It is now typed and catalogued
              once in `tools/ipc_boundary.rs`, and upstream the markers carry
              what the message used to — `TransportUnreachable` splits into
              `NotConfigured` / `DialFailed`, and `BoardNotOpen` is a marker in
              the anyhow chain rather than a bail. Three kinds, each with the
              transient class that is true of it: `IpcUnavailable` is `Network`
              when a dial failed (starting KiCAD makes the same call work) and
              `None` when nothing is configured (D75's false transient),
              `IpcRejected` is `None`, `BoardNotOpen` is `State`. `from_io`
              falls out of it, and `from_anyhow` delegates to it so the two
              cannot disagree on a code
            - The library half: `read_lib_table_checked` →
              `LibTableUnreadable` (keeps the `io::Error`, not its message),
              `resolve_footprint_path` → `FootprintPathError` (four variants;
              the three `NotFound` ones are told apart by `item_kind` —
              "library uri" is fixed in the environment, "library" by
              `register_footprint_library`, "footprint" by naming another),
              `resolve_symbol_lib_path` → `SymbolLibPathError` instead of
              `Option`, which is the one behavioural change: "not registered"
              and "URI does not expand" were both `None`, so the message had to
              name both and the caller could act on neither
            - One site keeps its prose on purpose:
              `board_footprint_sexp`'s malformed `.kicad_mod` — not IO, not a
              missing item, not a malformed argument. `kind()` returns
              `Option<ToolErrorKind>` and that `None` is the statement
- [x] D.6.2 Retry policy driven by `TransientClass` (`state` means reconcile
      first — a blind retry is useless). `mcp::retry::decide` is the single
      rule; `State` and `None` return no retry *and no wait*, so a call site
      cannot ask the policy for a delay it should not honour. The server
      deliberately does not retry on the caller's behalf — what was missing was
      one named rule, not a loop. Audited every retry site in the crate: the
      only real one is `integration.rs::get_with_backoff` (HTTP), which now
      consults the policy before looping; nothing retried a forbidden class
- [x] D.6.3 `FailureMode` on verdicts (`design` / `environment` /
      `configuration` / `manual_review`) + `MANUAL_STEP_REQUIRED` naming the
      exact GUI step — a broken environment and a broken design must drive
      opposite agent loops. **`COULD_NOT_RUN` can never be `design`**, and not
      by convention: the private constructor for that path has no `Design`
      variant to hand, which is INV1's rule expressed in the type system. An
      agent reading `design` on a broken environment would go repair a
      schematic that has nothing wrong with it. `MANUAL_STEP_REQUIRED` is a
      catalogued `ToolErrorKind::ManualStepRequired { tool, step }` — a code an
      agent loop can match on, not a prefix in prose — and `step` is read from
      the capability's own `Limitation::GuiOnlyNoApi` reason, the same string
      the matrix renders, so the two cannot drift. `manual_review` is declared
      and reserved: nothing in `verify()` has grounds to produce it today, and
      the code says so rather than leaving a reader to guess
- [x] D.6.4 Make the debt visible and non-regressive instead of estimated.
      `tests/error_catalog_debt.rs` scans for plain-text error sites, ranks
      them by file, and fails in **both** directions: a new uncatalogued site
      is refused, and a drop demands the ceiling come down — which is how the
      ceiling went 153 → 152 in this very lot

### Validation
Failure-injection cases resolve to the right class and the right agent loop:
missing validator → `environment`, unsupported document type →
`configuration`, real findings → `design`, `PASS` → no mode at all, plus an
exhaustive test that no `could_not_run` path can convert to `design`. Retry
policy is table-driven over all five `TransientClass` values. Gate green
including the benchmark; gateway tokens 2 204 → 2 207, inside the run-to-run
noise (the `toolsets` mode moved −9 with nothing of its own changed).

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

## D.8 — Operating mode, orthogonal to discovery — DONE

### Objectif
Profile controls *discovery*; mode (`READONLY` / `WRITE` / `MANUFACTURING` /
`EXPERIMENTAL`) controls *execution risk*. Loading a toolset must not grant
permission to mutate.

### Dépendances
F.3 (gateway), `kam-state`.

### Tâches
- [x] D.8.1 Mode held in `kam-state`, enforced at the gateway.
      `kam_state::OperatingMode` (`ReadOnly | Write | Manufacturing |
      Experimental`) + `ModeGuard`, clean-room per INV2: the crate has no idea
      what a tool or an effect is, and the mapping onto "may this call run"
      lives in `konnect-core::capability::mode_allows`, which needs `Effect`.
      Set once at startup from `KONNECT_MODE`; an unrecognised value is a
      startup failure, never a silent fall back to the unrestricted default.
      **D69** — never elevable in-session: the guard's only public mutator is
      `restrict_to`, whose more-restrictive-wins rule makes a less restrictive
      argument a no-op, so no meta-tool exposed to a model can widen what the
      process may do, because no such call exists. Deliberately
      `#[serde(skip)]` on `Config`: a stale `mode: "read-only"` left in a saved
      settings file must not lock a server the operator meant to run writable
- [x] D.8.2 A `read_only` context refuses *any* write tool, by capability class
      rather than by a listed set — the gate reads `capability::tool_effect` /
      `meta_tool_effect`, so a tool added tomorrow is covered by the same
      classification the matrix already renders. Enforced at both execution
      points and nowhere else: `mcp::handler::dispatch_tool` (direct calls and
      meta-tools) and `handle_kicad_invoke` (once per batch entry).
      `kicad_invoke` itself is exempt at the outer dispatch on purpose — its
      `Write` label in `META_TOOL_EFFECTS` is a coverage-audit artefact, not a
      bound on its entries, and enforcing it there would refuse an all-reads
      batch a `ReadOnly` caller is allowed to run. `kicad_agent` is `Write`, so
      the agent loop is never reached under `ReadOnly` and `apply_plan`'s
      internal path needs no second gate. New `ToolErrorKind::
      WriteRefusedByMode`, `TransientClass::None`: retrying the identical call
      can never help
- [ ] D.8.3 Give `MANUFACTURING` / `EXPERIMENTAL` a rule of their own. They
      parse and are carried end to end today but behave exactly like `WRITE`,
      because no MANIFEST entry distinguishes a manufacturing output from an
      ordinary write. The rule and the classification that makes it observable
      have to land together — a mode that claims a restriction it does not
      enforce is the failure mode INV4 exists to prevent

### Validation
A write tool called under `READONLY` is refused before the first mutation (INV4)
— proven end to end through `McpHandler::handle_message`, not by calling a
handler directly, with the work directory's bytes identical before and after the
refusal, and a positive control in `Write` mode asserting the same call *does*
mutate the file (otherwise the refusal would prove nothing).
`crates/konnect-core/tests/mode_gate.rs`, plus table-driven coverage of every
`MANIFEST` and `META_TOOL_EFFECTS` entry in `capability::tests`. Gate green
including the benchmark: gateway 21/21, `MCP_CALLS` median 4, 2 186 external
tokens, retrieval 62.0 % / 100 % — unchanged, which is what a `Write` default
has to mean.

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
- [x] J.2.4.3 `download_jlcpcb_database` could not fetch anything: its source,
      `https://bouni.github.io/kicad-jlcpcb-tools/jlcpcb_parts.db`, returned HTTP
      404 (checked 2026-08-17). The host had not moved — the *shape* of the
      artifact had: upstream publishes each library as one deflate archive split
      into 80 MB chunks, `<name>.db.zip.001` upwards, with a plain-text manifest
      holding the chunk count. Four libraries exist, from `basic-preferred`
      (~2 MB) to `all-parts` (several GB), so `library` selects one and the
      default is the small one. The tool now reads the manifest, concatenates the
      chunks, inflates, proves the result is really the parts database, and only
      then renames it into place; a failure at any step leaves nothing behind.
      The `GAP` is retired
- [x] J.2.4.4 the JLCPCB query tools were querying a schema no published
      database has ever had — `SELECT LCSC, MFR_Part, ... FROM components` with a
      numeric `Price` — while the published file holds an FTS5 `parts` table with
      quoted column names (`"LCSC Part"`, `"MFR.Part"`, `"Library Type"`,
      `"First Category"`), a `Price` that is a tier *string*
      (`1-199:0.018,200-:0.015`) and a text `Stock`. Nothing caught it because the
      only fixture was one that invented the `components` schema, and every other
      probe asserted the absent-database path. All four tools now speak the
      published schema; `price` is the parsed quantity-1 unit price with the raw
      tiers alongside, cheapest-first ordering and `max_price_usd` are applied to
      that parsed price rather than to a string, and `category` filters the two
      columns that actually carry it. `download_jlcpcb_database` records which
      library it fetched, so a search that finds nothing can say whether it
      searched 1 600 parts or the whole catalogue

### Validation
Each fix lands with the test that proved the defect, and the `PARTIAL` row it
retires disappears from the generated matrix. For J.2.4.3/J.2.4.4 the fixture is
the published DDL verbatim and the archive is served from a loopback server, so
the download path and the schema are proved without a third party; one
`#[ignore]`d probe fetches the real `basic-preferred` library and is the check to
run when a download starts failing.

## J.3 — PCB E2E without a GUI session — DONE, and green in CI

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
      distinguishable. The runner-shaped risk — a KiCad profile that has never
      been written — was rehearsed locally by pointing `APPDATA` at an empty
      directory: the script writes the minimal `kicad_common.json`, KiCad
      accepts it, and the suites pass 3/3, exit 0. What no local run can settle
      is whether `windows-latest` gives pcbnew a usable window station.
- [x] J.3.4 Make the gate actually run on a GitHub runner. It does, and it is
      green: run 32026731031, `live-ipc` 3/3, exit 0. `windows-latest` gives
      pcbnew a usable window station after all — the answer to J.3.3's open
      question is yes. Six failures stood between the job existing and the job
      passing, and every one was named by a measurement rather than guessed:
      Actions had never registered the workflows (no push had ever touched
      `.github/workflows/`); KiCad opened its pipe under the 8.3 short name
      (`RUNNER~1`) while the harness waited for the long spelling, and a pipe name
      is a literal with no path resolution; then three modal dialogs, each served
      *before* the API and so indistinguishable from a hung KiCad — no OpenGL 2.1
      on the runner, the first-run wizard's library page, and its Updates &
      Privacy page; and finally a genuine test failure, because
      `live_ipc.kicad_pcb`'s named net had never been committed. The harness now
      matches the pipe by shape and exports the name that exists, writes the three
      library tables and answers the privacy prompts in profiles it creates, asks
      for software rendering there, and — when it does give up — enumerates
      pcbnew's windows and their child controls, which is what turned each of
      those dialogs from a timeout into a sentence

### Validation
Either an unattended PCB E2E in the gate, or a written constraint with evidence.
Both, as it turned out — and the gate is green on a runner, not only locally.

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

## K.1 — Claude Code and Codex (AGY dropped, D70)

### Objectif
The handoff must be harness-agnostic: another agent, notably Codex, resumes from
`plan.md`, `progress.md`, Git and the tests without any Claude transcript.

### Dépendances
F.3 (the gateway is the whole external surface).

### Tâches
- [ ] K.1.1 Run the golden suite through each harness in scope — Claude Code
      and Codex (D70). The runner exists and works (K.1.3); what is missing is
      the measurement itself, and it is gated on K.1.4's single remaining
      external blocker, not on code
- [x] K.1.2 Adopt the eval design: `expected_tools`, `allowed_tools`,
      `forbidden_tools`, a `safety` tier checked against the capability registry
      (a `read_only` case rejects *any* write tool), `max_calls`, and an
      instability rate across repeated runs. The registry had no notion of
      read vs write, so it gained one: `capability::tool_effect` (verb table,
      six handler-verified exceptions, `Write` fail-safe that a test forbids
      from ever being load-bearing), rendered as an `effect` column the bench
      reads without a running server. The audit judges the *executed* path,
      not the task file, or `forbidden_tools` could never fail. `read_only` is
      checked twice: declaratively against the registry, and by a byte
      fingerprint of `$WORK` that catches a registry which lies (D56). New
      task `sch_inspection` — the tier's only exercise, since every other
      golden task authors something
- [x] K.1.3 `bench/harness_runner.py` — the same golden suite, driven by a real
      agent instead of the oracle path. `bench/runner.py` replays each task's
      scripted calls and so measures what the server costs when the reasoning
      is free; this one states the task in plain language
      (`bench/agent_prompts.yaml`, no tool names — naming one would measure
      instruction-following, not retrieval) and scores the result with the
      *same* `audit()`, `fingerprint()`, `check_assertion()` and thresholds,
      imported rather than reimplemented, because that is the only reason the
      two numbers are comparable. Each harness declares an `isolation` level:
      `tools-off` for Claude Code, whose built-ins `--tools ""` genuinely
      removes, and `read-only-sandbox` for the two that cannot remove theirs.
      The report therefore carries two rates — `SUCCESS_RATE`, strict, where
      any off-server call is contamination, comparable only at equal isolation;
      and `DESIGN_PASS_RATE`, which ignores contamination and is comparable
      across harnesses — and prints the isolation level next to them, so the
      two are never silently compared. Proven end-to-end by a real scored
      Claude Code run
- [ ] K.1.4 The codex harness. Its adapter is written and its transcript
      parser proven against real output, but it cannot produce a measurement
      yet for a reason outside the code: the Codex account is at its usage
      limit until 2026-08-20.

      **AGY is out of scope (D70, decided by the user 2026-08-18.)** The
      adapter, `AgyMcpConfigGuard` and `parse_agy_stream` stay in
      `bench/harness_runner.py` — they cost nothing while unused and deleting
      proven code buys nothing — but agy is no longer a harness K.1.1 has to
      measure, and its blocker no longer gates this phase. What was recorded
      about it stands as a finding rather than as work owed: `agy` 1.1.13
      ignores workspace MCP configuration — both `.mcp.json` and the officially
      documented `.agents/mcp_config.json` were tested and left it reporting no
      MCP server at all (antigravity-cli#60), so it solved the task with its own
      file tools and measured nothing of Konnect. Its only working wiring was
      the user's own global `~/.gemini/config/mcp_config.json`, which is why the
      guard existed at all. It was exercised offline against three starting
      states and both refusals, and never against a real agy run.
- [x] K.1.5 The meta-tools had no declared effect, and the first real agentic
      run found it: a `read_only` task was failed for calling
      `find_capabilities` and `load_tools` — the discovery tools an agent
      *must* call — because the matrix covers only MANIFEST tools, so
      `bench/capabilities.py`'s "unknown ⇒ write" fail-safe caught them. The
      fail-safe was right to exist and wrong to be the answer, which is exactly
      what D58 forbids. All twelve gateway tools now carry a hand-decided
      effect (`META_TOOL_EFFECTS`), rendered as its own section of the
      generated matrix. `effect` keeps its D56 meaning — can this call mutate
      the *project on disk* — so `load_tools`, `load_toolset` and
      `unload_toolset` are `read` even though they do change which tools
      `tools/list` exposes; that distinction is written down where the table
      is, since it is the kind of nuance a later reader would otherwise invert.
      `kicad_invoke` is `write` (it carries arbitrary batches) with no effect
      on any run, because D57 already unwraps it to its inner calls before the
      audit. The exhaustiveness guarantee is structural rather than a copied
      list: a `define_meta_tools!` macro generates the dispatch `match` and
      `META_TOOL_NAMES` from one invocation, so a new meta-tool without an
      effect fails a test naming it
Thresholds: `min_pass_rate 0.95`, `max_safety_violations 0`,
`max_unnecessary_call_rate 0.05`, `max_instability_rate 0.05`. Enforced by
`bench/runner.py --enforce`, which exits non-zero on any of them; met by
`--load-mode gateway --repeat 3` and `--load-mode tools --repeat 2`
(`search` is exempt — its failure rate measures retrieval, not the server).

`bench/harness_runner.py` applies the same four thresholds to the agentic runs,
plus `off_server_calls == 0` — enforced at `tools-off` isolation, skipped with a
stated reason at `read-only-sandbox`, where the harness cannot remove its own
tools. K.1.1 is met when every harness that can be measured has been, and every
one that cannot has its reason recorded here rather than a missing number.

---

# Phase L — Hardening — TODO

## L.1 — Known debt

### Tâches
- [x] L.1.1 E10 — `MutexGuard` held across `await` in `sch_components.rs`. A real
      correctness smell, not a lint preference; upstream CI never linted test code
      so it never fired. Fixed by moving the three env-var test locks
      (`SYMBOL_DIR_ENV`, `FOOTPRINT_DIR_ENV`, `CONFIG_HOME`) to `tokio::sync::Mutex`,
      whose guard is `Send`, so the test futures survive a move to
      `multi_thread` and no longer poison. `FOOTPRINT_DIR_ENV` had the same
      defect hidden inside a wrapper struct clippy could not see through
- [x] L.1.2 The operation-library anti-drift test checks examples rather than
      parsing signatures. Strengthen it so a signature change cannot pass. Done:
      the `*_SIGNATURE` DSL is now parsed into a flat field list (nested
      objects, arrays of objects, and both union shapes), and the minimal
      examples are checked against it from both directions — nothing an example
      uses may be undocumented, and every field documented required and outside
      a union must make `expand` fail, naming that field, when removed. Both
      directions verified by negative control
- [x] L.1.3 The persistent symbol index is keyed on directory mtime and entry
      count: a symbol added inside an existing library directory without touching
      its mtime is not seen. Blast radius is a did-you-mean list, never a wrong
      resolution — revisit only if that changes. **It changed**: H.6.1's
      `canonical_lib_id` reads that same index and rewrites a `lib_id` when it
      finds exactly one owner, so a stale index turns a refusal into a silent
      pick. The fingerprint now includes each library entry's own mtime (D51),
      at a measured 3.7 ms for the 223 libraries of a stock KiCad 10 install
- [x] L.1.4 The gate is a gate nowhere: `ci.yml` triggers on `main` only, and
      this fork's default *and* working branch is `agentic/main`, so no push has
      ever run it. Its `clippy` step also omits `--all-targets` (which is why
      L.1.1 never fired in CI) and its `fmt` step would fail today on ~38
      pre-existing rustfmt hunks in 15 files. Trigger it on the branch that
      exists, lint all targets, and clear the drift so `fmt` means something
- [x] L.1.5 The first CI run L.1.4 made possible found one non-hermetic test:
      `kicad_invoke_reports_what_it_changed_in_design_terms` placed `Device:R`
      through whatever KiCAD the machine had, so it passed on a developer box
      and failed on every runner. It now spawns the server with
      `KICAD10_SYMBOL_DIR` pointed at a fixture library — env on the child, not
      on the test process, which runs these in parallel threads — and asserts on
      a marker property only the fixture carries, so the assertion is the same
      one on both
- [x] L.1.6 Same run, second finding: `the_frozen_baseline_measurement_still_holds`
      re-derives the frozen baseline from the tree at `BASELINE_COMMIT`, which a
      depth-1 checkout does not contain. The `check` job now checks out the full
      history. The test refused to pass vacuously, which is why it was the thing
      that reported it

### Validation
`cargo clippy --workspace --locked --all-targets -- -D warnings` clean, and
`.\gate.ps1` green end to end — including the `fmt` step, which L.1.4 unblocks.

## L.2 — Failure injection and concurrency

### Tâches
- [x] L.2.1 Fuzz the S-expression parser/writer round trip. The round trip that
      exists is not parse → serialize → parse — this crate has no tree
      serialiser — but parse → locate a block by byte offset → replace that
      text → reparse, which is how every write in the project happens.
      `tests/proptest_writer.rs` fuzzes that path against a generator that
      knows its own ground truth (block counts come from the generated tree,
      never from a second call to the code under test): empty-edit identity,
      excise/reinsert byte-for-byte, replacement still reparses, every
      `find_block_starts` offset is a char boundary landing on `(tag`, deleting
      a tag's direct children drops its count by exactly the number removed,
      and none of the six finders panics on arbitrary content at an arbitrary
      offset. **No production bug found**, including the one this task was
      aimed at: `apply_edits` uses `String::replace_range`, which panics on a
      non-boundary offset, but the finders only ever match ASCII bytes
      (`(`, `)`, `"`, `\`), and no UTF-8 continuation byte can equal one — so
      every offset they return is a valid boundary by construction, on CJK and
      emoji alike. The properties still earn their place: the negative control
      (removing string-awareness from `find_block_starts`) is caught and
      shrunk to `(kicad_sch (symbol "(label 😀)"))`
- [x] L.2.2 Inject failures per `TransientClass` and assert the recovery policy.
      What already existed proved *which class* an error carries; what was
      missing is that the policy each class advertises actually holds. Now
      pinned: `state` fails identically on a blind retry — twice, so nothing
      teaches a loop that hammering eventually gets through — carries no
      `retry_after_ms`, touches no file, and yields only to reconciliation;
      `lock` is provoked by a genuine race (two tasks, one `ToolContext`, one
      `operation_id`) and the loser gets `retry_after_ms 250` while the winner
      finishes intact and the replay is memoized, not re-applied; `none`
      repeats byte-identically with no retry hint. A real `io::Error` is also
      shown to survive `SexpError` → `anyhow` and come out as `Io { code }`
      rather than an opaque `HandlerError` — the failure mode that would make
      a recovery loop abandon a call that would have worked. `Timeout` and
      `Network` are **not** covered: nothing here provokes them without a live
      KiCAD IPC session (phase I gated), and they were left to the `#[ignore]`
      live suites (D26) rather than simulated
- [x] L.2.3 Concurrent user edits: a GUI holding the same file open is outside
      the file-level rollback (D12). The premise as written was wrong and the
      task corrected it: `base_revisions` does **not** catch this. It is
      checked once, before the batch starts, so it only rejects a *stale
      start* — a GUI save landing mid-batch is invisible to it. What actually
      catches it is the per-write compare-and-swap in
      `write_atomic_if_unchanged`: every schematic tool does `read_consistent`
      → compute → conditional write, and a foreign save between those two
      steps makes `expected` stale. `tests/concurrent_gui_edit.rs` drives a
      real racing writer (a plain `std::fs::write` thread that never opens the
      advisory lock — the honest stand-in for "applications that do not honor
      the lock, including KiCad itself") against a 300-call batch and pins:
      the conflict surfaces as `error_kind: conflict` / `transient: state`,
      not as an opaque `handler_error`; the final file is always one coherent
      version — the rolled-back original or the GUI's own last save, never a
      torn mix and never the batch's edits on top of a discarded GUI save; and
      the identical call succeeds on replay once the world settles, which is
      what makes `state` the honest class. That path needed a production fix:
      `ToolErrorKind::from_anyhow` walked the cause chain for `io::Error`
      only, and a write conflict carries none, so it decayed to
      `HandlerError`/`None` — a client told "deterministic, fix your request"
      for the one error meaning "re-read and recompute". New
      `ToolErrorKind::Conflict { path }`, classified `State`. Separately, the
      *literal* held-handle case is pinned on Windows in `writer.rs`: a handle
      opened without `FILE_SHARE_DELETE` blocks the publishing rename, and the
      document survives intact with no scratch left behind — but it arrives as
      `permission_denied`, indistinguishable from a genuine ACL failure, so it
      is deliberately **not** reclassified as `Lock`; see L.2.5
- [x] L.2.4 The race in L.2.2 had to be driven in-process, because
      `run_stdio` (`crates/konnect/src/transport/stdio.rs`) reads one JSON-RPC
      line and awaits it to completion before reading the next: over stdio a
      single process can never have two `kicad_invoke` calls in flight, and the
      idempotency ledger is in-memory per `ToolContext`, so two processes would
      not race either. `OperationInFlight` is therefore only reachable through
      the HTTP transport, whose `axum::serve` does handle requests
      concurrently. Decide whether that is the intent — if the ledger is meant
      to protect across processes it needs to outlive one, and if it is not,
      say so where a reader of `OperationInFlight` will find it.
      **Settled: it is the intent, and it stays in memory.** The window the
      ledger protects is a client retrying a call it just made, measured in
      seconds; a journal on disk would buy durability across a restart at the
      cost of a staleness failure mode of its own. What matters is that the
      cross-process case is not left uncovered — it is covered by a *different*
      mechanism, keyed on the document's content rather than the caller's
      identity: `base_revisions` for a stale start, and the per-write
      compare-and-swap (L.2.3) for a change landing mid-batch. Neither cares
      which process, or which GUI, moved the file. That reasoning now sits on
      the `OperationInFlight` variant itself, where someone debugging one will
      read it, and
      `protocol_stdio.rs::an_operation_id_does_not_cross_a_process_boundary_but_base_revisions_does`
      pins both halves so the prose cannot rot: a second process sees the same
      key as fresh (no `replayed`, no `operation_in_flight`), and a third
      presenting the creation-time revision is refused `stale_revision` having
      run nothing
- [x] L.2.5 A held file handle and a denied ACL both arrive as
      `permission_denied` (found by L.2.3, pinned on Windows in
      `writer.rs::a_handle_held_without_delete_sharing_blocks_the_publishing_rename`).
      One is worth waiting for — the GUI closes the document and the same write
      works — and the other never is, but `ToolErrorKind::Io { code:
      "permission_denied" }` gives a recovery loop no way to tell them apart.
      **They can be separated at the source, and now are.** When
      `write_atomic_unlocked`'s rename fails with `PermissionDenied`,
      `refine_rename_failure` re-opens the *target* asking only for `DELETE` —
      the access the rename itself needs. A handle held without
      `FILE_SHARE_DELETE` answers `ERROR_SHARING_VIOLATION`, which no ACL
      denial produces; an ACL that forbids us answers `ERROR_ACCESS_DENIED` a
      second time. Only the sharing violation is relabelled, as `ResourceBusy`
      → `Io { code: "resource_busy" }` → `TransientClass::Lock`, so the caller
      is told to wait without `permission_denied` as a whole being
      reclassified. A probe that succeeds leaves the original error alone: the
      rename was refused for a reason this cannot name, and guessing beats
      nothing only when the guess is right. `"resource_busy"` was already in
      `transient_class`'s `Lock` arm but unreachable — `io_code` never emitted
      it — so this also closes a dead branch. Both halves are pinned on real
      OS behaviour, not simulation: the held-handle test opens with a
      restrictive share mode, and the ACL test writes a genuine deny ACE with
      `icacls` (needing `DE` on the file *and* `DC` on the parent, since
      `FILE_DELETE_CHILD` otherwise grants the deletion regardless of the
      file's own ACL) and restores it from a `Drop` guard
- [x] L.2.6 L.2.3's GUI stand-in could tear the file it raced. Found by CI, not
      locally: every job had been dying at `arduino/setup-protoc` on a
      `codeload` 429 since before L.2 closed, so `agentic/main` had never
      actually run this test on ubuntu or macos. Once the 429 cleared, the test
      failed on all three OSes with `Parse error at byte 0: Unexpected end of
      input` classified `handler_error`, where the assertion demands
      `conflict`. The defect was in the test: `spawn_gui_writer` saved with a
      bare `std::fs::write`, which truncates before it writes, so the batch's
      `read_consistent` could land on a zero-byte file and the run measured a
      torn read instead of the per-write compare-and-swap it claims to prove.
      The stand-in now writes a sibling temp file and renames it over the
      target — a *truer* GUI, not a weaker one, since KiCad saves atomically,
      and the rename still lands mid-batch and still makes `expected` stale,
      which is the only thing the test needs from it. A rename Windows refuses
      because the target is open is uncounted and non-fatal; `gui_writes > 0`
      still holds. The lesson is about the gate, not the race: a green
      `gate.ps1` on one OS is not a green CI, and a CI red for an
      infrastructure reason hides the failures underneath it
- [x] L.2.7 L.2.5's ACL test is a false red in an elevated shell. An elevated
      process holds `SeBackupPrivilege` and `SeRestorePrivilege`, which bypass
      the deny ACE the test installs: the rename succeeds, `unwrap_err()`
      panics, and a developer sees a failure on a clean tree that CI — which
      runs unelevated — never reproduces. The test now detects high or system
      integrity by SID (`S-1-16-12288` / `S-1-16-16384`, language-independent)
      and skips with a reason, the same shape as its existing early return
      when `USERNAME` is unset. It is skipped where it cannot prove anything,
      not weakened where it can
- [x] L.2.8 `board_design_rules_round_trip_through_the_file` was flaky in CI,
      and what it raced over was the test binary's environment, not the write
      path. Observed once, in run 32103900156, on **macos-latest alone**:
      `windows-latest` in that run was *cancelled* by the matrix fail-fast, not
      failed, and reading it as a two-OS failure sent the first diagnosis
      looking for something the two share. The failure is
      `'set_design_rules' failed: IO error: Invalid argument (os error 22)`,
      EINVAL out of the write, on a commit that touched only `plan.md` and
      `progress.md`. `redirected_user_config` repoints `HOME`/`APPDATA` at a
      `TempDir` under a mutex only the config tests take; a design-rules test
      takes no guard, and its write still resolves its document lock through
      `dirs::data_local_dir()` — on macOS `$HOME/Library/Application Support`,
      the same `konnect/` subtree the redirected config lives in. Its lock file
      was therefore created inside another test's `TempDir` and deleted out
      from under it when that test returned. Windows never saw this because the
      same lookup reads `LOCALAPPDATA`, which nothing here redirects; the
      fixture copy was never the sharing to blame. The harness now points
      `KONNECT_STATE_DIR` at `<CARGO_TARGET_TMPDIR>/konnect-state` once per
      test binary, so the lock path stops reading `HOME` at all
- [x] L.2.8.1 Which syscall returned EINVAL is unknowable for that run, because
      `SexpError::Io` carried neither the operation nor the path — one bare
      `Invalid argument` for half a dozen files `write_atomic` touches. Every
      IO error on the write and lock paths now names both, with `ErrorKind`
      carried through unchanged so `refine_rename_failure` (L.2.5) and every
      `.kind()` match downstream still see exactly what they saw

### Validation
Silent corruption stays 0 under injection; no partial batch survives a failure.
L.2.6 additionally: 20 consecutive release runs of the test locally, then CI
green on ubuntu, macos and windows (run 32060085312) — the first green
`agentic/main` of the phase.
L.2.8: `gate.ps1` green locally, then CI green on all three OSes with no job
cancelled (run 32108806941), macOS included. A rare flake failing to reappear
once proves nothing on its own; what the run adds is that the fix costs no
other test. The proof of the fix itself is structural — the lock path no longer
reads a variable another test rewrites, observable as the locks now living
under `target/tmp/konnect-state/locks/`.

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
