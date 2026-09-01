//! W.2 — a footprint can be created *and then corrected* without leaving the
//! MCP.
//!
//! # What was missing
//!
//! `create_footprint` derives a footprint's courtyard, silkscreen, fab outline
//! and pin-1 marker from the pad layout it is handed. That is the right default
//! and it is not always right. Two real cases from the Hi-Fi benchmark, both
//! reproduced below:
//!
//! * a 5 mm-pitch film capacitor whose 3.5 mm body overran its 1.6 mm pads, so
//!   the courtyard — derived from the pads alone — came out 2.6 mm deep for a
//!   3.5 mm part. A courtyard smaller than the part is worse than no courtyard:
//!   DRC then reports clearance as satisfied while the bodies collide.
//! * a fuse, which is not polarised, given a pin-1 silk dot and a chamfered fab
//!   corner it has no business carrying — and the dot placed at x = -9.3 on a
//!   footprint whose courtyard stopped at -9.0.
//!
//! Neither could be fixed through the MCP: `edit_footprint_pad` touches pads
//! only, and the `pcb_*` toolsets operate on a `.kicad_pcb`, never a library
//! `.kicad_mod`. The recorded consequence was a choice between editing a
//! library file by hand and shipping a wrong footprint.
//!
//! # What this file proves
//!
//! The read/modify/write loop is closed: `get_footprint_info` reports each
//! primitive in exactly the shape `set_footprint_graphics` takes back, a
//! mutation is atomic and layer-scoped, and re-reading afterwards returns
//! precisely what was written. Then both Hi-Fi footprints, rebuilt from their
//! real dimensions, and corrected through the MCP alone.

mod harness;

use harness::Harness;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// A two-pad SMD footprint with generated graphics on several layers, so a
/// layer-scoped edit has neighbours it must not disturb.
async fn a_footprint(h: &Harness) -> PathBuf {
    let library = h.dir.path().join("Probe.pretty");
    std::fs::create_dir_all(&library).expect("the library directory is creatable");
    let footprint = library.join("PROBE.kicad_mod");
    h.json(
        "create_footprint",
        json!({
            "output": harness::as_str(&footprint),
            "name": "PROBE",
            "package_type": "smd",
            "body_width": 2.0, "body_height": 1.2,
            "pads": [
                { "number": "1", "type": "smd", "shape": "rect", "x": -0.8, "y": 0.0, "width": 0.9, "height": 0.95 },
                { "number": "2", "type": "smd", "shape": "rect", "x":  0.8, "y": 0.0, "width": 0.9, "height": 0.95 }
            ]
        }),
    )
    .await;
    footprint
}

/// The graphics `get_footprint_info` reports for one layer.
async fn graphics_on(h: &Harness, footprint: &Path, layer: &str) -> Vec<Value> {
    let info = h
        .json(
            "get_footprint_info",
            json!({"footprint_path": harness::as_str(footprint), "graphics_layer": layer}),
        )
        .await;
    info["graphics"]
        .as_array()
        .unwrap_or_else(|| panic!("get_footprint_info reports a graphics array: {info}"))
        .clone()
}

