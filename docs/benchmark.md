# Benchmark

Everything in this document is measured on this machine with the harness in
`bench/`. Nothing here is estimated. Where a target was not reached, it says so.

**Machine** — Windows 11 Pro 26200, AMD Ryzen 7 9800X3D (8C/16T), 32 GiB RAM,
RTX 5080 (16 303 MiB VRAM), KiCad 10.0.3 per-user install, protoc 35.1,
rustc 1.96.0 (pinned).

**Baseline** — `mixelpixx/Konnect` v0.2.2 at commit `5cd6454`, unmodified.

---

## What is measured, and why it is measured this way

Three numbers matter and only one of them is obvious:

* `RESPONSE_TOKENS` — the text tool results push into the caller's context.
* `CATALOG_TOKENS` — `tools/list` payloads the client re-fetches because the
  server sent `notifications/tools/list_changed`. **The caller cannot decline
  this.** A harness that ignores the notification would be broken, so the cost
  is real even though no tool call appears to have caused it.
* `EXTERNAL_TOKENS` = the two added together. This is what a harness actually
  eats per task, and it is the number the project is judged on.

Early runs of this harness undercounted by 8 000 tokens per task because the
bench client ignored `list_changed`. It now re-fetches like a real client does
(`bench/mcp_client.py`, `auto_refresh_tools`).

Success is never judged from a model's prose. Assertions run KiCad's own ERC
through `kicad-cli`, or read the design back through the server's query tools.
A task passes only if every step succeeded *and* every assertion held.

## Golden tasks

Six tasks in `bench/tasks/`, each starting from an empty temp directory:

| id | category | what it exercises |
|---|---|---|
| `sch_divider` | schematic_simple | placement, power symbols, pin-to-pin wiring, net label, ERC=0 |
| `sch_ldo` | schematic_simple | AMS1117 + input/output caps, batched wires and junctions, ERC=0 |
| `sch_template_stm32` | schematic_complex | reference-circuit template instantiation |
| `sch_hierarchy` | schematic_complex | hierarchical sheet, sheet pins, sheet duplication, page renumbering |
| `manufacturing_exports` | manufacturing | BOM, netlist, schematic SVG through `kicad-cli` |
| `recovery` | recovery | five wrong inputs must each fail loudly, then the session must still build a correct ERC-clean design |

Coordinates in the task specs are exact 1.27 mm grid multiples and the pin
offsets are the ones `get_symbol_info` actually reports, so a run is
reproducible rather than approximately right.

### Load modes

The same six tasks run under four loading strategies:

* **`toolsets`** — `list_toolboxes` then `load_toolset([...])`. What upstream
  Konnect offers, and what its skills tell an agent to do.
* **`tools`** — `load_tools([exact names])`. Fine-grained loading with oracle
  knowledge of the tool names. The floor for anything that still goes through
  `tools/list`.
* **`search`** — `find_capabilities(intent)` on the task's plain-language
  intents, load whatever comes back. No oracle. Scores retrieval, not the
  server.
* **`gateway`** — `kicad_describe([exact names])` then one batched
  `kicad_invoke`. Same oracle as `tools`, so the comparison isolates *how the
  schemas arrive* and nothing else. Assertions run through the gateway too: a
  `run_erc` called directly would be a tool the gateway never had to expose,
  and the number would be a lie.

`toolsets`, `tools` and `gateway` all get oracle knowledge, so differences
between them are loading mechanics, never search quality.

---

## Results

### MCP surface cost (`bench/surface.py`, tiktoken `o200k_base`)

| | tools | tokens |
|---|---|---|
| baseline `tools/list` at startup — Konnect v0.2.2 | 19 | 1 680 |
| baseline `tools/list` at startup — fork, step 1 | 21 | 1 958 |
| baseline `tools/list` at startup — fork, step 2 | 16 | **1 454** |
| baseline `tools/list` at startup — fork, gateway | 18 | 1 725 |
| baseline `tools/list` at startup — fork, Phase D | 18 | 1 912 |
| baseline `tools/list` at startup — fork, Phase E | 18 | 1 952 |
| baseline `tools/list` at startup — fork, Phase E + validators | 18 | 1 998 |
| full catalogue, all 18 toolsets loaded | 193 / 195 / 197 | 22 329 / 22 190 / 22 648 |

