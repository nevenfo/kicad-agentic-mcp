//! Property-based tests for the S-expression *writer*: `apply_edits` and the
//! balanced-block finders.
//!
//! `proptest_parser.rs` covers the parser. This file covers the other half of
//! the contract this crate actually relies on: KiCAD files are never
//! serialized from a tree — every write is "locate a block by byte offset,
//! replace that text, reparse". The round trip that matters is therefore
//! parse → locate → edit → reparse, not parse → serialize → parse.
//!
//! The generator below deliberately includes non-ASCII content (accented
//! Latin, CJK, emoji outside the BMP) in quoted values, because
//! `proptest_parser.rs`'s ASCII-only strategy keeps every byte offset a
//! trivially valid char boundary and so cannot exercise `String::replace_range`
//! (used by `apply_edits`), which panics on a non-boundary offset. It also
//! includes "trap" strings shaped like a block (`"(symbol trap)"`) so that
//! string-awareness in the finders is actually tested, not just assumed.

use konnect_sexp::{
    parse_sexp,
    writer::{
        apply_edits, find_balanced_block, find_block_starts, find_block_with_leading_whitespace,
        find_direct_child_blocks, find_enclosing_block, find_enclosing_direct_child_block,
    },
    SexpEdit,
};
use proptest::prelude::*;

// ─── Document Model ────────────────────────────────────────────────────────

/// A tiny tree used only to *generate* text and to know the ground truth
/// (e.g. "how many real `symbol` blocks exist"), independent of anything the
/// code under test computes.
#[derive(Debug, Clone)]
enum Node {
    Atom(String),
    Str(String),
    Block { tag: String, children: Vec<Node> },
}

/// Count `Block` nodes with the given tag, recursively. Text that merely
/// *looks* like a block (inside a `Str`) never counts — that's the point.
fn count_tag(node: &Node, tag: &str) -> usize {
    match node {
        Node::Block { tag: t, children } => {
            let here = usize::from(t == tag);
            here + children.iter().map(|c| count_tag(c, tag)).sum::<usize>()
        }
        _ => 0,
    }
}

/// Render the tree to KiCAD-shaped text: block children go on their own
/// indented line (real files are formatted this way), leaves stay inline.
fn serialize(node: &Node, indent: &str, depth: usize) -> String {
    match node {
        Node::Atom(a) => a.clone(),
        Node::Str(s) => format!("\"{s}\""),
        Node::Block { tag, children } => {
            let mut out = format!("({tag}");
            let has_block_child = children.iter().any(|c| matches!(c, Node::Block { .. }));
            for c in children {
                if matches!(c, Node::Block { .. }) {
                    out.push('\n');
                    out.push_str(&indent.repeat(depth + 1));
                } else {
                    out.push(' ');
                }
                out.push_str(&serialize(c, indent, depth + 1));
            }
            if has_block_child {
                out.push('\n');
                out.push_str(&indent.repeat(depth));
            }
            out.push(')');
            out
        }
    }
}

// ─── Strategies ──────────────────────────────────────────────────────────────

/// Tags this crate's own finders/callers actually look for.
fn block_tag_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("symbol".to_string()),
        Just("wire".to_string()),
        Just("label".to_string()),
        Just("property".to_string()),
        Just("pts".to_string()),
        Just("xy".to_string()),
        Just("uuid".to_string()),
        Just("at".to_string()),
        Just("lib_id".to_string()),
    ]
}

fn ascii_string_strategy() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9 ._:/+-]{0,20}"
}

/// Non-ASCII values a real project can contain: accented Latin (2-byte),
/// CJK (3-byte), emoji outside the BMP (4-byte) — and "trap" strings shaped
/// like a block, which must never be mistaken for real structure.
fn unicode_string_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("café".to_string()),
        Just("garçon niño".to_string()),
        Just("世界 日本語".to_string()),
        Just("emoji 😀🔥 text".to_string()),
        Just("𝔘𝔫𝔦𝔠𝔬𝔡𝔢".to_string()),
        Just("(symbol trap)".to_string()),
        Just("(wire 世界)".to_string()),
        Just("(label 😀)".to_string()),
    ]
}