/// The vertical slice, end to end: read a footprint's graphics, change one
/// primitive, write, read back, and get exactly what was written.
#[tokio::test]
async fn a_graphic_can_be_read_changed_written_and_read_back_exactly() {
    let h = Harness::new();
    let footprint = a_footprint(&h).await;

    let before = graphics_on(&h, &footprint, "F.CrtYd").await;
    assert_eq!(
        before.len(),
        1,
        "the generator draws one courtyard: {before:?}"
    );
    assert_eq!(before[0]["type"], "rect");

    let widened = json!([{
        "type": "rect",
        "start": { "x": -3.0, "y": -2.0 },
        "end": { "x": 3.0, "y": 2.0 },
        "stroke_width_mm": 0.05,
        "fill": "none"
    }]);
    let written = h
        .json(
            "set_footprint_graphics",
            json!({
                "footprint_path": harness::as_str(&footprint),
                "selector": { "layer": "F.CrtYd" },
                "mode": "replace",
                "graphics": widened
            }),
        )
        .await;
    assert_eq!(written["matched_count"], 1, "{written}");
    assert_eq!(written["added_count"], 1, "{written}");

    let after = graphics_on(&h, &footprint, "F.CrtYd").await;
    assert_eq!(after.len(), 1, "{after:?}");
    assert_eq!(after[0]["type"], "rect");
    assert_eq!(after[0]["start"], json!({ "x": -3.0, "y": -2.0 }));
    assert_eq!(after[0]["end"], json!({ "x": 3.0, "y": 2.0 }));
    assert_eq!(after[0]["fill"], "none");
    assert_eq!(after[0]["stroke_width_mm"], 0.05);

    // KiCad still loads it: the file parses as a footprint and the pads it was
    // created with are untouched.
    let info = h
        .json(
            "get_footprint_info",
            json!({"footprint_path": harness::as_str(&footprint)}),
        )
        .await;
    assert_eq!(info["pad_count"], 2, "{info}");
    assert_eq!(info["has_courtyard"], true, "{info}");
}

/// One call touches one layer. Everything else in the file — the other layers'
/// graphics, the pads, the metadata — comes through byte for byte.
#[tokio::test]
async fn an_edit_is_scoped_to_its_layer_and_leaves_the_rest_of_the_file_alone() {
    let h = Harness::new();
    let footprint = a_footprint(&h).await;

    let source_before = std::fs::read_to_string(&footprint).expect("the footprint is readable");
    let silk_before = graphics_on(&h, &footprint, "F.SilkS").await;
    let fab_before = graphics_on(&h, &footprint, "F.Fab").await;
    assert!(!silk_before.is_empty() && !fab_before.is_empty());

    h.json(
        "set_footprint_graphics",
        json!({
            "footprint_path": harness::as_str(&footprint),
            "selector": { "layer": "F.CrtYd" },
            "mode": "replace",
            "graphics": [{
                "type": "rect",
                "start": { "x": -3.0, "y": -2.0 }, "end": { "x": 3.0, "y": 2.0 },
                "stroke_width_mm": 0.05, "fill": "none"
            }]
        }),
    )
    .await;

    assert_eq!(graphics_on(&h, &footprint, "F.SilkS").await, silk_before);
    assert_eq!(graphics_on(&h, &footprint, "F.Fab").await, fab_before);

    // Every line that is not the courtyard is unchanged, in order.
    let source_after = std::fs::read_to_string(&footprint).expect("the footprint is readable");
    let untouched = |source: &str| -> Vec<String> {
        source
            .lines()
            .filter(|line| !line.contains("F.CrtYd"))
            .map(str::to_string)
            .collect()
    };
    assert_eq!(untouched(&source_before), untouched(&source_after));
}

