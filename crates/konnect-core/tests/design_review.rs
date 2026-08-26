//! The heuristic audits, exercised end to end (J.2.3.4).
//!
//! Six `review` tools shipped with no test that runs. What is asserted here is
//! deliberately narrow: that each audit **finds the thing it exists to find**,
//! **says nothing when there is nothing**, and reports it in the shape a caller
//! can act on — a finding with a severity, an issue and a recommendation.
//!
//! What is not asserted is that the advice is right. These are heuristics, and
//! the matrix says so: `run_design_review` and its audits are a checklist, and
//! ERC/DRC decide whether a design is sound (INV1). A test that pinned the
//! wording of the advice would be pinning an opinion.
//!
//! No `kicad-cli` and no running KiCAD.

mod harness;

use harness::{pins, Harness, TWO_RESISTORS};
use serde_json::{json, Value};

/// Two resistors joined by one wire, with `net` naming it.
async fn sheet_with_net(h: &Harness, net: &str) -> String {
    let sch = harness::as_str(&h.fixture(TWO_RESISTORS)).to_string();
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
        json!({ "schematic": sch, "net": net, "x": 107.95, "y": 46.99 }),
    )
    .await;
    sch
}

/// Every finding an audit reports has to be actionable: what is wrong, how bad,
/// and what to do. A bare string is not a finding.
fn assert_findings_are_actionable(findings: &Value, audit: &str) {
    for finding in findings.as_array().expect("findings is a list") {
        for key in ["severity", "issue", "recommendation"] {
            assert!(
                finding[key].as_str().is_some_and(|s| !s.is_empty()),
                "a '{audit}' finding has no '{key}': {finding}"
            );
        }
        assert!(
            ["error", "warning", "info"].contains(&finding["severity"].as_str().unwrap()),
            "'{audit}' reported an unknown severity: {finding}"
        );
    }
}

// ─── The audits, one at a time ───────────────────────────────────────────────

/// The BOM audit finds the missing footprints, and stops reporting one when it
/// is assigned. An audit that cannot be satisfied is noise.
#[tokio::test]
async fn the_bom_audit_finds_missing_footprints_and_lets_go_when_they_are_assigned() {
    let h = Harness::new();
    let sch = harness::as_str(&h.fixture(TWO_RESISTORS)).to_string();

    let before = h
        .json("check_bom_health", json!({ "schematic": sch }))
        .await;
    assert_eq!(before["total_components"], 2);
    assert_eq!(
        before["missing_footprint"], 2,
        "neither resistor has a footprint: {before}"
    );
    assert_findings_are_actionable(&before["findings"], "bom_health");

    h.json(
        "add_component_annotation",
        json!({
            "schematic": sch,
            "reference": "R1",
            "key": "Footprint",
            "value": "Resistor_SMD:R_0603_1608Metric"
        }),
    )
    .await;

    let after = h
        .json("check_bom_health", json!({ "schematic": sch }))
        .await;
    assert_eq!(
        after["missing_footprint"], 1,
        "R1 now has a footprint and should have dropped out: {after}"
    );
    assert_eq!(
        after["findings"][0]["component"], "R2",
        "the remaining finding should be R2's: {after}"
    );
}

/// The power-rail audit only has something to say once there is a power rail.
/// The contrast is the test: a checker that always fires proves nothing.
#[tokio::test]
async fn the_power_rail_audit_is_quiet_until_there_is_a_rail() {
    let h = Harness::new();

    let plain = Harness::new();
    let signal = sheet_with_net(&plain, "SIGNAL").await;
    let quiet = plain
        .json("audit_power_rails", json!({ "schematic": signal }))
        .await;
    assert_eq!(
        quiet["power_nets"].as_array().map(|n| n.len()),
        Some(0),
        "SIGNAL is not a power rail: {quiet}"
    );
    assert_eq!(quiet["findings"].as_array().map(|f| f.len()), Some(0));

    let vcc = sheet_with_net(&h, "VCC").await;
    let loud = h
        .json("audit_power_rails", json!({ "schematic": vcc }))
        .await;
    assert_eq!(
        loud["power_nets"][0], "VCC",
        "VCC should be recognised as a rail: {loud}"
    );
    assert!(
        !loud["findings"].as_array().unwrap().is_empty(),
        "a rail with no decoupling and no test point has findings: {loud}"
    );
    assert_findings_are_actionable(&loud["findings"], "power_rails");
}

