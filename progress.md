# progress.md — KiCad Agentic MCP

Append-only working log. Error history is never deleted.

---

## GOAL

Turn `mixelpixx/Konnect` into a KiCad **agentic control layer**: large internal
capability surface, small external MCP surface, local LLM agents that absorb
operational work, a deterministic engine for everything that does not need
generative reasoning, task state and evidence outside the LLM context, and
verification that comes from KiCad rather than from the agent's own opinion.

## CURRENT PHASE

**H, in progress.** The seam (`kam-llm`) and the oracle (`bench/model_fit.py`)
are built, and the measurement has now run four times on `qwen3.5-9b`: 240
attempts, **0 at grade 3**. No model is selected and
`LLM_CALLS_PER_SUCCESSFUL_TASK` remains unmeasured — there is no successful task
to divide by.

What the four runs have measured so far is **our own prompt and our own
operation library**, not the model. Each run removed one defect of ours and
exposed the next: E14 (item shapes undocumented), E15 (a failed plan reporting
success, and an oracle that read a failed check as a passing one), E16 (two
placeholder notations one character apart), and now E17 — scalars documented
without a type, and no operation that can create a project. That is the honest
state: the model has not yet been given a library it can write against.

**E closed, G closed.** A (bootstrap), B (cartography), C (baseline benchmark)
and F (compact surface) were already done; D shipped revisions, idempotency,
transactional batches and the error catalog, and still owes stable IDs and
snapshot handles. E now shipped its last piece — ProjectGraph — alongside the
semantic diff, evidence handles, independent verification and the Task State
Manager. G shipped the IR, the compiler, the KiCAD operation library, the `plan`
toolset and plan-owned postconditions. The capability matrix is generated and
committed, and closed **E8** on its way through.

Next up is the decision that has been deferred twice: **Phase H**, the local
model runtime, whose precondition set is now empty — a plan exists for a model
to write, an anchor exists for it to be reminded by, a graph exists for it to
ask instead of dumping, a matrix exists that says which 107 tools it should not
be handed, and **E6 is closed**, so the direct tool path no longer produces
silently wrong geometry for a model to be measured on.

## CURRENT TASK — E17, the library the model was asked to write against

The E16 run (60 attempts, placeholders expanded, `strict: false`) is recorded in
ERROR HISTORY with its full outcome breakdown and local KPIs. Its two remaining
failure modes are both ours and both in `crates/konnect-core/src/plan/ops.rs`:
a scalar field documented without a type is read as a boolean (`"schematic":
true`, 24/60), and no operation in the library can create the project the task
asks for (17/60, in three distinct shapes converging on the same gap).

In flight: type every scalar in the `*_SIGNATURE` constants, and add a `create`
operation expanding to the existing `create_project` tool — whose result already
carries `schematic` and `pcb`, which is what makes the `${create.schematic}` that
seven attempts guessed resolve. Both single-sourced next to their expander, both
covered by the anti-drift test that builds a minimal document out of nothing but
the documented signature. Then the same 60 attempts again, comparable because the
tasks, hints and ladder do not move.

Queued behind it, one variable at a time: `--strict-json`, which now exists on
`model_fit.py` and was `false` for all four runs.

---

## PREVIOUS TASK — Phase H, the seam and the oracle, before any model

Phase H's first two commits deliberately produce **no verdict about any model**.
They build the two things a verdict would otherwise be invented from.

