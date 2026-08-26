//! Libraries, footprints and pads, exercised end to end (J.2.3.5).
//!
//! Ten `libraries` and `footprints` tools shipped with no test that runs. The
//! trick with this lot is not to need an installed KiCAD: everything here
//! builds its own library with `create_footprint` / `create_symbol` and then
//! registers, lists, reads and edits *that*. The only tool left `#[ignore]`d is
//! `search_footprints`, which searches the installed libraries by design and
//! has nothing to search without them.
//!
//! No `kicad-cli` and no running KiCAD.

mod harness;

use std::path::{Path, PathBuf};

use harness::Harness;
use serde_json::json;

/// A minimal `.kicad_pro`, so project-scoped library registration has a project
/// to write its table into.
const PROJECT: &str = "{\n  \"board\": {},\n  \"libraries\": {\n    \"pinned_footprint_libs\": [],\n    \"pinned_symbol_libs\": []\n  },\n  \"meta\": { \"filename\": \"probe.kicad_pro\", \"version\": 3 }\n}\n";

/// Build a two-pad SMD footprint inside a `.pretty` directory and return
/// (library dir, footprint file).
async fn a_footprint_library(h: &Harness) -> (PathBuf, PathBuf) {
    let library = h.dir.path().join("Probe.pretty");
    std::fs::create_dir_all(&library).expect("the library directory is creatable");
    let footprint = library.join("R_PROBE.kicad_mod");

    h.json(
        "create_footprint",
        json!({
            "output": harness::as_str(&footprint),
            "name": "R_PROBE",
            "description": "Two-pad probe footprint",
            "package_type": "smd",
            "pads": [
                { "number": "1", "type": "smd", "shape": "roundrect", "x": -0.8, "y": 0.0, "width": 0.9, "height": 0.95 },
                { "number": "2", "type": "smd", "shape": "roundrect", "x":  0.8, "y": 0.0, "width": 0.9, "height": 0.95 }
            ]
        }),
    )
    .await;

    (library, footprint)
}

fn a_project(h: &Harness) -> PathBuf {
    h.write("probe.kicad_pro", PROJECT)
}

// ─── Footprint files ─────────────────────────────────────────────────────────

/// A footprint written by `create_footprint` is one `get_footprint_info` can
/// read back, with the pads that were asked for. The two tools are each
/// other's only check outside KiCAD.
#[tokio::test]
async fn a_created_footprint_reads_back_with_its_pads() {
    let h = Harness::new();
    let (_library, footprint) = a_footprint_library(&h).await;

    let info = h
        .json(
            "get_footprint_info",
            json!({ "footprint_path": harness::as_str(&footprint) }),
        )
        .await;
    assert_eq!(info["name"], "R_PROBE", "the footprint's name: {info}");
    assert_eq!(info["pad_count"], 2, "both pads were asked for: {info}");
    assert_eq!(
        info["has_courtyard"], true,
        "`package_type: smd` asks for a courtyard: {info}"
    );
    assert_eq!(
        info["has_3d_model"], false,
        "no model was attached and none should be claimed: {info}"
    );
}

/// `edit_footprint_pad` changes one pad and leaves the other alone — a pad
/// editor that rewrote every pad would pass a test that only looked at the one
/// it edited.
#[tokio::test]
async fn editing_a_pad_touches_only_that_pad() {
    let h = Harness::new();
    let (_library, footprint) = a_footprint_library(&h).await;

    h.json(
        "edit_footprint_pad",
        json!({
            "footprint_path": harness::as_str(&footprint),
            "pad_number": "2",
            "width": 1.4,
            "height": 1.2
        }),
    )
    .await;

    let text = std::fs::read_to_string(&footprint).expect("the footprint is readable");
    assert!(
        text.contains("1.4") && text.contains("1.2"),
        "the new pad size was not written:\n{text}"
    );
    assert!(
        text.contains("0.9"),
        "pad 1 kept its 0.9 mm width and should still be there:\n{text}"
    );
}

