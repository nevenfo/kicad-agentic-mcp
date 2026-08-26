# Demo run 3 — the same starting state, a second time

Run on 2026-08-26, from the **committed** pre-state in `examples/demo/`, with the
same prompt, against the same **published** v1.1.0 binary (INV-R1) — the one that
does not yet carry R.9's fixes. This is R.3.7: the run that decides whether the
demo is reproducible or whether run 2 was a lucky take.

**It reproduces.** Same circuit, same verdict, different coordinates.

## Run 2 against run 3

| | Run 2 | Run 3 |
|---|---|---|
| Outcome | success | success |
| Wall clock | 377 s | **424 s** |
| Turns | 47 | **52** |
| Cost | 1.83 USD | **2.17 USD** |
| Unconnected items, before → after | 5 → **0** | 5 → **0** |
| Track segments | 11 | **11** |
| DRC errors | 0 | **0** |
| DRC warnings | 3, silkscreen | **5, silkscreen** |
| `C1` from `U1` | 4.839 mm | **4.71 mm** |
| `C2` from `U1` | 4.888 mm | **4.71 mm** |

What had to match, matched: three parts, three nets closed in copper, KiCad
reporting no unconnected item and no error. What did not match is the geometry —
run 3 put `C1` at (153.6, 94.9) where run 2 put it at (153.0, 94.95), and it
rotated `C2` by 180° to bring its ground pad to the regulator side, which run 2
did not do. That is the difference `examples/demo/README.md` predicts and asks
for: *what has to match is the circuit, not the pixels*.

Run 3 also found something run 2 did not: the SOT-223's tab is a separate pad
from pin 2 even though both carry `VOUT`, and KiCad demanded explicit copper
between them. It was the run's last DRC error, and the model closed it.

## Where the time went, and where it did not

The wall clock is the conversation. The product's share of it is measurable from
the same stream — the interval between each tool call and its result:

| | Run 2 | Run 3 |
|---|---|---|
| Konnect calls | 33 | 32 |
| **All Konnect calls, total** | **2.29 s** | **4.73 s** |
| of which `run_drc` (KiCad's own check) | 1.45 s (1 call) | 3.78 s (3 calls) |
| **Board-changing calls, total** | **0.686 s** (15 calls) | **0.773 s** (17 calls) |
| Slowest single write | 0.064 s | 0.072 s |
| `route_trace`, 11 calls | 0.44 s | 0.44 s |

Every change the demo makes to the board — two placements, one rotation, eleven
traces, the saves — lands in **under a second**, both times. Six to seven minutes
of wall clock buy about two seconds of work, and half of that two seconds is
KiCad running DRC.

This is what R.3.10 settles: the 40 s figure, written before anything was
measured, was never a product budget. Published as one, it would have measured a
model's turn count.

## What the run confirmed about R.9's findings

Run 3 met two of them live, on the published binary that does not yet carry the
fixes — and in both cases the cost was not a lost turn but a **wrong conclusion**
the model then reported to the user:

- **F-16** (fixed in R.9.1, unreleased). `launch_kicad_ui` answered `program not
  found`, and the model concluded, in its final answer, that *KiCad is installed
  nowhere on this machine* and that Konnect had *fallen back to its file engine*.
  Both are false: KiCad was running with this very board open, and the writes
  went to it over IPC — which is why the running editor held the traces that
  `kicad-cli` then verified. It spent four further turns trying to find KiCad
  with `Bash` and `PowerShell`; the allowlist refused every one.
- **F-15** (documented in R.9.3, unreleased). The model reported that
  `get_component_pads` *serves a stale cache*. It reads the board file, not a
  cache — but from where the caller stands the difference is invisible, which is
  exactly why the fix is to say `"source": "file"` out loud. The model worked
  around it with `get_component_list`, which reads over IPC.

Two runs, two different models, the same two dead ends. Neither stopped the demo;
both cost turns and produced statements about the product that were not true.

## Anything else

- The allowlist held for the third time: every `Bash` and `PowerShell` attempt was
  refused, so nothing but KiCad touched the board.
- The five remaining warnings are all silkscreen, and the model explained why
  rather than hiding them: the `U1` reference field is anchored 4.5 mm above the
  package centre, which is where the 5 mm placement constraint puts a capacitor.
  No placement satisfies both, and Konnect exposes no way to move a text field.
- Verification is `kicad-cli`'s, run afterwards on the file the running KiCad
  saved: 0 unconnected items, 11 segments, 5 warnings, **no errors**.