**`crates/kam-llm` — the seam** (clean-room, MIT OR Apache-2.0, no `konnect-*`
dependency, D11's rule for the fifth time). `provider::Provider` is the whole
contract: one `async fn complete`, object-safe on purpose so the router can
hold `Box<dyn Provider>` and swap backends without a caller changing. The
vocabulary around it — `Message`, `ToolDef`, `ToolCall`, `StructuredOutput` —
is shaped like MCP's own tool definitions, so a tool catalogue can be handed
across untranslated.

* `openai_compat` is the one concrete backend, because D31 already ruled that
  LM Studio and `llama-server` both speak it and `vLLM` has no native Windows
  path. It **refuses a non-loopback base URL** in `OpenAiCompatConfig::new`
  and makes the override a separate named constructor, so exposing a local
  inference backend to the network is a decision somebody typed rather than a
  default they inherited.
* `usage::Usage` exists so `LOCAL_INPUT_TOKENS`, `LOCAL_OUTPUT_TOKENS`,
  `TTFT_LOCAL` and `TOKENS_PER_SECOND_LOCAL` are a field read at the call site
  instead of a second instrumentation pass. A backend that reports no counts
  leaves them at `0` rather than estimating one.
* `hardware::probe` never panics and never guesses: `nvidia-smi` first, a
  Windows display-adapter fallback that reports names and **not** VRAM
  (`Win32_VideoController.AdapterRAM` is a 32-bit field that misreports modern
  cards, so it is not read rather than read wrong), and a backend probe that
  checks `PATH` presence and says so — a capability probe, not a liveness
  check, so it opens no socket.

19 tests in the crate. It ranks nothing, chooses nothing and routes nothing;
that is the next step and it depends on a measurement that has not run.

**`bench/model_fit.py` — the oracle.** The grade never comes from reading the
model's answer. Every attempt goes through the real built server on a 0–3
ladder: `0` not schema-valid JSON, `1` valid JSON that `preview_plan` refuses,
`2` compiles and applies but breaks a task invariant or the ERC budget, `3`
applies clean. `check_assertion` and `GatewayClient` are **imported from
`bench/runner.py`**, not reimplemented, so the model is judged by the same path
that is already proved against real `kicad-cli` — a harness with its own
compiler would refuse a plan for a reason it invented.

The prompt is four blocks in fixed order, the first three byte-identical across
every task and every model so a prefix cache can hold them, and the schema and
operation-library blocks are pulled from `kicad_describe(["apply_plan"])`
against the running server rather than hand-typed, because a copied schema
drifts from `kam-plan` silently.

Four tasks: `01_divider`, `02_ldo`, `03_decoupling_bank`, `04_reference_heavy`.

**The oracle is proved; the model is not.** The selftest passes on all four
rungs — a correct divider plan grades 3, an unknown operation grades 1 with the
compiler's own refusal (`unknown_op: 'levitate' is not a known plan operation`),
floating pins grade 2 at 4 ERC errors against a budget of 0, and malformed JSON
grades 0. The stable prefix measures **800 tk** (255 rules + 329 schema + 216
operation library) and is byte-identical across all four tasks and all three
hint levels, which is the property a prefix cache needs.

**The first real run measured nothing, and the reason is recorded as E13:** the
LM Studio runtime cannot load `qwen3.5-9b` at all — `unknown model
architecture: 'qwen35'` — so all 60 attempts failed before reaching the model
and `bench/results/model-fit-qwen3.5-9b.json` carries `null` grades throughout.
That is D31's stated risk arriving rather than a surprise, and it leaves the
shortlist at zero runnable candidates until `gpt-oss-20b` is in place.

---

## PREVIOUS TASK — E7, closed where an agent will actually read it

The matrix had labelled fifteen in-process connectivity tools `ADVISORY` since
the capability pass, and it changed nothing for the one reader that matters: a
model calling `find_single_pin_nets` reads the tool's *description*, not
`docs/capability-matrix.md`. It saw nothing saying the answer is not a verdict —
on a tool that has returned `single_pin_net_count: 0` while `kicad-cli` found
six unconnected pins.

The description now carries it: *"Advisory: connectivity is derived in-process
and has disagreed with kicad-cli ERC. For a verdict, use run_erc."*

**Membership comes from one source.** `capability::is_advisory_tool` queries the
same `MANIFEST` the matrix renders, and `router::registry::tools_for` appends
the suffix as it builds each toolset's `ToolDef`s. Nobody retypes the list into
fifteen descriptions, so the matrix and the descriptions cannot disagree about
*which* tools are advisory. The wording is deliberately allowed to differ: the
matrix's `reason()` is archival prose read once, the suffix is paid on every
`tools/list`. The test asserts the equivalence rather than a hard-coded list —
for every tool in every toolset, `description.ends_with(ADVISORY_SUFFIX)` must
equal `is_advisory_tool(name)` — and pins the count at fifteen.

**Measured:** startup catalogue **2 034 — unchanged**, because none of the
fifteen is a starter tool. Full catalogue **25 238 → 25 642 tk (+404)**, all of
it on those fifteen at +27 each, and **exactly zero** on any other tool. Golden
suite 18/18 at 2 190 tk/task against 2 178: per-task deltas are mixed-sign
(`sch_hierarchy` +5, `manufacturing_exports` −12, `recovery` +6), which is the
established noise band and not the suffix — a task that actually described an
advisory tool would have moved +27 in one direction. **759 tests**, gate green.

The honest limit: this closes E7's *disclosure*, not E7. The internal
connectivity analysis still disagrees with `kicad-cli`. What is now true is that
no agent can call it without being told so, and the evidence path never asks it.

---

## PREVIOUS TASK — E6, closed as a class rather than as a symptom

`add_power_symbol` wrote its coordinate verbatim while
`add_schematic_component` snapped to the 1.27 mm grid, so a power symbol placed
at the same nominal coordinate as a resistor landed 0.33 mm off the pin and ERC
reported `Pin not connected` for both. No tool errored. The schematic was simply
wrong — six ERC errors on the first divider probe.

The fix is not the one tool. Every electrically meaningful `(at x y)` a
schematic tool writes now goes through one `snap_reporting()` helper over
`konnect_sexp::geometry::snap_point`, and the grid is a single
`SCHEMATIC_GRID_MM` constant — `plan::ops` aliases it rather than restating it,
because two grid literals drifting apart is E6 with extra steps. **Nine more
tools were silently affected** and are fixed with it: `add_no_connect`,
`add_net_label`, `add_junction`, `batch_add_junction`, `connect_passthrough`,
`connect_to_net`, `add_schematic_text`, `add_sheet_pin`, and `apply_template` —
whose 15 mm column spacing is not a multiple of 1.27, so it drifted off-grid
even from an on-grid origin.

**The tool does not lie about it.** An on-grid input is snapped silently and
gains no field; an input that moved gets `requested: {x, y}` and
`snapped_to_grid: true` in the reply, with `x`/`y` reporting what was actually
written. A sheet body and `move_sheet` are deliberately left unsnapped: a sheet
outline is not a connection point.

**Verified by running it, not by reasoning about it** (E12's rule): a new
`#[ignore]`d e2e places R1/R2 with VCC/GND power symbols at exactly the
off-grid coordinates that produced E6, runs real `kicad-cli sch erc`, and
asserts **0 errors**. Both e2e tests in the file pass.

**758 tests**, `gate.ps1` green, golden suite **18/18 at 2 178 tk/task**, 4 MCP
calls, P50 64 ms — placement changed, the benchmark did not.

---

## PREVIOUS TASK — a world model the model can ask, instead of a document it must read

ProjectGraph is the last of Phase E. The point is not that a graph exists; it is
that an agent can ask a *question* — which symbols carry this value, what sits
next to this pad — and pay for the answer rather than for the document.

* `kam-graph` — clean-room, MIT OR Apache-2.0, knows nothing about KiCAD.
  `graph` is the indexed store (items keyed by stable key, typed, attributed,
  spatially placed); `query` is the filter/neighbor/count language. 46 tests.
* `konnect-core::graph` — the KiCAD half: extractors keyed on KiCAD's own UUIDs,
  and `GraphStore`, which caches a built graph against the content revision it
  describes, so a second query on an unmoved document rebuilds nothing (D18's
  rule, applied to the graph).
* `konnect-core::tools::graph` — `graph_query`, `graph_neighbors`, `graph_stats`.
  A toolset, not gateway verbs, so **0 startup tokens** — asserted by
  `the_graph_toolset_costs_nothing_until_it_is_used`, which checks both halves:
  none of the three appear in the startup catalogue, and `graph_stats` is
  callable through `kicad_invoke` without a catalogue refresh.

**Measured, and the first measurement was a defect.** `graph_query kind=symbol`
returned **525 tk** for six items where the plain `list_schematic_components`
dump costs **310** — a query tool more expensive than the dump it exists to
replace. `fields` (`compact` default, `full`) now drops geometry, `angle` and
`unit`, and omits `kind` per item when the query already pinned it: unfiltered
**525 → 340 tk**, and a *filtered* query (`attrs value=10k`, two items) is
**109 tk** against the same 310. An unrecognised `fields` is refused with
`invalid_argument` before anything runs (D17's rule again).

**340 is still 10 % above the dump, and that is recorded rather than fixed** —
see D30. The graph's win is filtering and adjacency, not serialisation.

**752 tests**, `gate.ps1` green, golden suite **18/18 at 2 174 tk/task**
(2 178 before — inside the ±12 noise, no saving claimed), 4 MCP calls, startup
**2 034 unchanged**.

---

## PREVIOUS TASK — a plan that can be held to its own promise

`kam-plan`'s IR has carried a `validators` list since Phase G and nothing ran it;
the verdict came from the enclosing `kicad_invoke(verify: "auto")`. It runs now,
which is what lets a plan declare "this is only done if ERC is clean" and be held
to it rather than merely audited afterwards.

* Four names: `erc` / `drc` mean **this plan introduced no new finding** (delta
  against a baseline, by stable finding id); `erc_clean` / `drc_clean` mean
  **zero errors, absolutely**. An unrecognised name is refused in `build()`,
  before the first mutation — D17's rule, applied one level down.
* The documents checked are the ones the plan's own steps touch, collected from
  the compiled step arguments. Baseline comes from `ctx.validation`; when the
  cache has none and the plan asked for a no-regression check, one is computed
  **before** the first step. A document that did not exist gets an empty
  baseline, not an unknown one.
* Failure returns `error_kind: "postcondition_failed"` with the check, the
  document, the counts and the introduced ids — and `is_error` is what makes the
  enclosing atomic `kicad_invoke` roll the plan back. A validator that could not
  run is a failure, never zero findings (E4).
* `preview_plan` reports `validators_plan` — which validator would run against
  which document — and still spawns nothing.
* `kam-plan` stays ignorant of KiCAD (D11): `Postcondition` lives in
  `konnect-core::evidence::validators`.

**Measured** on the divider, release binary, real `kicad-cli`: no `validators`
**48 ms**, `erc_clean` **1 114 ms**, `erc` **2 182 ms** — the second run is the
baseline the no-regression promise requires. The reply is byte-identical in all
three cases, so **a passing postcondition costs no tokens at all**; the price is
latency, and it is opt-in for exactly that reason. A unit test gives the context
an impossible `kicad-cli` path to prove the empty-`validators` path spawns
nothing.

The e2e test is the part worth trusting: `erc_clean` fails on a plan that leaves
floating pins and a duplicate reference, and passes on the golden suite's own
divider, against real `kicad-cli`. It was run, not merely written — see E12.

**706 tests** (`konnect-core` 321 → 327), 18/18 golden, 4 MCP calls, startup
2 034 unchanged, gate and benchmark clean.

(Next at the time: ProjectGraph — done, above.)

---

## PREVIOUS TASK — the capability matrix, and the defect it forced

`docs/capability-matrix.md` is rendered from `konnect-core::capability`, and the
point of it is the rule rather than the table: `SUPPORTED` is not a field
anybody sets. `capability::coverage` reads the repository's own tests and golden
benchmark tasks and publishes the strongest proof it can find, so a tool nothing
exercises reads `NOT_TESTED` however finished its code looks, and an `#[ignore]`d
test counts as `gated` — shown, and not a claim. What KiCAD has no API for leaves
the denominator, so the percentage separates "we didn't" from "KiCAD can't".
Three tests keep it from becoming fiction: the manifest must name every
registered tool, may name no tool that does not exist, and the committed markdown
must match what the code renders (`KAM_UPDATE_MATRIX=1` regenerates it).

The result is uncomfortable and that is the intended behaviour: **27.3 %** of
KiCAD-domain entries at first render, 107 of 193 tools with no proof that runs,
and the whole `pcb_components` / `pcb_routing` path resting on `#[ignore]`d tests
because `ipc!` has no file fallback and needs a GUI session.

**E8 closed by the same pass.** `export_bom` reads a `.kicad_sch` and nothing
else and was registered in `pcb_export`; it now lives in `sch_export`. The matrix
is what forced it — the tool published as `PARTIAL` with its own misplacement as
the stated limitation on every render. Measured in `toolsets` mode, the only mode
that can see it: `manufacturing_exports` used to pay **+1 757 catalogue tokens**
over its schematic-only peers and now pays exactly what they pay (8 880). In
`gateway` mode the fix is worth nothing, as expected — the catalogue is never
refreshed there — and the column's 2 171 → 2 178 is inside the ±12 spread of
repeated runs on one build. No saving is claimed there. `bom` coverage 0 % →
100 %, KiCAD-domain coverage 27.3 % → **28.0 %**.

**700 tests, 18/18, 4 MCP calls, startup 2 034 — unchanged**; `fmt`, `clippy` and
the benchmark gate clean.

---

## PREVIOUS TASK — Phase G, the plan

A change can now be described once instead of enumerated, and a description that
cannot finish is refused before the first mutation:

* `kam-plan` — clean-room, MIT OR Apache-2.0, knows nothing about KiCAD. `ir` is
  the plan document; `refs` resolves `${op.field}` so a later operation reads an
  earlier one's output instead of round-tripping through the model; `compile`
  expands each operation through an `OpLibrary` and **refuses a reference that
  names an operation which does not exist, runs later, or is itself**; `execute`
  is a state machine the async host drives, so the crate needs no runtime.
* `konnect-core::plan` — the KiCAD half: `call`, `place`, `power`, `label`,
  `wire`, `connect`, `decouple`. Every coordinate an operation emits is snapped
  to the 1.27 mm grid before it reaches a tool, which makes **E6 unreachable
  inside a plan**: the same off-grid input that produced six ERC errors produces
  none, asserted end to end against real `kicad-cli`.
* `konnect-core::tools::plan` — `preview_plan` (compile, list the calls, change
  nothing) and `apply_plan` (compile and run). A toolset, not gateway verbs, so
  **0 startup tokens**; run inside `kicad_invoke` so a plan inherits the batch's
  snapshot, rollback, diff, `verify` and task filing, and every inner step is
  written to the call log under its own name.

**Measured** (`bench/plan_cost.py`, same design built both ways, void unless the
semantic diff and the ERC verdict match): divider **2 180 → 1 124** external
tokens (−48.4 %), decoupling bank **2 265 → 882** (−61.1 %, and −69.2 % on the
request alone). Golden suite unchanged at **2 171 tk/task, 4 MCP calls, 18/18**;
startup **2 034 — unchanged**; **682 tests**.

`LLM_CALLS_PER_SUCCESSFUL_TASK` is still unmeasured and is not claimed.

---

## PREVIOUS TASK — Phase E, task state

A batch now describes its change, hands out a handle to the detail, can prove
the design still holds, and files itself under the task it belongs to:

* `kam-evidence` — clean-room, MIT OR Apache-2.0, knows nothing about KiCAD.
  `diff` matches items by stable key so re-serialisation noise is removed
  structurally; `store` is the bounded handle store; `finding` gives every
  validator finding a stable id hashed from validator + rule + location.
* `konnect-core::evidence` — the KiCAD half: extractors for `.kicad_sch` and
  `.kicad_pcb` keyed on KiCAD's own UUIDs, plus `validators`, which runs
  `kicad-cli` ERC/DRC and caches each verdict against the revision it
  describes.
* `kicad_invoke` arguments: `diff` (`none` / `summary` / `changes`) and
  `verify` (`none` / `auto`). The reply carries `kicad://diff/N` and, when
  verified, `kicad://evidence/N`; both resolve over MCP `resources/read`,
  which until now returned an empty array.
* `kam-state::task` — the objective, constraints, verified facts, failed
  attempts and evidence handles, held outside any model's context. The ACTIVE
  TASK anchor is rendered from the record on every read, never stored, so a
  reminder cannot drift from what it reminds about. Hard constraints are
  *refused* at the bound rather than evicted.
* `konnect-core::tools::task` — four tools in their own toolset, so they cost
  **zero** startup tokens and are reached through `kicad_invoke`.
  `kicad_invoke(task_id=…)` files the batch's revisions, evidence and failures
  under the task by itself and returns the refreshed anchor.

**2 175 external tokens/task, 4 MCP calls, 18/18, 606 tests.** Startup surface
1 952 → 2 034, once per session, of which 36 is `task_id` and 0 is the task
tools. `verify` costs ~1.1 s per batch on a real project, measured, which is
why it is opt-in.

---

## TODO

```
[x] Étape 0 — progress.md and plan.md
[x] Gate 0 — compare Konnect / kicad-mcp-pro / legacy KiCAD-MCP-Server
[x] Verify licences, choose the base officially            -> Konnect, AGPL-3.0-only
[x] Clone the base into a clean workspace                  -> konnect-agentic, branch agentic/main
[x] Read architecture / CI / tests / manifests
[x] Run the base's build + tests                           -> 469 green
[x] Build the initial benchmark                            -> bench/, 6 golden tasks, 3 load modes
[x] Measure baseline tokens / calls / latency / success    -> docs/benchmark.md
[x] Reduce the external MCP surface (first two levers)     -> -70.1 % external tokens
[x] Optimise progressive disclosure / tool retrieval       -> done + measured; retrieval is the weak link
[x] Compress heavy tool schemas + shrink the starter kit    -> 3 698 -> 3 197 tk/task, startup 1 958 -> 1 454
[x] Compact gateway (kicad_describe + kicad_invoke)         -> 1 995 tk/task, 4 calls, CATALOG_TOKENS = 0
[x] Map the scope gaps (capability matrix)                  -> generated from evidence, 27.3 % -> 28.0 %
[x] Revisions + optimistic concurrency (base_revisions)     -> content-addressed, kam-state
[x] Transactions / rollback / idempotency at the MCP layer  -> kicad_invoke, 2 033 tk/task, 18/18
[x] Error catalog (TransientClass, stable io codes)         -> E9 closed, E11 stays fixed
[ ] Stable IDs (UUID-addressed items, not path+coordinates)
[ ] Snapshots as first-class handles (kicad://snapshot/N)
[x] Semantic diff                                           -> kam-evidence + konnect-core::evidence, 2 158 tk/task
[x] Handles / resources / evidence packs                    -> kicad://diff/N + kicad://evidence/N, MCP resources
[x] Independent verification (ERC/DRC in the reply)         -> verify: auto, stable finding ids, cached baselines
[x] Task State Manager                                      -> kam-state::task + task toolset, 0 startup tokens
[x] Attention anchor (ACTIVE TASK), rendered from the record -> TaskState::anchor, ~40 tk
[x] ProjectGraph / World Model                              -> kam-graph + graph toolset, 0 startup tokens, filtered query 109 vs 310 tk
[ ] Context Manager (budgets, compaction, retrieval)
[x] Plan IR + compiler + reference checking                 -> kam-plan, 46 tests
[x] Deterministic executor + operation library + batching   -> plan toolset, -48.4 % / -61.1 %
[x] Plan-owned postconditions                               -> erc/drc + erc_clean/drc_clean, rollback on failure
[ ] Direct mode / Agent mode split
[x] Local model provider abstraction + hardware probe               -> kam-llm, loopback-only, 19 tests
[x] Model-fit benchmark harness (grade a plan by compiling it)      -> bench/model_fit.py, 4 tasks, 0-3 ladder
[x] Run the model-fit measurement                                   -> 6 runs, 360 attempts on qwen3.5-9b; grade 3 = 4/60 one-shot, 46/60 compile
[x] Repair loop measured (--repair 1)                               -> 0 of 58 failures converted; LLM_CALLS/success 15 -> 59 (D35)
[ ] Measure a second model (gpt-oss-20b) before any router threshold
[ ] Discard a repair that lowers the grade (11 of 58 were kept anyway)
[ ] Local model router NO_LLM/SMALL/MEDIUM/LARGE/ESCALATE
[ ] Error catalog completeness, retries, recovery policy
[ ] Event journal / deltas
[ ] Custom KiCad gate  (default: NO — see D3)
[ ] Multi-harness tests (Claude Code / Codex / AGY)
[ ] Hardening + failure injection
[ ] Final benchmark + comparison table
```

---

## DECISIONS

### D1 — Base = Konnect (2026-08-10)

Recorded in `plan.md` § Gate 0 with measurements. Only Rust candidate, single
process, already had progressive disclosure, transactions, atomic writes,
observability and 469 green tests. `kicad-mcp-pro` is broader (380 tools,
2 852 tests, generated parity matrix) but its schematic writer depends on
`kicad-sch-api`, which drops `global_label` nodes on save; its profiles are
boot-time only; and it needs three build systems. The legacy server is the
TS→Python→SWIG chain Konnect exists to replace.

### D2 — Licence posture (2026-08-10)

AGPL-3.0-only, personal use, no distribution today → unblocked. Generic
subsystems go into new clean-room `kam-*` crates so a future re-licence does not
require rewriting them. MIT code from `kicad-mcp-pro` may be absorbed with its
notice; nothing flows the other way.

### D3 — No KiCad fork for schematic IPC (2026-08-10)

Verified against KiCad 10.0 sources: `schematic_commands.proto` declares no
commands, `eeschema/api/api_handler_sch.cpp` registers only `GetOpenDocuments`,
`getItemFromDocument()` returns `std::nullopt` (TODO). Schematic IPC is being
built upstream for **KiCad 11** (`kicad-python` 0.8.0, `kicad-cli api-server`).
Forking KiCad 10 would duplicate work against a moving upstream. Konnect's
S-expression engine stays the schematic path. Re-evaluate at KiCad 11.

### D4 — No async events exist in KiCad IPC (2026-08-10)

KiCad exposes only `KINNG_REQUEST_SERVER` (REQ/REP); there is no pub/sub. The
event journal and `changes_since(rev)` must be ours: internal revision counter,
targeted diffing, file watching. The MCP layer must not advertise push
notifications it cannot deliver.

### D5 — Keep both loading paths (2026-08-10)

`find_capabilities` + `load_tools` is the cheap path; `list_toolboxes` +
`load_toolset` is kept because sweeping a whole domain is sometimes right, and
because every existing skill and client uses it. Removing it would be a
breaking change for a benefit already obtained by adding the alternative.

### D6 — Plural stemming rejected on evidence (2026-08-10)

Implemented in `capability_search.rs`, measured, removed. It moved retrieval
recall at 8 results/query from 100 % to 98.2 % and helped at no limit. The
rejection is documented at the code site so it is not re-attempted.

### D7 — No `$defs` / `$ref` in `inputSchema` (2026-08-10)

`create_symbol` inlines the same pin-item object three times; a local `$ref`
would have removed ~400 tokens from it. Rejected after checking the whole chain,
not just the spec:

* **MCP 2026-07-28**: explicitly allows it. `inputSchema` is full JSON Schema
  2020-12, `$defs` is named in the composition-keyword section, and there is a
  dedicated `$ref` resolution section (local `$ref` fine; network `$ref` MUST
  NOT be auto-dereferenced).
* **Anthropic Messages API**: supported under `strict: true` with documented
  limits (no external `$ref`, no recursion, no `allOf` + `$ref`). **Undocumented
  for non-strict tools** — the mode Claude Code actually uses.
* **Client chain: not reliable.** `openai/codex` #3152 and #13746 (schema
  degraded to `{"request": string}` / arrays of strings), `gemini-cli` #13326
  (Gemini API rejects `$defs`), `mcpb` #174 (Claude Desktop on Windows fails to
  compile schemas with `$defs`), plus the same symptom in Kiro, mastra, n8n and
  autogen.

A tool schema the model receives mangled is worse than a fat one. Compression
therefore stays inside inlined schemas, which every client already handles. Note
that several of those issues recommend the opposite direction — servers should
*inline* their `$ref`s before exposing them — which is where this already is.

### D8 — `config` is no longer a starter toolset (2026-08-10)

7 tools, 625 tokens, re-sent on every `tools/list` refresh, and **zero** calls
across the whole golden suite. Split: `load_user_config` and
`get_effective_config` (118 tk) stay, admitted individually via a new
`STARTER_TOOLS` list; the five write / design-rule tools leave. Before shipping,
`find_capabilities` was checked on their own intents and ranks each removed tool
**first** ("remember that I always use JLCPCB" → `save_user_config`, "add a
design rule for decoupling" → `add_design_rule`). The behavioural cost is one
discovery call for a rarely-used feature; the saving is 507 tokens on every
refresh in every load mode.

### D9 — The gateway is two verbs, not seven (2026-08-10)

`plan.md` sketched ~7 external verbs (`kicad_status`, `kicad_capabilities`,
`kicad_delegate`, `kicad_query`, `kicad_verify`, `kicad_evidence`,
`kicad_history`). Shipped: **two**, `kicad_describe` and `kicad_invoke`. The
other five all describe *what* is being called, not *how*, and the registry
already answers that — `kicad_invoke("run_erc")` is a verify, and
`kicad_invoke("list_schematic_nets")` is a query. Adding verbs whose only
content is a category would have re-created the schema cost the gateway exists
to remove. `kicad_status`, `kicad_evidence` and `kicad_history` earn their own
verbs when there is state behind them (Phases D/E); until then
`server_stats` / `get_recent_calls` cover it.

Both loading paths stay (D5 still holds): `load_toolset` and `load_tools` are
what every shipped skill and existing client uses, and removing them would break
them for a saving already obtained by adding a third path.

### D10 — `atomic` defaults to `stop_on_error`, not to `true` (2026-08-10)

The first Phase D build rolled back any batch containing a failure. The
benchmark scored the `recovery` task **0/3**: it deliberately fails five calls
mid-batch with `stop_on_error: false`, and the design built by the remaining
calls — which the assertions check — was being thrown away.

A caller who passes `stop_on_error: false` has declared the calls independent
and the survivors wanted; undoing them is the opposite of the request. So
`atomic` follows `stop_on_error` unless set explicitly. Found by measurement,
not by review, which is the argument for running the suite before believing a
safety feature is safe.

### D11 — `kam-state` is MIT OR Apache-2.0 inside an AGPL fork (2026-08-10)

Plan.md's licence mitigation said generic subsystems must stay re-licensable.
`kam-state` is the first one, so it carries its own permissive licence and
depends on no `konnect-*` type — the rule is now enforced by the crate's
manifest rather than by intention. It knows nothing about KiCAD beyond a list of
file suffixes.

### D12 — Rollback is file-level, not KiCad's undo stack (2026-08-10)

The snapshot restores bytes on disk. That is a complete undo for the
S-expression tools, and **not** an undo for anything applied over IPC to a
running KiCAD, nor for a GUI holding the same file open. `base_revisions` is the
detection half of that gap: it refuses a batch whose document moved. When the
PCB/IPC path gains mutations, it needs `BeginCommit`/`EndCommit` (plan.md, KiCad
10 ground truth) rather than this.

### D13 — the diff is on by default, and that costs 6.1 % (2026-08-10)

`diff` could have defaulted to `none` and kept the per-task number at 2 033.
It does not, because "every change carries its proof" is a project rule and a
reply of `ok: 3` is not reviewable. The measured price is **+40 startup tokens**
(the schema property) and **~85 per task** (one summary line per mutating
batch). The ≤ 2 000 target is now missed by 158 and is recorded as missed.

The counter-argument that decided it: the alternative to 85 tokens of summary
is a harness re-reading the documents to find out what happened, which costs an
order of magnitude more. `diff: "none"` exists for a caller who disagrees, and
`diff: "changes"` buys the per-item detail.

### D14 — documents are items, not a file count (2026-08-10)

The first build reported `create_project` as `no design change`: three files
appeared and their *contents* — an empty schematic, an empty board — differ in
no item. The batch that changed the most was described as the one that changed
nothing.

Fixed by making the document itself an item, so a creation reads `document +3`
through the same diff engine rather than through a special case. A file that is
modified but has no extractor (`.kicad_pro`) is still counted as
`undescribed_files`, because guessing at its contents would be worse than
admitting the gap.

Found by `bench/probes/semantic_diff.yaml` on a real project, not by review —
the unit tests all passed, because none of them created a project.

### D20 — the task tools are a toolset, not gateway verbs (2026-08-11)

D9 said a verb earns its place when there is state behind it, and the Task
State Manager is state. Four always-visible meta-tools would still have cost
every client a few hundred startup tokens per session, including every client
that never opens a task — on a startup number already 1 000 over target.

As a registry toolset they cost **zero** at startup: `find_capabilities` finds
them by intent and `kicad_invoke` calls them without a catalogue refresh, which
is precisely the case the gateway was built for. Measured: startup went
1 998 → 2 034, and **all 36 tokens of that are the `task_id` property on
`kicad_invoke`**, not the tools. A stdio test asserts both halves — that none of
the four appear in the startup catalogue, and that they are callable anyway —
so the property cannot decay into a convenience.

### D21 — the anchor is rendered, and batches file themselves (2026-08-11)

Two related choices about who is trusted to remember what.

`TaskState::anchor()` builds the ACTIVE TASK block from the record on every
read, and the block is never stored. A cached anchor could disagree with the
task it describes, and a model paraphrasing its own objective back into its own
prompt is the exact drift the Task State Manager exists to prevent.

`kicad_invoke(task_id=…)` attaches the batch's revisions, evidence handles,
`kicad-cli` verdicts and failures to the task without being asked. These are
facts the batch already produced; routing them through "the agent calls
`update_task` afterwards" would make the audit trail depend on the one thing a
model does least reliably. `update_task` stays for the things only the caller
knows — subgoals, assumptions, what to stop trying.

One consequence recorded rather than hidden: an unknown `task_id` does **not**
fail the batch. The mutations already happened, and reporting them as a failure
would be a worse lie than reporting that the filing did not happen. The reply
carries `task: {id, error: "unknown_task"}`.

### D22 — a plan is refused at compile time, not discovered at step 4 (2026-08-11)

`compile` could have resolved references lazily and failed at the step that
needed one. It does not. Every `${op.field}` is checked against the operations
that will have run by then, *before* the first mutation, so a plan that cannot
finish never starts — the same order of business as `check_base_revisions`
running before the idempotency key is claimed.

Unknown and forward references are separate errors on purpose: one is fixed by
renaming, the other by reordering, and a single "bad reference" would make the
caller guess which. What the compiler deliberately does **not** check is whether
the referenced *field* will exist in the run-time output — that would mean
modelling every tool's result shape here. It stays a step failure with the
reference named, which is honest about which half was proved.

### D23 — plan operations refuse a coordinate they cannot snap (2026-08-11)

The one guarantee the operation library makes is that every coordinate it emits
is on the 1.27 mm grid. A `${prev.x}` in a coordinate field would put a hole in
it: the value only exists at run time, and the snap happens at compile time.

Passing such a value through unsnapped was the obvious choice and is the wrong
one — it would make the guarantee true *usually*, which is the same as not
having it, and E6 is precisely a bug that only bites usually. So a coordinate
must be a number, and the error says to use `call` for the other case. That
keeps references for identifiers, where they belong.

### D24 — `decouple` places and wires; it does not review (2026-08-11)

The macro computes positions, connections and ground symbols for a bank of
capacitors. It has no opinion on whether 100 nF is right for that rail, whether
four is enough, or whether the placement is good, and it says so in its own
documentation rather than implying competence by silence.

This is the same rule as E7 one level up: an internal routine must not be the
thing that says a design is sound. `kicad_invoke(verify: "auto")` gets that
verdict from `kicad-cli` or the plan does not carry one.

`ic` is required only when a capacitor names a `pin`, because a bank that all
sits on a rail wires to no IC — a schema demanding a name it will never use is
a schema asking for a fact to satisfy itself.

### D25 — the plan runs inside `kicad_invoke` rather than beside it (2026-08-11)

`apply_plan` could have compiled and handed the calls back for the caller to
pass to `kicad_invoke`. That costs a round trip *and* makes the caller pay
tokens for the full expansion, which is the cost a plan exists to remove — on
the decoupling bank it would have re-introduced the 732-token payload the plan
reduces to 225.

Running the steps inside the batch means the plan inherits the snapshot, the
rollback, `base_revisions`, the semantic diff, the `verify` verdict and the task
filing without re-implementing any of them: a plan is one MCP call and still a
transaction. The cost is that `apply_plan` dispatches tools itself, so it is the
one tool built without the `tool!` macro (it needs the `Arc<ToolContext>` the
macro dereferences away), and it writes each inner step to the observability log
by hand — otherwise a plan would be a mutation without an audit record, which is
a V1 success criterion at 0.

### D28 — a plan's promise is checked after the steps, and it fails the batch (2026-08-11)

A postcondition could have been advisory: run the validator, report the verdict,
let the caller decide. That is what `kicad_invoke(verify: "auto")` already does,
and duplicating it inside the plan would have added a second way to say the same
thing.

`apply_plan` instead returns `is_error` when a declared postcondition fails, so
an atomic `kicad_invoke` rolls the whole plan back. The reason is what a plan is
*for*: it exists so a change can be described once instead of enumerated, and a
description that ends "…and ERC must be clean" is not honoured by a reply that
says ERC is dirty and keeps the change. The steps summary survives in the error
body — the caller still learns what ran — but the verdict decides the outcome.

The line this does not cross: the plan does not choose *what* clean means. The
verdict is `kicad-cli`'s, through the same `evidence::validators` path as
`verify`, cache and stable finding ids included (E7's rule at one more level).

### D29 — `erc` and `erc_clean` are two different promises, and both are spelled out (2026-08-11)

One name would have been simpler. It would also have been ambiguous in the way
that matters: on a design that already has three ERC errors somewhere else, does
"ERC" mean the plan must fix them, or must merely not add a fourth?

`erc_clean` is absolute — zero errors — and needs no baseline. `erc` is "no new
findings by stable id", which cannot be evaluated without a verdict on the state
the plan started from; when the cache does not have one, it is computed **before**
the first mutation rather than inferred. Measured, that is the difference between
1 114 ms and 2 182 ms on the same divider, and it is the honest price of a promise
about a delta.

Both refuse an unrecognised name in `build()`, before anything runs. A plan that
believes it is being checked and is not is the failure mode D17 already refused
for `verify`, and a plan is a worse place for it: the caller is further away from
the mutation.

### D31 — the backend is OpenAI-compatible HTTP, and the model stays unchosen (2026-08-11)

Phase H's first two questions are answered from primary sources; the third is
deliberately not.

**Backend.** `vLLM` has no native Windows support — Linux or WSL2 only, per its
own installation doc — so it is out for this machine without a second OS.
`llama.cpp` is the only native path, and Blackwell `sm_120` needs
`-DCMAKE_CUDA_ARCHITECTURES=120` from source; the upstream issue asking for it
(#22696) closed *not planned*. LM Studio is already installed, wraps
`llama.cpp`, and exposes both tools and `response_format: json_schema` on an
OpenAI-compatible endpoint — enough for everything Phase H needs to measure.

So the provider abstraction targets **OpenAI-compatible HTTP** and nothing else,
which costs nothing in generality: LM Studio and `llama-server` both speak it.
LM Studio is the default because it is installed; `llama-server` is the escape
hatch the moment a measurement needs a flag LM Studio does not expose — KV cache
type, MoE expert offload (`--n-cpu-moe`). That switch must be a config change,
never a code change, which is the whole reason the trait exists.

**Shortlist**, both Apache-2.0, both with tool calling documented on their own
model card rather than inferred:

| candidate | shape | why it is on the list |
|---|---|---|
| `Qwen3.5-9B` | 9B, `Q6_K`/`Q8_0` | BFCL-V4 **66.1**, TAU2-Bench **79.1**; comfortably inside the budget with room for KV cache |
| `openai/gpt-oss-20b` | 20B MoE, 3.6B active, native `MXFP4` | more capable, and ~12–13 GB is most of the 14 GB budget |

Ruled out on the 16 GB budget, with the number: `Qwen3.5-27B` at `Q4_K_M` is
~16.5 GB — over budget before any KV cache, despite the best BFCL score of the
family (68.5). `Qwen3.6-35B-A3B` fits only with CPU expert offload, which is a
latency question, not a capability one, and belongs in the benchmark rather than
in a preemptive exclusion. `Qwen3-Coder-Next` is 80B-A3B, out.

**The model is not chosen and will not be chosen by reputation.** Every VRAM
figure above is an estimate that no primary source confirmed, BFCL V4 has not
been updated since 2026-04-12, and `llama.cpp`'s support for Qwen3.5's hybrid
Gated DeltaNet architecture is suggested by the existence of community GGUFs and
confirmed by nobody. All three get measured on this machine.

**There is no open-weight model specialised in EDA, PCB or electronics.** The
projects that look like one (`kicad-llm-plugin`, `circuit-synth`, `tscircuit`,
`PCBSchemaGen`) are systems built on general models. Recorded so the question is
not re-asked: the local model will be a general one, and the electronics
competence has to come from the deterministic engine and the validators, which
is what this architecture already assumes.

### D35 — one repair round costs double and converts nothing, and the wall is now ERC (2026-08-12)

D34 built the repair round to test the architecture's own claim: the compiler and
the validators say exactly what is wrong, so a second call should fix it without
anyone being clever. Measured, same 60 attempts, `--repair 1`, only that changed:

| | round 0 | after one repair |
|---|---|---|
| `truncated` | 9 | 5 |
| `compile_failed` | 5 | 6 |
| `not_applied` | 25 | 23 |
| `applied_invalid` | 19 | **24** |
| `success` | 2 | **2** |

**Of 58 failed first rounds, the repair converted 0 to success.** It moved 22 up
the ladder, left 25 where they were and pushed 11 **down** — the harness keeps
the last plan, so a repair that is worse than what it replaced is what the run
records. `LLM_CALLS_PER_SUCCESSFUL_TASK` therefore went from **15 one-shot to
59** (118 calls, 2 successes). The 4/60 → 2/60 difference in successes is
sampling noise at temperature 0.2 and is not claimed as a regression; **0
conversions out of 58** is not noise, and it is the finding.

The candidates from E21 are used, and that is worth separating from the verdict:
of the 24 attempts whose first round failed at apply — nearly all of them
inventing a library — **11 repairs wrote a real `Device:` symbol**. So an
actionable error does change the model's next plan about half the time. It just
lands one rung higher instead of at the top, because what is left is not a format
error.

**What is left is ERC.** 24 of 60 final attempts apply cleanly and fail the
budget: `erc errors=1 limit=0` (8), `2 limit=0` (5), `4 limit=0` (3). A message
saying "one pin is not connected" does not teach a 9B where the wire goes, and
this is the boundary this project always said it would find — the electronics
competence has to come from the deterministic engine and the validators, not from
the local model (D31).

Two consequences recorded rather than acted on yet:

* **A repair that makes the design worse should be discarded, not kept.** The
  machinery to decide that already exists — semantic diff, ERC verdict, rollback,
  revisions — and the harness simply does not use it. That is a product change,
  not a benchmark one.
* **72 % of every generated token is deliberation**: 177 695 reasoning tokens of
  247 994 output. Any budget claim about the local model that ignores the split
  is wrong by a factor of three.

### D34 — the repair round gets the error and nothing else (2026-08-12)

`LLM_CALLS_PER_SUCCESSFUL_TASK` is 15 one-shot (4/60), and the architecture's
answer to that has always been the deterministic engine: the compiler and the
validators already say exactly what is wrong, so a second call should be able to
fix it without anyone being clever. `bench/model_fit.py --repair N` measures
whether that is true instead of assuming it.

A repair round is given its own previous plan and the server's **verbatim**
refusal. No advice, no restated rules, no extra hint, no worked example —
anything else would measure the hint rather than the error message, and the
error message is the thing under test. The block is appended *after* the dynamic
task so the stable prefix stays byte-identical and a prefix cache still holds
across rounds.

Between rounds the work directory is emptied and the **paths stay the same**. A
fresh `mkdtemp` per round would move `$SCH` under the model's feet and recreate
E16 one round apart; wiping the contents means a repair starts from the state
round 0 started from, and the paths in its own previous plan remain valid.

Cost accounting: tokens are summed across rounds, because what a task cost is
what it took and not what its last try took. TTFT and tok/s stay the final
round's — a median across rounds would describe no single generation. `llm_calls`
is now an aggregated metric, so one-shot and repaired runs are compared on the
number that matters rather than on success rate alone.

### D33 — constrained decoding is off, measured, and it is not the setting it looked like (2026-08-12)

E15 queued `response_format.json_schema.strict` as its own run, on the theory
that a best-effort grammar explained the ~17 % invalid-JSON residue. It was run
against the E18 build, one variable changed and nothing else:

| | `strict: false` | `strict: true` |
|---|---|---|
| `invalid_json` | 7 | **16** |
| `compile_failed` | 30 | 19 |
| `not_applied` | 14 | 16 |
| `applied_invalid` | 5 | 8 |
| `success` | **4** | **1** |

Constrained decoding made it **worse on both ends**, so it stays off.

**The mechanism is not "the grammar produces bad JSON", and that matters more
than the setting.** Twelve of the sixteen failures returned an **empty**
`content` while billing ~6 000 completion tokens. Probed directly against the
backend: `qwen3.5-9b` is a reasoning model whose deliberation arrives as
`delta.reasoning_content` and is billed inside `completion_tokens` —
1 388 of 1 460 tokens on a trivial probe. So what the strict run most likely
measured is a model deliberating until it hit a generation cap, never reaching
its answer.

*Likely*, because the harness could not tell: it recorded neither
`finish_reason` nor `reasoning_tokens`, so "the reply never finished" and "the
reply is malformed" were the same row. That gap is E20, and the strict
comparison is worth re-running once the instrumentation can state the cause
instead of inferring it. Until then the setting decision stands on the outcome
counts, which need no mechanism to be true.

### E20 — a reply cut off at the token cap was counted as a reply the model got wrong (2026-08-12) — FIXED

`bench/model_fit.py` collected `delta.content` and `completion_tokens` and
nothing else, so an attempt that spent its whole budget on `reasoning_content`
and produced no answer landed in `invalid_json` — blaming the model for our
token budget. Same class as E15: a thing that could not run, scored as a thing
that ran and failed.

Fixed: `finish_reason` and `completion_tokens_details.reasoning_tokens` are
captured and persisted per attempt, `local_reasoning_tokens` joins the aggregated
metrics, and a **sixth outcome** — `truncated` — separates `finish_reason:
length` from malformed JSON. The grade stays 0 either way: `outcome` is
categorical beside the ladder and never renumbers it (E15's rule). Selftest
green on all five existing rungs, so the split changed no historical grade.

The reasoning split is a KPI in its own right, not bookkeeping: a task whose
answer is 300 tokens and whose deliberation is 6 000 costs what the 6 000 costs,
and `LOCAL_OUTPUT_TOKENS` alone never said so.

### D32 — a scalar carries its type, and `create` is an operation because three failure shapes asked for it (2026-08-12)

Two changes to the operation library, both decided by the E16 run rather than by
taste.

**Every scalar in a signature now carries a type** (`schematic:path`,
`pitch?:number`, `tool:string`). The notation already typed compound fields, so
a bare name was the one thing in it that looked like a flag — and 24 of 60
attempts wrote `"schematic": true`. The vocabulary is deliberately three words
(`path`, `string`, `number`) with no `enum(...)` form: the prose after each
signature already lists `direction`'s values, and the library is paid on every
prompt that carries it.

**`create{path,name}` is an operation, not a `call`.** It expands to exactly one
`create_project`, which is what `call` would have done — the difference is that
the model can find it. The prompt hands over an operation library and no tool
catalogue, so `create_project` was unguessable from what the model was given,
and it showed: 7 attempts referenced `${create.schematic}`, 10 targeted a
schematic nothing had created, 1 wrote `op: create_project`. Three independent
shapes converging on one missing operation is evidence for adding it; hinting
harder would have taught the benchmark, not the tool.

The rejected alternative was to pre-create the project in `fresh_env`. It would
have removed the failure and measured a task nobody performs: a real delegation
starts from an empty directory, and the golden suite's own tasks create their
projects through tools.

### D30 — the graph wins on filtering and adjacency, not on serialisation (2026-08-11)

The honest result of the first measurement: `graph_query kind=symbol` with no
filter costs **340 tk** compact against **310 tk** for the `list_schematic_components`
dump of the same six items. Ten per cent worse, after the compact projection
already took 525 down to 340.

It stays that way. The remaining 30 tokens are the full UUID key (~23 tk/item)
and the repeated `document` field, and both were considered:

* **Shortening the key was rejected.** `key` is what `graph_neighbors` takes.
  A short id would need a second resolution index in `kam-graph` and would buy
  tokens by making the graph's one distinguishing capability need a round trip —
  the graph exists to be addressable, so the address is not the thing to
  compress.
* **The unfiltered query is not the use case.** A query that names no filter is
  a dump written in query syntax, and `list_schematic_components` is a better
  dump. The measurements that matter are the other two: a filtered query is
  **109 tk against 310** (−65 %), and `graph_neighbors` answers a question no
  dump answers at all without the caller re-deriving geometry.

Recorded rather than hidden, because the alternative is a tool whose
documentation implies a saving in a case where it costs more. The tool's own
description says which case it wins.

### D26 — `SUPPORTED` is discovered, never declared (2026-08-11)

The matrix could have carried a status field per capability, which is how most
parity tables are built and why most of them are optimistic. It does not:
`capability::coverage` scans `#[test]` names, `#[ignore]` attributes and the
golden benchmark tasks for each tool and publishes the strongest proof it finds.
A tool with no proof reads `NOT_TESTED` regardless of how finished its code is,
and an `#[ignore]`d test — the ones needing a live KiCAD GUI — reads `gated`,
which is displayed and supports no claim.

The consequence is the point: the first render says **27.3 %**, and the whole
`pcb_components` / `pcb_routing` path is unproved because `ipc!` has no file
fallback. A hand-maintained table would have said "supported" for all of it and
been wrong in the direction that costs a user a board.

Capabilities KiCAD has no API for (`GUI_ONLY_NO_API`, `REQUIRES_CUSTOM_KICAD`)
leave the denominator entirely, so the percentage never conflates "we did not
build it" with "KiCAD cannot do it". `MISSING` carries what a tool-keyed matrix
structurally cannot report — buses, a standalone drill export, IPC-D-356, the
stackup write KiCad 10 declares and does not implement.

### D27 — the matrix reports the defects it finds, then they get fixed (2026-08-11)

`export_bom` published as `PARTIAL` with "registered in `pcb_export` while
reading only schematic data" as its own limitation string. That is E8, sitting in
a generated document, re-rendered on every build.

It could have stayed that way — the tool works, and the workaround is one extra
`load_toolset`. It did not, because a limitation that is a *taxonomy* mistake
rather than a KiCAD one has no business in the denominator's numerator forever:
moving one `tool!` registration is cheaper than every future agent discovering
the same thing at run time. The benchmark task that documented the workaround in
a comment lost both the comment and the toolset.

Recorded honestly: the fix pays in `toolsets` mode only (−1 757 catalogue tokens
of premium for that task) and is worth exactly nothing through the gateway,
which never refreshes a catalogue. The general win is not tokens — it is one
fewer `toolset_not_loaded` on a path an agent had no reason to expect one.

### D16 — an expired handle is not an unknown one (2026-08-11)

The evidence store is bounded, so a handle can outlive its body. `get()` could
have returned one "not found" for both cases and been simpler. It does not,
because the two demand opposite responses: an evicted handle means "re-run the
check, the reply's own summary is still accurate", and an unknown one means the
caller invented a URI. Conflating them would let an agent read "not found" as
"the server is lying about what it did". The store keeps a high-water mark of
issued ids for exactly this discrimination.

### D17 — `verify` is opt-in, and a typo is refused (2026-08-11)

Measured on `bench/probes/validators.yaml`: the same batch is **7 ms** without
verification and **~1 100 ms** with it. Making ERC the default would pay a
second on every placement to make the occasional checkpoint cheaper, which is
the wrong trade for a per-task latency KPI.

That makes the default *silence*, and silence is dangerous in a way `diff`'s
default is not: `diff` defaults to saying something, so a misspelled value
still produces an audit line. A misspelled `verify` would produce nothing while
the caller believed a check had run — E4 exactly. So `verify` rejects an
unrecognised value with `invalid_argument` and runs no calls, while `diff`
keeps falling back to its default. The two arguments look symmetric and are
deliberately not.

### D18 — the ERC baseline is cached, not recomputed (2026-08-11)

A true before/after would run the validator twice per batch: ~2.2 s instead of
~1.1 s, for a number the session already has. Each verdict is instead stored
against the content revision it describes (`konnect-core::evidence::validators::Cache`),
so the next batch on the same document finds its baseline for free.

The honest cost is that the *first* verification of a session has no baseline,
and it reports `baseline: "unknown"` rather than implying the design went from
zero to zero. A document that did not exist before gets an empty baseline
instead, because nothing was wrong with it — it was not there.

Verified end to end by the probe: batch 1 `errors: 4, baseline: unknown`,
batch 2 `errors: 2, fixed: 2`.

### D19 — findings are identified by rule and location, never by prose (2026-08-11)

`kam-evidence::finding` hashes `validator + rule + location` into a 12-hex id.
Prose is excluded on purpose: KiCAD rewords descriptions between versions, and
two unconnected pins on one sheet share every word. This is what makes
`fixed: 2` mean two ids that left rather than a count that fell — a count going
from 4 to 4 is also what two fixes plus two regressions look like.

Two genuinely identical findings (same rule, same location) get an ordinal
folded into the digest rather than being collapsed: collapsing would
under-report, and sharing an id would make one look fixed the moment the other
was. Requires KiCAD's `type` field, which the CLI parser previously discarded;
`ErcViolation`/`DrcViolation` now keep it as `rule`.

### D15 — the diff engine is format-agnostic on purpose (2026-08-10)

`kam-evidence` could have parsed KiCAD S-expressions directly and been half the
code. It does not, for two reasons that both cost something now and pay later:
the licence rule (D11) keeps generic subsystems free of AGPL-derived code, and
the split means a second document format costs an extractor rather than a
second diff engine. The extractor lives in `konnect-core::evidence` where the
KiCAD knowledge already is.

---

## BENCHMARKS

Full detail and method: **`docs/benchmark.md`**. Headline:

| Metric | Konnect baseline | Fork, Phase F | Fork, Phase D | Fork, Phase E | Fork, Phase G | Fork, matrix | Fork, graph | Δ vs baseline |
|---|---|---|---|---|---|---|---|---|
| SUCCESS_RATE | 18/18 | 18/18 | 18/18 | 18/18 | 18/18 | 18/18 | **18/18** | = |
| EXTERNAL_TOKENS/task | 12 373 | 1 995 | 2 033 | 2 175 | 2 171 | 2 178 | **2 174** | **−82.4 %** |
| CATALOG_TOKENS/task | 8 389 | 0 | 0 | 0 | 0 | 0 | **0** | −100 % |
| MCP_CALLS median/task | 11 | 4 | 4 | 4 | 4 | 4 | **4** | −7 |
| WALL_CLOCK_P50 | 70 ms | 72 ms | 67 ms | 68 ms | 66 ms | 64 ms | 62 ms | ≈ |
| WALL_CLOCK_P95 | 888 ms | 916 ms | 911 ms | 854 ms | 877 ms | 885 ms | 951 ms | ≈ |
| `tools/list` at startup | 1 680 tk | 1 725 tk | 1 912 tk | 2 034 tk | 2 034 tk | 2 034 tk | **2 034 tk** | +354 (once per session) |

ProjectGraph costs **nothing** on this suite in either column, which is the
expected result and not a claimed win: the three `graph_*` tools are a toolset,
so they are absent from the startup catalogue, and the golden tasks never call
them. The full catalogue grew 203 → 206 tools (25 058 → 25 238 tk), paid only by
a client that loads everything. `graph_query`'s schema is **662 tk**, second
heaviest in the repository after `create_symbol` — worth watching if it is ever
promoted out of a toolset.

The 2 171 → 2 178 → 2 174 across the last three columns is **not** a regression
or a saving to explain: repeated
runs of the same build spread ±12 on a single task (`sch_hierarchy` measures
2 195 / 2 200 / 2 208 in one three-run set), so the two columns are the same
number. The E8 fix that landed with the matrix pays in `toolsets` mode, where
`manufacturing_exports` lost a **1 757-token catalogue premium** over its peers,
and pays nothing through the gateway, which never refreshes a catalogue.

The plan path is measured separately, by `bench/plan_cost.py`, because the
golden suite is a scripted oracle: it already knows every call, so it can never
pay the cost of not knowing them, which is the whole thing a plan removes. Same
design built twice, void unless both shapes produce the same semantic diff and
the same ERC verdict:

| | as a batch | as a plan | Δ |
|---|---|---|---|
| divider — request tokens | 517 | 470 | −9.1 % |
| divider — response tokens | 1 663 | 654 | −60.7 % |
| divider — external tokens | 2 180 | **1 124** | **−48.4 %** |
| decoupling bank — request tokens | 767 | 236 | **−69.2 %** |
| decoupling bank — external tokens | 2 265 | **882** | **−61.1 %** |

Two different savings, worth keeping apart. The divider's is structural — five
tool schemas become one, six per-call results become one summary — and its
request barely moves, because every coordinate in a divider is data the caller
chose and a plan does not compress data. The bank's is the macro: one `decouple`
replaces nine calls and eight power-symbol positions the caller never writes.

Phase D bought preconditions, idempotency and rollback for **+38 tokens/task**
and **+187 startup tokens**. Phase E bought the semantic diff (+125/task, +40
startup), the evidence handle (+14/task, +0 startup) and independent
verification (+0/task on this suite, +46 startup — the suite does not call
`verify`) and the Task State Manager (+0/task, +36 startup, and **0** for the
four task tools themselves because they are a toolset rather than gateway
verbs). All of it moves the startup number further from its ≤ ~1 000 target
and the per-task number further from ≤ 2 000; both targets are recorded as
missed rather than moved, and no win is netted off against them.

`verify`'s real cost is not tokens but latency: **7 ms → ~1 100 ms** for the
same batch, measured on `bench/probes/validators.yaml`. That is the whole
reason it is opt-in.

Intermediate `tools` mode now sits at **3 770** tk/task, not the 3 197 recorded
at Phase F. Corrected rather than quietly updated: the drift is Phase D/E's
meta-tool growth (+580 at startup) being re-sent on each `tools/list` refresh,
not Phase G — the golden suite never loads the `plan` toolset, which
`bench/results/fork-phaseG-plan-tools.json` shows directly. It is kept measured
because it is what a client that does not use the gateway pays.

Startup is 45 tokens above upstream while carrying **four** extra meta-tools;
the +278 regression from step 1 was repaid by the starter-kit work and then
partly re-spent on the gateway verbs, which pay for themselves after the first
task of any session.

Build/test baseline: `cargo build --release -p konnect` 81 s cold;
`cargo test --workspace --lib --tests` 469 → 487 → 525 → 567 → 588 → 606 →
682 → 700 → 706 → 752 → 758 → **759 passed, 0 failed** on the fork, plus the `#[ignore]`d e2e
tests, of which `plan_postconditions_e2e` was run against a real KiCad 10 for
this phase rather than only compiled. `cargo fmt --check` and `cargo clippy --workspace --locked -D
warnings` are clean.

---

## ERROR HISTORY

### E1 — `protoc` missing (2026-08-10) — RESOLVED

`konnect-ipc/build.rs` needs `protoc` on `PATH` or `$PROTOC`; neither existed.
Installed `Google.Protobuf` via winget → `libprotoc 35.1`. winget's PATH edit
does not reach an already-open shell, so every build command in this project
sets `PROTOC` explicitly, and `gate.ps1` discovers it automatically.

### E2 — Konnect auto-installed into the user's `~/.claude` (2026-08-10) — NOTED

Running the built binary triggers `install::needs_install()` →
`run_install_silent()`, which wrote six skills (`kicad-*`, `konnect`) and two
agents into `C:\Users\FlowUP\.claude\` at 14:18:36. Gated by a `.installed`
marker so it will not repeat, but it is a side effect upstream performs without
asking. **Open action:** add a `KAM_NO_INSTALL=1` opt-out before any automation
runs the binary in a loop.

### E3 — Per-user KiCad installs were invisible (2026-08-10) — FIXED

```
Library 'Device' not found in the installed KiCAD symbol libraries (lib_id 'Device:R').
```

Every symbol lookup failed on a stock machine. `winget install KiCad.KiCad`
produces "KiCad 10.0 (current user)" under `%LOCALAPPDATA%\Programs\KiCad\10.0`,
and the hard-coded search list only knew `C:\KiCad` and `C:\Program Files\KiCad`.
Worse, the list was written **twice** — `konnect-core::tools::kicad_share_roots`
(knew KiCad 8) and `library::find_symbol_dirs` (stopped at 9) — and the copies
had drifted.

Fix: one module, `konnect-schematic-editor::kicad_paths`, used by both. Adds
`%LOCALAPPDATA%\Programs`, `%APPDATA%`, and `%ProgramFiles%` read from the
environment instead of hard-coded. Major-outer/prefix-inner ordering so a stale
`C:\KiCad\8.0` can never shadow a current per-user 10.0. Three tests, one of
which asserts the winget prefix is present.

### E4 — `run_erc` reported "0 errors" while doing nothing (2026-08-10) — FIXED IN HARNESS

```
Tool error: Failed to spawn kicad-cli: kicad-cli.exe
```

`kicad-cli` was not on `PATH`. The failure was visible, but on an empty
schematic the *next* run still returned `{"errors":0,"total":0}` — a benchmark
that scores a no-op as a pass. `bench/konnect.bench.toml` now pins the absolute
`kicad_cli` path for every run.

### E5 — Benchmark undercounted external tokens by ~8 000/task (2026-08-10) — FIXED

The bench client skipped `notifications/tools/list_changed` instead of
re-fetching `tools/list`, so `load_toolset`'s real cost was invisible. A real
client has no such choice. `bench/mcp_client.py` now refreshes on notification
and `CATALOG_TOKENS` is reported as its own line.

### E6 — Grid snapping is inconsistent between placement tools (2026-08-10) — FIXED

`add_schematic_component` / `batch_place_components` snap to the 1.27 mm grid
(100, 80 → 100.33, 80.01). `add_power_symbol` does **not** — it writes the
coordinate verbatim. Placing a power symbol at the same nominal coordinate as a
resistor therefore leaves it 0.33 mm off the pin, and KiCad ERC reports
`Pin not connected` for both. No tool errors; the schematic is simply wrong.

Observed on the first divider probe: 6 ERC errors, all from this.

Fixed 2026-08-11, as a class. One `snap_reporting()` helper over
`konnect_sexp::geometry::snap_point`, one `SCHEMATIC_GRID_MM` constant that
`plan::ops` aliases instead of restating, and every electrically meaningful
`(at x y)` a schematic tool writes routed through it. Looking for the other
instances is what the fix was actually worth: **nine more tools had the same
bug** — `add_no_connect`, `add_net_label`, `add_junction`,
`batch_add_junction`, `connect_passthrough`, `connect_to_net`,
`add_schematic_text`, `add_sheet_pin`, and `apply_template`, whose 15 mm column
spacing is not a multiple of 1.27 and therefore drifted off-grid even from an
on-grid origin. Only `add_power_symbol` had ever been observed failing.

A snap that moves the coordinate is reported (`requested`, `snapped_to_grid`);
one that changes nothing is silent. Sheet outlines and `move_sheet` are left
unsnapped on purpose — not connection points.

Proved by `power_symbol_snaps_to_grid_like_components_e6`, which reconstructs
the original failing placement and asserts 0 errors from real `kicad-cli sch
erc`. Run, not merely written.

### E7 — Konnect's own connectivity analysis disagrees with KiCad ERC (2026-08-10) — OPEN

On the same broken schematic, `find_single_pin_nets` returned
`{"single_pin_net_count": 0}` and `list_schematic_nets` returned `{"count": 0}`
while `kicad-cli sch erc` reported 6 unconnected-pin errors. The internal
analysis is not a substitute for the real validator. This is direct evidence for
the project's own rule: **never report OK from an internal check when a real
validator exists.**

Partly addressed 2026-08-11: `kicad_invoke verify: "auto"` gets its verdict
from `kicad-cli`, never from `find_single_pin_nets`, so the *evidence path* no
longer depends on the internal analysis.

Second half, same day: the capability matrix now labels all fifteen in-process
connectivity tools with a shared `ADVISORY` limitation — "connectivity derived
in-process, and it has disagreed with kicad-cli ERC (E7) — the verdict comes from
run_erc / verify" — so none of them can publish as `SUPPORTED` and the disclaimer
is rendered next to each.

Third half, same day — DISCLOSURE CLOSED. `router::registry::tools_for` appends
an advisory suffix to the description of every tool the same `MANIFEST` marks
`ADVISORY`, so a model reads it at the call site instead of in a document it
will never open. +404 catalogue tokens on those fifteen, +0 at startup, +0 on
every other tool.

**What remains open is the defect itself, not its disclosure:** the in-process
connectivity analysis still disagrees with `kicad-cli`. It is now labelled
everywhere it is reachable and excluded from every path that produces a verdict.
Making it agree would mean re-implementing KiCad's connectivity, which is a
Phase J question, not a patch.

### E8 — `export_bom` lives in the `pcb_export` toolset (2026-08-10) — FIXED

`export_bom(schematic)` reads only schematic data but is registered under
`pcb_export`. An agent that loaded every schematic toolset still gets
`toolset_not_loaded` and pays a failed call plus a `load_toolset` round trip.
Taxonomy defect; fix belongs with the capability matrix work.

Fixed 2026-08-11, and the matrix is what forced it: the capability manifest had
to state the misplacement as the tool's own `PARTIAL` limitation, printed on
every render. The registration moved to `sch_export` (7 tools, `pcb_export` 12),
`bench/tasks/05_manufacturing_exports.yaml` dropped `pcb_export` from its toolset
list, and the skill and tool-directory docs that pointed at the old home were
corrected. Measured in `toolsets` mode: that task's **+1 757 catalogue-token
premium over its schematic-only peers is gone**. Worth nothing in `gateway` mode
by construction. `bom` domain coverage 0 % → 100 %.

### E9 — Error messages leak the OS locale (2026-08-10) — FIXED

```
{"error":{"kind":"handler_error","reason":"IO error: Le fichier spécifié est introuvable. (os error 2)"}}
```

`std::io::Error` is formatted straight into the agent-facing payload, so the
same failure has different text on a French and an English machine. Error
matching, dedup, and any stable-finding-id scheme break on that. The error
catalog must carry a stable code and keep the localized string as a detail
field.

Fixed with `ToolErrorKind::from_anyhow`, which walks the `anyhow` source chain
for a `std::io::Error` and emits `Io { code, detail }` — `code` from an explicit
`ErrorKind` mapping (never `Debug`, which is not a stability promise), `detail`
keeping the OS's own words for a human. Used at both places a handler error
reaches an agent: `handler.rs` dispatch and `kicad_invoke`'s inner loop. A test
asserts the French string classifies as `not_found`.

Still open in one respect: `SexpError::Io`'s own `Display` ("IO error: {0}")
remains localized. That is now a *detail* string rather than the matchable
value, which is the part that mattered.

### E11 — Structured errors were Debug-formatted into opaque strings (2026-08-10) — FIXED

Found by the gateway's first test. `get_symbol_info` with no `lib_id` returned:

```
Tool error: CallToolResult { content: [Text { text: "{\"error\":{\"field\":\"lib_id\",\"kind\":\"invalid_argument\",...
```

The cause is `require_str(args, "k").map_err(|e| anyhow::anyhow!("{:?}", e))?`
— eight sites across `library.rs` and `integration.rs`. The helper builds a
proper structured `invalid_argument` with a `field`, and the handler then
Debug-formats it into a string and re-wraps it as `handler_error`. Every
consumer that matches on `error.kind` — recovery loops, the observability
`error_kind` column, any future stable finding id — silently degrades to
"something went wrong" on exactly these calls.

This predates the fork and affects the normal path as much as the gateway; the
gateway only made it visible because its test asserted on the error *kind*
rather than on the presence of an error.

Fixed with a `try_arg!` macro that returns the structured result unchanged, and
the macro's doc comment states the anti-pattern so it does not come back. All
eight sites converted.

### E12 — a floating passive pin is an ERC **error**, not a warning (2026-08-11) — FIXED IN TEST

The first postconditions e2e test asserted that a two-resistor schematic with the
pair wired together was ERC-clean, on the strength of a comment in this repo's own
`e2e_kicad.rs` ("a 2-part net has floating-pin warnings"). It failed:

```
assertion `left != right` failed: a clean schematic must pass erc_clean:
{"error":{"check":"erc_clean","document":"clean.kicad_sch","errors":1,...},
 "message":"Postcondition 'erc_clean' failed on clean.kicad_sch: 1 error(s), 0 warning(s), 1 new finding(s)."}
```

Probed directly against KiCad 10 `kicad-cli sch erc`:

```json
{"errors":1,"warnings":0,"violations":[
  {"description":"Pin not connected: Symbol R1 Pin 1 [Passive, Line]","severity":"error"}]}
```

So the postcondition machinery was right and the fixture was wrong. The clean
case is now the golden suite's own divider — `bench/tasks/01_sch_divider.yaml`,
which asserts `erc_max_errors: 0` on every benchmark run — where the two
`PWR_FLAG`s and two power symbols sit exactly on the outer resistor pins and
nothing floats. The broken case keeps floating pins *and* a duplicate reference.
Verified by running the test, not by reasoning about it: **it passes.**

The misleading comment in `e2e_kicad.rs` is corrected in place, with the measured
severity written down, because it is what caused the wrong assumption once.

Worth keeping in mind for Phase H: a plausible-looking schematic that a model
would call finished is ERC-*error* territory in KiCad 10 as soon as one pin is
left alone. `erc_clean` is a stricter promise than it sounds.

### E13 — the first model-fit run measured nothing: the runtime cannot load the model (2026-08-11) — RESOLVED

The harness ran to completion, exit 0, and produced
`bench/results/model-fit-qwen3.5-9b.json` with **every** `grade` at `null` and
`success_rate: null`. All 60 attempts (4 tasks × 3 hint levels × 5 repeats)
failed identically, before the model was ever reached:

```
backend HTTP 400 at http://127.0.0.1:1234/v1/chat/completions:
{"error":{"message":"Failed to load model \"qwen3.5-9b\". Error: Failed to load model",...}}
```

`/v1/models` lists the model, and 14 984 MiB of the RTX 5080's 16 303 MiB were
free, so it is neither absent nor a VRAM ceiling. The cause is one line, from
`lms load qwen3.5-9b` rather than from the HTTP layer, which hides it:

```
error loading model: error loading model architecture: unknown model architecture: 'qwen35'
```

The selected runtime is `llama.cpp-win-x86_64-nvidia-cuda12-avx2@1.104.2`, and
`lms runtime update` answers *"All matching runtime extensions are already
up-to-date"* — so this is not a stale install that an update fixes.

**This is the risk D31 wrote down, arriving — but not in the shape it expected.**
D31 recorded that `llama.cpp`'s support for Qwen3.5's hybrid Gated DeltaNet
architecture was "suggested by the existence of community GGUFs and confirmed by
nobody". Checked against primary sources, upstream **does** support it:
`LLM_ARCH_QWEN35` and `LLM_ARCH_QWEN35MOE` are in `src/llama-arch.cpp` on
`master` and both are listed in `llm_arch_is_hybrid()`; the merge is
[PR #19468](https://github.com/ggml-org/llama.cpp/pull/19468) (2026-02-10, after
[#19435](https://github.com/ggml-org/llama.cpp/pull/19435) was merged and
reverted two days earlier), with `GATED_DELTA_NET` backend coverage landing
per-backend since.

So the defect is **local and one version deep**: this machine runs LM Studio
**0.3.39**, and LM Studio's own 0.4.0 release note (2026-01-28) says the engine
"graduates to version 2.0.0" and the CLI switches from semver to commit hashes.
A runtime still numbered `1.104.2` is therefore on the pre-0.4.0 channel, which
predates the 2026-02-10 merge — which is exactly why `lms runtime update` can
say "already up-to-date" and still not know the architecture. (The version
mapping is not documented by LM Studio; the chain above is an inference from
their release note, and it is labelled as one.)

Two ways out, and they are not equivalent:

* **Update LM Studio past 0.4.0.** This is the only path that makes the D31
  shortlist's first candidate runnable at all. It is a desktop-app update on the
  user's machine, so it is theirs to make, not something to do to them silently.
* **Use a model the installed runtime already knows.** `gpt-oss-20b` (llama.cpp
  [PR #15091](https://github.com/ggml-org/llama.cpp/pull/15091), native MXFP4)
  and `Qwen3-14B-GGUF` (arch `qwen3`, official Qwen repo) both predate this
  runtime by a wide margin. This is the unblocked path and is the one taken.

The VRAM figures matter for which of the two: llama.cpp's own gpt-oss guide
([discussion #15396](https://github.com/ggml-org/llama.cpp/discussions/15396))
puts it at 12.0 GB of model data and **~14.9 GB at 8 192 tokens of context**,
against 14 984 MiB free here — a margin of essentially zero, so a partial
offload is likely and the measurement must report it rather than assume it.
`Qwen3-14B` at `Q5_K_M` is 10.5 GB and leaves room for KV cache, which makes it
the fallback if gpt-oss spills.

**Resolved the same evening, and by the first branch rather than the second.**
LM Studio updated itself mid-session — 0.3.39 → **0.4.20** — which killed the
`gpt-oss-20b` download in flight (WebSocket closed, a 5.39 GB `.part` left
behind, then `spawn ... LM Studio.exe ENOENT` while the app restarted) and
switched the engine channel. `lms runtime ls` now selects
`llama.cpp-win-x86_64-nvidia-cuda12-avx2@**2.28.2**`, and:

```
Model loaded successfully in 6.84s. (7.71 GiB)
```

So the inference in the paragraph above is confirmed rather than merely
plausible: the blocker was the application's version, not the model, not the
GGUF, and not upstream. 7.71 GiB resident leaves real headroom on 16 303 MiB,
unlike the gpt-oss estimate, and the D31 shortlist's first candidate is
measurable after all. `gpt-oss-20b` stays worth having as the second data point
and its download has to be restarted.

Two things this does **not** invalidate: the oracle passed its selftest before
any model was involved (grades 3/1/2/0, expected = obtained), and the stable
prompt prefix measured **800 tk**, identical across all four tasks and all three
hint levels. What is unmeasured is everything about a model.

One harness defect found on the way and fixed in the invocation rather than in
the code: `.\target\release\konnect.exe` passed through a `bash` layer loses its
backslashes and becomes `.targetreleasekonnect.exe` (`FileNotFoundError WinError
2`). Forward slashes work. Worth knowing before blaming the harness for the next
`WinError 2`.

### E14 — the plan's operation library documents field names, not field shapes (2026-08-11) — FIXED

The first real model-fit run, once E13 was out of the way: `qwen3.5-9b`, 60
attempts (4 tasks × 3 hint levels × 5 repeats), **0 at grade 3, 0 at grade 2,
ceiling grade 1**. Not one plan compiled. One failure dominates:

```
invalid_argument (components[0].lib_id): required
```

34 of 60, on the `place` operation, across every task and every hint level.

**It is our defect, not the model's.** `PLAN_DESCRIPTION`
(`crates/konnect-core/src/tools/plan.rs:46`) is the only thing an agent reads
about the operation library — `bench/model_fit.py` builds its OPERATION LIBRARY
block from `schema.properties.plan.description`, exactly as any MCP client
would. It says:

```
place{schematic,components,at?,pitch?,direction?}
```

It names `components`. It never says what an item of `components` contains, and
the string `lib_id` appears nowhere in it — while
`crates/konnect-core/src/plan/ops.rs:100` makes it required. A model cannot
guess a field name the tool documents nowhere, and neither can Claude. The same
gap explains the secondary failures: `at` must be a complete `{x, y}` (`ops.rs`
`need_point` / `optional_point`, and the test `place_refuses_half_a_position`
proves the halves are refused), and nothing says so.

This is E7's class one level in: the description at the call site is the only
documentation that gets read, and it did not carry what the call requires.

**What the run therefore measured is the schema, not the model.** The grades
above are not evidence about `qwen3.5-9b` and are not being recorded as such.
The honest statements are: the harness works end to end against a real local
model (60/60 attempts completed, no HTTP failure), throughput is **99–104
tok/s** at **95–543 ms** TTFT, the model fits entirely on the GPU
(**9 020–9 109 MiB** peak of 16 303, no spill), and `model_fit.py` produces
every local KPI the metric list asks for — `local_input_tokens`,
`local_output_tokens`, `ttft_ms`, `tokens_per_second`, `wall_clock_ms`,
`vram_peak_mib`.

The fix is to the tool, never to the benchmark: the operation signatures must
state their item shapes, by a mechanism that cannot drift from the expander
that enforces them — E6's single-constant rule and E7's single-`MANIFEST` rule,
applied again. The tasks, hints and grading ladder stay untouched so the re-run
is comparable, and the comparison will be labelled as what it is: **a
documentation defect being fixed, not a model improving.**

**Fixed 2026-08-11, and measured on the same 60 attempts.** A `*_SIGNATURE`
constant sits directly above each `expand_*` in `plan/ops.rs`, `OP_LIBRARY`
pairs them with their names, and `plan::description()` assembles the one string
both `preview_plan` and `apply_plan` publish. The anti-drift link is a test, not
a promise: `every_documented_signature_has_a_working_minimal_example` holds a
minimal document per operation, checks the set against `OP_LIBRARY` by name and
order so no operation can be added without one, and expands each — so an
expander requiring a field its example does not carry fails the build. Nothing
was relaxed — `lib_id` is still required and `at` is still a complete `{x, y}`.

**Corrected 2026-08-12, having claimed more than the code does.** This paragraph
originally said the document was built "out of nothing but its documented
signature". It is not: the examples are hand-written JSON, and nothing parses the
`*_SIGNATURE` strings. The test therefore catches an expander that drifts from
its *example*, and catches an example that drifts from its *signature* only as
far as a human transcribed it faithfully — which is weaker than claimed, and is
the guarantee both E14 and E17 rest on. Found while implementing E17, by looking
for the parser the prose implied and not finding one.

| | before | after |
|---|---|---|
| `components[*].lib_id: required` | **34/60** | **1/60** |
| grade ≥ 2 (the plan compiles and applies) | **0/60** | **23/60 (38.3 %)** |
| grade 3 | 0/60 | **0/60** |
| local input tokens (mean) | 1 175 | 1 667 |
| stable prefix | 800 tk | 1 285 tk |
| TTFT / tok/s / VRAM peak | 178 ms / 102.9 / 9 031 MiB | 168 ms / 101.9 / 9 038 MiB |

The +492 measured input delta matches the +485 the prefix grew by, so the cost
is exactly the documentation and nothing else. Startup catalogue is **2 034 —
unchanged**, because `plan` is still a lazy toolset; the full catalogue grows
25 642 → 26 130. Golden suite **18/18 unchanged**, gate green at **779 tests**.

Neither of the two failure modes this run was instrumented to catch appeared:
no `${op_id.field}` in a coordinate, and no plan over `MAX_STEPS_PER_OP`. Both
gaps stay undocumented on purpose until a measurement points at them.

**Grade 3 is still zero and the wall has moved rather than fallen.** The new
dominant failure is 23 attempts at grade 2 reporting `applies: true`,
`erc_errors: 0` and `found=[]` — a plan that applies, an ERC that says clean,
and a `list_schematic_components` that sees nothing. That is E4's shape, and
diagnosing it produced E15.

### E15 — a totally failed plan reports success, and a failed check reads as clean (2026-08-12) — FIXED

Reproduced live, outside the model, by replaying one captured grade-2 plan. The
symptom is a chain of three, and only the first belongs to the model:

**The model's part.** It writes `"schematic": "$SCH"` — the *literal* notation
from the DYNAMIC TASK block, never substituted for the path the same block
defines two lines later. Every operation then targets a file that does not
exist.

**Ours, first half — the tool.** `handle_apply_plan`
(`crates/konnect-core/src/tools/plan.rs:502`) returns
`Ok(CallToolResult::json(&body))` unconditionally. Replaying the plan returns
top-level `isError=False` while the body says
`"ok":0, "failed_at":0, "not_run":7, "rollback":true, "error":"IO error: … introuvable"`.
A plan that applied nothing and rolled everything back is presented to the agent
as a success. This is inconsistent with the project's own D28, which already
makes a *postcondition* failure set `is_error`: the **step** failure path never
learned the same thing.

**Ours, second half — the oracle, and this one is worse.**
`bench/runner.py:139-166` `check_assertion` parses the JSON of `run_erc`,
`list_schematic_components` and `list_schematic_nets` without ever looking at
`isError`. Measured live: `run_erc({"schematic":"$SCH"})` answers `isError=True`
with `{"error":"kicad-cli exited with 3: Échec du chargement…"}`, the assertion
reads a defaulted `violations=[]`, and concludes **0 ERC errors**. A check that
could not run scores as a check that passed — the exact failure E4 named on day
one, sitting inside the thing that grades everything else.
`bench/model_fit.py:384-389` has the same hole one level up: it tests only the
top-level error, so a full rollback counts as `applies: true`.

The consequence is not confined to Phase H. **Any `erc_max_errors` assertion
whose call fails passes**, so every golden-suite number resting on one is
unproven until this is fixed and the suite is re-run. That re-run is expected to
be able to lower 18/18, and if it does, the lower number is the true one and
stays.

The evidence file has its own gap, recorded for the next harness change:
`bench/results/*.json` never persists the model's raw plan text, which is why
this needed a live replay instead of a query over the results already on disk.

**Fixed 2026-08-12, in all three places.** `handle_apply_plan` sets
`is_error: report.failed_at.is_some()` with the report body untouched, so the
caller still learns exactly what ran; `kicad_invoke`'s `entry["ok"] =
!result.is_error` then makes an atomic batch roll a failed plan back the same
way a failed postcondition already did. `bench/runner.py` grew `_call_failed` /
`_call_error_detail` — the same test `step_errors` was already using — and they
now guard `erc_max_errors`, `components_present`, `nets_present` **and
`no_single_pin_nets`**, a fourth case with the identical hole that the
diagnosis had not named. `bench/model_fit.py` additionally reads `failed_at` /
`rollback` out of the body, so a future regression on the Rust side cannot
silently restore `applies: true`. A test named for what it protects —
`a_failed_step_sets_is_error_so_an_atomic_batch_rolls_back` — locks the first
half.

**The golden suite was re-run against the stricter oracle and still scores
18/18** at 2 194 tk/task, 4 MCP calls, P50 64 ms. That is the answer to the
question the fix raised: the suite's passes were real, not artefacts of failed
calls reading as empty. Gate green.

**Re-measured on the same 60 attempts, and the headline did not move:**
`{0:10, 1:27, 2:23, 3:0}` against `{0:11, 1:26, 2:23, 3:0}` before the fix.
Grade ≥ 2 is 38.3 % either way, and **grade 3 is 0 across all 180 attempts of
the three runs**.

What changed is what the failures *say*. The dominant mode is now explicit —
**22 of 60** report `IO error: Le chemin d'accès spécifié est introuvable.
(os error 3)`, failing at `op1` before any step that could create the document —
where the same attempts previously showed a silent `found=[]`. That is the fix
working: the failure was always there, and it is now named.

It also exposes a defect in the **grading ladder itself**: grade 2 currently
covers two opposite outcomes — "the plan applied and the design is wrong" and
"the plan did not apply at all". While they share a rung, the headline number
means nothing, and that is being fixed in the harness before any model is
compared to another.

Two further measurements from the same run, both about our own prompt rather
than the model:

* **8 of 60 emit an unsubstituted `${…}`** — `${SCH}` twice,
  `${create.schematic}` six times. The benchmark's DYNAMIC TASK block writes
  `$SCH = <path>` while the Plan IR uses `${op_id.field}` as its reference
  syntax, so a model that reads `$SCH` as a plan reference is confusing two
  notations **we** put side by side. All eight are correctly refused at compile
  time — no false grade 3 comes out of it — but the collision is ours.
* **10 of 60 are not valid JSON**, which should be impossible: `model_fit.py`'s
  own docstring claims it sends `response_format: {type: json_schema}`. Either
  it does not, or the backend ignores it. That contradiction is being resolved
  as a fact before anything is changed on the strength of it.

**The ladder now separates the two, and re-counting the three runs on disk says
what E15 was worth.** `outcome` is a categorical field beside `grade`, never a
renumbering, so the historical grades stay comparable: `invalid_json`,
`compile_failed`, `not_applied`, `applied_invalid`, `success`. Re-counted from
fields the runs already persisted:

| run | invalid_json | compile_failed | not_applied | applied_invalid | success |
|---|---|---|---|---|---|
| before E14 | 10 | 50 | 0 | 0 | 0 |
| after E14 | 11 | 26 | 0 | **23** | 0 |
| after E15 | 10 | 27 | **23** | 0 | 0 |

The 23 moving from `applied_invalid` to `not_applied` is **not a regression and
is not the model changing** — it is the same 23 plans, correctly relabelled.
Before E15 they claimed `applies: true` while having rolled back in full; that
claim was the defect, and the new column is what was true all along.

`raw_response`, `compiled_plan` and a structured `failure` (operation, kind,
verbatim message) are now persisted per attempt, truncated at 4 000 chars with
an explicit flag — the instrumentation gap that forced two live replays costs
about +150–250 KB per 60-attempt run.

**The JSON contradiction is resolved, and it is neither of the obvious
answers.** The harness *does* send `response_format`, captured verbatim:

```json
{"response_format":{"type":"json_schema","json_schema":{"name":"kicad_plan","schema":{…},"strict":false}}}
```

`strict` is hard-coded **false**, while `kam-llm`'s own `openai_compat` exposes
it as a caller parameter and its test uses `true`. A best-effort grammar is
consistent with a ~17 % residue of invalid JSON. Turning it on is a change to
the measurement setup, so it is queued as its own run rather than folded in —
one variable at a time.

One place is deliberately left as it is: `bench/runner.py:322`
(`find_capabilities` during `search`-mode setup) still reads its result without
an error check, because a tool the search misses is *supposed* to surface as a
failed step — that is how retrieval is scored, and it is a comment in the code
rather than an oversight.

### E16 — two placeholder notations, one character apart, both ours (2026-08-12) — FIXED

The benchmark's DYNAMIC TASK block wrote `$SCH = <path>` while the Plan IR's
reference syntax is `${op_id.field}`. Measured on the run after E15: **32 of 60**
attempts tripped over the collision — 22 copied `"$SCH"` verbatim into an
argument and failed at `op1` with `IO error: … (os error 3)`, and 10 promoted it
to a plan reference (`${SCH}`, `${create.schematic}`) on a plan that has no such
operation. Two notations one character apart, side by side in a prompt we wrote,
is our defect and not the model's.

Fixed in `bench/model_fit.py`: `$WORK` / `$NAME` / `$SCH` / `$PCB` are expanded
in the objective and in the hints **before** the model sees them, so the prompt
carries literal paths only. `${create.schematic}` survives untouched — it is a
genuine plan reference, and `04_reference_heavy` exists to measure whether the
model writes one.

**Measured on the same 60 attempts** (`qwen3.5-9b`, temperature 0.2,
`strict: false`, stable prefix **1 285 tk unchanged**), against the post-E15 run:

| | after E15 | after E16 |
|---|---|---|
| `$…`-style placeholder in the raw reply | 32/60 | **0/60** |
| `invalid_json` | 12 | 10 |
| `compile_failed` | 25 | **40** |
| `not_applied` | 23 | **10** |
| `success` (grade 3) | 0 | **0** |

The fix did what it was aimed at and nothing more: the literal `$SCH` is gone
from every reply, and the `io` failures fell 22 → 10. The ten that remain carry a
**different** error — `os error 2` (file not found) where the old ones were
`os error 3` (path not found) — because they name the right path in a directory
where nothing was ever created. That is a second defect, not a residue of this
one, and it is E17.

Hint level barely moves any of it: `full` (complete geometry, pin offsets,
PWR_FLAG) fails 14 compile / 4 not-applied / 2 invalid-JSON against `none`'s
13 / 4 / 3. What the model gets wrong is the IR's own encoding, not the
electronics — which is the useful finding, because encoding is ours to document.

Local KPIs from the run, all measured rather than estimated: TTFT median
**185 ms**, **102.6 tok/s**, local input median **1 684 tk**, output median
**1 866 tk**, VRAM peak **9 077 MiB** of 16 303 (no spill), wall clock median
**18.4 s** per attempt.

**Grade 3 is 0 across all 240 attempts of the four runs.** No model is chosen,
and `LLM_CALLS_PER_SUCCESSFUL_TASK` stays unmeasured: there is no successful task
to divide by.

One measurement variable was still hard-coded while this ran:
`response_format.json_schema.strict` was `false`, which E15 recorded as a likely
explanation for the ~17 % invalid-JSON residue. It is now `--strict-json` on
`model_fit.py` and written into the results file, to be run as its own
comparison — one variable at a time.

### E17 — the operation library documents names without shapes, and cannot create a project (2026-08-12) — FIXED

Diagnosing E16's remainder produced two defects of the same class as E14: what
the model gets wrong is what the tool never told it.

**24 of 60 emitted `"schematic": true`** — a boolean where a path belongs. The
signature notation types every compound field (`components:[{lib_id,x?,y?,…}]`,
`at?:{x,y}`) and leaves scalars bare (`place{schematic,components:[…],…}`), so in
that notation a bare name reads as a flag. E14 gave the *items* their shapes and
left the *scalars* without one; this is the half that was missed.

**17 of 60 could not create the project.** The task's work directory is a fresh
empty `mkdtemp` and the objective says to create the project there, and none of
the seven operations can: 7 attempts referenced `${create.schematic}`, 10
targeted a schematic nothing had created (`os error 2` at `op1`), and 1 wrote
`op: create_project` — the right tool name in the wrong slot. The `call` escape
hatch cannot rescue it, because the prompt hands the model an operation library
and no tool catalogue, so `create_project` is unguessable from what it was given.
Three independent failure shapes converging on the same missing operation is
evidence for adding it, not for hinting harder.

Both fixes belong to the tool and neither to the benchmark: the tasks, hints and
grading ladder stay untouched so the re-run is comparable, and the comparison
will be labelled as what it is — **a library defect being fixed, not a model
improving.**

**Fixed and measured the same day**, on the same 60 attempts. Every scalar in
all eight `*_SIGNATURE` constants now carries a type (`schematic:path`,
`pitch?:number`, `tool:string`; three words only, because the prose after each
signature already lists `direction`'s values), and `create{path:path,name:string}`
heads `OP_LIBRARY`, expanding to one `create_project` — the tool whose result
already carried `schematic` and `pcb`. **782 tests**, gate green.

| | after E16 | after E17 |
|---|---|---|
| `"schematic": true` (a boolean where a path belongs) | 24/60 | **0/60** |
| cannot create the project (`io` at `op1`) | 10/60 | **0/60** |
| `invalid_json` | 10 | 10 |
| `compile_failed` | 40 | 44 |
| `not_applied` | 10 | 5 |
| `applied_invalid` | 0 | **1** |
| `success` (grade 3) | 0 | **0** |
| stable prefix | 1 285 tk | **1 549 tk** (+264) |

Both targets went to zero and neither came back in another form. The +264 tokens
are the types and the new operation, paid on every prompt that carries the
library — the same trade E14 made, recorded rather than netted off.

Grade 3 is still 0, and the headline number barely moved, but the *kind* of
failure did, in a way that is worth more than the number: **one attempt applied a
real design end to end** and failed only its ERC budget (`erc errors=3 limit=2`
on the decoupling bank, at hint level `none`). That is the first plan in 300
attempts to reach KiCAD's own validator on its merits rather than on a
mislabelled rollback.

Two new classes surfaced underneath, both previously masked:

* **36 of 60 now fail on how to *name* an earlier operation** — `${0.schematic}`
  (25), `${create.schematic}` (8), `${ops[0].schematic}` (2), `${create0.…}` (1).
  That is E18, and it is not a documentation defect: the rule is stated
  explicitly in the prompt.
* **4 of 60 invent a symbol library** — `Resistor_Small`, `Resistor_SMD`,
  `Resistor`, `Linear_Regulator` — where KiCAD wants `Device:R`. Only at hint
  levels that do not name it: the `full` hint does. The product answer is a
  symbol-search step before placement, not a longer prompt, and it is recorded
  here rather than fixed, because no measurement yet says how often it matters.

Local KPIs, unchanged within noise despite the longer prefix: TTFT median
**172 ms**, **102.3 tok/s**, local input median **1 952 tk**, output **1 730 tk**,
VRAM peak **9 120 MiB**, wall clock median **17.4 s**.

### E18 — the model will not name an operation the way the rule says, and three of its four guesses are unambiguous (2026-08-12) — IN PROGRESS

With E17's walls gone, one failure dominates: 36 of 60 attempts cannot say
*which* operation produced the schematic they want to write to.

```
25  operation 'op2' refers to ${0.schematic}, but no operation '0' exists in this plan
 8  refers to ${create.schematic}, but no operation 'create' exists
 2  '${ops[0].schematic}' is not a reference; expected ${op_id.field}
 1  ${create0.schematic}
```

**This one is not a documentation defect, and saying so matters.** The prompt
already states the rule verbatim — *"The id is whatever you gave that operation;
if you gave none, it is 'op1', 'op2', … by position"* — and the model does not
comply with it. Writing the rule more loudly would be teaching the benchmark.

What can be fixed deterministically is the engine's tolerance, because three of
the four guesses have exactly one possible meaning:

* `${ops[N].field}` — explicit, zero-based, cannot collide with an id.
* `${create.schematic}` — an operation named by its **type**, unambiguous when
  exactly one operation in the plan has that type; two candidates is an error
  that names both.
* A **plan-level `defaults`** object, filling any field an operation omits, so
  the common case needs no reference at all: `defaults:{schematic:<path>}` once
  instead of the same absolute path in every operation. Generic in `kam-plan` —
  no field is special-cased, because that crate may not learn the word
  `schematic` (D11).

**`${0.field}` stays refused**, and that is the deliberate half. Auto-assigned
ids are one-based (`op1`, `op2`), so a bare `${1.field}` may mean the first
operation or the second, and resolving it would be a coin flip that writes to a
real file. The refusal instead lists the ids that do exist, which is what a
repair pass or a retry actually needs. The measured consequence is that the 25
positional references stay failures until the model or a repair loop fixes
them — recorded, not hidden.

**Shipped, and the resolution happens at compile time rather than at run time.**
`refs::rewrite` normalises `${ops[N].field}` and a unique-type `${create.field}`
into the canonical id *while the plan compiles*, so `execute.rs` and `Outputs`
never learn the new spellings and keep seeing ids only — one place to be wrong
instead of two. `Plan::defaults` merges key by key in `compile()` immediately
before `expand()`, never over a key the operation already gave (an explicit
`null` included), and a non-object `defaults` is refused in `Plan::from_json`
before any mutation. `kam-plan` still does not contain the word `schematic`
(D11): nothing is special-cased, the KiCAD half is one documentation string in
`konnect-core::plan::ops::description()`.

The two new refusals say what to do instead, verbatim:

```
operation 'op2' refers to ${0.schematic} with a bare number, which is ambiguous
between a position and an id; write ${ops[N].field} for position N (zero-based),
or name an id — ids in this plan: op1, op2
operation 'op2' refers to ${place.x} by type 'place', but 2 operations have that
type (op1, op3); name one of them by id instead
```

`kam-plan` 46 → **59 tests**, `konnect-core` 357, gate green.

**Measured on the same 60 attempts — and this is the first run that produced a
grade 3.**

| | after E17 | after E18 |
|---|---|---|
| `invalid_json` | 10 | 7 |
| `compile_failed` | 44 | **30** |
| `not_applied` | 5 | 14 |
| `applied_invalid` | 1 | 5 |
| **`success` (grade 3)** | 0 | **4** |
| stable prefix | 1 549 tk | **1 802 tk** (+253) |

**4 of 60**, i.e. `LLM_CALLS_PER_SUCCESSFUL_TASK ≈ 15` for a one-shot 9B with no
repair loop. That is a poor number and it is the first honest one this project
has: three successes at hint `full`, one at `none`, none at `minimal`; by task,
three on the decoupling bank and one on the LDO.

**The lever was not the one that was documented loudest.** The model used
`defaults` in **0** of 60 attempts and `${ops[N].field}` in **0** — both new,
both written into the description, both ignored. What moved the number is the
unique-op-type reference: `${create.schematic}`, which the model was already
writing before it was legal, now resolves. Documentation the model does not read
buys nothing; accepting what it already writes buys everything. Worth
remembering before the next fix is written as a paragraph.

The bare numeric refusal held as designed and is now the largest single failure
at **17 of 60**. Counted across the raw replies, the two spellings are **`${0.…}`
90 times and `${1.…}` 6 times**, which changes the ambiguity argument: `${0.…}`
has exactly one possible meaning, because auto ids start at `op1` and no
operation is ever named `0`. Only `${1.…}` is genuinely two-headed. That is E19.

Also newly visible, now that plans reach the file system: **9 of 60 invent a
symbol library** (`Resistor_SMD` 7, `Resistor` 2) and **9 of 60 write a
pin-to-net connection** the `connect` operation cannot express —
`{ref1: "R1", pin1: "2", ref2: "+3V3"}`, a pin wired to a rail, which KiCAD does
support through `connect_to_net` and the operation library does not. Some also
write `pin1: 1` as a number where the signature says string. All three are E19.

Local KPIs: TTFT median **161 ms**, **103.0 tok/s**, local input median
**2 209 tk**, output **1 694 tk**, VRAM peak **9 052 MiB**, wall clock median
**16.8 s**.

### E19 — accept what the model already writes, when it has exactly one meaning (2026-08-12) — FIXED

E18's lesson, applied deliberately: the three levers here are all forms the model
writes today and the engine refused, and none of them requires guessing.

**The bare numeric reference resolves by elimination** (17/60). `${N.field}` has
two candidate meanings — the operation at zero-based position N, and an operation
whose id is literally `N` — and each is only a candidate if it exists *and* runs
strictly before the referring operation. Exactly one survivor resolves; two
survivors keep the `AmbiguousNumericReference` refusal; none falls through to the
unchanged unknown/forward/self errors. So `${0.…}` (90 occurrences in the last
run) resolves, because no operation is ever named `0`, and `${1.…}` (6) is
refused only when both readings are genuinely live. D23 is untouched: a `${...}`
in a coordinate is still refused, one level up in `ops.rs::coordinate()`.

**`connect` accepts a pin wired to a rail** (9/60): `{ref1,pin1,net}` explicitly,
`{ref1,pin1,ref2}` with no `pin2` — which is what the model actually writes, and
a `ref2` without a `pin2` cannot mean a pin — and the compact `{from,to}` where a
side containing `.` is `REF.PIN` and a side without one is a net. The decision is
syntactic and documented as such: no document is read to make it, and zero dots
on both sides is still refused. It expands to `batch_connect_to_net` rather than
`connect_to_net`, because the latter wants coordinates that do not exist while a
plan is still compiling.

**A numeric pin is coerced to its decimal string**, reusing the existing
`pin_field()` helper and extended to no other field.

`kam-plan` 59 → **62 tests**, `konnect-core` 357 → **362**, gate green, and
`docs/capability-matrix.md` regenerated rather than hand-edited —
`batch_connect_to_net` moves to `SUPPORTED` on the strength of the new tests.

**One existing test was replaced, and that is a behaviour change rather than a
test fix:** `connect_refuses_a_pin_it_cannot_find` asserted that `"from": "U1"`
is refused, which the new syntactic rule contradicts — a dotless side is now a
net name. It is replaced by tests for both new readings plus the case that is
still an error (neither side carries a dot). Recorded because a deleted
assertion is the cheapest place to hide a regression.

**Measured, and the compile wall fell:**

| | after E18 | after E19 |
|---|---|---|
| `invalid_json` | 7 | 2 |
| `truncated` (new in E20, was inside `invalid_json`) | — | 4 |
| **`compile_failed`** | 30 | **6** |
| `not_applied` | 14 | 29 |
| `applied_invalid` | 5 | 17 |
| `success` (grade 3) | 4 | **4** |
| stable prefix | 1 802 tk | 1 969 tk (+167) |

**46 of 60 plans now compile and reach the file system**, against 24 before, and
grade 3 did not move. The failures moved wholesale from "the plan is not
well-formed" to "the plan is well-formed and the design is wrong", which is the
harder half and the one the project actually wants to be measuring.

Two things now dominate, and only one of them is ours:

* **20 of 60 invent a KiCAD library identifier** — `Resistor` 8,
  `Resistor_SMD` 6, plus `Resistor_Small`, `LinearRegulator`, `Regulator_LDO`,
  `Capacitor_SMD`, `AMS1117-3.3`, `Device_R`, `resistor`, and two invented power
  symbols (`power:#FLG01`, `power:+GND`, where KiCAD has `power:PWR_FLAG` and
  `power:GND`). **This happens at hint level `full` too** — 10 of the 20 — where
  the task text names `Device:R` in the same prompt, two paragraphs above. It is
  not a documentation gap and no wording will close it.
* **15 of 60 apply cleanly and fail the ERC budget** (`erc errors=4 limit=2`,
  `3 limit=0`), plus 2 that lose components outright
  (`missing=['C1','C2'] found=['#PWR001',…,'U1']`). That is design competence,
  measured against KiCAD's own validator — the thing this benchmark exists to
  find out.

`finish_reason` earned its keep immediately: **4 of 60 are `length`**, correctly
labelled `truncated` rather than blamed on the model as malformed JSON, and
median reasoning is **1 070 of 1 797 output tokens** — 60 % of the generation
budget is deliberation the plan never uses.

### E21 — the failure that names candidates, and the candidates that were worse than none (2026-08-12) — FIXED

`Library 'Resistor' not found in the installed KiCAD symbol libraries` named
what was wrong and nothing about what would be right, so neither a human nor a
repair round could act on it without a separate search. The error now carries a
deterministic `candidates` list — capped at 8, deduplicated, computed **only on
the failure path**, and never substituted for what the caller asked for. The
`kind` is unchanged, so anything matching on it keeps working.

**The first build of it was actively misleading, and running it is what showed
that.** Measured live during the repair smoke test:

```
"reason":"Library symbol 'Sensor:R_0805' not found … Did you mean:
          Sensor:MAX30102, Sensor:RPR-0521RS?"
"candidates":["Sensor:MAX30102","Sensor:RPR-0521RS"]
```

`R_0805` is a resistor; `MAX30102` is a pulse oximeter. When the named library
*existed* and only the symbol was missing, the lookup searched inside that
library and never looked outside it — so `Device:R` was unreachable by
construction, and the two symbols that happen to live in `Sensor` were the only
possible answers. A repair round fed those would have followed them: worse than
the bare message it replaced.

Fixed in two parts. The search is now three capped passes that run whether or
not the library exists — an exact case-insensitive symbol-name match across
**all** installed libraries first, then a fuzzy match ranked **globally** rather
than within one library, then library-name proximity only when the named library
does not exist. And the similarity floor was too permissive to reject nonsense:
`ceil(m·2/3)` let `R_0805`→`MAX30102` through at distance 6, so it is now
`ceil(m/2)` — at least half the characters must actually agree — checked against
the existing shortcuts (`cp`→`C_Polarized`, `r_pot_trim`→`R_Potentiometer_Trim`)
which match at distance 1 and are unaffected. `Sensor:R_0805` now returns exactly
`["Device:R"]`, and nothing plausible returns an omitted field rather than a
filler.

A pre-existing test race was found and fixed on the way: two tests set
`KICAD10_SYMBOL_DIR` without sharing a lock, which could have failed an already
green test at random. The lock is now shared.

`konnect-schematic-editor` 17 → **21 tests**, `konnect-core` 361 → **364**, gate
green. Golden suite re-run after E17–E21: **6/6, 2 183 tk/task, 4 MCP calls,
P50 63 ms** — inside the ±12 noise band, no regression.

### E10 — Pre-existing clippy debt under `--all-targets` (2026-08-10) — DEFERRED

Upstream CI runs `cargo clippy --workspace --locked -- -D warnings` (no
`--all-targets`), so test code was never linted and lib-level
`await_holding_lock` warnings in `sch_components.rs` never fired. Under
`--all-targets` the baseline fails too, so this is inherited, not introduced.
The six literal-bool / length-comparison lints in
`konnect-schematic-editor/tests/integration.rs` are fixed. The
`MutexGuard`-held-across-await instances are real correctness smells and are
deferred to Phase L hardening; `gate.ps1` mirrors the upstream gate so it does
not silently accumulate more.

---

## OPEN QUESTIONS

1. Local model that fits 16 GB VRAM with reliable tool-calling and structured
   output. **Narrowed, not answered** (D31): backend is OpenAI-compatible HTTP
   over LM Studio, with `llama-server` as the escape hatch; `vLLM` is out on
   Windows. Candidates are `Qwen3.5-9B` and `gpt-oss-20b`, both Apache-2.0 with
   documented tool calling. Every VRAM figure is still an unconfirmed estimate
   and BFCL V4 is four months stale, so the choice waits on measurement on this
   machine. There is **no** EDA-specialised open-weight model — the electronics
   competence stays in the deterministic engine and the validators.
2. Can PCB E2E run unattended on Windows, or does `KICAD_API_SOCKET` require a
   live GUI session? Blocks PCB benchmark coverage.
3. Is tool-granular loading enough, or does the compact-gateway design (~7
   stable verbs, catalogue never changes, `CATALOG_TOKENS` → 0) need to land
   before Phase H? Current data says the gateway is the only way past 3 698.

---

## NEXT ACTION (2026-08-12, Phase H)

**Phase H has its first real numbers, and they are not good news for the local
model.** `qwen3.5-9b`, six 60-attempt runs, every defect between the model and
the compiler removed one at a time and measured separately:

```
grade 3 one-shot            4/60      LLM_CALLS_PER_SUCCESSFUL_TASK  15
grade 3 with one repair     2/60      LLM_CALLS_PER_SUCCESSFUL_TASK  59
plans that compile         46/60      (was 6/60 before E17)
plans that apply and fail ERC  24/60
```

The ladder was walked from the bottom and every rung was ours until the last
one: item shapes (E14), a plan that lied about applying (E15), two placeholder
notations (E16), scalars documented without a type and no way to create a project
(E17), reference spellings the engine refused (E18/E19), a token cap counted as a
model error (E20), a symbol error with no candidates (E21). Fixing all of them
took plans-that-compile from 6/60 to 46/60 and left grade 3 at 4/60, because the
remaining failure is **ERC correctness**, which no error message and no schema
teaches.

Three next actions, in order of what the evidence supports:

1. **Measure a second model before concluding anything about size.** `gpt-oss-20b`
   is the D31 shortlist's other candidate and its download was interrupted by
   E13. The whole point of the harness is that it grades by compiling, so a
   second model costs one run and no new code. Nothing about the router should be
   built until at least two models have a number.
2. **Discard a repair that makes the design worse.** D35's measurement: 11 of 58
   repairs lowered the grade and were kept anyway. The semantic diff, the ERC
   verdict and rollback already exist; keeping the better of two attempts is a
   deterministic decision this repo can make without a model.
3. **Do not build the escalation router on one model's numbers.** The router is
   the next TODO item, and its thresholds would currently be fitted to a single
   9B on four tasks.

Still open and unchanged: **E7**'s underlying disagreement between the in-process
connectivity analysis and `kicad-cli` (disclosed everywhere, Phase J to fix),
**E10**'s `MutexGuard` held across `await` (Phase L), the anti-drift test that
checks examples rather than parsing signatures (corrected in the E14 record, not
yet strengthened), and the strict-JSON comparison that deserves a re-run now that
`finish_reason` can state the mechanism instead of inferring it.

## NEXT ACTION (superseded — kept for the record)

**Phases E and G are closed.** A change can be described once rather than
enumerated, refused before it starts if it cannot finish, expanded
deterministically, run as one transaction, proved against KiCAD's own ERC, and
questioned afterwards through an indexed graph instead of a document dump.

**E6 is closed** and took nine undiscovered instances of itself with it. **E7's
disclosure is closed** — its underlying disagreement with `kicad-cli` is now a
Phase J scope question rather than a defect an agent can be misled by. No open
defect blocks Phase H.

**Phase H is the next action**, and its precondition set is empty. Everything it needs to be
measured against exists and none of it has a local consumer yet: the plan is
written by hand rather than by a model, the ACTIVE TASK anchor is exercised only
through an MCP reply, the graph is queried by a probe, and the capability matrix
says which 107 unproved tools a model should not be handed. The question H
answers — does a small local model write a *valid* plan, and how often — is the
one that decides whether any of this reduces
`LLM_CALLS_PER_SUCCESSFUL_TASK` in practice. That metric is still unmeasured and
still not claimed.

Open defects: **E6 and E8 are closed.** **E7** is three-quarters closed — the
evidence path uses `kicad-cli` and the matrix labels the internal tools
advisory, but their own descriptions still do not. **E10** (`MutexGuard` held
across `await` in `sch_components.rs`) is still deferred to Phase L, and is a
real correctness smell rather than a lint preference.

Two limitations the current evidence inherits and does not hide:

* The diff reports **objects**, not **connectivity**. `VDD3V3 connections: +2`
  is real on a `.kicad_pcb`, where pads name their net in the file. A schematic
  has no netlist in the document, so deriving one would mean re-implementing
  the connectivity that E7 shows already disagrees with `kicad-cli`.
* `verify` only checks documents the batch **changed**. A read-only batch gets
  no verdict, and a caller wanting a bare check still calls `run_erc`. Whether
  that is the right boundary is an open question, not a settled one.

Three the plan path adds, recorded rather than hidden:

* **A plan operation cannot take a coordinate from a previous step.** It is
  refused, not passed through, because snapping happens at compile time and a
  guarantee that holds *usually* is the bug E6 already is (D23). `call` is the
  way out, and it does no arithmetic.
* **The macro library is small and mechanical.** Seven operations, of which one
  (`decouple`) is a real circuit macro. Whether that is the right seven is
  unmeasured: no model has been asked to write a plan yet, so the evidence for
  which operations matter does not exist.
* **`LLM_CALLS_PER_SUCCESSFUL_TASK` is not measured and is not claimed.** What
  is measured is that a nine-call sequence fits in one operation and that the
  payload is between a half and a third of the size. That is the mechanism, not
  the effect.