/// Every primitive kind a real footprint needs, appended and read back.
#[tokio::test]
async fn every_supported_primitive_round_trips() {
    let h = Harness::new();
    let footprint = a_footprint(&h).await;

    let graphics = json!([
        { "type": "line", "start": { "x": -2.0, "y": -1.0 }, "end": { "x": 2.0, "y": -1.0 }, "stroke_width_mm": 0.1 },
        { "type": "arc", "start": { "x": -1.0, "y": 0.0 }, "mid": { "x": 0.0, "y": -1.0 }, "end": { "x": 1.0, "y": 0.0 }, "stroke_width_mm": 0.1 },
        { "type": "rect", "start": { "x": -2.0, "y": -2.0 }, "end": { "x": 2.0, "y": 2.0 }, "stroke_width_mm": 0.1, "fill": "none" },
        { "type": "circle", "center": { "x": 0.0, "y": 0.0 }, "radius_mm": 0.5, "stroke_width_mm": 0.1, "fill": "solid" },
        { "type": "poly", "points": [{ "x": -1.0, "y": -1.0 }, { "x": 1.0, "y": -1.0 }, { "x": 0.0, "y": 1.0 }], "stroke_width_mm": 0.1, "fill": "none" }
    ]);

    let written = h
        .json(
            "set_footprint_graphics",
            json!({
                "footprint_path": harness::as_str(&footprint),
                "selector": { "layer": "Cmts.User" },
                "mode": "append",
                "graphics": graphics
            }),
        )
        .await;
    assert_eq!(written["added_count"], 5, "{written}");

    let read_back = graphics_on(&h, &footprint, "Cmts.User").await;
    let kinds: Vec<&str> = read_back
        .iter()
        .filter_map(|g| g["type"].as_str())
        .collect();
    assert_eq!(
        kinds,
        vec!["line", "arc", "rect", "circle", "poly"],
        "{read_back:?}"
    );

    assert_eq!(read_back[1]["mid"], json!({ "x": 0.0, "y": -1.0 }));
    assert_eq!(read_back[3]["radius_mm"], 0.5);
    assert_eq!(read_back[3]["fill"], "solid");
    assert_eq!(read_back[4]["point_count"], 3);
}

/// Deleting a layer's graphics removes them and nothing else.
#[tokio::test]
async fn deleting_a_layer_removes_only_that_layer() {
    let h = Harness::new();
    let footprint = a_footprint(&h).await;
    let silk_before = graphics_on(&h, &footprint, "F.SilkS").await;

    let deleted = h
        .json(
            "set_footprint_graphics",
            json!({
                "footprint_path": harness::as_str(&footprint),
                "selector": { "layer": "F.CrtYd" },
                "mode": "delete"
            }),
        )
        .await;
    assert_eq!(deleted["matched_count"], 1, "{deleted}");
    assert_eq!(deleted["added_count"], 0, "{deleted}");

    assert!(graphics_on(&h, &footprint, "F.CrtYd").await.is_empty());
    assert_eq!(graphics_on(&h, &footprint, "F.SilkS").await, silk_before);
}

/// A call the tool cannot honour changes nothing at all. Half-applying a
/// geometry edit would leave a footprint KiCad may still load and no one can
/// trust.
#[tokio::test]
async fn a_refused_call_leaves_the_footprint_byte_identical() {
    let h = Harness::new();
    let footprint = a_footprint(&h).await;
    let before = std::fs::read(&footprint).expect("the footprint is readable");

    let refusals = [
        // Not a KiCad layer name.
        json!({"selector": {"layer": "F.Nonsense"}, "mode": "replace", "graphics": [
            {"type": "line", "start": {"x": 0.0, "y": 0.0}, "end": {"x": 1.0, "y": 0.0}, "stroke_width_mm": 0.1}
        ]}),
        // A degenerate line: start and end coincide.
        json!({"selector": {"layer": "F.CrtYd"}, "mode": "replace", "graphics": [
            {"type": "line", "start": {"x": 1.0, "y": 1.0}, "end": {"x": 1.0, "y": 1.0}, "stroke_width_mm": 0.1}
        ]}),
        // A polygon with too few distinct points to enclose anything.
        json!({"selector": {"layer": "F.CrtYd"}, "mode": "replace", "graphics": [
            {"type": "poly", "points": [{"x": 0.0, "y": 0.0}, {"x": 1.0, "y": 0.0}], "stroke_width_mm": 0.1, "fill": "none"}
        ]}),
        // An unfilled shape with no stroke would be invisible.
        json!({"selector": {"layer": "F.CrtYd"}, "mode": "replace", "graphics": [
            {"type": "rect", "start": {"x": -1.0, "y": -1.0}, "end": {"x": 1.0, "y": 1.0}, "stroke_width_mm": 0.0, "fill": "none"}
        ]}),
        // Append with nothing to append.
        json!({"selector": {"layer": "F.CrtYd"}, "mode": "append", "graphics": []}),
    ];

    for refusal in refusals {
        let mut args = refusal.clone();
        args["footprint_path"] = json!(harness::as_str(&footprint));
        let result = h
            .call("set_footprint_graphics", args.clone())
            .await
            .expect("the tool answers rather than erroring out");
        assert!(result.is_error, "this call must be refused: {args}");
        assert_eq!(
            harness::body(&result)["error"]["kind"],
            "invalid_argument",
            "a malformed request is the caller's, not the world's: {args}"
        );
        assert_eq!(
            std::fs::read(&footprint).expect("the footprint is readable"),
            before,
            "a refused call must not touch the file: {args}"
        );
    }
}

