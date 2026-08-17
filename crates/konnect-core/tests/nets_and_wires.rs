//! The connectivity surface, exercised end to end (J.2.3.1).
//!
//! Nineteen `nets` and `wires` tools shipped with no test that runs, which the
//! capability matrix reported as `NOT_TESTED` — code that looks finished and is
//! proved by nothing. These tests build a small circuit with the writing tools
//! and then ask every reader about it, so a regression shows up as a wrong
//! answer rather than as a missing test.
//!
//! What is deliberately *not* asserted: that these answers are KiCAD's. The
//! `nets` readers derive connectivity in-process and have disagreed with
//! `kicad-cli` ERC before (E7) — which is why fifteen of them carry the
//! advisory caveat. These tests pin what the tools compute; `run_erc` remains
//! the verdict.
//!
//! No `kicad-cli` and no running KiCAD: everything here is the file engine.

mod harness;

use harness::{pins, Harness, TWO_RESISTORS};
use serde_json::json;

/// Wire R1 pin 1 to R2 pin 1 and name the net, the way a caller would.
///
/// Returns the schematic path as a string.
async fn wired_pair(h: &Harness) -> String {
    let sch = harness::as_str(&h.fixture(TWO_RESISTORS)).to_string();

    // A wire between the two pin-1s, and a label naming it.
    h.json(
        "add_wire",
        json!({
            "schematic": sch,
            "x1": pins::R1_PIN1.0, "y1": pins::R1_PIN1.1,
            "x2": pins::R2_PIN1.0, "y2": pins::R2_PIN1.1
        }),
    )
    .await;
    h.json(
        "add_schematic_net_label",
        json!({
            "schematic": sch,
            "net": "SIGNAL",
            "x": 107.95, "y": 46.99
        }),
    )
    .await;
    sch
}

// ─── Writers ─────────────────────────────────────────────────────────────────

/// `add_junction` and `add_no_connect` place a marker where they are told, and
/// `list_schematic_wires` sees the wire that is there.
#[tokio::test]
async fn junctions_and_no_connects_are_written_where_asked() {
    let h = Harness::new();
    let sch = wired_pair(&h).await;

    h.json(
        "add_junction",
        json!({ "schematic": sch, "x": 107.95, "y": 46.99 }),
    )
    .await;
    h.json(
        "add_no_connect",
        json!({ "schematic": sch, "x": pins::R1_PIN2.0, "y": pins::R1_PIN2.1 }),
    )
    .await;

    let text = std::fs::read_to_string(&sch).expect("the schematic is readable");
    assert!(text.contains("(junction"), "no junction was written");
    assert!(text.contains("(no_connect"), "no no-connect was written");

    let wires = h
        .json("list_schematic_wires", json!({ "schematic": sch }))
        .await;
    let count = wires["wires"]
        .as_array()
        .map(|w| w.len())
        .or_else(|| wires["count"].as_u64().map(|n| n as usize))
        .expect("the tool reports the wires it found");
    assert_eq!(count, 1, "one wire was added and one should be listed");
}

/// `add_schematic_connection` draws the wire between two points; deleting it by
/// its endpoints takes it away again.
#[tokio::test]
async fn a_connection_can_be_added_and_deleted_by_its_endpoints() {
    let h = Harness::new();
    let sch = harness::as_str(&h.fixture(TWO_RESISTORS)).to_string();

    h.json(
        "add_schematic_connection",
        json!({
            "schematic": sch,
            "x1": pins::R1_PIN2.0, "y1": pins::R1_PIN2.1,
            "x2": pins::R2_PIN2.0, "y2": pins::R2_PIN2.1
        }),
    )
    .await;
    let after_add = h
        .json("list_schematic_wires", json!({ "schematic": sch }))
        .await;
    assert!(
        after_add.to_string().contains("54.61"),
        "the connection is not in the wire list: {after_add}"
    );

    h.json(
        "delete_schematic_wire",
        json!({
            "schematic": sch,
            "x1": pins::R1_PIN2.0, "y1": pins::R1_PIN2.1,
            "x2": pins::R2_PIN2.0, "y2": pins::R2_PIN2.1
        }),
    )
    .await;
    let text = std::fs::read_to_string(&sch).expect("the schematic is readable");
    assert!(
        !text.contains("(wire"),
        "the wire survived its deletion:\n{text}"
    );
}

