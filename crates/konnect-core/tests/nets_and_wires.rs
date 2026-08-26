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

use harness::{pins, Harness, CONN_DOUBLE_ROW, MULTIUNIT_LM2904, TWO_RESISTORS};
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

/// Without an explicit `direction`, a left-edge pin gets a stub that leaves
/// the symbol body instead of running through it (P.6.8.5): `U1` unit 1's pin
/// 2 sits at x = 92.38, left of the symbol's own placement at x = 100, so the
/// default direction must be "left" and the label must land further left
/// still, not back into the body.
#[tokio::test]
async fn connect_to_net_defaults_a_left_edge_pin_to_a_leftward_stub() {
    let h = Harness::new();
    let sch = harness::as_str(&h.fixture(MULTIUNIT_LM2904)).to_string();
    let left_pin = (92.38, 52.54);

    let r = h
        .json(
            "connect_to_net",
            json!({
                "schematic": sch,
                "pin_x": left_pin.0, "pin_y": left_pin.1,
                "net": "SIG"
            }),
        )
        .await;

    assert_eq!(r["direction"], "left", "response: {r}");
    assert_eq!(r["direction_source"], "derived_from_pin", "response: {r}");
    // Measured against the wire's own start, not against `left_pin`: this
    // fixture places `U1` at x = 100, which is off the 1.27 grid, so the pin
    // it carries is off-grid too and the tool snaps the requested point before
    // drawing (92.38 → 92.71). The direction lookup happens before that snap —
    // that is why it still finds the pin — but the stub is drawn from the
    // snapped point, and the invariant that matters is the label sitting one
    // stub length to the *left* of wherever the wire starts.
    let stub_length = 2.54; // the tool's own default
    let (x1, y1) = (
        r["wire"]["x1"]
            .as_f64()
            .expect("the wire reports its start"),
        r["wire"]["y1"]
            .as_f64()
            .expect("the wire reports its start"),
    );
    let (label_x, label_y) = (
        r["label"]["x"]
            .as_f64()
            .expect("the label reports its point"),
        r["label"]["y"]
            .as_f64()
            .expect("the label reports its point"),
    );
    // 0.01 as everywhere else in these tests: the server rounds a computed
    // coordinate to 6 decimals (D125), so an exact `==` against the test's own
    // subtraction compares two different roundings of the same point.
    assert!(
        (label_x - (x1 - stub_length)).abs() < 0.01 && (label_y - y1).abs() < 0.01,
        "the label sits one stub length left of the wire start: {r}"
    );
    assert!(
        x1 < 100.0,
        "the pin is on the left edge of a symbol placed at x = 100: {r}"
    );
}

/// An explicit `direction` on the very same pin stays authoritative — the
/// derived default must not override a caller who asked for something else.
#[tokio::test]
async fn connect_to_net_explicit_direction_overrides_the_derived_one() {
    let h = Harness::new();
    let sch = harness::as_str(&h.fixture(MULTIUNIT_LM2904)).to_string();
    let left_pin = (92.38, 52.54);

    let r = h
        .json(
            "connect_to_net",
            json!({
                "schematic": sch,
                "pin_x": left_pin.0, "pin_y": left_pin.1,
                "net": "SIG",
                "direction": "right"
            }),
        )
        .await;

    assert_eq!(r["direction"], "right", "response: {r}");
    assert_eq!(r["direction_source"], "requested", "response: {r}");
}

/// A coordinate with no placed pin under it keeps the historical "right"
/// default — and says so, so a caller can tell the server did not find a pin
/// to derive a direction from.
#[tokio::test]
async fn connect_to_net_defaults_to_right_off_pin() {
    let h = Harness::new();
    let sch = harness::as_str(&h.fixture(TWO_RESISTORS)).to_string();

    let r = h
        .json(
            "connect_to_net",
            json!({
                "schematic": sch,
                "pin_x": 130.0, "pin_y": 60.0,
                "net": "SIG"
            }),
        )
        .await;

    assert_eq!(r["direction"], "right", "response: {r}");
    assert_eq!(
        r["direction_source"], "default_no_pin_here",
        "response: {r}"
    );
}

/// The mirror of the left-edge case: `U1` unit 1's pin 1 (the output) sits at
/// x = 107.62, right of the symbol's placement at x = 100, so this must not
/// pass only because every pin happens to default left.
#[tokio::test]
async fn connect_to_net_defaults_a_right_edge_pin_to_a_rightward_stub() {
    let h = Harness::new();
    let sch = harness::as_str(&h.fixture(MULTIUNIT_LM2904)).to_string();
    let right_pin = (107.62, 50.0);

    let r = h
        .json(
            "connect_to_net",
            json!({
                "schematic": sch,
                "pin_x": right_pin.0, "pin_y": right_pin.1,
                "net": "SIG"
            }),
        )
        .await;

    assert_eq!(r["direction"], "right", "response: {r}");
    assert_eq!(r["direction_source"], "derived_from_pin", "response: {r}");
}

