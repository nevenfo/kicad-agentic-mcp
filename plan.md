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

- [x] `SUCCESS_RATE` ≥ baseline — **35/35 against the baseline's 35/35** (M.1,
      seven tasks × 5, both servers measured back to back on 2026-08-24). Equal,
      which is what `≥` asks; a scripted route succeeds by construction on both
      sides, and the fork's margin is in what the route costs and in what
      happens when nobody scripts it
- [x] median `MCP_CALLS` per task ≤ 5 — **4**
- [ ] `WALL_CLOCK_P50` ≤ baseline — **86 ms against 77 ms, missed by 9 ms**
      (M.1). The recorded pair was 65 against 70. The mechanism is per-task and
      visible: the fork loses where it guarantees something — `recovery`
      +109 ms, the task built to exercise the transaction journal, the snapshot
      manifest and the evidence store — and wins where there is nothing to
      guarantee (`sch_inspection`, 14 → 6 ms). The direction is stable and the
      magnitude is not: an earlier `--repeat 3` pair the same day reads 69
      against 87, the fork slower by 18 rather than by 9. Recorded as missed
      (INV6); nothing was tuned to recover it
- [x] silent corruption / silent stale-state write = **0** — refused by `base_revisions`
- [x] mutations without an audit record = **0**
- [ ] external tokens/task ≤ 2 000 — **2 249**, missed by 249 in deliberate
      trades (diff on by default, task filing, verification, and D.5's snapshot
      handle at +18); recorded as missed, never netted off against a win.
      M.1 re-measured it and found +68 since `k12-gateway.json` (2026-08-17):
      F.5.7's descriptions and K.2's annotations both ride inside
      `kicad_describe` results, so both are paid per task and not only on the
      startup surface. K.2's +342 at startup was measured when it landed; this
      is its per-task share, measured now
- [ ] `tools/list` at startup ≤ ~1 000 — **2 831**, missed; only reachable by
      retiring the toolset-loading path, which would break every shipped skill.
      Re-measured at `91b9911` before touching anything: the recorded 2 034 was
      stale — the surface had already drifted to **2 489** as descriptions grew.
      K.2's annotations then cost **+342** (2 489 → 2 831, +13.7 %; full catalog
      29 399 → 33 183). Paid deliberately: a surface a headless client will not
      call is worth no tokens at all. The cheaper shape was measured, not
      assumed — dropping `openWorldHint` from read tools saves 78 of the 342
      and was rejected, because omitting it asserts the MCP default of
      *open world* about every read tool to save 2.8 %
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

# Phase F — Compact MCP surface — DONE

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

