# Local agents

What exists today is a **seam** and an **oracle**, not an agent runtime. This
document describes both, states what they have measured, and is explicit about
the parts that are deliberately not built yet.

The project's position on local models has not changed since the base was
selected: a local model is a planner and an interpreter of intent, never a
substitute for a deterministic engine. Geometry, connectivity, ERC, DRC,
transactions, diffs and rollback are code. The model's only job is to turn an
objective into a plan the compiler can refuse.

---

## The seam — `crates/kam-llm`

Clean-room, `MIT OR Apache-2.0`, and it depends on no `konnect-*` crate. That is
enforced by its manifest rather than by intention (rule D11 in `progress.md`), so
a future re-licence does not require rewriting it, and so nothing about KiCAD can
leak into the backend abstraction.

| module | what it is |
|---|---|
| `provider` | the whole contract: one `async fn complete`, object-safe so a router can hold `Box<dyn Provider>` and swap backends without a caller changing. `Message`, `ToolDef`, `ToolCall`, `StructuredOutput` and `ReasoningEffort` are the vocabulary around it, shaped like MCP's own tool definitions so a tool catalogue can be handed across untranslated. |
| `openai_compat` | the one concrete backend. |
| `usage` | `Usage` exists so `LOCAL_INPUT_TOKENS`, `LOCAL_OUTPUT_TOKENS`, `TTFT_LOCAL` and `TOKENS_PER_SECOND_LOCAL` are a field read at the call site rather than a second instrumentation pass. A backend reporting no counts leaves them at `0` instead of estimating. |
| `hardware` | `probe` never panics and never guesses. `nvidia-smi` first; a Windows display-adapter fallback that reports names and **not** VRAM, because `Win32_VideoController.AdapterRAM` is a 32-bit field that misreports modern cards and reading it wrong is worse than not reading it; and a backend probe that checks `PATH` presence only — a capability probe, not a liveness check, so it opens no socket. |

### Why OpenAI-compatible HTTP is the only backend

Decided from primary sources, recorded as D31:

* **`vLLM` has no native Windows support** — Linux or WSL2 only, per its own
  installation documentation.
* **`llama.cpp`** is the only native path, and Blackwell `sm_120` needs
  `-DCMAKE_CUDA_ARCHITECTURES=120` from source; the upstream issue asking for a
  prebuilt path closed *not planned*.
* **LM Studio** wraps `llama.cpp`, is already installed, and exposes both tools
  and `response_format: json_schema` on an OpenAI-compatible endpoint.

So the abstraction targets OpenAI-compatible HTTP and nothing else, which costs
nothing in generality: LM Studio and `llama-server` both speak it. `llama-server`
is the escape hatch the moment a measurement needs a flag LM Studio does not
expose (KV cache type, MoE expert offload). **That switch must be a config
change, never a code change** — it is the entire reason the trait exists.

### Loopback is the default and the override is named

`OpenAiCompatConfig::new` **refuses a non-loopback base URL**. Exposing a local
inference backend to the network requires a separate, explicitly named
constructor, so it is a decision somebody typed rather than a default they
inherited.

---

## The oracle — `bench/model_fit.py`

**The grade never comes from reading the model's answer.** Every attempt is
compiled and applied by the real built server on a 0–3 ladder:

| grade | meaning | `outcome` |
|---|---|---|
| 0 | not schema-valid JSON, or the generation never finished | `invalid_json`, `truncated` |
| 1 | valid JSON the compiler refuses | `compile_failed` |
| 2 | compiles, but does not apply, or applies and breaks a task invariant or the ERC budget | `not_applied`, `applied_invalid` |
| 3 | applies clean | `success` |

`outcome` is categorical **beside** the ladder and never renumbers it. That rule
exists because a run once counted a reply cut off at the token cap as a reply the
model got wrong (E20).

`check_assertion` and `GatewayClient` are **imported from `bench/runner.py`**,
not reimplemented, so the model is judged by the same path already proved against
real `kicad-cli`. A harness with its own compiler would refuse a plan for a reason
it invented.

The prompt is four blocks in fixed order — immutable rules, plan schema,
operation library, then the dynamic task and the ACTIVE TASK anchor. The first
three are byte-identical across every task, hint level and model so a prefix cache
can hold them, and the schema and operation-library blocks are pulled from
`kicad_describe(["apply_plan"])` against the running server rather than
hand-typed, because a copied schema drifts silently.

Four tasks: `01_divider`, `02_ldo`, `03_decoupling_bank`, `04_reference_heavy`.
The default measurement has three historical hint levels (`full`, `minimal`,
`none`); the optional `geometry` isolation arm keeps only pin offsets and their
derived coordinates.

### The oracle is proved before any model runs

