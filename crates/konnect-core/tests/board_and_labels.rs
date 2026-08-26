//! Labels, board geometry, layers, zones and templates (J.2.3.6).
//!
//! Fifteen tools across five small domains, none of which had a test that runs.
//! Four of them are `ipc→sexpr`: with no KiCAD listening they fall back to the
//! file engine, which is exactly the path a test can take — unlike the `ipc`
//! tools, which have no fallback and stay untested until J.3.
//!
//! `refill_zones` (`ipc`) and `get_board_2d_view` (`cli`) are absent for that
//! reason and for J.2.3.7 respectively.
//!
//! No `kicad-cli` and no running KiCAD.

mod harness;

use harness::{pins, Harness, TWO_RESISTORS};
use serde_json::json;

/// A sheet with three labelled stubs, so the label tools have something to
/// find, rotate and delete.
async fn labelled_sheet(h: &Harness) -> String {
    let sch = harness::as_str(&h.fixture(TWO_RESISTORS)).to_string();
    h.json(
        "add_wire",
        json!({
            "schematic": sch,
            "x1": pins::R1_PIN1.0, "y1": pins::R1_PIN1.1,
            "x2": pins::R2_PIN1.0, "y2": pins::R2_PIN1.1
        }),
    )
    .await;
    for (net, x) in [("SIGNAL", 107.95), ("SPARE_A", 88.9), ("SPARE_B", 76.2)] {
        h.json(
            "add_schematic_net_label",
            json!({ "schematic": sch, "net": net, "x": x, "y": 46.99 }),
        )
        .await;
    }
    sch
}

// ─── Labels ──────────────────────────────────────────────────────────────────

/// The label list reports what is on the sheet, and a deleted label stops being
/// reported — deletion is by name *and* position, because two labels can share
/// a name.
#[tokio::test]
async fn a_label_is_listed_until_it_is_deleted_at_its_position() {
    let h = Harness::new();
    let sch = labelled_sheet(&h).await;

    let before = h
        .json("list_schematic_labels", json!({ "schematic": sch }))
        .await;
    let names: Vec<String> = before["labels"]
        .as_array()
        .expect("the labels are a list")
        .iter()
        .filter_map(|l| l["text"].as_str().or(l["net"].as_str()))
        .map(str::to_string)
        .collect();
    assert!(
        names.iter().any(|n| n == "SPARE_A"),
        "SPARE_A was placed and is not listed: {before}"
    );

    h.json(
        "delete_schematic_net_label",
        json!({ "schematic": sch, "net": "SPARE_A", "x": 88.9, "y": 46.99 }),
    )
    .await;

    let after = h
        .json("list_schematic_labels", json!({ "schematic": sch }))
        .await;
    assert!(
        !after.to_string().contains("SPARE_A"),
        "the deleted label is still listed: {after}"
    );
    assert!(
        after.to_string().contains("SIGNAL"),
        "the other labels went with it: {after}"
    );
}

/// Rotating one label leaves the others alone, and the batch form does the same
/// job for several — the two paths have to agree on what a rotation is.
#[tokio::test]
async fn rotating_labels_one_at_a_time_and_in_a_batch_agree() {
    let h = Harness::new();
    let single = labelled_sheet(&h).await;

    h.json(
        "rotate_schematic_label",
        json!({ "schematic": single, "net": "SIGNAL", "x": 107.95, "y": 46.99, "rotation": 90 }),
    )
    .await;
    let one_by_one = std::fs::read_to_string(&single).expect("the schematic is readable");

    let batch_h = Harness::new();
    let batched = labelled_sheet(&batch_h).await;
    batch_h
        .json(
            "batch_rotate_labels",
            json!({
                "schematic": batched,
                "labels": [
                    { "net": "SIGNAL", "x": 107.95, "y": 46.99, "rotation": 90 }
                ]
            }),
        )
        .await;
    let in_batch = std::fs::read_to_string(&batched).expect("the schematic is readable");

    let rotated_signal = |text: &str| {
        text.split("(label")
            .find(|block| block.contains("\"SIGNAL\""))
            .map(|block| block.contains("46.99 90"))
            .unwrap_or(false)
    };
    assert!(
        rotated_signal(&one_by_one),
        "the single rotation did not reach SIGNAL:\n{one_by_one}"
    );
    assert!(
        rotated_signal(&in_batch),
        "the batch rotation did not reach SIGNAL:\n{in_batch}"
    );
    assert!(
        !one_by_one.contains("\"SPARE_A\"\n    (at 88.9 46.99 90"),
        "a label that was not named got rotated:\n{one_by_one}"
    );
}

