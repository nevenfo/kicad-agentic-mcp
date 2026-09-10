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

use harness::Harness;
use serde_json::{json, Value};

/// The environment is process-wide, so anything that redirects it takes this
/// first. A `tokio` mutex rather than `std`'s (E10): every holder below keeps
/// the guard across the harness's `.await`s, and a `std::sync::MutexGuard`
/// there is not `Send`. It also cannot be poisoned, so a panicking test still
/// leaves a usable lock for the next one.
static CONFIG_HOME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Point the user-config directory at a temporary directory and keep it there
/// until the returned guard drops.
async fn redirected_user_config() -> (tempfile::TempDir, tokio::sync::MutexGuard<'static, ()>) {
    let guard = CONFIG_HOME.lock().await;
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
    let (_home, _guard) = redirected_user_config().await;
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
    let (_home, _guard) = redirected_user_config().await;
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
    let (_home, _guard) = redirected_user_config().await;
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
    let (_home, _guard) = redirected_user_config().await;
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
    assert!(
        text.contains("crystal"),
        "the user rule is missing: {listed}"
    );
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

/// Set four distinct constraints, read four distinct constraints, and leave
/// the board file alone.
///
/// The values are deliberately far apart. The implementation this replaced
/// would have passed a test using one value four times while writing a single
/// key, and the four numbers here cannot all be right by accident.
///
/// What this test cannot show is whether KiCAD agrees, because it only asks
/// our own reader — which is exactly how the previous implementation was
/// green while `kicad-cli` refused the board it produced (X1). That question
/// belongs to `kicad_applies_the_constraints_it_was_given`, below.
#[tokio::test]
async fn design_rules_round_trip_through_the_project_file() {
    let h = Harness::new();
    let board = h.write("rules.kicad_pcb", harness::CLEARANCE_BOARD);
    h.write("rules.kicad_pro", harness::BLANK_PROJECT);
    let board = harness::as_str(&board).to_string();
    let before = std::fs::read_to_string(&board).expect("the board is readable");

    h.json(
        "set_design_rules",
        json!({
            "board": board,
            "min_clearance": 0.15,
            "min_track_width": 0.13,
            "min_via_diameter": 0.6,
            "min_through_hole_diameter": 0.3,
        }),
    )
    .await;

    let reported = h.json("get_design_rules", json!({ "board": board })).await;
    let rules = &reported["rules"];
    assert_eq!(rules["min_clearance"], json!(0.15));
    assert_eq!(rules["min_track_width"], json!(0.13));
    assert_eq!(rules["min_via_diameter"], json!(0.6));
    assert_eq!(rules["min_through_hole_diameter"], json!(0.3));

    assert_eq!(
        std::fs::read_to_string(&board).expect("the board is readable"),
        before,
        "the board file was modified; KiCAD keeps constraints in the project file"
    );
}

/// The project file keeps everything the call did not name.
#[tokio::test]
async fn setting_one_constraint_disturbs_nothing_else() {
    let h = Harness::new();
    let board = h.write("rules.kicad_pcb", harness::CLEARANCE_BOARD);
    let project = h.write(
        "rules.kicad_pro",
        r#"{
  "board": {
    "design_settings": {
      "defaults": {
        "board_outline_line_width": 0.05
      },
      "rules": {
        "min_hole_to_hole": 0.25,
        "min_text_height": 0.8
      }
    }
  },
  "meta": {
    "filename": "rules.kicad_pro",
    "version": 3
  },
  "sheets": [["deadbeef", "Root"]]
}
"#,
    );

    h.json(
        "set_design_rules",
        json!({ "board": harness::as_str(&board), "min_clearance": 0.42 }),
    )
    .await;

    let after: Value = serde_json::from_str(&std::fs::read_to_string(&project).expect("readable"))
        .expect("still JSON");
    let rules = &after["board"]["design_settings"]["rules"];
    assert_eq!(rules["min_clearance"], json!(0.42), "the ask did not land");
    assert_eq!(
        rules["min_hole_to_hole"],
        json!(0.25),
        "a rule nobody named was changed"
    );
    assert_eq!(rules["min_text_height"], json!(0.8));
    assert_eq!(
        after["board"]["design_settings"]["defaults"]["board_outline_line_width"],
        json!(0.05),
        "a setting outside `rules` was changed"
    );
    assert_eq!(after["sheets"], json!([["deadbeef", "Root"]]));
    assert_eq!(after["meta"]["filename"], json!("rules.kicad_pro"));
}

/// The three argument names that name nothing in KiCAD are refused, and say
/// what to ask for instead.
///
/// Aliasing them silently is the tempting option and the wrong one:
/// `min_via_drill` is a constraint KiCAD does not have, so a caller who sends
/// it believes something that is not true, and quietly applying it to
/// `min_through_hole_diameter` would confirm the belief.
#[tokio::test]
async fn the_old_argument_names_are_refused_by_name() {
    let h = Harness::new();
    let board = h.write("rules.kicad_pcb", harness::CLEARANCE_BOARD);
    h.write("rules.kicad_pro", harness::BLANK_PROJECT);
    let board = harness::as_str(&board).to_string();

    for (old, replacement) in [
        ("min_trace_width", "min_track_width"),
        ("min_via_size", "min_via_diameter"),
        ("min_via_drill", "min_through_hole_diameter"),
    ] {
        let result = h
            .call("set_design_rules", json!({ "board": board, old: 0.3 }))
            .await
            .expect("the tool answered");
        assert!(result.is_error, "`{old}` was accepted");
        let text = harness::body(&result).to_string();
        assert!(
            text.contains(replacement),
            "the refusal of `{old}` does not name `{replacement}`: {text}"
        );
    }
}

