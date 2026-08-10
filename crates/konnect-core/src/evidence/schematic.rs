//! `.kicad_sch` → [`ItemSet`].
//!
//! Symbols, wires, junctions, labels, sheets and no-connects. Deliberately
//! absent: `lib_symbols`, which is a copy of the library rather than a
//! statement about the design, and would report a library update as hundreds
//! of changes to a schematic nobody edited.

use super::sexp;
use kam_evidence::{Attr, Item, ItemSet};
use konnect_sexp::SexpNode;

/// Label-like nodes, each carrying its text as the first data child.
const LABEL_TAGS: &[&str] = &["label", "global_label", "hierarchical_label"];

/// Positional nodes with no interesting attributes beyond where they are.
const POINT_TAGS: &[&str] = &["junction", "no_connect"];

pub(crate) fn extract(root: &SexpNode) -> ItemSet {
    let mut set = ItemSet::new();

    for (ordinal, node) in root.find_all("symbol").into_iter().enumerate() {
        set.insert(symbol(node, ordinal));
    }
    for (ordinal, node) in root.find_all("wire").into_iter().enumerate() {
        set.insert(segment("wire", node, ordinal));
    }
    for (ordinal, node) in root.find_all("bus").into_iter().enumerate() {
        set.insert(segment("bus", node, ordinal));
    }
    for tag in LABEL_TAGS {
        for (ordinal, node) in root.find_all(tag).into_iter().enumerate() {
            set.insert(label(tag, node, ordinal));
        }
    }
    for tag in POINT_TAGS {
        for (ordinal, node) in root.find_all(tag).into_iter().enumerate() {
            set.insert(point_item(tag, node, ordinal));
        }
    }
    for (ordinal, node) in root.find_all("sheet").into_iter().enumerate() {
        set.insert(sheet(node, ordinal));
    }

    set
}

/// A placed component. `Reference` is the label because that is what a reviewer
/// says out loud; `lib_id` is an attribute because swapping the part behind a
/// reference is exactly the change worth reporting.
fn symbol(node: &SexpNode, ordinal: usize) -> Item {
    let lib_id = node.find_str("lib_id").unwrap_or("?");
    let reference = sexp::property(node, "Reference").unwrap_or(lib_id);
    let key = sexp::uuid(node).unwrap_or_else(|| sexp::positional_key("symbol", node, ordinal));

    let mut item = Item::new("symbol", key, reference).with("lib_id", Attr::text(lib_id));
    if let Some((x, y)) = sexp::at(node) {
        item = item.with("at", Attr::point(x, y));
    }
    if let Some(angle) = sexp::angle(node) {
        item = item.with("angle", Attr::Num(angle));
    }
    if let Some(value) = sexp::property(node, "Value") {
        item = item.with("value", Attr::text(value));
    }
    if let Some(footprint) = sexp::property(node, "Footprint") {
        // An unassigned footprint is written as an empty property. Reporting
        // "footprint: -" for every unassigned part would bury the assignments,
        // so absence stays absence.
        if !footprint.is_empty() {
            item = item.with("footprint", Attr::text(footprint));
        }
    }
    if let Some(unit) = node.find_f64("unit") {
        item = item.with("unit", Attr::Num(unit));
    }
    item
}

/// A wire or bus: two endpoints under `(pts (xy …) (xy …))`.
fn segment(kind: &str, node: &SexpNode, ordinal: usize) -> Item {
    let ends = endpoints(node);
    let key = sexp::uuid(node).unwrap_or_else(|| match ends {
        Some(((x1, y1), (x2, y2))) => format!("{kind}@{x1:.4},{y1:.4}->{x2:.4},{y2:.4}"),
        None => format!("{kind}#{ordinal}"),
    });

    let mut item = Item::new(kind, key, kind);
    if let Some(((x1, y1), (x2, y2))) = ends {
        item = item
            .with("from", Attr::point(x1, y1))
            .with("to", Attr::point(x2, y2));
    }
    item
}