/// The decoupling audit counts power pins, and a sheet of plain resistors has
/// none — so it reports nothing rather than inventing a rule to break.
#[tokio::test]
async fn the_decoupling_audit_reports_nothing_when_there_are_no_power_pins() {
    let h = Harness::new();
    let sch = sheet_with_net(&h, "VCC").await;

    let audited = h
        .json("audit_decoupling", json!({ "schematic": sch }))
        .await;
    assert_eq!(audited["audit"], "decoupling");
    assert_eq!(
        audited["total_power_pins"], 0,
        "Device:R has no power pins: {audited}"
    );
    assert_findings_are_actionable(&audited["findings"], "decoupling");
}

/// The connection audit reports a clean sheet as clean. Like the other
/// in-process connectivity readers this is advisory (E7) — what is pinned is
/// that it can stay silent, not that its silence is a verdict.
#[tokio::test]
async fn the_connection_audit_stays_silent_on_a_wired_sheet() {
    let h = Harness::new();
    let sch = sheet_with_net(&h, "SIGNAL").await;

    let audited = h
        .json("audit_connections", json!({ "schematic": sch }))
        .await;
    assert_eq!(audited["audit"], "connections");
    assert_eq!(
        audited["findings"].as_array().map(|f| f.len()),
        Some(0),
        "two pins on one wire is not a connection problem: {audited}"
    );
}

/// The manufacturing audit is per fab house, and it says which one it judged
/// against — a DFM answer without its target is unusable.
#[tokio::test]
async fn the_manufacturing_audit_names_the_fab_house_it_judged_against() {
    let h = Harness::new();
    let board = harness::as_str(&h.fixture("test.kicad_pcb")).to_string();

    let default = h
        .json("audit_manufacturing", json!({ "board": board }))
        .await;
    assert_eq!(
        default["fab_house"], "jlcpcb",
        "jlcpcb is the documented default: {default}"
    );
    assert_eq!(
        default["components"]["front"], 2,
        "the fixture's two footprints are on the front: {default}"
    );

    let oshpark = h
        .json(
            "audit_manufacturing",
            json!({ "board": board, "fab_house": "oshpark" }),
        )
        .await;
    assert_eq!(oshpark["fab_house"], "oshpark");
    assert_findings_are_actionable(&oshpark["findings"], "manufacturing");
}

// ─── The aggregate ───────────────────────────────────────────────────────────

/// `run_design_review` is the other audits put together, and it has to carry
/// their findings rather than re-deriving them: each finding names the audit it
/// came from, and both a BOM issue and a power issue reach the same report.
#[tokio::test]
async fn the_design_review_aggregates_the_audits_it_runs() {
    let h = Harness::new();
    let sch = sheet_with_net(&h, "VCC").await;

    let review = h
        .json("run_design_review", json!({ "schematic": sch }))
        .await;
    let review = &review["design_review"];
    let findings = review["findings"]
        .as_array()
        .expect("a review has findings");

    let audits: Vec<&str> = findings
        .iter()
        .filter_map(|f| f["audit"].as_str())
        .collect();
    assert!(
        audits.contains(&"bom_health") && audits.contains(&"power_rails"),
        "the review should carry both audits' findings: {review}"
    );
    assert!(
        review["verdict"].as_str().is_some_and(|v| !v.is_empty()),
        "a review without a verdict is a list, not a review: {review}"
    );
    assert_eq!(
        review["errors"].as_u64().unwrap_or(0) as usize,
        findings.iter().filter(|f| f["severity"] == "error").count(),
        "the error count and the errors listed disagree: {review}"
    );
}

