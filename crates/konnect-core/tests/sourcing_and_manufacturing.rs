//! Sourcing, datasheets, cost, DFM and the last file-engine strays (J.2.3.8).
//!
//! Eleven tools, and the interesting half of them talk to a third party: the
//! JLCPCB parts database, LCSC's datasheet API, Freerouting's jar. **A third
//! party is not a test dependency.** What is asserted here is what the tools do
//! when it is absent — which is the state every fresh install is in, and the
//! one where a bad answer does the most damage: a "no parts found" that is
//! really "no database" would send a caller looking for a part that exists.
//!
//! Anything that would actually reach the network is `#[ignore]`d and reads
//! `gated` in the matrix.
//!
//! No `kicad-cli` and no running KiCAD, except in the one probe that says so.

mod harness;

use harness::{Harness, TWO_RESISTORS};
use serde_json::json;

// ─── Absent third parties ────────────────────────────────────────────────────

/// Every JLCPCB tool has to distinguish "no database" from "nothing found", and
/// say which — the two look the same to a caller and mean opposite things.
#[tokio::test]
async fn the_jlcpcb_tools_say_the_database_is_missing_rather_than_finding_nothing() {
    let h = Harness::new();

    let stats = h.json("get_jlcpcb_database_stats", json!({})).await;
    // P.6.9.20: assert *which* database was looked at before asserting that it
    // is absent. With `jlcpcb_db_path: None` the handler fell back to
    // `%APPDATA%\konnect\jlcpcb.db`, so this test measured the machine while
    // its message claimed to measure the harness — and failed the day a real
    // database landed there. The path assertion is what keeps the fixture
    // honest if someone puts the fallback back.
    assert!(
        stats["path"]
            .as_str()
            .is_some_and(|p| p.contains("no-such-jlcpcb.db")),
        "the harness must name its own absent database, not the machine's: {stats}"
    );
    assert_eq!(
        stats["exists"], false,
        "no database is configured in this harness: {stats}"
    );
    assert!(
        stats["note"]
            .as_str()
            .is_some_and(|n| n.contains("download_jlcpcb_database")),
        "the answer should name the tool that fixes it: {stats}"
    );

    let suggested = h
        .call(
            "suggest_jlcpcb_alternatives",
            json!({ "value": "100nF", "footprint": "C_0603_1608Metric" }),
        )
        .await;
    let reported = match suggested {
        Ok(result) => harness::body(&result).to_string(),
        Err(e) => e.to_string(),
    };
    assert!(
        reported.contains("database"),
        "a missing database must not read as 'no alternatives': {reported}"
    );
}

/// Freerouting is an optional external jar, and its absence is a fact about the
/// machine rather than an error — reported as such, with where to get it.
#[tokio::test]
async fn the_freerouting_check_reports_absence_without_failing() {
    let h = Harness::new();

    let checked = h.json("check_freerouting", json!({})).await;
    assert_eq!(
        checked["available"], false,
        "no jar is configured in this harness: {checked}"
    );
    assert!(
        checked["note"].as_str().is_some_and(|n| n.contains("http")),
        "the answer should say where to get it: {checked}"
    );
}

/// A datasheet lookup that finds nothing returns a null URL and says why. The
/// failure mode guarded against is a made-up URL, which is worse than none.
///
/// This one does attempt an outbound LCSC lookup for a part number that cannot
/// exist. It is left running rather than `#[ignore]`d because the assertion
/// holds either way — offline the lookup fails, online it finds nothing, and
/// both must produce the same null — which is the property worth pinning.
#[tokio::test]
async fn a_datasheet_lookup_that_finds_nothing_returns_nothing() {
    let h = Harness::new();

    let looked_up = h
        .json(
            "get_datasheet_url",
            json!({ "mpn": "NO_SUCH_PART_J23_PROBE" }),
        )
        .await;
    assert!(
        looked_up["datasheet_url"].is_null(),
        "an unknown part must not come back with a URL: {looked_up}"
    );
    assert_eq!(looked_up["mpn"], "NO_SUCH_PART_J23_PROBE");
}

/// `enrich_datasheets` needs an LCSC id on a component to look anything up, and
/// reports how many it changed. Nothing to work from means zero updates and a
/// note saying so — not an error, and not a silent success either.
#[tokio::test]
async fn enrichment_reports_zero_updates_when_there_is_nothing_to_enrich() {
    let h = Harness::new();
    let sch = harness::as_str(&h.fixture(TWO_RESISTORS)).to_string();
    let before = std::fs::read_to_string(&sch).expect("the schematic is readable");

    let enriched = h
        .json("enrich_datasheets", json!({ "schematic": sch }))
        .await;
    assert_eq!(
        enriched["updated"], 0,
        "the fixture's resistors carry no LCSC id: {enriched}"
    );

    let after = std::fs::read_to_string(&sch).expect("the schematic is readable");
    assert_eq!(before, after, "a no-op enrichment rewrote the schematic");
}

