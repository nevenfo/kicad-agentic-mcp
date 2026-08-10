# plan.md — KiCad Agentic MCP

Living plan for turning an existing KiCad MCP server into an **agentic control
layer**: many internal capabilities, a small external MCP surface, local agents
that absorb operational work, a deterministic engine that does everything that
does not need generative reasoning, and independent verification against KiCad
itself.

Status keys: `TODO` / `WIP` / `DONE` / `BLOCKED` / `DROPPED`.

---

## Gate 0 — Base selection

### Decision

```
BASE_SELECTED   = mixelpixx/Konnect  (fork at commit 5cd6454, v0.2.2, 2026-08-05)
WHY             = see "Evidence" below
LICENSE         = AGPL-3.0-only (workspace-wide, crates/*/Cargo.toml inherit)
WORKSPACE       = C:\Users\FlowUP\kicad-agentic-mcp\konnect-agentic  (branch agentic/main)
REFERENCE CLONES= C:\Users\FlowUP\kicad-agentic-mcp\_gate0\{konnect,kicad-mcp-pro,kicad-mcp-server-legacy}
```

### Candidates measured

| Repo | License | Language / size | KiCad access | Verdict |
|---|---|---|---|---|
| `mixelpixx/Konnect` | **AGPL-3.0-only** | Rust, 86 files / 43 283 lines + 12 `.proto` | NNG+protobuf IPC (PCB) · nom S-expr engine (schematic) · `kicad-cli` (export/ERC/DRC) | **BASE** |
| `oaslananka/kicad-mcp-pro` v3.30.1 | MIT | Python ≥3.13, 251 src files / 79 642 lines + 382 test files / 72 475 lines | `kicad-cli` · `kipy` IPC · `kicad-sch-api` (3rd-party, corrupts `global_label` on save) | **IDEA + CODE DONOR** |
| `mixelpixx/KiCAD-MCP-Server` | MIT | TS 13 979 + Python 69 914 lines | TypeScript → Python → SWIG `pcbnew` | **ANTI-PATTERN REFERENCE** |

### Evidence for Konnect

Measured locally, not assumed:

* **Builds and passes clean.** `cargo build --release -p konnect` → 81 s cold.
  `cargo test --workspace --lib --tests` → **469 passed, 0 failed, 5 ignored**.
* **Single-process Rust runtime, single binary.** No TS→Python→SWIG hop. The
  legacy server's chain is exactly what Konnect removed; re-introducing it would
  be a regression.
* **Progressive disclosure already exists and works.** `router/registry.rs`
  `STARTER_KIT = ["project", "config"]`; 18 toolsets, 187 domain tools + 6
  meta-tools. Measured with `bench/surface.py`:

  | metric | value |
  |---|---|
  | `tools/list` at startup | 19 tools · **1 680 tokens** · 7 567 B |
  | `tools/list` all toolsets loaded | 193 tools · **22 329 tokens** · 94 748 B |
  | disclosure ratio | **0.075** |

* **Transactions already exist.** `konnect-sexp/src/transaction.rs` (1 131
  lines) + `konnect transaction status|recover|abandon` CLI + atomic writes
  (`writer.rs`, `fs4` locking, scratch-file cleanup test).
* **Observability already exists.** `konnect-core/src/observability.rs` (335
  lines), JSONL call log, `get_recent_calls`, `server_stats`.
* **Registry invariants are test-enforced** (`tool_count` truth, no duplicate
  tool names, ≤20 tools/toolset). Good soil for a capability matrix.
* **Correct KiCad strategy for v10** — confirmed against KiCad sources, see
  "KiCad 10 ground truth" below: PCB over IPC, schematic over S-expressions.
  That is not a shortcut, it is the only thing that works on 10.0.

### What is reused / refactored / not copied

