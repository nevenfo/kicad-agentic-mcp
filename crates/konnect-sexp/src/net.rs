//! Net accessors that read both KiCAD `(net …)` forms.
//!
//! KiCAD ≤ 20250907 writes a net table at the top of the board and refers to
//! nets from items by id: `(net <id> "<name>")`. KiCAD 20260206 dropped the
//! table entirely and writes the name directly on the item: `(net "<name>")`.
//! Reading the name at a fixed child index only works for the first form; on
//! the second it silently returns the id string, or nothing (upstream #142).
//!
//! The two forms are told apart by shape, never by a version-number
//! threshold: `get(1)` is `SexpNode::Str` on the id-less form (the name comes
//! first) and `SexpNode::Atom` on the id-bearing form (an integer comes
//! first, the name is at index 2).

use crate::parser::SexpNode;

/// Read the net name from a `(net …)` node, in either form. `None` if `node`
/// is not a `(net …)` list.
pub fn net_name(node: &SexpNode) -> Option<&str> {
    if node.head() != Some("net") {
        return None;
    }
    match node.get(1)? {
        SexpNode::Str(s) => Some(s.as_str()),
        SexpNode::Atom(_) => node.get(2)?.as_str(),
        SexpNode::List(_) => None,
    }
}

/// Read the net id from a `(net <id> "<name>")` node. `None` on the id-less
/// form, or if `node` is not a `(net …)` list.
pub fn net_id(node: &SexpNode) -> Option<i32> {
    if node.head() != Some("net") {
        return None;
    }
    match node.get(1)? {
        SexpNode::Atom(s) => s.parse().ok(),
        _ => None,
    }
}

/// Whether the board carries a net table: at least one `(net <id> …)`
/// declaration as a direct child of the root. This is the discriminant a
/// writer needs before inserting a new table entry — it measures the thing
/// the insertion actually depends on, rather than guessing from a version.
pub fn board_uses_net_table(tree: &SexpNode) -> bool {
    tree.find_all("net").iter().any(|n| net_id(n).is_some())
}

/// Count of distinct, non-empty net names seen anywhere in the board — the
/// table (old form) and/or every item's `(net …)` (both forms), since item
/// nets live nested inside footprints/pads rather than as direct children of
/// the root.
pub fn count_distinct_nets(tree: &SexpNode) -> usize {
    let mut names: Vec<&str> = Vec::new();
    collect_net_names(tree, &mut names);
    names.sort_unstable();
    names.dedup();
    names.len()
}

fn collect_net_names<'a>(node: &'a SexpNode, out: &mut Vec<&'a str>) {
    let Some(children) = node.children() else {
        return;
    };
    if node.head() == Some("net") {
        if let Some(name) = net_name(node) {
            if !name.is_empty() {
                out.push(name);
            }
        }
    }
    for child in children {
        collect_net_names(child, out);
    }
}

/// Next free net id: one past the highest id in the table, or `1` when the
/// table only holds `(net 0 "")`. A board with no net table at all also
/// yields `1`, which would be meaningless to write — callers must check
/// [`board_uses_net_table`] first rather than read anything into that value.
pub fn next_net_id(tree: &SexpNode) -> i32 {
    tree.find_all("net")
        .iter()
        .filter_map(|n| net_id(n))
        .max()
        .map(|max| max + 1)
        .unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_sexp;

    #[test]
    fn net_name_and_id_on_the_old_form() {
        let n = parse_sexp(r#"(net 6 "HDMI_+5V")"#).unwrap();
        assert_eq!(net_name(&n), Some("HDMI_+5V"));
        assert_eq!(net_id(&n), Some(6));
    }

    #[test]
    fn net_name_and_id_on_the_new_form() {
        let n = parse_sexp(r#"(net "VCC")"#).unwrap();
        assert_eq!(net_name(&n), Some("VCC"));
        assert_eq!(net_id(&n), None);
    }

    #[test]
    fn non_net_node_yields_none() {
        let n = parse_sexp(r#"(pad "1" thru_hole circle)"#).unwrap();
        assert_eq!(net_name(&n), None);
        assert_eq!(net_id(&n), None);
    }

    #[test]
    fn board_uses_net_table_true_on_old_form() {
        let tree = parse_sexp(
            r#"(kicad_pcb (net 0 "") (net 1 "GND") (footprint (pad "1" (net 1 "GND"))))"#,
        )
        .unwrap();
        assert!(board_uses_net_table(&tree));
    }

    #[test]
    fn board_uses_net_table_false_on_new_form() {
        let tree =
            parse_sexp(r#"(kicad_pcb (footprint (pad "1" (net "GND")) (pad "2" (net "VCC"))))"#)
                .unwrap();
        assert!(!board_uses_net_table(&tree));
    }

    #[test]
    fn count_distinct_nets_old_form_counts_names_not_declarations() {
        let tree = parse_sexp(
            r#"(kicad_pcb
                 (net 0 "")
                 (net 1 "GND")
                 (net 2 "VCC")
                 (footprint (pad "1" (net 1 "GND")) (pad "2" (net 2 "VCC")))
                 (footprint (pad "1" (net 1 "GND"))))"#,
        )
        .unwrap();
        // GND and VCC only — net 0's empty name is excluded, and GND's repeat
        // across two pads must not be double counted.
        assert_eq!(count_distinct_nets(&tree), 2);
    }

    #[test]
    fn count_distinct_nets_new_form_counts_names_not_declarations() {
        let tree = parse_sexp(
            r#"(kicad_pcb
                 (footprint (pad "1" (net "GND")) (pad "2" (net "VCC")))
                 (footprint (pad "1" (net "GND")) (pad "2" (net ""))))"#,
        )
        .unwrap();
        // GND repeated across pads counts once; VCC counts once; the empty
        // net name is excluded.
        assert_eq!(count_distinct_nets(&tree), 2);
    }

    #[test]
    fn next_net_id_skips_gaps_in_the_table() {
        let tree = parse_sexp(r#"(kicad_pcb (net 0 "") (net 1 "GND") (net 5 "VCC"))"#).unwrap();
        assert_eq!(next_net_id(&tree), 6);
    }

    #[test]
    fn next_net_id_is_one_when_only_net_zero_exists() {
        let tree = parse_sexp(r#"(kicad_pcb (net 0 ""))"#).unwrap();
        assert_eq!(next_net_id(&tree), 1);
    }
}
