# PROGRESS

## Phase actuelle

**K — multi-harness.** K.2 is closed. K.1 is down to the campaign itself
(K.1.1). Phases D, F, L are closed; phase I stays gated by hardware (this
machine has KiCad 10.0, not the KiCad 11 / `kicad-cli api-server` it needs).
Phase M depends on K.1.1 and on nothing else.

## Tâche actuelle

**K.1.1 — the campaign.** The codex half is complete (14 runs, no void run).
The claude half has 6 usable runs of 14; the 8 others were void and are being
re-run task by task.

## Dernière tâche validée

**K.1.13** — a run the harness cut short is not a failed run. The claude half
spent its Pro 5-hour window mid-suite: 7 runs were quota-rejected (six of them
in ~380 ms with zero calls) and 1 hit the `--max-budget-usd` cap, and all 8 were
scored as failures. The claude CLI reports a rejected quota as `is_error: true`
*with* `subtype: "success"`, so the parser printed the opposite of what
happened. `HarnessResult.aborted` now names the real cause, `report()` keeps
void runs out of every rate (but not out of `COST_USD`, which is spend), and
`no_void_runs` is a hard threshold so the exclusion cannot launder an
incomplete campaign.

Validation (spends nothing, offline against the 14 captured transcripts):
- 8 void runs identified with their causes; `DESIGN_PASS_RATE` 6/14 → **6/6**
- negative control: the 14 codex runs re-report identically, `no_void_runs` PASS

Before it, **K.1.11/K.1.12** — `design_success` now blocks on `SAFETY_KINDS`
only (a route the task did not script is not a wrong design), and at
`read-only-sandbox` isolation the `min_pass_rate` gate reads
`ON_SERVER_PASS_RATE` instead of re-admitting the `off_server_calls` check the
report had just SKIPped. Codex `DESIGN_PASS_RATE` 3/14 → 10/14, `SUCCESS_RATE`
unchanged at 1/14 (no safety violation was masked).

## Décisions actives

The standing decision log (D35–D95) lives in **`decisions.md`**, one entry per
decision with the evidence that settled it. Newest, and the ones this phase
turns on: D95 (discovery is exempt from the audit), D93/D94 (annotations are
part of the shipping surface), D92 (a headless harness measures its own home),
D91 (an audit judges what went *through* the gateway).

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
  `--dry-run` spends nothing and touches no config. `--log-dir` is what makes a
  paid run re-scorable offline: the `.jsonl` goes back through `parse_stream` +
  `audit` instead of re-running the agent, and `--out` writes
  `asdict(HarnessRun)`, so a captured campaign round-trips into `report()`
  unchanged. `HarnessResult.aborted` / `HarnessRun.aborted` mark a run the
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

**Finish K.1.1.** Running now in the background: the 3 tasks that have never
produced a single real claude run — `manufacturing_exports`, `recovery`,
`sch_inspection`, `--repeat 2` each, `--model claude-sonnet-5`,
`--max-budget-usd 2.00`, one `harness_runner.py` invocation per task with
`--log-dir` and `--out`. When they land, merge them with the 6 usable runs from
the first half, re-report, and decide with the user whether to spend more of
the window on the remaining 2 void runs (`sch_ldo` ×1, `sch_hierarchy` ×1) and
on the `claude-opus-5` anchor. `no_void_runs` FAILs until the claude half is
whole.

What the campaign already establishes, and no further run changes: **every run
that actually reached Konnect built a correct design** — codex
`ON_SERVER_PASS_RATE` 8/8, claude 6/6. What codex's half is really carrying is
`SERVER_UNUSED 6/14`: on those runs it never called Konnect at all and solved
the task with its own sandboxed shell. That is a finding about the harness, not
about the server.