/// Listing a `.pretty` directory finds the footprints in it, by name.
#[tokio::test]
async fn a_pretty_directory_lists_the_footprints_in_it() {
    let h = Harness::new();
    let (library, _footprint) = a_footprint_library(&h).await;

    let listed = h
        .json(
            "list_library_footprints",
            json!({ "library_path": harness::as_str(&library) }),
        )
        .await;
    assert!(
        listed.to_string().contains("R_PROBE"),
        "the footprint just written is not listed: {listed}"
    );
}

// ─── Library registration ────────────────────────────────────────────────────

/// Registering a library project-scoped writes the table beside the project and
/// not into the user's KiCAD install — which is what makes a project's
/// libraries travel with it.
#[tokio::test]
async fn a_project_scoped_footprint_library_is_registered_beside_the_project() {
    let h = Harness::new();
    let (library, _footprint) = a_footprint_library(&h).await;
    let project = a_project(&h);

    h.json(
        "register_footprint_library",
        json!({
            "library_path": harness::as_str(&library),
            "nickname": "Probe",
            "scope": "project",
            "project": harness::as_str(&project)
        }),
    )
    .await;

    let table = h.dir.path().join("fp-lib-table");
    assert!(
        table.is_file(),
        "no fp-lib-table beside the project: {:?}",
        std::fs::read_dir(h.dir.path())
            .map(|d| d.flatten().map(|e| e.file_name()).collect::<Vec<_>>())
    );
    let text = std::fs::read_to_string(&table).expect("the table is readable");
    assert!(
        text.contains("Probe"),
        "the nickname is not in the table:\n{text}"
    );
}

/// The symbol side of the same story, and `list_symbol_libraries` scoped to the
/// project has to see it — a registration nothing can list is not a
/// registration.
#[tokio::test]
async fn a_registered_symbol_library_is_listed_for_that_project() {
    let h = Harness::new();
    let project = a_project(&h);
    let library = h.dir.path().join("Probe.kicad_sym");

    h.json(
        "create_symbol",
        json!({
            "library_path": harness::as_str(&library),
            "name": "PROBE_PART",
            "reference_prefix": "U",
            "value": "PROBE"
        }),
    )
    .await;
    h.json(
        "register_symbol_library",
        json!({
            "library_path": harness::as_str(&library),
            "nickname": "ProbeSyms",
            "scope": "project",
            "project": harness::as_str(&project)
        }),
    )
    .await;

    let listed = h
        .json(
            "list_symbol_libraries",
            json!({ "project": harness::as_str(&project), "scope": "project" }),
        )
        .await;
    assert!(
        listed.to_string().contains("ProbeSyms"),
        "the project library is missing from the project-scoped listing: {listed}"
    );
}

/// `delete_symbol` removes one symbol and leaves the library — and the other
/// symbol — intact.
#[tokio::test]
async fn deleting_a_symbol_leaves_the_library_and_its_other_symbols() {
    let h = Harness::new();
    let library = h.dir.path().join("Probe.kicad_sym");

    for name in ["KEEP_ME", "DELETE_ME"] {
        h.json(
            "create_symbol",
            json!({
                "library_path": harness::as_str(&library),
                "name": name,
                "reference_prefix": "U",
                "value": name
            }),
        )
        .await;
    }

    h.json(
        "delete_symbol",
        json!({
            "library_path": harness::as_str(&library),
            "symbol_name": "DELETE_ME"
        }),
    )
    .await;

    let text = std::fs::read_to_string(&library).expect("the library is readable");
    assert!(
        !text.contains("DELETE_ME"),
        "the deleted symbol is still there:\n{text}"
    );
    assert!(
        text.contains("KEEP_ME"),
        "the other symbol went with it:\n{text}"
    );
}

// ─── Pads on a board ─────────────────────────────────────────────────────────