fn leaf_strategy() -> impl Strategy<Value = Node> {
    prop_oneof![
        "[a-z_][a-z0-9_]{0,8}".prop_map(Node::Atom),
        ascii_string_strategy().prop_map(Node::Str),
        unicode_string_strategy().prop_map(Node::Str),
    ]
}

/// Recursively generate a tree shaped like a `.kicad_sch` fragment.
fn node_strategy() -> impl Strategy<Value = Node> {
    let leaf = leaf_strategy();
    leaf.prop_recursive(3, 20, 4, |inner| {
        (block_tag_strategy(), prop::collection::vec(inner, 0..4))
            .prop_map(|(tag, children)| Node::Block { tag, children })
    })
}

/// A full document: `(kicad_sch <items...>)`, plus the indentation style
/// (KiCAD's own writers use tabs; this crate's writer uses two spaces —
/// finders must be agnostic to both).
fn doc_strategy() -> impl Strategy<Value = (Node, &'static str)> {
    (
        prop::collection::vec(node_strategy(), 1..6),
        prop_oneof![Just("  "), Just("\t")],
    )
        .prop_map(|(items, indent)| {
            (
                Node::Block {
                    tag: "kicad_sch".to_string(),
                    children: items,
                },
                indent,
            )
        })
}

/// Rendered text alongside the tree it came from, so tests can check against
/// ground truth rather than against another call to the code under test.
fn doc_with_meta_strategy() -> impl Strategy<Value = (String, Node)> {
    doc_strategy().prop_map(|(root, indent)| (serialize(&root, indent, 0), root))
}

