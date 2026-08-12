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

Four tasks: `01_divider`, `02_ldo`, `03_decoupling_bank`, `04_reference_heavy`,
each at three hint levels (`full`, `minimal`, `none`).

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

## What is deliberately not built

**The router.** `NO_LLM / SMALL_LOCAL / MEDIUM_LOCAL / LARGE_LOCAL /
EXTERNAL_ESCALATION` is the next step and it stays unbuilt on purpose. The two
models are now separated on one build — `gpt-oss-20b` at `medium` reaches grade 3
four times as often as `qwen3.5-9b` (12/60 vs 3/60, p = 0.0246) at half the output
tokens (D38, superseding D36) — so the 9B is not a cheaper tier to route *to*, it
is a model that costs more per success. What a router still has to earn is the
`NO_LLM` boundary, whose thresholds must come from measurements rather than from
n = 60 on four tasks.

**Specialised agents.** The intended minimum is router/supervisor, schematic,
PCB and verification, with anything further added only on measured evidence. None
of them exists yet, and adding them before a routing decision can be justified
would be building the abstraction before the flow.

When they do exist, two rules already apply: handoffs are structured payloads
rather than conversations, and a local agent is never the only source of
validation — the verdict comes from `kicad-cli` and the deterministic validators,
or the change does not carry one.