```
WHAT_IS_REUSED      = konnect-sexp (parser/writer/transaction/geometry),
                      konnect-ipc (NNG+prost client, board protos),
                      konnect-schematic-editor (typed schematic model),
                      konnect-core tools/* (187 tools = the capability inventory),
                      transport/{stdio,http}, observability, install/packaging, CI.
WHAT_IS_REFACTORED  = MCP surface (187 tools -> ~7 external verbs + internal capability
                      index), tool dispatch (add Plan IR + deterministic executor between
                      MCP and tools), revision/snapshot/idempotency layer on top of the
                      existing transaction engine, error catalog, evidence/resources,
                      Task State + Context/Attention manager, local agent runtime.
WHAT_IS_NOT_COPIED  = anything from KiCAD-MCP-Server's TS->Python->SWIG chain;
                      kicad-mcp-pro source code verbatim into AGPL crates without an
                      explicit MIT attribution header (MIT -> AGPL is legal one-way, but
                      it must be labelled, so default to clean-room reimplementation).
```

### License impact — recorded, not ignored

* Konnect is **AGPL-3.0-only** and its `COMMERCIAL.md` advertises a separate
  commercial licence. A fork is a derivative work.
* For **personal / local use** there is no distribution, so no AGPL obligation
  is triggered today. Work proceeds unblocked.
* **If distributed or offered as a network service**, the whole fork must ship
  its complete corresponding source under AGPL-3.0.
* MIT code from `kicad-mcp-pro` **may** be absorbed (MIT → AGPL is compatible
  one-way) provided the MIT copyright notice travels with it. The reverse is
  forbidden: nothing AGPL may be pushed back into an MIT project.
* **Mitigation for a future re-licence**: keep every generic subsystem
  (Task State, Context/Attention manager, Plan IR, local model router, evidence
  store, benchmark harness) in **new crates with no AGPL-derived code**, so they
  can be re-licensed or re-based without rewriting them. Enforced by rule:
  new `kam-*` crates must not `use konnect_*` types that were copied from
  upstream — they depend on traits we define.

### Why not `kicad-mcp-pro` as the base

It is the more *feature-complete* project (380 tools vs 187, generated parity
matrix, 2 852 tests, evals harness) and its ideas are worth more than its code.
It is not the base because:

* **Its schematic writer is a liability.** Writes go through the third-party
  `kicad-sch-api >=0.5.0,<0.6`, which **drops `global_label` nodes on save**.
  The mitigation is a round-trip guard that *refuses the write* and raises
  `SCHEMATIC_WRITE_UNSAFE` — so `sch_modify_property` is permanently `partial`.
  Konnect's own nom-based S-expression engine with atomic writes has no such
  dependency. This is the single strongest argument for a Rust base.
* **Profiles are start-up-time, not runtime.** `PROFILE_CATEGORIES` is chosen
  when the process boots; the server does not emit
  `notifications/tools/list_changed` and cannot change profile hot — switching
  requires reconnecting the MCP client. Konnect's router already loads and
  unloads toolsets live. We need *live* disclosure, not a boot flag.
* **Cold-start latency is a known hot spot.** 251 Python modules force deferred
  background tool registration with a 30 s budget and a `SERVER_INITIALIZING`
  error for requests that arrive too early; `tools/list` can trigger a cached
  IPC capability probe (network I/O on a list call).
* **Three build systems** (`uv`/`hatchling` + `pnpm` + `cargo`/Tauri), 17 direct
  runtime deps including OpenTelemetry. One Rust binary deletes all of it.
* **~4 000 lines of closed-form SI/PI/EMC heuristics** (`analysis` domain sits
  at 23.1 % coverage, all advisory, blocked for release sign-off). Low
  value-per-line; explicitly not ported.

### Ideas adopted from `kicad-mcp-pro` (MIT — reimplemented clean-room)