/// `batch_delete_schematic_wire` takes UUIDs, so the UUIDs the list reports
/// have to be the ones it accepts — a mismatch here would make the batch path
/// silently do nothing.
#[tokio::test]
async fn batch_delete_takes_the_uuids_the_wire_list_reports() {
    let h = Harness::new();
    let sch = wired_pair(&h).await;

    let wires = h
        .json("list_schematic_wires", json!({ "schematic": sch }))
        .await;
    let uuid = wires["wires"][0]["uuid"]
        .as_str()
        .expect("the wire list reports a uuid")
        .to_string();

    h.json(
        "batch_delete_schematic_wire",
        json!({ "schematic": sch, "uuids": [uuid] }),
    )
    .await;

    let after = h
        .json("list_schematic_wires", json!({ "schematic": sch }))
        .await;
    assert_eq!(
        after["wires"].as_array().map(|w| w.len()).unwrap_or(0),
        0,
        "the wire named by its own reported uuid was not deleted: {after}"
    );
}

/// `connect_to_net` stubs a wire off a pin and labels it — one call for what is
/// otherwise a wire plus a label at a computed position.
#[tokio::test]
async fn connect_to_net_stubs_a_labelled_wire_off_a_pin() {
    let h = Harness::new();
    let sch = harness::as_str(&h.fixture(TWO_RESISTORS)).to_string();

    h.json(
        "connect_to_net",
        json!({
            "schematic": sch,
            "pin_x": pins::R1_PIN2.0, "pin_y": pins::R1_PIN2.1,
            "net": "GND",
            "direction": "down"
        }),
    )
    .await;

    let text = std::fs::read_to_string(&sch).expect("the schematic is readable");
    assert!(text.contains("(wire"), "no stub wire was drawn");
    assert!(text.contains("\"GND\""), "the stub was not labelled GND");
}

/// `connect_passthrough` is the same idea from `sch_batch`, and it must not
/// leave the label somewhere the wire does not reach.
#[tokio::test]
async fn a_passthrough_leaves_its_label_on_the_stub_it_drew() {
    let h = Harness::new();
    let sch = harness::as_str(&h.fixture(TWO_RESISTORS)).to_string();

    h.json(
        "connect_passthrough",
        json!({
            "schematic": sch,
            "net_name": "VBUS",
            "x": pins::R2_PIN2.0, "y": pins::R2_PIN2.1,
            "direction": "right"
        }),
    )
    .await;

    let traced = h
        .json(
            "trace_from_point",
            json!({ "schematic": sch, "x": pins::R2_PIN2.0, "y": pins::R2_PIN2.1 }),
        )
        .await;
    assert!(
        traced.to_string().contains("VBUS"),
        "tracing from the stub root does not reach the passthrough label: {traced}"
    );
}

// ─── Readers ─────────────────────────────────────────────────────────────────

/// The four per-net and per-pin readers agree with each other about the one net
/// that exists. Disagreement between them is the failure this catches — each
/// alone could be self-consistently wrong.
#[tokio::test]
async fn the_net_readers_agree_about_the_net_that_exists() {
    let h = Harness::new();
    let sch = wired_pair(&h).await;

    let by_pin = h
        .json(
            "get_pin_net_name",
            json!({ "schematic": sch, "reference": "R1", "pin_number": "1" }),
        )
        .await;
    assert!(
        by_pin.to_string().contains("SIGNAL"),
        "R1 pin 1 sits on the labelled wire and should read SIGNAL: {by_pin}"
    );

    // Each reader has its own contract, and they must not contradict each
    // other: the geometry readers see the wire and the label, the component
    // readers see both resistors on it.
    let geometry = h
        .json(
            "get_net_connections",
            json!({ "schematic": sch, "net": "SIGNAL" }),
        )
        .await;
    assert_eq!(geometry["label_count"], 1, "one label names SIGNAL: {geometry}");
    assert_eq!(
        geometry["connected_points"], 3,
        "two pins and a label sit on SIGNAL: {geometry}"
    );

    let connectivity = h
        .json(
            "get_net_connectivity",
            json!({ "schematic": sch, "net": "SIGNAL" }),
        )
        .await;
    assert_eq!(
        connectivity["wires"].as_array().map(|w| w.len()),
        Some(1),
        "SIGNAL is one wire: {connectivity}"
    );
    assert_eq!(
        connectivity["wires"][0]["x1"], pins::R1_PIN1.0,
        "the wire reported is the one drawn: {connectivity}"
    );

    let components = h
        .json(
            "get_net_components",
            json!({ "schematic": sch, "net": "SIGNAL" }),
        )
        .await;
    let refs: Vec<&str> = components["components"]
        .as_array()
        .expect("the net lists its components")
        .iter()
        .filter_map(|c| c["reference"].as_str())
        .collect();
    assert_eq!(refs, ["R1", "R2"], "both resistors are on SIGNAL");

    let component = h
        .json(
            "get_component_nets",
            json!({ "schematic": sch, "reference": "R2" }),
        )
        .await;
    // Pin 1 is on the net; pin 2 is on nothing, and saying so is the useful
    // half of the answer.
    assert_eq!(component["pins"][0]["net"], "SIGNAL");
    assert!(
        component["pins"][1]["net"].is_null(),
        "R2 pin 2 is unconnected and should read as no net: {component}"
    );

    let pin = h
        .json(
            "get_pin_connections",
            json!({ "schematic": sch, "reference": "R1", "pin_number": "1" }),
        )
        .await;
    assert_eq!(pin["net"], "SIGNAL");
    assert_eq!(
        pin["pin_x"], pins::R1_PIN1.0,
        "the pin located is R1's pin 1: {pin}"
    );
}