`--selftest` involves no model and exercises all four rungs plus the
best-round selection: a correct divider plan grades 3, an unknown operation
grades 1 with the compiler's own refusal, floating pins grade 2 against an ERC
budget of 0, malformed JSON grades 0, and a descending repair sequence keeps its
best round. It must print `SELFTEST PASSED` before any measurement is believed.

### Repairs keep the best round, not the last

`--repair N` allows extra calls after a failure, each fed its own previous plan
and the server's **verbatim** refusal — no advice, no restated rules, no worked
example, because the error message is the thing under test.

Measured, one repair round converted **0 of 58** failures into a success and
pushed 11 of them *down* the ladder (D35). Since the harness recorded the last
round, those 11 were what the run reported. `select_best_round` now records the
best-graded round instead, ties going to the earlier one. Cost accounting stays
separate and honest: tokens and `llm_calls` are summed over **every** round
performed, discarded ones included, because what a task cost is what it took.
TTFT and tokens/second stay the kept round's, because they describe a single
generation.

---

## Everything that is a measurement variable

A run that does not record what it sent cannot be compared to another run. Three
settings were each discovered the hard way, and each is now recorded in the
results file:

| variable | why it exists |
|---|---|
| `--strict-json` | constrained decoding. Measured off *and* on: `strict: true` was **worse on both ends** and stays off (D33). |
| `--reasoning-effort` | **omitting it is not "the default"** — for `gpt-oss-20b` it is `low`, measured, with identical token counts and an identical harmony-parse failure. Unset sends no field at all, so historical runs stay comparable (E22). |
| `loaded_context_length` | probed from the backend and recorded. A run at `high` once graded 0/60 with 51 attempts at `finish_reason: length`, because the instance was loaded at 8 192 of a possible 131 072 (E23). |

The same `ReasoningEffort` option exists on `kam-llm`'s `CompletionRequest`,
unset meaning an absent field on both sides, proved by test. A setting the
benchmark can select and the runtime cannot send would make the measurement
unusable in production.

---

## What has been measured

Full tables in `docs/benchmark.md` and `progress.md`. The findings that shape the
design:

* **The wall is ERC correctness, not format.** Once the library defects were
  removed, plans that compile went from 6/60 to 46/60 while grade 3 stayed at
  4/60. A message saying "one pin is not connected" does not teach a 9B where the
  wire goes.
* **There is no EDA-specialised open-weight model.** The projects that look like
  one are systems built on general models. The electronics competence has to come
  from the deterministic engine and the validators — which is what this
  architecture already assumes.
* **Deliberation is 66–97 % of local output tokens.** Any budget that counts
  answers rather than generation is wrong by up to an order of magnitude.
* **More effort is not more capability.** `high` was dominated by `medium` on the
  same context window: identical success at 2.8× the tokens and 2.8× the wall
  clock.

---

## Context budget contract

`kam-context` owns one `ContextBudget` per conversation context. Limits are
explicit: the crate has no implicit model or window default. Each completed
call is accounted from `kam_llm::Usage`; `completion_tokens` already includes
`reasoning_tokens`, so reasoning remains a reported split and is never added a
second time. Missing backend counts produce `Unmeasured`, not a false zero-cost
success.

The initial `gpt-oss-20b`, `medium`, 32 768-token profile is grounded in the
same real E27/E28 calls used to fit the router:

| run / arm (n = 20) | input p95 | output p95 | reasoning p95 |
|---|---:|---:|---:|
| E27 `full` | 2 707 | 5 044 | 4 584 |
| E28 `geometry` | 2 620 | 4 451 | 4 171 |

The runtime profile therefore reserves **5 120 completion tokens** (the next
256-token boundary above the largest observed p95), leaving **27 648 prompt
tokens**. Crossing that prompt boundary while the measured call still fits
returns `CompactionRequired`; crossing either the context window or the
completion reserve returns `Exceeded`. These are accounting boundaries, not a
claim that the model needs all remaining prompt space. A different model,
effort, or loaded window requires a separately supplied profile.

Compaction renders the full objective, hard constraints, success criteria and
verified facts directly from `TaskState`; this durable core is refused rather
than truncated when it cannot fit. Caller-ranked retrieval is inserted next as
atomic bundles, before the evictable transcript. This is deliberate after E28:
electrical, Plan IR and geometry guidance that is only useful together travels
as one bundle, rather than letting a tight budget silently keep geometry alone.
The remaining capacity holds the newest contiguous transcript suffix.

The integration test `recorded_e27` replays the committed E27 decoupling calls:
the stable prefix (`2 015` tokens), per-call input/output/reasoning counts and
the measured `full - none` retrieval delta all come from
`model-fit-gpt-oss-20b-medium-e27.json`. Repeated recorded completions force one
compaction cycle; the result stays within `27 648`, retains the durable task and
retrieval bundle, and drops only oldest transcript messages.

---

## Runtime boundary and remaining work