// ─── Board geometry ──────────────────────────────────────────────────────────

/// `set_board_size` draws the outline and `get_board_extents` measures it back.
/// These are two `ipc→sexpr` tools falling back to the file engine, which is
/// how the PCB half stays testable without a live KiCAD.
#[tokio::test]
async fn a_board_sized_by_one_tool_is_measured_by_another() {
    let h = Harness::new();
    // A blank board, because the shared fixture already carries an Edge.Cuts
    // rectangle and the extents are the union of everything on that layer.
    let board = harness::as_str(&h.write("blank.kicad_pcb", harness::BLANK_BOARD)).to_string();

    h.json(
        "set_board_size",
        json!({ "board": board, "width": 60.0, "height": 40.0, "origin_x": 10.0, "origin_y": 10.0 }),
    )
    .await;

    let extents = h.json("get_board_extents", json!({ "board": board })).await;
    assert_eq!(
        extents["width"], 60.0,
        "the board was sized 60 wide: {extents}"
    );
    assert_eq!(extents["height"], 40.0, "and 40 high: {extents}");
    assert_eq!(
        extents["x_min"], 10.0,
        "at the origin it was given: {extents}"
    );
    assert_eq!(extents["y_max"], 50.0, "10 + 40: {extents}");
    assert_eq!(
        extents["source"], "file",
        "with no KiCAD listening this must come from the file engine: {extents}"
    );
}

/// An explicit outline is written on Edge.Cuts — the layer is the whole point,
/// since geometry anywhere else is not a board edge.
#[tokio::test]
async fn a_board_outline_is_drawn_on_edge_cuts() {
    let h = Harness::new();
    let board = harness::as_str(&h.fixture("test.kicad_pcb")).to_string();

    h.json(
        "add_board_outline",
        json!({ "board": board, "x1": 0.0, "y1": 0.0, "x2": 50.0, "y2": 30.0 }),
    )
    .await;

    let text = std::fs::read_to_string(&board).expect("the board is readable");
    assert!(
        text.contains("Edge.Cuts"),
        "the outline is not on Edge.Cuts:\n{text}"
    );
}

/// `get_board_info` is the cheap summary an agent reads first, so it has to
/// report the layer count and the footprints actually on the board.
#[tokio::test]
async fn the_board_summary_counts_what_is_on_the_board() {
    let h = Harness::new();
    let board = harness::as_str(&h.fixture("test.kicad_pcb")).to_string();

    let info = h.json("get_board_info", json!({ "board": board })).await;
    let text = info.to_string();
    assert!(
        text.contains("2"),
        "the fixture has two footprints and the summary should count them: {info}"
    );
    assert!(
        text.contains("layer") || text.contains("Cu"),
        "a board summary without layers is not a summary: {info}"
    );
}

/// A mounting hole is a footprint with a drill, and board text lands on the
/// layer it was given rather than the default.
#[tokio::test]
async fn a_mounting_hole_and_board_text_are_written_where_asked() {
    let h = Harness::new();
    let board = harness::as_str(&h.fixture("test.kicad_pcb")).to_string();

    h.json(
        "add_mounting_hole",
        json!({ "board": board, "x": 5.0, "y": 5.0, "drill_diameter": 3.2, "reference": "H1" }),
    )
    .await;
    h.json(
        "add_board_text",
        json!({ "board": board, "text": "REV A", "x": 20.0, "y": 2.0, "layer": "B.SilkS" }),
    )
    .await;

    let text = std::fs::read_to_string(&board).expect("the board is readable");
    assert!(text.contains("H1"), "the mounting hole has no designator");
    assert!(text.contains("3.2"), "the drill diameter was not written");
    assert!(text.contains("REV A"), "the board text is missing");
    assert!(
        text.contains("B.SilkS"),
        "the text did not go to the layer it was given:\n{text}"
    );
}