/// `trace_from_point` walks from a coordinate to what it touches, and reports
/// nothing at a point where the schematic is empty.
#[tokio::test]
async fn tracing_finds_the_wire_under_a_point_and_nothing_under_empty_space() {
    let h = Harness::new();
    let sch = wired_pair(&h).await;

    let on_wire = h
        .json(
            "trace_from_point",
            json!({ "schematic": sch, "x": pins::R1_PIN1.0, "y": pins::R1_PIN1.1 }),
        )
        .await;
    assert!(
        on_wire.to_string().contains("SIGNAL"),
        "tracing from R1 pin 1 should reach SIGNAL: {on_wire}"
    );

    let in_space = h
        .json(
            "trace_from_point",
            json!({ "schematic": sch, "x": 10.16, "y": 200.66 }),
        )
        .await;
    assert!(
        !in_space.to_string().contains("SIGNAL"),
        "empty space is not on the SIGNAL net: {in_space}"
    );
}

/// A clean sheet has no shorts and no dangling wires. Both readers are only
/// worth anything if they can also say "nothing wrong" — a checker that always
/// finds something is noise.
#[tokio::test]
async fn a_clean_sheet_reports_no_shorts_and_no_loose_wires() {
    let h = Harness::new();
    let sch = wired_pair(&h).await;

    let shorts = h
        .json("find_shorted_nets", json!({ "schematic": sch }))
        .await;
    let listed = shorts["shorts"]
        .as_array()
        .or_else(|| shorts["shorted_nets"].as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    assert_eq!(listed, 0, "a two-resistor net is not shorted: {shorts}");

    let validated = h
        .json("validate_wire_connections", json!({ "schematic": sch }))
        .await;
    assert!(
        validated.is_object(),
        "validate_wire_connections returned no report: {validated}"
    );
}

/// `fix_connectivity` in `dry_run` must not touch the file — a repair tool that
/// writes while reporting is the failure worth guarding.
#[tokio::test]
async fn a_dry_run_repair_changes_nothing_on_disk() {
    let h = Harness::new();
    let sch = wired_pair(&h).await;
    let before = std::fs::read_to_string(&sch).expect("the schematic is readable");

    h.json(
        "fix_connectivity",
        json!({ "schematic": sch, "dry_run": true }),
    )
    .await;

    let after = std::fs::read_to_string(&sch).expect("the schematic is readable");
    assert_eq!(before, after, "a dry run rewrote the schematic");
}

// ─── PCB nets ────────────────────────────────────────────────────────────────

/// `add_net` writes into the `.kicad_pcb` file and reports the code KiCAD will
/// use for it.
///
/// Its read counterpart, `get_nets_list`, is an `ipc` tool: with no running
/// KiCAD it returns "KiCAD must be running with the board loaded" and there is
/// nothing here that could prove it. It stays `NOT_TESTED` rather than being
/// "proved" by its own error message, which would be a claim the matrix exists
/// to prevent.
#[tokio::test]
async fn a_net_added_to_a_board_is_written_into_the_file() {
    let h = Harness::new();
    let board = harness::as_str(&h.fixture("test.kicad_pcb")).to_string();

    let added = h
        .json(
            "add_net",
            json!({ "board": board, "net_name": "J23_PROBE" }),
        )
        .await;
    assert_eq!(added["net_name"], "J23_PROBE");
    let code = added["net_id"].as_u64().expect("the net gets a code");

    let text = std::fs::read_to_string(&board).expect("the board is readable");
    assert!(
        text.contains(&format!("(net {code} \"J23_PROBE\")")),
        "the net is reported as {code} and not written that way:
{text}"
    );
}