Step 1 made the startup surface **278 tokens larger** — the price of the two new
meta-tools (`find_capabilities`, `load_tools`) — and that regression is what
bought the per-task reduction below. Step 2 paid it back and more: **1 958 →
1 454**, which is also 13.5 % under upstream's own 1 680 with the meta-tools
still in place.

Heaviest single tools before compression: `create_symbol` **1 448 tk**,
`create_footprint` 530, `add_hierarchical_sheet` 318. `create_symbol` alone was
6.4 % of the whole catalogue.

### Step 2 — where the 504 startup tokens came from

Two changes, both measured with `bench/surface.py`:

1. **Schema compression on the three heaviest tools.** `create_symbol`
   **1 448 → 1 077** (−25.6 %), `add_hierarchical_sheet` 318 → 285,
   `create_footprint` 530 → 519. No property, enum value or default was
   removed. The win is structural: `create_symbol` inlines the same pin-item
   object **three times** (`pins`, `units[].pins`, `power_pins`), so every word
   in it was billed three times. Dropping the two self-evident per-field
   descriptions there (the `type` and `style` enums document themselves) and
   stating the one fact they carried — NC is spelled `no_connect` — once in the
   tool description took the pin item from 219 to 118 tokens per copy.
   `create_footprint`'s prose value lists (`"'smd', 'thru_hole',
   'np_thru_hole'"`) became real `enum`s: nearly the same token count, but now
   validated, and `shape` gained the two KiCad values the prose had omitted.
   Catalogue effect: 22 799 → 22 384 (−1.8 %). It does **not** move the golden
   suite, because no golden task loads the `library` toolset — stated here
   rather than quietly folded into the headline.
2. **`config` left the starter kit.** It was 7 tools and **625 tokens** in every
   single `tools/list`, and the golden suite calls **zero** of them. The two
   read paths a session actually opens with (`load_user_config`,
   `get_effective_config`, 118 tk) are now admitted individually through a new
   `STARTER_TOOLS` list; the five write / design-rule tools cost 507 tokens per
   refresh and are one `find_capabilities` call away. Verified before shipping:
   `find_capabilities` ranks the removed tools **first** on their own intents —
   "remember that I always use JLCPCB" → `save_user_config`, "add a design rule
   for decoupling" → `add_design_rule`, "list my design rules" →
   `list_design_rules`. Discoverability is preserved; only the permanent cost is
   gone.

### Golden suite

18 runs (6 tasks × 3 repeats) per column, except `search` (1 repeat, deterministic).

| Metric | Konnect baseline | Fork `toolsets` | Fork `tools` | Fork `search` | Fork `gateway` |
|---|---|---|---|---|---|
| SUCCESS_RATE | 18/18 (100 %) | 18/18 (100 %) | 18/18 (100 %) | 6/6 (100 %) | **18/18 (100 %)** |
| MCP_CALLS median/task | 11 | 11 | 10 | 16 | **4** |
| WALL_CLOCK_P50 (ms) | 70 | 71 | 64 | 69 | 72 |
| WALL_CLOCK_P95 (ms) | 888 | 902 | 1 183 | 333 | 916 |
| RESPONSE_TOKENS/task | 3 984 | 2 056 | 964 | 2 765 | 1 995 |
| CATALOG_TOKENS/task | 8 389 | 8 163 | 2 281 | 7 103 | **0** |
| **EXTERNAL_TOKENS/task** | **12 373** | 10 220 | 3 197 | 9 868 | **1 995** |
| RETRY_RATE | 0 | 0 | 0 | 0 | 0 |
| ROLLBACK_RATE | 0 | 0 | 0 | 0 | 0 |

**Headline: 12 373 → 1 995 external tokens per task, −83.9 %, with success rate
unchanged and MCP calls down from 11 to 4.**

Both V1 surface targets are met in the `gateway` column: `EXTERNAL_TOKENS/task`
≤ 2 000 and median `MCP_CALLS` ≤ 5.

That column is the Phase F measurement, kept as the reference point for the
surface work. Phase D then added the transaction guarantees and moved it to
2 033 — 33 tokens over the target, for reasons and numbers in "Phase D" below.

### The gateway — where the last 1 200 tokens went