fn endpoints(node: &SexpNode) -> Option<((f64, f64), (f64, f64))> {
    let pts = node.find("pts")?;
    let xy = pts.find_all("xy");
    let first = xy.first()?;
    let last = xy.last()?;
    Some((
        (first.get_f64(1)?, first.get_f64(2)?),
        (last.get_f64(1)?, last.get_f64(2)?),
    ))
}

/// A net name written on the sheet. The text is the label because a net called
/// `VDD3V3` is what the reviewer is looking for.
fn label(tag: &str, node: &SexpNode, ordinal: usize) -> Item {
    let text = node.get(1).and_then(|n| n.as_str()).unwrap_or("?");
    let key = sexp::uuid(node).unwrap_or_else(|| format!("{tag}:{text}#{ordinal}"));

    let mut item = Item::new(tag, key, text).with("text", Attr::text(text));
    if let Some((x, y)) = sexp::at(node) {
        item = item.with("at", Attr::point(x, y));
    }
    if let Some(shape) = node.find_str("shape") {
        item = item.with("shape", Attr::text(shape));
    }
    item
}

fn point_item(tag: &str, node: &SexpNode, ordinal: usize) -> Item {
    let key = sexp::uuid(node).unwrap_or_else(|| sexp::positional_key(tag, node, ordinal));
    let mut item = Item::new(tag, key, tag);
    if let Some((x, y)) = sexp::at(node) {
        item = item.with("at", Attr::point(x, y));
    }
    item
}

/// A hierarchical sheet instance.
fn sheet(node: &SexpNode, ordinal: usize) -> Item {
    let name = sexp::property(node, "Sheetname").unwrap_or("sheet");
    let key = sexp::uuid(node).unwrap_or_else(|| sexp::positional_key("sheet", node, ordinal));

    let mut item = Item::new("sheet", key, name);
    if let Some(file) = sexp::property(node, "Sheetfile") {
        item = item.with("file", Attr::text(file));
    }
    if let Some((x, y)) = sexp::at(node) {
        item = item.with("at", Attr::point(x, y));
    }
    item
}

#[cfg(test)]
mod tests {
    use super::*;
    use kam_evidence::Diff;

    fn parse(text: &str) -> ItemSet {
        extract(&konnect_sexp::parse_sexp(text).unwrap())
    }

    const DIVIDER: &str = r#"
(kicad_sch
  (version 20260306)
  (lib_symbols (symbol "Device:R" (property "Reference" "R" (at 0 0 0))))
  (junction (at 100 90) (uuid "j-1"))
  (wire (pts (xy 100 80) (xy 100 90)) (uuid "w-1"))
  (label "VOUT" (at 100 90 0) (uuid "l-1"))
  (symbol (lib_id "Device:R") (at 100 80 0) (unit 1) (uuid "s-1")
    (property "Reference" "R1" (at 102 79 0))
    (property "Value" "10k" (at 102 81 0))
    (property "Footprint" "" (at 100 80 0)))
  (symbol (lib_id "Device:R") (at 100 100 0) (unit 1) (uuid "s-2")
    (property "Reference" "R2" (at 102 99 0))
    (property "Value" "10k" (at 102 101 0))))
"#;

    #[test]
    fn a_divider_extracts_its_symbols_wires_labels_and_junctions() {
        let set = parse(DIVIDER);
        let counts = set.counts_by_kind();
        assert_eq!(counts["symbol"], 2, "lib_symbols must not be counted");
        assert_eq!(counts["wire"], 1);
        assert_eq!(counts["label"], 1);
        assert_eq!(counts["junction"], 1);
    }