## F.5 — Retrieval precision — MET

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
- [x] F.5.7 Decide whether `apply_plan` / `preview_plan` should name the design
      actions their operation library covers — place, power, label, wire,
      connect, decouple — which is exactly the lever that recovered
      `get_schematic_component` in F.5.5. Opened by F.5.2's finding, and
      deliberately not taken with it: a description that ranks for "place
      resistor symbols" puts `apply_plan` in competition with
      `batch_place_components` on every direct task, and the suite's 62.0 % is
      what would pay for it. Measure both sides on all seven tasks — plan
      reachability from a goal query, and precision on the direct suite —
      before shipping either answer.
      **Answer: no — the description names the *goal*, not the actions**, and
      the two candidates were measured against the same baseline on one build
      (62.0 % precision / 100.0 % recall, plan-by-goal 0 of 4 queries).
      *Naming the actions* ("create a project, place component symbols, add
      power and ground symbols for supply rails, label nets, draw wires,
      connect pins, decouple a rail with capacitors") costs 62.0 % → 60.3 %
      precision on the direct suite and, more legibly, **+140 catalog tokens on
      3 of the 7 tasks** — `sch_divider`, `manufacturing_exports` and
      `recovery` each load `apply_plan`'s schema for a task that never needs
      it. *Naming the goal* ("Use it to build a whole schematic design in one
      call") buys the identical reachability — `apply_plan` at rank 2 on
      "build a resistive voltage divider", 0 of 4 goal queries → 1 of 4 — at
      62.0 % precision unchanged and not one catalog token moved on any task.
      The three goal queries still missing are the long ones: clause splitting
      decides per clause, and `apply_plan` is the best answer to none of
      "supply rails", "a wire between them", "a labelled output". So D68 stands
      as measured — the plan path is not *retrievable* from a stated design
      goal — but its edge is now one query wide instead of zero. Shipped: the
      goal sentence, on both tools

### Validation
Precision @8 ≥ 60 % with recall @8 ≥ 98 %, measured by the existing retrieval
probe, before/after on the same build. `bench/runner.py --load-mode search` is
the probe of record — `examples/retrieval_probe.rs` explores offline and
asserts it matches production `search()`, but a config only lands once the
server-side run confirms it.

---

# Phase D — Domain stabilisation — DONE except D.5.3, which is conditional by design

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

## D.4 — Stable IDs — DONE

### Objectif
Address items by UUID rather than by path + coordinates, so a reference survives
a move and two agents cannot mean different things by the same address.

### Dépendances
D.1 (revisions). The graph already keys on KiCad's own UUIDs, so the extraction
side exists (`konnect-core::graph`).

### Tâches
- [x] D.4.1 UUID-addressed item handles across the schematic tools
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
  - [x] D.4.1.7 A `uuid` that names unit 2 of a multi-unit symbol now edits
        unit 2. `resolve_component_reference` became
        `resolve_component_target`, which resolves to the symbol's *position* —
        in the loaded schematic, among the parsed instances, or as a byte range
        — and nothing redescends by designator afterwards (D81). No
        `by_uuid`/`by_uuid_mut` was added to `cse`: position is what the
        handlers need, and only `remove_at` was missing
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
  - [x] D.4.1.6 The plural address forms, left out of D.4.1.2 to keep it one
        unit. Seven tools, and the shape follows each tool's own rather than
        one uniform rule: an address that is an array of strings gets a
        parallel `uuids` array (`batch_get_schematic_pin_locations`,
        `group_components`, `bulk_move_schematic_components`,
        `batch_delete_schematic_components`), and an address that is already an
        object entry gets a `uuid` field inside it
        (`batch_edit_schematic_components`, `batch_rotate_labels`,
        `batch_delete_no_connect`). Both arrays together are the union, an item
        named twice acted on once. `move_labels_by_offset` is deliberately out:
        its `net` selects every label on a net — a selector, not an address
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

## D.7 — Event journal / deltas — DONE

### Objectif
`changes_since(rev)`. KiCad has no pub/sub, so this is ours to build.

### Dépendances
D.1 (revisions), E.1 (semantic diff).

### Tâches
- [x] D.7.1 Append-only JSONL run journal with `pre_snapshot_path`,
      `post_snapshot_path`, `rollback_token` per entry. Paths are relative to
      the journal directory and only the files the batch *changed* are imaged —
      the two together are what keep the cost proportional to the change and
      the file free of the caller's filesystem layout (D72). Nothing from the
      journal enters an MCP reply: a `rollback_token` a client could read would
      be an address no tool accepts (D82 read the other way round), and D12
      keeps rollback inside the batch
- [x] D.7.2 `changes_since(rev)` as a **meta-tool**, not a MANIFEST tool: it
      answers about the server's own record, and a domain tool would move
      `CAPABILITY_COVERAGE`'s frozen denominator (D44). `rev` is a revision
      token `kicad_invoke` already publishes (D82). Three answers it must tell
      apart — the document is at `rev` still; it moved and the journal says
      which batches moved it; it moved and *we* did not write it, which is the
      foreign-edit case the revision comparison detects on the spot
- [x] D.7.3 Never advertise push notifications over MCP, pinned by a test
      rather than by nobody having added one: `resources` already ships
      `subscribe: false` / `listChanged: false` (`mcp/server.rs`), and
      `tools.listChanged` stays `true` because it is real and fires — it
      describes *session* state, which is not a disk mutation (D60)

### Validation
A journal replay reconstructs the same semantic diff the batch reported.

**Deliberate substitution, recorded rather than silent:** D.7.2 says "targeted
diffing + file watching" and ships the diffing without the watcher. A watcher
is a background daemon that has to survive a restart to be worth anything, and
the one question it would answer — has this document moved since `rev` — is
already answered on demand by D.1's content-addressed revision, with no state
to keep in sync. D.7.3 is the other half of the argument: a watcher whose
findings may never be pushed has no consumer but the poll that exists anyway.

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
- [x] D.8.3 `MANUFACTURING` is a **design freeze**, and `EXPERIMENTAL` is an
      alias of `WRITE` that says so (user decision, 2026-08-20). The rule is
      implementable because a fabrication output *does* write to disk — the
      distinction is not whether a call writes but *what* it writes, so a
      `WriteTarget { DesignDocument, Derived }` sits orthogonal to `Effect`
      rather than replacing it. `Manufacturing` refuses a `Write` whose target
      is a source document and allows a derived one, which makes the scale
      linear at last: `ReadOnly < Manufacturing < Write`, with `restrict_to`
      still unable to elevate (D69). The fail-safe is `DesignDocument`, the
      mirror of D58's `Write` fail-safe: a tool added tomorrow is refused under
      `Manufacturing` rather than allowed by accident. `EXPERIMENTAL` is given
      no rule on purpose — no use case for one exists anywhere in the repo, and
      inventing one to justify a name reads the rule backwards — so it is
      documented as a deliberate alias and pinned by a test rather than left as
      a promise (D89)

### Validation
D.8.3 is proved in the same shape, through `McpHandler::handle_message`: under
`MANUFACTURING` a design-document write is refused with the working directory's
bytes identical before and after, a `Derived` write succeeds (the positive
control, without which the refusal proves nothing), a `kicad_invoke` entry is
gated exactly like a direct call, and `EXPERIMENTAL` runs the very tool
`MANUFACTURING` refused. Table-driven in `capability::tests`: no `export_*` tool
is a `DesignDocument` write and no name in the derived list is dead. The
matrix's `effect` column is deliberately untouched — `bench/capabilities.py`
keeps only exact `read`/`write` values, so the new fact went into a column of
its own; the bench's table is unchanged at 215 entries.

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

## D.9 — Serialised IPC command queue — DONE

### Objectif
KiCad's API server is single-threaded on the UI thread. The lock matters less
than the guarantee that a retry never double-applies.

### Dépendances
D.3 (idempotency keys already exist), PCB path only. No longer gated on J.3:
J.3.4 answered the GUI-session question and the `live-ipc` job is green on a
GitHub runner, so this lot's validation has somewhere to run.

### Tâches
- [x] D.9.1 A FIFO queue per IPC address, each behind its own worker thread
      (`tools/ipc_queue.rs`), with `with_ipc` submitting **synchronously** so the
      order is the order callers were invoked and not the order their futures
      were polled. Neither a retry nor a timeout was added, and both refusals are
      the decision rather than an omission (D87). The concrete failure it closes
      is `place_footprint`'s four-command read-modify-write, whose "does this
      reference already exist" check two concurrent callers could both pass
- [x] D.9.2 Atomicity on the IPC path, by two remedies rather than one (D88).
      Of the three multi-mutation sites, only `modify_track` needed a commit —
      `replace_track` wraps its `delete_track` + create in `run_commit`, because
      a delete and a create cannot be merged and a failing second half destroyed
      a track and put nothing back. The L-bend and the differential pair are
      mutations of the *same* nature, so `add_tracks(&[TrackSpec])` sends them as
      one `CreateItems` — atomic by construction, one `get_nets` for the batch
      instead of one per track, and an unknown net name fails before anything is
      sent. Every other IPC site sends exactly one mutation and was deliberately
      left alone

### Validation
Concurrent callers cannot interleave: proved by `ipc_queue`'s own tests — eight
concurrent jobs observe a maximum concurrency of 1, jobs run in submission
order, a panicking job does not wedge the queue, and two distinct addresses do
reach 2. A replayed idempotency key applying once stays D.3's ledger's job;
D.9.1 deliberately adds no second mechanism for it (D87). Atomicity is proved
against the mock KiCAD by the sequence of commands actually sent:
`BeginCommit → DeleteItems → CreateItems → EndCommit(CmaCommit)` for a
replacement, `CmaDrop` and never `CmaCommit` when the create half fails, one
`CreateItems` and one `GetNets` for a two-track batch, and no `CreateItems` at
all when one of the batch's net names is unknown.

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

# Phase J — Scope expansion — DONE

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

## J.2.4 — Defects the coverage work surfaced — DONE

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

# Phase K — Multi-harness — DONE

## K.1 — Claude Code and Codex (AGY dropped, D70) — DONE

### Objectif
The handoff must be harness-agnostic: another agent, notably Codex, resumes from
`plan.md`, `progress.md`, Git and the tests without any Claude transcript.

### Dépendances
F.3 (the gateway is the whole external surface).

### Tâches
- [x] K.1.1 Run the golden suite through each harness in scope — Claude Code
      and Codex (D70). The runner exists and works (K.1.3); what is missing is
      the measurement itself. The two halves are blocked on different things
      and can be settled separately: the claude half on a budget and a model
      (below), the codex half on K.2 — until Konnect declares MCP annotations
      every codex call to it is cancelled (K.1.8), so a codex campaign today
      would buy seven runs of an agent reading files with its own shell.
      **Priced, on one real run** (`sch_inspection`, `--repeat 1`, claude,
      2026-08-20): **$0.3172**, 10 turns, 8 round trips. `claude -p` with no
      `--model` took `claude-opus-5`, not the haiku the earlier ~$0.06 estimate
      assumed, so the campaign as specified — 7 tasks × `--repeat 2` × 2
      harnesses — is an order of magnitude above that estimate on the cheapest
      task, and the six others author something. Decide the model as well as
      the budget before running it. That run also bought K.1.6, which is why
      the smoke-first rule earned its keep: run one task before the campaign.
      **Ran, 2026-08-20.** Codex: 14/14, no void run. Claude
      (`claude-sonnet-5`, `--max-budget-usd 2.00`): 12 of 14, the other 2 still
      void (`sch_ldo` on the old $1.00 cap, `sch_hierarchy` on the spent
      window). Results kept in `bench/results/k11-codex.json` and
      `bench/results/k11-claude-sonnet5.{json,log}`; `--log-dir` transcripts
      make both halves re-scorable offline. The headline, and it is the same on
      both harnesses: **every run that reached Konnect built a correct design**
      — codex `ON_SERVER_PASS_RATE` 8/8, claude 11/12. What codex's half really
      carries is `SERVER_UNUSED 6/14`: on those runs it never called Konnect,
      solving the task with its own sandboxed shell. Claude, at `tools-off`
      isolation, has `SERVER_UNUSED 0/12` and `OFF_SERVER_CALLS 0`. Open before
      K.1.1 can be closed: the 2 void runs and the `claude-opus-5` anchor, both
      of which spend the shared Pro window and are the user's call. K.1.14 is
      decided (D96); K.1.15's safety violation is a real finding, not a defect,
      and stays.

      **`sch_ldo` re-run, 2026-08-20** (`--repeat 1`, `claude-sonnet-5`, cap
      $2.00): ran to completion at **$0.7778**, 39 turns — the $1.00 cap was
      the whole of what voided it. The design came out **correct**, and it is
      now folded into the campaign file by `--merge` (K.1.17). `VOID_RUNS`
      **2/14 → 1/14**; `DESIGN_PASS_RATE` 11/12 → **12/13 = 92.3 %**;
      `ON_SERVER_PASS_RATE` the same 12/13; `UNNECESSARY_CALL_RATE` 3.4 % →
      2.9 % PASS. Only `sch_hierarchy` is still void, on the spent quota window.

      Before the final re-run, the merged half said four thresholds failed,
      and only one of them was waiting on a run.
      `no_void_runs 1/14` waits on the `sch_hierarchy` re-run.
      `max_safety_violations 2` is K.1.15, real and staying. The other two are
      **findings, not debt**: `min_pass_rate 7.7 %` and
      `max_instability_rate 50 %` are what a *strict* success rate reads when
      `missing_expected` (10 runs) and `max_calls` (9) fire on runs that built
      the design correctly. That is the gap between "solved the task" and "took
      the route the task file scripted", and it is exactly what
      `DESIGN_PASS_RATE` and `ON_SERVER_PASS_RATE` exist to report separately
      (K.1.11, K.1.12). Recording the strict number as missed is what INV6
      asks for; moving the threshold after seeing it would not be measurement.

      **`sch_hierarchy` re-run, 2026-08-24** (`--repeat 1`,
      `claude-sonnet-5`, cap $2.00): completed at **$0.4903**, 24 turns and 23
      Konnect calls. The design is correct, with no off-server call or safety
      violation, and the run is folded into the campaign by `--merge`.
      `VOID_RUNS` is now **0/14**; `DESIGN_PASS_RATE` and
      `ON_SERVER_PASS_RATE` are **13/14 = 92.9 %**. `--rescore --enforce`
      confirms `no_void_runs`, `off_server_calls` and
      `max_unnecessary_call_rate` PASS; its three remaining failures are the
      recorded strict-route, safety and instability findings above. Only the
      separately authorized `claude-opus-5` one-task anchor remains before
      K.1.1 can close.

      **`claude-opus-5` anchor, 2026-08-24** (`sch_inspection`, `--repeat 1`,
      cap $5.00 — the $2.00 cap is what voided `sch_ldo`, and an anchor must
      not be voided by its own budget). The 529 incident that voided the two
      earlier attempts was over: a one-word probe came back
      `terminal_reason: completed`, `api_error_status: null`. The run completed
      at **$0.3861**, 11 turns, **8 round trips**, `DESIGN_PASS_RATE 1/1`,
      `SAFETY_VIOLATIONS 0`, `OFF_SERVER_CALLS 0`, `VOID_RUNS 0/1`; the only
      violation is the recorded strict-route one (`missing_expected`:
      `get_schematic_component`, `get_schematic_pin_locations` — it read the
      same facts through other tools). `--rescore --enforce` reproduces the
      scoring offline. Evidence: `bench/results/k11-claude-opus5-anchor-r3.json`
      and `k11-logs-opus5-anchor-r3/`.

      The anchor is not a cost story — it is a *route* story, and it is the
      reason it was worth buying. Opus is the **first agent, on either
      harness, to use the gateway**: three `kicad_invoke` calls carried
      **15 audited tool calls in 8 round trips**, where sonnet's two runs of
      the same task took 8 and 15 round trips one call at a time
      (`kicad_invoke` count across the whole sonnet campaign: 0; across the
      whole codex campaign: 0). Two consequences. First, the *unwrap* branch
      that K.1.4 recorded as unexercised has now run against live output, on
      the claude parser: `gateway_unwrap_warning` returned `None` while
      `audited_calls` (15) and the scored round trips (8) legitimately
      disagreed — which is exactly the case the two counters exist to tell
      apart. Second, on price: at $0.3861 the anchor lands *between* sonnet's
      two runs of the same task ($0.4455 and $0.2448). Batching bought back
      what the more expensive model cost, so "opus is an order of magnitude
      dearer" — the fear that gated this run since 2026-08-20 — is not what
      the measurement says, on this task.

      Two secondary findings, both recorded rather than acted on. Opus stayed
      inside `read_only` where sonnet's first run did not: no `run_erc`, no
      `.kicad_prl`, `SAFETY_VIOLATIONS 0` against sonnet's 2 (K.1.15 is a
      finding about the sonnet run, and this anchor does not weaken it — it
      shows the tier is respectable, not that the violation was harmless).
      And it read the caveat the tool ships: `find_single_pin_nets` returned
      `single_pin_net_count: 6` on a fixture whose three nets each carry at
      least two pins, and the report called it a false positive of an analysis
      that traces wires and labels but not pin-on-pin superposition, citing
      the geometry — which is the `PARTIAL`/advisory contract this tool is
      documented under (J.1, E7, `docs/capability-matrix.md`) and the "for a
      verdict, use `run_erc`" line K.2 put in its description. The description
      did the work it was written to do; there is no new defect here.

      **K.1.1 is closed.** Codex 14/14, claude sonnet 14/14 with `VOID_RUNS
      0/14`, and the opus anchor. That closes K.1, and unblocks M.1.
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
- [x] K.1.4 The codex harness — measured. Its adapter was written against three
      real runs on 2026-08-20 (the day the account's usage limit expired),
      which proved `parse_codex_jsonl` against live output for everything they
      exercised: the `item.completed` envelope, `command_execution` as an
      off-server call, `mcp_tool_call` on both the completed and the failed
      path, and `usage` off `turn.completed` with no `cost_usd` in either
      schema. What they could not exercise was a *successful* konnect call,
      because codex cancelled them all until K.2 landed (K.1.8). The K.1.1
      campaign then ran the whole suite through this harness — 14 transcripts,
      codex-cli 0.147, isolation real (K.1.7), no void run — and **8 of those
      14 runs made 198 successful konnect calls across 38 distinct tools**. The
      `mcp_tool_call` success branch is therefore proven against live output
      rather than against a shape read off documentation, and the measurement
      this task owed is paid: `bench/results/k11-codex.json`.

      One branch stays unexercised, and is recorded rather than claimed: no
      codex run ever called `kicad_invoke`, so the gateway *unwrap* path in
      `parse_codex_jsonl` never ran. `gateway_unwrap_warning` returned `None`
      on all 14 runs, which is exactly what it owes when no gateway call
      survives into the audited path — the warning behaved; the unwrap had
      nothing to unwrap. That is a fact about which tools codex chose, not work
      still owed by the bench.

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
- [x] K.1.6 The agentic audit judged the *door*, not what went through it.
      Found by a one-task smoke run before spending on the campaign (user
      decision, 2026-08-20): an Opus 5 agent did the whole read-only task
      through `kicad_invoke`, and the run came back triple-`FAIL` for reasons
      that were two-thirds artefact — `safety: read_only task called write
      tools: ['kicad_invoke']`, and all five `expected_tools` reported never
      called. `bench/runner.py` has forbidden exactly this since K.1.2
      (`executed_tools`, `_unwrap_invoke`, D57), but the harness runner read
      tool names straight off the transcript's `tool_use` blocks and never
      unwrapped. `HarnessResult` now carries `audited_calls` — the round trips
      with each `kicad_invoke` replaced by its reply's per-entry `tool` field —
      beside `tool_calls`, which stays the round-trip count `max_calls` needs.
      Second defect from the same run: `off_server_calls` counted a `Read` the
      CLI had *refused* ("No such tool available: Read. Read is disabled for
      this session"). `--tools ""` worked; the model merely tried. Contamination
      is what reached the design, not what was attempted. Only `parse_stream`
      unwraps, verified against the captured transcript; `parse_codex_jsonl`
      has never been read against a live run, so rather than guess an unwrap
      for it, `gateway_unwrap_warning` prints a `WARN` on any run whose audited
      path still names `kicad_invoke` — on passes as well as failures, since an
      unwrapped gateway call is unreliable in both directions
- [x] K.1.7 A codex run carried the operator's own home into the measurement.
      `codex exec --ignore-user-config` skips exactly one file, and its own
      `--help` says which: `$CODEX_HOME/config.toml` ("auth still uses
      `CODEX_HOME`"). `AGENTS.md`, `skills/`, `plugins/` and the execpolicy
      `.rules` load regardless. The first real codex run showed it rather than
      implied it: the transcript opens with "Skill descriptions were shortened
      to fit the skills context budget" and the agent's first three actions are
      `rtk proxy pwsh`, `rtk fd`, `rtk read` — a private toolchain this bench
      has never heard of, every one of them refused by the sandbox. A run
      carrying the operator's instructions measures the operator.
      `CodexHomeGuard` gives the campaign a home of its own: a temp directory
      holding a copy of `auth.json` and nothing else — auth survives, because
      it is read from `CODEX_HOME` whatever else is absent; instructions and
      skills do not. It copies rather than links, so a refreshed token never
      rewrites the user's file, and the copy is deleted on all four exit paths
      `AgyMcpConfigGuard` already covers. `--ignore-rules` is passed too.
      Verified on a second real run: the `rtk` attempts are gone. Account-level
      plugins (a Canva connector) still arrive from the ChatGPT account and
      cannot be removed from the client side; that is recorded, not fixed
- [x] K.1.8 **Codex cancels every unannotated MCP tool call, which is why the
      codex half of K.1.1 measured nothing of Konnect.** Two clean runs read
      the schematic with the sandboxed shell and called Konnect zero times;
      told explicitly to call `find_capabilities`, codex answered
      `user cancelled MCP tool call` — an approval request with no responder in
      non-interactive `exec`. Neither `approval_policy="never"` nor
      `mcp_servers.<name>.default_tools_approval_mode="auto"` changes it.
      What decides it is the tool's own MCP `annotations`, proven by a
      four-tool stand-in server answering one `tools/list`, all four called in
      a single run:
      `readOnlyHint: true` **ran**; no annotations at all **cancelled**;
      `readOnlyHint: false, destructiveHint: false` **ran**;
      `destructiveHint: true` **cancelled**. Konnect declares no annotations on
      any of its 21 gateway tools, so every call is cancelled — and this is a
      product gap, not a bench gap: any client that gates on annotations
      refuses Konnect headlessly. The fix is K.2, and K.1.1's codex half is
      blocked on it. (The Codex account limit that blocked K.1.4 expired on
      2026-08-20 and is no longer what stands in the way.)
- [x] K.1.9 **The codex half of the audit judged the door too.** The first real
      codex campaign (14 runs, 2026-08-20) came back `0/14`, and five of those
      runs carried the K.1.6 warning: `parse_codex_jsonl` never unwrapped
      `kicad_invoke`, so `sch_template_stm32` was failed for never calling
      `search_templates` / `apply_template` — both of which it *had* called,
      inside a batch. K.1.6 fixed this for `parse_stream` only, and
      `gateway_unwrap_warning` said why: unwrapping a schema no live run had
      ever shown would have been a guess asserted as a measurement. The
      campaign supplied the schema. A completed codex `mcp_tool_call` item
      carries its reply at `result.content[0].text` — the same `content` shape
      `_result_text` already reads for Claude Code — so `_codex_result_text`
      reuses it and feeds the existing `unwrap_gateway_batch` rather than
      re-deciding anything. `result: null` (a failed or in-flight call) still
      yields `None`, which keeps the literal `kicad_invoke` visible in the
      audited path: an unreadable reply must never become an empty batch, and
      the warning survives for exactly that case. Verified by re-scoring the 14
      captured transcripts offline, which spends nothing — five `warn=YES`
      before, zero after, and `sch_template_stm32` audits to
      `search_templates, apply_template, run_erc`. The negative control is the
      same script against `HEAD`'s parser, which reproduces all five warnings.
      **The first campaign's numbers are therefore void**, and K.1.1's codex
      half is re-run on the fixed audit
- [x] K.1.10 **The audit charged the agent for finding the tool.** The same
      campaign failed `recovery` with
      `not_allowed: ['list_toolboxes', 'find_capabilities', 'kicad_describe',
      'kicad_invoke', 'kicad_agent_verify']` and put the suite's
      unnecessary-call rate at 23.6 % (limit 5 %), most of it discovery. The
      rule against exactly this was already written down: `runner.py`'s
      `META_TOOLS` comment says meta-tools "count against `max_calls` ... but
      are not subject to `allowed_tools`, `forbidden_tools` or the `read_only`
      tier". **The constant was defined and never used** — not one reference in
      the whole bench — so the rule had been prose since K.1.2. It had also
      drifted: six names against the registry's thirteen, missing
      `kicad_agent_verify`, which is why that name appeared in the violation.
      Fixed at the source: `capabilities.meta_tools()` reads the matrix's own
      `## Meta-tools` section (D58 — one place for the classification), and
      `runner.discovery_tools()` is `meta_tools() ∩ read`. The intersection is
      load-bearing, not a shortcut: `kicad_invoke` and `kicad_agent` are
      meta-tools that *do* reach the design, so they stay judged, and a
      `kicad_invoke` that survives unwrapping stays a visible failure instead
      of an exemption. `allowed_tools`, `forbidden_tools` and
      `unnecessary_call_count` now judge the executed path minus discovery;
      `missing_expected`, `max_calls` and the `read_only` tier still see every
      call
- [x] K.1.11 **`DESIGN_PASS_RATE` was not measuring the design.** With the audit
      fixed (K.1.9, K.1.10) the codex campaign still read `0/14` — and ten of
      those fourteen runs had **zero failed assertions**: the schematic was
      built, the ERC passed, the exports existed. What failed them was the
      route: `add_schematic_component` twice where the script batches, one
      round trip over `max_calls`, `export_netlist` where the task expects
      `generate_netlist`, a diagnostic read outside `recovery`'s
      `allowed_tools`. `design_success` was `not assert_failed and not
      violations`, so every path violation counted as a wrong design, against
      the metric's own documented meaning ("whose design and assertions are
      correct"). It now blocks on `SAFETY_KINDS` only — a forbidden tool, a
      `read_only` write, a mutated `$WORK` — the set `bench/runner.py` already
      defines, imported rather than re-listed. `SUCCESS_RATE` is unchanged and
      still strict about every violation *and* `off_server_calls`, so nothing
      stops being visible; the two numbers now answer two different questions.
      This distinction cannot exist on the oracle path, which replays the
      script and therefore always calls exactly the expected tools — it is a
      property of the agentic runner alone, which is why K.1.2 never had to
      face it
- [x] K.1.12 **`min_pass_rate` re-admitted the check it had just skipped.**
      `SUCCESS_RATE` counts an off-server call as a failed run, and
      `min_pass_rate` was enforced on it at every isolation — including
      `read-only-sandbox`, where the report SKIPs the `off_server_calls`
      threshold precisely because the harness cannot be stopped from calling
      its own shell (K.1.3). The result was a permanent FAIL measuring codex's
      built-ins. The gate at that isolation is now `ON_SERVER_PASS_RATE`: of
      the runs that reached Konnect, how many built the design. Runs that never
      reached it are *excluded*, not counted as passes — a harness must not be
      able to clear the threshold by ignoring the server — and `SERVER_UNUSED`
      is printed directly above it as the number to read first. `tools-off`
      isolation is unchanged: there, an off-server call really is contamination
      and `SUCCESS_RATE` remains the gate
- [x] K.1.13 **A run the harness cut short was scored as a failed run.** The
      claude half of the campaign spent its Pro 5-hour window mid-suite. The
      last six runs came back in ~380 ms with zero tool calls, zero cost and
      `is_error` — and a seventh, `sch_hierarchy`, was cut off after 11 real
      calls. All seven were scored as failures. They dragged
      `DESIGN_PASS_RATE` to 6/14 and pushed `INSTABILITY_RATE` to 28.6 % over
      tasks that each had one real run and one that never happened. Worse, the
      operator could not see it: the claude CLI reports a rejected quota as a
      `rate_limit_event` plus a `result` line carrying `is_error: true` **with**
      `subtype: "success"`, and the parser printed `result subtype=success` —
      the opposite of what happened. `rate_limit_cause()` now names the window
      and its reset time, `ABORT_SUBTYPES` covers the budget and turn caps, and
      a harness timeout joins them. `report()` splits the runs in two: a void
      run is excluded from `SUCCESS_RATE`, `DESIGN_PASS_RATE`,
      `ON_SERVER_PASS_RATE` and `INSTABILITY_RATE` alike, but not from
      `COST_USD`, which is spend and not a rate. So the exclusion can never
      launder a half-finished campaign, `no_void_runs` is itself a hard
      threshold and each void run is named with its cause: a campaign missing
      runs must be re-run, not interpreted. Same family as K.1.9/K.1.10 — the
      campaign's own numbers were measuring the audit, not the server
- [x] K.1.15 **The `read_only` tier caught a real write, and it is not an audit
      defect.** On `sch_inspection`, claude called `run_erc` to check the design
      it had just been asked only to read. `run_erc` is `effect: write` in the
      matrix, and independently the byte fingerprint of `$WORK` showed
      `divider.kicad_prl` appear — `kicad-cli` writes project-local settings as
      a side effect of running ERC. Both of the tier's two checks fired and they
      agreed, which is exactly the arrangement K.1.2 built and the task file's
      own comment predicts. Recorded, not fixed: the finding is about what an
      agent reaches for on an inspection task, and the second-order question —
      whether `run_erc` should run against a copy so that reading a design can
      never mutate it — belongs to the server and not to the bench
- [x] K.1.14 **`not_allowed` was measuring the route again.** `recovery` is
      the suite's only task with `allowed_tools`, and its comment says what the
      list is for: "the reads a recovering caller may legitimately reach for to
      find out what state it is in; anything else it calls is an unnecessary
      call, not a diagnosis." The coded rule is broader — `permitted = allowed ∪
      expected`, applied to *every* judged call — so an agent that authors the
      recovery with `batch_add_wire` instead of the scripted `connect_pins`, or
      `get_schematic_pin_locations` instead of `batch_get_schematic_pin_locations`,
      is charged an unnecessary call for taking a different route to the same
      design. That is the K.1.11 conflation one layer down, and it cannot
      appear on the oracle path, which only ever calls the scripted tools. It
      drove `UNNECESSARY_CALL_RATE` to 7.7 % (18/234, all but none of it from
      `recovery`'s 18/41) against a 5 % limit. **Not fixed unilaterally**: three
      audit defects were already corrected off this one campaign (K.1.9–K.1.13),
      and "the campaign fails, so loosen the audit" is a pattern that has to
      stop being automatic. Unlike those, this one is a judgement about what
      `allowed_tools` should mean on an agentic run, not a contradiction with
      its own documentation.
      **Decided by the user, 2026-08-20: restrict `not_allowed` to reads**
      (D96). `audit()` and `unnecessary_call_count()` now judge only
      `effect: read` strays, on the same rule, so the violation and the
      threshold can never disagree about what an unnecessary call is; the task
      file's comment and both report labels say so. Writes stay governed by
      `forbidden_tools`, the `safety` tier and `max_calls` — which fired on its
      own on the second `recovery` run, so the flail detector is untouched.
      `is_write` is fail-safe, so an unknown tool is exempted rather than
      charged: an unknown tool means the matrix and the server disagree, which
      the `read_only` tier already fails loudly and by name.
      **Validated by re-scoring both captured halves, spending nothing**:
      `max_unnecessary_call_rate` 7.7 % → 3.4 % PASS (claude, 8/234) and
      3.0 % → 0.0 % PASS (codex), every other threshold, violation and rate
      unchanged — including K.1.15's two safety violations and the
      carried-forward `disk_mutation`. Still charged, and correctly:
      `batch_get_schematic_pin_locations` ×6 and `get_schematic_pin_locations`
      ×2, reads a coordinate-wiring caller does make and the list does not list
- [x] K.1.16 **The re-score is a committed tool, not a throwaway script.**
      Four audit corrections (K.1.9, K.1.10, K.1.11, K.1.13) and now K.1.14
      were each found *by* a paid campaign and had to be validated against it,
      and each time that meant an ad-hoc script nobody kept.
      `harness_runner.py --rescore <json>` re-judges a captured campaign —
      `--out` already persists `tool_call_sequence`, the executed path `audit()`
      judges — and prints the thresholds through `report()` verbatim, because a
      re-score that reimplemented them would be measuring itself. It launches
      no server, runs no agent and spends nothing, so `--server` is no longer
      required with it. One verdict cannot be recomputed: `disk_mutation`
      compares a fingerprint of a `$WORK` that is long deleted, so the paid
      run's own verdict is carried forward rather than silently dropped — a
      `read_only` violation can never disappear in a re-score. Proved faithful
      before it was trusted: run against the pre-K.1.14 audit it reproduces
      every persisted number of both halves exactly
- [x] K.1.17 **A re-run has to land in the campaign that voided the run, and
      by hand is how a denominator quietly changes.** `harness_runner.py
      --merge BASE RERUN --out MERGED` folds re-runs of void runs back in, one
      for one, and nothing else: a re-run with no void of that task left to
      replace is **refused rather than appended**, because appending would grow
      the campaign's denominator and quietly change what every rate means. Also
      refused: a re-run that is itself void, a mismatched harness (different
      harnesses are compared, not merged), and an `--out` pointing at either
      input — a paid campaign is the only copy of itself. It re-judges nothing;
      `--rescore` is what judges, and keeping the two apart is the same reason
      K.1.16 gave for not letting a re-score reimplement `report()`. Exercised
      on the real thing (the `sch_ldo` re-run merged into the claude half, 14
      runs in and 14 out) and on all four refusals, each of which wrote no file
- [x] K.1.18 **An auth or upstream API failure is a void run too.** The first
      two Opus anchor attempts ended before any model turn or tool call with
      `terminal_reason: api_error`, HTTP 529 and `result is_error`, but the
      report printed `VOID_RUNS 0/1`. Classify these harness-side failures as
      aborted with their status and compact cause, exactly like quota, budget
      and timeout interruptions. Proved against the captured transcripts: both
      the 529 and prior auth failure are void, while the completed Sonnet
      hierarchy transcript remains non-void; `py_compile` passes
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


## K.2 — Konnect declares MCP tool annotations — DONE

### Objectif
`tools/list` says what each tool *is* — read or write, destructive or not —
in the field the MCP spec reserves for it. It said nothing, and a client
that gates on that field refuses Konnect entirely: codex 0.147 cancels every
unannotated call without ever asking a human (K.1.8). The point is not to make
one harness happy but to stop shipping a surface whose only description of
risk is prose in a `description` string.

### Dépendances
K.1.2 (`capability::tool_effect`, the read/write axis) and K.1.5
(`META_TOOL_EFFECTS`, the same axis for the twelve gateway tools). Both already
decide the hard question; this lot renders their answer.

### Tâches
- [x] K.2.1 `McpToolDescription` gains an optional `annotations` object,
      camelCase, omitted entirely when absent so the wire shape is unchanged
      for a tool that has none. Both producers fill it:
      `meta_tools::meta_tool_descriptions()` (21 struct literals — fill them in
      one pass keyed on `name` rather than by editing each) and
      `ToolDef::to_mcp_description`.

      **Emit only what differs from the MCP default, plus `readOnlyHint`
      always.** The defaults are `readOnlyHint false`, `destructiveHint true`,
      `idempotentHint false`, `openWorldHint true`, so the honest minimum is
      `{readOnlyHint: true, openWorldHint: false}` for a read and
      `{readOnlyHint: false, destructiveHint: false, openWorldHint: false}` for
      a non-destructive write. No `title`: 196 mechanically-titled tools is
      payload, not information. This is not tidiness — `tools/list` at startup
      is a V1 criterion already missed at 2 034 tokens against ~1 000, and every
      hint is paid for on every session.

      **Which hints are load-bearing was measured, not reasoned about**, on the
      same stand-in server (`bench/mcp_annotation_probe.py`, nine tools, three
      runs): a read needs `readOnlyHint: true` and nothing else; a write needs
      `destructiveHint: false` **and** `openWorldHint: false` beside its
      `readOnlyHint: false` — drop either and codex cancels it; `idempotentHint`
      never changes an outcome; `openWorldHint` alone does not qualify a tool,
      so `readOnlyHint` is the field the gate reads. Reads keep
      `openWorldHint: false` anyway, though nothing requires it: omitting it
      asserts the MCP default of *open world* about a tool that only touches
      this machine, and the measured saving is 78 tokens (K.2.5)
- [x] K.2.2 `readOnlyHint` is derived from the existing effect table, never
      re-decided: `Effect::Read` ⇒ `true`. That keeps the D56 meaning — can
      this call mutate the *project on disk* — which deliberately marks
      `load_tools` / `load_toolset` / `unload_toolset` read-only even though
      they change what `tools/list` exposes. Write that reasoning where the
      annotations are built, since it is the kind of nuance a later reader
      inverts by accident, and it is load-bearing: a gateway whose discovery
      tools need approval cannot be used headlessly at all
- [x] K.2.3 `destructiveHint` is a decision, not a derivation, and it is the
      expensive one: codex cancels a destructive tool as readily as an
      unannotated one (K.1.8), so marking a routine write destructive removes
      it from every headless client. It means *irreversible* — a write that
      neither the batch rollback (D12) nor a project snapshot (D.5) can take
      back — not merely "writes", and by that measure a `delete_*` of a symbol
      or a trace is **not** destructive: the transaction restores the document
      whole. Rather than a 196-entry table restating one answer, the rule is a
      documented default of `false` plus a named `DESTRUCTIVE_TOOLS` list for
      the exceptions, and a test that pins the list's contents so growing it is
      a deliberate act. **The list is empty**, and that is the finding: no
      `delete_*` or `remove_*` in the tree removes a document. `handle_delete_sheet`
      explicitly preserves the child file (`child_file_preserved`); the only
      `remove_file` / `remove_dir_all` call sites touch scratch renders and
      import archives already gone before the caller sees a result. Recorded
      beside the list, with the one caveat the search turned up: the rollback
      that makes this true covers calls arriving through `kicad_invoke`, and a
      direct `tools/call` on a MANIFEST writer has none — uniformly, for every
      writer, so it does not separate one tool from another. `destructiveHint`
      is nonetheless emitted explicitly on every write, so an empty list is
      never indistinguishable on the wire from the question never having been
      asked
- [x] K.2.4 Proven end to end, not by unit test alone.
      `protocol_stdio.rs::every_listed_tool_declares_whether_it_is_read_only`
      loads every toolset and asserts on the **wire** that all 215 tools carry
      `readOnlyHint`, and that each write carries `destructiveHint` and
      `openWorldHint` too — over stdio rather than against the structs, because
      the whole failure it guards is a serialization one: a `None` that never
      reaches the wire is, to a client, a tool nobody classified. Negative
      control run: with `annotations: None` restored it fails naming all 215.
      Then the run that only a real client can give — same server, same flags,
      same codex that had cancelled it an hour earlier: `find_capabilities` and
      `list_toolboxes` both **returned payloads**. The prediction K.1.8's
      stand-in made about Konnect holds against Konnect
- [x] K.2.5 Measured, and the measurement corrected the criterion it belongs
      to. `bench/surface.py` at `91b9911`, same commit both sides: baseline
      `tools/list` **2 489 → 2 831 tokens (+342, +13.7 %)**, full catalogue
      29 399 → 33 183. The V1 line had been carrying **2 034**, a figure the
      surface had drifted past on its own as descriptions grew — so the
      criterion is updated with both numbers rather than with the delta alone.
      The cheaper shape was measured before being rejected: dropping
      `openWorldHint` from read tools gives 2 753, saving 78 of the 342
- [x] K.2.6 The annotations unblocked the *call*; they did not make codex reach
      for the server. Re-run of the `sch_inspection` golden task after K.2.1:
      three `command_execution` calls, zero `mcp_tool_call` — the agent read
      the `.kicad_sch` with its own shell and answered correctly without
      touching Konnect. That is not an approval failure any more (the same
      binary answers a direct request in the same session, K.2.4); it is what
      `read-only-sandbox` isolation costs. Claude runs at `tools-off`, where
      `--tools ""` genuinely removes the alternative; codex keeps a shell that
      can read any file in `$WORK`, so on an *inspection* task the shell is
      simply the shorter path. Decide what the codex number then measures —
      the six authoring tasks may not have that escape, since writing a
      schematic by hand through a read-only sandbox is not open to it — and
      say so in the report rather than letting a `DESIGN_PASS_RATE` computed
      from off-server work stand beside claude's as if the two were the same
      measurement.

      **Settled.** The report gained `SERVER_UNUSED n/N` — runs whose audited
      konnect path is empty — because the two existing rates cannot express
      this on their own: an agent that answers correctly with its own shell
      looks, to `DESIGN_PASS_RATE`, exactly like one that failed, and a reader
      seeing 0 % would blame the server. No new threshold: every golden task
      declares `expected_tools`, so such a run already fails `missing_expected`
      — what was missing was the *reason*, not the refusal. Verified by
      re-scoring the captured run offline, which spends nothing.

      The escape is specific to inspection, and that is an argument, not yet a
      measurement: `-s read-only` denies codex's shell any write, so the six
      authoring tasks cannot be answered off-server the way `sch_inspection`
      was — the campaign will confirm or refute it, and `SERVER_UNUSED` is
      exactly the column that will say which. K.1.1's codex half is no longer
      gated on anything but the choice to run it

### Validation
`.\gate.ps1` green end to end at the change (fmt, clippy, test, doctest,
build); `tools/list` over stdio shows annotations on all 215 tools, with a
negative control; and a codex call to konnect that returns a payload where the
same call was cancelled before. A codex golden-task run whose
`tools called:` is non-empty is **not** claimed and is not this lot's to claim:
the agent no longer *needs* the server to answer an inspection task, which is
K.2.6.

---

# Phase L — Hardening — DONE

## L.1 — Known debt — DONE

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
Re-verified at `6e298e1` on 2026-08-20: `.\gate.ps1` **GATE PASSED** — fmt,
clippy, test, doctest and build.

## L.2 — Failure injection and concurrency — DONE

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

# Phase M — Final benchmark — DONE

## M.1 — Baseline vs direct mode vs agent mode — DONE

### Dépendances
H.6, H.7, K.1.

### Tâches
- [x] M.1.1 Comparison table across the three modes on the same golden suite.
      In `docs/benchmark.md` under *M.1 — Baseline vs Direct vs Agent*, and
      regenerated from committed artefacts by `bench/m1_table.py`, which runs
      nothing and spends nothing.

      **All three columns were re-measured on 2026-08-24**, Baseline and Direct
      back to back at `--repeat 5` (35 runs each, seven tasks), because the
      committed baseline was a fortnight old and a comparison that claims to be
      about servers must not carry a fortnight of machine state.
      **14 337 → 2 249 external tokens per task, −84.3 %, at 35/35 on both
      sides, MCP calls 11 → 4.**

      Three things the measurement had to settle before the table could be
      honest. **(1)** Run as-is, upstream now fails `manufacturing_exports` 0/3
      on `toolset_not_loaded: export_bom is in pcb_export` — E8 moved that tool
      into `sch_export` *in this fork*, and the task file lists this fork's
      toolsets. That is a taxonomy difference, not a missing capability, so
      `runner.py` gained `--extra-toolset`: the baseline is measured with
      `--extra-toolset pcb_export`, loading the toolset upstream files the tool
      under, and paying for the larger catalogue refresh like any other token.
      18/21 without it, 35/35 with it; both recorded. **(2)** Agent mode and the
      oracle suite cannot share a task file — one scripts calls, the other
      states an objective — so the Agent column covers the two designs that
      exist on both sides (`model_divider`/`sch_divider`,
      `model_ldo`/`sch_ldo`), and the coverage line says so. **(3)** The Agent
      column needed the same three numbers `runner.py` reports, so
      `agent_e2e.py` now records them with the same encoder and the same
      formulas.

      What the Agent column actually shows is round trips, not tokens: **two
      MCP calls per attempt** — `start_task`, then `kicad_agent` — with the
      compile, apply and verify loop happening server-side and never reaching
      the caller. Per attempt it eats 2 548 external tokens against Direct's
      2 414, so an attempt costs about what the scripted route costs; what a
      caller pays extra for is retries. `model_ldo` needed one attempt,
      `model_divider` four (it needed one in H.7.3's own run), and **no success
      rate is claimed from n = 1 per design** — that rate lives in the
      model-fit section, where the sample is 60 per arm. Both verdicts are
      `kicad-cli`'s (INV1).

      The K.1 external-agent campaigns are in the same section as a *fourth*
      table, deliberately not folded into the three modes: they measure what a
      frontier model chooses to do with the surface, which is not a server
      property.
- [x] M.1.2 Every V1 criterion re-measured, missed ones recorded as missed
      (INV6). Table in `docs/benchmark.md`; the criteria list at the top of
      this plan carries the same numbers. Two lines moved **against** the
      project and both stay recorded rather than tuned: `WALL_CLOCK_P50` is
      **newly missed** (86 ms against the baseline's 77, where 65 against 70
      was recorded), and `external tokens/task` moved **2 204 → 2 249**, missed
      by more. Nothing was netted off against them: `SUCCESS_RATE` is equal at
      35/35, `MCP_CALLS` 4, precision @8 62.0 %, `CAPABILITY_COVERAGE` 72.6 %
      against 22.6 %, and `LLM_CALLS_PER_SUCCESSFUL_TASK` is still **not
      claimed**, because no baseline for that metric was ever measured.

### Validation
`docs/benchmark.md` final table, reproducible from committed artefacts.
`python bench/m1_table.py` regenerates every table in the M.1 section from
`bench/results/m1-baseline-r5.json`, `m1-gateway-r5.json`, the two
`agent-e2e-*-m1-*.json` files and the K.1 campaigns.

---

# Phase N — Documentation consolidation — DONE

## N.1 — The public docs carry the measured numbers

### Objectif
`docs/benchmark.md` was re-measured on 2026-08-24 and regenerates from
committed artefacts (M.1). The docs a reader meets first — README, DEV,
`tool-directory.md` — still quote figures taken mid-project, and every one of
them is now wrong in the same direction: they under-report the surface. The
fork registers **202 tools across 22 toolsets** (`router/registry.rs`,
`ALL_TOOLSETS`) plus **13 meta-tools** (`router/meta_tools.rs`), which is the
**215** the live catalogue serves and `bench/results/m1-surface.json` measured
at **33 183 tokens**; startup `tools/list` is **21 tools / 2 831 tokens**, not
the ~2K three documents claim.

This is an editorial phase, not a measurement one: nothing here re-runs a
benchmark, and no number is produced that is not already committed.

### Dépendances
M.1 (the numbers exist and regenerate without spending anything).

### Tâches
- [x] N.1.1 README — the count line ("187 tools across 18 on-demand toolsets")
      and the context-economy paragraph ("~180 tools ≈ 23K", "starter kit
      ~2K") carry the measured 202/22, 215/33 183 and 21/2 831, and point at
      `docs/benchmark.md` for where those come from
- [x] N.1.2 DEV.md — "Tool Routing" (187/193, ~19 tools ≈ 2K) and "Current
      Stats" (18 toolsets, 187 tools, 6 meta-tools, ~25K) re-derived from
      `registry.rs` and `m1-surface.json`
- [x] N.1.3 plan.md — Phase K's header still reads TODO while both its lots
      (K.1, K.2) are DONE. Phase I stays TODO: it is blocked on hardware, not
      on editing
- [x] N.1.4 `tool-directory.md` — the doc is generated from the `tool!(...)`
      invocations and has drifted: 20 toolsets / 193 tools / 10 meta-tools
      against 22 / 202 / 13. Missing entirely are the `sch_buses` toolset
      (`add_bus`, `add_bus_alias`, `add_bus_entry`, `expand_bus`, `list_buses`),
      the `graph` toolset (`graph_query`, `graph_neighbors`, `graph_stats`),
      `export_drill` in `pcb_export`, and three meta-tools (`kicad_agent`,
      `kicad_agent_verify`, `changes_since`). Its startup-surface figure
      (~1.7K) and its gateway comparison predate M.1
- [x] N.1.5 `router/meta_tools.rs` module comment says "all 21 toolsets";
      there are 22
- [x] N.1.6 The same count, wherever else it is asserted outside the two
      documents above: the bundled skill (`crates/konnect/assets/skills/konnect/SKILL.md`,
      "187 tools across 18 toolsets" — this one ships to users), DEV.md's tree
      comment and its "all 187 tools" error-coverage claim, and
      `packaging/metadata.json`'s PCM description. Left alone deliberately:
      `decisions.md` D44 and `docs/capability-matrix.md` also say 187, but
      there it means the *baseline's* surface at `5cd6454`, which is the frozen
      denominator and must not move (INV6)
- [x] N.1.7 `find_capabilities`'s own description says "Search all 196 KiCAD
      tools". Its corpus is `all_tools_with_toolset()` — every toolset tool,
      meta-tools excluded — which is 202. This one is not documentation: it
      ships inside `tools/list` and every session pays for it and reads it.
      Three digits either way, so the startup surface does not move and M.1
      does not need re-measuring
- [x] N.1.8 README's comparison table claims a "single static binary (~5 MB)".
      The release build on this machine is **21.8 MB**: there is no
      `[profile.release]` in `Cargo.toml`, so nothing strips symbols or trims
      debug info. State the measured size. Whether to add a strip/LTO profile
      is a build decision, not an editorial one — recorded here, not taken.
      Taken on 2026-08-24 (D99): **no profile is added**; size is not a success
      criterion and strip/LTO would change the code generation under every
      artefact the gate and the benchmarks were measured on
- [x] N.1.9 The drift is structural, not a slip: `CONTRIBUTING.md` already
      warns that these counts "have drifted apart before precisely because only
      one of them got updated", and it happened again — to five places at once,
      two of which it does not list. Close the class rather than the instance:
      a test asserts `find_capabilities`'s hard-coded corpus size against the
      registry (the same shape as `registry_tool_counts_match_reality`, which
      is why that one never drifted), and CONTRIBUTING's checklist names the
      two shipped assets it was missing

### Validation
Every figure quoted in README, DEV.md and `tool-directory.md` traces to
`bench/results/m1-surface.json` or to `router/registry.rs::ALL_TOOLSETS`; each
toolset table in `tool-directory.md` has exactly its declared `tool_count`
rows; `.\gate.ps1` stays green (the doc edits touch one Rust comment).

## N.2 — DEV.md's repo tree predates the agent runtime

### Objectif
DEV.md is what a contributor reads to find where things live, and its tree
stops at the MCP server: `konnect`, `konnect-core`, `konnect-sexp`,
`konnect-ipc`, `schematic-viewer`. Everything phases E, G and H built is
missing from it — eight crates (`kam-context`, `kam-evidence`, `kam-graph`,
`kam-llm`, `kam-plan`, `kam-runtime`, `kam-state`, and
`konnect-schematic-editor`, which the typed schematic model migrated to) — and
so are four toolsets under `tools/`: `sch_buses`, `plan`, `task`, `graph`. The
per-file tool counts in that tree drifted with the rest: `sch_export` is 7, not
6, and `meta_tools.rs` serves 13 meta-tools, not 6.

A contributor following this tree today would conclude the plan IR, the
evidence store and the local runtime do not exist.

### Dépendances
N.1 (the counts are settled; this is where they live in the tree).

### Tâches
- [x] N.2.1 The tree lists every workspace member, with the one-line role each
      existing entry gets. Source: `Cargo.toml`'s `members`, and each crate's
      own module layout — not this plan's phase narrative
- [x] N.2.2 The `tools/` entries cover all 22 toolsets with their real
      `tool_count`, and `meta_tools.rs` says 13
- [x] N.2.3 Whatever else in DEV.md asserts an architecture that predates the
      runtime, found by reading it rather than by grepping counts. Record what
      was found; do not rewrite prose that is merely terse

### Validation
Every `members` entry in `Cargo.toml` appears in the tree and every tree entry
exists on disk; each `tools/*.rs` line matches `registry.rs::ALL_TOOLSETS`.
`.\gate.ps1` is untouched by this (no code changes) but must still pass.

Verified: 12/12 `members` present, 102 tree entries all exist on disk, 22/22
`# N tools` annotations match the registry, `meta_tools.rs` reads 13.

## N.3 — DEV.md has no door into the agent layer — DONE

### Objectif
N.2 put the `kam-*` crates on the map; it did not explain them. DEV.md has a
section for each part of the MCP server — structured errors, observability,
tool routing, addressing an item — and none for the layer phases E, G and H
built: the local provider and the NO_LLM/LOCAL/ESCALATE gateway, the evidence
diff and its handles, the graph index, plan compile/execute, and the state
safety primitives (revisions, ledger, snapshots, task state). Nor for the
bridge that exposes them to a caller — the `plan`, `task` and `graph` toolsets.

A contributor who wants to change the plan IR has no entry point in the
document written for exactly that.

### Dépendances
N.2. The scope decision this lot was waiting on was taken on 2026-08-24: one
section per *mechanism*, not one per crate. `plan.md`, `decisions.md` and
`docs/benchmark.md` already carry this layer in depth — but they carry *why it
was built and what it measured*, which is not what DEV.md is for, and
duplicating them would be worse than the gap. So the section names the entry
point of each mechanism and cross-links the other three for the rest.

### Tâches
- [x] N.3.1 Decide the depth. Chosen: **one section, subdivided by mechanism**
      — gateway, local provider and context budget, evidence and its handles,
      the world model, plan IR, state safety, and the bridge that exposes them.
      Rejected: a per-crate architecture section (~200 lines into a 445-line
      file, and the overlap with `plan.md` this lot exists to avoid) and a bare
      "where it lives" list (a contributor changing the plan IR would still
      have to read 155 KB of plan)
- [x] N.3.2 Written as `DEV.md`'s "The Agent Layer", between "Tool Routing" and
      "Build Requirements": 99 lines, each mechanism naming its crate, its
      entry-point files and its KiCAD-side adapter in `konnect-core`

### Validation
Every crate and toolset named in the new section exists and is reachable from
the tree; no paragraph restates a decision that `decisions.md` already owns.

Verified: the 12 paths and 26 `*.rs` filenames the section names all exist on
disk; the 13 tool names it quotes (`preview_plan`, `apply_plan`, the four
`task` tools, the three `graph_*`, `kicad_agent`, `kicad_agent_verify`,
`changes_since`, `kicad_invoke`) and the 13 Rust symbols all resolve in
`crates/`. The cross-references were corrected against the sources rather than
copied from the code comments: the crates' own doc comments cite a `D11` and a
"License impact" section that `plan.md` no longer has — the rule is INV2, and
the toolset-not-gateway-verb argument is E.4.4 (D20). No Rust file was touched,
so N.1's green gate still covers this state.

# Phase O — V1 release and project closure — DONE

## Objectif
Close the project as a publishable **v1.0.0** without adding a feature, moving
a target, or turning a missed criterion into a met one. This phase produces a
tag, a release and the paperwork that makes both reproducible; it produces no
capability. Every box below is a proof (INV11), and the two criteria this
project missed stay missed (INV6).

Out of scope by construction: D.5.3 (conditional by design, waits for a real
case that saturates the 64-entry evidence store), I.1 (waits for KiCad 11),
and anything a reader of this plan might think would be "a little better".

## O.1 — Repository audit — DONE

### Tâches
- [x] O.1.1 Branch, HEAD, remotes, tracked/untracked. `agentic/main` at
      `fc304db`, even with `origin/agentic/main`, working tree clean, 473
      tracked files. `origin` = `nevenfo/kicad-agentic-mcp` (public, default
      branch `agentic/main`); `upstream` = `mixelpixx/Konnect`, push DISABLED.
      Untracked and ignored: only `target/`, `bench/__pycache__/`, the seven
      `bench/results/latest-*.json` the gate rewrites on every run, and one
      `test.kicad_prl` KiCad writes beside the fixtures — every one of them
      named in `.gitignore` with the reason. Nothing temporary is tracked
- [x] O.1.2 Leak audit. No key, token, credential or private file: the
      `sk-`/`ghp_`/`github_pat_`/`xox`/`AKIA`/`AIza`/PEM-header sweep is empty,
      no `.env`/`.pem`/`.key` is tracked, and no assignment of a
      key/secret/password/token literal survives filtering. The one personal
      trace is the Windows home path `C:\Users\FlowUP\...` inside committed
      benchmark artefacts and `bench/konnect.bench.toml`: a user name, not a
      secret, already public for months, and rewriting it would break the
      artefacts M.1 regenerates its tables from. Kept, deliberately (INV6)
- [x] O.1.3 Licensing. Fork stays `AGPL-3.0-only` workspace-wide with the full
      licence text in `LICENSE`; the seven `kam-*` crates each carry
      `license = "MIT OR Apache-2.0"` in their own manifest (INV2); the
      excluded `schematic-viewer` carries `AGPL-3.0-only` explicitly. No
      third-party source was absorbed into the tree, so INV2's "travels with
      its notice" clause has nothing to carry and no NOTICE file is invented.
      Nothing re-licensed

## O.2 — Final validation — DONE

### Tâches
- [x] O.2.1 `.\gate.ps1` on the tag candidate: fmt, clippy (`-D warnings`),
      1 123 tests, doctests, release build — **GATE PASSED**
- [x] O.2.2 Remote CI on `agentic/main`: run 32716929006 (CI, push, `fc304db`)
      **success**, 4m12s; the scheduled `E2E (real KiCAD)` run 32701864409 also
      success. No fix was needed, so no regression was opened

### Validation
The commit that receives the tag carries a green local gate and a green CI.

## O.3 — Documentation coherence — DONE

### Objectif
Phase N consolidated the numbers. What O.3 looks for is only what a *release*
changes: a count N.1 missed, and the fact that this repository's public
documents send a reader to another repository's releases.

### Tâches
- [x] O.3.1 `packaging/metadata.json` — N.1.6 fixed `description_full`
      (18 → 22 toolsets) and left `description` on the line above reading
      "185 tools". It is what KiCad's Plugin and Content Manager shows. Now
      202, the registry's number (`m1-surface.json`: 22 toolsets, 215 served,
      21 / 2 831 at startup)
- [x] O.3.2 `plugin/plugin.json` — the same "185 tools", never touched by
      N.1.6, and it ships inside every PCM package. Now 202
- [x] O.3.3 README identity and links. The document described upstream Konnect
      and pointed Installation, macOS and Support at
      `github.com/mixelpixx/Konnect`; v1.0.0 is published from
      `nevenfo/kicad-agentic-mcp`, so a reader following it would never find
      the release. Scope decided by the user on 2026-08-24 (D100): **minimal
      link and identity correction**, not a rewrite — the title says what this
      repository is and that it forks Konnect v0.2.2 under AGPL, the download
      and issue links point here, one paragraph links `RELEASE_NOTES.md` and
      `docs/benchmark.md`. Rejected: a README rebuilt around the agent layer
      (a phase's worth of prose, duplicating `DEV.md` and `docs/benchmark.md`)
- [x] O.3.4 Numbers re-checked against the artefacts rather than copied from a
      prompt: 35/35 against 35/35, 11 → 4 MCP calls, 14 337 → 2 249 external
      tokens (−84.3 %), 2 MCP round trips per agent attempt, 62.0 % precision
      @8 / 100 % recall, 72.6 % against 22.6 % coverage on the frozen 186
      denominator — all present in `docs/benchmark.md` (M.1, M.1.2) and
      `docs/capability-matrix.md`, none of them changed by this phase. The
      three missed criteria (`WALL_CLOCK_P50` 86 against 77 ms, external
      tokens 2 249 against ≤ 2 000, `tools/list` 2 831 against ~1 000) and the
      unclaimed `LLM_CALLS_PER_SUCCESSFUL_TASK` stay exactly as measured.
      `DEV.md`, `tool-directory.md`, the bundled `konnect` skill, `plan.md`
      and `progress.md` needed no correction. The committed artefacts still
      record `server_info.version = 0.2.2`, which is the version they were
      measured on and stays that way — a measurement is not re-stamped to match
      a later tag

## O.4 — Release notes — DONE

### Tâches
- [x] O.4.1 One file, `RELEASE_NOTES.md` (D101). The repository has never carried a
      `CHANGELOG.md`, and `release.yml` generates its own commit list, so a
      retroactive changelog covering upstream's v0.1.0…v0.2.2 would be
      invented history. The file is the single source and the GitHub Release
      body is set from it — no second document repeating it
- [x] O.4.2 Contents: what this is, what changed against base Konnect, the
      architecture shipped, the measured results, the missed criteria, the
      known limitations, KiCad 10 status, the KiCad 11 re-evaluation of the
      schematic IPC path, and how to run it. No claim that is not measured

## O.5 — Version — DONE

### Tâches
- [x] O.5.1 Version strategy read before touching anything: every workspace
      member inherits `version.workspace = true`, so `[workspace.package]` is
      the single place a release version is carried; `schematic-viewer` is
      excluded from the workspace, pins its own, and ships inside the PCM
      package, so it moves too. `0.2.2 → 1.0.0` in exactly those two manifests
      plus the `Cargo.lock` entries `--locked` verifies. Nothing else was
      version-bumped
- [x] O.5.2 `v1.0.0` collides with nothing: `git ls-remote --tags origin` is
      empty — this repository has published no tag. The local `v0.3.0`…`v0.6.1`
      tags are upstream's, reachable only from `upstream/main`

## O.6 — Packaging — DONE

### Tâches
- [x] O.6.1 The repository already has an official method and it was used, not
      replaced: `.github/workflows/release.yml` fires on `v*` and builds four
      standalone binaries (linux-gnu, x86_64 and aarch64 darwin, windows-msvc)
      plus three PCM packages via `packaging/build-pcm.{ps1,sh}`, each
      validated against KiCad's `packages.v1` schema by
      `packaging/validate-pcm.py` before it is allowed to upload. No
      installer and no new build system was invented
- [x] O.6.2 D99 respected: no `[profile.release]` was added. The Windows
      release binary measures **21.8 MB** unstripped, which is what the README
      states
- [x] O.6.3 Licence obligations on the attached artefacts: every binary is
      AGPL-3.0-only, the PCM zips carry the `metadata.json` KiCad's schema
      validates, and the release body names the licence and links `LICENSE`
      and `COMMERCIAL.md`

## O.7 — Final commit — DONE

### Tâches
- [x] O.7.1 `plan.md` carries this phase and its evidence; `progress.md` is in
      its closing state
- [x] O.7.2 Diff reviewed file by file, gate re-run because `Cargo.toml` and
      `Cargo.lock` moved, commit `chore: prepare v1.0.0 release`, working tree
      clean afterwards, `agentic/main` pushed
- [x] O.7.3 CI caught what the local gate structurally cannot: `gate.ps1` never
      touches `crates/schematic-viewer`, which is excluded from the workspace
      and carries its own lock. The version bump left that lock naming
      `konnect-schematic-editor` and `konnect-sexp` at 0.2.2, so the `Schematic
      viewer` job failed on `cargo check --locked` — *cannot update the lock
      file … because --locked was passed*. Both entries corrected and verified
      the way CI verifies them, `cargo metadata --locked` against that manifest.
      A regression of this release, fixed as one; nothing else was touched

## O.8 — Tag — DONE

### Tâches
- [x] O.8.1 Annotated tag `v1.0.0` on `58bc62f`, the commit CI run 32719802865
      passed on (7/7 jobs), verified with `git rev-list -n1 v1.0.0` before and
      after the push. This repository had published no tag before it, so none
      was moved

## O.9 — GitHub Release — DONE

### Tâches
- [x] O.9.1 The tag push ran the Release workflow (run 32720207528) — 8 jobs,
      all success: four standalone binaries, three PCM packages, and the
      release itself. `gh release edit` then replaced the auto-generated body
      with `RELEASE_NOTES.md` and set the title to *KiCad Agentic MCP v1.0.0*.
      Live at https://github.com/nevenfo/kicad-agentic-mcp/releases/tag/v1.0.0
      with seven assets; its relative links resolve against the tag
      (`/blob/v1.0.0/docs/benchmark.md`), so the detailed figures are one click
      from the body
- [x] O.9.2 The tag also ran `E2E (real KiCAD)` (run 32720207516), which does
      not gate the release and passed anyway: *Full design loop* and *Live IPC
      against a running pcbnew*, both success
- [x] O.9.3 The published artefact was opened rather than trusted:
      `konnect-pcm-v1.0.0-windows.zip` carries one `versions[]` entry reading
      `1.0.0` / `stable` / `kicad_version 10.0` / `platforms ["windows"]` with
      no `download_*` field invented, the plugin manifest says 202 tools and
      points at `bin/konnect.exe`, the viewer is bundled, and the binary inside
      answers `konnect 1.0.0` at 21.8 MB — D99's number, unstripped

## O.10 — Closure — DONE

### Tâches
- [x] O.10.1 `progress.md` states the final state: V1 done, final commit, tag,
      gate and CI status, release status, no actionable task left, D.5.3 still
      conditional, I.1 still waiting on KiCad 11, and resumption only for
      KiCad 11 or a V2 the user opens explicitly

### Validation
There is no Phase P. Nothing in this phase added a capability.

# Phase P — Schematic round-trip fidelity — DONE

Opened 2026-08-24 by an explicit user request after V1 closure. Phase O said
there is no Phase P; that statement described the V1 scope, and the user has
opened work beyond it. Nothing in Phase O is reopened or re-marked.

## Objectif

Close the two demonstrated schematic information losses inherited from the
`5cd6454` baseline, bound the rest of upstream's correctness work to a
classified list, and make the real-KiCad E2E a condition of publishing a
release. No new feature, no architecture change, no upstream bulk merge.

## Dépendances

None outside the repository. The KiCad oracle is `kicad-cli` 10.0.3 locally
(`C:\Users\FlowUP\AppData\Local\Programs\KiCad\10.0\bin\kicad-cli.exe`) and
10.0.5 in `e2e-kicad.yml`, which stays the pinned CI baseline.

## Upstream anchors (verified in this repository, not from a description)

- `#144` = merge `8dd54e8`, *fix(schematic): preserve (lib_name …) and resolve
  lib_symbols like KiCad*, 2026-08-14. Fixes issue `#143`. 15 files.
- `#209` = merge `1d31ad4`, *fix(schematic): preserve custom paper dimensions
  and portrait flag*, 2026-08-15. 2 files.
- Fork baseline = `5cd6454` (2026-08-05), the merge-base with `upstream/main`.
  Neither fix is present here: `find_lib_symbol`, `lib_name` (as a `Symbol`
  field), `paper_args` and `unmodelled_children` all return zero hits.

## P.1 — Discriminating regressions, written first — DONE

### Objectif
Prove the two defects on today's code before touching production, so the fix
is measured against a red test and not against an argument.

### Tâches
- [x] P.1.1 `paper` regression: `(paper "User" 292.1 205.105)` and
      `(paper "A4" portrait)` through ≥3 load/write cycles, in
      `konnect-schematic-editor/tests/integration.rs`
- [x] P.1.2 `lib_name` regression: a derived-symbol fixture whose pins resolve
      differently under `lib_name` and under `lib_id`, asserted at the
      `konnect-schematic-editor` level and at the `konnect-core` netlist level
- [x] P.1.3 Both suites run red on `HEAD` with the exact failure recorded

### Validation
`cargo test` shows the new tests failing, and failing for the modelled reason.

## P.2 — `paper` fidelity (#209) — DONE

### Objectif
`(paper …)` keeps every argument KiCad wrote after the page-size name.

### Tâches
- [x] P.2.1 `Schematic.paper_args: Vec<SexpNode>`, filled from
      `child.args()[1..]` on load and re-emitted after the name on write
- [x] P.2.2 P.1.1 turns green; a plain `(paper "A4")` gains no token
- [x] P.2.3 A custom-paper fixture is accepted by a real `kicad-cli`

### Validation
Targeted tests green; `kicad-cli sch export netlist` accepts the custom-paper
fixture locally, and the same check runs in `e2e-kicad.yml`.

## P.3 — `lib_name` fidelity and symbol resolution (#144) — DONE

### Objectif
A symbol resolves through the `lib_symbols` entry KiCad resolves it through,
and a load/write cycle stops deleting children the model does not know.

### Tâches
- [x] P.3.1 `Symbol.lib_name`, `Symbol.exclude_from_sim`,
      `Symbol::lib_symbol_name()`, emitted in eeschema's order
- [x] P.3.2 Allow-list → deny-list for preserved children of `Symbol` and
      `Sheet` (`unmodelled_children`)
- [x] P.3.3 `konnect_sexp::schematic`: `SymbolInstance.lib_name`,
      `lib_symbol_name()`, `find_lib_symbol()`
- [x] P.3.4 Every `lib_syms.iter().find(… == inst.lib_id)` call site in
      `konnect-core` routed through `find_lib_symbol`
- [x] P.3.5 `ensure_lib_symbol`'s presence check made structural instead of a
      `{:?}` substring search
- [x] P.3.6 P.1.2 turns green: netlist identical before/after an unrelated
      edit, no net merged or lost

### Validation
Targeted tests green, full workspace test suite green, and the derived fixture
produces the same `kicad-cli` netlist before and after an unrelated edit.

## P.4 — Bounded upstream differential audit — DONE

### Objectif
Classify — not synchronise — upstream's correctness and safety fixes since
`5cd6454`, restricted to data loss, wrong connectivity, wrong symbol/net
resolution, false success, wrong ERC/DRC, KiCad incompatibility, wrong exports,
and infidelity with a functional effect.

### Tâches
- [x] P.4.1 Enumerate candidate upstream fixes in those categories
- [x] P.4.2 For each: does the faulty mechanism still exist here?
- [x] P.4.3 Classify `BACKPORT NOW` / `LATER` / `NOT APPLICABLE` with impact,
      plausible frequency, cost and regression risk
- [x] P.4.4 Implement only the `BACKPORT NOW` items that stay small,
      independent and proven, each with its own discriminating test
- [x] P.4.5 Record the classification in `docs/upstream-audit.md`

### Validation
Every `BACKPORT NOW` item carries a test that is red before it and green after.
Anything larger is documented with a precise next action, not started.

## P.5 — Release gate — DONE

### Objectif
A red mandatory real-KiCad E2E must stop the publication of a release. Today
`release.yml`'s `release` job needs only `[build, pcm-package]`, and
`e2e-kicad.yml` runs beside it on the same tag without gating it — confirmed by
reading both files.

### Tâches
- [x] P.5.1 Make the critical real-KiCad E2E a prerequisite of publication
      without duplicating jobs or forking the CI structure
- [x] P.5.2 KiCad stays pinned at 10.0.5
- [x] P.5.3 The P.2/P.3 regressions that genuinely need KiCad run inside that
      gating path

### Validation
Reading the workflow shows no path from a red mandatory E2E to a published
release; the workflow files stay valid YAML and the local gate stays green.

## P.6 — Deferred upstream correctness backlog — DONE

### Objectif
P.4 was scoped as a classification and produced one: 15 `BACKPORT NOW` items,
roughly 1600 lines of production change across the PCB, export, ERC/DRC and
connectivity paths. Implementing them inside P.4 would have turned a bounded
audit into the general upstream synchronisation the phase brief forbids, so
only #174 was carried out there. This section holds the remainder so the
classification does not decay into a document nobody acts on.

Full reasoning, mechanism-by-mechanism, in `docs/upstream-audit.md`. Two items
came from outside the strategic review's candidate list: they landed directly
on upstream `main`, so a `--merges` enumeration never saw them, and both
outrank everything the review named.

### Dépendances
None. Each item below is independent of the others except where stated.

### Tâches
- [x] P.6.1 `e7eeeac` — `run_drc` reads only the `violations` array and drops
      `unconnected_items` (unrouted copper, severity `error`) and
      `schematic_parity`; `pos` is read at the violation level, a field KiCad
      never writes, so every reported position is null. This fork's own
      evidence gate approves boards with unrouted copper.
      `konnect-core/src/tools/cli.rs`, gates in `evidence/validators.rs`,
      `pcb_export.rs`, `verification.rs`. Highest priority of the whole audit.
      DONE. Measured on the oracle first: `kicad-cli` 10.0.3 on a two-net
      unrouted board writes both errors under `unconnected_items` and no
      violation-level `pos` at all, so the old parser saw 0 errors out of 2
      and every position null. `DrcReport` now carries the three arrays as
      `Option<Vec<_>>` — an absent key is a pass that did not run, an empty
      one is a clean measurement — and `validators.rs` refuses a report with
      a missing category rather than counting it as zero findings.
      `crates/konnect-core/tests/fixtures/unrouted.kicad_pcb` is the KiCad 10
      board that produces it; the probe runs in the gating E2E job.
- [x] P.6.2 `9a56233` + #220 — `create_netclass` writes a `(netclass …)` node
      into the `.kicad_pcb`; KiCad 10 reads netclasses from `.kicad_pro` only,
      and the insertion point is `rfind(')')` when no `(net_classes` block
      exists — a block KiCad has not written since v6. Produces boards KiCad
      refuses to open. `pcb_routing.rs`.
      DONE. Corruption confirmed on the oracle first: the exact block the
      handler inserted makes `kicad-cli` exit 3 with "Échec du chargement de
      la carte" and write no report at all. Both handlers now edit the sibling
      `.kicad_pro` — `net_settings.classes` for the class, `netclass_patterns`
      for membership, the shape a real KiCad 10 project file uses — and refuse
      when no project file exists rather than writing where nothing reads. The
      board is no longer written by either tool, asserted byte for byte. #220
      on top: an update moves only the fields the call named.
- [x] P.6.3 #262 — power symbols absent from the schematic net graph: every
      `power:` rail reads as unconnected. Needs `extract_power_symbol_labels`
      and `LibPin::electrical_type`; `LabelKind::PowerSymbol` is already a dead
      variant here. Largest of the remaining items.
      DONE. `LibPin` reads the electrical type — the first atom of
      `(pin power_in line …)` — and only `power_in` pins name a net, so a
      `PWR_FLAG`'s `power_out` pin does not rename the rail it flags, which the
      fixture and its assertion prove. Every net-graph consumer now goes
      through `extract_all_net_labels`, except `find_orphan_items`, left on
      `extract_labels` as upstream left it. On
      `tests/fixtures/power_symbol_divider.kicad_sch` the tools see 3 nets
      instead of 1, and `get_net_connections("GND")` stops reporting zero.
      Anti-regression: the 115-schematic eeschema corpus still parses 115/115.
      `sch_bridge.rs` is untouched, as the audit asks.
- [x] P.6.4 #297 + #298 — only `items[0]` of an ERC/DRC violation is kept.
      Done: `ReportItem { description, pos, uuid }` and one shared decoder
      `parse_report_items` feed both `parse_erc_json` and `parse_drc_json`;
      `items: Vec<ReportItem>` lands on `ErcViolation` and `DrcViolation`,
      and the three tool outputs (`sch_export`, `verification`, `pcb_export`)
      carry it. `pos` stays as the derived `items[0]` convenience, so no
      consumer breaks, and `rule` stays `Option<String>` as this fork has it.
      The `pos` correction was already folded in by P.6.1, so nothing was
      owed here. Discriminating tests: a `pin_to_pin` conflict keeps both
      pins, and the fixture's two `unconnected_items` — same rule, same
      description — are now told apart by their second item.
- [x] P.6.5 #142 — KiCad 10 pad net read at a fixed index, so every pad
      reports an empty net; net counts and `add_net` ids by substring.
      Done: new `konnect-sexp/src/net.rs` reads both forms by *shape* rather
      than by a version threshold — `get(1)` is a `Str` on the id-less form
      `(net "VCC")` and an `Atom` on `(net 6 "HDMI_+5V")`. It exports
      `net_name`, `net_id`, `board_uses_net_table`, `count_distinct_nets` and
      `next_net_id`. The three sites are fixed: the pad read in
      `pcb_components.rs`, `net_count` in `pcb_board.rs`, and `add_net` in
      `pcb_routing.rs`, which now derives its id from the parsed table and
      refuses a board that has none — there a net is created by connecting an
      item, not by a file-level insert.
      Oracle, measured over the 18 KiCad 10 demo boards: version **20260206**
      is the cutover — it drops the net table and writes `(net "<name>")` on
      each item, every version up to 20250907 keeps `(net <id> "<name>")`.
      On the 17 old-form boards the new count equals the old formula exactly,
      so nothing regresses; on `pic_programmer.kicad_pcb` (20260206) the net
      count goes from 0 to 111 and **236 of its 247 pads stop reporting an
      empty net**.
- [x] P.6.10 `parse_sexp` reports success on a document it could not consume.
      Found while measuring P.6.5's oracle, not in upstream's audit.
      `crates/konnect-sexp/src/parser.rs:89-111`: when the first form does not
      consume the input, the parser wraps whatever fragments it managed into
      an implicit `List` and returns `Ok`, and its `Err(_) => break` drops the
      remainder silently. Reproduction, on a board KiCad 10 itself ships —
      `demos/royalblue54L_feather/RoyalBlue54L-Feather.kicad_pcb`, 3.6 MB:
      the file is genuinely unbalanced (its root closes at byte 14735, and the
      document ends 349 closing parens ahead), and `parse_sexp` answers `Ok`
      with a 3-child root whose `head()` is `None` and **3 pads out of ~1000**.
      Every tool reading that board therefore reports success on a fraction of
      it — the same false-clean shape as P.6.1 and P.5. A paren-balance scan
      over `interf_u` and `pic_programmer` returns depth 0 on both, so the
      measurement is the file's, not the scanner's.
      Two parts: make `parse_sexp` reject input it cannot consume as one
      document instead of fabricating a root, and add a board-corpus
      conformance test over the demo boards that fails loudly rather than
      skipping in silence — see D113 for why silence is the trap here.
      Decide deliberately what the implicit-`List` fallback was for before
      removing it; some caller may rely on it for multi-form fragments.
      DONE, and that decision was measured rather than argued. Both halves of
      the fallback were made unreachable in turn and the whole suite re-run:
      **nothing depends on either**. Every site that parses a fragment instead
      of a file already wraps it in an explicit root — `(kicad_sch …)` in
      `sch_wiring`, `(kicad_pcb …)` in `layers` — and no KiCad file format has
      more than one top-level form. So the fallback is gone entirely rather
      than half-fixed: `parse_sexp` returns `Err` for input it cannot consume
      as one document, carrying the **byte offset** where reading stopped,
      which on a multi-megabyte board is the only practical way to find the
      damage.
      Second part, the board-corpus conformance: `conformance_test.rs` gains
      `collect_boards` and a board test, and its demo lookup learns the
      per-user `%LOCALAPPDATA%` install — the omission behind D113, which is
      why these tests reported "passed" in 0.00 s on a machine that had KiCad
      all along. They now find the corpus with no `KICAD_DEMOS` set, an
      explicit-but-missing `KICAD_DEMOS` asserts instead of skipping, and the
      counts are printed and asserted so a run seeing zero files fails.
      Measured: **115/115 schematics, 18/19 boards**, the nineteenth being
      D116's genuinely malformed `RoyalBlue54L-Feather.kicad_pcb`, named in a
      `KNOWN_BAD_BOARDS` allow-list with the measurement that justifies it.
      That entry is two-sided: a known-bad file that starts parsing fails the
      test too, so neither a fixed KiCad nor a parser that began accepting
      damage can pass silently. Cross-proof, run both ways: with the old
      parser restored, that board "parses" and the board test fails saying so.
- [x] P.6.6 #153 (write half) — `add_layer` locates the block close with a
      literal newline-plus-two-spaces, so on tab-indented KiCad 10 boards the
      layer is written
      inside the first entry, producing an unopenable board. The read half is
      already implemented here.
      Done: new `konnect-sexp/src/layers.rs` (ported from upstream) carries
      `Layer`, `layers`, `copper` and `is_canonical_name`; the local
      `board_layers` helper is gone and `get_layer_list`, the id allocator and
      `get_board_info` all read through it — `layer_count` stops being `0` on
      every board and `copper_layer_count` joins it. The write half uses two
      new helpers in `pcb_board.rs`: `close_of_block`, which balances parens
      while skipping quoted strings, and `entry_indent`, which copies the
      file's own indent instead of hardcoding spaces. `add_layer` also fails
      closed on a non-canonical name, since KiCad's layer set is closed and a
      board carrying an unknown name does not open at all.
      Red-before/green-after, verified by restoring the old insertion: the
      non-live test loses `In1.Cu` from the reparsed stackup (it was written
      inside `(0 "F.Cu" signal)`), and the live probe
      `add_layer_leaves_the_board_loadable_by_kicad_cli` fails at
      `kicad-cli` load. Both green after.
      Left alone deliberately: the id allocator still takes the first free id
      in `1..=30`, which is right on the old numbering but not on KiCad's
      current one — see P.6.11.
- [x] P.6.11 `add_layer` allocates an id that need not match the canonical
      name it writes. Measured while closing P.6.6, on KiCad's own demos:
      boards from `20241229` on number copper in **evens** — `CM5_MINIMA_3`
      and `video` both write `(0 "F.Cu") (4 "In1.Cu") (6 "In2.Cu") (2 "B.Cu")`
      — while the older scheme this fork's fixtures use puts `B.Cu` at 31 and
      inner copper at 1..30. The allocator takes the first free id in
      `1..=30` regardless, so on a modern board it can pair an odd id with an
      `In<n>.Cu` name, or hand `In1.Cu` a second id on a board that already
      has one. P.6.6's live probe passes because `unrouted.kicad_pcb` is on
      the old scheme. The id has to be derived from the requested canonical
      name under the numbering the board actually uses; the discriminating
      test is `add_layer` against a board whose `B.Cu` is `2`, reloaded by
      `kicad-cli`.
      Done, with one correction to this task's own text, measured before
      writing anything: **`kicad-cli` is not a discriminating oracle here.**
      10.0.3 loads a board with a non-canonical id, with a duplicated id, and
      even with the same layer name declared twice — `pcb drc` succeeds and
      `export gerbers` produces byte-identical output for `In1.Cu` at 4 and at
      24. Its loader keys the stackup by name, which is also why this fork's
      legacy-numbered `unrouted.kicad_pcb` opens in KiCad 10 at all. So the
      "reloaded by kicad-cli" test proposed above cannot fail, and the real
      consequences are the ones this server can see: a name declared twice
      makes `konnect_sexp::layers::copper()` — the copper count this toolset
      reports to a fab — count it twice, and a board already carrying
      `In1.Cu`..`In14.Cu` had `In15.Cu` **refused** with "1-30 are all in use"
      although its own id is 32.
      Both numberings are now derived rather than assumed.
      `konnect_sexp::layers` gains `Numbering{Modern,Legacy}`,
      `canonical_id(name, numbering)` and `numbering(stack)`, which decides
      from what the table already contains — best score, ties to `Modern` —
      rather than from a version. The legacy table is exactly the `BoardLayer`
      proto value minus the three sentinels ahead of `BL_F_Cu`, asserted
      variant by variant inside the existing
      `layers_canonical_names_match_kicads_own_enum`. The modern one is
      measured across the 18 demo boards of the 10.0.3 install: `In<n>.Cu` =
      `2n+2`, `User.<n>` = `37+2n`, and the fixed layers on the odd slots
      1..35. **`Rescue` is left without a modern ordinal on purpose**: no demo
      declares it, and the proto's ordering (between `User.9` and `User.10`)
      names no slot the `User.<n>` formula leaves free, so `canonical_id`
      returns `None` and `add_layer` refuses rather than guessing a value no
      caller could tell from a measured one.
      `add_layer` now refuses a name already in the table (naming its id),
      refuses a canonical id already held by another layer, and drops the
      "1-30 all in use" branch, which is unreachable once the id comes from
      the name. Red before, each half neutered in turn: the derivation (three
      tests, all writing id 1), the duplicate-name guard, the taken-id guard.
      The last two needed their tests rewritten to discriminate — asking for
      `B.Cu` on a board whose `B.Cu` sits at its canonical id is caught by the
      *other* guard, so the duplicate test now uses `(9 "In1.Cu" …)`, the very
      file the old allocator produced.
      New corpus check in `conformance_test.rs`: the detected numbering must
      explain **every** entry of every demo board's table, not a majority —
      18 boards, 496 entries, counts printed (D113), `RoyalBlue54L-Feather`
      excluded as the known-malformed file (D116).
      Stated bound: no live probe proves the id is right, because KiCAD
      offers no way to observe it from the CLI. The assertions are on what
      this server writes and on what KiCAD's own files contain.
- [x] P.6.7 Smaller, independent, each with its own discriminating test. Split
      into one id per item so a commit closes exactly one: all eleven are closed,
      each by its own commit and its own discriminating test.
  - [x] P.6.7.1 #212 — one junction dot per wire instead of per T. Done: a
        single `add_missing_junctions` helper in `sch_wiring.rs` guards on a
        coincident dot, and the three unguarded loops call it —
        `handle_add_wire`, `handle_batch_add_wire`, `handle_connect_to_net`
        (upstream had the same three). Each was already followed by a
        correctly guarded mid-segment-pin loop, now folded into the same
        helper. Discriminating tests: a rail with three taps, drawn wire by
        wire and again as one batch — red before at **3 dots stacked on the
        first T**, one dot per T after.
  - [x] P.6.7.2 #213 — `#PWR{count+1}` re-issues a live designator. Done:
        `next_pwr_number` collects the numbers actually in use and hands out
        the lowest free one, so a deletion's number is refilled rather than
        skipped; `add_power_symbol`'s own description now says so. Red before
        at `["#PWR001", "#PWR003", "#PWR003"]` — three symbols added,
        `#PWR002` deleted, and the fourth add duplicated `#PWR003`.
  - [x] P.6.7.3 #214 — deleted wires leave orphaned junction dots. Done:
        `prune_orphaned_junctions` drops the dots a removed wire left with
        nothing to justify them, on both `delete_schematic_wire` and the batch
        path (which reports `junctions_pruned_count`); `locate_wire_for_delete`
        and `wires_in_ranges` come with it. `locate_wire_for_delete` is adapted
        rather than copied — this fork resolves the uuid through
        `find_schematic_item_by_uuid`, not upstream's standalone block scan.
        `split_wire_at_point` no longer routes through `handle_delete_wire`:
        its two halves cover the same segment, so every dot stays justified,
        and going through the pruning path would drop dots in the gap between
        the delete and the re-insert. It also becomes a single write.
        Conservation rule, and it is tested in both directions: a dot needs two
        wires, or one wire plus a pin landing mid-segment. Red before at
        `[(63.5, 50.8), (25.4, 25.4)]` against `[(25.4, 25.4)]` — the ghost dot
        survives the delete; and with the rule dropped the other way, the
        mid-segment pin's dot is wrongly pruned (`[]` against
        `[(101.6, 76.2)]`).
  - [x] P.6.7.4 #274 — pad count and courtyard by substring. Done:
        `get_footprint_info` reads all three properties off the parsed
        footprint — `find_all("pad")`, a `layer` of `B.CrtYd`/`F.CrtYd` on a
        direct child, `find_all("model")` — instead of probing the source text.
        Red before on a reduced but unrewritten KiCad 10 stock footprint (tabs
        and CRLF): `pad_count` **0 instead of 6**, and a footprint whose only
        mention of the courtyard is inside its `descr` reported
        `has_courtyard: true`.
  - [x] P.6.7.5 #140 — net and track counts by substring. Done:
        `count_nets_and_tracks` in `manufacturing.rs` reads the parsed tree —
        nets through `konnect_sexp::net::count_distinct_nets` (P.6.5's shared
        accessor, so a KiCad 9 net counts once rather than through its
        declaration *and* every reference), tracks as the direct
        `segment`/`via`/`arc` children of `(kicad_pcb …)`. Arcs are counted
        there and not by walking, since `(arc …)` also appears inside a zone
        outline's `(pts …)` — a polygon corner, not routed copper, and its own
        test. Red before, measured end to end: a routed KiCad 10 board reported
        `net_count` **0 instead of 2** and `track_count` **0 instead of 4**,
        and an unrouted one reported 0 nets instead of 4 — so the
        `net_count > 3 && track_count == 0` guard could never fire, and the
        last check before fabrication passed a board that was never routed.
  - [x] P.6.7.9 `validate_for_manufacturing` counted copper layers by substring
        too: `content.matches("signal)") + content.matches("signal \"")`
        (`manufacturing.rs`). Found while closing P.6.7.5, not in upstream's
        audit. KiCad marks copper with four kinds — `signal`, `power`, `mixed`,
        `jumper` — so a board using `power` for a plane was undercounted, and
        the probe also matched the word anywhere else in the file. The `.Cu`
        suffix is the invariant, and `konnect_sexp::layers` already decides by
        it (P.6.6); `Layer::is_copper` was checked first to confirm it does.
        Measured on the installed KiCad 10 demo corpus before writing the test,
        and both directions are real: `complex_hierarchy` 1 against 2 (one
        `power`), `One-Air-Max` 2 against 4 (two `power`),
        `jetson-agx-thor-baseboard` 9 against 10 (one `jumper`), and
        `multichannel_mixer-unrouted` **11 against 2** — the word counted all
        over the file. The dead `let _layers = …` binding went at the same time.
        Second site, outside this item's wording and found by that measurement:
        `handle_estimate_cost` carries the same probe as its fallback when the
        `layers` argument is absent, and there the count picks the **price**
        bracket. Both are routed through `konnect_sexp::layers` now. `.max(2)`
        stays but changes job: it is no longer compensating for a miscount, it
        is the floor for a board whose `(layers …)` genuinely cannot be read,
        where 0 would price as free.
        Fixture `unrouted_power_planes.kicad_pcb`, derived from
        `unrouted.kicad_pcb` so it keeps that file's version and its old
        ordinal scheme (D117): `F.Cu` signal, `In1.Cu`/`In2.Cu` power, `B.Cu`
        signal, plus a net named `TEST_signal)_PROBE` so the fixture is wrong
        in *both* directions at once, as the real boards are. `kicad-cli`
        10.0.3 loads it — the honest oracle, and the check D111 exists to
        force.
        Red before, on both: `left: Number(3) right: 4`. The cost half printed
        the consequence in the open — 3 fell through to the `_ =>` pricing
        branch and quoted `pcb_fabrication: $30.00` where the 4-layer bracket
        is $7.00.
  - [x] P.6.7.6 #139 — `export_bom` ignores `exclude_dnp` and `format`, both
        advertised in its schema. Done: `BomOptions` and a `bom_args` the flag
        can be asserted against without KiCad; the handler reads `exclude_dnp`
        (schema default `true`) and passes `--exclude-dnp`. Ground truth first:
        `kicad-cli sch export bom --help` on 10.0.3 has **no `--format` flag at
        all** — it offers `--fields`, `--labels`, `--group-by`, `--sort-field`,
        `--filter`, `--exclude-dnp` and delimiters. So `format` was not given
        an invented mapping: it is a closed set of `"csv"`, declared as an
        `enum` in the schema and refused otherwise, rather than accepted and
        dropped. `manufacturing.rs`'s package keeps its prior behaviour through
        `BomOptions::default()` (DNP included), since its own schema exposes no
        such knob. Note the deliberate contract change: `export_bom` called
        without `exclude_dnp` now honours the `true` its schema always
        advertised, so DNP parts stop appearing by default.
        Red before on the only honest oracle — the CSV KiCad writes: with
        `exclude_dnp: true` the DNP part **R2 was still in the BOM**.
  - [x] P.6.7.10 `export_bom` exposed none of `--fields`, `--labels` or
        `--group-by`, which `kicad-cli sch export bom --help` does offer
        (verified on 10.0.3 while closing P.6.7.6). Upstream's #139 carried
        them; this fork's item named only `exclude_dnp` and `format`, so they
        were left out rather than folded in silently. This item asked to decide
        before implementing: **yes, expose all three.** Without `--fields` the
        BOM comes out as KiCad's default `Reference,Value,Footprint,QUANTITY,DNP`
        — no MPN column, no LCSC column, so a BOM nobody can order from, on a
        server that carries a whole sourcing toolset.
        Form follows P.6.7.7's `--layers`: arrays of strings in the schema,
        joined into the single comma-separated value the CLI takes. A field
        left out pushes no flag at all, so kicad-cli applies its own default
        rather than one this repository invented — and the schema descriptions
        quote those real defaults.
        Three behaviours measured against 10.0.3 before any code, because each
        decided whether a guard was owed. Two said no:
        * `--fields` longer than `--labels` does **not** shift columns — the
          unlabelled column takes the field's own name as its header; more
          labels than fields simply ignores the extras;
        * `--labels` without `--fields` applies to the leading default fields
          and leaves the rest with their default labels.
        Guarding either would be a guard that guards nothing (D126). The third
        said yes: `--group-by` naming a field absent from the effective field
        set is accepted, exits 0, and **silently groups nothing** — measured
        with two resistors sharing a value that stayed unmerged. So that one is
        refused server-side, before kicad-cli is spawned, as an
        `invalid_argument` on `group_by` naming the exported fields; `${}`
        delimiters are normalised first, since the CLI accepts a field either
        way. The default set is mirrored in our code to make that check
        possible, which couples us to a CLI default — stated rather than
        hidden.
        Red before: the unit half by `BomOptions` failing to compile against
        the new argument vector, the guard half with the check disabled. The
        honest oracle is a `#[ignore]`d probe like P.6.7.6's — a temporary copy
        of the fixture with an `MPN` field added to R1, exported with a custom
        label, and the column checked in the CSV KiCad actually wrote. Run here
        against 10.0.3: passes. `BomOptions` loses `Copy` (it now holds
        `Vec<String>`), which no caller depended on, and `manufacturing.rs`
        keeps its behaviour through `default()`.
  - [x] P.6.7.7 #266 — `--layers` repeated per layer, no `--mode-single`.
        Done: `single_file_pcb_export_args` joins the layers into the one
        comma-separated value KiCad 10 takes and asks for `--mode-single`, so
        `--output` is the file the caller named rather than a directory;
        `export_pdf` and `export_svg_pcb` both go through it. An empty layer
        list passes no `--layers` at all, since `--layers ""` would ask for
        nothing. `cli_failure_diagnostics` is the other half and is worth
        having on its own: kicad-cli writes argument errors to **stdout**, and
        the error path reported stderr only.
        Ground truth, measured directly on 10.0.3 before coding: `--layers` is
        documented `[nargs=0..1]` as "Comma separated list", and repeating it
        exits 1 with "Duplicate argument --layers" on stdout and **an empty
        stderr**. Red before, on the live probe: `export_pdf` with two layers
        failed, and with the old stderr-only diagnostic the whole message read
        `kicad-cli exited with 1:` — nothing at all. The pre-existing board
        export probe passed throughout because it asks for a single layer; it
        takes two to make the duplicate.
  - [x] P.6.7.8 #263 — `run_erc` on a sub-sheet reports invocation artefacts.
        Reproduced against KiCad 10.0.3 before writing anything, on a copy of
        `demos/complex_hierarchy`: `sch erc` on the sub-sheet `ampli_ht`
        returns **67 violations, 46 of them `lib_symbol_issues`** ("the current
        configuration does not include the symbol library"), against **0** on
        the project's own root sheet. Confirmed as described.
        Done: `owning_project_root` recognises a sheet that belongs to a
        project rooted elsewhere and `run_erc` refuses it with a structured
        `invalid_argument` on `schematic` naming the root to retry against.
        The audit is wrong on one point — `project_root_for` does **not**
        exist in this fork's `library.rs` — so nothing was made `pub(crate)`
        there; only `MAX_HIERARCHY_DEPTH` was, and the detection looks in the
        file's own directory rather than walking ancestors. Stated bound: a
        sheet moved out of its project's directory is not caught, and that is
        deliberate, not a gap to fix silently.
        The refusal is the only behaviour change, and the reverse bound is
        tested as explicitly as the defect: a root, a project-less schematic, an
        unreferenced neighbour and a directory holding several projects are all
        left alone. Red before: the sub-sheet resolved to `None`, so no refusal
        was raised at all.
  - [x] P.6.7.11 The measurement P.6.7.8 rests on lives only in a comment.
        The refusal is decided before `kicad-cli` is reached, so the unit tests
        prove the server's own logic and no live probe was owed — but nothing
        in the suite would notice if a future KiCad stopped producing those
        `lib_symbol_issues`, which would leave the refusal unjustified and
        invisible. Same shape as D113. A probe over a copied demo hierarchy,
        asserting the sub-sheet/root asymmetry rather than an absolute count,
        would anchor it. Decide whether it belongs in the gating E2E job.
        Done: `erc_on_a_sub_sheet_reports_library_artefacts_its_root_does_not`
        (`tests/cli_tools.rs`) goes around the server's own refusal on purpose
        — it calls `cli::run_erc` directly on both sheets of a copied
        `complex_hierarchy` — and asserts the asymmetry: zero
        `lib_symbol_issues` on the root, at least one on `ampli_ht`. Re-measured
        here on 10.0.3 before writing it, and reproduced by the probe itself:
        **0 violations on the root, 67 on the sub-sheet, 46 of them
        `lib_symbol_issues`** — P.6.7.8's numbers exactly. The counts are
        printed, not asserted (D113: a probe must show what it measured), since
        the totals move with the demo's cleanliness and with KiCad's rule set
        while the asymmetry is all the refusal claims. Second half closes the
        loop the first opens: the artefacts are still produced, *and* the
        server still refuses that sheet naming the root to retry against —
        which is what turns "KiCad changed" into "we now block a call that
        works". Red before, by pointing the sub-sheet call at the root: the
        `child_issues > 0` assertion fires with its own message.
        The demo copy is now one helper, `copied_complex_hierarchy`, shared
        with P.6.9.3's probe rather than a second hand-written loop (D136).
        Decision on the gating job: **yes, in `e2e`**. A red here is a
        statement about the artifact's own behaviour — it refuses a legitimate
        call — not about runner liveness, which is the criterion that keeps
        `live-ipc` advisory. The other candidate homes do not fit: `live-ipc`
        is the IPC path and this is kicad-cli, and there is no non-gating CLI
        job to put it in.
- [x] P.6.8 `LATER` items — #271, #179, #185, #148, #186, #138, #162 — each
      carries its precise next action in `docs/upstream-audit.md`; re-read it
      rather than re-deriving. #271 depends on P.6.3, which is closed, so
      nothing here is blocked any more. Split into one id per item, ordered by
      consequence and not by issue number, so a commit closes exactly one — the
      shape P.6.7 and P.6.9 used. All eight `LATER` items are closed, and so is
      P.6.8.9, which belongs to no upstream issue: it was found while measuring
      P.6.8.5.
  - [x] P.6.8.1 #179 pin half — fifteen call sites still resolve pins with the
        unit-blind `extract_lib_pins`, and `sch_batch.rs:390-399` finds an
        instance by `reference` alone. Together they compute unit 2's pin
        against unit 1's placement transform, so `batch_connect_to_net` on the
        second half of an op-amp drops its net label at the wrong coordinate.
        Wrong connectivity, not a wrong report — first for that reason. The
        unit-aware extractor already exists here
        (`konnect_sexp::schematic::extract_lib_pins_for_unit`, and
        `SymbolInstance::unit` carries the number), so this is a sweep plus a
        candidates loop, not a port. No fixture in this repo has a multi-unit
        symbol at all: the discriminating test needs one built from a real
        KiCad library symbol.
        Done. `batch_connect_to_net` now collects **every** instance carrying
        the reference and keeps the one whose own unit declares the requested
        pin, and all fifteen call sites take
        `extract_lib_pins_for_unit(sym, inst.unit)`. The mixed verdict this
        task expected did not survive contact: every one of the fifteen already
        had a `SymbolInstance` in hand, so none was inspecting a library
        definition outside a placement and none was left blind.
        `extract_lib_pins` itself is untouched — it has legitimate callers.
        The fixture is the real `Amplifier_Operational:LM2904` copied out of
        the installed library, with `U1` placed twice (unit 1 at x=100, unit 2
        at x=160); `kicad-cli sch erc` accepts it — 12 unconnected-pin
        violations, exit 0. What makes it discriminating is a coincidence in
        KiCad's own symbol: unit 1's pin 3 and unit 2's pin 5 sit at the same
        local point, so the defect did not produce an obviously wrong result,
        it produced a plausible one.
        Red before: with the lookup neutered the pin-5 label lands at
        **x=92.38** — unit 1's pin 3 — instead of **x=152.38**; with the
        unit-aware extractor reverted, `get_component_nets` reports **8 pins
        for unit 1 instead of 3**. The mirror is asserted too (pin 3 on unit 1
        does not move), and a pin on no unit still errors, naming the units
        tried; the "no library symbol resolved at all" case was split off from
        that message, since "units tried: []" described the wrong problem.
        `docs/capability-matrix.md` moved one line, exactly D128's case: the
        new unit test is a lexicographically smaller evidence source for
        `get_component_nets` than `tests/nets_and_wires.rs`. Status unchanged
        (`PARTIAL`, `test`), so it is regenerated rather than fought.
  - [x] P.6.8.2 #179 edit half — `find_symbol_instance_block`
        (`tools/mod.rs:651`) and `sch_batch.rs:321`/`:330` return the first
        match only. Port `find_all_symbol_instance_blocks` / `field_value_ranges`
        as a separate change, after P.6.8.1.
        Done, and the task turned out to need a design rather than a port:
        the four operation families do **not** share a per-unit meaning.
        Measured first, on P.6.8.1's fixture: desynchronise `Value` between the
        two units and export the netlist, and KiCad reports the **first**
        block's value — `CHANGED` when unit 1 was edited, the stale `LM2904`
        when only unit 2 was. So editing the first block alone produced a
        correct netlist *by accident* while leaving a file that contradicts
        itself: every unit past the first still shows the old value in
        eeschema, and every by-unit read — ours included — returns it. Not a
        netlist defect, a file-consistency one, and the doc says so.
        The four families: **property writes** by reference now touch every
        unit (`edit_schematic_component`, `batch_edit`,
        `add_component_annotation`, and `replace_component`'s `lib_id` — its
        `unit` stays per block, being a per-unit fact); **deletes** by
        reference remove every unit, since half a component is not a thing
        KiCad can open; **geometry** by reference is **refused** on a
        multi-unit symbol, naming the units and their uuids, because moving
        U1A without U1B is legitimate — it is all eeschema ever does — while
        silently picking the first is not; **reads** answer with the unit they
        resolved, plus the sibling uuids for `get_schematic_component`.
        That refusal is INV8's **first** clause, not a breach of its second:
        an input with two meanings stays refused, and the second clause
        governs widenings, where this is the withdrawal of an acceptance that
        should not have been granted. The test that pinned the old behaviour
        (`move_by_reference_still_addresses_the_first_unit`) was pinning the
        defect; it is rewritten, not deleted, and carries that reasoning.
        `find_all_symbol_instance_blocks` is the one definition of "an
        instance block" and `find_symbol_instance_block` is now its `.next()`;
        `symbol_block_uuid` likewise moved to `tools/mod.rs`, since the
        single-symbol refusal and the batch one were reading uuids by two
        copies of the same search (D136). Property edits are applied in
        descending offset order, so an earlier block's range is never
        invalidated by a later block's rewrite.
        Red before, verified again after the fact: with the multi-block write
        truncated to one block, `editing_value_by_reference_updates_every_unit`
        fails and the other five stay green.
        Stated bound: `bulk_move`, `batch_edit` and `add_component_annotation`
        translate a `uuid` to a `reference` before acting — pre-existing, not
        introduced here — so a uuid-addressed `bulk_move` on a multi-unit
        symbol is refused with the rest. The refusal names
        `move_schematic_component` with a `uuid`, which does reach one unit.
  - [x] P.6.8.3 #271 — `find_orphan_items` counts wire endpoints and label
        positions and nothing else, so a wire ending on a pin is reported
        dangling and an unconnected pin is never reported at all: false
        positives and false negatives on any sheet with components. ~470 lines
        plus two `konnect-sexp` extractors; needs `LibPin::electrical_type`,
        which P.6.3 landed.
        Done, and far cheaper than the audit's ~470 lines: this fork already
        had the machinery upstream had to build. `find_isolated_pins` — the
        finder behind `find_single_pin_nets` — already resolved pins per unit,
        honoured `no_connect` markers and asked the net graph, so the fix is to
        *use* it rather than port `PointIndex`/`WireIndex`. What was extracted
        is `placed_pins`, one definition of "the pins this sheet really has",
        now shared by both (D136).
        Measured against 10.0.3 before writing anything, on the new fixture
        `orphan_items.kicad_sch`: `sch erc` reports exactly three
        `pin_not_connected` — R1 pin 2, R2 pin 1, R2 pin 2 — and leaves R1
        pin 1 out because a wire ends on it; `label_dangling` for the label in
        empty space; `isolated_pin_label` for the one sitting mid-wire, which
        is KiCAD saying that one *is* attached. A second probe settled the
        mid-segment question: a wire end touching another wire's **body** is
        `unconnected_wire_endpoint`, and adding the junction makes it go away —
        so touching a body rescues a pin or a label, never another wire's end.
        The handler now reports `unconnected_pin` alongside the two old kinds,
        stops calling a wire end on a pin dangling, and stops calling a
        mid-wire label floating — the net graph carries every label as a node
        of its own, so asking it whether a label is attached always answered
        yes, and the geometric test `on_a_wire` is the one that means
        something.
        Red before, each half neutered in turn: without the pin rescue the
        wire's pin end comes back as a second `dangling_wire_end`; without the
        pin finder the three unconnected pins vanish from the report.
        Stated bound: ERC's own rule names are not modelled. `wire_dangling`
        fires on these fixtures in cases the three measured facts do not
        explain, and its `pos` is the wire's anchor rather than the offending
        end; this tool reports geometric attachment, which is what "orphan"
        means here, and `run_erc` stays the verdict (E7).
        `docs/capability-matrix.md` moves one line again (D128): the new test
        lives in `nets_and_wires.rs`, lexicographically ahead of
        `symbols_and_schematic.rs`. Status unchanged.
  - [x] P.6.8.4 #185 — `run_design_review` runs its four audits against the
        single `schematic` path and derives the verdict from finding counts, so
        "LOOKS GOOD" is what a caller gets when the audits inspected one sheet
        of twelve. Decide **first** whether the verdict belongs in
        `design_review` or in this fork's `evidence/` validators — upstream has
        no such layer — then port the coverage structs. Same principle as
        P.6.9.16, different evidence.
        Decided, then done: **the verdict stays in `design_review`**, not in
        `evidence/`. The precedent was already in the same function —
        `drc_incomplete` turns a missing DRC pass into `INCOMPLETE` from
        evidence the handler gathered — and `evidence/` serves the gating
        validators, a different consumer. The coverage gap is the same shape of
        fact, so it takes the same shape of answer.
        The four schematic audits now run once per reachable sheet, every
        finding carries the sheet it came from, and the review reports
        `schematic_coverage` beside `drc`: sheets reachable, sheets audited,
        and each sheet a `(sheet …)` reference named but could not load, with
        the reason. A coverage gap makes the verdict
        `INCOMPLETE — sheet(s) not audited: …`, **naming** them, and it sits
        above warnings for the same reason `drc_incomplete` does: a warning
        must not stand in front of a review that did not look.
        No fourth walker: `reachable_sheets` is added once in `tools/mod.rs`
        and `sch_export::sheet_tree_contains` is reduced to a call on it
        (D136). That brings a stated widening — the walk includes the root, so
        a target that *is* the root now answers true; its single caller,
        `owning_project_root`, has already returned for that case before
        asking, and P.6.7.8's tests hold unchanged.
        No hierarchical fixture existed in this repo; four were built, with the
        defect on a sub-sheet only, and `kicad-cli sch erc` accepts the root.
        Red before: with the walk truncated to the root, the sub-sheet defect
        goes back to `LOOKS GOOD` with an empty finding list, and the missing
        sub-sheet stops being reported. Single-sheet reviews are unchanged —
        the nine existing tests never moved.
  - [x] P.6.8.5 #148 — the net-label stub direction defaults to `"right"`
        (`sch_wiring.rs:1975`), so connecting a left-edge pin drives the stub
        across the symbol body, over other pins, and the mid-segment-pin loop
        then plants junction dots on them. The `justify` half is already done
        here. Port `pin_outward_at`/`stub_direction` as the default, leaving an
        explicit `direction` authoritative.
        Done. `pin_outward_at` derives the direction from the pin itself — the
        dominant axis of `pin - instance_origin` — and needs no rotation or
        mirror bookkeeping of its own, because both points are already in sheet
        space: the vector between them points outward in whatever orientation
        the symbol was placed in. A caller's explicit `direction` stays
        authoritative, and a coordinate with no placed pin under it keeps the
        historical `"right"`. The response says which of the three happened
        (`direction_source`: `requested` / `derived_from_pin` /
        `default_no_pin_here`) — a silent default and a derived direction must
        not read alike to a caller.
        The lookup runs against the caller's own `(pin_x, pin_y)` **before**
        `snap_reporting`: real pin positions are what `placed_pins` holds, so
        looking up the snapped point missed every pin whose sheet was not
        grid-exact.
        No fourth walker (D136): `placed_pins` moves from `sch_analysis` to
        `tools/mod.rs`, where `all_pin_endpoints` was the same "for each
        instance, for each pin of its unit" loop, and becomes that function's
        body. It gains `origin_x`/`origin_y`, which is what the direction is
        measured against.
        Fixture built for it, because none here had two rows facing opposite
        ways: `conn_double_row.kicad_sch`, a real
        `Connector_Generic:Conn_02x05_Odd_Even` as `J1`. Positions are
        `kicad-cli sch erc`'s, not assumed — odd pins at x = 96.52, even pins
        at x = 109.22, so pin 9 and pin 10 share y = 101.6; the sheet loads
        clean, with 10 `pin_not_connected` errors and nothing else.
        Red before, with the derivation neutralised: the three derived-default
        tests fail, and on the double-row sheet the stub off pin 9 runs through
        the body to x = 111.52 and the mid-segment pass writes a junction at
        `(109.22 101.6)` — exactly on pin 10. That is the defect, observed in
        the file rather than argued from the code.
  - [x] P.6.8.6 #138 residual — the doc comment above `export_drill` claims the
        directory form of `--output` was verified against KiCAD 10, while
        upstream appends a trailing separator because kicad-cli otherwise reads
        the last component as a file name. The two claims contradict each other
        and only a kicad-cli run settles it. Separately: `separate_th` defaults
        to `false` here, where upstream always separates because a single
        `MixedPlating` file distinguishes NPTH by a comment most Excellon
        readers drop.
        Done, and both halves are settled by measurement rather than by
        reading a diff.
        The trailing separator is **not needed** on 10.0.3: all four cases —
        output directory present or missing, with and without a trailing
        separator — write `<board>.drl` *inside* the named directory, creating
        it when absent, for drill and for gerbers alike. This fork's doc
        comment was right, upstream's `MAIN_SEPARATOR` workaround does not
        apply to this version, and `a_file_path_as_output_would_have_become_a_directory`
        was already the standing proof. No code change; the doc now carries the
        measurement and says what upstream does differently.
        `separate_plated` now defaults to **true**, but not in
        `DrillOptions::default()`, which must keep mirroring `kicad-cli` — two
        live probes assert KiCAD's own defaults through it. The policy lives in
        one place instead, `cli::SEPARATE_PLATED_HOLES` and
        `cli::fab_drill_options()`, shared by the three paths that hand a
        fabricator a file: the `export_drill` tool, the Gerber export's drill
        companion, and the manufacturing package (D136 — one definition, or it
        drifts).
        What decides it, measured on a board carrying one plated `thru_hole`
        and one `np_thru_hole`: in a single file the two are told apart **only**
        by a comment line above the tool definition
        (`; #@! TA.AperFunction,NonPlated,NPTH,ComponentDrill`), while the body
        is plain Excellon — `T1`/`T2` and coordinates, no plating information
        at all. A reader that drops comments plates the mounting hole. With
        `--excellon-separate-th` the distinction moves into the file itself,
        `-PTH.drl` and `-NPTH.drl`, each with its own `TF.FileFunction`. The
        failure mode of one file is silent and physical; of two, a fab receives
        a file it already knows.
        Tests: the published schema default must equal the constant the
        handler falls back on — a divergence there would be P.6.9.15's defect
        with a physical consequence — plus a lexical guard that the fallback is
        the constant and not a literal, its needle split by `concat!` so it
        cannot satisfy its own search (D133). Live probe added:
        `fab_drill_options` writes `-PTH`/`-NPTH`. Measured and not assumed:
        KiCAD writes **both** files even on a board with no non-plated hole.
  - [x] P.6.8.7 #162 — `query_traces` emits no uuid while `delete_trace`
        requires one, so there is no path from listing a trace to deleting it.
        Twelve lines across three files; the audit says to bundle it into the
        next PCB change rather than schedule it alone.
        Done, on its own after all: the twelve lines are a decode, and their
        proof is a mock-server round-trip that no other PCB change would have
        carried. `IpcTrack` gains `uuid: Option<String>`, filled from the
        `Track` message's own `id` — KiCAD sent it all along and the decode
        dropped it — and `query_traces` puts it first in each entry. `Option`
        rather than an empty string on purpose: a track without an id must not
        read as one with a usable id and be handed to a delete.
        Red before: with the field forced to `None`, the mock test fails on
        the assertion that says why the id matters.
  - [x] P.6.8.8 #186 — `Reference` and `Value` are placed at a hard-coded
        ±3.81 mm at rotation 0, whatever the symbol and whatever the placement
        rotation. Visual only, no electrical effect and no data loss: last on
        purpose.
        Done. `library::field_anchor` reads the anchor from the **embedded**
        `lib_symbols` entry — the definition this sheet will actually be drawn
        with, already flattened by `ensure_lib_symbol` — and
        `tools::push_placed_fields` runs it through `transform_pin`, the same
        transform a pin goes through: library Y-up flipped, placement rotation
        and mirror applied, translated to the instance origin, rounded to six
        decimals (D125). All four fields, at both sites that place a symbol
        (`add_schematic_component` and `add_power_symbol`), through one helper
        rather than two copies of the same four lines (D136); `mm` moves up to
        `tools/mod.rs` for the same reason.
        What decides the design is a measurement over the KiCad 10 demo
        corpus — 12 894 placed `Reference`/`Value` fields, each compared with
        the position its file actually carries:

        | rule                                        | reproduces |
        |---------------------------------------------|------------|
        | fixed `y ∓ 3.81`, angle 0 (what this did)   | 7.5 %      |
        | library anchor **transformed**               | 41.4 %     |
        | library anchor translated, rotation ignored  | 24.2 %     |

        The absolute numbers do not settle it and are not meant to: the
        remainder is fields a human dragged, which no rule reproduces and none
        should (D138). What settles it is the rotated buckets, where the old
        rule is essentially never right — 10 of 2 440 at 90° — and rotating
        the anchor beats not rotating it by more than ten to one.
        Two further properties, measured the same way but only over the fields
        the corpus shows eeschema itself placed (position reproduced exactly,
        so nobody moved them): the field's **text angle is the library's**, not
        the placement's — lib 0° → 0° in 4 906 of 5 071, lib 90° → 90° in all
        261 — and the library's `(effects …)` comes across whole, justification
        included, in the overwhelming majority. A `left`-justified reference
        written without its justify shifts by half its own width.
        Fallback kept and tested: a library entry that declares no such field
        keeps the historical offsets, which is exactly the case the old rule
        was the only rule for.
        Red before, with the anchor lookup neutralised: the three new
        placement tests and the power-symbol test fail; the fallback test
        stays green, as it must. Live: `Device:R` — whose reference anchor is
        off-axis (`at 2.032 0 90`) and whose text angle is 90, not the 0 this
        used to write — placed at 0° and at 90° on a demo sub-sheet, and
        kicad-cli still loads the hierarchy. Gating step added to
        `e2e-kicad.yml`.
  - [x] P.6.8.9 — found while measuring P.6.8.5, and not part of it: when a
        pin does not sit on the 1.27 grid, `connect_to_net` snaps the caller's
        `(pin_x, pin_y)` and then draws the stub from the **snapped** point, so
        the wire starts beside the pin instead of on it and the tool answers
        success for a connection the file does not carry. Measured on
        `multiunit_lm2904.kicad_sch`, whose `U1` is placed at x = 100: the pin
        `placed_pins` reports at x = 92.38 gets a wire starting at x = 92.71,
        0.33 mm away.
        Done, and the first thing measured was how much it matters — the
        premise that off-grid pins are exotic is **false**: across the KiCad 10
        demo corpus, **3 670 of 48 068** placed pin endpoints do not sit on the
        1.27 mm grid (7.6 %), from 127 off-grid placements out of 6 447 and
        from pin lengths that are not grid multiples. The defect is ordinary,
        not a corner case.
        The rule chosen, of the two the entry offered: **the snap yields to a
        pin actually found at the requested point**. The alternative — report
        that the wire did not start where asked — is already half-present
        (`snap_reporting` returns `requested`) and it does not help: the caller
        asked for the right point, and being told the server moved it does not
        put the wire on the pin.
        One helper, `tools::snap_unless_pin`, taking the pin list the caller
        already holds so nothing re-parses a sheet (D136). Six sites, all in
        `sch_wiring.rs`, chosen because each writes something whose *meaning*
        is the connection point: `add_wire` and `batch_add_wire` (both
        endpoints), `connect_to_net`, `add_junction`, `batch_add_junction`,
        `add_no_connect`. Deliberately not `add_power_symbol` or the component
        placers: those position a *symbol*, whose own anchor KiCad wants on the
        grid, and E6 put the snap there for a reason. `add_schematic_connection`
        never snapped at all.
        Red before, with the pin check disabled: the three off-grid tests fail
        and the live probe fails on KiCad's own count.
        Live, and this is the item's real proof: `kicad-cli sch erc` on the
        fixture reports one fewer `pin_not_connected` after a no-connect is
        placed on the off-grid pin — and with the snap restored it not only
        keeps reporting that pin but adds `no_connect_dangling` at the snapped
        point, KiCad's own name for a marker that marks nothing. Gating step
        added to `e2e-kicad.yml`.
        Measured and left alone: KiCad also warns `endpoint_off_grid` about
        such a pin. That is the sheet author's choice; moving our marker
        somewhere else was never a fix for it.
- [x] P.6.9 The 16 direct-to-`main` upstream fixes of Appendix A are triaged,
      by P.4's method and against this fork's own code: **8 BACKPORT NOW, 4
      LATER, 4 NOT APPLICABLE**, each verdict carrying a `file:line` citation
      in `docs/upstream-audit.md`. The four that do not apply are excluded
      because the mechanism is absent here, not because it was judged minor:
      this fork has no sync path at all (`2904841`, `59d0ead`), no ancestor
      walk for a `.kicad_pro` (`ec705c3`, and `rg '\.ancestors\(\)'` over the
      tree returns nothing), and no board-coverage block (`d5774b3`, whose
      `find_all` trap was swept for and found nowhere else). Three items are
      cheaper here than upstream because P.6 already landed the half they rest
      on — `f2372ca` on `konnect_sexp::net`, `977f0c5` on `DrcReport`,
      `de70351`/`8591707` on `update_field`/`insert_property` — and one is
      worse here than upstream's own starting point: `ff518c8`, whose layer
      table is shorter in this fork than in the code upstream fixed. The order
      below is by consequence and is not the table's order.
  - [x] P.6.9.1 `ff518c8` — `layer_from_name`
        (`crates/konnect-ipc/src/builders.rs:42-61`) maps every unknown name to
        `BL_UNDEFINED`, and the graphic, text and footprint-instance paths send
        it (`builders.rs:198`, `:374`, `client.rs:1398`) where the pad path
        drops it (`client.rs:1305-1307`). KiCAD indexes its layer bitset with
        whatever arrives and faults at `0xc0000005`, taking the session's
        unsaved board with it; Konnect sees only an NNG timeout. Reached by
        placing any official-library footprint carrying a `Dwgs.User` child,
        since `pcb_components.rs:353` reads the graphics out of the real
        `.kicad_mod`. Widen the table by computation (`BL_Rescue = 62` sits
        between `BL_User_9` and `BL_User_10`), add a fallible
        `try_layer_from_name`, and validate the root, pad and graphic layers
        before a single child is built. First: it is the only item that
        destroys work the tool never touched.
        Done, and not the way upstream did it: the mapping is derived
        instead of listed. The proto enum's own names *are* the KiCad names
        with `.` replaced by `_` behind a `BL_` prefix, so `from_str_name`
        answers for every layer the schema knows — inner copper to
        `In30.Cu`, `User.1` through `User.45` across the `BL_Rescue = 62`
        gap, and whatever a future KiCad adds — with no arithmetic to get
        wrong. `try_layer_from_name` refuses the three sentinels by name as
        well, since `BL_UNDEFINED` is exactly what must never be sent, and
        `build_footprint_item` checks the footprint's own layer, every pad
        layer and every graphic layer before it builds a single child.
        Measured over the installed corpus rather than estimated: **915 of
        the 15,433 official footprints (5.9%) name a layer the old
        fifteen-entry table did not know** — `Dwgs.User`, `Cmts.User`,
        `F.Adhes`, `Margin`, `User.2`, and every inner copper layer past
        `In2.Cu`. That measurement is now
        `crates/konnect-ipc/tests/layer_corpus_test.rs`, in the gating E2E
        job, so the next name KiCad adds fails a test instead of an editor.
        The corpus found what the earlier sample had not: `*.SilkS`, a
        fourth pad-layer wildcard nobody had expanded (NPTH pads in
        `Connector_RJ` and `Heatsink`). It was silently dropped from the pad
        before, and once layers are validated it would have failed the whole
        placement instead, so it is expanded to both silkscreen sides here.
        Red before, each half neutered in turn: the derived table (four
        tests), the validation (one), the `*.SilkS` expansion (one).
        Stated bound: no live probe. The upstream measurement is KiCAD
        faulting at `0xc0000005`, which needs a GUI session with the API
        enabled, so the assertions are on what leaves this process — the
        layer actually emitted, and the refusal that stops an
        unrepresentable one.
  - [x] P.6.9.2 `f2372ca` — zone net references written as net 0. Two private
        `find_net_id` copies resolve a net name by string offset
        (`pcb_board.rs:113`/`:909`, `pcb_routing.rs:52`/`:546`) and a KiCad 10
        board has no ids to find, so every pour is written
        `(net 0) (net_name "GND")` onto the unconnected pseudo-net and reported
        as success. Write-side counterpart of P.6.5's read-side fix, so the
        shape detection to reuse is already in `konnect_sexp::net` (D115): add
        the write-side sibling, emit plural `(layers …)` on KiCad 10, refuse a
        net a legacy board does not declare instead of zeroing it, and delete
        both copies. Upstream's second half — refusing the edit when KiCAD
        holds that board open — is a separate task, not this one.
        Done, with one correction to this task's own text, measured on
        KiCad's demos before writing anything: `(layer …)` versus
        `(layers …)` is **not** a difference between the two file forms. It
        is a matter of how many layers the zone covers — `vme-wren`
        (20241229) writes both — so a single-layer pour, which is all these
        tools can make, stays singular on every board. What does differ is
        the net node, and by more than the id: `pic_programmer` (20260206)
        writes `(zone (net "GND") …)` with **no** `net_name` sibling, while
        `StickHub` (20250907) and `CM5_MINIMA_3` (20250513) write
        `(net <id>)` with the name in a sibling `(net_name …)` — not
        `(net <id> "<name>")`, which is the pad form. See D121.
        `konnect_sexp::net` gained `NetRef`, `net_ref_for_write` and
        `NetRef::zone_tokens`, sharing P.6.5's by-shape discriminant so read
        and write cannot disagree; both `find_net_id` copies are gone; and
        `add_zone` reports `net_id` only on a board that has one, since 0
        named the unconnected pseudo-net as though the zone had landed there
        on purpose.
        The refusal is scoped to the form that can justify it: a legacy
        board that does not declare the net is refused, naming `add_net`,
        and the file is asserted byte-identical afterwards; a table-less
        board declares nothing, so a name it has never seen is written as-is
        and KiCad creates the net on load — both directions tested.
        New fixture `kicad10_no_net_table.kicad_pcb`, a 20260206 board with
        nets named on the pads, verified to load in kicad-cli 10.0.3 (0
        errors, 0 unconnected) before being used as an oracle.
        Red before: `net_ref_for_write` neutered to the old zero-or-id
        behaviour fails three of the four integration tests.
        Live probe added to the gating job: a pour on that board leaves a
        file kicad-cli still opens. Its bound is stated in the test — it
        proves file validity, not that the copper is electrically on GND,
        which DRC cannot show on a net with a single pad.
  - [x] P.6.9.3 `e7b0c54` — a child sheet's `(instances (project … (path …)))`
        is keyed to the child instead of the root: `project_name_for`
        (`tools/mod.rs:452`) returns the file's own stem and `ensure_root_uuid`
        (`:497`) its own uuid, used as the whole path. Both are right on a root
        sheet and name nothing KiCad matches on a sub-sheet, so every symbol
        placed there reads as unannotated while the tool reports success. Sites:
        `sch_components.rs:492`, `sch_batch.rs:468`, `sch_wiring.rs:1754`.
        Resolve the sheet's real place in its project — nearest `.kicad_pro`,
        its sibling root `.kicad_sch`, then a depth-bounded walk recording each
        stepped-through `(sheet …)` uuid — reusing `owning_project_root`
        (`sch_export.rs:582`, P.6.7.8) and widening its directory-only bound if
        the measurement requires it. Anything unresolvable falls back to
        today's standalone behaviour, which must stay tested.
        Done. `sheet_instance_context` resolves the sheet's place in its
        project — nearest `.kicad_pro` for the name, its sibling root
        `.kicad_sch` for the head uuid, then a depth-bounded backtracking
        walk that records each stepped-through `(sheet …)` uuid — and
        `instance_targets` is the single place the three write sites now ask
        for the answer, falling back to today's derivation when nothing
        resolves. `owning_project_root`'s directory-only bound was **not**
        widened: the same `project_root_schematic` is reused, so a sheet
        moved out of its project's directory still keeps standalone
        behaviour, and that stays a stated bound rather than a silent gap.
        Beyond the upstream fix, from reading the demo rather than the
        commit: `complex_hierarchy` places `ampli_ht.kicad_sch` **twice**,
        and its symbols carry one `(path …)` per placement. Upstream builds
        a single path; a symbol written with one of two is annotated in one
        instance and invisible in the other, so this emits one entry per
        placement, sorted so two identical calls produce identical files.
        Measured, and it corrects this task's own impact claim: with the old
        derivation restored, `kicad-cli sch erc` reports **zero** violations
        (it does not run the annotation check) and the exported netlist
        **still lists** the symbol, because KiCad falls back to the
        Reference property when no instance path matches. The real
        consequence is per-instance annotation inside eeschema, which no CLI
        available here can observe. The live probe was therefore rewritten
        to claim only what it proves — that KiCad accepts a two-path block
        and still builds a netlist containing the symbol — and its doc
        comment records the measurement so nobody re-derives it.
        Red before: `sheet_instance_context` neutered to `None` fails three
        of the four tests in `tests/sheet_instances.rs`.
  - [x] P.6.9.4 `f8a8db0` — every typed write reformats the whole sheet. The
        writer indents two spaces where KiCad writes tabs
        (`konnect-schematic-editor/src/sexp/writer.rs:109-113`), collapses each
        closing paren onto the last child (`:104`), and inserts blank lines
        before 22 tags (`:3-24`) where a KiCad sheet has one, at the end.
        `Schematic::overwrite` (`schematic/mod.rs:163-165`) sends the whole
        document through it, and ~20 production sites reach it, so a one-line
        edit arrives as a few-thousand-line diff. Sniff the indent unit at load
        and carry it on `Schematic`; fix the paren and the blank lines. The
        largest item of the eight and the only one whose blast radius is every
        byte-level assertion in the suite — land it alone, with upstream's
        demo-corpus reduction as the acceptance number. KiCad's width-based
        packing of `(xy …)` inside `(pts …)` stays out of scope, and the task
        must say so rather than leave it looking forgotten.
        Done. `WriteStyle { indent, crlf }` is sniffed from `original_source`
        in `Schematic::from_sexp` and carried on `Schematic`; `save` and
        `to_source` go through `write_styled`, while `write` keeps the
        default (tab, LF) for the six sites that serialize a bare fragment
        outside any file. `BLANK_BEFORE` is gone and the multi-line branch
        closes on its own line at the parent's depth.
        A **fourth** cause, not in this task's own statement and found by
        measuring rather than by reading the commit: every KiCAD 10 demo
        sheet shipped by the Windows installer is **CRLF**. Writing LF into
        one reproduces this task's exact symptom — the whole document in the
        diff — by a different axis, and would also have flattered the
        acceptance measurement, since `str::lines()` drops a trailing `\r`.
        It is therefore its own field on `WriteStyle`, and its own tests: the
        demo-corpus measurement cannot see it. See D123.
        Measured on eight demo sheets, one per demo project, `add_junction`
        then `to_source`, counted as insertions+deletions over an LCS: before
        170.71%–175.97% of lines (over 100% because near-every indented line
        differs in content *and* position, so each counts twice); after
        3.18%–17.22%. Bound asserted at 25%, set from the measured range and
        not from upstream's 3151→360 on a corpus we do not have.
        Residual, characterised rather than assumed: a no-op round-trip of
        `ecc83-pp.kicad_sch` differs on 315 of 3545 lines, and every one of
        them is either an unpacked `(xy …)` — the out-of-scope divergence,
        documented in the writer — or the two lines of `(embedded_fonts no)`
        moving, which is `to_sexp`'s child order, not the writer. Nothing is
        lost or duplicated.
        Red before: the demo measurement fails on the first sampled file
        (`CM5.kicad_sch`, 175.97%); neutering `sniff_write_style` to the
        default fails the CRLF and indent-unit tests while the paren and
        blank-line tests stay green, which is the split those tests exist to
        make.
        Live probe added to the gating job: a sheet re-laid in eeschema's own
        formatting, round-tripped through the typed model with a one-element
        edit, is still a sheet KiCAD loads and builds a netlist from. The
        conformance suite measures how little the new shape disturbs; only
        KiCAD can say the shape is legal.
  - [x] P.6.9.5 `de70351` — two text-path handlers never got the fix their
        typed sibling has. `add_component_annotation`
        (`sch_components.rs:1432`) appends a `(property …)` unconditionally, at
        a hardcoded `(at 0 0 0)` and a hardcoded indent (`:1477`), so a repeated
        key leaves two fields with one name and the text renders at the sheet
        origin; and it does not refuse the reserved keys, so a `Reference` set
        this way skips the instances rewrite. `bulk_move` (`sch_batch.rs:706`)
        rewrites only the symbol's own `(at …)` (`:747-757`) while property
        coordinates are absolute, so field text stays where the part used to
        be. Lift the in-place branch out of `edit_schematic_component`
        (`update_field` `:795`, `insert_property` `:825`) into a shared helper;
        move each property anchor by the delta the symbol actually moved — the
        snapped one — leaving its rotation alone, and locate property blocks
        with a string-aware scan. Stay on the `SexpEdit` path: the typed model
        would import P.6.9.4's reserialisation.
        Done. `tools/mod.rs` gained the shared scanners — `symbol_property_blocks`,
        `quoted_string_after`, `find_symbol_property`, `symbol_property_at_spans`,
        `symbol_insertion_site`, `set_symbol_property` — all walking by nesting
        depth and quote/escape state through `find_direct_child_blocks`, never
        by substring. `update_field`/`insert_property` collapsed into `set_field`
        over the same helper, so both paths update-or-insert identically and
        the naive `find` that a property *value* containing `(property "` could
        derail is gone.
        Reserved keys are **`Reference` alone**, and the narrowing is the
        finding, not a concession: it is the only key stored twice — in the
        property *and* in `(instances …)` — so it is the only one this generic
        path can desynchronise. `Value`/`Footprint`/`Datasheet` have a dedicated
        argument on `edit_schematic_component` but no second copy, and the BOM
        audit legitimately sets `Footprint` through this tool. A four-key list
        broke `the_bom_audit_finds_missing_footprints_and_lets_go_when_they_are_assigned`;
        the test was right and the list was wrong.
        Anchor for a new property, measured on `CM5.kicad_sch` rather than
        assumed: a hidden `Description` on a symbol at `(at 139.7 241.3 0)` is
        written at `(at 139.7 241.3 0)`, and on a symbol rotated 270° at
        `(at 119.38 238.76 270)` it is written at `(at 119.38 238.76 0)` — the
        symbol's own (x, y), rotation always 0. `(at 0 0 0)` was right only for
        a symbol that happened to sit at the origin. Indentation is read off an
        existing sibling, so a tab-indented sheet stays tab-indented (P.6.9.4).
        Two defects the fix introduced, caught in review and fixed with the
        task: the property anchor is a plain addition, so unlike the symbol's
        own anchor it is not covered by `snap_point`, and a field at 241.3 came
        out as `246.38000000000002` — float noise written into a file, the
        exact damage P.6.9.4 removed. Coordinates now go through `mm()`,
        rounding to six decimals: measured across 126 933 `(at …)` values in
        the demo corpus, every one but `59.209102362204725` (an inch
        conversion, not noise) carries at most four decimals, while addition
        noise appears around the thirteenth. And a move that snapped back to a
        standstill still rewrote every field, turning `(at x y)` into
        `(at x y 0)`; it now writes nothing. `add_component_annotation` also
        answered `added_property` after *updating* one, so a `created` flag now
        says which of the two happened.
        Red before: two same-key calls leave two `(property "MPN" …)`; a
        lookalike value derails `find_symbol_property`; `bulk_move` moves only
        the symbol; `symbol_own_at_span` on an unterminated `(at` panics with
        `byte range starts at 36 but ends at 32`; the noise and standstill
        tests fail against the unrounded, unguarded write.
  - [x] P.6.9.6 `8591707` (residual half only) — `edit_schematic_component`
        declares `fields` in its schema (`sch_components.rs:92-95`) and the
        handler never reads it (`:666-770`), so a call passing only `fields`
        returns `{"changes": []}` as a success: `changed` is empty, but so is
        `errors`, and the fork's own "changed nothing is a failure" guard
        (`:734-746`) requires a non-empty `errors` to fire. The `new_reference`
        half of the upstream commit is already fixed here
        (`update_instance_reference`, `:860`). Loop the object's keys through
        the same helpers P.6.9.5 shares out, refusing the reserved names.
        Measure before copying upstream's macro rewrite of the apply closure —
        it was forced by their borrow shape, which may not be ours.
        Done. The `fields` map is parsed into `Vec<(&str, String)>` before the
        `apply` closure is built (`sch_components.rs:686-720`), through a new
        `property_text` helper: a JSON string is stored as-is, a number or
        boolean as its text form — KiCAD stores every property as text — and
        anything with no text form is refused rather than stringified into
        nonsense. A `fields` that is present but not an object is an
        `InvalidArgument` on `fields`, not a silence.
        `apply` gained a `reject` parameter: the named arguments pass `&[]`,
        the `fields` loop passes `RESERVED_PROPERTY_KEYS`, so a `Reference`
        smuggled through the generic map is refused instead of rewriting the
        property while `(instances …)` keeps the old designator (D124). The
        loop sits after `datasheet` and before `new_reference`, because the
        rename must stay last — it is what makes the symbol findable by
        designator for every field before it.
        The guard is now `changed.is_empty()` alone, with the reason
        `no editable field was given` when `errors` is empty. It previously
        required a non-empty `errors` to fire, which is exactly what let a
        `fields`-only call — and a call with no editable argument at all —
        report `{"changes": []}` as a success.
        Upstream's macro rewrite was measured and not copied, as the item
        required. A first version looping `apply` directly and pushing invalid
        values into `errors` failed with
        `error[E0499]: cannot borrow 'errors' as mutable more than once at a
        time` — the closure holds the borrow live until `new_reference`. The
        conversion pass up front resolves it with no macro.
        Red before: six tests, among them
        `a_fields_only_edit_writes_the_property_the_symbol_lacks`
        (`left: Array [] right: Array [String("MPN → RC0805FR-074K7L (added)")]`)
        and `an_edit_that_changes_nothing_is_a_failure`
        (`an empty edit reported success: {"changes":[],"reference":"R1"}`).
  - [x] P.6.9.7 `6ed6cac` — five write paths run on substituted required
        arguments, because nothing enforces `required` server-side and the
        handlers read with `unwrap_or`: `create_footprint`
        (`library.rs:625-633`; `name` → "Footprint", `pads` → empty, then
        `write_atomic` over the target), `create_symbol` (`:2293-2302`),
        `copy_routing_pattern` (`verification.rs:556-566`; omitting only
        `dest_x`/`dest_y` duplicates the source region onto the board origin),
        `export_dxf` (`pcb_export.rs:513-527`; an empty `layers` makes P.6.7.7
        pass no `--layers` at all, so kicad-cli picks its own set) and
        `place_component_array` (`pcb_components.rs:1483-1484`, `count_x`/
        `count_y` → 1, the rest already guarded). The root cause is that
        `tools/mod.rs:414-441` has `require_str` and `require_f64` and no
        `require_array`/`require_u64`, so every array- and integer-typed
        required argument here is hand-rolled. Add both helpers. An explicitly
        empty array stays accepted; only absence is refused. Assert the target
        file is byte-identical after a refused `create_footprint` — asserting
        the error alone would pass even if the write happened first.
        Done. `tools/mod.rs:438-461` gained `require_array` and `require_u64`
        beside the two that existed. `require_array` returns a borrowed
        `&[Value]`, not an owned `Vec`: it removes `create_footprint`'s
        `.cloned()` and feeds `export_dxf`'s `.iter()` directly. An explicitly
        empty array passes — "a footprint with no pads" is an answer; only
        absence or a non-array is refused, because that is the caller who never
        said. `require_u64` leans on `serde_json::as_u64`, which already
        rejects negative, fractional and string.
        The five sites route strictly by their own schema's `required` list,
        not by what looked risky: `create_footprint` (`library.rs:625`) takes
        `name` and `pads`, `create_symbol` (`:2297`) `name` and
        `reference_prefix`, `copy_routing_pattern` (`verification.rs:559`) all
        six coordinates, `export_dxf` (`pcb_export.rs:523`) `layers`, and
        `place_component_array` (`pcb_components.rs:1485`) `count_x` alone —
        `count_y` carries `"default": 1` in the schema and stays optional.
        `docs/capability-matrix.md` moved one line: the scanner keeps the
        lexicographically smallest source (`capability/coverage.rs:93`), and
        the new unit test in `verification.rs` sorts before the integration
        test that had been proving `copy_routing_pattern`. Status `SUPPORTED`
        unchanged; regenerated with `KAM_UPDATE_MATRIX=1`.
        Red before, five tests, among them
        `create_footprint_without_pads_leaves_the_target_file_byte_identical`
        (panic `a call with no pads must be refused`) and
        `copying_a_pattern_without_a_destination_is_refused_not_dropped_on_the_origin`
        (panic `a copy with no destination must be refused`). The byte-identity
        assertion is the point: asserting the error alone would pass even if
        the write happened first.
        Known contract change: a caller that omitted `pads`, `layers`,
        `dest_x`/`dest_y`, `count_x`, `name` or `reference_prefix` now gets
        `invalid_argument` instead of a silent write. That is the item.
  - [x] P.6.9.8 `977f0c5` — `run_design_review` (`design_review.rs:522-625`)
        and `validate_for_manufacturing` (`manufacturing.rs:281-390`) both
        answer "is my board ready?" and neither has ever run DRC; the second's
        only routing test is still `net_count > 3 && track_count == 0`
        (`:351`), which fires only on a board with no tracks at all, so a board
        routed except for one net reads `READY`. P.6.7.5 corrected how those
        numbers are counted, not what the predicate concludes. Run DRC when a
        board is in scope and fold errors, unconnected items and
        schematic-parity findings into both verdicts; when DRC cannot run, the
        verdict is INCOMPLETE / NOT READY naming the missing evidence, and the
        DRC summary is null rather than zeroed. `DrcReport` and
        `missing_categories()` already exist from P.6.1, which is the hard
        half. Schematic-only reviews stay unchanged. Sequence against P.6.8's
        #185 so neither undoes the other.
        Done. A new `tools/drc_gate.rs` is the single place that turns a DRC
        report into words, so both verdicts say the same thing:
        `DrcEvidence::{Measured(DrcReport), Unavailable(String)}`,
        `gather(cli, board, refill)` — which turns every failure mode (no
        configured binary, spawn error, unreadable board) into `Unavailable`
        carrying the reason verbatim, rather than an error that would abort
        the whole review — and `assess(&DrcEvidence) -> DrcGate { summary,
        findings, incomplete, connectivity_measured }`.
        `summary` is `Value::Null` when nothing was measured, and each absent
        category stays `null` inside it when some were: never an object of
        zeroes standing in for a report nobody has. An absent category also
        emits its own finding — "its absence is not zero findings" — and sets
        `incomplete`.
        Each handler now only gathers evidence and delegates to
        `validate_for_manufacturing_with(args, &DrcEvidence)` and
        `run_design_review_with(args, ctx, Option<&DrcEvidence>)`. That is what
        makes P.6.8's #185 compose with this instead of re-deriving it, and it
        is also what lets every verdict be proved on an injected `DrcReport`
        with no KiCAD in the environment — no gated test was added, and D111 is
        sidestepped entirely because no proof goes through `kicad-cli`.
        Verdict vocabulary, each extended in its own idiom: manufacturing gains
        `INCOMPLETE` beside `NOT READY` / `NEEDS REVIEW` / `READY`; design
        review gains `INCOMPLETE — DRC did not run, so the board is unverified`
        beside its sentence-form verdicts. Precedence is measured error >
        incompleteness > warning, and the incompleteness finding is itself a
        `warning`, so it cannot masquerade as a blocker under `NOT READY`.
        `net_count > 3 && track_count == 0` is kept, but **only** when
        `!gate.connectivity_measured`. The measurement behind that choice: once
        `unconnected_items` is `Some`, the heuristic is strictly subsumed — a
        board with nets and no copper cannot have an empty unconnected list —
        so running it anyway could only add a false positive contradicting a
        measurement. Proved by `the_track_count_heuristic_yields_to_a_measured_drc`
        (4 nets, 0 tracks, clean DRC injected → no "no traces routed" issue).
        Red before: `a_board_review_without_drc_evidence_cannot_look_good`
        (`tests/design_review.rs:325`) —
        `left: String("LOOKS GOOD — no critical issues found")`,
        `right: "INCOMPLETE — DRC did not run, so the board is unverified"`.
        No existing test was modified. `an_unrouted_kicad_10_board_is_flagged`
        stays green on its own terms: its context carries
        `kicad_cli: String::new()`, so DRC is unavailable and the heuristic
        fallback applies — the one case where it still has something to say.
        Known cost, unmeasured here for want of KiCAD: both tools now spawn a
        `kicad-cli pcb drc` process per call when a binary is configured.
  - [x] P.6.9.9 `4536d10` (LATER) — the read-only and batch half of the same
        root cause as P.6.9.7: an omitted `query` becomes `""` and
        `contains("")` is always true, so `search_symbols`
        (`library.rs:2807`), `search_footprints` (`:2960`) and
        `search_templates` (`templates.rs:287`, which has no limit) return
        everything; `suggest_alternatives` (`integration.rs:837-853`) defaults
        `value` and `footprint` to `""`, becomes `LIKE '%%'` on both columns
        and caches the result; both JLCPCB handlers check the database before
        the arguments, sending a caller who forgot `query` to download a
        2.5M-part catalogue; and `batch_add_wire` (`sch_wiring.rs:579-584`)
        re-serialises the file for a call that added nothing. Bundle with
        P.6.9.7 if that item is already open in the same modules.
        Done, on P.6.9.7's helpers — every site routes through
        `try_arg!(require_*)`, strictly by its own schema's `required` list
        (D127). All five schemas already declared the argument required; the
        handlers simply never read it that way.
        `search_symbols` (`library.rs:2812`), `search_footprints` (`:2965`)
        and `search_templates` (`templates.rs:287`) take `require_str` on
        `query`; `search_footprints`'s echoed `"query"` in the result now
        reads the validated value rather than a second `unwrap_or("")`.
        `search_templates` gained the limit it never had, copying its two
        siblings' convention exactly — argument `limit`, integer, default 50,
        `if results.len() >= limit { break }`.
        The JLCPCB ordering defect turned out to have **three** occurrences,
        not the two the item named, and all three are fixed:
        `suggest_alternatives` (`integration.rs:837`) now validates `value`
        and `footprint` before `db_path.exists()` — so also before
        `cache_key`/`get`/`put`, which is what stops a refused query from
        polluting the cache; `get_jlcpcb_part` (`:779`) had `require_str` on
        `lcsc_id` already but ran it *after* the database test; and
        `search_jlcpcb_parts` (`:616`) had both defects at once — a `required`
        `query` read with `unwrap_or("")`, checked after the database.
        `batch_add_wire` (`sch_wiring.rs:582`) takes `require_array` on
        `wires`; an explicitly empty batch stays legitimate and now returns
        `{"added_wires": 0}` without loading or rewriting the file at all.
        Red before, ten tests, among them
        `suggest_alternatives_refuses_its_missing_arguments_before_the_database_is_looked_for`
        (`left: Some("file_not_found") right: Some("invalid_argument")` — the
        ordering proof, and it holds with no JLCPCB database installed),
        `a_refused_suggestion_puts_nothing_in_the_cache`, and
        `an_empty_batch_of_wires_leaves_the_schematic_byte_identical`
        (`a batch that added nothing reserialised the file` — the red showed
        `sheet_instances` reflowed onto several lines).
        `docs/capability-matrix.md` moved, and this time it is a **gain**, not
        a displacement: `search_footprints` goes from `NOT_TESTED | gated` — an
        `#[ignore]`d test was its only proof — to `SUPPORTED | test`. Domain
        `footprints` 85.7 % → 100 %, KiCAD domains 120 → 121 supported
        (73.2 % → 73.8 %), fork proved 135 → 136 (72.6 % → 73.1 %).
        One adjacent change: `seed_test_db` (`integration.rs:1459`) became
        `pub(super)` to be shared with the new test module, matching
        `create_published_schema`/`response_json` beside it.
        Contract change, deliberate: omitting `query`, `value`, `footprint`,
        `lcsc_id` or `wires` now returns `invalid_argument` instead of a whole
        catalogue or an empty rewrite.
  - [x] P.6.9.10 `791f95b` (LATER) — nothing validates `required` at the
        dispatch: `execute_tool` (`mcp/handler.rs:210`) turns absent arguments
        into `{}`. This is the floor beneath P.6.9.7 and P.6.9.9 and must land
        *after* them — added first it fires before any handler runs, and a
        per-tool test could no longer tell a fixed handler from a broken one.
        Presence only; an explicit `null` counts as absent.
        Done. `tools/mod.rs:425` gained `first_missing_required(schema, args)`
        — presence only, `args.get(k).unwrap_or(&Value::Null).is_null()`, so an
        explicit `null` counts as absent — and `handler.rs` wraps it in
        `missing_required_refusal`, which returns the same
        `ToolErrorKind::InvalidArgument` shape the `require_*` helpers produce.
        One vocabulary of refusal, wherever it happens.
        Placed **after** the mode gate on both paths, and the placement was
        measured rather than assumed: moving it ahead of the domain gate turns
        `the_mode_gate_still_answers_first` red (`invalid_argument` instead of
        `write_refused_by_mode`) and moves nothing else. The gate answers
        first — a `ReadOnly` caller gets no argument coaching on a call that
        would be refused anyway. For domain tools the check also sits after
        `get_tool`, hence after auto-load, which is what makes it reachable at
        all for a toolset not yet loaded.
        `kicad_invoke`: envelope only. The check reads the **top-level**
        `required` of the published schema (`["calls"]`) and never the nested
        `required: ["tool"]` inside `items` — batch entries stay validated and
        gated one by one inside `handle_kicad_invoke`, exactly as the mode gate
        treats them.
        Lying schemas found: **none**. The single pre-existing test that
        flipped is not one: `auto_load_toolsets_config_loads_and_executes_on_miss`
        (`crates/konnect/tests/protocol_stdio.rs:424`) calls `route_trace` with
        `{}` and asserted `field == "net_name"`, the first key the *handler*
        checks. The schema declares `required: ["board", "net_name", …]`
        (`pcb_routing.rs:83`) and `board` is genuinely mandatory, so the
        refusal now names `board`. The assertion moved to `"board"`; `kind` is
        unchanged and the test's intent — proving auto-load happened — is
        preserved and in fact strengthened, since only a loaded tool has a
        schema to consult.
        Red before: `a_missing_required_key_is_refused_before_the_handler_runs`
        (`left: String("not_found") right: "invalid_argument"`) — observability
        chosen so "the handler ran" is visible: `move_schematic_component`
        addressed by an unknown `uuid`, whose `not_found` can only come from a
        resolver that already read the file.
  - [x] P.6.9.11 `c6a6407` (LATER) — `get_path` (`tools/mod.rs:442-447`)
        returns `anyhow::Result` so handlers can use `?`, and the dispatch
        stringifies it through the `handler_error` fallback
        (`mcp/handler.rs:338`), while `require_str` returns a structured
        `InvalidArgument`. Whether a caller can tell "you forgot an argument"
        from "the tool tried and failed" therefore depends on which helper the
        handler reached for first. Carry the distinction in the error chain and
        downcast at the dispatch, as `konnect_ipc::TransportUnreachable`
        already does — classify by type, never by matching message text. A path
        that is present but unusable stays a handler error.
        Done, and the measurement came first because P.6.9.10 might have
        emptied the item. Static pass over all 202 `tool!(…)` registrations —
        name → top-level `required` → the handler's `get_path` keys, callees
        followed one level — found **zero** sites still reachable through
        `tools/call`: the two raw candidates (`expand_bus` `sch_buses.rs:306`,
        `run_design_review` `design_review.rs:528`/`:420`) are both already
        guarded by an `is_string()`/`Some(_)` check.
        The item is not moot, though, and the reason is worth recording:
        `handle_kicad_invoke` calls `(def.handler)(&call_args, …)` **without**
        `first_missing_required` — the deliberate envelope-only exemption
        P.6.9.10 chose and froze in
        `the_gateway_envelope_is_checked_but_not_its_entries`. All **172**
        `get_path` sites are therefore observable through the gateway, where an
        entry missing its path key answered `handler_error` while an entry
        missing a `require_*` argument answered `invalid_argument` — the exact
        split this item is about, surviving in the one place the dispatch check
        does not reach.
        `MissingArgument { key }` lives in `mcp/error.rs` beside
        `TransportUnreachable`/`BoardNotOpen` and is read by a downcast pass in
        `ToolErrorKind::from_anyhow`, ahead of the `io::Error`/`Conflict`
        passes — an incomplete request never got far enough to produce the io
        failure the later pass looks for. Both dispatch paths already funnel
        through `from_anyhow`, so one downcast covers them. No message text is
        matched anywhere, and
        `get_path_missing_classifies_as_invalid_argument_by_type` proves the
        classification by `downcast_ref` alone.
        "Present but unusable" is deliberately not reclassified: `get_path`
        tests no existence, so a bad path stays an `io::Error` in the chain and
        keeps `Io { code: "not_found" }` — guarded by
        `a_path_that_is_present_but_unusable_is_still_the_tools_failure`.
        Red before: `an_absent_path_argument_is_an_invalid_argument_like_any_other`
        (`missing_path_argument.rs:85`) —
        `left: String("handler_error") right: "invalid_argument"`.
        Two side effects handled rather than papered over:
        `docs/capability-matrix.md` moved one line — a **displacement**, not a
        gain: `list_schematic_wires`'s evidence goes to the alphabetically
        first test file, now `tests/missing_path_argument.rs` (D128). And
        `error_catalog_debt.rs` went 2 → 3 because the new unit test wrote
        `ToolErrorKind::HandlerError` literally, which the debt scanner counts;
        rewritten as a negative assertion (`!matches!(…, InvalidArgument{..})`)
        so the guard is identical and the debt ceiling was **not** raised.
  - [x] P.6.9.12 `6693681` (LATER) — `register_in_lib_table`
        (`library.rs:1549-1583`) returns `Ok(())` the moment the nickname
        exists, and both handlers — footprint (`:1355-1385`) and symbol
        (`:1442-1478`) — report a bare `"success": true`, so a no-op is
        indistinguishable from a registration and a stale project URI cannot be
        corrected at all. Upstream had already fixed the footprint half under
        #205 before this commit; here neither half is fixed, so there is no
        asymmetry to repair — one path reporting inserted/unchanged/updated,
        with a `replace_existing` policy preserving the entry's own
        `options`/`descr`. Check what `tool-directory.md` promises before
        changing the contract.
        Done, one path for both halves.
        `enum LibTableRegistration { Inserted, Unchanged, Updated{previous_uri},
        UriConflict{existing_uri} }` is returned by `register_in_lib_table`
        (fifth parameter `replace_existing`), and a single shared
        `registration_result(…)` converts it for both handlers:
        `{"success", "result": "inserted|unchanged|updated", "nickname",
        "scope", "table", "uri"}`, plus `previous_uri` on an update.
        `UriConflict` answers `InvalidArgument { field: "replace_existing" }`
        naming the existing URI, the requested one and the remedy — a call that
        needs re-parameterising is exactly what `InvalidArgument` means, and no
        `HandlerError` literal was written (the debt scanner counts those).
        `replace_existing` defaults to **false**, and the default is the
        argument: the old behaviour touched nothing, so defaulting to `true`
        would turn every repeated call into a silent rewrite of someone else's
        entry. Correcting a URI is an explicit request, and the refusal names
        the flag that grants it.
        Lookup no longer uses `content.contains("(name \"X\")")` over the
        whole file — a substring test that a `descr` quoting the nickname was
        enough to fool. `find_lib_entry` reuses `find_block_starts` +
        `find_balanced_block` from `konnect_sexp::writer`, the same pair
        `parse_lib_table` already uses on these tables in this very file, so no
        second parser was written. An update rewrites the located `(uri …)`
        sub-block and nothing else, so the entry's own `options`/`descr` and
        its formatting survive; `unchanged` returns before any `write_atomic`,
        which is what makes the file byte-identical.
        Both schemas gained `replace_existing` (boolean, default false) and
        both `tool-directory.md` lines (l.304, l.309) now state the `result`
        vocabulary and the policy.
        Red before, five tests, each looping over both tools:
        `a_first_registration_reports_inserted` (`left: Null right: "inserted"`),
        `a_different_uri_without_replace_existing_is_refused`
        (`register_footprint_library answered success for a URI it did not write`)
        and `a_nickname_quoted_inside_a_descr_is_not_a_registration`
        (`mistook a quoted nickname in a descr for a registration`). The
        `updated` fixture carries a non-empty `(options "hand-written")` and
        `(descr "the caller's own note")`, without which the preservation
        assertion would prove nothing.
        One existing test took the new argument
        (`registering_a_symbol_library_scaffolds_a_sym_root`); its assertions
        are unchanged, and no test asserted the bare `"success": true`.
        Contract change: the body gains `result`/`uri`, and a nickname
        registered against a different URI is now refused where it used to
        answer success.
  - [x] P.6.9.13 — `handle_group_components` (`sch_components.rs:1553-1562`)
        has P.6.9.5's defect A verbatim and was outside its scope: it inserts
        `(property "Group" …)` unconditionally, at a hardcoded `(at 0 0 0)` and
        a hardcoded two-space indent, so grouping the same component twice
        leaves two `Group` properties, the text renders at the sheet origin,
        and the indentation is wrong for every eeschema-authored sheet. The
        helper it needs already exists — route it through `set_symbol_property`
        like the other two. Proof to reproduce first: two `group_components`
        calls naming the same component yield two `Group` properties.
        Done, and small: the hand-rolled insert — locate `(instances`, splice a
        `format!`ed property with a hardcoded anchor and indent — is replaced by
        one `set_symbol_property` call, the helper P.6.9.5 shared out. `reject`
        is empty because the key is the literal "Group": no caller-supplied
        name can collide with a reserved one here.
        One knock-on handled: an entry that now fails is a per-reference error,
        so `batch.unresolved` became a mutable `batch_errors` the loop can add
        to, and the response's `errors` reports both kinds instead of only the
        unresolved references.
        Red before, two tests:
        `regrouping_a_component_updates_its_group_rather_than_adding_a_second`
        (`left: 2 right: 1` — two `(property "Group"` after two calls) and
        `a_group_property_is_anchored_on_the_symbol_not_the_sheet_origin`
        (`left: "0 0 0"`).
  - [x] P.6.9.14 — `batch_edit_schematic_components` carries the same family
        of defect on `fields` that P.6.9.6 just closed on the single-component
        path, and was outside its scope. `sch_batch.rs:950-965` guards with
        `if let Some(new_val) = field_val.as_str()`, so a number or boolean
        value is **silently dropped** — no write, no error, and the component
        can still report success from another field in the same spec. It
        resolves through `field_value_range` rather than `set_symbol_property`,
        so a key the symbol does not carry yet fails with
        `Field 'X' not found on 'R1'` instead of being inserted — the very
        refusal J.2.4.1 removed from the single path. And it opposes no
        rejection to `Reference`, so the batch path can rewrite the property
        while `(instances …)` keeps the old designator (D124). Route it through
        the shared helpers and the same `property_text` conversion. Proof to
        reproduce first: a batch spec with `fields: {"Qty": 2}` reports success
        and writes nothing.
        Done. The technical difficulty was not the diagnosis but reconciling
        two write models: this handler accumulates `SexpEdit`s whose byte
        ranges index the *original* content and are only correct applied at
        once, while `set_symbol_property` returns an already-spliced document —
        it must, since an insertion's position and indentation are read off the
        symbol as it stands.
        Resolved in two phases. Phase 1 is unchanged: the standard fields stay
        offset edits applied in a single `apply_edits`. Phase 2 walks the
        validated `(field, text)` pairs — parked per component in a new
        `PendingProperties` — over the resulting string, re-locating the symbol
        with `find_symbol_instance_block` before every write, exactly as
        `set_field` does on the single-component path and for the same reason:
        a previous insertion in the same batch has moved everything after it.
        Every phase-1 offset stays valid, nothing is ever reserialised, and a
        one-field edit is still a one-line diff (P.6.9.4).
        `property_text` went `fn` → `pub(crate) fn` — matching
        `place_one_component`, the file's only other exported neighbour —
        rather than being duplicated, so both paths refuse the same values.
        `RESERVED_PROPERTY_KEYS` is the `reject`, so `Reference` is turned away
        before any write. A `fields` that is present but not an object is now
        an error instead of being ignored.
        Red before, four of the six: `a_batch_edit_writes_a_field_given_as_a_number`
        (`{"errors":[],"updated":[],"updated_count":0}` — success, nothing
        written), `a_batch_edit_adds_a_field_the_symbol_does_not_carry_yet`
        (`errors: ["Field 'MPN' not found on 'R1'"]`) and
        `a_batch_edit_refuses_to_rewrite_the_reference_property`
        (`changes: ["Reference → R9"]`). The other two — in-place update, and
        the one-line-diff bound — were green before and after, and stand as
        non-regression guards.
        Behaviour note: the `fields` path's error message changes shape
        (`Field 'X' not found on 'R1'` → `'X' on 'R1': …`); no test or doc
        asserted the old one.
  - [x] P.6.9.15 — `place_component_array`'s schema and handler disagree on
        `spacing_y`. The schema documents `"spacing_y": { "default": 0 }`
        (`pcb_components.rs:1030`); the handler reads
        `args["spacing_y"].as_f64().unwrap_or(spacing_x)` (`:1511`). A caller
        who trusts the published schema and omits `spacing_y` asks for a single
        row and gets a square grid — every part of an N×M array placed at the
        wrong y. Found while doing P.6.9.7 and deliberately left there: it is
        an *optional* argument whose default is wrong, not a required one being
        substituted, so it is a different defect. Decide which of the two is
        right — measure what a row array is normally asked for — then make the
        other match, and cover it with a test that places a 3×2 array without
        `spacing_y` and asserts the y coordinates.
        Done. The **schema** was the half that was wrong. A default of 0 is not
        defensible: it stacks every row of an N x M array on the same y, which
        nobody asks for, while the handler's fallback to `spacing_x` gives a
        square grid — the ordinary reading of "place these in a grid, 2.54
        apart". So `"default": 0` is gone and the description now says what an
        omitted `spacing_y` actually does; the handler is unchanged apart from
        a comment recording which half was wrong.
        The test is a schema-contract test rather than a placement test,
        because the behaviour was never broken: `place_component_array` is an
        IPC tool whose y coordinates cannot be observed without a live KiCAD,
        and the defect was entirely in what the published contract promised.
        Red before: `the_schema_does_not_promise_a_spacing_y_default_the_handler_ignores`
        — `the schema still publishes a spacing_y default the handler
        overrides: {"default":0,"description":"Row spacing in mm","type":"number"}`.
        `docs/capability-matrix.md` gained a line: `place_component_array` goes
        `NOT_TESTED | gated` to `SUPPORTED | test`, placement 9.1 % to 18.2 %,
        KiCAD domains 121 to 122 supported. The gain is real but the mechanism
        is not flattering, and it is what turned up P.6.9.19: the tool has been
        exercised since P.6.9.7, and the scanner simply could not see it,
        because it recognises `"<tool>"` or `handle_<tool>` and this handler is
        named `handle_place_array`.
  - [x] P.6.9.16 — P.6.9.10 validates `required` at the dispatch, but nothing
        proved a schema's `required` list *honest*: that every key it names is
        genuinely mandatory, and that no argument the handler cannot do without
        is missing from it. Done as `required_schema_honesty.rs`, one pass over
        the whole registry — but not in the shape this item proposed. The
        proposed shape, calling each tool with `{}` **through dispatch**, was
        written first and measured tautological: `missing_required_refusal`
        (`handler.rs:344`) refuses before the handler runs and builds the
        refusal *from the required list itself*, so "the refusal names a key
        from its own `required` list" is true by construction and can never go
        red. It passed on 215 tools in 0.41 s and proved nothing.
        The shape that works calls `(tool.handler)(&{}, ctx)` **directly**,
        bypassing the dispatch gate, so the handler's own answer is what is
        scored — both directions in one call per tool: a handler that succeeds
        despite a non-empty `required` over-promises; one that refuses
        `invalid_argument` on a field absent from its own list is the omission
        class of P.6.9.7/P.6.9.9. `Err(anyhow)` is reconverted through
        `ToolErrorKind::from_anyhow` first, as `dispatch_tool` does, or the
        `get_path`/`MissingArgument` path shows up as ~90 false positives.
        Cost measured: 0.25 s, 215 tools, 191 with a `required` list checked,
        21 without, 3 excluded. The exclusions are what the shape costs:
        without dispatch in front, a handler that does I/O, spawns a process or
        reaches the network on `{}` now really does — `download_jlcpcb_database`
        (fetches hundreds of MB), `launch_kicad_ui` (spawns the GUI),
        `save_project` (writes a live session's board).
        Red before, by mutation: `list_footprint_libraries` `"required": []` →
        `["bogus_field"]` gives `list_footprint_libraries: requires
        ["bogus_field"] but succeeded on an empty argument object — the schema
        over-promises, or the handler ignores the key`.
        Five sites came out of the first run. Four schemas were wrong, one
        handler was, and one of the five was not a lie at all:
        * `get_datasheet_url` — not lying: the handler needs `mpn` **or**
          `lcsc_id`, which `required` cannot express, so `required: []` was the
          only correct answer and the refusal names `mpn` as a representative.
          The schema now publishes the real contract as
          `anyOf: [{required:[mpn]}, {required:[lcsc_id]}]`, and the pass reads
          `anyOf` branches as honest. Handler unchanged.
        * `autoroute` — schema wrong. `handle_autoroute` takes `_args` and
          always answers `ManualStepRequired`: the tool has been a stub since
          kicad-cli 10 dropped the DSN/SES round trip. `required: ["board"]`
          made the dispatch demand an argument nothing reads, for a tool that
          will do nothing either way. Now `required: []`, `board` kept in
          `properties` against the day IPC lands.
        * `get_nets_list` — schema wrong. Handler takes `_args`: it queries the
          **open KiCAD session** over IPC, never a file. `board` was neither
          read nor readable, and publishing it suggested the caller was
          querying *that* file. Removed from `required` and `properties`.
        * `query_traces` — same mechanism: the handler reads only `net_name`
          and `layer`. Same fix.
        * `run_design_review` — both halves wrong, in opposite directions.
          `run_design_review_with` only *tests* `args["schematic"].is_string()`
          and skips every schematic audit when it is absent, so a call carrying
          neither `schematic` nor `board` came back `{"ok":true,"verdict":"LOOKS
          GOOD — no critical issues found","findings":[]}` — a passed review of
          nothing. Dispatch hid it; `kicad_invoke` did not, its batch entries
          skipping `first_missing_required` (D131), which is how it was proved
          red. But `required: ["schematic"]` could not be the fix either:
          `a_board_review_without_drc_evidence_cannot_look_good` calls the
          handler with `board` alone and asserts success, so a board-only
          review is intended and the schema was refusing a legitimate call. The
          contract is `schematic` **or** `board`: schema now `required: []` plus
          the same `anyOf` shape as `get_datasheet_url`, and the handler refuses
          only when both are absent — the case `anyOf` cannot reach, since a
          batch entry never gets schema-validated at all.
        Two things the shape does **not** reach, both written into the test's
        own docs: a `required` list naming several keys is only ever proved on
        the *first* key the handler happens to check (see P.6.9.21), and the
        exclusion list has to hide its own tool names from the coverage scanner
        (D133).
  - [x] P.6.9.17 — a `kicad_invoke` entry that failed through the
        `Err(anyhow)` path reported `error_kind` but no field, while the same
        failure on the `Ok(CallToolResult::error_kind)` path reported both.
        Found during P.6.9.11 and left alone then as out of scope: the
        asymmetry predates it, affects every kind rather than the new one, and
        is in the gateway's result assembly, not in the classification. The
        consequence is that a batch caller can be told an argument is invalid
        without being told which — the field P.6.9.11 took care to carry all
        the way through `from_anyhow`.
        The mechanism: the `Ok` arm hands back the handler's whole structured
        body under `result`, so the variant's own fields ride along for free;
        the `Err` arm summarises into `error_kind` + `transient` + `error`, and
        kept only the discriminant. Which arm a refusal takes is an
        implementation detail of the handler — `get_path` returns
        `anyhow::Result`, `require_str` a `CallToolResult` — so a batch caller
        cannot know in advance whether it will be told the field. It matters
        most here of all places, because batch entries skip the dispatch's
        `required` check by design (D131), making this the only refusal they
        get.
        `ToolErrorKind::field()` lifts it out — `Option<&str>`, `None` for
        every kind that is not about an argument, so "not about an argument" is
        distinguishable from "about an argument nobody named". `InvalidArgument`
        is the only variant carrying one. The gateway emits it as
        `error_field`, matching the flattened `error_kind` beside it rather
        than inventing a nested object the `Ok` arm does not have either.
        Fixed in the same pass, same class: the one entry the gateway assembles
        by hand rather than classifying — a call with no `tool` key — also said
        `invalid_argument` and named nothing. It is the single refusal that
        cannot even report which tool it is about, so it now names `tool`.
        Red before: `an entry told its argument is invalid must be told which
        one: {"error":"Missing required argument: 'schematic'","error_kind":
        "invalid_argument","index":0,"ok":false,"tool":"audit_decoupling",
        "transient":"none"}` — the message carried the name in prose, and
        nothing structured did.
  - [x] P.6.9.18 — `batch_edit_schematic_components` still refused `footprint`
        on a symbol carrying no `Footprint` property, through the "standard
        fields" loop that resolved by `field_value_range` (`sch_batch.rs`).
        J.2.4.1 removed exactly that refusal from the single-component path,
        because a part placed without a footprint has no such property at all
        and assigning one is the most common edit after placement; P.6.9.14
        fixed the `fields` half of this handler but left the standard-field
        half, which was a separate loop. Found while doing P.6.9.14 and left
        alone then as out of scope.
        Fixed by routing `value` and `footprint` into the same `updates` vector
        the `fields` map uses, written through `set_symbol_property`, which
        inserts a property the symbol does not carry yet instead of refusing.
        `Reference` keeps its own path — `new_reference` still has to rewrite
        `(instances …)` (D124), and the generic property path would only do
        half the job.
        Collision rule, taken from the single-component path rather than
        invented: `edit_schematic_component` applies its named `value` /
        `footprint` / `datasheet` arguments first and its `fields` map second
        (`sch_components.rs:754-763`), so the map wins there. The batch pushes
        standard fields into `updates` before the map for the same result, and
        a test freezes it — a spec naming the same property both ways is no
        longer left to iteration order.
        Falls out of the fix: `field_value_range` had no caller left and is
        gone, and with it the last offset-based edit in this handler, so the
        two-phase split it forced is gone too. Everything is one pass over
        `updates` now, each write re-locating the symbol by reference against
        the growing string, exactly as `set_field` does on the single path — a
        one-field edit is still a one-line diff (P.6.9.4).
        Red before: `{"errors":["Field 'Footprint' not found on 'R1'"],
        "updated":[],"updated_count":0}` on the fixture `bus_two_resistors`,
        which carries no `(property "Footprint" …)`. Two tests added, both
        integration (no D128 evidence shift):
        `a_batch_edit_sets_a_footprint_the_symbol_does_not_carry_yet` and
        `a_batch_edit_resolves_a_footprint_collision_the_same_way_the_single_component_path_does`.
  - [x] P.6.9.19 — the capability scanner recognises a tool by `"<tool>"` or
        `handle_<tool>` (`capability/coverage.rs:210`), and 24 of the 198
        registered tools have a handler whose name does not match their own:
        `place_component_array` is `handle_place_array`,
        `batch_edit_schematic_components` is `handle_batch_edit`,
        `route_differential_pair` is `handle_route_diff_pair`,
        `list_schematic_wires` is `handle_list_wires`, and twenty more. The
        item assumed those tools read `NOT_TESTED` while a test exercises them,
        and asked for the real number first: **for each of the 24, does a test
        already exercise it?**
        Measured, and the answer closes the item differently than it proposed:
        the hidden coverage is **zero**. 22 of the 24 are already `SUPPORTED`,
        `PARTIAL` or `EXTERNAL_TOOL` because a test names the tool as a string
        — the scan's *other* criterion, untouched by the handler mismatch. The
        remaining two, `route_differential_pair` and `open_schematic_viewer`,
        have no test at all: nothing in the repository mentions either the tool
        name or the handler name outside the tool definition. Their
        `NOT_TESTED` is correct, not hidden.
        What makes the mismatch harmless is therefore not luck and not the
        naming: it is this repository's testing convention. `tests/harness/mod.rs`
        goes through `ToolRouter` by name rather than calling a private handler,
        on purpose — "the tool has to be registered, findable by name, and take
        the arguments its schema advertises". Every test written that way trips
        the first criterion regardless of what the handler is called. The
        exposure is real but narrow and currently empty: a tool proved *only*
        by a unit test calling its handler directly would read `NOT_TESTED`.
        So neither remedy the item floated is worth doing. Renaming the 24
        handlers moves **zero** matrix lines and zero percentage points, and one
        of the 24 cannot be renamed at all: `handle_get_pin_connections` serves
        both `get_pin_connections` and `get_pin_net_name`
        (`sch_analysis.rs:83,95`), so making it match one name unmatches the
        other. An alias table buys the same nothing and adds a hand-written list
        to keep in sync — the exact failure mode D120 rejects elsewhere.
        What the measurement *did* find worth fixing is the matrix's own
        preamble, which claimed the scan errs in one direction only: "That
        direction is deliberate: a scanner that guesses wide inflates the number
        it exists to keep honest." D133 had already disproved that — a tool name
        quoted in a test that does not call it counts as proof, which is why
        `required_schema_honesty.rs` splits three names with `concat!`. A
        document whose whole claim is that its percentage is measured cannot
        describe its own method as safer than it is. The preamble
        (`capability/render.rs`) now states both directions, names the
        `ToolRouter` convention that keeps the under-reporting empty, and tells
        a future test author to break a tool name they mention without calling.
        No status or percentage changes: the regenerated matrix differs by its
        preamble alone.
  - [x] P.6.9.20 — `the_jlcpcb_tools_say_the_database_is_missing_rather_than_finding_nothing`
        (`sourcing_and_manufacturing.rs:25`) asserted `stats["exists"] == false`
        on the grounds that "no database is configured in this harness" — but
        the harness set `jlcpcb_db_path: None`, and `resolve_db_path`
        (`tools/integration.rs:248`) reads that as *fall back to the
        machine-wide default*, `%APPDATA%\konnect\jlcpcb.db`. The test therefore
        asserted a fact about the machine while its message claimed one about
        the fixture, and it started failing the day a real database was
        downloaded here — 2026-08-25, `downloaded_at_unix 1787658362`, 1 581
        parts, 1,66 MB. Same family as D113: not a test that skips in silence,
        but one that measures something other than what it says.
        The harness now names a path under `CARGO_TARGET_TMPDIR` that is never
        created (`absent_jlcpcb_db`), so absence is a property of the fixture
        the way `kicad_cli: ""` already makes "no kicad-cli" one. The test
        asserts *which* database was looked at before asserting it is absent,
        which is what stops the fallback from creeping back.
        That assertion turned up a second defect in the tool itself: the
        `exists: false` branch of `handle_jlcpcb_stats` reported no `path` at
        all, only the `exists: true` branch did. With three possible sources for
        that path — an explicit `output_path`, the configured `jlcpcb_db_path`,
        the machine-wide default — "no database" without saying which one leaves
        a caller unable to tell a misconfigured path from a missing download:
        the very distinction this tool exists to draw. `path` is now reported on
        both branches.
        Red before: `the harness must name its own absent database, not the
        machine's: {"exists":false,"note":"Run download_jlcpcb_database to fetch
        the parts database"}`. Found while validating P.6.9.16 and independent
        of it — reproduced on a stashed tree.
  - [x] P.6.9.21 — `required_schema_honesty.rs` calls each handler with `{}`,
        which is missing *every* required key at once, so it only ever proves
        the **first** key that handler happens to check. A schema listing
        `["board", "uuid"]` whose handler reads only `uuid` passes that pass
        exactly like one that reads both. The class is real and wider than what
        P.6.9.16 caught: measured in `pcb_routing.rs`, `"board"` appears in 33
        tool schemas and is read by 5 handlers.
        The shape this item floated — omit each required key in turn, filling
        the others with placeholder values — was rejected on measurement rather
        than tried: a plausible-looking path that does not exist makes a
        perfectly correct handler answer `file_not_found` before it ever looks
        at the omitted key, so "does not require it" and "requires it after
        some other check" are indistinguishable. The only unambiguous signal
        would be a *success* despite a missing key, and almost nothing succeeds
        on placeholder values. Near-zero yield, massive false positives.
        Done instead as `required_schema_static_honesty.rs`: a key the schema
        declares `required` and the handler's body never reads is a lie with no
        execution needed and no dangerous false positive. It reads the handler
        body out of `src/tools/*.rs` — the handler is located through the
        `tool!` block, never by assuming `handle_<tool>` (P.6.9.19) — and looks
        for each required key in the argument-reading forms this codebase
        actually uses. Precedent for scanning our own sources: the coverage
        scanner does it already.
        Measured: 193 tools, 416 required keys, 19 resolved only through one
        level of indirection (the `ipc!` macro, and `handle_add_bus`'s loop
        over a literal `[x1, y1, x2, y2]`), 0 handlers unmapped. Five liars,
        all in `pcb_routing.rs` — see P.6.9.22, which is what they turned out
        to be.
        The limit, written into the test's own docs: it proves a key is
        **read**, not that it is **honoured**. `route_pad_to_pad` reads `board`
        with `get_path` to find its pads in the file, so the scan clears it,
        and then routed over IPC without checking that KiCAD holds that board —
        it could read A's pads and lay copper on B. That case is what the guard
        in P.6.9.22 closes instead.
  - [x] P.6.9.22 — the five keys P.6.9.21 found were not five careless schemas.
        They were one missing guard, and the worst defect of this phase.
        `pcb_components.rs` defined an `ipc!` macro that resolves `board` from
        the arguments and calls `ensure_board_is_active` before the body runs.
        `pcb_routing.rs` defined a *second* `ipc!`, two-argument, that read no
        `board` and checked nothing, falling through to `get_board_document()`
        — KiCAD's **first** open document. So a caller naming a board in a
        schema that requires it had the request executed against whatever board
        happened to be in front. Six of the eight handlers on that path write
        copper: vias, traces, differential pairs.
        This is not inference. `find_open_board` exists precisely for it, and
        its doc comment records the live symptom: "with the user's own project
        focused and the target board open behind it, first-document targeting
        either fails or, worse, would mutate the wrong board." The guard was
        written, and one file stayed beside it.
        One definition now — `ipc_boundary::guarded_ipc`, in the module whose
        whole premise is that this boundary is typed once and no handler
        re-derives it — imported `as ipc` by both files. Two copies are what
        let one diverge.
        Reverses P.6.9.16 on two tools, deliberately. I had dropped `board`
        from `query_traces` and `get_nets_list`, concluding they query the open
        session rather than a file. That described the defect as if it were the
        intent: the handlers ignored `board` because their macro never resolved
        one, not because a caller naming a board meant nothing. `board` is
        restored to both schemas, `required` included, and now honoured. The
        comments I left there are rewritten to say so, reversal included.
        Guard that closes the class rather than the eight sites:
        `no_ipc_call_bypasses_the_guarded_macro_in_pcb_routing` requires zero
        textual `with_ipc(` in `pcb_routing.rs`. Red before at `left: 1, right:
        0`, green after. It is what would have caught `route_pad_to_pad`, whose
        `board` *is* read and was never honoured, and what catches a future
        handler written next to the path instead of through it.
        Proof is structural throughout: the behaviour needs a live KiCAD with
        two boards open, and `e2e-kicad.yml` has no routing probe to hang one
        on.
  - [x] P.6.9.23 — the same unguarded shape survived outside `pcb_routing.rs`:
        nine direct `with_ipc(` calls no macro guarded, in `pcb_board.rs`,
        `pcb_export.rs` and `pcb_components.rs`, eight of them writing.
        Measured one at a time rather than guarded in bulk, and the nine split
        three ways.
        Six were real instances of P.6.9.22. Five in `pcb_board.rs`
        (`set_board_size`, `get_board_extents`, `add_board_outline`,
        `add_board_text`, `import_svg_logo`) and one in `pcb_export.rs`
        (`refill_zones`, whose `KiCadIpcClient::refill_zones` calls
        `get_board_document()` internally — the first open document again).
        Three in `pcb_components.rs` were already correct, and by two different
        mechanisms worth distinguishing: `place_array` and `align_components`
        already carried the inline check, while `place_component` is guarded a
        level down, `place_footprint` calling `find_open_board` itself. No
        schema was missing `board`; no schema defect in the nine.
        Two shapes of guard, because one of them does not fit everywhere.
        `guarded_ipc` answers `ipc_error_result` and returns, which is right
        where there is nothing else to try — that is `refill_zones`, routed
        through it. But `pcb_board.rs` has a deliberate file fallback, and
        returning would make it unreachable, so those five take the check
        inline inside the closure and let the failure travel as a value.
        `IpcFailure::allows_file_fallback()` then does exactly the right thing
        without being asked twice: `BoardMismatch` returns `false` alongside
        `Rejected`, because both prove KiCAD answered and editing the file
        underneath a live editor would race it.
        `get_board_extents` is the one read among the six, and it inverts:
        its fallback is unconditional, so a `BoardMismatch` now falls through
        to computing extents from the file the caller actually named. Before
        the guard, a KiCAD holding some *other* board answered with that
        board's extents, reported as `"source": "ipc"` as though they were the
        requested board's.
        The guard is generalised rather than copied — the lesson of P.6.9.22
        was that two copies diverge. `no_ipc_call_bypasses_the_guarded_macro_or_an_inline_board_check`
        scans every file in `src/tools` except the definition site, and fails
        any `with_ipc(` whose paren-balanced argument span contains neither
        `ensure_board_is_active(` nor — as one named, justified exception —
        `place_footprint(`. Red before at `total_violations left: 6, right: 0`,
        naming `pcb_board.rs` lines 364, 475, 735, 823, 961 and
        `pcb_export.rs` line 629; green after.
        Structural proof again: the behaviour needs a live KiCAD holding two
        boards, which no probe here provides. The guard is textual, so a future
        handler that embeds one of those substrings without actually guarding
        would pass — the same accepted limit as this file's other pass, and
        written in its docs.
### Validation
Each implemented item carries a test that is red before it and green after,
and — where KiCad is the only honest oracle — a probe in
`schematic_fidelity_live.rs` or its PCB equivalent, inside the gating E2E job.
No item is closed on "the existing suite still passes".

## P.7 — The suite must prove itself on CI's machine, not this one — DONE

### Objectif
P.6 closed with `cargo test --workspace` green here and the PR pushed. The
gating CI run on the very same commit (`8aeaff7`) was **red on all three
OSes**, and had been since the commit that introduced the test. The local
suite and the CI suite were measuring two different machines, and the local
one was the one with KiCad installed. Close that gap where it is: in the test,
in the harness that let it stay silent, and in the CI command that hid
whatever was behind it.

### Dépendances
None. The defect is in test code and one workflow line; no production path
changes.

### Tâches
- [x] P.7.1 — `a_component_placed_on_a_child_sheet_is_written_with_the_roots_path`
      (`crates/konnect-core/tests/sheet_instances.rs`) placed `Device:R` into a
      child built from `blank_schematic_template()`, whose `lib_symbols` is
      empty. `library::ensure_lib_symbol` therefore had to resolve the id from
      the **installed** libraries, which exist on the machine the test was
      written on and on no CI runner — `cargo test` in `ci.yml` installs no
      KiCad. The tool answered `Library 'Device' not found`, wrote nothing,
      and the assertion then read a file it had never touched, reporting the
      absence of `(project "proj"` as an instance-derivation defect.
      Fixed by making the placement a property of the fixture: the child is
      `harness::TWO_RESISTORS`, which embeds `Device:R` and is already the
      repository's answer to this exact question — its doc comment says "so no
      installed libraries are needed". Red before under a simulated
      KiCad-less machine, green after, and green with KiCad present.
      Scope measured rather than assumed: the whole workspace suite was run
      with `ProgramFiles`, `ProgramFiles(x86)`, `LOCALAPPDATA`, `APPDATA` and
      the three `KICAD<major>_SYMBOL_DIR` variables pointed at an empty
      directory — every root `kicad_paths::share_roots` knows on Windows. One
      test failed. The class is this one test, not a family.
- [x] P.7.2 — the harness let it stay silent. `Harness::json`'s doc said
      "Panics if the tool errored", and it only ever checked `Result::Err` —
      but a refusing handler here returns `Ok(CallToolResult { is_error: true
      })`: `require_str`, `get_path` and `lib_symbol_not_found_error` all build
      a result rather than an error. So a test could assert a tool's effect on
      a file while the tool had refused to act, and the failure it eventually
      produced named the wrong thing. `json` now asserts `!is_error` and
      prints the refusal body. Measured against the whole `konnect-core`
      suite on a KiCad-less machine: no other test depended on the old
      leniency. It turns P.7.1's failure message from a dump of an untouched
      file into `'add_schematic_component' refused: Library 'Device' not
      found …`.
- [x] P.7.3 — `ci.yml`'s test step ran `cargo test` without `--no-fail-fast`,
      so the run stopped at the first failing binary. `sheet_instances` sits
      two thirds down the alphabet, and the log said nothing either way about
      the eleven binaries after it — the red run could not be read as evidence
      of scope. Added `--no-fail-fast`, matching the command this project
      already uses locally.

- [x] P.7.4 — the gating E2E was red for the same reason, one layer out. It is
      not a per-PR job, so `conformance_test`'s board half — added in this
      phase — had never run in CI at all; the run dispatched by hand on this
      branch is its first. It failed on
      `every_installed_demo_board_parses_or_is_a_known_bad_file`:
      `RoyalBlue54L-Feather.kicad_pcb` **parses** on the 10.0.5 CI pins, while
      `KNOWN_BAD_BOARDS` — measured on the local 10.0.3 (D116) — says it must
      not. The list stated a fact about one KiCad install and read as a fact
      about the parser, so the test failed on a machine where nothing was
      wrong. Same family as P.7.1 and D113, one level up: not "is KiCad
      installed" but "which KiCad".
      Fixed by measuring instead of listing. `paren_balance` walks the file's
      own bytes, honouring quoted strings and their escapes, and
      `malformation` reports why a file is not one balanced s-expression —
      root closing early, a non-blank tail after it, or a depth that never
      returns to zero. The test then asserts the parser *agrees* with that
      measurement in both directions: a balanced file it refuses is our bug, a
      malformed file it accepts is the silent damage the test exists to catch.
      No file name appears in it, so which board is damaged travels with the
      install. Renamed to
      `the_parser_agrees_with_each_demo_boards_own_paren_balance`, since that
      is now what it proves.
      The scanner reproduces D116's numbers exactly on 10.0.3 — "root closes at
      byte 14735 of 3618800, ending at depth -349" — which is what makes it
      trustworthy as a replacement for the list it removes.
      `numbering_detection_explains_every_layer_entry_in_the_demo_corpus` loses
      its by-name skip too: it already skipped anything that fails to parse.

- [x] P.7.5 — with the conformance step green, the next E2E run reached the
      probes behind it — none of which had ever run in CI either — and
      `a_symbol_added_to_a_child_sheet_leaves_the_hierarchy_loadable` failed on
      its opening guard, `before["total"] == 0`, with 40 violations. All 40
      were `warning`, all "The current configuration does not include the
      footprint library …", `errors: 0`. A KiCad that has never been launched
      has no user `fp-lib-table`, and every runner is one. The third machine
      fact in this section, after "is KiCad installed" and "which KiCad":
      "has KiCad ever been run".
      The guard was also inert. Its comment said "anything reported afterwards
      is this call's doing", and the probe never read ERC again — it took one
      measurement and asserted a constant against it. So it is replaced by the
      comparison it described: every violation that does not name `R999` must
      be exactly as numerous after the edit as before, and at least one must
      name `R999` — which is the root seeing the symbol the child sheet was
      given, an assertion the probe did not previously make at all.
      `errors == 0` was tried first and measured false: `R999` goes in unwired,
      and KiCad rates `pin_not_connected` an **error**, not a warning — 2 of
      them. "Nothing else moved" is the shape that survives both machines.
      The axis has no local oracle. `%APPDATA%` was pointed at an empty
      directory and `kicad-cli sch erc` still reported 0 violations against
      `complex_hierarchy`, the redirected directory staying empty: KiCad
      resolves its config through the Windows shell API, not the environment
      variable. So this fix is proved by CI and by nothing else, which is
      exactly what a gating E2E job is for.
- [x] P.7.6 — the E2E workflow hid the same way `ci.yml` did. Its probes are
      twelve separate `cargo test` steps, so a red one stops the job and says
      nothing about the eleven behind it: two full runs were spent finding one
      defect each, in order, when both were visible from the start. Every probe
      step now carries `if: always() && steps.kicad.outcome == 'success'` — it
      runs even after an earlier failure, the job still goes red, and a failed
      install does not turn into a dozen identical failures. P.7.3 at the
      workflow's scale.

### Validation
- `cargo test --workspace --locked --lib --tests --no-fail-fast` on a
  simulated KiCad-less machine (every Windows share root and the three symbol
  env vars pointed at an empty directory): **57 suites, 1385 tests, 0 failed**
- the same command in the normal environment, KiCad 10.0.3 installed:
  **57 suites, 1385 tests, 0 failed**
- `cargo fmt --all -- --check` and
  `cargo clippy --workspace --locked --all-targets -- -D warnings`: PASS, 0
- the CI run on the pushed commit (`1ff991b`, run `32937415695`) is green on
  all three OSes — the check that was red is the one that had to turn, and it
  did: `Check & Test` passes on ubuntu, macos and windows, where the same
  workflow on `8aeaff7` (run `32936272573`) failed on all three.
- `cargo test -p konnect-core --test conformance_test` locally, KiCad 10.0.3:
  **6 passed**, the corpus reporting 18/19 boards parsed and one malformed
  with D116's own numbers.
- the gating E2E, dispatched by hand on this branch (it has no per-PR
  trigger): run `32939555970` on `6ae15c2` is **green on every step** — the
  design loop, conformance, the layer corpus, all nine probes, the IPC wedge
  and the PCM package, with nothing skipped but "upload artifacts on failure".
  It is the only oracle for P.7.4 and P.7.5: which KiCad ships a demo, and
  whether that KiCad has ever been launched, are not observable from here.
  Runs `32937438691` and `32938428303` are the two red ones it replaces.
- the PR checks on the same commit: green on all three OSes, plus Clippy,
  Format, PCM packaging and the schematic viewer (run `32939559816`).

---

# Phase Q — Release v1.1.0 — DONE

Opened 2026-08-26 by explicit user request, immediately after Phase P merged.
Scope is publication only: **no new capability, no Dependabot work, no symbol
or footprint authoring, no KiCad 11**. The phase ships what `agentic/main`
already contains, and stops.

## Objectif

Make the post-Phase-P state actually installable by a stranger: version bumped
everywhere a version is carried, release notes that name what a user can
observe changing, every gate green on the commit that gets tagged, the tag
pushed, and the published artefact opened rather than trusted.

## Version — decided, not assumed

**`v1.1.0`, not `v1.0.1`.** The audit that opened this phase measured four
behaviours a user can observe differing from v1.0.0, all landed by Phase P:

1. `create_netclass` / `assign_net_to_class` write the sibling `.kicad_pro`
   (`net_settings`, `netclass_patterns`) and never touch the board file — at
   v1.0.0 they inserted a `(netclass …)` node into the `.kicad_pcb`, which D112
   measured as making `kicad-cli` exit 3.
2. `run_drc` reads `unconnected_items` and `schematic_parity`, so a board with
   unrouted copper is now **refused** by the evidence gate that approved it at
   v1.0.0 (P.6.1).
3. Power symbols join the schematic net graph (P.6.3), changing netlists that
   v1.0.0 reported as disconnected.
4. `register_footprint_library` / `register_symbol_library` answer a `result`
   vocabulary (`inserted` / `unchanged` / `updated`) and accept
   `replace_existing`.

A patch number would under-announce all four. Nothing here is breaking, so the
minor is the exact number.

## Dépendances

None outside the repository. Phase P is merged; `agentic/main` is at `d962552`
with CI green (run `32940958142`). The release machinery is Phase O's and is
reused unchanged, not rebuilt.

## Invariants de la phase

- No production code changes. If a gate goes red for a reason that is not the
  version bump, the phase **stops** and the defect is triaged as its own item —
  a release phase does not become a fix phase in silence.
- The version lives in five files, and `crates/schematic-viewer/Cargo.lock` is
  one of them (O.7.3: it is outside the workspace, `gate.ps1` never touches it,
  and at v1.0.0 it turned CI red on `cargo check --locked` alone).
- macOS ships unsigned and un-notarised, as at v1.0.0. That is documented in the
  release notes, not silently shipped — decided by the user for this release.
- PCM is **built and verified only**. Submitting to the official KiCad addon
  repository is out of scope, and `packaging/metadata.json` keeping upstream's
  `identifier` / `author` is therefore not a blocker here. Recorded as Q.5.3.

## Q.1 — The version moves in every file that carries one — DONE

### Objectif
`1.0.0 → 1.1.0` everywhere, verified the way CI verifies it rather than by
reading the diff.

### Tâches
- [x] Q.1.1 `Cargo.toml` `[workspace.package].version`, and the `Cargo.lock`
      entries `--locked` verifies
- [x] Q.1.2 `crates/schematic-viewer/Cargo.toml`, its `tauri.conf.json`, and
      **its own `Cargo.lock`** — the three entries `schematic-viewer`,
      `konnect-schematic-editor` and `konnect-sexp`
- [x] Q.1.3 Nothing else is version-bumped: the four remaining `1.0.0` strings
      in the tree are prose (`README.md` status line, `RELEASE_NOTES.md` title
      and install line), handled by Q.2

### Validation
`cargo metadata --locked --format-version 1` succeeds against **both**
manifests — the root workspace and `crates/schematic-viewer/Cargo.toml`. That
second command is the one CI's `Schematic viewer` job runs and the one the
v1.0.0 release failed.

## Q.2 — Public documents state the version they ship — DONE

### Objectif
A reader of the release page learns what changed and what is not covered,
without opening the plan.

### Tâches
- [x] Q.2.1 `RELEASE_NOTES.md` becomes the v1.1.0 body: title, a new
      *What changed in v1.1.0* section naming the four observable behaviours
      above plus the CI-fidelity work (P.7) in one line, and the install line
      pointing at `konnect-pcm-v1.1.0-<platform>.zip`
- [x] Q.2.2 The macOS Gatekeeper limitation is stated in *Getting started* with
      the exact step a user needs, not a euphemism
- [x] Q.2.3 `README.md` status line reads v1.1.0
- [x] Q.2.4 Every other number quoted publicly is re-checked rather than
      assumed: `202 tools / 22 toolsets` (unchanged — `v1.0.0..HEAD` registers
      no new tool), the 21.8 MB binary size, and the benchmark figures, which
      Phase P did not re-measure and which therefore still describe v1.0.0

### Validation
`rg '1\.0\.0'` over the tree returns only historical references (plan, decisions,
progress, upstream audit), never a statement about the shipping version.

## Q.3 — Every gate green on the commit that gets tagged — DONE

### Objectif
The tag lands on a commit already proven, so the release workflow confirms
rather than discovers.

### Tâches
- [x] Q.3.1 Local gate on the release commit: `cargo fmt --all -- --check`,
      `cargo clippy --workspace --locked --all-targets -- -D warnings`,
      `cargo test --workspace --locked --lib --tests --no-fail-fast`
- [x] Q.3.2 CI green on the pushed commit, all three OSes plus the
      `Schematic viewer` job
- [x] Q.3.3 The **gating E2E dispatched by hand** on the release commit and
      green: `gh workflow run e2e-kicad.yml -R nevenfo/kicad-agentic-mcp --ref
      <branch>`. It has no per-PR trigger, its last green run is `32939555970`
      on `6ae15c2`, and `release.yml` needs it. Running it after the tag would
      risk a published tag with no release behind it

### Validation
Three green runs named by id in `progress.md`, on the commit `git rev-list -n1`
resolves the tag to.

## Q.4 — Tag and publish — DONE

### Objectif
One tag, one release, no artefact invented by hand.

### Tâches
- [x] Q.4.1 Work lands on `agentic/main` through a PR from
      `ai/Q-release-1.1.0` — the default branch takes no direct push
- [x] Q.4.2 Annotated tag `v1.1.0` on the merge commit, verified with
      `git rev-list -n1 v1.1.0` before and after the push. `v1.1.0` collides
      with nothing: this repository has published `v1.0.0` only
- [x] Q.4.3 Release workflow run `32948098418`: **green**, 9 jobs of which one
      is deliberately skipped — `Live IPC against a running pcbnew`, which the
      gating mode drops. Four standalone binaries, three PCM packages, seven
      assets. The merge commit's tree is byte-identical to `fb18d96`'s, the
      tree every gate ran on
- [x] Q.4.4 The release body is `RELEASE_NOTES.md` (`gh release edit`), the
      title *KiCad Agentic MCP v1.1.0*, and its relative links resolve against
      the tag

### Validation
`gh release view v1.1.0` lists seven assets and a body that is not the
auto-generated commit list.

## Q.5 — The published artefact is opened, not trusted — DONE

### Objectif
O.9.3's standard: the zip a user downloads is inspected on this machine.

### Tâches
- [x] Q.5.1 `konnect-pcm-v1.1.0-windows.zip` downloaded from the release and
      opened: 8 entries, `metadata.json` carrying exactly one `versions[]`
      entry — `1.1.0` / `stable` / `kicad_version 10.0` / `platforms
      ["windows"]`, no `download_*` field invented — the plugin manifest at
      `plugins/plugin.json`, `plugins/bin/schematic-viewer.exe` bundled, and
      the extracted `plugins/bin/konnect.exe` answering **`konnect 1.1.0`**
- [x] Q.5.4 Q.2.4 was answered by assumption and the artefact corrected it: the
      published binary is **23.7 MiB**, not the 21.8 MB the notes claimed and
      the 22 MB the README claimed. Measured against v1.0.0's own published
      binary — 22 860 288 bytes then, 24 848 384 now, **+1.9 MiB** — which also
      settles the unit: this repository's "MB" has always been MiB. Corrected
      in `RELEASE_NOTES.md` and both README sites, and the release body
      re-posted. The file inside the `v1.1.0` tag keeps the pre-correction
      number; a tag is not moved to fix prose
- [x] Q.5.2 `progress.md` states the closing state: released, gates named,
      nothing open
- [x] Q.5.3 The two known non-blockers are recorded rather than fixed:
      `packaging/metadata.json` still carries upstream's
      `com.github.mixelpixx.konnect` identifier and `mixelpixx` as author,
      which only matters at official PCM submission; and eight Dependabot PRs
      remain open, none of which the release depends on

### Validation
The version the binary reports is the version the tag names is the version the
package declares — measured, in that order, from the downloaded file.

## Q.6 — A test that measured the runner's filesystem, found by this phase — DONE

### Objectif
The phase invariant says a gate going red for a reason that is not the version
bump stops the release and is triaged as its own item. One did. This is it.

### Ce qui a rougi
CI run `32944662909`, on `c377a41` — a commit that touches **only** `plan.md`
and `progress.md`. `Check & Test (ubuntu-latest)` failed with three tests in
`konnect-schematic-editor --lib`, on identical source to the green run before
it. Two of the three were collateral: they read `PoisonError`. Run
`32945946481`, on `724e5a7` — another Markdown-only commit — then rejected the
first fix and reported the failure alone, which is itself the proof of Q.6.3.

### Tâches
- [x] Q.6.1 The real failure is
      `a_symbol_added_inside_an_existing_library_makes_the_index_stale`. It
      writes the index cache, then creates a symbol inside
      `Device.kicad_symdir` and requires the cache to read stale.
      `fingerprint_children` hashes each library entry's mtime, so the
      assertion holds only if the filesystem gives that directory a *new*
      stamp. Whether it does is a property of the **machine**: NTFS stamps in
      100 ns units and the test is green **30 times out of 30** here, while an
      ext4 volume whose inodes are 128 bytes wide carries no sub-second field
      at all, and on that runner the symbol lands inside the tick the cache
      already recorded. D140's class one level down — an assertion about the
      machine wearing the clothes of an assertion about the code
- [x] Q.6.2 The first fix was **wrong, and CI said so**: waiting a
      millisecond before writing assumes the granularity is finer than a
      millisecond, which is the very thing in question, and the run came back
      red on the same assertion. A stamp already written does not move on its
      own, so what has to be waited for is the **observable value**: the test
      now recreates the symbol until the directory's mtime differs from the one
      the cache recorded, bounded at ~2 s — comfortably past a one-second
      granularity, and a failure after that is a real defect rather than a slow
      disk. The failure message prints both stamps, so the next red run says
      which of the two it is
- [x] Q.6.3 The other two reds of the first run are one defect of
      amplification, not two failures: the panic poisoned `ENV_LOCK`, so every
      later test taking it died with `PoisonError` instead of reporting its own
      verdict. That mutex guards an environment variable, not an invariant over
      data. `env_lock()` now recovers the guard with `into_inner()`. **Proved by
      the second red run**: the same defect then reported as `34 passed;
      1 failed` instead of three failures. P.7.6's rule at the mutex level
- [x] Q.6.4 Production code is untouched. Every edit is inside
      `mod suggestion_tests`, so the phase invariant holds

### Validation
`cargo test -p konnect-schematic-editor --lib`: 37 passed, 0 failed, and 30
consecutive runs of the repaired test green. The oracle that matters is
`Check & Test (ubuntu-latest)`, since it is the only machine where this has
ever been red.

# Phase R — Launch & adoption — COMPLETED

Opened 2026-08-26 by explicit user request, immediately after Phase Q published
v1.1.0. Scope is **adoption, not capability**: turn a published and technically
validated release into a project a stranger can understand, install, try and
judge without asking the maintainer anything.

## Objectif

A person who has never spoken to the maintainer reaches, unaided:

1. an installed Konnect from the **published** v1.1.0 release,
2. an MCP client that connects to it,
3. a first task executed against a real KiCad project,
4. a verdict from KiCad that the task landed,

and the maintainer receives enough structured feedback from the first users to
decide the next technical phase on evidence rather than on preference.

## Ce que R n'est pas

- **No new product capability.** A capability is added only if a defect directly
  blocks installation, first use, or the public demo — and then it is the
  minimum fix, triaged first (see the classification invariant below).
- **No architectural refactor**, no opportunistic feature, no KiCad 11 work.
- **No Dependabot sweep, no macOS signing, no official KiCad addon-repository
  submission** unless one of them turns out to block R itself. All three stay in
  `progress.md`'s recorded-but-untreated list, where Phase Q left them.
- **No telemetry.** The feedback loop of R.5 is human-reported and opt-in by
  construction; nothing is added to the binary that reports anything anywhere.

## Dépendances

- v1.1.0 is published: <https://github.com/nevenfo/kicad-agentic-mcp/releases/tag/v1.1.0>,
  tag on `80da119`, 7 assets. Phase Q verified the Windows PCM package by
  opening it.
- KiCad **10.0.3** at `C:\Users\FlowUP\AppData\Local\Programs\KiCad\10.0\bin\`
  — the install this phase tests against.
- `C:\Users\FlowUP\Documents\KiCad\10.0\3rdparty\` is **empty**: no Konnect
  plugin is installed on this machine. That is the initial condition R.1 needs
  and it exists by accident; it is recorded here because it cannot be recreated
  once R.1 has run.
- The repository is public with **0 stars, 0 issues, no topics, no
  description-linked homepage, Discussions disabled**. That is the adoption
  baseline R.4 and R.5 move.

## Invariants de la phase

- **INV-R1 — the artefact under test is the published one.** Every step of R.1
  and R.3 runs against a file downloaded from the GitHub release page. A local
  `target/release/konnect.exe` is never substituted, not even to unblock a step.
  Rationale: Q.5 proved the published artefact can differ from what the
  repository asserts about it (D146).
- **INV-R2 — one checkbox is one proof.** A step is checked when its evidence
  exists — a command output, a file on disk, a screenshot, a KiCad verdict —
  never because it plausibly worked.
- **INV-R3 — every problem found is classified before it is fixed**, into
  exactly one of: **UX**, **packaging**, **documentation**, **configuration**,
  **product**. The class decides who fixes it and in which phase. A product
  defect discovered here does not become a silent code change in a launch phase.
- **INV-R4 — the walk is recorded as a stranger would experience it**, including
  the wrong turns. A step that only worked because the maintainer knew something
  is a documentation defect, and is logged as one.
- Existing invariants INV1–INV9 hold unchanged.

## Ordre d'exécution et dépendances internes

```
R.1  install walk from the published release      (no dependency — runs first)
  ├── R.7  kicad-cli discovery                    (opened by R.1: blocks first use)
  ├── R.2  README / Quick Start                   (needs R.1's measured walk)
  ├── R.3  canonical demo                         (needs R.1 and R.7)
  │     ├── R.8  ipc_address discovery            (opened by R.3.1, user-approved)
  │     └── R.9  triage of what run 1 found       (opened by R.3.4)
  └── R.5  feedback loop                          (needs R.1's friction list)
        R.4  public launch kit                    (needs R.2 and R.3)
        R.6  decision gate                        (needs R.4 and R.5 + real feedback)
```

R.7, R.8 and R.9 did not exist when the phase opened. R.1 found the first and
classified it **product**; R.3.1 found the second while measuring the live PCB
path, classified it **configuration**, and the user approved fixing it on
2026-08-26. Both are the same defect shape — an external address the product
requires and never derives — and neither adds a capability. R.9 came later and
is different in kind: R.3.4's first run found five **product** defects at once,
including one — routing needs a net, and nothing creates one — that is a missing
capability rather than a bug. R.9 triages them; the phase's exception, not
momentum, decides which are fixed here.

The user also decided, the same day, that **no release happens until R closes**:
R.7's and R.8's fixes ride one v1.1.1 at the end, not one release per finding.

R.2, R.3 and R.5 are independent of each other and may run in any order once
R.1 is closed. R.4 publishes nothing without the user's explicit go — it
prepares. R.6 is a decision, not an implementation.

## R.1 — The install path a stranger walks, from the published release

### Objectif
Walk **release page → install → MCP connection → real KiCad project → first
task → KiCad's verdict** on this machine, from the published assets only, and
produce a friction list where every entry carries its class (INV-R3).

### Dépendances
None. Runs first. Requires the empty-`3rdparty` initial condition recorded
above, which is consumed by R.1.3.

### Tâches
- [x] R.1.1 The initial condition is recorded before anything is installed:
      `3rdparty/` contents, KiCad version from `kicad-cli version`, absence of a
      `konnect` entry in every MCP client config on this machine
- [x] R.1.2 `konnect-pcm-v1.1.0-windows.zip` is downloaded **from the release
      page** (`gh release download`, or the browser URL a user would click), and
      its SHA-256 is recorded. No local build is used anywhere in R.1
- [x] R.1.3 The package is installed the documented way — KiCad 10 → Plugin and
      Content Manager → *Install from File* → restart KiCad — and the
      installed-plugin path is read **from disk**, not assumed from the README
- [x] R.1.4 KiCad shows the plugin where the README says it will: PCB Editor →
      *Tools → External Plugins* → **Konnect**. It does, and before the KiCad
      API is enabled — the entry is the legacy SWIG Action Plugin. Three earlier
      attempts were blocked by a UAC dialog and were left unproven rather than
      assumed (INV-R2)
- [x] R.1.11 The IPC API half of the package is checked where KiCad would show
      it. The declared toolbar button (`show-button: true`, scope `pcb`) appears
      **nowhere** — API off, API on, or after a restart (F-11). *Preferences →
      Plugins* in KiCad 10 is a single API page with no plugin list; enabling
      « Activer l'API KiCad » and restarting yields
      `Écoute à ipc://…\Temp\kicad\api.sock`, the socket every PCB tool needs
      and that KiCad ships switched off (F-09)
- [x] R.1.5 An MCP client is pointed at the installed binary using only what the
      README gives, and the connection is proved by a real handshake: the
      starter kit lists, and the tool count matches what the README claims
- [x] R.1.6 A **real KiCad project** is opened — a copy of a KiCad-shipped demo
      or a project created in KiCad, never a repository test fixture — and its
      pre-state is recorded
- [x] R.1.7 One first task is executed through MCP against that project, chosen
      as the thing a new user would try first, and the elapsed time from
      *client connected* to *task returned* is measured
- [x] R.1.8 **KiCad delivers the verdict** (INV1): the project is re-opened or
      run through `kicad-cli` and the change is confirmed to exist and to be
      readable by KiCad's own tooling
- [x] R.1.9 The friction list is written, one line per problem, each classified
      UX / packaging / documentation / configuration / product, each naming the
      file or surface that must change and the R item that will change it
- [x] R.1.10 Anything classified **product** is triaged explicitly: does it block
      installation, first use or the demo? If not, it is recorded and left
      alone — R does not fix product defects it is not blocked by

### Validation
The walk is reproducible from the written record alone: a reader who has only
the friction list and the recorded commands can repeat every step. The first
task is confirmed by KiCad, not by the tool's own success message. Every
friction entry has exactly one class.

## R.2 — README and Quick Start, written for the reader who has not decided yet

### Objectif
The current README opens with architecture and rationale — it is written for
someone already convinced. A stranger needs, above the fold: what this does,
what it costs to try, and the shortest path to seeing it work.

### Dépendances
R.1 closed. Every command and path published here is one R.1 actually ran.

### Tâches
- [x] R.2.1 A **Quick start** section sits above the essays: numbered steps from
      the release download to a first verified task, each step a copy-pasteable
      block, with the total time R.1 measured stated honestly
- [x] R.2.2 Requirements a reader must satisfy *before* step 1 — KiCad 10, a
      running KiCad for PCB tools, an MCP client — are stated at the top, not
      three screens down
- [x] R.2.3 Every documentation defect from R.1.9 is fixed at its source: the
      README, `examples/*.json`, `docs/TROUBLESHOOTING.md`, or
      `RELEASE_NOTES.md`. The fix names the observed failure, not a euphemism
- [x] R.2.4 The install path published in the README and both example configs is
      the one read from disk in R.1.3, character for character
- [x] R.2.5 The macOS and Linux caveats stay where a reader meets them **before**
      downloading, not after — unsigned binaries and un-QA'd Linux are cost
      information, not fine print
- [x] R.2.6 A first-time reader's decision is answerable in the first screen:
      what it is, what it needs, what it does not do yet

### Validation
Re-walked against its own text. Every factual claim in the Quick start is one
R.1 or R.1.11 measured: the install path (identical on disk), the 21-tool
startup surface, the install firing on file selection with *Apply Pending
Changes* inert, the API page reading `Listening on ipc://…` after a restart, and
the *External Plugins* entry needing an open project first. Anchors resolve.

**One claim is not yet proved and is R.3's job**: step 5 is written as a prompt
to a model, and no model has run it. What R.1 proved is the same work through a
scripted MCP client. The wording stays; R.3 either confirms it or changes it.

## R.3 — The canonical demo: one task, under 40 seconds, visible in KiCad

### Objectif
One short, reproducible demonstration that shows the value of an agentic MCP
over KiCad to someone who has not read a line of documentation. **The result
appears in KiCad**, not in a terminal. A terminal may be on screen; it may not
be the only thing on screen.

### Dépendances
R.1 closed — the demo runs on the installed, published binary (INV-R1).

### The task, chosen — R.3.1
**A live PCB edit over the IPC API, watched in KiCad's own canvas.** The AI
places a subcircuit's footprints on an open board and routes them; pcbnew
redraws as it happens, and KiCad's undo stack holds the result.

Measured before choosing, not assumed: with `ipc_address` set, two
`place_component` calls put two 0805 footprints on the running board in
**176 ms**, each replying `"source": "ipc"`. The write reached pcbnew, not the
file.

Against the three criteria: it is the only candidate where **KiCad itself
redraws** — every other path changes a file that something else then has to
show; two footprints snapping into place and a trace appearing between them is
not something a text editor does; and the tool time is milliseconds, so the
40 s budget is spent on the model, not on the server.

Rejected, with the reason:

- **`apply_template ldo_3v3` into a schematic** — the fastest and simplest
  (108 ms, R.1 step 7, no IPC at all), but `apply_template` **places without
  wiring** (F-07), so the picture is a scatter of symbols rather than a circuit,
  and eeschema does not redraw a file changed underneath it. It would need the
  bundled viewer, which is not KiCad.
- **The live schematic viewer refreshing as the AI edits** — genuinely
  impressive and it is the "watch it happen" feature, but the window is
  Konnect's own. The brief says the result must appear in KiCad.
- **JLCPCB part search** — real value, no visual.
- **A full manufacturing export** — the output is a folder of Gerbers. Nothing
  to watch.

Cost of the choice, stated rather than hidden: the demo needs the KiCad API on
and `ipc_address` configured (F-12). That is one line of setup, off-camera and
documented — it is not inside the 40 s.

### Amended by R.3.8, on run 1's evidence — 2026-08-26

The choice above holds: the live PCB edit is still the only candidate where
KiCad itself redraws, and the rejected candidates are rejected for the same
reasons. What did **not** hold is an assumption inside it — that a board can be
routed at all. It cannot, unless it already carries a netlist, and nothing in
the tool surface puts one there (F-13).

So the demo keeps its transport and changes its **pre-state and its task**,
decided by the user on 2026-08-26:

- **The pre-state carries its own netlist.** A real project — schematic with
  `U1` AP1117-33, `C1`/`C2` 10 µF, footprints assigned, nets `VIN`, `VOUT`,
  `GND`, ERC clean — pushed onto the board once through KiCad's own *Update PCB
  from Schematic*. The board therefore starts with three footprints in a heap
  and three real nets.
- **The task is what a layout engineer actually does with that**: place the two
  capacitors near the regulator, route the three nets, run DRC. Nothing is
  created; what exists is arranged and connected. That is one batched
  `kicad_invoke` plus a check — inside the four turns run 1 measured the budget
  to be worth.
- **The setup cost rises and is stated**: the demo now ships a schematic as well
  as a board, and the sentence that says why is the honest one — *a PCB has nets
  only because a schematic gave it some*.

### Tâches
- [x] R.3.1 The task is **chosen and justified in writing** against three
      criteria: visually unambiguous in KiCad's own canvas, under 40 s wall
      clock end to end, and impossible to mistake for something a text editor
      could have done. Rejected candidates are named with the reason
- [x] R.3.2 A **fixed starting project** is committed under `examples/` — small,
      self-contained, with a stated pre-state — so two runs start identically
- [x] R.3.3 The exact prompt is committed with it. The demo is a prompt, not a
      script: what is being demonstrated is a model doing the work
- [x] R.3.4 The demo is run end to end on the published install and **timed**.
      If it exceeds 40 s, the task is narrowed or replaced — the budget is not
      moved (INV6). Run three times on the published v1.1.0: run 1 failed its
      own criterion and returned five defects, R.3.8 narrowed the task on that
      evidence, and runs 2 and 3 passed it at 377 s and 424 s. The budget was
      **not** moved to fit them — the task was narrowed first, and when
      narrowing further was shown to be pointless (the floor is turns, not
      work), the question went to the user as R.3.10
- [x] R.3.5 The end state is verified by KiCad (`kicad-cli` ERC/DRC or a reopen),
      and the verification is part of the demo, not an afterthought. **Proved by
      run 2**: the model ran DRC itself as its last act, and `kicad-cli` run
      afterwards on the file agrees — 5 unconnected items before, **0** after,
      11 track segments, 3 silkscreen warnings, no errors
- [x] R.3.6 A capture exists — screen recording or a before/after pair — showing
      the KiCad window changing, embedded in the README and usable in R.4.
      A **before/after pair**, `resources/images/demo-{before,after}.png`: the
      committed pre-state and run 2's end state, rendered by `kicad-cli pcb
      render` at the same zoom and the same pivot, so `U1` sits on the same
      pixels in both and only what the prompt changed moves. The README section
      *What one prompt does* carries them, `examples/demo/README.md` gives the
      command that regenerates either half, and the caption quotes only what
      KiCad said — 5 unconnected before, 0 after, 11 segments, 3 warnings, no
      errors. **It carries no time claim**: that sentence is R.3.10's to write,
      once the user has decided what the 40 s figure measures
- [x] R.3.7 A second run from the committed starting state reproduces the same
      end state, proving the demo is not a lucky take. **It reproduces**
      (`docs/launch/demo-run-3.md`): 5 unconnected items before and **0** after,
      **11** track segments both times, no errors, both capacitors within 5 mm
      of `U1` — and different coordinates, a 180° rotation run 2 did not make,
      and one extra DRC error found and closed (the SOT-223 tab needs explicit
      copper to pin 2). The circuit matched; the pixels did not, which is what
      `examples/demo/README.md` asks for. It also met F-16 and F-15 live and
      turned both into **false statements in its final answer** — that KiCad was
      installed nowhere, and that Konnect had fallen back to its file engine,
      while the writes were in fact reaching the running editor over IPC. Two
      runs, two models, the same two dead ends: R.9.1 and R.9.3 were the right
      calls, and they reach users only through R.7.7's v1.1.1
- [x] R.3.10 **Opened by run 2.** The 40 s budget is re-aimed or restated, by
      the user, on the evidence of `docs/launch/demo-run-2.md`. Whatever is
      decided is written where the demo is published, in the same words as the
      measurement — a budget that moves silently is worse than a budget that was
      wrong. **Decided by the user on 2026-08-26: publish both numbers.** The
      40 s stops being a budget and becomes a named measurement of *product*
      time; the conversation time is published beside it, in minutes, as the
      number a viewer actually waits. Written after R.3.7, so the figures cover
      both runs rather than one. **Written**: `README.md` and
      `examples/demo/README.md` both carry the pair — board changes in
      **0.686 s / 0.773 s** of Konnect time (slowest single write 0.07 s;
      2.3 s / 4.7 s including KiCad's own DRC), against **377 s / 424 s** of
      wall clock over 47 / 52 turns. `demo-run-3.md` carries the per-call
      measurement the pair rests on, and `demo-run-2.md`, which asked the
      question, records the answer
- [x] R.3.9 **Opened by run 2.** There is no *route this net* tool: three nets
      cost eleven `route_trace` calls, one segment each. Recorded as a
      capability gap, classified with F-13 in R.9.4's family, and decided at R.6
      — not fixed here. Recorded in `docs/launch/demo-run-2.md` with the call
      histogram that proves it, and carried into R.6.5's candidate list so the
      gate cannot lose it
- [x] R.3.8 **Opened by run 1.** The task is narrowed or replaced on the
      evidence of `docs/launch/demo-run-1.md`, and R.3.1's justification is
      amended rather than rewritten — the rejected candidates and the reason the
      live PCB path was chosen still hold; what did not hold is the assumption
      that a board with no netlist can be routed. Done and committed: the
      pre-state under `examples/demo/` is a real project — schematic, ERC 0/0,
      footprints assigned, three nets — pushed onto the board once through
      KiCad's own *Update PCB from Schematic*; the narrowed prompt sits beside
      it; and R.3.1's justification above is **amended**, not rewritten. Run 2
      then proved the narrowed task passes

### Run 2 — 2026-08-26 — the task passed, the clock did not

Full evidence: `docs/launch/demo-run-2.md`. The narrowed task on the
netlist-carrying pre-state **succeeded**: capacitors at 4.839 mm and 4.888 mm
from `U1`, three nets closed in copper, and KiCad's own verdict — 5 unconnected
items before, **0** after, 11 segments, 3 silkscreen warnings, no errors.

And it took **377 s** across 47 turns. The shape of the calls says why: the model
routed **one segment per turn**, eleven turns of copper, and never batched
through `kicad_invoke`. At the 8–10 s per turn both runs measured, no reasonable
prompt reaches 40 s — the tool calls answer in milliseconds; the conversation
does not.

So R.3.4 stays open, and what it is blocked on is **not** the task any more. The
40 s figure was written in R.3 before anything was measured, and what it bounds
is model conversation time rather than product time. Moving it quietly to fit a
measurement is exactly what INV6 and D146 forbid, so the choice goes to the user:
re-aim the 40 s at the interval the viewer actually watches — first write to last
write inside KiCad, which is sub-second here — or publish the real number and
build R.4's claims on it. **R.3.10** carries that decision.

### Run 1 — 2026-08-26 — failed its own criterion, and found why

Full evidence: `docs/launch/demo-run-1.md`. In short: **406 s** against a 40 s
budget, 41 turns, stopped by `max_turns`; three footprints placed live in
pcbnew and **zero** track segments; `kicad-cli` DRC 0 errors, 0 unconnected,
5 warnings.

The failure is not a slow path, it is a closed one. **Routing addresses nets,
and nothing in the tool surface creates a net on a board that has no netlist**:
`route_trace` and `route_pad_to_pad` are refused with `Net 'VIN' not found on
board`, `add_net` targets a file format KiCad 10 no longer writes, no tool
assigns a net to a pad, and no tool performs *Update PCB from Schematic*. A
board has nets only if KiCad put them there from a schematic.

R.3.1's measurement did not reach this: two `place_component` calls in 176 ms
proved the **transport**, and the transport is sound. The capability behind it
is not.

Four further defects came out of the same run — F-13 to F-17, classified
**product** in `docs/launch/demo-run-1.md` and carried by **R.9**.

Two facts the run establishes for whoever narrows the task:

- **10 s per turn**, measured. A 40 s budget is about **four turns**: discover,
  load, one batched `kicad_invoke`, verify. A demo that needs a library search
  or an error recovery is already over budget.
- The published binary, configured by hand as documented, **is** reached by a
  standalone MCP client and does write into a running KiCad. The Quick start's
  step 5 claim, which R.2 left unproved, holds for placement.

### Validation
Two runs from the committed pre-state, both under 40 s, both ending in the same
KiCad-verified state, with a capture that a stranger can watch and understand
without narration.

## R.4 — Public launch kit

### Objectif
Everything needed to announce the project, drafted and reviewed **in the
repository**, so that publishing becomes a single decision rather than a writing
session. Nothing here is posted without the user's explicit go.

### Dépendances
R.2 and R.3 closed — the kit quotes the Quick start and shows the demo.

### Tâches
- [x] R.4.1 Repository metadata: description, topics, and a homepage pointing at
      the release or the demo. **Applied and verified on 2026-08-27** from
      `docs/launch/launch-kit.md` § R.4.1: a description that leads
      with what the thing is rather than with its architecture, the release page
      as homepage because the project has no site, and twelve topics — six for
      the KiCad audience, five for the MCP audience, one for the language.
      Applied with `gh repo edit`; the repository name, licence and PCM
      identifier were not changed
- [x] R.4.2 A one-paragraph pitch and a one-sentence pitch, both stating the
      limitation set (Windows most-tested, macOS unsigned, PCB tools need KiCad
      running) — a launch that hides the caveats buys a first wave of users who
      leave angry. Both written, both carrying the caveats in their own body
      rather than in a footnote
- [x] R.4.3 Long-form announcement drafts under `docs/launch/`, one per intended
      venue, each adapted rather than pasted: the audience of a KiCad forum and
      the audience of an MCP directory do not want the same first sentence.
      Four: `announce-kicad-forum.md` opens on the fear that audience actually
      has — a model writing to their files — and answers it with KiCad's undo
      and KiCad's verdict; `announce-reddit-kicad.md` shows the image first;
      `announce-hn.md` leads with the verification stance and the token
      measurement; `announce-mcp-directory.md` is four paste-ready lengths plus
      the metadata table those forms ask for
- [x] R.4.4 A candidate venue list with, for each, the submission requirement it
      imposes (format, licence statement, screenshot, maintainer account). Five
      venues in one table, the fifth — KiCad's official PCM repository — named
      as **out of R's scope** and blocked on F-03 besides. The requirements are
      dated and the kit says to re-read each venue's own rules immediately
      before posting; that is a go/no-go line, not a courtesy
- [x] R.4.5 The kit states explicitly what is **not** claimed: no success-rate
      claim beyond what `docs/benchmark.md` measured, no platform claim beyond
      Windows. Six items, and every draft repeats them in its own body: the
      18/18 is six golden tasks on one machine; the token figures are v1.0.0's
      and were not re-run; Windows only; **no claim that it is fast end to end**
      — the six-to-seven minutes is published beside the sub-second product
      time; no KiCad endorsement; parts are placed, not authored
- [x] R.4.6 Nothing is published. The phase produces drafts and a go/no-go list;
      the posting decision, and the account that posts, are the user's. Nothing
      was posted and the repository metadata was not touched. The go/no-go list
      has six lines, and the second is **ship v1.1.1 first** (R.7.7): every
      draft's install path assumes the two manual configuration steps are gone,
      so announcing before the release means either rewriting each draft or
      sending the first wave down the manual path

### Validation
The user can publish any item in the kit without editing it first. Every factual
claim in every draft traces to `docs/benchmark.md`, `RELEASE_NOTES.md`, or a
measurement made in R.1/R.3.

## R.5 — First-user feedback loop

### Objectif
A stranger who hits a wall has somewhere obvious to say so, in a shape that
answers the maintainer's questions rather than only theirs. And the maintainer
can count.

### Dépendances
R.1 closed — the friction list says which questions actually matter.

### Tâches
- [x] R.5.1 The five minimal metrics are defined in writing, each with its
      collection method and its "unknown" value: **install succeeded**,
      **time to first task**, **first blocker**, **task attempted**,
      **success / failure**
- [x] R.5.2 GitHub issue templates exist and produce those five fields as
      structured data: a *first-run report*, a *bug report*, and a *feature
      request* that does not swallow the other two
- [x] R.5.3 The first-run report is **short enough to be filled in after a
      failure** — a user who just gave up will not complete a 20-field form
- [x] R.5.4 The feedback route is discoverable from the place people fail: the
      README Quick start, the troubleshooting doc, and the release page all
      point at it
- [x] R.5.5 A tally lives in the repository (`docs/adoption.md`): one row per
      report, the five metrics, and nothing that identifies a person beyond
      their own public GitHub handle
- [x] R.5.6 Whether GitHub Discussions is enabled is decided explicitly — it is
      currently off — and the choice is recorded with its reason. **Decision: it
      stays off.** A project with no issues does not need a second empty
      surface; splitting a handful of early reports across two places makes both
      look dead and makes the tally harder to keep honest. Revisit when
      questions that are *not* bug reports outnumber the ones that are
- [x] R.5.7 It is stated in the repository that no telemetry exists and none is
      planned. A tool that edits a user's design files earns trust by not
      phoning home

### Validation
A stranger can file a first-run report in under two minutes from the link the
README gives, and the resulting issue contains all five metrics without a
follow-up question from the maintainer.

## R.6 — Decision gate for the next technical phase

### Objectif
End R with a **decision founded on real feedback**, not with a new technical
phase started by momentum.

### Dépendances
R.4 and R.5 closed, and real feedback received — or an explicit finding that
none arrived, which is itself evidence.

### Tâches
- [x] R.6.1 The promotion criteria are written **before** the feedback is read:
      what evidence would promote each named candidate — Dependabot hygiene,
      macOS signing, the official PCM submission, symbol/footprint authoring,
      KiCad 11 / plan item I.1, Linux QA. Criteria written after the data are
      not criteria. Eleven candidates, one criterion each, in
      `docs/launch/decision-gate.md` § 4 — the six named here plus R.6.5's four
      and *reach* itself. The order of writing is stated in the document rather
      than assumed: the tally was read first and is **empty**, and an empty
      tally can select nothing, so no criterion could have been fitted to it
- [x] R.6.2 The R.1 friction list, the R.3 demo result and the R.5 tally are
      summarised into one page a decision can be made from. One page,
      `docs/launch/decision-gate.md`: eleven frictions with their disposition
      today, three demo runs with what each one bought, and the tally
- [x] R.6.3 The gate is presented to the user with a recommendation and its
      evidence. The user decides; R does not open the next phase.
      **Recommendation: publish, then decide the rest with data.** Nine of the
      eleven criteria name a first-run report, an outside download or an outside
      install — inputs no further engineering can produce. Order: ship v1.1.1,
      apply the metadata and post the kit, re-open the gate when the tally stops
      being zero. Explicitly **not** recommended: opening a PCB-capability phase
      on run 1's five defects
- [x] R.6.4 If no feedback arrived, the gate says so and the decision is made on
      the R.1 friction list alone — an empty tally is a finding about reach, not
      a reason to postpone the decision. **None arrived**: 0 stars, 0 forks, 0
      outside issues, 0 first-run reports, 2 downloads and both the
      maintainer's. The gate says so in its first section, and says the one
      thing that follows from it — the project has never been announced
      anywhere, so zero reach produced zero feedback whatever the software is
      like. The tally is evidence about distribution, not about demand
- [x] R.6.5 **Opened by R.3 and R.9.** The candidates the demo produced are
      named alongside the ones R.6.1 already lists, each with the artefact that
      found it: **nets on a board** (F-13, R.9.4 — no tool creates or assigns
      one, and no *Update PCB from Schematic* equivalent exists), **a route
      this net tool** (R.3.9 — three nets cost eleven `route_trace` calls),
      **PCB reads over IPC** (F-15, R.9.3 — two reads still read the file while
      every write goes to the running editor), and **IPC placement matching the
      library** (F-17, R.9.5). None of them is promoted here; R.6.1's rule
      holds — the criteria are written before the evidence is read

### Validation
One page, one recommendation, every claim on it traceable to an R artefact. The
next phase is opened by the user, in a separate decision, after R is closed.


## R.7 — The one defect that blocks first use — OPENED BY R.1

### Objectif
R.1 found exactly one problem that satisfies this phase's narrow exception: it
**directly blocks first use**, on a default Windows KiCad install, for every
capability that depends on `kicad-cli`. Fix that one, and nothing else. The full
diagnosis is `docs/launch/first-run-walk.md` § *F-01 in detail*.

The defect: the server's `kicad_cli` default is the bare name `kicad-cli.exe`
(`crates/konnect/src/config.rs:75`), resolved through `PATH`, and KiCad's Windows
installer does not put its `bin` on `PATH`. `detect_kicad()`
(`crates/konnect/src/install.rs:402`) is never called by the server, and would
miss this machine anyway: its Windows path list has no
`%LOCALAPPDATA%\Programs\KiCad` entry — while its macOS branch does handle the
per-user case — and its registry probe reads `HKLM\SOFTWARE\KiCad\10.0`, a key
that exists in neither hive here.

Consequence: `verify:"auto"`, ERC, DRC and every export fail with
`Failed to spawn kicad-cli: kicad-cli.exe`. INV1 says the verdict is KiCad's; on
a stock install the server cannot obtain it.

### Dépendances
R.1's diagnosis, complete. Blocks **R.3** — a demo whose result is verified by
KiCad cannot run while the verdict path is broken — and feeds **R.2** (F-02, the
README claim that auto-detection happens).

### Invariants
- The fix is a **discovery path only**. No tool signature changes, no new tool,
  no behaviour change when `kicad_cli` is already configured or already on
  `PATH`.
- A discovery that fails still fails loudly. The current error message is
  correct and must survive: silently continuing without a validator is the
  failure mode INV4 exists to prevent.

### Tâches
- [x] R.7.1 A failing test first, on the observable: with `kicad_cli` unset and
      nothing named `kicad-cli` on `PATH`, resolution finds a KiCad installed
      under a per-user prefix
- [x] R.7.2 The server resolves `kicad_cli` at startup instead of trusting a
      bare name: explicit config → `PATH` → known install prefixes → registry.
      The first hit wins and is logged once, so a user can see which KiCad
      answered
- [x] R.7.3 The Windows prefix list gains `%LOCALAPPDATA%\Programs\KiCad\<ver>`,
      and the registry probe reads the uninstall key that a per-user install
      actually writes — `HKCU\…\Uninstall\KiCad <ver>` → `InstallLocation` —
      in addition to `HKLM`
- [x] R.7.8 **Found in review of R.7.3, fixed there.** The candidate list was
      ordered prefix-first — every 10.0 root, then every 9.0 root, then the
      per-user prefix appended behind both — so a machine carrying a
      system-wide KiCad 9 and a per-user KiCad 10 would have resolved to the
      **9**. Reordered version-first, prefix-second, with a test that asserts
      it: `every_candidate_of_a_newer_version_comes_before_any_older_one`
- [x] R.7.4 `plugin/settings_dialog.py::detect_kicad_cli` learns the same two
      locations, so the PCM settings dialog and the server agree — including
      R.7.8's ordering, whose pre-existing root-first loop had the same
      inversion and would have disagreed with the server it is meant to match
- [x] R.7.5 The gate is green on the changed tree: `fmt` clean,
      `clippy --workspace --locked --all-targets -- -D warnings` silent, and
      the full suite at **1 392 passed, 0 failed, 38 ignored across 57
      suites** — 1 385 at v1.1.0, plus this lot's seven new tests
- [x] R.7.6 The walk of R.1 step 8 is repeated on the fixed binary and
      `verify:"auto"` returns KiCad's ERC counts instead of an `io` error.
      Re-run by the principal, not taken from the worker's report: startup logs
      `kicad_cli: found at standard install path -> …\AppData\Local\Programs\KiCad\10.0\bin\kicad-cli.exe`
      and the validator answers `{"check":"erc","errors":0,"warnings":0}`.
      The negative case is proved too — an explicitly configured
      `konnect-no-such-kicad-cli` is logged as *using configured value as-is*
      and still fails with `Failed to spawn kicad-cli`, so INV4 holds and no
      silent substitution was introduced
- [x] R.7.7 The release question is **put to the user, not decided here**: this
      fix only reaches users through a new artefact, and F-03
      (`packaging/metadata.json` pointing at the upstream repository) would ride
      the same release. Whether v1.1.1 happens inside R is the user's call.
      **Decided by the user on 2026-08-26: yes, one v1.1.1 at the end of R** —
      not now, and not one release per finding. It carries R.7 (`kicad_cli`
      discovery), R.8 (`ipc_address` derivation), R.9.1 (the `kicad` GUI binary)
      and R.9.2 (the undeclared stackup), plus F-03. Until it ships the README
      keeps saying plainly that v1.1.0 needs the manual steps. The release
      itself is a bounded plan of its own, validated before anything moves

### Validation
On this machine, with no `kicad_cli` in any config and no `kicad-cli` on `PATH`,
a `kicad_invoke` with `verify:"auto"` returns an ERC verdict from KiCad. The
gate is green. Nothing else in the tool surface moved — `v1.1.0..HEAD` registers
no new tool and no changed signature.

## R.8 — The PCB half of the product configures itself — OPENED BY R.3.1

### Objectif
Same defect shape as R.7, on the other transport. Every PCB tool needs
`ipc_address`, and nothing derives it: `default_ipc_address()`
(`crates/konnect/src/config.rs`) reads `KICAD_API_SOCKET`, an environment
variable that exists **only when KiCad launches the plugin itself**. A
standalone MCP client — Claude Desktop, Claude Code, the configuration this
project's own README documents — never has it, so every PCB tool fails until the
user copies an `ipc://` path out of KiCad's preferences by hand.

The path is deterministic: KiCad opens its socket under the system temp
directory, `<temp>/kicad/api.sock`. On this machine, measured from KiCad's own
preferences page: `ipc://C:\Users\FlowUP\AppData\Local\Temp\kicad\api.sock`.

Decided by the user on 2026-08-26, alongside R.7's release question: derive it,
same treatment as `kicad-cli`.

### Dépendances
R.3.1's measurement — with `ipc_address` set explicitly, the live IPC path works
and answers in 176 ms. The transport is sound; only its address is missing.

### Invariants
- **Address resolution only.** No transport change, no new tool, no behaviour
  change when `ipc_address` is already configured or `KICAD_API_SOCKET` is set.
- **A wrong address still fails loudly**, with the remedy the current message
  already gives. INV4: a caller must never believe a PCB write landed when no
  KiCad was listening.
- On Windows an `ipc://` address is an NNG **named pipe**, not a file: the
  directory `%LOCALAPPDATA%\Temp\kicad\` is empty even while KiCad is listening.
  Resolution therefore **constructs** the Windows path and must not gate it on a
  filesystem existence check.

### Tâches
- [x] R.8.1 A failing test first, on the observable: with `ipc_address` empty and
      `KICAD_API_SOCKET` unset, resolution yields the platform's default socket
      address instead of an empty string
- [x] R.8.2 The chain, first hit wins, logged once at startup like R.7's:
      explicit `ipc_address` → `KICAD_API_SOCKET` → the platform default
- [x] R.8.3 The platform default is right on all three: Windows
      `<std::env::temp_dir()>\kicad\api.sock`, constructed and not existence-
      checked; macOS `/tmp/kicad/api.sock`, which is what `README.md` already
      documents and is **not** what `temp_dir()` returns there; Linux
      `/tmp/kicad/api.sock`
- [x] R.8.4 The `not_configured` error text stops linking
      `github.com/mixelpixx/Konnect` and points at this repository's
      `docs/TROUBLESHOOTING.md`. Its three-step remedy is good and is kept
- [x] R.8.5 The gate is green on the changed tree: `fmt`, `clippy -D warnings`,
      the full suite
- [x] R.8.6 End to end on this machine, with `ipc_address` configured **nowhere**
      and `KICAD_API_SOCKET` unset: a PCB tool reaches the running KiCad and
      replies `"source": "ipc"`. And the negative case — KiCad not listening —
      still fails with the actionable message. Re-run by the principal:
      `ipc_address: using platform default -> ipc://…\Temp\kicad\api.sock`,
      then `get_component_list` answering `ok` with the two footprints R.3.1
      left on that board
- [x] R.8.7 **Found in review of R.8.2, fixed there.** Deriving the address makes
      `not_configured` nearly unreachable — which is the point — but that path
      carried the only guidance for the commonest beginner failure. What a user
      now meets is the *unreachable* message, and it named the address without
      naming the fix: KiCad ships the API **off**, and a running KiCad with the
      board open still refuses the connection until it is switched on. The
      sentence moved to `crates/konnect-core/src/tools/ipc_boundary.rs`, where a
      first-time user actually reads it

### Validation
A user who follows the README's Quick start, enables the KiCad API and opens a
board can call a PCB tool without ever seeing an `ipc://` string. Nothing else
in the tool surface moved.
## R.9 — What the demo run found, triaged — OPENED BY R.3.4

### Objectif
Run 1 of the demo returned five product defects (F-13…F-17,
`docs/launch/demo-run-1.md`). This lot **classifies them and fixes only what the
phase's narrow exception allows**. It is a triage lot, not a capability lot: R
does not become a PCB development phase because a demo failed.

### Dépendances
R.3.4's run 1, and the evidence document it produced.

### Invariants
- The phase exception is unchanged: a capability is added **only** if the defect
  directly blocks installation, first use, or the public demo, **and** the fix
  is the minimum one. A defect that is merely embarrassing waits for R.6.
- Nothing here is fixed before it is classified (INV-R3).

### Tâches
- [x] R.9.1 **F-16 — `launch_kicad_ui` cannot find `kicad`.** Same defect shape
      as R.7, same fix: D149's chain applied to the `kicad` executable.
      Small, bounded, and it removes a dead end a model walked into.
      The chain itself moved to `konnect_core::kicad_locate` — `konnect`
      depends on `konnect-core`, so the resolver could not stay in the
      installer and still be reachable from the tool that spawns the GUI.
      `find_kicad_binary` (`tools/verification.rs`), which used to scan four
      hardcoded `C:`/`D:` roots and miss the per-user prefix entirely, now
      calls it; `main.rs` resolves `kicad_binary` at startup and logs it once,
      like `kicad_cli`. Proved end to end on this machine by the principal,
      through the freshly built binary with `kicad_binary` configured nowhere:
      `kicad_binary: found at standard install path -> …\AppData\Local\Programs\KiCad\10.0\bin\kicad.exe`,
      then `launch_kicad_ui` answering `{"launched":true}` with a `kicad`
      process alive to show for it. INV4's negative case holds too — a
      configured `konnect-no-such-kicad.exe` is logged *using configured value
      as-is* and still fails `Failed to launch KiCAD (…): program not found`
- [x] R.9.2 **F-14 — `get_layer_list` calls a valid board malformed.** A board
      KiCad opens without complaint is reported `malformed_document: no (layers)
      section`. Either the reader tolerates the absent section or the error says
      what is actually wrong; a wrong diagnosis is worse than an error.
      The reader tolerates it: `get_layer_list` answers with KiCAD's own
      default stackup, flagged `"declared": false` and with a note saying the
      table was not read from the file. `add_layer` still refuses — there is
      no table to insert into — but its message now says how to get one
      (`open it once in KiCAD's PCB editor and save`) instead of calling the
      board malformed; only `error.kind`, the machine discriminant, stays
      `malformed_document`. KiCad's own verdict backs both halves: the
      four-`gr_line` board with no `(layers)` passes `kicad-cli pcb drc`
      (0 violations, 0 unconnected)
- [x] R.9.3 **F-15 — reads and writes disagree about where the board is.** PCB
      writes go to the running pcbnew over IPC; `get_component_pads` and
      `get_pad_position` read the file on disk, so a footprint just placed is
      invisible until something saves. Decide and record whether R fixes this or
      documents it — the run lost several turns to it, and so will every user.
      **Decided: R documents it.** Rerouting the read means moving the whole PCB
      read surface onto IPC, and the alternative — saving the user's board
      behind their back before every read — changes their file without being
      asked. Both are capability-scale, and the demo passes without either, so
      the phase's own rule keeps them out. What R does instead is stop the
      silence: both pad reads now answer `"source": "file"`, like every other
      split read in this crate, `docs/TROUBLESHOOTING.md` gains the symptom
      under the words a user would use for it, and both tool descriptions say
      where they read from. `both_pad_reads_declare_that_they_read_the_file`
      holds the disclosure in place. The rerouting itself is a named candidate
      for the R.6 gate
- [x] R.9.4 **F-13 — routing is unreachable on a board without a netlist.** This
      is a missing capability, not a bug: creating or assigning nets, or an
      *Update PCB from Schematic* equivalent. It is **out of R's scope** by the
      phase's own rule; it is recorded here and becomes a named candidate for
      the R.6 gate, where evidence decides it. **Recorded, not fixed**, and R.3.8
      already paid its cost in the open: the demo's pre-state carries a
      schematic, and `examples/demo/README` says plainly that a PCB has nets
      only because a schematic gave it some and that the *Update PCB from
      Schematic* step is setup rather than demo, because Konnect has no
      equivalent. That sentence is the user-facing half of this record
- [x] R.9.5 **F-17 — `lib_footprint_mismatch`.** Footprints placed over IPC do
      not match the library copy they name. Classified and recorded; fixed only
      if R.9.2's or R.9.3's work makes it trivial. Neither made it trivial —
      they touch a stackup reader and two file reads, not the geometry a
      placement sends over the wire — so it is **recorded, not fixed**. Run 2
      bounds it usefully: its footprints reached the board through KiCad's own
      *Update PCB from Schematic* and were only *moved* over IPC, and its DRC
      returned three silkscreen warnings and no mismatch at all. So the defect
      belongs to IPC placement building a footprint, not to IPC touching one,
      which is why the demo does not meet it. **Product, minor**, and a named
      candidate for the R.6 gate

- [x] R.9.6 **Found in review of R.9.2, fixed there.** The first answer for an
      undeclared stackup was *two* layers, F.Cu and B.Cu. That is what KiCAD's
      copper default is, not what KiCAD's default *is*: handed the same board,
      `kicad-cli pcb upgrade` writes back **24** layers — the two copper ones
      and twenty-two technical ones, `Edge.Cuts` among them, which is the very
      layer such a board is already drawing its outline on. A caller asking
      whether `Edge.Cuts` exists would have been told no. The measured table
      lives in `konnect_sexp::layers::default_stackup()`, and
      `default_stackup_ids_are_the_canonical_ones` ties it to `canonical_id`
      under `Numbering::Modern`, so two independent measurements of KiCAD's
      scheme cannot drift apart

### Validation
Every one of F-13…F-17 is either fixed with a test that would have caught it, or
recorded with the reason it waits and the phase it waits for. No defect leaves R
unclassified.

## R.10 — v1.1.1, the release R decided to ship — OPENED BY R.7.7

### Objectif
Four discovery fixes and one packaging fix reach a user only through a new
artefact. R.7.7's decision was **one v1.1.1 at the end of R**, not a release per
finding. This lot is that release, and nothing else travels in it.

### Dépendances
R.9 closed (its fixes are in the tree, gate green). R.4's kit assumes this
release has happened: every draft's install path is the config-free one.

### Invariants
- **Scope is closed.** R.7, R.8, R.9.1, R.9.2 and F-03. No opportunistic fix
  rides along; a defect found while releasing is recorded, not fixed in the tag.
- **D144** — the real-KiCad E2E is run by hand **before** the tag, never after.
- **D146** — any public figure this release does not re-measure is either
  re-measured on the published artefact or explicitly dated to the version that
  did measure it.
- **INV-R1** — what is verified at the end is the **published** artefact, not
  the local build that produced it.

### Tâches
- [x] R.10.1 **F-03**, and only the half that is safe: `packaging/metadata.json`
      gains this fork's author and homepage, so the Plugin Manager stops sending
      a first user to the upstream issue tracker. The `identifier`
      (`com.github.mixelpixx.konnect`) is **kept**: it is the install directory
      name, and it appears in the README, both example configs, the demo harness
      and every existing install. Renaming it would break all of them to fix a
      cosmetic string, and the reason is written down where the file is
- [x] R.10.2 Version bump to 1.1.1 — workspace `Cargo.toml`, the viewer crate,
      `Cargo.lock` — and nothing else claims a version by hand
- [x] R.10.3 `RELEASE_NOTES.md` is rewritten as the **body of v1.1.1** (D143),
      not appended to: what changed for a user (the two discovery chains, the
      GUI binary, the undeclared stackup, the PCM metadata), what did not
      (no new tool, no changed signature), and which figures still belong to
      v1.0.0
- [x] R.10.4 Every sentence that documents the manual steps as *required* is
      updated where it lives — `README.md`'s status block and Quick start,
      `docs/TROUBLESHOOTING.md`, `examples/*.json`. A release that removes a
      manual step and leaves the documentation demanding it has not removed it
- [x] R.10.5 The gate is green on the release commit, and the real-KiCad E2E is
      run **by hand before the tag** (D144)
- [x] R.10.6 Tag, push, and the release workflow's seven assets are checked for
      presence and size on the release page itself
- [x] R.10.7 **The published artefact is installed and walked** on this machine
      with **no `konnect-settings.json` at all**: PCM install from the published
      zip, KiCad API on, one PCB tool and one `kicad-cli`-backed check answering
      without either path configured by hand. That is the claim v1.1.1 exists to
      make, and R.1's walk is what it is measured against
- [x] R.10.8 The two `%LOCALAPPDATA%` discovery tests introduced by R.7/R.9.1
      run only on Windows. They construct a Windows-only install tree and must
      not fail the macOS/Linux PR gate after the release workflow has passed

### Validation
A user who downloads v1.1.1 and follows the Quick start reaches a KiCad-verified
result without editing a configuration file, on this machine, proved on the
published artefact. `RELEASE_NOTES.md` describes v1.1.1 and no other version.
Nothing outside the closed scope changed between v1.1.0 and the tag.

## Critères de sortie de la phase R

- [x] A stranger's path from the release page to a KiCad-verified first task is
      walked, measured, and written down (R.1) — `docs/launch/first-run-walk.md`,
      eleven frictions, detours included
- [x] The README answers *what, what it costs, how to start* in its first screen
      (R.2). R.3.6's before/after pair was added above the Quick start and then
      **compressed** to keep that true: the images and four lines, with the
      per-call numbers left to the run documents
- [x] ~~One demo, under 40 s~~, verified by KiCad, reproducible from a committed
      starting state (R.3). **Amended by R.3.10**, on the user's decision and on
      three measured runs: the demo is verified by KiCad and reproduces from the
      committed starting state, and its time is published as the two numbers
      that were actually measured — under a second of board changes, six to
      seven minutes of conversation. The 40 s was written before anything had
      been run, and is struck rather than quietly re-aimed
- [x] A launch kit the user can publish without rewriting (R.4)
- [x] A feedback route that yields the five metrics without a follow-up question
      (R.5) — three issue forms, `docs/adoption.md`, and the baseline it tallies
      against
- [x] A written decision for the next phase, with its evidence (R.6) — written
      and put to the user; the phase it opens is the user's to open

# Phase S — Correctif E2E bibliothèque projet et IPC document-aware

## Objectif final

Rendre compatibles `register_symbol_library(scope=project)` et
`add_schematic_component`, puis supprimer les faux échecs IPC/path discovery
révélés par le benchmark Hi-Fi, sans contournement par édition directe des
documents KiCad ni régression PCB.

## Invariants

- Une bibliothèque projet est résolue depuis le projet/document concerné, jamais
  depuis le CWD implicite ; `${KIPRJMOD}` est relatif au projet.
- La logique existante est consolidée plutôt que dupliquée.
- `kicad_invoke` conserve ses garanties transactionnelles et une découverte de
  documents conservatrice.
- Les commandes IPC envoyées correspondent au type réel du document/éditeur.
- Aucun document du projet Hi-Fi utilisateur n'est utilisé pour développer le
  correctif.

## S.1 — Reproduction et résolution de bibliothèque projet

### Tâches

- [x] S.1.1 Inspecter architecture, tests, commits récents et tracer le chemin
  `register_symbol_library` → `sym-lib-table` → `add_schematic_component`.
- [x] S.1.2 Ajouter une régression E2E temporaire reproduisant
  `TestLocal:TEST_IC`, puis corriger le resolver et ses erreurs structurées.
- [x] S.1.3 Prouver bibliothèque projet, `${KIPRJMOD}`, persistance et
  non-régression `Device:R` par tests automatiques.

### Validation

Le test échoue sur le comportement antérieur et passe après correctif ; le
schéma relu contient instance, référence, `lib_id`, symbole embarqué et pins.

## S.2 — IPC document-aware et découverte transactionnelle des chemins

### Tâches

- [x] S.2.1 Reproduire/auditer `open_project`, `save_project`,
  `GetOpenDocuments`, `save_board` contre protobufs et handlers KiCad 10.
- [x] S.2.2 Corriger le routage PCB/schematic et la classification
  `AS_UNHANDLED` sans casser l'API publique.
- [x] S.2.3 Durcir `no_project_path_found` pour `schematic`, `board`, `.kicad_pro`,
  outils sans chemin et `documents` explicites, avec tests ciblés.

### Validation

Les tests document-aware et rollback passent, les usages PCB existants restent
verts et aucun faux `no_project_path_found` ne touche le scénario S.1.

## S.3 — Validation globale, live et livraison

### Tâches

- [x] S.3.1 Passer format, lint/typecheck/build et suites pertinentes.
- [x] S.3.2 Sur projet temporaire et KiCad 10.0.3, valider création,
  enregistrement, placement, lecture, sauvegarde/relecture pour symbole projet,
  puis `Device:R`.
- [x] S.3.3 Examiner le diff, préserver les changements utilisateur et créer un
  checkpoint Git propre limité aux fichiers de la phase S.

### Validation

Les preuves permettent de reprendre l'étape B1.1 du benchmark Hi-Fi, suivie
ici sous T.1.1 pour éviter la collision avec la tâche historique B.1.1, et
aucun fichier Hi-Fi utilisateur n'a été modifié.

# Phase T — Reprise du benchmark Hi-Fi

## T.1 — Placement depuis une bibliothèque projet

### Dépendances

Phase S validée ; projet Hi-Fi ouvert dans KiCad 10.0.3.

### Tâches

- [x] T.1.1 Exécuter l'étape B1.1 du benchmark Hi-Fi : placer via le MCP
  `HifiAmp_TPA3255_Local:LM5010ASD` comme U1 sans modifier directement le
  document KiCad ni dupliquer une instance existante.

### Validation

Après sauvegarde et relecture, le schéma contient exactement une instance U1
avec ce `lib_id`; les autres documents et éléments utilisateur sont préservés.

## T.2 — Suite du benchmark

### Tâches

- [ ] T.2.1 Définir la prochaine étape fonctionnelle et ses critères de
  validation à partir du brief du benchmark Hi-Fi.

### Validation

La prochaine modification du projet et son résultat attendu sont explicitement
identifiés avant toute nouvelle écriture.

# Phase U — Publication v1.1.2

## U.1 — Patch release du correctif Phase S

### Objectif

Publier `v1.1.2` sans changement fonctionnel supplémentaire, puis reprendre la
phase T à son état existant.

### Dépendances

Phase S et T.1.1 validées ; commit `9bcd9fb` inclus dans le candidat ; ancien
exécutable de rollback préservé.

### Tâches

- [x] U.1.1 Vérifier branche, HEAD, worktree, dernier tag/release, stratégie de
  version, notes, workflows et cohérence du `schematic-viewer` exclu.
- [x] U.1.2 Mettre à jour strictement les versions, lockfiles et release notes
  nécessaires pour `1.1.2`, avec la limite IPC document-aware explicite.
- [x] U.1.3 Exécuter le gate local de release, dont la régression bibliothèque
  symbole projet et les validations séparées du `schematic-viewer`.
- [x] U.1.4 Committer et pousser le candidat sur `agentic/main`, puis vérifier
  la CI PASS pour exactement ce commit.
- [x] U.1.5 Créer et pousser le tag annoté `v1.1.2` sur le commit validé, puis
  vérifier le workflow, la GitHub Release et au moins un artefact publié.
- [x] U.1.6 Confirmer la propreté finale et rétablir comme prochaine action la
  poursuite du benchmark Hi-Fi à T.2.1.

### Validation

Version `1.1.2`, gate local et CI du commit tagué PASS, tag et Release GitHub
présents, artefacts attendus publiés, un artefact inspecté avec version et
package PCM cohérents, worktree propre et aucun changement fonctionnel ajouté.

# Phase V — Routage DocumentType Eeschema

## V.1 — Diagnostic et régression

### Objectif

Reproduire hors projet Hi-Fi pourquoi Konnect `v1.1.2`, lancé depuis Eeschema,
émet `DOCTYPE_PCB`, puis verrouiller le contrat Eeschema/PCB/contexte inconnu.

### Tâches

- [x] V.1.1 Cartographier launcher, manifeste, contexte KiCad, transport IPC et
  construction/transmission de `DocumentType` pour Eeschema et Pcbnew.
- [x] V.1.2 Produire un cas minimal hors projet Hi-Fi qui échoue sur `v1.1.2`.
- [x] V.1.3 Ajouter une régression rouge discriminant Eeschema → schematic,
  Pcbnew → PCB et contexte indéterminé → erreur explicite.

### Validation

La cause exacte est prouvée par le code et un test échoue avant le correctif
pour Eeschema sans dépendre du projet Hi-Fi.

## V.2 — Correctif et validations locales

### Tâches

- [x] V.2.1 Appliquer le plus petit correctif à la source sans fallback PCB.
- [x] V.2.2 Passer les tests ciblés, compatibilité IPC/plugin/pipes,
  `open_documents`, `save_project` et protections document-aware.
- [x] V.2.3 Passer le gate complet et les E2E/CI pertinents.

### Validation

Eeschema résout explicitement `DOCTYPE_SCHEMATIC`, Pcbnew conserve
`DOCTYPE_PCB`, l'indéterminé refuse sans mutation et toutes les gates passent.

## V.3 — Validation réelle et benchmark

### Tâches

- [x] V.3.4 Préalable à V.3.1 : rétablir un serveur API IPC KiCad fonctionnel.
  Deux préconditions, indépendantes de la version de KiCad — `10.0.3` et
  `10.0.6` se comportent à l'identique.
  1. Aucun doublon d'identifiant de plugin sous `3rdparty`. Trois répertoires
     déclarant `com_github_mixelpixx_konnect` (le plugin vif plus deux copies
     de rollback) tuent l'éditeur 3 s après le démarrage, `0xC0000005` dans
     `wxbase332u_vc_x64_custom.dll`, le pipe étant publié puis perdu. Un seul
     ou deux répertoires ne reproduisent pas. Les copies de rollback vivent
     désormais hors de `3rdparty`.
  2. Aucun dialogue modal au démarrage. L'assistant `Configuration de KiCad`
     fait répondre `AS_NOT_READY` sur un pipe pourtant présent ; la validation
     utilise un `KICAD_CONFIG_HOME` dédié où `do_not_show_again` est répondu.
- [x] V.3.1 Installer une build de développement avec rollback identifié, puis
  valider Eeschema : pipes, lecture, `save_project`, réouverture et persistance.
  La session de validation doit avoir chargé `kicad-agentic-mcp` au démarrage
  ou à la reprise : Codex ne garantit aucun rechargement MCP à chaud.
- [x] V.3.2 Effectuer le smoke-test Pcbnew et le smoke-test Hi-Fi non destructif,
  sans poursuivre B1.3 ni éditer directement les fichiers KiCad.
- [x] V.3.3 Consigner `MCP_BUG — incorrect DocumentType routing for Eeschema`
  avec cause, impact, reproduction, correctif, validations et état Hi-Fi.

### Validation

Les deux éditeurs et le projet Hi-Fi préservé confirment le correctif réel ;
l'échec reste visible dans le benchmark.

## V.4 — Patch release v1.1.3

### Tâches

- [x] V.4.1 Préparer version, lockfiles et notes selon la politique existante.
- [x] V.4.2 Committer/pousser le candidat, obtenir la CI PASS sur ce commit.
- [x] V.4.3 Taguer `v1.1.3`, vérifier workflow, release et artefacts publiés.
- [x] V.4.4 Installer `v1.1.3` comme seule version en vigueur pour les clients,
  vérifier le runtime actif et conserver uniquement le rollback explicite.

### Validation

Le tag pointe sur le commit réellement validé, les artefacts sont cohérents et
`v1.1.3` est la version active sans fallback vers un ancien binaire.

# Phase W — v1.1.4, les trois limitations Pareto

## Objectif

Trois limitations démontrées par le benchmark Hi-Fi bloquent des workflows
réels. Elles sont corrigées ensemble, séquentiellement, chacune vérifiée avant
la suivante :

1. un `.kicad_sch` possédé par Eeschema peut être muté sous lui, et la
   sauvegarde de l'éditeur écrase silencieusement la mutation ;
2. `create_footprint` dérive le courtyard de la seule enveloppe des pastilles,
   et aucun outil n'édite ensuite les graphiques d'une empreinte ;
3. `on_board`, `in_bom` et `dnp` ne sont ni lisibles ni écrivables.

## Invariants de la phase

- L'architecture ne bouge pas : PCB par IPC, schématique par S-expression
  contrôlée, ERC/DRC/exports par `kicad-cli`. Le routage `DocumentType` de
  `v1.1.3` n'est pas touché.
- Un lock KiCad n'est jamais supprimé, jamais déplacé, jamais réputé périmé.
  Le fichier de lock ne porte que `hostname` et `username` : la fraîcheur n'y
  est pas décidable, donc elle n'est pas décidée.
- Ambigu vaut refus. Une mutation refusée ne laisse ni octet modifié, ni
  scratch, ni journal.
- Aucun test n'est supprimé ni affaibli pour obtenir du vert.

## Ancrages vérifiés sur cette machine, pas décrits

Sonde `scratchpad/probe-lock.ps1`, KiCad 10.0 réel, fixture
`KonnectValidationV31`, profil `KICAD_CONFIG_HOME` dédié :

- ouvrir `X.kicad_sch` dans `eeschema.exe` crée **deux** fichiers frères,
  `~X.kicad_sch.lck` et `~X.kicad_pro.lck`, dans le répertoire du document ;
- leur contenu est exactement `{"hostname":"…","username":"…"}`, 50 octets,
  **sans PID ni horodatage** ;
- une fermeture propre (`WM_CLOSE`) retire les deux.

Frontière commune d'écriture, relevée dans le code et non supposée : tout
chemin de mutation schématique passe par `crates/konnect-sexp/src/writer.rs`
(`write_atomic`, `write_atomic_if_unchanged`, `transact_atomic`,
`write_new_atomic`) ou par `crates/konnect-sexp/src/transaction.rs`
(`commit_file_transaction`). `commit_command` de `command.rs` délègue à
`transact_atomic`.

## W.1 — Garde de possession Eeschema

### Objectif

Aucune mutation d'un `.kicad_sch` dont le lock frère natif existe.

### Tâches

- [x] W.1.1 Écrire les régressions rouges d'abord : lock présent → refus
  typé, fichier bit-identique, aucun scratch, aucun journal, lock intact ;
  lock absent → la même mutation passe.
- [x] W.1.2 Implémenter le garde dans `konnect-sexp::writer`, au niveau
  partagé, avec un nouveau `SexpError` distinct de `Conflict`.
- [x] W.1.3 Recontrôler au plus près de la frontière de commit, juste avant
  le `rename`, et prouver par un test de course que le garde n'est pas
  seulement en entrée d'opération.
- [x] W.1.4 Étendre `commit_file_transaction` : refus avant toute écriture de
  journal.
- [x] W.1.5 Classer l'erreur dans la taxonomie MCP et la rendre lisible par
  un client (`error_kind`, `transient`).
- [x] W.1.6 Valider contre un Eeschema réel : mutation refusée éditeur
  ouvert, acceptée après fermeture, relecture indépendante.

### Validation

Sur fixture ouverte dans Eeschema : refus typé, SHA-256 du `.kicad_sch`
inchangé, répertoire du projet sans fichier nouveau autre que ceux de KiCad,
lock intact. Après fermeture : même appel PASS et modification relue.

## W.2 — Graphiques d'empreinte et courtyard

### Objectif

Créer une empreinte correcte, puis la corriger sans quitter le MCP.

### Tâches

- [ ] W.2.1 Chemin vertical minimal : lire les graphiques d'un `.kicad_mod`,
  modifier une primitive, écrire, relire, vérifier.
- [ ] W.2.2 Étendre aux primitives réelles : `fp_line`, `fp_arc`, `fp_rect`,
  `fp_circle`, `fp_poly`.
- [ ] W.2.3 Corriger le courtyard de `create_footprint` : enveloppe du corps
  **et** des pastilles, plus la garde selon les conventions vérifiées.
- [ ] W.2.4 Corriger le repère de broche 1 : plus de repère imposé sur un
  composant non polarisé, et un repère qui reste dans le courtyard.
- [ ] W.2.5 Rejouer les deux empreintes Hi-Fi défectueuses, sans édition
  externe.

### Validation

`CF_Film_Box_P5.00mm_7.2x3.5mm` : courtyard au moins aussi grand que le corps.
`Fuse_Schurter_UMT-H_5.3x16mm` : plus de faux repère de broche 1 hors
courtyard. Les fixtures existantes passent, ou leur changement est justifié.

## W.3 — Attributs natifs `on_board`, `in_bom`, `dnp`

### Tâches

- [ ] W.3.1 Lecture : exposer les trois attributs dans les réponses des
  outils de lecture de symboles.
- [ ] W.3.2 Écriture : les accepter dans `edit_schematic_component` et son
  chemin batch, sans les dégrader en propriétés personnalisées.
- [ ] W.3.3 Tester activation, désactivation, combinaison des trois, et
  conservation des autres propriétés du symbole.
- [ ] W.3.4 Lever B2.8 du benchmark Hi-Fi par le MCP seul.

### Validation

Pour chacun des trois : état initial → mutation MCP → relecture indépendante →
valeur attendue, plus une validation KiCad pertinente.

## W.4 — Régression globale et reprise du benchmark

### Tâches

- [ ] W.4.1 `gate.ps1` complet vert.
- [ ] W.4.2 Tests live Eeschema et Pcbnew verts, routage `DocumentType`
  toujours vert.
- [ ] W.4.3 ERC Hi-Fi sans régression par rapport à Gate C2.

## W.5 — Patch release v1.1.4

- [ ] W.5.1 Version, lockfiles, notes.
- [ ] W.5.2 CI PASS sur le commit candidat, tag, release, artefacts.