// ─── Properties ──────────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// 1. Identity: an empty edit list changes nothing, and the result still
    /// reparses to the same tree.
    #[test]
    fn apply_edits_empty_is_identity((text, _root) in doc_with_meta_strategy()) {
        let out = apply_edits(text.clone(), vec![]);
        prop_assert_eq!(&out, &text);
        prop_assert_eq!(parse_sexp(&out).ok(), parse_sexp(&text).ok());
    }

    /// 2 & 4. Excise/reinsert round trip, over documents that include
    /// non-ASCII content in quoted values and trap strings.
    #[test]
    fn excise_reinsert_roundtrip(
        (text, _root) in doc_with_meta_strategy(),
        tag in block_tag_strategy(),
        idx in 0usize..8,
    ) {
        let starts = find_block_starts(&text, &tag);
        prop_assume!(!starts.is_empty());
        let start = starts[idx % starts.len()];
        let (s, e) = find_balanced_block(&text, start)
            .expect("a start returned by find_block_starts must yield a balanced block");

        prop_assert!(text.is_char_boundary(s));
        prop_assert!(text.is_char_boundary(e));
        prop_assert!(text[s..e].starts_with('('));
        prop_assert!(text[s..e].ends_with(')'));
        prop_assert!(parse_sexp(&text[s..e]).is_ok(), "excised block must parse alone");

        // (a) delete then reinsert the exact same text at s reproduces the
        // original document byte-for-byte.
        let removed = text[s..e].to_string();
        let without = apply_edits(text.clone(), vec![SexpEdit::delete(s, e)]);
        let reinserted = apply_edits(without, vec![SexpEdit::insert(s, removed)]);
        prop_assert_eq!(&reinserted, &text);

        // (b) replacing with a different well-formed block still reparses.
        let replacement = "(uuid \"11111111-1111-1111-1111-111111111111\")".to_string();
        let replaced = apply_edits(text, vec![SexpEdit::replace(s, e, replacement)]);
        prop_assert!(parse_sexp(&replaced).is_ok());
    }

    /// 3 & 4. `find_block_starts` health: every offset lands on `(tag`, at a
    /// valid char boundary, resolves through `find_balanced_block`, and the
    /// count matches the tree's real block count — trap strings shaped like
    /// a block (including non-ASCII ones) must not be counted.
    #[test]
    fn block_starts_are_healthy(
        (text, root) in doc_with_meta_strategy(),
        tag in block_tag_strategy(),
    ) {
        let starts = find_block_starts(&text, &tag);
        prop_assert_eq!(starts.len(), count_tag(&root, &tag), "a quoted trap was mistaken for a block");

        for &off in &starts {
            prop_assert!(text.is_char_boundary(off));
            prop_assert!(text[off..].starts_with('('));
            prop_assert!(text[off + 1..].starts_with(tag.as_str()));

            let block = find_balanced_block(&text, off);
            prop_assert!(block.is_some());
            let (s, e) = block.unwrap();
            prop_assert!(text.is_char_boundary(s));
            prop_assert!(text.is_char_boundary(e));
        }
    }

    /// 6. Deleting every top-level occurrence of a tag (a set of
    /// non-overlapping sibling edits) leaves a document that still reparses,
    /// and the tag's total count drops by exactly the number of blocks
    /// removed (including any of the same tag nested inside them).
    #[test]
    fn full_document_edit_roundtrip(
        (text, _root) in doc_with_meta_strategy(),
        tag in block_tag_strategy(),
    ) {
        let children = find_direct_child_blocks(&text, "kicad_sch");
        let prefix = format!("({tag}");
        let matching: Vec<(usize, usize)> = children
            .into_iter()
            .filter(|&(s, _)| text[s..].starts_with(&prefix))
            .collect();
        prop_assume!(!matching.is_empty());

        let before = find_block_starts(&text, &tag).len();
        let removed: usize = matching
            .iter()
            .map(|&(s, e)| find_block_starts(&text[s..e], &tag).len())
            .sum();

        let edits = matching.iter().map(|&(s, e)| SexpEdit::delete(s, e)).collect();
        let out = apply_edits(text, edits);

        prop_assert!(parse_sexp(&out).is_ok());
        prop_assert_eq!(find_block_starts(&out, &tag).len(), before - removed);
    }

    /// 5. No finder ever panics, on arbitrary (possibly non-UTF-8-shaped-at-
    /// the-byte-level -- Rust `&str` strategies are always valid UTF-8, but
    /// offsets are still generated independently and can land anywhere,
    /// including mid-character or out of bounds).
    #[test]
    fn finders_never_panic_on_arbitrary_input(
        content in ".{0,300}",
        tag in "[a-z_]{1,10}",
        offset in 0usize..400,
    ) {
        let _ = find_balanced_block(&content, offset);
        let _ = find_block_with_leading_whitespace(&content, offset);
        let _ = find_block_starts(&content, &tag);
        let _ = find_enclosing_block(&content, &tag, offset);
        let _ = find_direct_child_blocks(&content, &tag);
        let _ = find_enclosing_direct_child_block(&content, &tag, offset);
    }

    /// 5, adversarial: bracket/quote soup mixed with non-ASCII, the shape
    /// most likely to desynchronize a byte-scanning finder.
    #[test]
    fn finders_never_panic_on_bracket_soup_utf8(
        content in r#"[()"a-z0-9\\ 世界😀é]{0,300}"#,
        tag in "[a-z_]{1,10}",
        offset in 0usize..400,
    ) {
        let _ = find_balanced_block(&content, offset);
        let _ = find_block_with_leading_whitespace(&content, offset);
        let _ = find_block_starts(&content, &tag);
        let _ = find_enclosing_block(&content, &tag, offset);
        let _ = find_direct_child_blocks(&content, &tag);
        let _ = find_enclosing_direct_child_block(&content, &tag, offset);
    }
}

/// Deterministic smoke test pinning the shape the generator above is meant
/// to resemble, with unicode content spelled out for readability.
#[test]
fn strategy_shape_smoke() {
    let text = "(kicad_sch\n\t(symbol\n\t\t(property \"Reference\" \"café 世界\")\n\t\t(property \"Note\" \"(symbol trap)\")\n\t)\n\t(wire (pts (xy 0 0) (xy 1 1)))\n)";
    let node = parse_sexp(text).unwrap();
    assert_eq!(node.head(), Some("kicad_sch"));

    let starts = find_block_starts(text, "symbol");
    assert_eq!(starts.len(), 1, "the quoted trap must not be counted");
    let (s, e) = find_balanced_block(text, starts[0]).unwrap();
    assert!(parse_sexp(&text[s..e]).is_ok());
}