/// The severity filter is what keeps a review readable, so it has to actually
/// filter: asking for errors must not return warnings or info.
#[tokio::test]
async fn the_severity_filter_drops_everything_below_it() {
    let h = Harness::new();
    let sch = sheet_with_net(&h, "VCC").await;

    let errors_only = h
        .json(
            "run_design_review",
            json!({ "schematic": sch, "severity_filter": "error" }),
        )
        .await;
    let review = &errors_only["design_review"];
    assert_eq!(review["severity_filter"], "error");
    for finding in review["findings"].as_array().expect("findings is a list") {
        assert_eq!(
            finding["severity"], "error",
            "the filter asked for errors and let this through: {finding}"
        );
    }

    // The dropped findings are still counted — a filter that hid them from the
    // totals would make the report look cleaner than the design is.
    let everything = h
        .json(
            "run_design_review",
            json!({ "schematic": sch, "severity_filter": "info" }),
        )
        .await;
    let all = everything["design_review"]["findings"]
        .as_array()
        .map(|f| f.len())
        .unwrap_or(0);
    assert!(
        all > review["findings"].as_array().map(|f| f.len()).unwrap_or(0),
        "'info' should return more than 'error': {everything}"
    );
}

// ─── DRC evidence in the verdict ─────────────────────────────────────────────

/// A schematic-only review never had a board to run DRC on, and must come
/// back exactly as it did before DRC entered the verdict: the three original
/// verdicts, and no `drc` key claiming anything either way.
#[tokio::test]
async fn a_schematic_only_review_is_untouched_by_the_drc_gate() {
    let h = Harness::new();
    let sch = sheet_with_net(&h, "VCC").await;
    let review = h
        .json("run_design_review", json!({ "schematic": sch }))
        .await;
    let review = &review["design_review"];
    assert!(
        review.get("drc").is_none(),
        "no board was reviewed, so there is nothing to say about DRC: {review}"
    );
    let verdict = review["verdict"].as_str().unwrap_or("");
    assert!(
        verdict.starts_with("NOT READY")
            || verdict.starts_with("NEEDS ATTENTION")
            || verdict.starts_with("LOOKS GOOD"),
        "the schematic-only vocabulary changed: {verdict}"
    );
    assert!(
        review["findings"]
            .as_array()
            .expect("findings is a list")
            .iter()
            .all(|f| f["audit"] != "drc"),
        "a schematic review invented a DRC audit: {review}"
    );
}

/// With a board in play and no `kicad-cli` behind the harness, the review has
/// not checked the board against a single design rule. Saying "LOOKS GOOD" is
/// the false clearance this item exists to remove.
#[tokio::test]
async fn a_board_review_without_drc_evidence_cannot_look_good() {
    let h = Harness::new();
    let board = harness::as_str(&h.fixture("test.kicad_pcb")).to_string();
    let review = h.json("run_design_review", json!({ "board": board })).await;
    let review = &review["design_review"];
    assert!(
        review["drc"].is_null(),
        "an unmeasured DRC is null, not an object of zeroes: {review}"
    );
    assert_eq!(
        review["verdict"], "INCOMPLETE — DRC did not run, so the board is unverified",
        "{review}"
    );
}

// ─── Sheet hierarchy coverage (P.6.8.4) ──────────────────────────────────────
//
// `hier_root.kicad_sch` places two sub-sheets: `hier_sub_clean.kicad_sch`
// (nothing on it) and `hier_sub_defect.kicad_sch` (two resistors on a VCC
// net with no decoupling cap — the same defect shape
// `the_power_rail_audit_is_quiet_until_there_is_a_rail` uses). `kicad-cli sch
// erc` (KiCad 10.0.3) accepts the full hierarchy from `hier_root.kicad_sch`
// with 2 violations, both unrelated pin-unconnected findings — the fixture
// loads.

