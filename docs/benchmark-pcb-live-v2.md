# PCB live benchmark V2 — fork against Konnect v0.13.0

Five live PCB workflows, the same intention given to both servers, **KiCad as the
referee**. It replaces the August comparison against Konnect v0.2.2 as the basis
for any claim about this fork versus upstream: that one measured token cost on
scripted schematic tasks, at the fork point, and contains no live PCB workload.

Harness: `bench/pcb_live_v2/` (`run.py`, `kicad_session.py`, `report.py`,
fixtures and their generators). Raw results: `bench/results/pcb_live_v2-*.json`.

## Environment

| | |
|---|---|
| KiCad | 10.0.6 (latest stable on 2026-10-04; 10.0.7 is at RC2) |
| OS | Windows 11 Pro 26200 |
| fork | branch `ai/pcb-live-bench-v2`; baseline binary built from v1.2.0 sources (`48465f1`), later runs from the changes below; SHA-256 of each binary is in every result file |
| upstream | `mixelpixx/Konnect` tag **v0.13.0**, commit `6bbe3e4f890ba1d37c0e5d5f38ccd03d90958c9e` (release 2026-10-02), built unmodified |
| live oracle | `kicad-python` (`kipy`) 0.8.0 — KiCad's official IPC binding, shared with neither server |
| saved oracle | `kicad-cli` 10.0.6 (`pcb drc`, `--schematic-parity`), plus the board file as KiCad wrote it |