/// A board whose project file is missing is reported, not repaired.
#[tokio::test]
async fn a_missing_project_file_is_an_error_not_a_new_file() {
    let h = Harness::new();
    let board = h.write("lonely.kicad_pcb", harness::CLEARANCE_BOARD);
    let project = h.path("lonely.kicad_pro");

    let result = h
        .call(
            "set_design_rules",
            json!({ "board": harness::as_str(&board), "min_clearance": 0.2 }),
        )
        .await
        .expect("the tool answered");

    assert!(result.is_error, "a missing project file passed silently");
    assert!(
        !project.exists(),
        "the tool invented a project file instead of reporting the broken project"
    );
}

/// KiCAD applies the constraints we wrote — the claim no amount of reading our
/// own bytes back can support.
///
/// The oracle is a count, not a string: `kicad-cli`'s violation descriptions
/// are translated into the user's language, its `type` identifiers are not.
/// The fixture's two tracks leave a 0.75 mm gap, so a clearance rule below it
/// is quiet and one above it is not. If KiCAD were ignoring the project file
/// — or refusing it — both runs would return the same counts and this fails.
#[tokio::test]
#[ignore = "requires kicad-cli; run with --ignored"]
async fn kicad_applies_the_constraints_it_was_given() {
    let h = Harness::new();
    let board = h.write("arbitrated.kicad_pcb", harness::CLEARANCE_BOARD);
    h.write("arbitrated.kicad_pro", harness::BLANK_PROJECT);
    let board_arg = harness::as_str(&board).to_string();

    h.json(
        "set_design_rules",
        json!({ "board": board_arg, "min_clearance": 0.2 }),
    )
    .await;
    let quiet = harness::kicad_reloads(&board);
    assert_eq!(
        quiet.get("clearance"),
        None,
        "0.2 mm is under the fixture's 0.75 mm gap and should not violate: {quiet:?}"
    );

    h.json(
        "set_design_rules",
        json!({
            "board": board_arg,
            "min_clearance": 1.5,
            "min_track_width": 0.13,
            "min_via_diameter": 0.6,
            "min_through_hole_diameter": 0.3,
        }),
    )
    .await;
    let loud = harness::kicad_reloads(&board);
    assert_eq!(
        loud.get("clearance"),
        Some(&1),
        "1.5 mm is over the fixture's 0.75 mm gap: KiCAD did not apply the rule: {loud:?}"
    );

    // Everything else KiCAD found is unchanged: the rule moved, the board did
    // not.
    assert_eq!(quiet.get("track_dangling"), loud.get("track_dangling"));

    let reported = h
        .json("get_design_rules", json!({ "board": board_arg }))
        .await;
    assert_eq!(reported["rules"]["min_clearance"], json!(1.5));
    assert_eq!(reported["rules"]["min_track_width"], json!(0.13));
    assert_eq!(reported["rules"]["min_via_diameter"], json!(0.6));
    assert_eq!(reported["rules"]["min_through_hole_diameter"], json!(0.3));
}

/// A per-layer constraint becomes a KiCAD rule, in the file KiCAD reads rules
/// from, and the board is not touched.
#[tokio::test]
async fn a_layer_constraint_becomes_a_rule_in_the_rules_file() {
    let h = Harness::new();
    let board = h.write("layers.kicad_pcb", harness::CLEARANCE_BOARD);
    let before = std::fs::read_to_string(&board).expect("the board is readable");

    h.json(
        "set_layer_constraints",
        json!({
            "board": harness::as_str(&board),
            "layer": "F.Cu",
            "min_clearance": 0.25,
            "min_trace_width": 0.2
        }),
    )
    .await;

    let rules = std::fs::read_to_string(h.path("layers.kicad_dru")).expect("the rules file exists");
    assert!(
        rules.starts_with("(version 1)"),
        "no rules-file header: {rules}"
    );
    assert!(
        rules.contains("(constraint clearance (min 0.25mm))"),
        "{rules}"
    );
    assert!(
        rules.contains("(constraint track_width (min 0.2mm))"),
        "{rules}"
    );
    assert!(rules.contains("A.Layer == 'F.Cu'"), "{rules}");

    assert_eq!(
        std::fs::read_to_string(&board).expect("the board is readable"),
        before,
        "the board was edited; a (rule ...) there is what KiCAD refuses to load"
    );
}

