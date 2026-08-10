//! `.kicad_pcb` → [`ItemSet`].
//!
//! Footprints, tracks, vias, zones and nets. Pads are not items of their own —
//! a 200-pin BGA would drown every other change — but they are counted, so a
//! net that gained two connections says so.

use super::sexp;
use kam_evidence::{Attr, Item, ItemSet};
use konnect_sexp::SexpNode;
use std::collections::BTreeMap;

pub(crate) fn extract(root: &SexpNode) -> ItemSet {
    let mut set = ItemSet::new();
    let mut pad_counts: BTreeMap<String, usize> = BTreeMap::new();

    for (ordinal, node) in root.find_all("footprint").into_iter().enumerate() {
        for pad in node.find_all("pad") {
            if let Some(net) = pad
                .find("net")
                .and_then(|n| n.get(1))
                .and_then(|n| n.as_str())
            {
                *pad_counts.entry(net.to_string()).or_insert(0) += 1;
            }
        }
        set.insert(footprint(node, ordinal));
    }
    for (ordinal, node) in root.find_all("segment").into_iter().enumerate() {
        set.insert(track(node, ordinal));
    }
    for (ordinal, node) in root.find_all("via").into_iter().enumerate() {
        set.insert(via(node, ordinal));
    }
    for (ordinal, node) in root.find_all("zone").into_iter().enumerate() {
        set.insert(zone(node, ordinal));
    }
    for node in root.find_all("net") {
        if let Some(item) = net(node, &pad_counts) {
            set.insert(item);
        }
    }

    set
}

/// A placed footprint, labelled by its reference designator.
fn footprint(node: &SexpNode, ordinal: usize) -> Item {
    let lib_id = node.get(1).and_then(|n| n.as_str()).unwrap_or("?");
    let reference = sexp::property(node, "Reference").unwrap_or(lib_id);
    let key = sexp::uuid(node).unwrap_or_else(|| sexp::positional_key("footprint", node, ordinal));

    let mut item = Item::new("footprint", key, reference).with("lib_id", Attr::text(lib_id));
    if let Some((x, y)) = sexp::at(node) {
        item = item.with("at", Attr::point(x, y));
    }
    if let Some(angle) = sexp::angle(node) {
        item = item.with("angle", Attr::Num(angle));
    }
    if let Some(layer) = node.find_str("layer") {
        item = item.with("layer", Attr::text(layer));
    }
    if let Some(value) = sexp::property(node, "Value") {
        item = item.with("value", Attr::text(value));
    }
    item
}

/// One routed segment. Labelled by its layer rather than by a designator,
/// because a track has no name a reviewer would recognise; the net it carries
/// is an attribute.
fn track(node: &SexpNode, ordinal: usize) -> Item {
    let ends = start_end(node);
    let layer = node.find_str("layer").unwrap_or("?");
    let key = sexp::uuid(node).unwrap_or_else(|| match ends {
        Some(((x1, y1), (x2, y2))) => format!("track@{x1:.4},{y1:.4}->{x2:.4},{y2:.4}/{layer}"),
        None => format!("track#{ordinal}"),
    });

    let mut item =
        Item::new("track", key, format!("track/{layer}")).with("layer", Attr::text(layer));
    if let Some(((x1, y1), (x2, y2))) = ends {
        item = item
            .with("from", Attr::point(x1, y1))
            .with("to", Attr::point(x2, y2));
    }
    if let Some(width) = node.find_f64("width") {
        item = item.with("width", Attr::Num(width));
    }
    if let Some(net) = node.find_f64("net") {
        item = item.with("net", Attr::Num(net));
    }
    item
}

fn start_end(node: &SexpNode) -> Option<((f64, f64), (f64, f64))> {
    let start = node.find("start")?;
    let end = node.find("end")?;
    Some((
        (start.get_f64(1)?, start.get_f64(2)?),
        (end.get_f64(1)?, end.get_f64(2)?),
    ))
}

fn via(node: &SexpNode, ordinal: usize) -> Item {
    let key = sexp::uuid(node).unwrap_or_else(|| sexp::positional_key("via", node, ordinal));
    let mut item = Item::new("via", key, "via");
    if let Some((x, y)) = sexp::at(node) {
        item = item.with("at", Attr::point(x, y));
    }
    if let Some(size) = node.find_f64("size") {
        item = item.with("size", Attr::Num(size));
    }
    if let Some(drill) = node.find_f64("drill") {
        item = item.with("drill", Attr::Num(drill));
    }
    if let Some(net) = node.find_f64("net") {
        item = item.with("net", Attr::Num(net));
    }
    item
}

/// A copper pour, labelled by the net it fills.
fn zone(node: &SexpNode, ordinal: usize) -> Item {
    let name = node.find_str("net_name").unwrap_or("zone");
    let key = sexp::uuid(node).unwrap_or_else(|| format!("zone:{name}#{ordinal}"));

    let mut item = Item::new("zone", key, format!("zone/{name}"));
    if let Some(layer) = node.find_str("layer") {
        item = item.with("layer", Attr::text(layer));
    }
    if let Some(net) = node.find_f64("net") {
        item = item.with("net", Attr::Num(net));
    }
    item
}