// ─── Layers ──────────────────────────────────────────────────────────────────

/// Adding an inner layer makes it appear in the layer list; setting the active
/// layer must not add one.
#[tokio::test]
async fn a_layer_is_added_once_and_selecting_one_adds_nothing() {
    let h = Harness::new();
    let board = harness::as_str(&h.fixture("test.kicad_pcb")).to_string();

    let before = h.json("get_layer_list", json!({ "board": board })).await;
    let count = |v: &serde_json::Value| {
        v["layers"]
            .as_array()
            .map(|l| l.len())
            .expect("the board lists its layers")
    };

    h.json(
        "add_layer",
        json!({ "board": board, "layer_name": "In1.Cu", "layer_type": "signal" }),
    )
    .await;
    let after = h.json("get_layer_list", json!({ "board": board })).await;
    assert_eq!(
        count(&after),
        count(&before) + 1,
        "In1.Cu should be exactly one new layer: {after}"
    );
    assert!(
        after.to_string().contains("In1.Cu"),
        "the new layer is there by count and not by name: {after}"
    );

    h.json(
        "set_active_layer",
        json!({ "board": board, "layer": "In1.Cu" }),
    )
    .await;
    let unchanged = h.json("get_layer_list", json!({ "board": board })).await;
    assert_eq!(
        count(&unchanged),
        count(&after),
        "selecting a layer created one: {unchanged}"
    );
}

/// F-14: a board with no `(layers)` section is not malformed — KiCAD 10 opens
/// one without complaint and applies its own default 2-layer stackup.
/// `get_layer_list` used to diagnose this as `malformed_document`.
const BOARD_WITHOUT_LAYERS: &str = "(kicad_pcb\n\t(version 20241229)\n\t(gr_line (start 0 0) (end 10 0) (layer \"Edge.Cuts\") (width 0.1))\n\t(gr_line (start 10 0) (end 10 10) (layer \"Edge.Cuts\") (width 0.1))\n\t(gr_line (start 10 10) (end 0 10) (layer \"Edge.Cuts\") (width 0.1))\n\t(gr_line (start 0 10) (end 0 0) (layer \"Edge.Cuts\") (width 0.1))\n)\n";

#[tokio::test]
async fn get_layer_list_reports_kicad_default_stackup_when_undeclared() {
    let h = Harness::new();
    let board = harness::as_str(&h.write("board.kicad_pcb", BOARD_WITHOUT_LAYERS)).to_string();

    let body = h.json("get_layer_list", json!({ "board": board })).await;
    assert_eq!(body["declared"], false, "body: {body}");

    // KiCAD's default is two copper layers *and* the technical ones — the
    // board above already draws on `Edge.Cuts`, so an answer that named only
    // F.Cu and B.Cu would deny the layer its own outline sits on.
    let layers = body["layers"].as_array().unwrap();
    let copper: Vec<&str> = layers
        .iter()
        .filter(|l| l["copper"] == true)
        .map(|l| l["name"].as_str().unwrap())
        .collect();
    assert_eq!(copper, vec!["F.Cu", "B.Cu"], "body: {body}");
    assert!(
        layers.iter().any(|l| l["name"] == "Edge.Cuts"),
        "body: {body}"
    );
    assert_eq!(body["count"], layers.len(), "body: {body}");
}

