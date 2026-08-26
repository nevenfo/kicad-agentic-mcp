//! Reading the `(layers …)` stackup table of a `.kicad_pcb`.
//!
//! Layer entries are the one table in the board file that is *not* keyed by a
//! tag. Everything else is `(tag …)` and can be found with [`SexpNode::find_all`];
//! a layer is `(0 "F.Cu" signal)`, whose head is the ordinal itself. There is no
//! tag to match on, so the entries have to be read by shape: every list child of
//! the `(layers …)` node is a layer.
//!
//! ```
//! use konnect_sexp::{parse_sexp, layers};
//! let board = parse_sexp(r#"(kicad_pcb (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (9 "F.Adhes" user)))"#).unwrap();
//! let stack = layers::layers(&board);
//! assert_eq!(stack.len(), 3);
//! assert_eq!(stack[0].name, "F.Cu");
//! assert_eq!(layers::copper(&stack).len(), 2);
//! ```

use crate::parser::SexpNode;

/// One entry of the board stackup.
#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    /// Ordinal as written in the file. Not an index: KiCAD leaves gaps
    /// (`0 F.Cu`, `2 B.Cu`, `9 F.Adhes` …) and inner copper occupies 1..=30.
    pub id: i32,
    /// Canonical name — `F.Cu`, `In1.Cu`, `Edge.Cuts`.
    pub name: String,
    /// `signal`, `power`, `mixed`, `jumper` or `user`.
    pub kind: String,
    /// Optional user-facing rename, e.g. `(0 "F.Cu" signal "Top Layer")`.
    pub user_name: Option<String>,
}

impl Layer {
    /// Copper is decided by the canonical name, not by `kind`.
    ///
    /// KiCAD marks copper with four different kinds (`signal`, `power`,
    /// `mixed`, `jumper`) and a board that uses `power` for a plane would be
    /// undercounted by a kind allow-list. The `.Cu` suffix is the invariant.
    pub fn is_copper(&self) -> bool {
        self.name.ends_with(".Cu")
    }
}

/// Read the stackup from a parsed board. Empty if there is no `(layers …)`.
pub fn layers(board: &SexpNode) -> Vec<Layer> {
    let Some(node) = board.find("layers") else {
        return Vec::new();
    };
    node.children()
        .unwrap_or(&[])
        .iter()
        // Skips the head atom (`layers`), which is a child like any other, and
        // any stray atom: a layer is always a list.
        .filter(|child| child.children().is_some())
        .filter_map(layer_from)
        .collect()
}

/// Copper layers only, in file order — the "how many layers is this board"
/// answer, and what a fab house quotes on.
pub fn copper(stack: &[Layer]) -> Vec<&Layer> {
    stack.iter().filter(|l| l.is_copper()).collect()
}

/// The fixed layer names, i.e. every `BoardLayer` variant that is neither
/// `In<n>.Cu` nor `User.<n>` nor a sentinel.
const FIXED_NAMES: &[&str] = &[
    "F.Cu",
    "B.Cu",
    "B.Adhes",
    "F.Adhes",
    "B.Paste",
    "F.Paste",
    "B.SilkS",
    "F.SilkS",
    "B.Mask",
    "F.Mask",
    "Dwgs.User",
    "Cmts.User",
    "Eco1.User",
    "Eco2.User",
    "Edge.Cuts",
    "Margin",
    "B.CrtYd",
    "F.CrtYd",
    "B.Fab",
    "F.Fab",
    "Rescue",
];

/// Highest `In<n>.Cu` / `User.<n>` KiCAD defines (`BL_In30_Cu`, `BL_User_45`).
const MAX_INNER_COPPER: u32 = 30;
const MAX_USER: u32 = 45;

/// Is this a layer name KiCAD will accept in a board file?
///
/// KiCAD does not take arbitrary layer names: the set is closed, and it is the
/// `BoardLayer` enum of the official API protos
/// (`konnect-ipc/proto/board/board_types.proto`), whose variant names map onto
/// file names by dropping the `BL_` prefix and turning the remaining `_` into a
/// `.` — `BL_F_Cu` → `F.Cu`, `BL_User_1` → `User.1`.
///
/// A board carrying a name outside that set is **rejected outright** by KiCAD —
/// it does not degrade, the file simply will not open. The
/// `layers_canonical_names_match_kicads_own_enum` test in `pcb_board.rs` keeps
/// this in step with the enum.
pub fn is_canonical_name(name: &str) -> bool {
    if FIXED_NAMES.contains(&name) {
        return true;
    }
    if let Some(n) = name.strip_prefix("In").and_then(|s| s.strip_suffix(".Cu")) {
        return matches!(n.parse::<u32>(), Ok(n) if (1..=MAX_INNER_COPPER).contains(&n))
            && !n.starts_with('0');
    }
    if let Some(n) = name.strip_prefix("User.") {
        return matches!(n.parse::<u32>(), Ok(n) if (1..=MAX_USER).contains(&n))
            && !n.starts_with('0');
    }
    false
}