`kicad_describe` + `kicad_invoke` (`crates/konnect-core/src/router/meta_tools.rs`)
call any registered tool without exposing it. Two costs vanish:

* **`CATALOG_TOKENS` → 0.** Nothing is added to `tools/list`, so no
  `notifications/tools/list_changed` fires and the client never re-fetches. In
  `tools` mode that refresh was 2 281 tokens per task — more than twice the
  964 tokens of actual tool output it accompanied. A stdio test asserts the
  catalogue is byte-identical before and after a `kicad_invoke` that runs an
  unloaded tool, so this cannot silently regress.
* **Round trips collapse.** A task's whole scripted path is one batched call:
  median MCP calls 10 → **4** (describe, the batch, and the assertions' own
  reads — which also go through the gateway, so nothing is measured through a
  cheaper door than the one being sold).

The trade is stated rather than hidden: `RESPONSE_TOKENS` rises from 964 to
1 995 because the schemas now arrive inside a `kicad_describe` result instead of
a catalogue refresh. That is the point — the caller asked for exactly those
schemas, once, instead of being handed the whole list including tools it already
had. Net per task: −1 202 tokens.

The two new meta-tools cost 271 tokens on the startup surface (1 454 → 1 725),
paid **once per session** rather than per task. Upstream's own startup is 1 680,
so the fork now sits 45 tokens above it while carrying four extra meta-tools.

Batching does not buy anonymity: every inner call is recorded with the shared
observer under its own tool name, asserted by a test that reads
`get_recent_calls` after a batch.

Progression across the three steps:

| | baseline | step 1 (`tools`) | step 2 (`tools`) | step 3 (`gateway`) |
|---|---|---|---|---|
| EXTERNAL_TOKENS/task | 12 373 | 3 698 | 3 197 | **1 995** |
| CATALOG_TOKENS/task | 8 389 | 2 785 | 2 281 | **0** |
| MCP_CALLS median/task | 11 | 10 | 10 | **4** |
| `tools/list` at startup | 1 680 | 1 958 | 1 454 | 1 725 |

Step 2 moves every load mode, because the startup surface is re-sent in every
catalogue refresh regardless of how tools are loaded: `toolsets` 10 725 →
10 220, `search` 10 394 → 9 868. Step 3 makes that irrelevant for callers that
use the gateway, and adds 271 tokens to startup for callers that do not.

P95 in the `tools` column moved 900 → 1 183 ms. That is `kicad-cli` spawn
variance on `manufacturing_exports`, not server work: P50 went *down* (72 → 64
ms) and no code on that path changed. It is recorded rather than smoothed.

P95 is dominated by `run_erc`, which spawns `kicad-cli` (1 086 ms mean). Nothing
in this work touched that, and nothing should: it is KiCad doing real work.

### Phase D — what the transaction guarantees cost

`kicad_invoke` became a transaction: `base_revisions` preconditions,
`operation_id` idempotency, and a directory snapshot restored when a batch
fails. Measured on the same 18 runs, `gateway` mode:

| Metric | Phase F gateway | Phase D gateway | Δ |
|---|---|---|---|
| SUCCESS_RATE | 18/18 | 18/18 | = |
| EXTERNAL_TOKENS/task | 1 995 | **2 033** | +38 (+1.9 %) |
| CATALOG_TOKENS/task | 0 | 0 | = |
| MCP_CALLS median/task | 4 | 4 | = |
| WALL_CLOCK_P50 (ms) | 72 | 67 | −5 |
| WALL_CLOCK_P95 (ms) | 916 | 911 | ≈ |
| `tools/list` at startup | 1 725 | 1 912 | +187, once/session |
| tests | 487 | **525** | +38 |

The +38 tokens per task are the `revisions` map the reply now returns for each
changed document — the input a caller needs to send back as `base_revisions`.
Paths are reported once as `revisions_root` plus basenames rather than as full
absolute paths, which is what keeps it at 38 rather than roughly 110.

The +187 startup tokens are `kicad_invoke`'s schema growing from 210 to 337
tokens for five new properties. That is a once-per-session cost and it moves the
startup number **away** from its ≤ ~1 000 target, which was already missed; it
is recorded as a regression rather than folded into the per-task headline.