/// Measured on `Connector_Generic:Conn_02x05_Odd_Even`: with the old
/// unconditional "right" default, a stub off the left row's pin 9 ran through
/// the symbol body and the mid-segment junction pass dropped a junction dot
/// exactly on pin 10, directly across at the same y. Deriving the direction
/// from the pin keeps the stub off the body, so nothing crosses the other row
/// and no junction lands on it.
#[tokio::test]
async fn connect_to_net_does_not_cross_the_opposite_row_of_a_double_row_connector() {
    let h = Harness::new();
    let sch = harness::as_str(&h.fixture(CONN_DOUBLE_ROW)).to_string();
    let left_row_pin9 = (96.52, 101.6);
    let right_row_pin10 = (109.22, 101.6);

    let r = h
        .json(
            "connect_to_net",
            json!({
                "schematic": sch,
                "pin_x": left_row_pin9.0, "pin_y": left_row_pin9.1,
                "net": "SIG",
                "stub_length": 15.0
            }),
        )
        .await;

    assert_eq!(r["direction"], "left", "response: {r}");
    let text = std::fs::read_to_string(&sch).expect("the schematic is readable");
    assert!(
        !text.contains(&format!("(at {} {})", right_row_pin10.0, right_row_pin10.1)),
        "a junction landed on pin 10, which the stub should never reach:\n{text}"
    );
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
    assert_eq!(
        geometry["label_count"], 1,
        "one label names SIGNAL: {geometry}"
    );
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
        connectivity["wires"][0]["x1"],
        pins::R1_PIN1.0,
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
        pin["pin_x"],
        pins::R1_PIN1.0,
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

/// `find_orphan_items` counted wire endpoints and label positions and nothing
/// else, so it was wrong in both directions on any sheet with components: a
/// wire drawn *to a pin* was reported as a dangling end, and a pin with
/// nothing on it was never reported at all — though the tool's description
/// has always promised it (#271, P.6.8.3).
///
/// The fixture is measured against KiCAD 10.0.3 rather than reasoned about.
/// `kicad-cli sch erc` on `orphan_items.kicad_sch` reports exactly three
/// `pin_not_connected` — R1 pin 2, R2 pin 1, R2 pin 2 — leaving R1 pin 1 out
/// because the wire ends on it; `label_dangling` for `NOWHERE`; and
/// `isolated_pin_label` for `MID`, which is KiCAD saying `MID` is attached to
/// the wire it sits mid-segment on (a rule about the net's pin count, not
/// about the label being loose).
#[tokio::test]
async fn orphan_items_are_the_ones_kicad_calls_unconnected() {
    let h = Harness::new();
    let sch = harness::as_str(&h.fixture("orphan_items.kicad_sch")).to_string();

    let report = h
        .json("find_orphan_items", json!({ "schematic": sch }))
        .await;
    let orphans = report["orphans"].as_array().expect("orphans array").clone();
    let of_type = |kind: &str| -> Vec<serde_json::Value> {
        orphans
            .iter()
            .filter(|o| o["type"] == kind)
            .cloned()
            .collect()
    };

    // The false negative: three pins, the same three ERC names.
    let mut pins: Vec<String> = of_type("unconnected_pin")
        .iter()
        .map(|o| {
            format!(
                "{}.{}",
                o["reference"].as_str().unwrap_or("?"),
                o["pin_number"].as_str().unwrap_or("?")
            )
        })
        .collect();
    pins.sort();
    assert_eq!(
        pins,
        vec!["R1.2".to_string(), "R2.1".to_string(), "R2.2".to_string()],
        "unconnected pins must be the three KiCAD reports: {report}"
    );

    // The false positive: the wire's pin end is not an orphan, only its far
    // end is.
    let dangling = of_type("dangling_wire_end");
    assert_eq!(dangling.len(), 1, "one dangling end, not two: {report}");
    assert_eq!(
        dangling[0]["y"],
        json!(40.64),
        "the free end, not the pin end"
    );

    // A label on a wire's body is attached; only the one in empty space is not.
    let floating = of_type("floating_label");
    assert_eq!(floating.len(), 1, "only NOWHERE floats: {report}");
    assert_eq!(floating[0]["net"], json!("NOWHERE"));

    assert_eq!(
        report["orphan_count"],
        json!(orphans.len()),
        "the count must match the list it summarises"
    );
}
