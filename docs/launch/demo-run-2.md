# Demo run 2 — the narrowed task, on a pre-state that carries its netlist

Run on 2026-08-26, immediately after `demo-run-1.md`, against the **published**
v1.1.0 binary (INV-R1), on the pre-state now committed in `examples/demo/`, with
the narrowed prompt committed beside it.

**The task succeeded. The clock did not.**

## Result

| | |
|---|---|
| Outcome | success — the model finished on its own, `is_error: false` |
| Wall clock | **377 s** — the budget is 40 s |
| Turns | 47 |
| Tool calls | 45, **one** of which was refused (a `Bash` command, blocked by the allowlist) |
| Cost | 1.83 USD |

## What KiCad says about the end state

Not what the model says — `kicad-cli`, run afterwards, on the file:

| | Pre-state | After |
|---|---|---|
| Unconnected items | **5** | **0** |
| Track segments | 0 | **11** |
| Violations | 1 warning (silkscreen clearance) | 3 warnings (2 × silkscreen overlap, 1 × silkscreen over copper) |

Measured from the board file, not from the reply:

| Ref | Position (mm) | Centre-to-centre from U1 |
|---|---|---|
| `U1` | 153.925, 90.200 | — |
| `C1` | 153.000, 94.950 | **4.839 mm** |
| `C2` | 153.000, 85.400 | **4.888 mm** |

The prompt asked for the capacitors within 5 mm. Both are, on the only axis that
fits: the model reported that the SOT-223 body is ±4.4 mm wide in X, so any
sideways placement lands at ≥ 5.8 mm. It placed them above and below instead.
Nets carry `/GND` on 10 items, `/VOUT` on 6, `/VIN` on 3 — the schematic's nets,
closed by copper.

Three silkscreen warnings remain, all reference-designator text overlapping a
neighbour. No errors, no unconnected items.

## Where the 377 seconds went

45 calls, and the shape of them is the finding:

```
 11  route_trace          one segment per call
  7  get_component_pads
  5  ToolSearch           (the client's own tool, not Konnect's)
  4  check_clearance
  3  Bash                 refused
  2  move_component
  2  save_project
  1  each: open_project, list_toolboxes, load_toolset, load_user_config,
          get_component_list, get_nets_list, run_drc, Glob, Read, Grep, Skill
```

The model routed **one segment per turn** — eleven turns of copper — and checked
its own work between placements. It never batched anything through
`kicad_invoke`, even though that tool exists and would have collapsed eleven
turns into one.

At the ~8–10 s per turn both runs measured, that is where the budget went. It is
not a slow product: the tool calls themselves answer in milliseconds
(R.3.1 measured two placements at 176 ms). **It is a slow conversation.**

## What this says about the 40 s budget

Run 1 failed because the task was impossible. Run 2 shows the task is now
possible, correct, and verified by KiCad — and still 9× over budget. Narrowing
the task further does not fix this: a single net would still cost a handful of
turns, and the floor for *any* prompt that requires the model to look before it
acts is well above 40 s.

The 40 s figure was chosen in R.3 before anything had been measured. What it
actually bounds is **model conversation time**, not product time. Two honest ways
out, and the choice belongs to the user:

1. **Keep 40 s and change what it measures** — time from the first write to the
   last, inside KiCad. That is the number the viewer watches, and it is
   sub-second here.
2. **State the real number.** A demo that says *one prompt, six minutes, and here
   is what KiCad says about the result* is not a weak demo. It is a truthful one,
   and R.4's launch kit is built on claims that survive checking.

What must not happen is quietly moving a published budget to fit a measurement
(INV6, D146).

**Decided by the user on 2026-08-26: publish both numbers.** The 40 s stops
being a budget; the demo states the product time and the conversation time side
by side, in `README.md` and `examples/demo/README.md`, with `demo-run-3.md`
carrying the per-call measurement that separates them.

## Anything else the run found

- Nothing new broke. F-13 through F-17 stand as run 1 recorded them; the
  netlist-carrying pre-state routes around F-13 rather than fixing it.
- The allowlist held again: the model's three `Bash` attempts were refused, so
  every change to the board went through KiCad.
- `route_trace` per segment is the only route to copper. There is no
  *route this net* tool — which is why eleven calls were needed for three nets.
  Recorded, not fixed here.