/// A graphic a `(group …)` names cannot be replaced or deleted: dropping a
/// group member leaves KiCad holding a reference to something that is gone.
#[tokio::test]
async fn a_grouped_graphic_is_refused_rather_than_silently_dropped() {
    let h = Harness::new();
    let library = h.dir.path().join("Probe.pretty");
    std::fs::create_dir_all(&library).expect("the library directory is creatable");
    let footprint = library.join("GROUPED.kicad_mod");
    std::fs::write(
        &footprint,
        concat!(
            "(footprint \"GROUPED\"\n",
            "  (version 20240108)\n",
            "  (generator \"konnect\")\n",
            "  (layer \"F.Cu\")\n",
            "  (attr smd)\n",
            "  (fp_line (start -1 -1) (end 1 -1) (stroke (width 0.12) (type solid)) (layer \"F.SilkS\") (uuid \"aaaaaaaa-0000-4000-8000-000000000001\"))\n",
            "  (group \"outline\" (uuid \"bbbbbbbb-0000-4000-8000-000000000002\")\n",
            "    (members \"aaaaaaaa-0000-4000-8000-000000000001\")\n",
            "  )\n",
            ")\n"
        ),
    )
    .expect("the footprint is writable");
    let before = std::fs::read(&footprint).expect("the footprint is readable");

    for mode in ["replace", "delete"] {
        let mut args = json!({
            "footprint_path": harness::as_str(&footprint),
            "selector": { "layer": "F.SilkS" },
            "mode": mode
        });
        if mode == "replace" {
            args["graphics"] = json!([{
                "type": "line", "start": {"x": -2.0, "y": -2.0}, "end": {"x": 2.0, "y": -2.0},
                "stroke_width_mm": 0.12
            }]);
        }
        let result = h
            .call("set_footprint_graphics", args)
            .await
            .expect("the tool answers rather than erroring out");
        assert!(result.is_error, "'{mode}' must refuse a grouped graphic");
        assert_eq!(harness::body(&result)["error"]["kind"], "conflict");
        assert_eq!(
            std::fs::read(&footprint).expect("the footprint is readable"),
            before
        );
    }

    // Appending beside a grouped graphic is fine: nothing existing is touched.
    let appended = h
        .json(
            "set_footprint_graphics",
            json!({
                "footprint_path": harness::as_str(&footprint),
                "selector": { "layer": "F.SilkS" },
                "mode": "append",
                "graphics": [{
                    "type": "line", "start": {"x": -1.0, "y": 1.0}, "end": {"x": 1.0, "y": 1.0},
                    "stroke_width_mm": 0.12
                }]
            }),
        )
        .await;
    assert_eq!(appended["added_count"], 1, "{appended}");
    assert_eq!(graphics_on(&h, &footprint, "F.SilkS").await.len(), 2);
}