The measured route is now exactly `NO_LLM | LOCAL | ESCALATE`. The earlier
five-tier proposal was rejected: `gpt-oss-20b` at `medium` reaches grade 3 four
times as often as `qwen3.5-9b` (12/60 vs 3/60, p = 0.0246) at half the output
tokens (D38), so the 9B is not a cheaper tier. Self-repair converted 0 of 58
attempts (D35), so it is not another runtime rung either.

The first piece of that boundary is built and it needed no router to hold it.
Sixteen of E26's sixty attempts failed to apply on a `lib_id` naming exactly one
installed symbol through a library that does not exist; `canonical_lib_id`
resolves that case in the library index and refuses everything ambiguous, taking
`not_applied` to 5/60 (p = 0.0148) and `LLM_CALLS_PER_SUCCESSFUL_TASK` from 5.0
to 3.75. The cheapest tier is not a smaller model, it is the call that never
happens — and the deterministic answer lives at the call site, not behind a
routing decision that would have to be made before knowing the answer exists.

**And it is the last piece the measurements ask for.** Replaying E27's applied
plans through `run_erc` (`bench/erc_residue.py`) names the 139 violations that
`erc_max_errors` had only counted: 68 `Pin not connected`, 62 `Input Power pin
not driven`, 9 `Label not connected`, spread so that the largest group a single
deterministic rule could flip is 2 of 60. E26's `lib_id` histogram was one shape
repeated sixteen times; this one is a model failing to wire what it placed, in
12 of the 16 attempts the ERC budget rejected. The five tiers are down to three
by measurement — `NO_LLM`, one local model, escalate — because `SMALL` costs
more per success (D38) and the self-repair rung converted 0 of 58 (D35).

What the prompt carries is now fitted one step further. E27 gave 9/20 grade 3
with `full` hints against 7/40 without (one-sided p = 0.0323). E28 retained only
pin offsets and derived coordinates and got 3/20, all on the decoupling macro:
indistinguishable from the 7/40 non-full residue (two-sided p = 1.0), and below
`full` in the pre-declared direction (one-sided p = 0.0412; two-sided p =
0.0824). A generic geometry block is therefore insufficient. The router payload
must retrieve task-specific electrical and Plan IR constraints together with
geometry; n = 20 does not justify naming one removed sentence as the mechanism.

### The gateway split is explicit

Direct mode is the existing `kicad_describe` / `kicad_invoke` path: the external
harness owns intent and the gateway executes deterministic calls. It must never
start a local model as a side effect. Agent mode is the distinct `kicad_agent`
gateway entry point; its local supervisor uses the measured
`NO_LLM | LOCAL | ESCALATE` route, but it reaches the same Plan IR compiler and
validators as direct mode.

The split is selected by the caller through the entry point, not inferred from
prompt wording and not set for an entire server process. This preserves current
clients, makes local inference an explicit cost/privacy decision, and permits
direct and agent tasks in one session. `ESCALATE` is a structured result carrying
the failure and evidence handles back to the caller; it is not permission for
the server to contact an external model. `LOCAL` uses `gpt-oss-20b`, effort
`medium`, a 32 768-token window and the measured 5 120-token completion reserve.
A model proposal is stored as an assumption, never as a verified fact; H.7.2
owns the validator verdict.

Local inference is opt-in and loopback-only. Configure it in `konnect.toml`:

```toml
local_llm_base_url = "http://127.0.0.1:1234/v1"
local_llm_model = "gpt-oss-20b"
```

`KONNECT_LOCAL_LLM_BASE_URL` and `KONNECT_LOCAL_LLM_MODEL` are fallbacks when
the file leaves either value unset. With no URL setting, `LOCAL` returns structured
`local_provider_unavailable`; Direct remains fully functional. A non-loopback
URL is rejected by the gateway.

**Specialised agents.** The supervisor and verification runtimes now exist.
Schematic and PCB execution still flow through the shared deterministic Plan IR
path. Anything further is added only on measured evidence.

`kicad_agent_verify` runs the existing validator or reuses only its exact-revision
cache entry. It returns `PASS`, `FAIL` or `COULD_NOT_RUN`, with counts, source and
`kicad://verification/*` evidence. Only completed `kicad-cli`/cache verdicts enter
`TaskState.verified_facts`; missing documents, unsupported types and CLI errors
produce no fact and cannot read as PASS.

`bench/agent_e2e.py` exercises the complete gateway without a repair pass. The
recorded H.7.3 `model_divider` run used `gpt-oss-20b` at `medium`, compiled and
applied 8/8 deterministic steps, then obtained ERC `PASS` with 0 errors and 0
warnings. Its counters prove one loopback local call and zero external calls;
the result is in `bench/results/agent-e2e-gpt-oss-20b-medium-h7.3b.json`.

When they do exist, two rules already apply: handoffs are structured payloads
rather than conversations, and a local agent is never the only source of
validation — the verdict comes from `kicad-cli` and the deterministic validators,
or the change does not carry one.