// ─── Cost and DFM ────────────────────────────────────────────────────────────

/// A cost estimate scales with quantity and says out loud that it is an
/// estimate. Both halves matter: a number with no disclaimer gets quoted to a
/// customer.
#[tokio::test]
async fn a_cost_estimate_scales_with_quantity_and_admits_what_it_is() {
    let h = Harness::new();
    let board = harness::as_str(&h.fixture("test.kicad_pcb")).to_string();

    let five = h
        .json("estimate_cost", json!({ "board": board, "quantity": 5 }))
        .await;
    let fifty = h
        .json("estimate_cost", json!({ "board": board, "quantity": 50 }))
        .await;

    let total = |v: &serde_json::Value| {
        v["cost_estimate"]["total_estimate"]
            .as_str()
            .and_then(|s| s.trim_start_matches('$').parse::<f64>().ok())
            .unwrap_or_else(|| panic!("no total in {v}"))
    };
    assert!(
        total(&fifty) > total(&five),
        "fifty boards cost more than five: {five} vs {fifty}"
    );
    let per_board = |v: &serde_json::Value| {
        v["cost_estimate"]["per_board"]
            .as_str()
            .and_then(|s| s.trim_start_matches('$').parse::<f64>().ok())
            .unwrap_or_else(|| panic!("no per-board price in {v}"))
    };
    assert!(
        per_board(&fifty) < per_board(&five),
        "the per-board price should fall with volume: {five} vs {fifty}"
    );
    assert!(
        five["disclaimer"].as_str().is_some_and(|d| !d.is_empty()),
        "an estimate with no disclaimer reads as a quote: {five}"
    );
}

/// The DFM check reports a verdict and the board facts behind it, so a caller
/// can see what was judged rather than only the judgement.
#[tokio::test]
async fn the_dfm_check_shows_the_board_facts_behind_its_verdict() {
    let h = Harness::new();
    let board = harness::as_str(&h.fixture("test.kicad_pcb")).to_string();

    let validated = h
        .json(
            "validate_for_manufacturing",
            json!({ "board": board, "fab_house": "jlcpcb" }),
        )
        .await;
    assert_eq!(validated["fab_house"], "jlcpcb");
    assert_eq!(
        validated["board_info"]["footprint_count"], 2,
        "the fixture has two footprints: {validated}"
    );
    assert!(
        validated["verdict"].as_str().is_some_and(|v| !v.is_empty()),
        "a DFM check without a verdict is a description: {validated}"
    );
}

// ─── The last file-engine strays ─────────────────────────────────────────────

/// The netlist summary is the cheap read of a sheet: what is on it, with pins.
#[tokio::test]
async fn the_netlist_summary_lists_the_components_and_their_pins() {
    let h = Harness::new();
    let sch = harness::as_str(&h.fixture(TWO_RESISTORS)).to_string();

    let summary = h
        .json("export_netlist_summary", json!({ "schematic": sch }))
        .await;
    assert_eq!(summary["component_count"], 2);
    assert_eq!(
        summary["components"][0]["pin_count"], 2,
        "a resistor has two pins: {summary}"
    );
}

/// `check_clearance` measures between two named components, and the measurement
/// has to be the geometry — the fixture's footprints are 10 mm apart.
#[tokio::test]
async fn clearance_is_measured_between_the_components_named() {
    let h = Harness::new();
    let board = harness::as_str(&h.fixture("test.kicad_pcb")).to_string();

    let measured = h
        .json(
            "check_clearance",
            json!({ "board": board, "ref1": "R1", "ref2": "R2" }),
        )
        .await;
    assert_eq!(
        measured["distance_mm"], 10.0,
        "the fixture places them 10 mm apart: {measured}"
    );
    assert_eq!(measured["ref1"], "R1");
    assert_eq!(measured["ref2"], "R2");
}