/// The first Hi-Fi footprint, rebuilt from its real dimensions: a 5 mm-pitch
/// film capacitor, body 7.2 x 3.5 mm, pads 1.6 mm square.
///
/// The recorded defect was a courtyard 2.6 mm deep around a 3.5 mm body.
#[tokio::test]
async fn the_film_capacitor_gets_a_courtyard_that_covers_its_body() {
    let h = Harness::new();
    let library = h.dir.path().join("HifiAmp_Local.pretty");
    std::fs::create_dir_all(&library).expect("the library directory is creatable");
    let footprint = library.join("CF_Film_Box_P5.00mm_7.2x3.5mm.kicad_mod");

    h.json(
        "create_footprint",
        json!({
            "output": harness::as_str(&footprint),
            "name": "CF_Film_Box_P5.00mm_7.2x3.5mm",
            "body_width": 7.2, "body_height": 3.5,
            "pads": [
                { "number": "1", "type": "thru_hole", "shape": "rect",   "x": -2.5, "y": 0.0, "width": 1.6, "height": 1.6, "drill": 0.8 },
                { "number": "2", "type": "thru_hole", "shape": "circle", "x":  2.5, "y": 0.0, "width": 1.6, "height": 1.6, "drill": 0.8 }
            ]
        }),
    )
    .await;

    let courtyard = graphics_on(&h, &footprint, "F.CrtYd").await;
    assert_eq!(courtyard.len(), 1, "{courtyard:?}");
    let depth = (courtyard[0]["end"]["y"].as_f64().unwrap()
        - courtyard[0]["start"]["y"].as_f64().unwrap())
    .abs();
    let width = (courtyard[0]["end"]["x"].as_f64().unwrap()
        - courtyard[0]["start"]["x"].as_f64().unwrap())
    .abs();
    assert!(
        depth >= 3.5,
        "the courtyard is {depth:.2} mm deep for a 3.5 mm body — the recorded defect was 2.6"
    );
    assert!(
        width >= 7.2,
        "the courtyard is {width:.2} mm wide for a 7.2 mm body"
    );
    // Through-hole pads take the 0.5 mm clearance, so the body plus clearance
    // on each side is the exact expected size.
    assert!((depth - 4.5).abs() < 1e-6, "depth = {depth}");
    assert!((width - 8.2).abs() < 1e-6, "width = {width}");
}