/// Which of the two `(layers …)` id schemes KiCAD has shipped a board with.
///
/// `LEGACY` is the scheme `BoardLayer` (the API proto) still enumerates, and
/// what `crates/konnect-core/tests/fixtures/unrouted.kicad_pcb` carries
/// (`F.Cu`=0, `B.Cu`=31). `MODERN` is what KiCAD >= 20241030 actually writes
/// to disk, measured across the 18 demo boards shipped with the 10.0.3
/// install (`F.Cu`=0, `B.Cu`=2, `In1.Cu`=4 … `In30.Cu`=62). `kicad-cli`
/// itself does not care which one a file uses — its loader remaps the table
/// by *name*, not by id, and happily opens a board mixing legacy ids, a
/// duplicated id, or the same name declared twice. That tolerance is why the
/// bug this enum fixes was invisible to `kicad-cli`: the wrong id only shows
/// up in this server's own layer counts and in a file this server itself
/// re-reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Numbering {
    Modern,
    Legacy,
}

/// Fixed (non-`In<n>.Cu`, non-`User.<n>`) ids under `LEGACY`.
///
/// Equal to the corresponding `BoardLayer` proto value minus 3 — verified
/// variant-by-variant by `layers_canonical_names_match_kicads_own_enum` in
/// `pcb_board.rs`. `Rescue` sits at 59, between `User.9` (58) and `User.10`
/// (60), which is why the `User.<n>` formula below is piecewise.
const LEGACY_FIXED_IDS: &[(&str, i32)] = &[
    ("F.Cu", 0),
    ("B.Cu", 31),
    ("B.Adhes", 32),
    ("F.Adhes", 33),
    ("B.Paste", 34),
    ("F.Paste", 35),
    ("B.SilkS", 36),
    ("F.SilkS", 37),
    ("B.Mask", 38),
    ("F.Mask", 39),
    ("Dwgs.User", 40),
    ("Cmts.User", 41),
    ("Eco1.User", 42),
    ("Eco2.User", 43),
    ("Edge.Cuts", 44),
    ("Margin", 45),
    ("B.CrtYd", 46),
    ("F.CrtYd", 47),
    ("B.Fab", 48),
    ("F.Fab", 49),
    ("Rescue", 59),
];

/// Fixed ids under `MODERN`, measured across the 18 demo boards of the
/// KiCAD 10.0.3 install (e.g. `CM5_MINIMA_3` for the `In<n>.Cu` progression).
///
/// `Rescue` is deliberately absent: no demo board declares it under this
/// scheme, and the proto enum's ordering (between `User.9` and `User.10`)
/// does not name a slot the `37+2n` `User.<n>` formula leaves free. Guessing
/// a value here would be indistinguishable from a measured one to every
/// caller, so `canonical_id` returns `None` for `Rescue` under `MODERN`
/// instead.
const MODERN_FIXED_IDS: &[(&str, i32)] = &[
    ("F.Cu", 0),
    ("F.Mask", 1),
    ("B.Cu", 2),
    ("B.Mask", 3),
    ("F.SilkS", 5),
    ("B.SilkS", 7),
    ("F.Adhes", 9),
    ("B.Adhes", 11),
    ("F.Paste", 13),
    ("B.Paste", 15),
    ("Dwgs.User", 17),
    ("Cmts.User", 19),
    ("Eco1.User", 21),
    ("Eco2.User", 23),
    ("Edge.Cuts", 25),
    ("Margin", 27),
    ("B.CrtYd", 29),
    ("F.CrtYd", 31),
    ("B.Fab", 33),
    ("F.Fab", 35),
];