/// `copy_routing_pattern` refuses a call that names no board rather than
/// guessing one, which is the only part of it a test without traces can pin.
#[tokio::test]
async fn copying_a_routing_pattern_needs_a_board_and_a_region() {
    let h = Harness::new();
    let board = harness::as_str(&h.fixture("test.kicad_pcb")).to_string();

    let missing_board = h
        .call("copy_routing_pattern", json!({ "src_x1": 0.0 }))
        .await;
    let reported = match missing_board {
        Ok(result) => harness::body(&result).to_string(),
        Err(e) => e.to_string(),
    };
    assert!(
        reported.contains("board"),
        "the refusal should name the missing argument: {reported}"
    );

    // A region with nothing in it copies nothing, and says so rather than
    // reporting a success that moved no copper.
    let empty = h
        .json(
            "copy_routing_pattern",
            json!({
                "board": board,
                "src_x1": 200.0, "src_y1": 200.0, "src_x2": 210.0, "src_y2": 210.0,
                "dest_x": 300.0, "dest_y": 300.0
            }),
        )
        .await;
    let text = empty.to_string();
    assert!(
        text.contains("0") || text.to_lowercase().contains("no "),
        "an empty region should report copying nothing: {empty}"
    );
}

// ─── Reaches a third party ───────────────────────────────────────────────────

/// `download_jlcpcb_database` against the real host. Everything about the
/// download path — chunk manifest, concatenation, inflation, validation, atomic
/// rename — is proved without a third party in
/// `crates/konnect-core/src/tools/integration.rs`; what only the real host can
/// tell us is whether it still publishes the artifacts under the names this
/// expects. That is what this probe is for, and it is the check to run when a
/// download starts failing (J.2.4.3).
///
/// It fetches `basic-preferred` — ~350 KB, the smallest non-empty library — so
/// the probe stays cheap. Assertions stay loose about the catalogue's contents,
/// which change daily; the schema and the plumbing are what is pinned.
/// `#[ignore]`d because it reaches the network:
///
///     cargo test -p konnect-core --test sourcing_and_manufacturing -- --ignored
#[tokio::test]
#[ignore = "reaches the network; run with --ignored"]
async fn the_published_database_still_downloads_and_answers_a_query() {
    let h = Harness::new();
    let db = h.path("jlcpcb.sqlite3");

    let downloaded = h
        .json(
            "download_jlcpcb_database",
            json!({
                "output_path": harness::as_str(&db),
                "library": "basic-preferred",
                "force": true
            }),
        )
        .await;
    assert_eq!(
        downloaded["success"],
        json!(true),
        "the published database could not be fetched: {downloaded}"
    );
    assert!(
        downloaded["part_count"].as_i64().unwrap_or(0) > 100,
        "a Basic/Preferred library with almost no parts is not the real one: {downloaded}"
    );
    assert!(db.exists(), "no database at {}", db.display());

    // The point of the download is that the query tools can read it.
    let found = h
        .json(
            "search_jlcpcb_parts",
            json!({ "query": "0402", "output_path": harness::as_str(&db), "limit": 5 }),
        )
        .await;
    assert!(
        found["count"].as_u64().unwrap_or(0) > 0,
        "the downloaded database answered nothing for '0402': {found}"
    );
    let part = &found["results"][0];
    assert!(
        part["lcsc"].as_str().is_some_and(|id| id.starts_with('C')),
        "an LCSC part number is expected: {part}"
    );
    assert!(
        part["stock"].as_i64().is_some(),
        "the published Stock is text and has to come back parsed: {part}"
    );
}

/// `snapshot_project` renders PDFs through `kicad-cli`, so it needs one.
#[tokio::test]
#[ignore = "requires kicad-cli; run with --ignored"]
async fn a_project_snapshot_writes_its_pdfs() {
    let cli = std::env::var("KICAD_CLI").unwrap_or_else(|_| {
        let default = r"C:\Users\FlowUP\AppData\Local\Programs\KiCad\10.0\bin\kicad-cli.exe";
        assert!(
            std::path::Path::new(default).exists(),
            "set KICAD_CLI to run this probe"
        );
        default.to_string()
    });
    let h = Harness::with_kicad_cli(cli);
    let sch = h.fixture(TWO_RESISTORS);
    let out = h.path("snapshots");

    let snapshot = h
        .json(
            "snapshot_project",
            json!({
                "schematic": harness::as_str(&sch),
                "output_dir": harness::as_str(&out),
                "label": "j23"
            }),
        )
        .await;

    let written: Vec<_> = std::fs::read_dir(&out)
        .expect("the snapshot directory exists")
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        written.iter().any(|name| name.ends_with(".pdf")),
        "no PDF in the snapshot directory ({written:?}): {snapshot}"
    );
    assert!(
        written.iter().any(|name| name.contains("j23")),
        "the label should be in the filename ({written:?})"
    );
}