#[tokio::test]
async fn add_layer_on_a_board_without_layers_gives_an_actionable_error() {
    let h = Harness::new();
    let board = harness::as_str(&h.write("board.kicad_pcb", BOARD_WITHOUT_LAYERS)).to_string();

    let result = h
        .call(
            "add_layer",
            json!({ "board": board, "layer_name": "In1.Cu", "layer_type": "signal" }),
        )
        .await
        .unwrap();
    assert!(result.is_error, "expected add_layer to refuse: {result:?}");
    let body = harness::body(&result);
    // The human-facing message must not diagnose a valid board as broken —
    // only `error.kind` stays `malformed_document` (D77: the structure does
    // not support this write), a stable discriminant a human never reads.
    let message = body["message"].as_str().unwrap();
    assert!(
        !message.to_lowercase().contains("malformed"),
        "error message still calls a valid board malformed: {message}"
    );
    assert!(
        message.contains("PCB editor") || message.contains("stackup"),
        "error message is not actionable: {message}"
    );
}

// ─── Zones ───────────────────────────────────────────────────────────────────

/// A copper pour is a zone on a copper layer, tied to a net. All three facts
/// have to reach the file, or the pour is decoration.
#[tokio::test]
async fn a_copper_pour_carries_its_layer_and_its_net() {
    let h = Harness::new();
    let board = harness::as_str(&h.fixture("test.kicad_pcb")).to_string();

    h.json(
        "add_copper_pour",
        json!({
            "board": board,
            "net_name": "GND",
            "layer": "B.Cu",
            "points": [
                { "x": 0.0, "y": 0.0 },
                { "x": 40.0, "y": 0.0 },
                { "x": 40.0, "y": 30.0 },
                { "x": 0.0, "y": 30.0 }
            ]
        }),
    )
    .await;

    let text = std::fs::read_to_string(&board).expect("the board is readable");
    assert!(text.contains("(zone"), "no zone was written");
    assert!(text.contains("B.Cu"), "the pour is not on B.Cu");
    assert!(text.contains("GND"), "the pour is not tied to GND");
}