Snapshot capture costs nothing measurable here: P50 went down, not up, because
the golden projects are 3–5 files of a few kilobytes each and the read is
dwarfed by the tool work it protects.

**One behavioural finding, from the benchmark rather than from review.** With
`atomic` defaulting to `true`, the `recovery` task scored **0/3**: it
deliberately fails five calls mid-batch with `stop_on_error: false` and then
asserts the design the remaining calls build, and an unconditional rollback
threw that away. `atomic` now defaults to `stop_on_error` — a caller who says
the calls are independent has said the survivors are wanted. A stdio test pins
the coupling so it cannot regress silently.

### Phase E — what the semantic diff costs

A batch used to answer "3 files changed". It now answers `symbol +2, wire +1` —
the domain diff between the before-image the snapshot already held and what is
on disk after. Same 18 runs, `gateway` mode:

| Metric | Phase D gateway | Phase E gateway | Δ |
|---|---|---|---|
| SUCCESS_RATE | 18/18 | 18/18 | = |
| EXTERNAL_TOKENS/task | 2 033 | **2 158** | +125 (+6.1 %) |
| CATALOG_TOKENS/task | 0 | 0 | = |
| MCP_CALLS median/task | 4 | 4 | = |
| WALL_CLOCK_P50 (ms) | 67 | 64 | −3 |
| WALL_CLOCK_P95 (ms) | 911 | 870 | −41 |
| `tools/list` at startup | 1 912 | 1 952 | +40, once/session |
| tests | 525 | **567** | +42 |

