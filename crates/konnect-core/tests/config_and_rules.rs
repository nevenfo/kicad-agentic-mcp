//! Configuration and design rules, exercised end to end (J.2.3.3).
//!
//! Twelve `config` and `rules` tools shipped with no test that runs. Two things
//! make this lot different from the others:
//!
//! * **The user-scoped config is a real file in the user's profile.** A test
//!   that wrote there would edit the machine it runs on, so `APPDATA` / `HOME`
//!   is redirected into a temporary directory for the duration, under a mutex
//!   because the whole binary shares one environment.
//! * **Design rules exist at two scopes**, and the interesting behaviour is how
//!   they combine: `get_effective_config` is the merge, and a merge is exactly
//!   where a precedence bug hides.
//!
//! No `kicad-cli` and no running KiCAD.

mod harness;

use std::sync::{Mutex, MutexGuard};

use harness::Harness;
use serde_json::json;

/// The environment is process-wide, so anything that redirects it takes this
/// first. Poisoning is irrelevant here — a panicking test still leaves a usable
/// lock for the next one.
static CONFIG_HOME: Mutex<()> = Mutex::new(());

/// Point the user-config directory at a temporary directory and keep it there
/// until the returned guard drops.
fn redirected_user_config() -> (tempfile::TempDir, MutexGuard<'static, ()>) {
    let guard = CONFIG_HOME.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().expect("tempdir");
    // `user_config_dir()` reads APPDATA on Windows and HOME elsewhere; setting
    // both keeps this test honest on either.
    std::env::set_var("APPDATA", dir.path());
    std::env::set_var("HOME", dir.path());
    (dir, guard)
}

// ─── User and project configuration ──────────────────────────────────────────

/// A saved key comes back, and the surrounding config is not flattened by the
/// write — `save_user_config` takes a dot path, so a shallow write would
/// silently drop everything else under the same parent.
#[tokio::test]
async fn a_user_config_key_is_saved_and_read_back_without_losing_its_siblings() {
    let (_home, _guard) = redirected_user_config();
    let h = Harness::new();

    h.json(
        "save_user_config",
        json!({ "key_path": "fab_constraints.fab_house", "value": "jlcpcb" }),
    )
    .await;
    h.json(
        "save_user_config",
        json!({ "key_path": "fab_constraints.min_trace_width", "value": 0.127 }),
    )
    .await;

    let loaded = h.json("load_user_config", json!({})).await;
    let fab = &loaded["config"]["fab_constraints"];
    let fab = if fab.is_null() {
        &loaded["fab_constraints"]
    } else {
        fab
    };
    assert_eq!(fab["fab_house"], "jlcpcb");
    assert_eq!(
        fab["min_trace_width"], 0.127,
        "the second write dropped the first key: {loaded}"
    );
}

/// Project config lives beside the project, not in the user profile, which is
/// what makes it shareable with the rest of a team.
#[tokio::test]
async fn project_config_is_written_beside_the_project() {
    let (_home, _guard) = redirected_user_config();
    let h = Harness::new();
    let project = harness::as_str(h.dir.path()).to_string();

    h.json(
        "save_project_config",
        json!({
            "project_dir": project,
            "key_path": "naming_conventions.net_prefix",
            "value": "N_"
        }),
    )
    .await;

    let on_disk = h.dir.path().join(".konnect").join("project.json");
    assert!(
        on_disk.is_file(),
        "the project config is not at {}",
        on_disk.display()
    );

    let loaded = h
        .json("load_project_config", json!({ "project_dir": project }))
        .await;
    assert!(
        loaded.to_string().contains("N_"),
        "the saved value did not come back: {loaded}"
    );
}

/// The effective config is the merge, and the project scope wins. If it did
/// not, a project could never override a machine-wide default — which is the
/// only reason both scopes exist.
#[tokio::test]
async fn the_project_scope_wins_in_the_effective_config() {
    let (_home, _guard) = redirected_user_config();
    let h = Harness::new();
    let project = harness::as_str(h.dir.path()).to_string();

    h.json(
        "save_user_config",
        json!({ "key_path": "fab_constraints.fab_house", "value": "oshpark" }),
    )
    .await;
    h.json(
        "save_project_config",
        json!({
            "project_dir": project,
            "key_path": "fab_constraints.fab_house",
            "value": "jlcpcb"
        }),
    )
    .await;

    let effective = h
        .json("get_effective_config", json!({ "project_dir": project }))
        .await;
    let text = effective.to_string();
    assert!(
        text.contains("jlcpcb"),
        "the project value is missing from the merge: {effective}"
    );
    assert!(
        !text.contains("oshpark"),
        "the user value survived a project override: {effective}"
    );
}