Upstream `main` moved after the tag (`3ad01f8`, 2026-10-04: e.g. #805 closing
#791, project-file writers vs a running KiCad). None of those commits touches
the five workflows below; the baseline stays the tag.

## Method

* Each run copies a fixture into a fresh directory, starts `pcbnew` on it with a
  private profile, lets **one** server carry out the intention by whatever calls
  it needs, then judges the final KiCad state. Servers may use different call
  sequences; what is compared is *intention → observable KiCad state*.
* Edits that stand in for a GUI user (a footprint moved and not saved, a value
  changed) are made by the oracle side through `kipy`, never by the server.
* `success: true` is never evidence. A run is **functional** only if the live
  read-back and the saved-file checks agree with the intention. A server that
  claimed success on a non-functional run scores a **false success**; one that
  refused, or has no tool for the intention, made no claim.
* **Every oracle has two controls**, injected by the oracle side: the correct
  end state (must pass) and a known-wrong one (must fail). A scenario whose
  controls do not separate is reported invalid and not scored. All five
  separated on every run.
* 3 runs per scenario per implementation.

| | Intention | Oracle | Wrong-result control |
|---|---|---|---|
| A | schematic changed (R2 10k→4.7k, new unconnected R3): bring the open board up to date | `kipy`: R1/R2/H1/H2 positions and layer, tracks identical, R2 = 4.7k, R3 present; `kicad-cli` parity = 0 | naive sync that applies the value and drops board-only H1 |
| B | R2 moved live, not saved: route GND R1.2 → R2.1 | `kipy`: copper chain to R2.1's **live** pad, R2 left where moved; saved DRC: 1 unconnected, no `track_dangling`/`shorting_items` | track drawn to R2.1's saved position |
| C | GND copper zone present; supply renamed +5V in the schematic, so unrouted pad R1.1 changes net | `kipy`: R1.1 on +5V, zone and tracks unchanged, placement kept; parity = 0 | nothing applied |
| D | C310/C311 to B.Cu in place, then C310.1 → via → U1.1 (PVDD) | `kipy`: layer, XY kept, pads mirrored (top/bottom or left/right); pad copper side read from the file KiCad saved; PVDD chain pad → via → pad; DRC: 3 unconnected, no dangling/short/clearance | copper started at C310.1's pre-flip position |
| E | is the board in parity with its schematic? asked diverged, then restored | `kicad-cli pcb drc --schematic-parity` run by the oracle: 1 then 0; server must report >0 then a measured 0 | a reporter that always answers 0 |

## Results — baseline (fork v1.2.0 vs upstream v0.13.0)

`bench/results/pcb_live_v2-baseline-20261004.json`. Every cell was identical on
all three runs; no harness error.

| | fork: functional | fork: false success | upstream: functional | upstream: false success | notes |
|---|---|---|---|---|---|
| A | 0/3 | 0/3 | **3/3** | 0/3 | fork has no sync tool. Upstream applies the delta and keeps placement, copper and H1/H2; it places the new R3 at (5.93, 0.5), outside the board outline |
| B | 0/3 | **3/3** | **3/3** | 0/3 | fork reads pad positions from the saved file and draws to R2's old place, answering `routed: true`; upstream reads the live board (`source: ipc`) |
| C | 0/3 | 0/3 | 0/3 | 0/3 | fork has no sync tool. Upstream **refuses** (`routed_pad_net_change`: "routed copper uses that net") although +3V3 carries no copper: the GND zone makes every net look routed — upstream issue #779, open |
| D | **3/3** | 0/3 | **3/3** | 0/3 | same correct end state. Fork refuses the flip while KiCad holds the board, so the editor must be closed and reopened (3/3 runs); its message said KiCad has no IPC flip, which is false on 10.0.6. Upstream flips over IPC |
| E | 0/3 | **3/3** | **3/3** | 0/3 | fork's `run_drc` never passes `--schematic-parity`; KiCad 10 then writes an empty array and the fork reports 0 while the referee finds 1. Upstream reports 1, then 0 |

MCP tool calls per run (same on all runs): A fork 1 / upstream 3; B 2 / 2;
C 1 / 2; D 10 / 9; E 3 / 3. Median wall clock 5–9 s per run, dominated by
starting `pcbnew`; D costs the fork +3.7 s for the editor restart. External
tokens were not compared: the scripted calls are fixed, so the bytes reflect
response verbosity, not an agent's cost.

## Classification

| capability | verdict | basis |
|---|---|---|
| route between pads with an unsaved live edit (B) | **upstream better — critical fork defect** | false success 3/3, = upstream #700, fixed upstream before v0.13.0 |
| schematic-parity check (E) | **upstream better — critical fork defect** | false green 3/3 |
| schematic → PCB sync, additions/updates (A) | **upstream better** | fork has no capability |
| sync with a net change on a zoned board (C) | **neither — upstream defect (fail-closed)** | upstream refuses wrongly, no corruption; fork absent |
| flip + via layer change (D) | **equivalent end state; upstream better on intervention** | both functional 3/3; fork needs an editor restart |

**Facts** are the table above. **Interpretations**: the two fork defects are
the same class the Hi-Fi stress test met — an answer computed from the saved
file or from a check that never ran, presented as a live truth. Upstream's C
failure is the safe direction: it refuses rather than corrupting.
**Uncertainties**: one machine, one KiCad version, two-to-five-part fixtures;
the scripted agent picks a fixed via location and fixed call sequences, so this
measures primitives, not planning; nothing here measures the 77 `UNPROVEN`
capabilities outside these five workflows.

## Decisions taken from these results

* **B** — backport: `route_pad_to_pad` reads both pads from the board KiCad has
  open, in the same guarded IPC call that writes the tracks; no silent file
  fallback.
* **E** — backport: `run_drc` always requests `--schematic-parity`, and a parity
  test KiCad says it could not run is reported as unchecked (`null` plus a
  diagnostic), never as 0.
* **D** — the false message is corrected. Driving KiCad's `FlipItems` needs a
  newer vendored proto; not done in this change, because the end state is
  already correct and the cost is one editor restart.
* **A / C** — `update_pcb_from_schematic` ported from v0.13.0 (selective import,
  no rebase): the prerequisite phase X9 named — a live suite to prove it — now
  exists, A and C. While porting, upstream #779 is fixed in the fork: a copper
  zone's net is readable (`Zone.settings.copper_settings.net`), so only that net
  counts as routed; a zone whose net cannot be read keeps the fail-closed
  fallback. Its capability-matrix row stays `UNPROVEN` — the proof rule wants a
  live read-back test inside the Rust suite, and this Python benchmark does not
  count toward it.
* Everything else upstream adds was left out: these five workflows give no
  measured reason to import it.

## Results — after the changes

`bench/results/pcb_live_v2-after-z5-20261004.json` (B, D, E after the two
backports) and `bench/results/pcb_live_v2-after-z7-20261004.json` (all five,
after the sync port; the fork binary's SHA-256 is in the file — it was built
from the working tree on top of `48a19d6`). Controls separated on every
scenario; every cell identical on its three runs.

| | fork: functional | fork: false success | upstream v0.13.0: functional | notes |
|---|---|---|---|---|
| A | **3/3** | 0/3 | 3/3 | both place the new R3 outside the board outline, at (5.93, 0.5) |
| B | **3/3** | 0/3 | 3/3 | |
| C | **3/3** | 0/3 | 0/3 (refused) | fork better: #779 fixed |
| D | 3/3 | 0/3 | 3/3 | fork still needs an editor restart for the flip |
| E | **3/3** | 0/3 | 3/3 | |

So, on these five live PCB workflows with KiCad as the referee: the fork now
matches upstream v0.13.0 on A, B and E, is ahead on C, and behind on D's
manual step. Before these changes it had two false successes (B, E) and two
missing capabilities (A, C). Nothing measured here covers the rest of either
surface.

No general superiority is claimed in either direction: on these five workflows,
at these versions, the tables say where each wins, loses or fails.