| # | Idea | Where it lands |
|---|---|---|
| 1 | **Profile × operating mode as orthogonal axes** — profile controls *discovery*, mode (`READONLY`/`WRITE`/`MANUFACTURING`/`EXPERIMENTAL`) controls *execution risk*. Loading a toolset must not grant permission to mutate. | `kam-state` + gateway |
| 2 | **`TransientClass` on every error** (`none`/`network`/`timeout`/`lock`/`state`) + `retry_after_ms`, instead of a bare `retryable: bool`. `state` means "reconcile first, blind retry is useless". | error catalog (Phase D) |
| 3 | **`stable_finding_id` = truncated SHA-256 of (rule + location)**. A finding keeps its ID across runs, so a fix is proven by diffing IDs, not by re-reading prose. Build it in the finding constructor so it cannot be forgotten. | `kam-evidence` |
| 4 | **Parity matrix with `gui-only-no-api` excluded from the denominator**, plus a test asserting every referenced tool name really exists. Separates "we didn't" from "KiCad can't". | `docs/capability-matrix.md` (generated) |
| 5 | **Serialised IPC command queue with idempotency keys** — the lock matters less than the guarantee that a retry never double-applies. Natural fit for an `mpsc` + worker task. | Phase D |
| 6 | **Append-only JSONL run journal** with `pre_snapshot_path` / `post_snapshot_path` / `rollback_token` per entry. Buys replay, rollback and eval material for ~170 lines. | `kam-evidence` |
| 7 | **Content-addressed plan + `plan → preview → apply → verify → rollback`** instead of a per-tool `dry_run: bool`. | `kam-plan` (Plan IR) |
| 8 | **Committed evidence snapshot of catalogue token cost, CI-gated.** Turns "we think it's lighter" into a failing test. | `bench/surface.py` + snapshot test |
| 9 | **Adapter matrix**: for each capability, which concrete backend actually runs (`ipc` / `cli` / `sexpr-file`). Makes fallbacks observable instead of implicit. | generated doc |
| 10 | **`FailureMode` on verdicts** (`design` / `environment` / `configuration` / `manual_review`) + a `MANUAL_STEP_REQUIRED` error that names the exact GUI step. A broken env and a broken design must drive opposite agent loops. | error catalog + evidence |

Their eval design is also worth copying: cases carry `expected_tools`,
`allowed_tools`, `forbidden_tools`, a `safety` tier checked against the
capability registry (so a `read_only` case rejects *any* write tool, not just
listed ones), `max_calls`, and an **instability rate** across repeated runs.
Release thresholds: `min_pass_rate 0.95`, `max_safety_violations 0`,
`max_unnecessary_call_rate 0.05`, `max_instability_rate 0.05`.

Reference numbers to beat (their `docs/evidence/progressive-disclosure-profile-snapshot.json`):
`expert` profile 380 tools ≈ **54 719 tokens**; `default` 24 tools ≈ 2 344 tokens.
Konnect baseline is already better at 1 680 / 22 329.

---

## KiCad 10 ground truth (verified against KiCad sources, 2026-08-10)

Installed here: **KiCad 10.0.3**, `C:\Users\FlowUP\AppData\Local\Programs\KiCad\10.0\bin\kicad-cli.exe`.