/// The second Hi-Fi footprint: a Schurter UMT-H fuse, whose land pattern is
/// taken off the datasheet drawing and which is not polarised.
///
/// Two recorded defects: a pin-1 mark on a part with no pin 1, and that mark's
/// silk dot placed outside the courtyard. Both are fixed here without the file
/// being opened by hand — first by creating it correctly, then by proving the
/// same correction is reachable on a footprint that already has the mark.
#[tokio::test]
async fn the_fuse_carries_no_false_pin1_mark_and_can_be_corrected_after_the_fact() {
    let h = Harness::new();
    let library = h.dir.path().join("HifiAmp_Local.pretty");
    std::fs::create_dir_all(&library).expect("the library directory is creatable");

    let pads = json!([
        { "number": "1", "type": "smd", "shape": "rect", "x": -6.875, "y": 0.0, "width": 3.75, "height": 5.6 },
        { "number": "2", "type": "smd", "shape": "rect", "x":  6.875, "y": 0.0, "width": 3.75, "height": 5.6 }
    ]);

    // ── Created correctly in one call ────────────────────────────────────────
    let clean = library.join("Fuse_Schurter_UMT-H_5.3x16mm.kicad_mod");
    h.json(
        "create_footprint",
        json!({
            "output": harness::as_str(&clean),
            "name": "Fuse_Schurter_UMT-H_5.3x16mm",
            "body_width": 15.4, "body_height": 5.35,
            "pin1_marker": false,
            "pads": pads
        }),
    )
    .await;

    let silk = graphics_on(&h, &clean, "F.SilkS").await;
    assert!(
        silk.iter().all(|g| g["type"] != "circle"),
        "a non-polarised part must carry no pin-1 dot: {silk:?}"
    );
    let fab = graphics_on(&h, &clean, "F.Fab").await;
    assert!(
        fab.iter().all(|g| g["type"] != "poly"),
        "a non-polarised part must carry no chamfered pin-1 corner: {fab:?}"
    );

    // ── The same defect, corrected after the fact ────────────────────────────
    // A footprint someone already generated with the mark, fixed through the
    // MCP alone: delete the silk dot, and square off the fab outline.
    let legacy = library.join("Fuse_Legacy.kicad_mod");
    h.json(
        "create_footprint",
        json!({
            "output": harness::as_str(&legacy),
            "name": "Fuse_Legacy",
            "body_width": 15.4, "body_height": 5.35,
            "pads": pads
        }),
    )
    .await;

    let marked_silk = graphics_on(&h, &legacy, "F.SilkS").await;
    assert!(
        marked_silk.iter().any(|g| g["type"] == "circle"),
        "the default still marks pin 1: {marked_silk:?}"
    );

    // The dot is inside the courtyard even before the correction — the
    // placement clamp. The recorded defect put it at x = -9.3 against a
    // courtyard edge of -9.0.
    let courtyard = graphics_on(&h, &legacy, "F.CrtYd").await;
    let (cx0, cx1) = (
        courtyard[0]["start"]["x"].as_f64().unwrap(),
        courtyard[0]["end"]["x"].as_f64().unwrap(),
    );
    let dot = marked_silk
        .iter()
        .find(|g| g["type"] == "circle")
        .expect("the pin-1 dot");
    let (dot_x, radius) = (
        dot["center"]["x"].as_f64().unwrap(),
        dot["radius_mm"].as_f64().unwrap(),
    );
    assert!(
        dot_x - radius >= cx0.min(cx1) - 1e-9 && dot_x + radius <= cx0.max(cx1) + 1e-9,
        "the pin-1 dot at {dot_x} ± {radius} escapes the courtyard [{cx0}, {cx1}]"
    );

    // Now remove it: keep the silk outline, drop the dot.
    let outline: Vec<Value> = marked_silk
        .iter()
        .filter(|g| g["type"] != "circle")
        .map(|g| {
            json!({
                "type": "rect",
                "start": g["start"], "end": g["end"],
                "stroke_width_mm": g["stroke_width_mm"],
                "fill": "none"
            })
        })
        .collect();
    h.json(
        "set_footprint_graphics",
        json!({
            "footprint_path": harness::as_str(&legacy),
            "selector": { "layer": "F.SilkS" },
            "mode": "replace",
            "graphics": outline
        }),
    )
    .await;

    // And square off the fab outline the chamfer left.
    h.json(
        "set_footprint_graphics",
        json!({
            "footprint_path": harness::as_str(&legacy),
            "selector": { "layer": "F.Fab" },
            "mode": "replace",
            "graphics": [{
                "type": "rect",
                "start": { "x": -7.7, "y": -2.675 }, "end": { "x": 7.7, "y": 2.675 },
                "stroke_width_mm": 0.1, "fill": "none"
            }]
        }),
    )
    .await;

    let fixed_silk = graphics_on(&h, &legacy, "F.SilkS").await;
    assert!(
        fixed_silk.iter().all(|g| g["type"] != "circle"),
        "the false pin-1 dot must be gone: {fixed_silk:?}"
    );
    let fixed_fab = graphics_on(&h, &legacy, "F.Fab").await;
    assert!(
        fixed_fab.iter().all(|g| g["type"] != "poly"),
        "the chamfer must be gone: {fixed_fab:?}"
    );

    // The corrected footprint is still a footprint, with its pads intact.
    let info = h
        .json(
            "get_footprint_info",
            json!({"footprint_path": harness::as_str(&legacy)}),
        )
        .await;
    assert_eq!(info["pad_count"], 2, "{info}");
    assert_eq!(info["has_courtyard"], true, "{info}");
}