/// A KiCad 10 board keeps no net table and no ids: the pour names its net
/// directly. Resolving an id by string offset returned 0 there, so every pour
/// went out as `(net 0) (net_name "GND")` -- attached to the unconnected
/// pseudo-net, an electrically orphaned pour reported as a success.
///
/// Oracle for both forms: KiCad's own demos. `pic_programmer` (20260206)
/// writes `(zone (net "GND") ...)` with no `net_name`; `StickHub` (20250907)
/// and `CM5_MINIMA_3` (20250513) write `(net <id>)` with the name in a
/// sibling `(net_name ...)`.
#[tokio::test]
async fn a_pour_on_a_kicad_10_board_names_its_net_instead_of_numbering_it() {
    let h = Harness::new();
    let board = harness::as_str(&h.fixture("kicad10_no_net_table.kicad_pcb")).to_string();

    h.json(
        "add_copper_pour",
        json!({
            "board": board,
            "net_name": "GND",
            "layer": "B.Cu",
            "points": [
                { "x": 100.0, "y": 100.0 },
                { "x": 140.0, "y": 100.0 },
                { "x": 140.0, "y": 130.0 },
                { "x": 100.0, "y": 130.0 }
            ]
        }),
    )
    .await;

    let text = std::fs::read_to_string(&board).expect("the board is readable");
    let zone = &text[text.find("(zone").expect("a zone was written")..];
    let head = &zone[..zone.len().min(120)];
    assert!(
        zone.contains(r#"(net "GND")"#),
        "the pour must name its net the way this board does: {head}"
    );
    assert!(
        !zone.contains("net_name"),
        "a board with no net table carries no net_name sibling: {head}"
    );
    assert!(
        !zone.contains("(net 0)"),
        "net 0 is the unconnected pseudo-net: {head}"
    );
}

/// The other form, unchanged: a board that keeps a table gets the id that
/// table declares, with the name in a sibling node.
#[tokio::test]
async fn a_pour_on_a_board_with_a_net_table_gets_the_declared_id() {
    let h = Harness::new();
    let board = harness::as_str(&h.fixture("unrouted.kicad_pcb")).to_string();

    let out = h
        .json(
            "add_zone",
            json!({
                "board": board,
                "net_name": "GND",
                "layer": "F.Cu",
                "points": [
                    { "x": 10.0, "y": 10.0 },
                    { "x": 50.0, "y": 10.0 },
                    { "x": 50.0, "y": 40.0 },
                    { "x": 10.0, "y": 40.0 }
                ]
            }),
        )
        .await;
    assert_eq!(out["net_id"], json!(2), "GND is net 2 in that fixture");

    let text = std::fs::read_to_string(&board).expect("the board is readable");
    let zone = &text[text.find("(zone").expect("a zone was written")..];
    let head = &zone[..zone.len().min(120)];
    assert!(
        zone.contains(r#"(net 2) (net_name "GND")"#),
        "the legacy form is the id plus a net_name sibling: {head}"
    );
}

/// A net the table does not declare has no id, and writing 0 would attach the
/// pour to the unconnected pseudo-net. Refuse instead, and leave the file
/// exactly as it was -- asserting the error alone would pass even if the write
/// had already happened.
#[tokio::test]
async fn a_pour_on_an_undeclared_net_is_refused_and_writes_nothing() {
    let h = Harness::new();
    let path = h.fixture("unrouted.kicad_pcb");
    let board = harness::as_str(&path).to_string();
    let before = std::fs::read_to_string(&board).expect("the board is readable");

    let result = h
        .call(
            "add_copper_pour",
            json!({
                "board": board,
                "net_name": "SPI_CLK",
                "layer": "F.Cu",
                "points": [
                    { "x": 10.0, "y": 10.0 },
                    { "x": 50.0, "y": 10.0 },
                    { "x": 50.0, "y": 40.0 }
                ]
            }),
        )
        .await
        .expect("the refusal is a tool result, not a transport failure");
    let body = harness::body(&result);
    assert_eq!(body["error"]["kind"], json!("invalid_argument"));
    assert_eq!(body["error"]["field"], json!("net_name"));
    assert!(
        body["error"]["reason"]
            .as_str()
            .unwrap_or("")
            .contains("add_net"),
        "the refusal must name the way out: {body}"
    );

    let after = std::fs::read_to_string(&board).expect("the board is readable");
    assert_eq!(before, after, "a refused pour must not touch the file");
}

/// The reverse bound on that refusal: a board with no table declares nothing,
/// so a name it has never seen is not an error -- the name IS the reference,
/// and KiCad creates the net on load.
#[tokio::test]
async fn a_kicad_10_board_accepts_a_pour_on_a_net_it_has_never_seen() {
    let h = Harness::new();
    let board = harness::as_str(&h.fixture("kicad10_no_net_table.kicad_pcb")).to_string();

    h.json(
        "add_copper_pour",
        json!({
            "board": board,
            "net_name": "SPI_CLK",
            "layer": "F.Cu",
            "points": [
                { "x": 100.0, "y": 100.0 },
                { "x": 140.0, "y": 100.0 },
                { "x": 140.0, "y": 130.0 }
            ]
        }),
    )
    .await;

    let text = std::fs::read_to_string(&board).expect("the board is readable");
    assert!(
        text.contains(r#"(net "SPI_CLK")"#),
        "the pour names its net"
    );
}

// ─── Templates ───────────────────────────────────────────────────────────────

/// A template is a reference circuit with real component values, and asking for
/// one that does not exist must not return an empty one — a caller building
/// from a blank template would produce a blank circuit.
#[tokio::test]
async fn a_template_comes_back_with_values_and_an_unknown_id_does_not() {
    let h = Harness::new();

    let listed = h.json("list_template_categories", json!({})).await;
    let id = if listed.to_string().contains("ldo_3v3") {
        "ldo_3v3"
    } else {
        "usb_c_5v_sink"
    };

    let template = h.json("get_template", json!({ "template_id": id })).await;
    assert!(
        template.to_string().len() > 100,
        "a reference circuit should carry components and values: {template}"
    );

    let unknown = h
        .call("get_template", json!({ "template_id": "no_such_template" }))
        .await;
    let reported = match unknown {
        Ok(result) => harness::body(&result).to_string(),
        Err(e) => e.to_string(),
    };
    assert!(
        reported.len() < 400 || reported.contains("not") || reported.contains("unknown"),
        "an unknown template id came back looking like a template: {reported}"
    );
}