/// The ordinal KiCAD itself assigns `name` under `numbering`.
///
/// `None` for a name `is_canonical_name` already rejects, and for `Rescue`
/// under `MODERN` (see [`MODERN_FIXED_IDS`]). This is the id a caller must
/// write for a layer to load as the name it asked for — writing any other
/// free id, as `add_layer` used to, produces a file `kicad-cli` still opens
/// (its loader keys off the name) but whose id no longer means what this
/// server's own layer-counting code assumes.
pub fn canonical_id(name: &str, numbering: Numbering) -> Option<i32> {
    if !is_canonical_name(name) {
        return None;
    }
    let fixed = match numbering {
        Numbering::Legacy => LEGACY_FIXED_IDS,
        Numbering::Modern => MODERN_FIXED_IDS,
    };
    if let Some((_, id)) = fixed.iter().find(|(n, _)| *n == name) {
        return Some(*id);
    }
    if let Some(n) = name.strip_prefix("In").and_then(|s| s.strip_suffix(".Cu")) {
        let n: i32 = n.parse().ok()?;
        return Some(match numbering {
            Numbering::Legacy => n,
            Numbering::Modern => 2 * n + 2,
        });
    }
    if let Some(n) = name.strip_prefix("User.") {
        let n: i32 = n.parse().ok()?;
        return Some(match numbering {
            Numbering::Legacy if n <= 9 => 49 + n,
            Numbering::Legacy => 50 + n,
            Numbering::Modern => 37 + 2 * n,
        });
    }
    None
}

/// Which numbering a board's own `(layers …)` table is written in.
///
/// Decided by evidence, not by version: count, under each scheme, how many
/// `(id, name)` entries the table already carries agree with
/// [`canonical_id`], and take the scheme with the higher score. A tie
/// (including the empty stack, 0-0) resolves to `MODERN` — that is what
/// KiCAD 10 itself writes for a fresh board, and what a KiCAD 10 install
/// will re-save the file as if it ever reopens it.
pub fn numbering(stack: &[Layer]) -> Numbering {
    let score = |n: Numbering| {
        stack
            .iter()
            .filter(|l| canonical_id(&l.name, n) == Some(l.id))
            .count()
    };
    let legacy = score(Numbering::Legacy);
    let modern = score(Numbering::Modern);
    if legacy > modern {
        Numbering::Legacy
    } else {
        Numbering::Modern
    }
}

/// The stackup KiCAD itself applies to a board whose file declares no
/// `(layers …)` section.
///
/// Such a board is **not** malformed — KiCAD 10 opens it without complaint —
/// but it carries no table to read, so a caller asking what layers it has has
/// to be told what KiCAD will use. Measured, not assumed: a four-`gr_line`
/// board with no `(layers …)` was handed to `kicad-cli pcb upgrade` on
/// KiCAD 10.0.3, and this is the table KiCAD wrote back, in its own order —
/// two copper layers and twenty-two technical ones, including the `Edge.Cuts`
/// such a board is usually already drawing on.
///
/// The ids are `Numbering::Modern`, which is what a tie (an empty stack) in
/// [`numbering`] already resolves to; `default_stackup_ids_are_the_canonical_ones`
/// keeps the two in step.
const DEFAULT_STACKUP: &[(i32, &str, &str, Option<&str>)] = &[
    (0, "F.Cu", "signal", None),
    (2, "B.Cu", "signal", None),
    (9, "F.Adhes", "user", Some("F.Adhesive")),
    (11, "B.Adhes", "user", Some("B.Adhesive")),
    (13, "F.Paste", "user", None),
    (15, "B.Paste", "user", None),
    (5, "F.SilkS", "user", Some("F.Silkscreen")),
    (7, "B.SilkS", "user", Some("B.Silkscreen")),
    (1, "F.Mask", "user", None),
    (3, "B.Mask", "user", None),
    (17, "Dwgs.User", "user", Some("User.Drawings")),
    (19, "Cmts.User", "user", Some("User.Comments")),
    (21, "Eco1.User", "user", Some("User.Eco1")),
    (23, "Eco2.User", "user", Some("User.Eco2")),
    (25, "Edge.Cuts", "user", None),
    (27, "Margin", "user", None),
    (31, "F.CrtYd", "user", Some("F.Courtyard")),
    (29, "B.CrtYd", "user", Some("B.Courtyard")),
    (35, "F.Fab", "user", None),
    (33, "B.Fab", "user", None),
    (39, "User.1", "user", None),
    (41, "User.2", "user", None),
    (43, "User.3", "user", None),
    (45, "User.4", "user", None),
];