/// A declared net. Net 0 is KiCAD's unconnected pseudo-net and is skipped: its
/// pad count changes on every edit and means nothing to a reviewer.
fn net(node: &SexpNode, pad_counts: &BTreeMap<String, usize>) -> Option<Item> {
    let number = node.get(1)?.as_str()?;
    if number == "0" {
        return None;
    }
    let name = node.get(2).and_then(|n| n.as_str()).unwrap_or(number);
    let connections = pad_counts.get(number).copied().unwrap_or(0);
    Some(
        // Keyed by name, not by number: KiCAD renumbers nets on re-import, and
        // a diff that reports every net as replaced because the numbering
        // shifted is worse than no diff at all.
        Item::new("net", name, name).with("connections", Attr::Num(connections as f64)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use kam_evidence::Diff;

    fn parse(text: &str) -> ItemSet {
        extract(&konnect_sexp::parse_sexp(text).unwrap())
    }

    const BOARD: &str = r#"
(kicad_pcb
  (version 20260206)
  (net 0 "")
  (net 1 "GND")
  (net 2 "VDD3V3")
  (footprint "Resistor_SMD:R_0603" (layer "F.Cu") (uuid "f-1") (at 84 31 0)
    (property "Reference" "R1" (at 0 0 0))
    (property "Value" "10k" (at 0 0 0))
    (pad "1" smd rect (at -0.8 0) (size 0.9 0.9) (net 2 "VDD3V3") (uuid "p-1"))
    (pad "2" smd rect (at 0.8 0) (size 0.9 0.9) (net 1 "GND") (uuid "p-2")))
  (segment (start 84 31) (end 90 31) (width 0.25) (layer "F.Cu") (net 2) (uuid "t-1"))
  (via (at 90 31) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 2) (uuid "v-1"))
  (zone (net 1) (net_name "GND") (layer "B.Cu") (uuid "z-1")))
"#;

    #[test]
    fn a_board_extracts_its_footprints_tracks_vias_zones_and_nets() {
        let counts = parse(BOARD).counts_by_kind();
        assert_eq!(counts["footprint"], 1);
        assert_eq!(counts["track"], 1);
        assert_eq!(counts["via"], 1);
        assert_eq!(counts["zone"], 1);
        assert_eq!(counts["net"], 2, "net 0 is not a net");
    }

    #[test]
    fn pads_are_counted_not_listed() {
        let set = parse(BOARD);
        assert!(set.get("footprint", "p-1").is_none(), "pads are not items");
        assert_eq!(
            set.get("net", "VDD3V3").unwrap().attrs["connections"],
            Attr::Num(1.0)
        );
    }

    #[test]
    fn a_net_that_gains_a_connection_says_so() {
        let before = parse(BOARD);
        let after = parse(&BOARD.replace(
            r#"(pad "2" smd rect (at 0.8 0) (size 0.9 0.9) (net 1 "GND") (uuid "p-2"))"#,
            r#"(pad "2" smd rect (at 0.8 0) (size 0.9 0.9) (net 2 "VDD3V3") (uuid "p-2"))"#,
        ));
        let lines = Diff::compute(&before, &after).render_lines(10);
        assert!(
            lines.contains(&"GND connections: -1".to_string())
                && lines.contains(&"VDD3V3 connections: +1".to_string()),
            "got {lines:?}"
        );
    }

    #[test]
    fn renumbering_a_net_is_not_a_replacement() {
        let before = parse(BOARD);
        // Same design, nets declared in the other order — what a re-import does.
        let after = parse(&BOARD.replace(r#"(net 2 "VDD3V3")"#, r#"(net 7 "VDD3V3")"#));
        let touched = Diff::compute(&before, &after).touched_labels();
        assert!(
            !touched.contains(&"VDD3V3".to_string()),
            "a net keyed by name survives renumbering, got {touched:?}"
        );
    }

    #[test]
    fn moving_a_footprint_reads_as_a_move() {
        let before = parse(BOARD);
        let after =
            parse(&BOARD.replace("(uuid \"f-1\") (at 84 31 0)", "(uuid \"f-1\") (at 82 29 0)"));
        assert_eq!(
            Diff::compute(&before, &after).render_lines(5),
            vec!["R1 moved: (84,31) -> (82,29)"]
        );
    }

    #[test]
    fn adding_a_track_is_one_line() {
        let before = parse(BOARD);
        let after = parse(&BOARD.replace(
            r#"(zone (net 1)"#,
            r#"(segment (start 90 31) (end 90 40) (width 0.25) (layer "B.Cu") (net 1) (uuid "t-2"))
  (zone (net 1)"#,
        ));
        assert_eq!(
            Diff::compute(&before, &after).render_lines(5),
            vec!["track track/B.Cu added"]
        );
    }

    #[test]
    fn reserialising_a_board_is_not_a_design_change() {
        let before = parse(BOARD);
        let after = parse(&BOARD.replace("(at 84 31 0)", "(at 84.0000 31.0000 0)"));
        assert!(Diff::compute(&before, &after).is_empty());
    }
}