/// Setting the same constraint twice replaces the rule instead of stacking a
/// second one, and a rule somebody else wrote — comments included — survives.
#[tokio::test]
async fn a_second_call_replaces_its_own_rule_and_spares_everyone_elses() {
    let h = Harness::new();
    let board = h.write("layers.kicad_pcb", harness::CLEARANCE_BOARD);
    h.write(
        "layers.kicad_dru",
        "(version 1)

# Hand-written, and it stays.
(rule \"mine\"
	(constraint clearance (min 0.9mm))
	(condition \"A.memberOfFootprint('U1')\"))
",
    );

    for mm in [0.25, 0.42] {
        h.json(
            "set_layer_constraints",
            json!({ "board": harness::as_str(&board), "layer": "F.Cu", "min_clearance": mm }),
        )
        .await;
    }

    let rules = std::fs::read_to_string(h.path("layers.kicad_dru")).expect("readable");
    assert_eq!(
        rules.matches("konnect F.Cu clearance").count(),
        1,
        "the rule was stacked rather than replaced: {rules}"
    );
    assert!(
        rules.contains("(min 0.42mm)"),
        "the second call did not win: {rules}"
    );
    assert!(
        !rules.contains("(min 0.25mm)"),
        "the first call is still there: {rules}"
    );
    assert!(
        rules.contains("# Hand-written, and it stays."),
        "a comment was lost: {rules}"
    );
    assert!(
        rules.contains("A.memberOfFootprint('U1')"),
        "someone else's rule was lost: {rules}"
    );
}

/// KiCAD reads the rule and enforces it.
///
/// The fixture leaves 0.75 mm between two tracks and the project file asks for
/// 0.1 mm, so nothing violates until the per-layer rule says 1.5 mm. If the
/// rule went somewhere KiCAD does not read — as it used to, into the board's
/// `(setup ...)`, which stops the board loading at all — the count does not
/// move and this fails.
#[tokio::test]
#[ignore = "requires kicad-cli; run with --ignored"]
async fn kicad_enforces_the_layer_rule_it_was_given() {
    let h = Harness::new();
    let board = h.write("layers.kicad_pcb", harness::CLEARANCE_BOARD);
    h.write("layers.kicad_pro", harness::BLANK_PROJECT);
    h.json(
        "set_design_rules",
        json!({ "board": harness::as_str(&board), "min_clearance": 0.1 }),
    )
    .await;
    let quiet = harness::kicad_reloads(&board);
    assert_eq!(quiet.get("clearance"), None, "unexpectedly loud: {quiet:?}");

    h.json(
        "set_layer_constraints",
        json!({ "board": harness::as_str(&board), "layer": "F.Cu", "min_clearance": 1.5 }),
    )
    .await;

    let loud = harness::kicad_reloads(&board);
    assert_eq!(
        loud.get("clearance"),
        Some(&1),
        "KiCAD did not enforce the layer rule: {loud:?}"
    );
}

/// A netclass is created with the widths it was given, and a net assigned to
/// it is recorded as a member — the assignment is the half that makes the
/// class mean anything. Both land in the sibling `.kicad_pro`: KiCad has kept
/// net classes in the project's `net_settings` since v7, and the board file
/// has no container for them at all — the pre-P.6.2 handlers inserted
/// `(netclass …)` straight into the board, a node pcbnew's parser rejects.
#[tokio::test]
async fn a_netclass_takes_its_widths_and_its_members() {
    let h = Harness::new();
    let board_path = h.fixture("test.kicad_pcb");
    let board = harness::as_str(&board_path).to_string();
    // create_netclass refuses without a sibling project — a class written
    // anywhere else is never read by KiCad.
    h.write(
        "test.kicad_pro",
        "{\n  \"meta\": { \"filename\": \"test.kicad_pro\", \"version\": 3 }\n}\n",
    );

    // add_net legitimately writes the board — it is the netclass tools that
    // must not — so the byte-identical check below is taken after it, not
    // before.
    h.json("add_net", json!({ "board": board, "net_name": "VBUS" }))
        .await;
    let board_before = std::fs::read_to_string(&board_path).expect("the board is readable");

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
        "assign_net_to_class",
        json!({ "board": board, "net_name": "VBUS", "netclass": "Power" }),
    )
    .await;

    // The board is untouched — every byte of it, not just "still parses".
    assert_eq!(
        std::fs::read_to_string(&board_path).expect("the board is readable"),
        board_before,
        "create_netclass/assign_net_to_class must never write the board file"
    );

    let project: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(board_path.with_extension("kicad_pro"))
            .expect("the project file is readable"),
    )
    .expect("the project file is valid JSON");
    let classes = project["net_settings"]["classes"]
        .as_array()
        .expect("net_settings.classes is an array");
    let power = classes
        .iter()
        .find(|c| c["name"] == "Power")
        .expect("the Power class is in net_settings.classes");
    assert_eq!(
        power["track_width"],
        json!(0.5),
        "the netclass track width was not written: {power}"
    );

    let patterns = project["net_settings"]["netclass_patterns"]
        .as_array()
        .expect("net_settings.netclass_patterns is an array");
    assert!(
        patterns
            .iter()
            .any(|p| p["pattern"] == "VBUS" && p["netclass"] == "Power"),
        "VBUS is not assigned to Power via netclass_patterns: {patterns:?}"
    );
}
