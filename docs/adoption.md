# Adoption

What actually happens when someone who has never spoken to the maintainer tries
to install and use Konnect. One row per first-run report. Kept because a launch
that is not counted is a launch that cannot be judged.

**No telemetry exists and none is planned.** Every number on this page came from
a person who chose to write it down. Konnect edits your design files; a tool that
does that earns trust by not phoning home.

## The five metrics

These are the minimum needed to tell "nobody tried it" apart from "everybody
tried it and hit the same wall". Each has an explicit *unknown*, because a
missing answer is data and must not be silently read as a good one.

| Metric | Where it comes from | Unknown is written as |
|---|---|---|
| **Install succeeded** | *Did the install finish?* | `?` — includes "I could not tell whether it worked", which is itself a finding about the verification step |
| **Time to first task** | *How long from downloading to your first working task?* | `?` — "Not sure". `never` is a **value**, not an unknown |
| **First blocker** | *What was the first thing that stopped you?* | blank — the reporter got through with nothing worth naming |
| **Task attempted** | *What did you ask it to do?* | required; a report without it is incomplete |
| **Success / failure** | *Did that task work?* | `?` — "I never got as far as trying" is recorded as `not tried`, not as a failure |

Reports arrive through the [first-run report](https://github.com/nevenfo/kicad-agentic-mcp/issues/new?template=first-run.yml)
form, which asks exactly these questions and little else — a person who has just
given up will not fill in twenty fields.

## Reports

Nothing here yet: the first-run form was published in Phase R and v1.1.0 has had
no external installs. An empty table is a finding about **reach**, not about
quality, and it is the finding the launch kit exists to change.

| # | Date | Platform | Install | Time to first task | First blocker | Task attempted | Result |
|---|---|---|---|---|---|---|---|
| — | — | — | — | — | — | — | — |

Identify people only by the public GitHub handle they filed under, and only when
crediting them. Never copy anything else out of a report into this file.

## Baseline at launch

Measured on 2026-08-26, so that later numbers have something to be compared
against:

| | |
|---|---|
| Stars | 0 |
| Issues, open or closed | 0 |
| First-run reports | 0 |
| Release downloads | 1 — the maintainer's own verification of the published package (Q.5) |
| Discussions | disabled, deliberately — see below |

**Why Discussions stays off.** A project with no issues does not need a second
empty surface; splitting a handful of early reports across two places makes both
look dead and makes the tally above harder to keep honest. Issues with templates
carry the structure this page needs. Revisit when the volume of questions that
are *not* bug reports makes a forum shape worth having.

## How the maintainer reads this

- **A blocker that appears twice is not anecdote.** It goes on the plan as a
  defect with its class — UX, packaging, documentation, configuration, or
  product — before anything is written to fix it.
- **A report that stops at "install succeeded: no" is the most valuable kind.**
  It means the walk in [launch/first-run-walk.md](launch/first-run-walk.md)
  missed something a real machine has.
- **`never` in *time to first task* outranks every success.** People who got
  there are already past the part that decides adoption.
