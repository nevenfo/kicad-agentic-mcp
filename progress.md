# PROGRESS

## Phase actuelle

**K — multi-harness.** K.2 is closed, and so are K.1.4 and K.1.17. K.1 is down
to the campaign itself (K.1.1), which is one re-run from complete — and that
re-run costs the shared Pro window, so it is the user's call. Phases D, F, L
are closed; phase I stays gated by hardware (this machine has KiCad 10.0, not
the KiCad 11 / `kicad-cli api-server` it needs). Phase M depends on K.1.1 and
on nothing else.

## Tâche actuelle

**K.1.1 — the campaign.** Codex: complete, 14/14, no void run. Claude
(`claude-sonnet-5`): **13 of 14 scored, 1 still void** (`sch_hierarchy`, on the
spent quota window). Both halves live in `bench/results/k11-codex.json` and
`bench/results/k11-claude-sonnet5.json`, with the `sch_ldo` transcript under
`bench/results/k11-logs/`, and are re-scorable offline for free (K.1.16).

## Dernière tâche validée

**The `sch_ldo` re-run, and K.1.17 to land it.** The run went to completion at
**$0.7778** / 39 turns — the old $1.00 cap was the entire reason it had been
void — and **built a correct design**. Folded into the campaign file with the
new `--merge`, then re-scored: `VOID_RUNS` **2/14 → 1/14**, `DESIGN_PASS_RATE`
11/12 → **12/13 = 92.3 %**, `ON_SERVER_PASS_RATE` the same 12/13,
`UNNECESSARY_CALL_RATE` 3.4 % → 2.9 % PASS, `SAFETY_VIOLATIONS` unchanged at 2.

**K.1.17** is the tool that did the folding, and its rule is D97: a re-run
*replaces* the void run it re-runs and never adds to the campaign, because one
appended run turns 14 into 15 and silently restates every rate. Four refusals,
each exercised and each writing no file: no void of that task left to replace,
a re-run that is itself void, a mismatched harness, an `--out` pointing at
either input. It judges nothing — `--rescore` is the only thing that judges.

**What the claude half now says, and one re-run will not change it.** Four
thresholds fail; only `no_void_runs 1/14` waits on a run. `max_safety_violations
2` is K.1.15, real and staying. The other two are **findings, not debt**:
`min_pass_rate 7.7 %` and `max_instability_rate 50 %` are what a *strict*
success rate reads when `missing_expected` (10 runs) and `max_calls` (9) fire on
runs that built the design correctly — the gap between solving the task and
taking the route the task file scripted, which is exactly why
`DESIGN_PASS_RATE` and `ON_SERVER_PASS_RATE` are reported separately (K.1.11,
K.1.12). INV6 asks for a missed criterion to be recorded as missed; moving a
threshold after seeing the number it produced is not an option.

The headline is unchanged and identical on both harnesses: **every run that
reached Konnect built a correct design** — codex `ON_SERVER_PASS_RATE` 8/8,
claude 12/13. Codex's real finding is `SERVER_UNUSED 6/14`: on those runs it
never called Konnect and solved the task with its own sandboxed shell — about
the harness, not the server. Claude at `tools-off` isolation has
`SERVER_UNUSED 0/13`, `OFF_SERVER_CALLS 0`.

## Décisions actives

The standing decision log (D35–D97) lives in **`decisions.md`**, one entry per
decision with the evidence that settled it. Newest, and the ones this phase
turns on: D97 (a re-run replaces, never adds), D96 (`allowed_tools` judges reads
only), D95 (discovery is exempt from the audit), D93/D94 (annotations are part
of the shipping surface), D92 (a headless harness measures its own home), D91
(an audit judges what went *through* the gateway).

Standing since 2026-08-20, from the user:
- the claude half runs on **`--model claude-sonnet-5`**, plus one task replayed
  in `claude-opus-5` as an anchor back to the K.1.6 smoke run
- `--max-budget-usd 2.00` (was 1.00, which voided `sch_ldo` at $1.04 — the
  re-run under the new cap finished at $0.78)
- `claude -p` bills no dollars: `~/.claude.json` says
  `billingType: stripe_subscription`, `hasExtraUsageEnabled: false`, and there
  is no `ANTHROPIC_API_KEY` on this machine. The `total_cost_usd` the CLI
  reports is an *estimate*. The scarce resource is the Pro 5-hour window,
  shared with whatever Claude Code session is open — which is what ended the
  first claude half — so campaign size is the user's call, run by run

## Blocage actif

Aucun.

## Fichiers / zones utiles

- `bench/harness_runner.py` — the agentic runner. Run it with `py -3.11`.
  `HARNESSES` holds the argv builder, isolation level and parser per harness.
  Two modes spend nothing and need no `--server`: **`--rescore <json>`**
  re-judges a captured campaign with today's audit and prints the thresholds
  through `report()` verbatim, and **`--merge BASE RERUN --out MERGED`** folds
  a re-run into the void run it replaces (D97) without judging anything.
  `--dry-run` also spends nothing and touches no config. `--log-dir` writes the
  raw transcript. `HarnessResult.aborted` / `HarnessRun.aborted` mark a run the
  harness cut short. `tool_calls` (round trips, what `max_calls` counts) and
  `audited_calls` (what `audit()` judges) answer different questions.
  `CodexHomeGuard` keeps a codex run out of the operator's own `CODEX_HOME`.
  `--server` must be **absolute** (a relative path makes CreateProcess fail
  from the harness's own `$WORK` cwd); `--task` takes exactly one id.
- `bench/runner.py` — `audit()`, `fingerprint()`, `SAFETY_KINDS`, `THRESHOLDS`,
  `load_tasks()`. The harness runner imports all of it rather than
  reimplementing, which is the only reason the two numbers are comparable.
- `bench/agent_prompts.yaml` — one plain-language prompt per golden task, with
  no tool names, or the run would measure instruction-following.
- `decisions.md` — the why. `plan.md` — the roadmap. Git — the history.

## NEXT ACTION

**Close K.1.1 with the last void run**: `sch_hierarchy` ×1, `--repeat 1`,
`--max-budget-usd 2.00`, `--model claude-sonnet-5`, `--log-dir` set; then
`--merge` it into `bench/results/k11-claude-sonnet5.json` and re-score with
`--rescore --enforce`. Then the `claude-opus-5` anchor: one task replayed, its
own campaign file, compared and never merged (D97). **Both spend the shared Pro
window and need the user's go-ahead, run by run.** After that, phase M.