/// A defect that lives only on a sub-sheet must not be invisible to a review
/// run on the root. Before this item, `run_design_review` audited only the
/// path it was handed — the root, which carries no defect of its own — and
/// came back clean while the sub-sheet's missing decoupling sat unseen.
#[tokio::test]
async fn a_review_on_the_root_finds_a_defect_that_lives_only_on_a_sub_sheet() {
    let h = Harness::new();
    h.fixture("hier_sub_clean.kicad_sch");
    h.fixture("hier_sub_defect.kicad_sch");
    let root = harness::as_str(&h.fixture("hier_root.kicad_sch")).to_string();

    let review = h
        .json("run_design_review", json!({ "schematic": root }))
        .await;
    let review = &review["design_review"];
    let findings = review["findings"].as_array().expect("findings is a list");

    assert!(
        findings.iter().any(|f| {
            f["issue"]
                .as_str()
                .is_some_and(|s| s.contains("VCC") && s.contains("decoupling"))
                && f["sheet"]
                    .as_str()
                    .is_some_and(|s| s.contains("hier_sub_defect"))
        }),
        "the sub-sheet's missing decoupling should be in the review, tagged with its sheet: {review}"
    );
    assert_eq!(
        review["schematic_coverage"]["sheets_reachable"], 3,
        "root + two sub-sheets: {review}"
    );
    assert_eq!(
        review["schematic_coverage"]["sheets_audited"], 3,
        "every reachable sheet loaded: {review}"
    );
    assert!(
        review["verdict"]
            .as_str()
            .unwrap_or("")
            .starts_with("NOT READY"),
        "a missing decoupling cap is an error-level finding: {review}"
    );
}

/// A `(sheet …)` reference to a file that is not on disk must not be
/// silently skipped: the coverage numbers say so, the missing file is named,
/// and the verdict cannot claim "LOOKS GOOD" for ground it never covered.
#[tokio::test]
async fn a_sheet_reference_with_no_file_on_disk_makes_the_verdict_incomplete() {
    let h = Harness::new();
    h.fixture("hier_sub_clean.kicad_sch");
    // hier_sub_missing.kicad_sch is deliberately never fixtured.
    let root = harness::as_str(&h.fixture("hier_root_missing_sub.kicad_sch")).to_string();

    let review = h
        .json("run_design_review", json!({ "schematic": root }))
        .await;
    let review = &review["design_review"];

    assert_eq!(
        review["schematic_coverage"]["sheets_reachable"], 3,
        "root + the clean sub-sheet + the missing one, still counted: {review}"
    );
    assert_eq!(
        review["schematic_coverage"]["sheets_audited"], 2,
        "only root and the clean sub-sheet actually loaded: {review}"
    );
    let unaudited = review["schematic_coverage"]["unaudited"]
        .as_array()
        .expect("unaudited is a list");
    assert!(
        unaudited.iter().any(|u| u["sheet"]
            .as_str()
            .is_some_and(|s| s.contains("hier_sub_missing"))),
        "the missing sheet should be named: {review}"
    );
    assert!(
        review["verdict"]
            .as_str()
            .is_some_and(|v| v.starts_with("INCOMPLETE") && v.contains("hier_sub_missing")),
        "an unaudited sheet must not be papered over: {review}"
    );
}

/// The ordinary case — one file, nothing missing — must still report full
/// coverage and reach "LOOKS GOOD" exactly as it did before sheets were
/// walked at all.
#[tokio::test]
async fn a_single_sheet_review_reports_full_coverage() {
    let h = Harness::new();
    // A non-power net keeps the power-rail and decoupling audits silent; a
    // footprint on both resistors keeps the BOM audit silent too, so the only
    // thing left to prove is coverage and the verdict it allows.
    let sch = sheet_with_net(&h, "SIGNAL").await;
    for reference in ["R1", "R2"] {
        h.json(
            "add_component_annotation",
            json!({
                "schematic": sch,
                "reference": reference,
                "key": "Footprint",
                "value": "Resistor_SMD:R_0603_1608Metric"
            }),
        )
        .await;
    }

    let review = h
        .json("run_design_review", json!({ "schematic": sch }))
        .await;
    let review = &review["design_review"];
    assert_eq!(
        review["schematic_coverage"]["sheets_reachable"], 1,
        "{review}"
    );
    assert_eq!(
        review["schematic_coverage"]["sheets_audited"], 1,
        "{review}"
    );
    assert_eq!(
        review["schematic_coverage"]["unaudited"]
            .as_array()
            .map(|a| a.len()),
        Some(0),
        "{review}"
    );
    assert_eq!(
        review["verdict"], "LOOKS GOOD — no critical issues found",
        "two unconnected resistors with no rails and no BOM gaps: {review}"
    );
}
