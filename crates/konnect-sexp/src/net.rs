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

/// How a newly written item must refer to a net on *this* board.
///
/// The discriminant is the same one the readers use — does the board carry a
/// net table — so read and write can never disagree about which form a file is
/// in. Produced by [`net_ref_for_write`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetRef {
    /// KiCAD 20260206 and later: no table, no ids, the name is written on the
    /// item itself.
    ByName,
    /// Up to 20250907: the id declared by the board's own table. The name
    /// travels in a sibling node rather than inside `(net …)`.
    ById(i32),
}

/// A net the board's table does not declare. Writing it as id 0 would attach
/// the item to the unconnected pseudo-net and report success, which is the
/// defect this type exists to prevent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetNotDeclared {
    pub net_name: String,
}

impl std::fmt::Display for NetNotDeclared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "this board declares its nets in a table and has no entry for \
             '{}'; add it with add_net first",
            self.net_name
        )
    }
}

impl NetRef {
    /// The tokens a **zone** declares its net with, ready to inline into a
    /// `(zone …)` block.
    ///
    /// A zone is peculiar and the two forms differ in more than the id: the
    /// legacy form writes `(net <id>)` with the *name* in a sibling
    /// `(net_name "…")`, where a pad writes `(net <id> "<name>")` in one node;
    /// the id-less form writes `(net "<name>")` and no `net_name` at all.
    /// Measured on KiCad's own demos — `stickhub` and `cm5_minima` for the
    /// first, `pic_programmer` (20260206) for the second.
    ///
    /// The layer token is deliberately not decided here: `(layer …)` versus
    /// `(layers …)` turned out to be a matter of how many layers the zone
    /// covers, not which form the file is in — `vme-wren` (20241229) writes
    /// both — so a single-layer zone stays singular on every board.
    pub fn zone_tokens(&self, net_name: &str) -> String {
        match self {
            NetRef::ByName => format!("(net {})", quoted(net_name)),
            NetRef::ById(id) => format!("(net {id}) (net_name {})", quoted(net_name)),
        }
    }
}

/// Quote a net name as an s-expression string, escaping what KiCad escapes.
fn quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// How to write a reference to `net_name` on `tree`, or the reason it cannot
/// be written.
///
/// On a board with no net table the name *is* the reference, so any name can
/// be written and none can be wrong. On a board with a table the id has to
/// come from that table: a name it does not declare is refused rather than
/// zeroed, because id 0 is the unconnected pseudo-net and a pour attached to
/// it is electrically orphaned while the tool reports success.
pub fn net_ref_for_write(tree: &SexpNode, net_name: &str) -> Result<NetRef, NetNotDeclared> {
    if !board_uses_net_table(tree) {
        return Ok(NetRef::ByName);
    }
    tree.find_all("net")
        .iter()
        .find(|n| net_id(n).is_some() && self::net_name(n) == Some(net_name))
        .and_then(|n| net_id(n))
        .map(NetRef::ById)
        .ok_or_else(|| NetNotDeclared {
            net_name: net_name.to_string(),
        })
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

    /// Oracle: `pic_programmer.kicad_pcb` (20260206) writes a zone as
    /// `(zone (net "GND") (layer "B.Cu") …)` — the name on the net node and
    /// no `net_name` sibling at all.
    #[test]
    fn a_table_less_board_writes_the_zone_net_by_name() {
        let tree = parse_sexp(r#"(kicad_pcb (footprint (pad "1" (net "GND"))))"#).unwrap();
        let net_ref = net_ref_for_write(&tree, "GND").expect("no table, no refusal");
        assert_eq!(net_ref, NetRef::ByName);
        assert_eq!(net_ref.zone_tokens("GND"), r#"(net "GND")"#);
    }

    /// A board with no table declares nothing, so a name it has never seen is
    /// still writable — the name is the reference.
    #[test]
    fn a_table_less_board_accepts_a_net_it_has_never_seen() {
        let tree = parse_sexp(r#"(kicad_pcb (footprint (pad "1" (net "GND"))))"#).unwrap();
        assert_eq!(net_ref_for_write(&tree, "VCC"), Ok(NetRef::ByName));
    }

    /// Oracle: `StickHub.kicad_pcb` (20250907) and `CM5_MINIMA_3.kicad_pcb`
    /// (20250513) write `(net <id>)` with the name in a sibling `(net_name …)`
    /// — not `(net <id> "<name>")`, which is the pad form.
    #[test]
    fn a_table_board_writes_the_declared_id_and_a_net_name_sibling() {
        let tree = parse_sexp(
            r#"(kicad_pcb (net 0 "") (net 1 "GND") (net 97 "+5V")
                 (footprint (pad "1" (net 1 "GND"))))"#,
        )
        .unwrap();
        let net_ref = net_ref_for_write(&tree, "+5V").expect("the table declares +5V");
        assert_eq!(net_ref, NetRef::ById(97));
        assert_eq!(net_ref.zone_tokens("+5V"), r#"(net 97) (net_name "+5V")"#);
    }

    /// The whole point: a net the table does not declare must be refused, not
    /// written as id 0. Net 0 is the unconnected pseudo-net, so zeroing it
    /// produces an electrically orphaned pour reported as a success.
    #[test]
    fn a_table_board_refuses_a_net_it_does_not_declare() {
        let tree = parse_sexp(r#"(kicad_pcb (net 0 "") (net 1 "GND"))"#).unwrap();
        let err = net_ref_for_write(&tree, "VCC").expect_err("VCC is not in the table");
        assert_eq!(err.net_name, "VCC");
        assert!(
            err.to_string().contains("add_net"),
            "the refusal must name the way out, got {err}"
        );
    }

    #[test]
    fn a_net_name_carrying_a_quote_is_escaped() {
        assert_eq!(NetRef::ByName.zone_tokens(r#"N"1"#), "(net \"N\\\"1\")");
    }
}