/// `get_component_pads` and `get_pad_position` have to agree: the single-pad
/// lookup is the same answer as the one for that pad in the list. They are two
/// code paths over one fact.
#[tokio::test]
async fn the_pad_readers_agree_about_the_same_pad() {
    let h = Harness::new();
    let board = harness::as_str(&h.fixture("test.kicad_pcb")).to_string();

    let all = h
        .json(
            "get_component_pads",
            json!({ "board": board, "reference": "R1" }),
        )
        .await;
    assert_eq!(all["pad_count"], 2, "R1 has two pads: {all}");

    let one = h
        .json(
            "get_pad_position",
            json!({ "board": board, "reference": "R1", "pad_number": "1" }),
        )
        .await;
    let from_list = all["pads"]
        .as_array()
        .expect("the pads are a list")
        .iter()
        .find(|pad| pad["number"] == "1")
        .expect("pad 1 is in the list");
    assert_eq!(one["x"], from_list["x"], "the two readers disagree on x");
    assert_eq!(one["y"], from_list["y"], "the two readers disagree on y");
    assert_eq!(
        one["net"], from_list["net"],
        "the two readers disagree on the net"
    );
}

// ─── Needs the installed libraries ───────────────────────────────────────────

/// `search_footprints` searches the *installed* KiCAD libraries, so there is
/// nothing for it to find without them. `#[ignore]`d rather than faked, and
/// reported as `gated` in the matrix for the same reason:
///
///     cargo test -p konnect-core --test libraries_and_footprints -- --ignored
#[tokio::test]
#[ignore = "requires the installed KiCAD footprint libraries; run with --ignored"]
async fn searching_the_installed_libraries_finds_a_common_footprint() {
    let h = Harness::new();

    let found = h
        .json("search_footprints", json!({ "query": "0603", "limit": 5 }))
        .await;
    let results = found["results"].as_array().expect("results is a list");
    assert!(
        !results.is_empty(),
        "0603 parts exist in every KiCAD install: {found}"
    );
    for result in results {
        assert!(
            result["id"]
                .as_str()
                .is_some_and(|id| id.contains(':') && id.contains("0603")),
            "a result should be a Library:Footprint id matching the query: {result}"
        );
    }
}

// ─── Registration outcome (P.6.9.12) ─────────────────────────────────────────
//
// A registration that did nothing used to answer exactly what a registration
// that wrote the entry answered — a bare `"success": true` — so a stale URI
// could never be corrected and the caller could not tell it had not been.

/// The two registration tools, each with the table it writes and a library
/// path whose extension the tool expects.
const REGISTRARS: [(&str, &str, &str, &str); 2] = [
    (
        "register_footprint_library",
        "fp-lib-table",
        "fp_lib_table",
        "Probe.pretty",
    ),
    (
        "register_symbol_library",
        "sym-lib-table",
        "sym_lib_table",
        "Probe.kicad_sym",
    ),
];

/// Register `library` under `nickname` for `h`'s project, returning the tool's
/// own result so the outcome — not just the absence of an error — can be read.
async fn register(
    h: &Harness,
    tool: &str,
    library: &Path,
    nickname: &str,
    replace_existing: Option<bool>,
) -> konnect_core::mcp::protocol::CallToolResult {
    let project = h.path("probe.kicad_pro");
    let mut args = json!({
        "library_path": harness::as_str(library),
        "nickname": nickname,
        "scope": "project",
        "project": harness::as_str(&project)
    });
    if let Some(replace) = replace_existing {
        args["replace_existing"] = json!(replace);
    }
    h.call(tool, args).await.expect("the tool answered")
}

/// A nickname the table has never seen is inserted, and says so.
#[tokio::test]
async fn a_first_registration_reports_inserted() {
    for (tool, _table, _root, library_name) in REGISTRARS {
        let h = Harness::new();
        let _project = a_project(&h);
        let library = h.path(library_name);

        let body = harness::body(&register(&h, tool, &library, "Probe", None).await);
        assert_eq!(
            body["result"], "inserted",
            "{tool} did not report a first registration as inserted: {body}"
        );
    }
}

/// Registering the same library twice is still idempotent — and now says the
/// second call did nothing. The file must be byte-identical: a bare success
/// would pass even if the table had been rewritten underneath.
#[tokio::test]
async fn re_registering_the_same_uri_reports_unchanged_and_rewrites_nothing() {
    for (tool, table_name, _root, library_name) in REGISTRARS {
        let h = Harness::new();
        let _project = a_project(&h);
        let library = h.path(library_name);
        let table = h.path(table_name);

        register(&h, tool, &library, "Probe", None).await;
        let before = std::fs::read(&table).expect("the table was written");

        let body = harness::body(&register(&h, tool, &library, "Probe", None).await);
        assert_eq!(
            body["result"], "unchanged",
            "{tool} did not report a repeat registration as unchanged: {body}"
        );
        assert_eq!(
            std::fs::read(&table).expect("the table is still readable"),
            before,
            "{tool} rewrote a table it had nothing to change"
        );
    }
}