| Fact | Consequence for us |
|---|---|
| IPC = NNG REQ/REP + protobuf `ApiRequest`/`ApiResponse` envelope, `kicad_token` header | Konnect's `konnect-ipc` design is correct |
| Socket via `KICAD_API_SOCKET`, token via `KICAD_API_TOKEN`; API **disabled by default** (Preferences → Plugins → "Enable KiCad API") | `doctor` command must check this and say so |
| **No protocol version number**; only `GetVersion` → `{major,minor,patch}` | Capability probing must be behavioural, not version-string-based |
| **No async events / no pub-sub.** Only `KINNG_REQUEST_SERVER` | Event journal must be **ours**: internal revisions + targeted diffing + file watching. Do not promise push notifications |
| Server is single-threaded and runs on the UI thread | Serialize IPC access, own the timeout/retry policy, expect `AS_BUSY` |
| `BeginCommit` / `EndCommit(id, action)` exist on `API_HANDLER_EDITOR` (PCB **and** SCH) | Real transaction primitive to build atomicity on |
| **PCB coverage is complete** (stackup r/o, layers, nets, `GetConnectedItems`, `GetItemsByNetClass`, `RefillZones`, `HitTest`, `InteractiveMoveItems`, DRC injection, …) | PCB path = IPC |
| **Schematic IPC is effectively empty on 10.0**: `schematic_commands.proto` has no commands; `api_handler_sch.cpp` registers only `GetOpenDocuments`; `getItemFromDocument()` returns `std::nullopt` (TODO) | Schematic path = S-expression engine. This is a KiCad limitation, not a Konnect one. **Feeds the Custom-KiCad gate.** |
| `UpdateBoardStackup` declared but not implemented on 10.0 | Capability matrix entry: `GAP` |
| `kicad-python` 0.7.1 (KiCad 10) has **no** `schematic.py`; schematic support lands in 0.8.0 targeting KiCad 11 + `kicad-cli api-server` headless | Headless schematic IPC is a **KiCad 11** feature. Do not fork KiCad 10 for it |
| `kicad-cli` 10.0 verbs: `fp`, `jobset`, `pcb`, `sch`, `sym`, `version`; `sch erc`, `pcb drc`, full export matrix; `hpgl` non-functional | Deterministic validators + exports go through `kicad-cli` |
| S-expr versions on branch 10.0: board `20260206`, schematic `20260306`, symbol lib `20251024` | Parser/writer compat matrix |

**Consequence for the Custom-KiCad gate:** the one blocker that would justify a
KiCad fork (live schematic IPC) is *already being solved upstream for KiCad 11*.
Default position: **do not fork KiCad**; re-evaluate against KiCad 11 instead.

---

## Architecture target

```
harness (Claude Code / Codex / AGY)
        │  EXECUTION PATH: delegate            AUDIT PATH: query / verify / evidence
        ▼
┌──────────────────────────┐
│ MCP GATEWAY (small)      │  ~7 external verbs, stable, cacheable, annotated
├──────────────────────────┤
│ TASK STATE MANAGER       │  objective / constraints / verified facts / failed attempts
├──────────────────────────┤
│ CONTEXT + ATTENTION MGR  │  budgets, compaction, retrieval, ACTIVE TASK anchor
├──────────────────────────┤
│ AGENT ROUTER             │  NO_LLM | SMALL | MEDIUM | LARGE | ESCALATE
├──────────────────────────┤
│ LOCAL AGENT RUNTIME      │  supervisor / schematic / pcb / verification
├──────────────────────────┤
│ PLAN COMPILER + PLAN IR  │  typed, versioned, precondition-checked, batched
├──────────────────────────┤
│ DETERMINISTIC ENGINE     │  187 existing capabilities + revisions + transactions
├──────────────────────────┤
│ KiCad: IPC (PCB) · S-expr (SCH) · kicad-cli (validate/export)
├──────────────────────────┤
│ VALIDATION + EVIDENCE    │  ERC/DRC/connectivity, semantic diff, evidence packs
└──────────────────────────┘
```

Crate plan (new crates are clean-room, no upstream-derived code):

```
crates/konnect-*         existing, AGPL, refactored in place
crates/kam-state         Task State Manager           (new)
crates/kam-context       Context + Attention Manager  (new)
crates/kam-plan          Plan IR + compiler + executor contract (new)
crates/kam-llm           local provider abstraction + router (new)
crates/kam-evidence      handles, resources, semantic diff, evidence packs (new)
crates/kam-bench         benchmark runner + metrics schema (new)
```

---

## Phases

