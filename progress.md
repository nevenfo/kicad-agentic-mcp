# PROGRESS

## Phase actuelle

**K — multi-harness.** K.2 is closed, and so is K.1.4. K.1 is down to the
campaign itself (K.1.1), and what is left of it costs the shared Pro window, so
it is the user's call. Phases D, F, L are closed; phase I stays gated by
hardware (this machine has KiCad 10.0, not the KiCad 11 / `kicad-cli api-server`
it needs). Phase M depends on K.1.1 and on nothing else.

## Tâche actuelle

**K.1.1 — the campaign.** Codex: complete, 14/14, no void run. Claude
(`claude-sonnet-5`): 12 of 14 scored, 2 still void. Results are persisted in
`bench/results/k11-codex.json` and `bench/results/k11-claude-sonnet5.{json,log}`
and are re-scorable offline for free (K.1.16).

## Dernière tâche validée

**K.1.4 — the codex harness has paid its measurement.** The task was still
carrying the state of 2026-08-20's three exploratory runs, which said the
`mcp_tool_call` success branch was unproven and the blocker was K.2. Both have
moved: K.2 shipped the annotations, and the K.1.1 campaign put 14 real
transcripts through this harness. Checked against the captured results, at no
cost: **8 of the 14 runs made 198 successful konnect calls across 38 distinct
tools**, so the success branch is proven against live output.

One branch stays unexercised and is recorded as such rather than claimed: no
codex run ever called `kicad_invoke`, so the gateway unwrap path in
`parse_codex_jsonl` never ran, and `gateway_unwrap_warning` correctly returned
`None` on all 14 runs. That is a fact about which tools codex chose, not a debt
of the bench.

The headline is unchanged and identical on both harnesses: **every run that
reached Konnect built a correct design** — codex `ON_SERVER_PASS_RATE` 8/8,
claude 11/12. Codex's real finding is `SERVER_UNUSED 6/14`: on those runs it
never called Konnect and solved the task with its own sandboxed shell — about
the harness, not the server. Claude at `tools-off` isolation has
`SERVER_UNUSED 0/12`, `OFF_SERVER_CALLS 0`.

Of the claude half's thresholds, two are still red and both wait on runs rather
than on a decision:
- `max_safety_violations 2` — **real, and the tier working** (K.1.15); stays.
- `no_void_runs 2/14` — `sch_ldo` (old $1.00 cap) and `sch_hierarchy` (spent
  window) have to be re-run.

## Décisions actives

The standing decision log (D35–D96) lives in **`decisions.md`**, one entry per
decision with the evidence that settled it. Newest, and the ones this phase
turns on: D96 (`allowed_tools` judges reads only), D95 (discovery is exempt from
the audit), D93/D94 (annotations are part of the shipping surface), D92 (a
headless harness measures its own home), D91 (an audit judges what went
*through* the gateway).

Standing since 2026-08-20, from the user:
- the claude half runs on **`--model claude-sonnet-5`**, plus one task replayed
  in `claude-opus-5` as an anchor back to the K.1.6 smoke run
- `--max-budget-usd 2.00` (was 1.00, which voided `sch_ldo` at $1.04)
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
  **`--rescore <json>`** re-judges a captured campaign with today's audit and
  prints the thresholds through `report()` verbatim: no server, no agent,
  nothing spent, and `--server` is not required with it. `--dry-run` also
  spends nothing and touches no config. `--log-dir` writes the raw transcript.
  `HarnessResult.aborted` / `HarnessRun.aborted` mark a run the harness cut
  short. `tool_calls` (round trips, what `max_calls` counts) and
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

**Close K.1.1 by re-running the 2 void runs**, which is the only thing
`no_void_runs` is waiting on: `sch_ldo` ×1 and `sch_hierarchy` ×1, `--repeat 1`,
`--max-budget-usd 2.00`, `--model claude-sonnet-5`, `--log-dir` set. Then the
`claude-opus-5` anchor: one task replayed, to tie back to K.1.6. **Both spend
the shared Pro window and need the user's go-ahead, run by run.** Merge the new
runs into `bench/results/k11-claude-sonnet5.json` and re-score with
`--rescore --enforce`. After that, phase M.