The +125 splits as **+40 once per session** (the `diff` property on
`kicad_invoke`'s schema) and **~85 per task** across the batches that actually
change something — one `"diff":{"summary":"…"}` line per mutating batch.

The target of ≤ 2 000 external tokens per task is now missed by 158 and is
recorded as missed. The trade is deliberate and is the one the architecture
asks for: a batch that reports `done=true` cannot be reviewed, and the
alternative — the harness re-reading the documents to find out what happened —
costs far more than 85 tokens. `diff: "none"` turns it off for a caller who
disagrees, and `diff: "changes"` buys a line per item for one that wants the
detail.

Extraction cost is not visible in the wall clock: P50 and P95 both went down,
because the parse happens on files already in the page cache from the snapshot
that was taken anyway.

**One finding from the probe rather than from review.** The first build
reported `create_project` as `no design change`: three files appeared, and
their *contents* — an empty schematic, an empty board — differ in no item. The
batch that changes the most was being described as the one that changed
nothing. Documents are now items in their own right, so a project creation
reads as `document +3`. `bench/probes/semantic_diff.yaml` is what surfaced it;
the same shape is now pinned by a stdio test.

### Phase E — evidence handles and validators

Two additions on top of the semantic diff. The item-by-item detail moved behind
a handle (`kicad://diff/N`, served over MCP `resources/read`), and `verify:
"auto"` runs KiCAD's own ERC/DRC on every document a batch changed.

| Metric | Phase E diff | + handles | + validators | + task state | Δ vs Phase D |
|---|---|---|---|---|---|
| SUCCESS_RATE | 18/18 | 18/18 | 18/18 | **18/18** | = |
| EXTERNAL_TOKENS/task | 2 158 | 2 172 | 2 174 | **2 175** | +142 |
| CATALOG_TOKENS/task | 0 | 0 | 0 | **0** | = |
| MCP_CALLS median/task | 4 | 4 | 4 | **4** | = |
| WALL_CLOCK_P50 (ms) | 64 | 61 | 73 | 68 | ≈ |
| WALL_CLOCK_P95 (ms) | 870 | 886 | 885 | 854 | ≈ |
| `tools/list` at startup | 1 952 | 1 952 | 1 998 | **2 034** | +122, once/session |
| full catalogue | 22 688 | 22 688 | 22 734 | **23 411** | +763 |
| tests | 567 | 575 | 588 | **606** | +81 |

The handle costs **+14 tokens/task** — the URI itself, on mutating batches — and
nothing at startup. What it buys is that `diff: "changes"`'s `"... 25 more"` now
has somewhere to point, and that the pack behind it can grow without the reply
growing.

`verify` costs **+46 startup tokens** (one schema property) and nothing per task
on this suite, which does not use it. Its real cost is latency, and it is
measured rather than estimated: `bench/probes/validators.yaml` shows the same
batch at **7 ms without verification and ~1 100 ms with it**. That is the whole
argument for `verify` being opt-in — paying a second on every placement to make
the occasional checkpoint cheaper is the wrong trade.

The probe also shows the baseline cache working end to end:

```
[1] verify: auto   erc errors 4   baseline "unknown"     1120 ms
[2] verify: auto   erc errors 2   fixed 2                1093 ms
```

The second batch's baseline is the verdict the first one cached against that
document's revision, so the delta comes for free rather than from a second ERC
run. `fixed: 2` is two finding *ids* that disappeared, not a count that fell —
the distinction matters because two fixed and two introduced also moves a count
from 4 to 4.

### Phase E — what the Task State Manager costs

Four tools (`start_task`, `update_task`, `get_task`, `list_tasks`) and one
`task_id` argument on `kicad_invoke`. The interesting number is where the cost
did **not** land:

| | tokens |
|---|---|
| the four task tools, at startup | **0** |
| the four task tools, in the full catalogue | 677 |
| `task_id` on `kicad_invoke`'s schema, at startup | **36** |

Registering them as a toolset rather than as gateway verbs is what buys that.
A client that never opens a task pays 36 tokens once; one that does reaches
them through `kicad_invoke` with no catalogue refresh, which is the case the
gateway exists for. A stdio test asserts none of the four appear in the startup
catalogue *and* that they are callable anyway, so the property cannot regress
into a convenience.

Per task the suite is unchanged (2 175, within noise of 2 174) because the
golden tasks do not open a task. A batch that does pays the anchor — roughly 40
tokens — on each reply.

Running total against the targets: external tokens per task are **175 over** the
≤ 2 000 target, and startup is **1 034 over** its ≤ ~1 000 target. Both stay
recorded as missed. The 175 buys "mutations without an audit record: 0" and the
handle that keeps the audit affordable; the startup figure is a once-per-session
cost against a per-task saving of ~10 000.

### Phase G — what a plan costs against the batch it replaces

The golden suite measures the server. It cannot measure a plan, because it is a
scripted oracle: it already knows the exact calls, so it can never pay for not
knowing them. `bench/plan_cost.py` measures the claim directly instead — the
same design built twice by a fresh server, once as an enumerated batch and once
as a plan, with `verify: "auto"` on both, and the run is void unless the two
produce the same semantic diff and the same ERC verdict.

Both shapes are given **the same pre-snapped coordinates**. The plan would also
accept the round numbers a person types and snap them, where the batch would
write them verbatim and fail ERC (E6); that advantage is real and deliberately
kept out of these numbers.

Median of 3, tiktoken `o200k_base`, excluding the handshake and the startup
catalogue, which are identical in both shapes:

| | divider batch | divider plan | | bank batch | bank plan |
|---|---|---|---|---|---|
| MCP calls | 2 | 2 | | 2 | 2 |
| request tokens | 517 | **470** (−9.1 %) | | 767 | **236** (−69.2 %) |
| response tokens | 1 663 | **654** (−60.7 %) | | 1 498 | **646** (−56.9 %) |
| external tokens | 2 180 | **1 124** (−48.4 %) | | 2 265 | **882** (−61.1 %) |
| wall clock | 2 325 ms | 2 282 ms | | 2 311 ms | 2 338 ms |
| ERC errors | 0 | 0 | | 2 | 2 |
| semantic diff | identical | identical | | identical | identical |

Attributed, because "half the tokens" is not a claim until it says which half
paid (first run of each):

| | schemas req/resp | the change itself req/resp |
|---|---|---|
| divider, batch | 35 / 814 | 476 / 841 |
| divider, plan | 17 / **393** | 453 / **262** |
| bank, batch | 25 / 475 | 732 / 1 022 |
| bank, plan | 17 / **393** | **225** / **256** |

Two different savings, and they are worth separating:

* **The divider's saving is structural.** Five tool schemas become one (814 →
  393) and six per-call results become one execution summary (841 → 262). The
  request barely moves — 476 → 453 — because every coordinate in a divider is
  data the caller chose, and a plan does not compress data. That null result is
  the honest half of this table.
