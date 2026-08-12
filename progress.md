# PROGRESS

## Phase actuelle

H — Local AI runtime. H.1–H.5 done, H.6/H.7 open.

## Tâche actuelle

H.6.1 — NO_LLM first: extend the deterministic operation library wherever a new
measurement shows an LLM call can be removed entirely.

## Dernière tâche validée

H.5.4 — the `--strict-json` comparison re-run on the chosen model.

Validation :
- `bench/results/model-fit-gpt-oss-20b-medium-e26-strictjson.json` against
  `model-fit-gpt-oss-20b-medium-e26.json`, same build, effort and window
- grade 3 12/60 → 9/60 (p = 0.632), compiled 54/60 → 58/60 (p = 0.272),
  `compile_failed` 6 → 2 (p = 0.272) — nothing significant either way
- `invalid_json` 0/60 in both runs; `finish_reason: stop` on all 120 attempts
- `docs/benchmark.md` § Model fit updated; `strict_json` stays **off**

## Décisions actives

- D38 — **`gpt-oss-20b` at `medium`, ctx 32 768, is the chosen local model.** It
  reaches grade 3 four times as often as `qwen3.5-9b` (12/60 vs 3/60, p = 0.0246)
  at half the output tokens and 5.0 vs 20.0 LLM calls per success. Supersedes
  D36, whose opposite reading came from a pair straddling two builds.
- D37 — before attributing a number to a model, check the failure histogram: one
  refusal string repeated is ours, a spread across many is the model's. A library
  fix outweighed the entire model choice (`LLM_CALLS_PER_SUCCESSFUL_TASK`
  10.0 → 5.5 at p = 0.0001 on the compile rate).
- D33 holds, now on the mechanism rather than on outcome counts alone:
  `strict_json` off. Its failure mode is already at zero without it.
- E26 changed the harness's clock, not the model's answers: same 20B config,
  E24 → E26, grade 3 11 → 12 (p = 1.0), compiled 54/60 both times, wall P50
  15 798 → 12 612 ms.
- NO_LLM before any model-to-model routing. With one model chosen and the 9B
  costing more per success, there is no cheap tier to route *to*; the only
  boundary still worth fitting is *no LLM* vs LLM.

## Blocage actif

Aucun.

## Fichiers / zones utiles

- `crates/konnect-core/src/plan/ops.rs` — the deterministic operation library
  H.6.1 extends
- `bench/model_fit.py` — the oracle; `--reasoning-effort`, `--repair`,
  `--strict-json`, `select_best_round`, `loaded_context_length`
- `bench/results/model-fit-gpt-oss-20b-medium-e26.json` — the reference run; its
  failure histogram is where the next NO_LLM candidate has to come from
- `docs/benchmark.md` § Model fit — the standing table
- `docs/local-agents.md` — method, and the router's stated reasons for not existing
- LM Studio: `lms load openai/gpt-oss-20b --context-length 32768 --gpu max`

## NEXT ACTION

Start H.6.1 by reading the failure histogram of
`bench/results/model-fit-gpt-oss-20b-medium-e26.json`: group the 48 non-grade-3
attempts by failure string and find any group large enough to be a deterministic
operation rather than a model limitation. E25 found no dominant refusal string
left, so the candidate must come from this run's own residue — if none is large
enough, say so and move to H.6.2 rather than inventing one.
