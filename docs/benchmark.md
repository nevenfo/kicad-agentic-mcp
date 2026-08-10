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
| baseline `tools/list` at startup — fork, now | 16 | **1 454** |
| full catalogue, all 18 toolsets loaded | 193 / 195 | 22 329 / **22 190** |

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
