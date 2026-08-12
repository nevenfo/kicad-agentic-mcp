# PROGRESS

## Phase actuelle

H — Local AI runtime. H.1–H.4 done, H.5 partial (H.5.4 open), H.6/H.7 open.

## Tâche actuelle

H.5.4 — re-run the `--strict-json` comparison on the chosen model, now that
`finish_reason` can state the mechanism instead of leaving it inferred.

## Dernière tâche validée

H.5.3 — the two models measured on one build (E26) in a declared 32 768 window.

Validation :
- `bench/results/model-fit-qwen3.5-9b-e26.json` and
  `model-fit-gpt-oss-20b-medium-e26.json`, both `loaded_context_length: 32768`
- grade 3: 20B 12/60 vs 9B 3/60, Fisher exact **p = 0.0246**
- compiled: 54/60 vs 49/60, p = 0.295 — not claimed
- `docs/benchmark.md` § Model fit updated with both rows and the comparison

Committed together with H.4.6–H.4.11 (E22–E26). The H.5.4 run is in flight and is
deliberately outside that commit: its `--strict-json` result files land in a
second one.

## Décisions actives

- D38 — **`gpt-oss-20b` at `medium`, ctx 32 768, is the chosen local model.** It
  reaches grade 3 four times as often as `qwen3.5-9b` (p = 0.0246) at half the
  output tokens and 5.0 vs 20.0 LLM calls per success. Supersedes D36, whose
  opposite reading came from a pair straddling the E24 and pre-E24 builds.
- D37 — before attributing a number to a model, check the failure histogram: one
  refusal string repeated is ours, a spread across many is the model's. A library
  fix outweighed the entire model choice (`LLM_CALLS_PER_SUCCESSFUL_TASK`
  10.0 → 5.5 at p = 0.0001 on the compile rate).
- E26 changed the harness's clock, not the model's answers: same 20B config,
  E24 → E26, grade 3 11 → 12 (p = 1.0), compiled 54/60 both times, wall P50
  15 798 → 12 612 ms. The E24 build comparison therefore still stands.
- NO_LLM before any model-to-model routing — with one model chosen, the only tier
  boundary still worth fitting is *no LLM* vs LLM.

## Blocage actif

Aucun.

## Fichiers / zones utiles

- `bench/model_fit.py` — the oracle; `--reasoning-effort`, `--repair`,
  `--strict-json`, `select_best_round`, `loaded_context_length`
- `bench/results/model-fit-gpt-oss-20b-medium-e26.json` — the reference run
- `docs/benchmark.md` § Model fit — the standing table
- `docs/local-agents.md` — method and the router's stated reasons for not existing
- LM Studio: `lms load openai/gpt-oss-20b --context-length 32768 --gpu max`

## NEXT ACTION

Finish H.5.4. The run is already launched — `bench/model_fit.py --model
openai/gpt-oss-20b --reasoning-effort medium --strict-json`, everything else
identical to `model-fit-gpt-oss-20b-medium-e26.json`. Validation — a committed
`bench/results/model-fit-gpt-oss-20b-medium-e26-strictjson.json`, compared to that
run on grade 3, compile rate and `invalid_json`/`truncated` counts with Fisher
exact, and the outcome recorded in `docs/benchmark.md` § Model fit. D33 keeps
`strict_json` **off** unless this run says otherwise.