| Phase | Goal | Status |
|---|---|---|
| **A** Bootstrap | clean workspace, fork, build, tests, run real MCP | **DONE** |
| **B** Cartography | map transport / registry / IPC / sexp / validation / errors | **DONE** |
| **C** Baseline benchmark | golden projects + metrics, measured before any refactor | **DONE** — `docs/benchmark.md` |
| **F** Compact MCP surface | capability index + tool-granular loading, then the ~7-verb gateway | **WIP** — −70.1 % external tokens landed |
| **D** Domain stabilisation | stable IDs, revisions, snapshots, idempotency, error catalog | TODO |
| **E** World model / task state / evidence | ProjectGraph, Task State, handles, deltas | TODO |
| **G** Plan IR + deterministic executor | batching, preconditions, postconditions, rollback | TODO |
| **H** Local AI runtime | provider abstraction, hardware probe, model bench, router | TODO |
| **I** Custom KiCad gate | only if a measured blocker survives KiCad 11 | TODO |
| **J** Scope expansion | fill the highest-value capability gaps | TODO |
| **K** Multi-harness | Claude Code, Codex, AGY | TODO |
| **L** Hardening | fuzzing, failure injection, concurrent user edits | TODO |
| **M** Final benchmark | baseline vs direct mode vs agent mode | TODO |

---

## Hardware (probed 2026-08-10)

```
GPU   NVIDIA GeForce RTX 5080 — 16 303 MiB VRAM, driver 591.86 (CUDA-capable)
CPU   AMD Ryzen 7 9800X3D, 8 cores / 16 threads
RAM   32 GiB (33 346 146 304 B)
Disk  C: 1 240 GB free
Local runtimes present: LM Studio (`lms`), only `nomic-embed-text-v1.5` (84 MB) installed
```

16 GB VRAM is the hard budget for the local model router: it must fit a
tool-calling model **plus** KV cache **plus** whatever KiCad's GUI is using.
That rules out unquantised 30B+ and drives the SMALL/MEDIUM/LARGE tiers.

---

## Success criteria (V1)

| Metric | Target |
|---|---|
| `SUCCESS_RATE` | ≥ baseline, target ≥ 95 % on the standard suite |
| external `tools/list` tokens | ≪ 22 329 (full catalog); target ≤ ~1 000 fixed |
| median `MCP_CALLS` per delegated task | ≤ 5 |
| `LLM_CALLS_PER_SUCCESSFUL_TASK` | materially below baseline via Plan IR |
| `WALL_CLOCK_P50` | ≤ baseline on standard tasks |
| `CAPABILITY_COVERAGE` | > baseline |
| silent corruption / silent stale-state write | **0** |
| mutations without an audit record | **0** |

Anything not achieved gets written down as not achieved. No benchmark rigging.

---

## Open questions

* Which local model fits 16 GB VRAM with reliable tool-calling + structured
  output? Must be benchmarked, not assumed.
* Does KiCad 10.0.3 on Windows expose `KICAD_API_SOCKET` reliably enough for
  unattended E2E, or do PCB E2E tests need a GUI session? This currently blocks
  PCB benchmark coverage entirely.
* Tool-granular loading bottoms out at **3 698 external tokens per task**, of
  which 2 785 is still `tools/list` churn. Does the ~7-verb gateway (stable
  catalogue, `CATALOG_TOKENS` → 0) have to land before Phase H, or can the local
  agent absorb the churn because it holds the toolbelt across many tasks?

## Measured state (2026-08-10)

See `docs/benchmark.md` for method and full tables.

| | baseline | now | target |
|---|---|---|---|
| EXTERNAL_TOKENS/task | 12 373 | **3 698** | ≤ 2 000 |
| SUCCESS_RATE | 18/18 | 18/18 | ≥ baseline ✓ |
| MCP_CALLS median/task | 11 | 10 | ≤ 5 |
| `tools/list` at startup | 1 680 | 1 958 | ≤ ~1 000 |
| retrieval recall @8 | — | 100 % | ≥ 98 % ✓ |
| retrieval precision @8 | — | 22.4 % | ≥ 60 % ✗ |