/// A nickname already registered against a different URI is refused rather
/// than reported as a registration, and the table keeps the URI it had.
#[tokio::test]
async fn a_different_uri_without_replace_existing_is_refused() {
    for (tool, table_name, _root, library_name) in REGISTRARS {
        let h = Harness::new();
        let _project = a_project(&h);
        let old = h.path(library_name);
        let new = h.path(&format!("Moved_{library_name}"));
        let table = h.path(table_name);

        register(&h, tool, &old, "Probe", None).await;
        let result = register(&h, tool, &new, "Probe", None).await;
        let body = harness::body(&result);
        assert_ne!(
            body["result"], "inserted",
            "{tool} claimed to insert over an existing nickname: {body}"
        );
        assert!(
            result.is_error,
            "{tool} answered success for a URI it did not write: {body}"
        );

        let text = std::fs::read_to_string(&table).expect("the table is readable");
        assert!(
            text.contains(harness::as_str(&old)) && !text.contains(harness::as_str(&new)),
            "{tool} changed the URI without replace_existing:\n{text}"
        );
    }
}

/// `replace_existing` corrects a stale URI in place and leaves the rest of the
/// entry — the caller's own `options` and `descr` — exactly as it was.
#[tokio::test]
async fn replace_existing_corrects_the_uri_and_preserves_options_and_descr() {
    for (tool, table_name, root, library_name) in REGISTRARS {
        let h = Harness::new();
        let _project = a_project(&h);
        let new = h.path(library_name);
        let table = h.path(table_name);
        std::fs::write(
            &table,
            format!(
                "({root}\n\t(version 7)\n\t(lib (name \"Probe\") (type \"KiCad\") (uri \"/gone/old.path\") (options \"hand-written\") (descr \"the caller's own note\"))\n)\n"
            ),
        )
        .expect("the table is writable");

        let body = harness::body(&register(&h, tool, &new, "Probe", Some(true)).await);
        assert_eq!(
            body["result"], "updated",
            "{tool} did not report a corrected URI as updated: {body}"
        );

        let text = std::fs::read_to_string(&table).expect("the table is readable");
        assert!(
            text.contains(harness::as_str(&new)) && !text.contains("/gone/old.path"),
            "{tool} did not correct the URI:\n{text}"
        );
        assert!(
            text.contains("(options \"hand-written\")")
                && text.contains("(descr \"the caller's own note\")"),
            "{tool} discarded the entry's own options/descr:\n{text}"
        );
    }
}

/// The old presence check was `content.contains("(name \"X\")")` over the whole
/// file, so any entry whose description quoted that text made `X` look
/// registered — and the real registration was then skipped.
#[tokio::test]
async fn a_nickname_quoted_inside_a_descr_is_not_a_registration() {
    for (tool, table_name, root, library_name) in REGISTRARS {
        let h = Harness::new();
        let _project = a_project(&h);
        let library = h.path(library_name);
        let table = h.path(table_name);
        std::fs::write(
            &table,
            format!(
                "({root}\n\t(version 7)\n\t(lib (name \"Other\") (type \"KiCad\") (uri \"/other\") (options \"\") (descr \"see (name \\\"Probe\\\") elsewhere\"))\n)\n"
            ),
        )
        .expect("the table is writable");

        let body = harness::body(&register(&h, tool, &library, "Probe", None).await);
        assert_eq!(
            body["result"], "inserted",
            "{tool} mistook a quoted nickname in a descr for a registration: {body}"
        );

        let text = std::fs::read_to_string(&table).expect("the table is readable");
        assert!(
            text.contains(harness::as_str(&library)),
            "{tool} did not write the entry it reported:\n{text}"
        );
    }
}
