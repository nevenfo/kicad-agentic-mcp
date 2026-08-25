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

# Phase P — Schematic round-trip fidelity — P.1–P.5 DONE, P.6 open

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

## P.6 — Deferred upstream correctness backlog — TODO

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
- [ ] P.6.11 `add_layer` allocates an id that need not match the canonical
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
- [ ] P.6.7 Smaller, independent, each with its own discriminating test. Split
      into one id per item so a commit closes exactly one:
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
  - [ ] P.6.7.9 `validate_for_manufacturing` counts copper layers by substring
        too: `content.matches("signal)") + content.matches("signal \"")`
        (`manufacturing.rs`). Found while closing P.6.7.5, not in upstream's
        audit. KiCad marks copper with four kinds — `signal`, `power`, `mixed`,
        `jumper` — so a board using `power` for a plane is undercounted, and
        the probe also matches the word anywhere else in the file. The `.Cu`
        suffix is the invariant, and `konnect_sexp::layers::copper` already
        decides by it (P.6.6). The dead `let _layers = …` binding above it goes
        at the same time. Measure the miscount on a demo board with a plane
        before writing the test.
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
  - [ ] P.6.7.10 `export_bom` exposes none of `--fields`, `--labels` or
        `--group-by`, which `kicad-cli sch export bom --help` does offer
        (verified on 10.0.3 while closing P.6.7.6). Upstream's #139 carried
        them; this fork's item named only `exclude_dnp` and `format`, so they
        were left out rather than folded in silently. They are what a fab BOM
        needs for MPN/LCSC columns. Decide whether the tool should expose them
        before implementing.
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
  - [ ] P.6.7.11 The measurement P.6.7.8 rests on lives only in a comment.
        The refusal is decided before `kicad-cli` is reached, so the unit tests
        prove the server's own logic and no live probe was owed — but nothing
        in the suite would notice if a future KiCad stopped producing those
        `lib_symbol_issues`, which would leave the refusal unjustified and
        invisible. Same shape as D113. A probe over a copied demo hierarchy,
        asserting the sub-sheet/root asymmetry rather than an absolute count,
        would anchor it. Decide whether it belongs in the gating E2E job.
- [ ] P.6.8 `LATER` items — #271, #179, #185, #148, #186, #138, #162 — each
      carries its precise next action in `docs/upstream-audit.md`; re-read it
      rather than re-deriving. #271 depends on P.6.3.
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
  - [ ] P.6.9.6 `8591707` (residual half only) — `edit_schematic_component`
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
  - [ ] P.6.9.7 `6ed6cac` — five write paths run on substituted required
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
  - [ ] P.6.9.8 `977f0c5` — `run_design_review` (`design_review.rs:522-625`)
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
  - [ ] P.6.9.9 `4536d10` (LATER) — the read-only and batch half of the same
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
  - [ ] P.6.9.10 `791f95b` (LATER) — nothing validates `required` at the
        dispatch: `execute_tool` (`mcp/handler.rs:210`) turns absent arguments
        into `{}`. This is the floor beneath P.6.9.7 and P.6.9.9 and must land
        *after* them — added first it fires before any handler runs, and a
        per-tool test could no longer tell a fixed handler from a broken one.
        Presence only; an explicit `null` counts as absent.
  - [ ] P.6.9.11 `c6a6407` (LATER) — `get_path` (`tools/mod.rs:442-447`)
        returns `anyhow::Result` so handlers can use `?`, and the dispatch
        stringifies it through the `handler_error` fallback
        (`mcp/handler.rs:338`), while `require_str` returns a structured
        `InvalidArgument`. Whether a caller can tell "you forgot an argument"
        from "the tool tried and failed" therefore depends on which helper the
        handler reached for first. Carry the distinction in the error chain and
        downcast at the dispatch, as `konnect_ipc::TransportUnreachable`
        already does — classify by type, never by matching message text. A path
        that is present but unusable stays a handler error.
  - [ ] P.6.9.12 `6693681` (LATER) — `register_in_lib_table`
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
  - [ ] P.6.9.13 — `handle_group_components` (`sch_components.rs:1553-1562`)
        has P.6.9.5's defect A verbatim and was outside its scope: it inserts
        `(property "Group" …)` unconditionally, at a hardcoded `(at 0 0 0)` and
        a hardcoded two-space indent, so grouping the same component twice
        leaves two `Group` properties, the text renders at the sheet origin,
        and the indentation is wrong for every eeschema-authored sheet. The
        helper it needs already exists — route it through `set_symbol_property`
        like the other two. Proof to reproduce first: two `group_components`
        calls naming the same component yield two `Group` properties.

### Validation
Each implemented item carries a test that is red before it and green after,
and — where KiCad is the only honest oracle — a probe in
`schematic_fidelity_live.rs` or its PCB equivalent, inside the gating E2E job.
No item is closed on "the existing suite still passes".