* **The bank's saving is the macro.** One `decouple` operation replaces nine
  calls, and the eight power-symbol positions it computes are eight positions
  the caller never writes down: 732 → 225 request tokens, −69.2 %. This is
  where a plan stops being a nicer wrapper and starts removing work.

The bank is a fragment rather than a finished design — a rail with no source is
not driven, so both shapes report ERC 2. They report the *same* 2 on the same
twelve symbols, which is what makes the comparison like for like; the script
voids the run if they ever diverge.

Two things this does **not** measure, and neither should be read into it:

* **`LLM_CALLS_PER_SUCCESSFUL_TASK` is still unmeasured.** There is no model in
  this loop. What is measured is that the payload a model would have to emit and
  read is between a half and a third of the size, and that a nine-call sequence
  can be emitted as one operation — the mechanism by which the call count would
  fall, not the fall itself. That number needs Phase H.
* **Retrieval precision (22.4 %) is untouched.** A plan names its own
  capabilities instead of searching for them per step, which is why it was
  expected to help; but a caller still has to find `apply_plan` once. The
  measurement of that belongs with the capability matrix.

Per-task cost on the golden suite is **2 171** (was 2 175 — within noise; the
suite never opens a plan), startup `tools/list` is **2 034 — unchanged**, and
the full catalogue grew 23 411 → 24 082 for the two plan tools. That the startup
number did not move is the design working: the `plan` toolset costs nothing
until it is used, and a stdio test asserts both halves of that.

One number in this document was stale and is corrected rather than quietly
updated: the intermediate `tools` load mode now measures **3 770** tokens per
task, not the 3 197 recorded at Phase F. The drift is Phase D/E's meta-tool
growth being re-sent on each `tools/list` refresh (+580 startup × one refresh),
not this change — the golden suite never loads the plan toolset, which
`bench/results/fork-phaseG-plan-tools.json` shows directly.

### Plan-owned postconditions — what a promise costs

`apply_plan` now runs the plan's own `validators` list. Same divider, three
declarations, measured on a real project through the release binary:

| plan | wall clock | reply |
|---|---|---|
| no `validators` | **48 ms** | `{"ok":5,"ops":4,"steps":5}` |
| `validators: ["erc_clean"]` | **1 114 ms** | byte-identical |
| `validators: ["erc"]` | **2 182 ms** | byte-identical |

Three things this table says, in order of how easy they are to get wrong:

* **A passing postcondition costs no tokens.** The reply is the same 46 bytes
  with and without it. The cost is entirely latency, which is where D17 already
  put `verify`'s cost, and for the same reason.
* **`erc_clean` is one `kicad-cli` run; `erc` is two.** `erc_clean` is absolute —
  zero errors — so it needs no baseline. `erc` means "this plan introduced
  nothing", which cannot be known without a verdict on the state the plan
  started from; when the cache has none, one is computed *before* the first
  mutation. That is the whole 1 068 ms difference.
* **Nothing is paid when nothing is declared.** 48 ms is the plan without a
  single hash or spawn, and a unit test pins that down by giving the context a
  `kicad-cli` path that cannot exist: if the empty-`validators` fast path ever
  regressed into computing a baseline, the test would fail loudly instead of
  quietly getting slower.

The failure path is what the feature is for: a postcondition that fails returns
`error_kind: "postcondition_failed"` naming the check, the document, the counts
and the introduced finding ids, and `is_error` on that reply is what makes the
enclosing atomic `kicad_invoke` roll the whole plan back. A validator that could
not run at all is a failure too, never zero findings — E4 is the reason.

### E8 — a taxonomy fix that only one load mode can see

`export_bom` reads a `.kicad_sch` and nothing else, and was registered in the
`pcb_export` toolset. It now lives in `sch_export`, next to the other schematic
exports. The capability matrix is what forced the issue: the tool published as
`PARTIAL` with the misplacement as its stated limitation, so the table carried
the defect on every render until it was fixed.

The cost of the defect is only visible where a client loads whole toolsets,
which is exactly what every shipped skill does:

| `toolsets` mode, catalogue tokens/task | before | after |
|---|---|---|
| `manufacturing_exports` | 9 920 | **8 880** |
| the five other tasks (schematic-only) | 8 163 | 8 880 |