    #[test]
    fn a_symbol_is_labelled_by_its_reference_and_carries_its_part() {
        let set = parse(DIVIDER);
        let r1 = set.get("symbol", "s-1").unwrap();
        assert_eq!(r1.label, "R1");
        assert_eq!(r1.attrs["lib_id"], Attr::text("Device:R"));
        assert_eq!(r1.attrs["value"], Attr::text("10k"));
        assert_eq!(r1.attrs["at"], Attr::point(100.0, 80.0));
    }

    #[test]
    fn an_empty_footprint_property_is_absence_not_a_value() {
        let set = parse(DIVIDER);
        assert!(!set
            .get("symbol", "s-1")
            .unwrap()
            .attrs
            .contains_key("footprint"));
    }

    #[test]
    fn assigning_a_footprint_is_one_line_of_diff() {
        let before = parse(DIVIDER);
        let after = parse(&DIVIDER.replace(
            r#"(property "Footprint" "" (at 100 80 0))"#,
            r#"(property "Footprint" "Resistor_SMD:R_0603_1608Metric" (at 100 80 0))"#,
        ));
        let diff = Diff::compute(&before, &after);
        assert_eq!(
            diff.render_lines(5),
            vec!["R1 footprint: +Resistor_SMD:R_0603_1608Metric"]
        );
    }

    #[test]
    fn moving_a_symbol_reports_the_move_and_nothing_else() {
        let before = parse(DIVIDER);
        let after = parse(&DIVIDER.replace("(at 100 80 0) (unit 1)", "(at 98 78 0) (unit 1)"));
        let diff = Diff::compute(&before, &after);
        assert_eq!(diff.render_lines(5), vec!["R1 moved: (100,80) -> (98,78)"]);
    }

    #[test]
    fn reserialising_a_document_is_not_a_design_change() {
        let before = parse(DIVIDER);
        // Same design, floats written the long way and whitespace reflowed —
        // what KiCAD's own writer does on save.
        let after = parse(&DIVIDER.replace("(at 100 80 0)", "(at 100.0000 80.0000 0.0)"));
        assert!(
            Diff::compute(&before, &after).is_empty(),
            "formatting must not read as an edit"
        );
    }

    #[test]
    fn a_swapped_part_behind_the_same_reference_is_visible() {
        let before = parse(DIVIDER);
        let after = parse(&DIVIDER.replacen(
            r#"(symbol (lib_id "Device:R") (at 100 80 0)"#,
            r#"(symbol (lib_id "Device:R_Small") (at 100 80 0)"#,
            1,
        ));
        assert_eq!(
            Diff::compute(&before, &after).render_lines(5),
            vec!["R1 lib_id: Device:R -> Device:R_Small"]
        );
    }

    #[test]
    fn a_wire_without_a_uuid_still_gets_a_stable_key() {
        let a = parse(r#"(kicad_sch (wire (pts (xy 1 2) (xy 3 4))))"#);
        let b = parse(r#"(kicad_sch (wire (pts (xy 1 2) (xy 3 4))))"#);
        assert!(Diff::compute(&a, &b).is_empty());
    }

    #[test]
    fn a_label_is_named_by_its_text() {
        let set =
            parse(r#"(kicad_sch (global_label "VDD3V3" (shape input) (at 1 2 0) (uuid "g-1")))"#);
        let item = set.get("global_label", "g-1").unwrap();
        assert_eq!(item.label, "VDD3V3");
        assert_eq!(item.attrs["shape"], Attr::text("input"));
    }

    #[test]
    fn a_sheet_is_named_by_its_sheetname() {
        let set = parse(
            r#"(kicad_sch (sheet (at 10 20) (uuid "sh-1")
                 (property "Sheetname" "Power" (at 10 19 0))
                 (property "Sheetfile" "power.kicad_sch" (at 10 21 0))))"#,
        );
        let item = set.get("sheet", "sh-1").unwrap();
        assert_eq!(item.label, "Power");
        assert_eq!(item.attrs["file"], Attr::text("power.kicad_sch"));
    }
}
