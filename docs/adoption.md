# Adoption

What actually happens when someone who has never spoken to the maintainer tries
to install and use Konnect. This page records first runs, bugs, feature requests
and public reach without turning missing answers into successes.

**No telemetry exists and none is planned.** Every report below must come from
a person who chose to submit it, and every repository metric must come from a
dated GitHub snapshot. Never infer a user, an install or a successful run from a
download count.

## Intake

- [First-run report](https://github.com/nevenfo/kicad-agentic-mcp/issues/new?template=first-run.yml)
- [Bug report](https://github.com/nevenfo/kicad-agentic-mcp/issues/new?template=bug.yml)
- [Feature request](https://github.com/nevenfo/kicad-agentic-mcp/issues/new?template=feature.yml)

Record `?` when an OS, KiCad version, MCP client or outcome is not supplied. Do
not contact a reporter only to fill a tally cell. Identify people only by the
public GitHub handle they filed under, and only when crediting them.

## Feedback log

One row per external report, whether it is a first run, bug report or feature
request. The five decision metrics remain **install outcome**, **time to first
task**, **first blocker**, **task attempted** and **first-use outcome**. The
remaining fields preserve the environment, the reported need and any follow-up
without requiring a second ledger.

| # | Date | Source | Konnect | OS | KiCad | MCP client | Install | Time to first task | First blocker | Task attempted | First use | Bug | Feature requested | Known cause | Action |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| — | — | — | — | — | — | — | — | — | — | — | — | — | — | — | — |

Use these values consistently:

| Field | Values |
|---|---|
| Source | issue or public-feedback URL; venue plus date when no URL exists |
| Konnect | reported version, or `?` |
| OS / KiCad / MCP client | reported value, or `?` |
| Install | `success`, `failure`, or `?` |
| Time to first task | reported duration, `never`, or `?` |
| First blocker | reporter's first blocker; blank only when none was reported |
| First use | `success`, `partial`, `failure`, `not tried`, or `?` |
| Bug / Feature requested | short summary, `none` when explicitly absent, or `?` |
| Known cause | established cause, `unknown`, or `n/a` when no failure exists |
| Action | issue, commit or decision link; `none` when no action is taken |

As of 2026-08-27, no external first-run report has been received. The empty
table is evidence about reach, not product quality.

Link the source instead of copying logs or personal information. Record a bug
or feature request before deciding whether it belongs on the technical roadmap.

## Public metrics

Add dated snapshots; never rewrite an older row. `Release downloads` is the sum
of GitHub asset download counters across all releases, not a count of installs
or people. `v1.1.1 downloads` is the sum for that release alone.

| Date | Stars | Forks | Release downloads | v1.1.1 downloads | First-run reports | Bugs | Feature requests |
|---|---:|---:|---:|---:|---:|---:|---:|
| 2026-08-27, pre-announcement | 0 | 0 | 5 | 1 | 0 | 0 | 0 |

Source for the pre-announcement row: GitHub repository, issue and release asset
counters read on 2026-08-27. The five downloads were `v1.0.0: 2`, `v1.1.0: 2`
and `v1.1.1: 1`; these counters do not distinguish maintainer verification from
external downloads.

## How the maintainer reads this

- **A blocker that appears twice is not anecdote.** Classify it as UX,
  packaging, documentation, configuration or product before planning a fix.
- **An install failure is a useful report.** It shows the documented first-run
  walk missed something on a real environment.
- **`never` in time to first task outranks every success.** It identifies the
  part of the path that decides adoption.
- **Downloads are reach, not validation.** Only a report can establish whether
  installation or a task succeeded.