Read the columns down, not across: the *before* numbers are from the step-2
build and the five peers grew +717 since then from meta-tool growth, so the
comparable statement is the premium. The manufacturing task used to pay
**+1 757 catalogue tokens** over its peers for a schematic export — thirteen PCB
tool schemas re-sent on the refresh — and now pays exactly what they pay.
`bench/tasks/05_manufacturing_exports.yaml` lost the `pcb_export` entry from its
toolset list, and with it the comment explaining why a schematic task loaded a
PCB toolset.

In `gateway` mode the fix is worth nothing and that is expected: the gateway
never refreshes the catalogue, so a tool's toolset is not something the harness
pays for. The gateway column moved 2 171 → **2 178** tokens per task, which is
inside the ±12 spread of repeated runs on a single build (`sch_hierarchy` alone
measures 2 195 / 2 200 / 2 208 in the same three-run set). No saving is claimed
there.

What the fix does buy on every path is one fewer failed call: an agent holding
every schematic toolset previously got `toolset_not_loaded` from `export_bom`
and paid a `load_toolset` round trip to recover.

### ProjectGraph — a query that had to be made cheaper than the dump it replaces

The three `graph_*` tools are a toolset, so they cost **0** startup tokens
(`the_graph_toolset_costs_nothing_until_it_is_used` asserts both halves: absent
from the startup catalogue, callable through `kicad_invoke` without a refresh),
and the golden suite never calls them — 18/18 at 2 174 tk/task against 2 178,
inside the noise band, no saving claimed.

The measurement that mattered was the one the suite cannot make. Against a
six-symbol divider, on the payloads the probe actually returns:

| query | tokens |
|---|---|
| `graph_query kind=symbol`, `fields: full` | 525 |
| `graph_query kind=symbol`, `fields: compact` | **340** |
| `list_schematic_components` — the plain dump of the same six items | 310 |
| `graph_query attrs value=10k`, `fields: compact` — two items | **109** |

The first row is the defect the probe found: a query tool 69 % *more* expensive
than the dump it exists to replace. `fields: compact` — no geometry, no `angle`,
no `unit`, and `kind` omitted per item when the query already pinned it — takes
it to 340.

340 is still 10 % above the dump and stays there. The remainder is the full UUID
key (~23 tk/item), which `graph_neighbors` takes as input; shortening it would
buy tokens by making the graph's one distinguishing capability need a round
trip. The conclusion is written into the tool's own description rather than
hidden: **the graph wins on filtering (109 against 310) and on adjacency, which
no dump answers at all — not on serialising the same items.**

`graph_query`'s schema is **662 tk**, second heaviest in the repository after
`create_symbol`. Free today because it lives in a toolset; worth re-measuring if
it is ever promoted.

### E6 and E7 — two defects, and what closing them cost

**E6** (`add_power_symbol` wrote coordinates verbatim while
`add_schematic_component` snapped to the 1.27 mm grid, so both pins read
`Pin not connected`) is fixed as a class: one `snap_reporting()` helper, one
`SCHEMATIC_GRID_MM`. Searching for the other instances is what it was worth —
**nine further tools had the same bug** and none had ever been observed failing,
including `apply_template`, whose 15 mm column spacing is not a multiple of 1.27
and drifted off-grid from an on-grid origin. Golden suite 18/18 at 2 178,
P50 64 ms: placement changed, the benchmark did not. Proved by a `#[ignore]`d
e2e that rebuilds the original failing placement and gets 0 errors from real
`kicad-cli sch erc`.

**E7's disclosure** puts the advisory caveat in the `tool!` description of the
fifteen in-process connectivity tools, from the same `MANIFEST` the capability
matrix renders. Measured:

| | before | after |
|---|---|---|
| startup catalogue | 2 034 | **2 034** |
| full catalogue | 25 238 | **25 642** |
| the fifteen advisory tools | — | **+27 tk each** |
| every other tool | — | **+0** |

Golden suite 18/18 at 2 190 against 2 178, with mixed-sign per-task deltas
(`sch_hierarchy` +5, `manufacturing_exports` −12, `recovery` +6) — the noise
band, not the suffix, which would have moved a single task +27 in one direction.

### Where the baseline's tokens went

From `bench/analyze.py` on the 18-run baseline:

