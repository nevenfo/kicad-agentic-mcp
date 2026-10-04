# Structural schematic scan fixture

`structural_scans_kicad10.kicad_sch` is Eeschema-authored evidence for the
three saved-schematic paths repaired by issue #84:

- wire endpoint repair sees a loose wire at `(190.54, 193.04)` and one
  geometric destination at `(190.5, 193.04)`, represented by both R3 pin 1
  and the `SNAP` label;
- datasheet enrichment sees R1's placed-instance `LCSC` property (`C25804`)
  and empty `Datasheet` property;
- `SchematicBuilder` sees the complete top-level KiCad section ordering and
  full library/symbol records.

The seed was the existing `junction_reconcile.kicad_sch` fixture, which was
built through Konnect against KiCad's stock `Device:R` library. The deliberate
wire, label, and custom-property cases were added, then the whole document was
parsed and force-resaved by KiCad 10.0.5:

```text
kicad-cli sch upgrade --force structural_scans_kicad10.kicad_sch
```

That final Eeschema serialization is committed here. It was reload-checked
with KiCad 10.0.5's netlist exporter:

```text
kicad-cli sch export netlist --output structural_scans.net structural_scans_kicad10.kicad_sch
```

The ignored real-KiCad E2E regression repeats the force-resave and netlist
export. The three focused module tests copy this same fixture, exercise their
respective mutation/serialization paths, and verify committed readback.