/// KiCAD's own default stackup, for a board file that declares no
/// `(layers …)` section. See [`DEFAULT_STACKUP`] for how it was measured.
pub fn default_stackup() -> Vec<Layer> {
    DEFAULT_STACKUP
        .iter()
        .map(|(id, name, kind, user_name)| Layer {
            id: *id,
            name: (*name).to_string(),
            kind: (*kind).to_string(),
            user_name: user_name.map(|s| s.to_string()),
        })
        .collect()
}

fn layer_from(node: &SexpNode) -> Option<Layer> {
    // `(0 "F.Cu" signal "Top Layer")` — the ordinal is child 0, being the head
    // of the list, so every field sits one place earlier than a tagged node.
    let id = node.get_f64(0)? as i32;
    let name = node.get(1)?.as_str()?.to_string();
    let kind = node
        .get(2)
        .and_then(|n| n.as_str())
        .unwrap_or("user")
        .to_string();
    let user_name = node.get(3).and_then(|n| n.as_str()).map(str::to_string);
    Some(Layer {
        id,
        name,
        kind,
        user_name,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_sexp;

    fn board(inner: &str) -> SexpNode {
        parse_sexp(&format!("(kicad_pcb (layers {inner}))")).unwrap()
    }

    #[test]
    fn reads_id_name_and_kind() {
        let stack = layers(&board(r#"(0 "F.Cu" signal)"#));
        assert_eq!(
            stack,
            vec![Layer {
                id: 0,
                name: "F.Cu".into(),
                kind: "signal".into(),
                user_name: None
            }]
        );
    }

    #[test]
    fn reads_the_optional_user_name() {
        let stack = layers(&board(r#"(0 "F.Cu" signal "Top Layer")"#));
        assert_eq!(stack[0].user_name.as_deref(), Some("Top Layer"));
    }

    #[test]
    fn the_head_atom_is_not_a_layer() {
        // `(layers …)` yields `layers` itself as a child; counting children
        // blindly overcounts by exactly one.
        let stack = layers(&board(r#"(0 "F.Cu" signal) (2 "B.Cu" signal)"#));
        assert_eq!(stack.len(), 2);
    }

    #[test]
    fn ids_are_read_as_written_not_as_positions() {
        // KiCAD leaves gaps; a positional read would report 0,1,2.
        let stack = layers(&board(
            r#"(0 "F.Cu" signal) (2 "B.Cu" signal) (9 "F.Adhes" user)"#,
        ));
        assert_eq!(
            stack.iter().map(|l| l.id).collect::<Vec<_>>(),
            vec![0, 2, 9]
        );
    }

    #[test]
    fn copper_is_by_name_not_by_kind() {
        // `power` and `mixed` are copper too; `user` never is, even on a layer
        // whose name merely contains "Cu".
        let stack = layers(&board(
            r#"(0 "F.Cu" signal) (1 "In1.Cu" power) (2 "In2.Cu" mixed) (3 "B.Cu" jumper) (9 "F.Adhes" user) (60 "Cu.Marks" user)"#,
        ));
        assert_eq!(
            copper(&stack).iter().map(|l| &l.name).collect::<Vec<_>>(),
            vec!["F.Cu", "In1.Cu", "In2.Cu", "B.Cu"]
        );
    }

    #[test]
    fn a_board_without_a_layers_block_is_empty_not_a_panic() {
        assert!(layers(&parse_sexp("(kicad_pcb)").unwrap()).is_empty());
    }

    #[test]
    fn a_malformed_entry_is_skipped_and_the_rest_survive() {
        let stack = layers(&board(r#"(0 "F.Cu" signal) (nonsense) (2 "B.Cu" signal)"#));
        assert_eq!(
            stack.iter().map(|l| &l.name).collect::<Vec<_>>(),
            vec!["F.Cu", "B.Cu"]
        );
    }

    #[test]
    fn canonical_names_are_accepted() {
        for name in [
            "F.Cu",
            "B.Cu",
            "In1.Cu",
            "In30.Cu",
            "Edge.Cuts",
            "Margin",
            "Rescue",
            "User.1",
            "User.45",
            "Dwgs.User",
            "F.CrtYd",
        ] {
            assert!(is_canonical_name(name), "{name} should be canonical");
        }
    }

    #[test]
    fn invented_names_are_rejected() {
        // The one that matters: a caller-supplied name KiCAD has never heard of
        // produces a board that will not open at all.
        for name in [
            "TestLayer",
            "MyLayer",
            "",
            "f.cu",
            "F_Cu",
            "In0.Cu",
            "User.0",
        ] {
            assert!(!is_canonical_name(name), "{name} should not be canonical");
        }
    }

    #[test]
    fn out_of_range_and_padded_ordinals_are_rejected() {
        // KiCAD stops at In30.Cu / User.45, and does not zero-pad.
        for name in ["In31.Cu", "User.46", "In01.Cu", "User.01", "In.Cu", "User."] {
            assert!(!is_canonical_name(name), "{name} should not be canonical");
        }
    }

    #[test]
    fn canonical_id_matches_both_measured_schemes() {
        assert_eq!(canonical_id("In1.Cu", Numbering::Modern), Some(4));
        assert_eq!(canonical_id("In30.Cu", Numbering::Modern), Some(62));
        assert_eq!(canonical_id("User.1", Numbering::Modern), Some(39));
        assert_eq!(canonical_id("User.45", Numbering::Modern), Some(127));
        assert_eq!(canonical_id("B.Cu", Numbering::Modern), Some(2));
        assert_eq!(canonical_id("F.Mask", Numbering::Modern), Some(1));
        assert_eq!(canonical_id("Edge.Cuts", Numbering::Modern), Some(25));

        assert_eq!(canonical_id("In1.Cu", Numbering::Legacy), Some(1));
        assert_eq!(canonical_id("In30.Cu", Numbering::Legacy), Some(30));
        assert_eq!(canonical_id("User.1", Numbering::Legacy), Some(50));
        assert_eq!(canonical_id("User.45", Numbering::Legacy), Some(95));
        assert_eq!(canonical_id("B.Cu", Numbering::Legacy), Some(31));
        assert_eq!(canonical_id("F.Mask", Numbering::Legacy), Some(39));
        assert_eq!(canonical_id("Edge.Cuts", Numbering::Legacy), Some(44));
    }

    #[test]
    fn rescue_is_measured_only_under_legacy() {
        assert_eq!(canonical_id("Rescue", Numbering::Legacy), Some(59));
        assert_eq!(canonical_id("Rescue", Numbering::Modern), None);
    }

    #[test]
    fn an_unknown_name_has_no_ordinal_under_either_scheme() {
        assert_eq!(canonical_id("TestLayer", Numbering::Modern), None);
        assert_eq!(canonical_id("TestLayer", Numbering::Legacy), None);
    }

    #[test]
    fn numbering_is_read_from_the_table_itself() {
        let modern = layers(&board(
            r#"(0 "F.Cu" signal) (2 "B.Cu" signal) (4 "In1.Cu" signal)"#,
        ));
        assert_eq!(numbering(&modern), Numbering::Modern);

        let legacy = layers(&board(
            r#"(0 "F.Cu" signal) (1 "In1.Cu" signal) (31 "B.Cu" signal)"#,
        ));
        assert_eq!(numbering(&legacy), Numbering::Legacy);

        assert_eq!(numbering(&[]), Numbering::Modern);
    }

    #[test]
    fn tab_indentation_is_irrelevant_to_a_tree_read() {
        // The shape is what matters; KiCAD 10 writes tabs, 9 writes spaces.
        let spaces = parse_sexp("(kicad_pcb\n  (layers\n    (0 \"F.Cu\" signal)\n  )\n)").unwrap();
        let tabs = parse_sexp("(kicad_pcb\n\t(layers\n\t\t(0 \"F.Cu\" signal)\n\t)\n)").unwrap();
        assert_eq!(layers(&spaces), layers(&tabs));
    }

    /// The default stackup is measured from KiCAD, and `canonical_id` is
    /// measured from KiCAD's demo boards. Two independent measurements of the
    /// same scheme must agree, or one of them has drifted.
    #[test]
    fn default_stackup_ids_are_the_canonical_ones() {
        for layer in default_stackup() {
            assert_eq!(
                canonical_id(&layer.name, Numbering::Modern),
                Some(layer.id),
                "{} is not at its canonical modern id",
                layer.name
            );
        }
    }

    /// A default stackup that omits `Edge.Cuts` would tell a caller drawing a
    /// board outline that the layer it is drawing on does not exist.
    #[test]
    fn the_default_stackup_has_two_copper_layers_and_an_outline_layer() {
        let stack = default_stackup();
        let cu: Vec<&str> = copper(&stack).iter().map(|l| l.name.as_str()).collect();
        assert_eq!(cu, vec!["F.Cu", "B.Cu"]);
        assert!(stack.iter().any(|l| l.name == "Edge.Cuts"));
    }
}