| tool | calls | resp tk | tk/call | % of all response tokens |
|---|---|---|---|---|
| `load_toolset` | 18 | 39 747 | 2 208 | **57.0 %** |
| `list_toolboxes` | 18 | 13 680 | 760 | 19.6 % |
| `list_schematic_components` | 12 | 3 297 | 275 | 4.7 % |
| `batch_place_components` | 12 | 3 246 | 270 | 4.7 % |
| `apply_template` | 3 | 2 763 | 921 | 4.0 % |

**76.6 % of every response token the baseline emitted was the discovery
handshake**, not KiCad output. Adding the catalogue refresh on top, roughly 92 %
of external tokens per task were protocol overhead.

### Two changes produced the reduction

1. **`load_toolset` stopped echoing tool descriptions** (returns bare names).
   Those descriptions were already arriving a second time in the `tools/list`
   refresh the very same call triggers. 3 984 → 2 058 response tokens/task,
   **−48 %**, zero behaviour change.
2. **Tool-granular loading** (`find_capabilities` + `load_tools`). Loading the
   ~12 tools a task calls instead of the ~90 in the five toolsets that contain
   them: 8 667 → 2 785 catalogue tokens/task, **−68 %**.

### Retrieval quality — the part that is not good enough yet

`find_capabilities` is deterministic lexical scoring over tool names and
descriptions plus a small EDA synonym table. Sweeping the per-query result limit
on the golden intents:

| results/query | task success | recall | precision | external tk/task |
|---|---|---|---|---|
| 2 | 1/6 | 81.3 % | 61.5 % | 5 090 |
| 3 | 2/6 | 85.8 % | 44.3 % | 5 918 |
| 4 | 4/6 | 92.1 % | 37.3 % | 7 892 |
| 5 | 4/6 | 94.0 % | 31.8 % | 8 576 |
| **8** | **6/6** | **100 %** | 22.4 % | 10 394 |

Recall reaches 100 % only where precision has fallen to 22 %, so search-driven
loading lands at 10 394 external tokens — barely better than the baseline and
nearly 3× the oracle floor of 3 698. **Lexical retrieval is not competitive with
knowing the answer.** Closing that gap is the job of the Plan IR (a compiled
plan names its own capabilities) and the local agent router, not of a better
regex.

Recorded so it is not retried blindly: **plural stemming was implemented,
measured, and removed.** It looked obviously correct — "symbols" should match
`add_power_symbol` — but it moved recall at 8 results/query from 100 % to
98.2 % (losing `batch_place_components`) and helped nowhere. The comment in
`capability_search.rs` says so at the site.

---

## Reproducing

```powershell
$env:PROTOC = "<path to protoc.exe>"
.\gate.ps1 -Bench                       # fmt + clippy + tests + build + benchmark

# individual runs
python bench\surface.py --server .\target\release\konnect.exe --label mine
python bench\runner.py  --server .\target\release\konnect.exe --label mine --repeat 3 --load-mode gateway
python bench\analyze.py bench\results\latest-tasks.json
python bench\probe.py   --server .\target\release\konnect.exe --script bench\probes\divider.yaml
python bench\plan_cost.py --server .\target\release\konnect.exe --repeat 3
```

`bench/konnect.bench.toml` pins the `kicad-cli` path. Relying on `PATH` made
`run_erc` fail with `Failed to spawn kicad-cli` while the task still reported
"0 ERC errors" on an empty schematic — a benchmark that scores a no-op as a pass
is worse than no benchmark.

To run the baseline for comparison, export `KICAD10_SYMBOL_DIR` and
`KICAD10_FOOTPRINT_DIR` first: upstream cannot find a per-user KiCad install
(see `progress.md` E3), so without them every symbol lookup fails and the
comparison is meaningless.

## Not yet measured

* `LOCAL_*` tokens, `TTFT_LOCAL`, `VRAM_PEAK`, `KV_CACHE_PEAK` — no local model
  runtime exists yet (Phase H).
* `LLM_CALLS_PER_SUCCESSFUL_TASK` — the golden suite is a scripted oracle path
  with no model in the loop. It measures the server, not an agent.
* PCB tasks — they need KiCad running with the IPC API enabled. Only the
  schematic and export paths are covered, and both are file/CLI based.
* `PREFIX_CACHE_HIT_RATE`, `CACHE_HIT_RATE` — no local inference yet.