/// Design rules are plain English and scoped. A project rule must not leak into
/// another project, and `list_design_rules` has to show both scopes at once —
/// a caller reading only one of them would design against half the constraints.
#[tokio::test]
async fn design_rules_are_scoped_and_listed_together() {
    let (_home, _guard) = redirected_user_config();
    let h = Harness::new();
    let project = harness::as_str(h.dir.path()).to_string();

    h.json(
        "add_design_rule",
        json!({ "rule": "Never route power under a crystal", "scope": "user" }),
    )
    .await;
    h.json(
        "add_design_rule",
        json!({
            "rule": "This board is 2-layer only",
            "scope": "project",
            "project_dir": project
        }),
    )
    .await;

    let listed = h
        .json("list_design_rules", json!({ "project_dir": project }))
        .await;
    let text = listed.to_string();
    assert!(text.contains("crystal"), "the user rule is missing: {listed}");
    assert!(
        text.contains("2-layer"),
        "the project rule is missing: {listed}"
    );

    // A different project sees the user rule and not the other project's.
    let elsewhere = tempfile::tempdir().expect("tempdir");
    let other = h
        .json(
            "list_design_rules",
            json!({ "project_dir": harness::as_str(elsewhere.path()) }),
        )
        .await;
    let other_text = other.to_string();
    assert!(
        other_text.contains("crystal"),
        "a user rule applies to every project: {other}"
    );
    assert!(
        !other_text.contains("2-layer"),
        "a project rule leaked into another project: {other}"
    );
}

// ─── Board design rules ──────────────────────────────────────────────────────

/// Constraints written to the board come back from the reader. The pair has to
/// agree: a writer whose values the reader cannot see is worse than neither.
#[tokio::test]
async fn board_design_rules_round_trip_through_the_file() {
    let h = Harness::new();
    let board = harness::as_str(&h.fixture("test.kicad_pcb")).to_string();

    h.json(
        "set_design_rules",
        json!({
            "board": board,
            "min_clearance": 0.15,
            "min_trace_width": 0.13,
            "min_via_drill": 0.3,
            "min_via_size": 0.6
        }),
    )
    .await;

    let rules = h.json("get_design_rules", json!({ "board": board })).await;
    let text = rules.to_string();
    for expected in ["0.15", "0.13", "0.3", "0.6"] {
        assert!(
            text.contains(expected),
            "{expected} was set and is not reported back: {rules}"
        );
    }
}

/// A per-layer constraint is written against the layer it names.
#[tokio::test]
async fn a_layer_constraint_names_its_layer() {
    let h = Harness::new();
    let board = harness::as_str(&h.fixture("test.kicad_pcb")).to_string();

    h.json(
        "set_layer_constraints",
        json!({
            "board": board,
            "layer": "F.Cu",
            "min_clearance": 0.25,
            "min_trace_width": 0.2
        }),
    )
    .await;

    let text = std::fs::read_to_string(&board).expect("the board is readable");
    assert!(
        text.contains("F.Cu"),
        "the constrained layer is not named in the file"
    );
    assert!(
        text.contains("0.25"),
        "the clearance was not written:\n{text}"
    );
}

/// A netclass is created with the widths it was given, and a net assigned to it
/// is recorded as a member — the assignment is the half that makes the class
/// mean anything.
#[tokio::test]
async fn a_netclass_takes_its_widths_and_its_members() {
    let h = Harness::new();
    let board = harness::as_str(&h.fixture("test.kicad_pcb")).to_string();

    h.json(
        "create_netclass",
        json!({
            "board": board,
            "name": "Power",
            "clearance": 0.3,
            "trace_width": 0.5,
            "via_drill": 0.4,
            "via_diameter": 0.8
        }),
    )
    .await;
    h.json(
        "add_net",
        json!({ "board": board, "net_name": "VBUS" }),
    )
    .await;
    h.json(
        "assign_net_to_class",
        json!({ "board": board, "net_name": "VBUS", "netclass": "Power" }),
    )
    .await;

    let text = std::fs::read_to_string(&board).expect("the board is readable");
    assert!(text.contains("Power"), "the netclass is not in the file");
    assert!(
        text.contains("0.5"),
        "the netclass trace width was not written:\n{text}"
    );
    assert!(
        text.contains("VBUS"),
        "the net was not recorded as a member of the class:\n{text}"
    );
}
