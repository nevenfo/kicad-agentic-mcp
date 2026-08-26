# R.6 — The decision gate

One page a decision can be made from. It closes phase R and opens nothing: the
next phase is the user's to open, in a separate decision.

**Order of writing, stated because the rule depends on it.** R.6.1 says the
promotion criteria are written *before* the feedback is read. The R.5 tally was
read first — it is empty, and an empty tally cannot select a candidate, so
nothing below could have been reverse-engineered from it. Every criterion is
written against evidence R.1 and R.3 produced, and against what would have to
become true for a candidate to earn a phase.

## 1 — What the tally says (R.6.4)

Read from GitHub on 2026-08-26, the day v1.1.0 was published:

| Metric | Value |
|---|---|
| Stars | 0 |
| Forks | 0 |
| Watchers | 0 |
| Issues opened by anyone but the maintainer | **0** |
| First-run reports filed | **0** |
| Release asset downloads | 2 (`konnect-pcm-v1.1.0-windows.zip`), 0 for the other six — all the maintainer's |

**No outside feedback arrived, and that is not a finding about the product.**
The project has never been announced anywhere: R.4's kit is drafted and posted
nowhere, by the phase's own rule. Zero reach produces zero feedback whatever the
software is like. The empty tally therefore says one thing only — *the
distribution step has not happened yet* — and it must not be read as evidence
about demand, quality or fit.

That is why the recommendation at the bottom is about reach before capability.

## 2 — What R.1 found (the first-run walk)

Eleven frictions, classified before any fix (INV-R3). Their disposition today:

| Class | Found | Fixed in R | Left |
|---|---|---|---|
| **product, blocking** | F-01 (`kicad-cli` never discovered) | **fixed** (R.7) | — |
| **configuration, blocking-in-practice** | F-12 (`ipc_address` never derived) | **fixed** (R.8) | — |
| documentation | F-02, F-05, F-06, F-09 | fixed in R.2's README rewrite | — |
| UX | F-03 (PCM shows the upstream author and homepage), F-08 (PCM's confirmation step) | — | F-03 rides v1.1.1; F-08 is KiCad's own UI |
| packaging | F-04 (no checksums), F-11 (`plugin.json` declares a button KiCad never renders) | — | both open, neither blocks first use |
| product, non-blocking | F-07 (`apply_template` claims to wire what it only places) | — | open |

**The blocking path is closed.** A stranger on a stock Windows KiCad 10 can now
reach a KiCad-verified result without editing a config file — once v1.1.1 ships,
which is the whole of R.7.7.

## 3 — What R.3 found (the demo, three runs)

- **Run 1 failed its own criterion and was worth more failed than passed.** It
  returned five product defects (F-13…F-17) and proved the Quick start's step 5
  claim that R.2 had to leave unproved.
- **Runs 2 and 3 passed, and reproduce.** From the same committed starting
  state: 5 unconnected items before, **0** after, **11** track segments both
  times, no errors, capacitors inside the 5 mm the prompt asked for — at
  different coordinates, which is what should happen.
- **The 40 s budget was wrong about what it measured**, and is published as two
  measured numbers instead: under a second of board changes, six to seven
  minutes of model conversation (R.3.10).
- **Two defects cost more than turns.** Run 3 turned F-16 and F-15 into false
  statements in the model's own final answer — that KiCad was installed nowhere,
  and that Konnect had fallen back to its file engine, while KiCad was running
  with that board open and the writes were reaching it over IPC. Both are fixed
  or disclosed in R.9, and neither reaches a user before v1.1.1.

## 4 — Promotion criteria, written before the evidence could select anything (R.6.1)

A candidate is promoted only when its criterion is met by evidence, not by
enthusiasm. Ten candidates, each with the one thing that would earn it a phase:

| Candidate | What would promote it |
|---|---|
| **Reach — publish the launch kit** | Nothing has to become true. It is the only candidate whose evidence *cannot* arrive until it is done: every other line on this table is waiting on users who do not exist yet |
| **v1.1.1** (R.7.7) | Already decided by the user. Promoted by definition: R.7, R.8, R.9.1, R.9.2 and F-03 reach nobody without it |
| **Nets on a board** (F-13, R.9.4) | A first-run report, or a demo attempt, that dies because the board has no netlist and no tool can give it one. One outside report of this is enough — it is the wall run 1 hit |
| **A *route this net* tool** (R.3.9) | A second measured run whose turn count is dominated by one-segment routing, *and* a stated intent to publish a demo under a time bound. Absent the time bound it is an efficiency wish, not a phase |
| **PCB reads over IPC** (F-15, R.9.3) | Two independent reports of a user or model believing a stale position. Run 3 is the first; one more from outside promotes it, because at that point the disclosure has demonstrably failed to prevent the confusion |
| **IPC placement matching the library** (F-17, R.9.5) | Any report where `lib_footprint_mismatch` blocks a fabrication export rather than merely appearing in DRC |
| **macOS signing and notarisation** | One first-run report from macOS that stops at Gatekeeper. Zero macOS downloads so far; buying a signing identity for an audience of nobody is the wrong order |
| **Linux QA against a running KiCad** | One Linux first-run report, or a Linux download that is not the maintainer's |
| **Official PCM submission** | v1.1.1 shipped (F-03 fixed) **and** one outside install that succeeded, so the listing points at software a stranger has actually run |
| **Dependabot hygiene** (8 open PRs) | A CVE with a reachable path in this codebase, or a PR that also unblocks something else. Eight open PRs on a project with no users is housekeeping, not risk |
| **KiCad 11 / plan item I.1** | KiCad 11's release date announced, *or* the SWIG Action Plugin path breaking. The plugin entry point disappears in 11 (F-11's other half), so this one has an external clock and is the only candidate that can promote itself without a user |

## 5 — Recommendation

**Publish. Then decide the rest with data instead of guesses.**

The evidence supports exactly one conclusion: every capability candidate on that
table is waiting on the same missing input, which is a user who is not the
maintainer. Nine of the eleven criteria above name a first-run report, an
outside download or an outside install. None of them can be satisfied by more
engineering, and choosing one of them now would mean choosing it *because it is
interesting*, which is what R.6 exists to prevent.

The order the evidence supports:

1. **Ship v1.1.1** (R.7.7, already decided). Every draft in the launch kit
   assumes the two manual configuration steps are gone; announcing before it
   sends the first wave down the path R.1 measured as the worst part of the
   experience.
2. **Apply the repository metadata and post the kit**, in the order the go/no-go
   list proposes. This is the only action that changes the input to every other
   decision.
3. **Re-open the gate when the tally is no longer zero** — a first-run report, a
   download that is not the maintainer's, an issue. At that point the criteria
   above select the next phase on their own, which is the point of writing them
   now.

**What not to do**: open a PCB-capability phase on run 1's five defects. They are
real, they are recorded, and four of them have criteria above — but the demo
passes without them, and R's own rule has held all phase: a capability is added
when a defect blocks first use or the public demo, and then minimally.

The decision is the user's. R closes here either way; nothing in this document
opens a phase.
