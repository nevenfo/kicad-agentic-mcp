# gr_poly outline fixture provenance

`gr_poly_outline.kicad_pcb` is a minimal KiCad-authored board for the
`gr_poly`-recognition regression covered by #593. It exists so that
`board_outline_bbox` (`crates/konnect-sexp/src/board.rs`) and its placement
consumer are exercised against a real `gr_poly` Edge.Cuts outline as pcbnew
actually serializes one, rather than only against a rectangle produced by
string surgery on another fixture.

## Source and reduction

The outline is a real Edge.Cuts shape copied out of a large user project
board on which Konnect's placement scoring hit the bug this fixture
regression-tests (outline recognized only for `gr_line`/`gr_rect`/`gr_arc`/
`gr_circle`/`gr_curve`, so a `gr_poly`-outlined board was treated as having
no outline at all). The outline was pasted into a brand-new, otherwise-empty
PCB and the file was saved by KiCad's own `pcbnew`; no board content besides
that one outline and KiCad's standard board scaffolding (layer table, `setup`
block, `pcbplotparams`) was added.

- Board format version: `20260206`
- Generator / version: `pcbnew` / `10.0`
- SHA-256:
  `92c849c90a2c57293c75605d92b60275e617c552c6b395ccc68ba2d10c455f07`

## Shape

A single 12-vertex, non-rectangular (concave/notched) `gr_poly` on
`Edge.Cuts`, UUID `5702b6a5-7ccf-410a-9402-7e25b4e3fb76`. Its exact bbox,
computed by hand from the vertex list, is:

```
(103.42, 78.96) .. (154.48, 116.96)
```

This shape choice is deliberate: the concavity means the vertex-hull bbox is
still exact (straight edges), but the fixture is not reducible to a simple
rectangle-of-four-corners the way the earlier string-surgery test was —
catching mistakes that only show up on a real, irregular outline.

The committed file is therefore KiCad-authored serialization (copy-outline +
new board + KiCad save), not a hand-written test wrapper.
