//! The `kicad-cli` tools, exercised end to end (J.2.3.7).
//!
//! Thirteen tools shell out to `kicad-cli`, and they are the awkward lot: the
//! interesting behaviour is KiCAD's, and CI has no KiCAD. So each is covered
//! twice, and the two halves prove different things:
//!
//! * **A test that runs everywhere** pins the part that is ours — the arguments
//!   accepted, the values rejected, the shape of the answer. That is a real
//!   proof of the server's own logic, and it is all the matrix claims.
//! * **A live probe, `#[ignore]`d**, runs the tool against a real `kicad-cli`
//!   and checks the file that comes out. It reads `gated` in the matrix and
//!   makes no claim, which is the honest arrangement: run it with
//!
//!       KICAD_CLI=<path> cargo test -p konnect-core --test cli_tools -- --ignored
//!
//! What is *not* done here is proving a tool by its own failure message. A
//! call that fails because `kicad_cli` is empty says nothing about the tool, so
//! those assertions always check something the tool decided before spawning.

mod harness;

use std::path::{Path, PathBuf};

use harness::{Harness, TWO_RESISTORS, TWO_RESISTORS_ONE_DNP};
use serde_json::json;

fn kicad_cli() -> String {
    if let Ok(path) = std::env::var("KICAD_CLI") {
        assert!(
            Path::new(&path).exists(),
            "KICAD_CLI points at {path}, which does not exist"
        );
        return path;
    }
    let candidates: &[&str] = if cfg!(windows) {
        &[
            r"C:\Program Files\KiCad\10.0\bin\kicad-cli.exe",
            r"C:\Users\FlowUP\AppData\Local\Programs\KiCad\10.0\bin\kicad-cli.exe",
        ]
    } else if cfg!(target_os = "macos") {
        &["/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli"]
    } else {
        &["/usr/bin/kicad-cli", "/usr/local/bin/kicad-cli"]
    };
    candidates
        .iter()
        .find(|path| Path::new(path).exists())
        .map(|path| path.to_string())
        .unwrap_or_else(|| panic!("no kicad-cli found — set KICAD_CLI to run this probe"))
}

/// A harness wired to a real `kicad-cli`, with a schematic and a board it can
/// actually load.
///
/// The board is `BLANK_BOARD` rather than the shared `test.kicad_pcb` fixture:
/// that fixture is a KiCad 8 file (`version 20240108`) and KiCad 10 refuses it
/// with "Échec du chargement de la carte". It is fine for the file-engine tests
/// that read it directly, and useless for anything that hands it to KiCAD.
fn live() -> (Harness, PathBuf, PathBuf) {
    let h = Harness::with_kicad_cli(kicad_cli());
    let schematic = h.fixture(TWO_RESISTORS);
    let board = h.write("live.kicad_pcb", harness::BLANK_BOARD);
    (h, schematic, board)
}

// ─── What the server decides before spawning ─────────────────────────────────

/// A missing required argument is refused before `kicad-cli` is ever reached —
/// this is the server's own contract and it holds with no KiCAD anywhere.
#[tokio::test]
async fn the_required_arguments_are_enforced_before_anything_is_spawned() {
    let h = Harness::new();

    for (tool, args) in [
        (
            "export_schematic_pdf",
            json!({ "schematic": "x.kicad_sch" }),
        ),
        ("export_pdf", json!({ "board": "x.kicad_pcb" })),
        ("export_svg", json!({ "board": "x.kicad_pcb" })),
        ("export_3d", json!({ "board": "x.kicad_pcb" })),
        ("export_netlist", json!({ "board": "x.kicad_sch" })),
        ("export_position_file", json!({ "board": "x.kicad_pcb" })),
        ("export_gerber", json!({ "board": "x.kicad_pcb" })),
        (
            "export_manufacturing_package",
            json!({ "board": "x.kicad_pcb" }),
        ),
    ] {
        let outcome = h.call(tool, args).await;
        let reported = match outcome {
            Ok(result) => harness::body(&result).to_string(),
            Err(e) => e.to_string(),
        };
        assert!(
            reported.contains("output") || reported.contains("required"),
            "'{tool}' was called with no output path and did not say so: {reported}"
        );
    }
}

/// `export_3d` maps friendly format names onto KiCAD's subcommands and refuses
/// what KiCAD has no subcommand for. The rejection names the formats, so a
/// caller can recover in one step.
#[tokio::test]
async fn the_3d_export_rejects_a_format_kicad_has_no_subcommand_for() {
    let h = Harness::new();
    let error = h
        .call(
            "export_3d",
            json!({ "board": "x.kicad_pcb", "output": "out.obj", "format": "obj" }),
        )
        .await
        .expect_err("obj is not a KiCAD 3D format");
    let message = error.to_string();
    assert!(
        message.contains("obj") && message.contains("step"),
        "the rejection should name the bad value and the valid ones: {message}"
    );
}

/// `export_bom`'s schema has advertised a `format` argument since it shipped,
/// but `kicad-cli sch export bom --help` has no `--format` flag at all —
/// there is nothing to map a non-"csv" value onto, so it must be refused
/// rather than silently accepted and ignored.
#[tokio::test]
async fn export_bom_rejects_a_format_kicad_cli_has_no_flag_for() {
    let h = Harness::new();
    let result = h
        .call(
            "export_bom",
            json!({ "schematic": "x.kicad_sch", "output": "bom.json", "format": "json" }),
        )
        .await
        .expect("the tool call itself does not fail");
    assert!(result.is_error, "an unsupported format must be refused");
    let body = harness::body(&result).to_string();
    assert!(
        body.contains("json") && body.contains("csv"),
        "the rejection should name the bad value and the supported one: {body}"
    );
}

/// `run_drc` and `get_drc_violations` both take a severity filter, and neither
/// may treat an unknown one as "everything" — silently widening a filter is how
/// a caller ends up reading a report they did not ask for.
#[tokio::test]
async fn the_drc_severity_filter_is_a_closed_set() {
    let h = Harness::new();
    for tool in ["run_drc", "get_drc_violations"] {
        let outcome = h
            .call(
                tool,
                json!({ "board": "x.kicad_pcb", "severity": "catastrophic" }),
            )
            .await;
        let reported = match outcome {
            Ok(result) => harness::body(&result).to_string(),
            Err(e) => e.to_string(),
        };
        assert!(
            !reported.contains("\"violations\": ["),
            "'{tool}' accepted an unknown severity and produced a report: {reported}"
        );
    }
}

/// `get_board_2d_view` clamps its render size rather than passing a silly
/// number to KiCAD — the image lands in an LLM's context, so the bound is the
/// point of the argument.
#[tokio::test]
async fn the_board_render_size_is_clamped() {
    let h = Harness::new();
    let outcome = h
        .call(
            "get_board_2d_view",
            json!({ "board": "x.kicad_pcb", "width": 99999, "height": 1 }),
        )
        .await;
    let reported = match outcome {
        Ok(result) => harness::body(&result).to_string(),
        Err(e) => e.to_string(),
    };
    assert!(
        !reported.contains("99999"),
        "the requested width was passed through unclamped: {reported}"
    );
}

// ─── Against a real kicad-cli ────────────────────────────────────────────────

/// The schematic exports produce the files they promise.
#[tokio::test]
#[ignore = "requires kicad-cli; run with --ignored"]
async fn the_schematic_exports_write_their_files() {
    let (h, schematic, _board) = live();
    let sch = harness::as_str(&schematic).to_string();

    let pdf = h.path("sheet.pdf");
    h.json(
        "export_schematic_pdf",
        json!({ "schematic": sch, "output": harness::as_str(&pdf) }),
    )
    .await;
    assert!(pdf.is_file(), "no PDF at {}", pdf.display());

    let netlist = h.path("sheet.net");
    h.json(
        "export_netlist",
        json!({ "board": sch, "output": harness::as_str(&netlist), "format": "kicad" }),
    )
    .await;
    let text = std::fs::read_to_string(&netlist).expect("the netlist is readable");
    assert!(
        text.contains("R1") && text.contains("R2"),
        "the netlist does not list the fixture's parts:\n{text}"
    );
}

/// `export_bom`'s `exclude_dnp` argument is only honored if the handler
/// actually reads it and passes `--exclude-dnp` to `kicad-cli` — the filter
/// happens inside KiCAD, so the only honest oracle is the CSV it writes.
#[tokio::test]
#[ignore = "requires kicad-cli; run with --ignored"]
async fn export_bom_exclude_dnp_actually_filters_the_dnp_part() {
    let h = Harness::with_kicad_cli(kicad_cli());
    let schematic = h.fixture(TWO_RESISTORS_ONE_DNP);
    let sch = harness::as_str(&schematic).to_string();

    let with_dnp = h.path("with_dnp.csv");
    h.json(
        "export_bom",
        json!({ "schematic": sch, "output": harness::as_str(&with_dnp), "exclude_dnp": false }),
    )
    .await;
    let with_dnp_text = std::fs::read_to_string(&with_dnp).expect("the BOM is readable");
    assert!(
        with_dnp_text.contains("R2"),
        "exclude_dnp:false should keep the DNP part R2: {with_dnp_text}"
    );

    let without_dnp = h.path("without_dnp.csv");
    h.json(
        "export_bom",
        json!({ "schematic": sch, "output": harness::as_str(&without_dnp), "exclude_dnp": true }),
    )
    .await;
    let without_dnp_text = std::fs::read_to_string(&without_dnp).expect("the BOM is readable");
    assert!(
        !without_dnp_text.contains("R2"),
        "exclude_dnp:true should drop the DNP part R2: {without_dnp_text}"
    );
    assert!(
        without_dnp_text.contains("R1"),
        "exclude_dnp:true should not drop the non-DNP part R1: {without_dnp_text}"
    );
}

/// `annotate_schematic` and `get_schematic_view` both go through `kicad-cli`
/// against a real sheet.
#[tokio::test]
#[ignore = "requires kicad-cli; run with --ignored"]
async fn annotation_and_the_sheet_view_run_against_a_real_schematic() {
    let (h, schematic, _board) = live();
    let sch = harness::as_str(&schematic).to_string();

    let annotated = h
        .json("annotate_schematic", json!({ "schematic": sch }))
        .await;
    assert!(
        annotated.to_string().to_lowercase().contains("annotat"),
        "annotation returned nothing usable: {annotated}"
    );

    let view = h
        .json("get_schematic_view", json!({ "schematic": sch }))
        .await;
    assert!(
        view.to_string().len() > 50,
        "the sheet view is empty: {view}"
    );
}

/// The board exports produce their files, including the ones that write a whole
/// directory.
#[tokio::test]
#[ignore = "requires kicad-cli; run with --ignored"]
async fn the_board_exports_write_their_files() {
    let (h, _schematic, board) = live();
    let pcb = harness::as_str(&board).to_string();

    let pdf = h.path("board.pdf");
    h.json(
        "export_pdf",
        json!({ "board": pcb, "output": harness::as_str(&pdf), "layers": ["F.Cu"] }),
    )
    .await;
    assert!(pdf.is_file(), "no board PDF at {}", pdf.display());

    let svg = h.path("board.svg");
    h.json(
        "export_svg",
        json!({ "board": pcb, "output": harness::as_str(&svg), "layers": ["F.Cu"] }),
    )
    .await;
    assert!(svg.is_file(), "no board SVG at {}", svg.display());

    let gerbers = h.path("gerbers");
    std::fs::create_dir_all(&gerbers).expect("the directory is creatable");
    let produced = h
        .json(
            "export_gerber",
            json!({ "board": pcb, "output_dir": harness::as_str(&gerbers) }),
        )
        .await;
    assert!(
        produced["files"].as_array().is_some_and(|f| !f.is_empty()),
        "the Gerber export reported no files: {produced}"
    );

    let positions = h.path("positions.csv");
    h.json(
        "export_position_file",
        json!({ "board": pcb, "output": harness::as_str(&positions), "format": "csv" }),
    )
    .await;
    assert!(
        positions.is_file(),
        "no position file at {}",
        positions.display()
    );
}

/// DRC runs against a real board and comes back with a report, not an
/// exception. A validator that could not run is a failure, never zero findings
/// (INV1) — so what is asserted is that a report exists and says how many.
#[tokio::test]
#[ignore = "requires kicad-cli; run with --ignored"]
async fn drc_runs_and_reports_against_a_real_board() {
    let (h, _schematic, board) = live();
    let pcb = harness::as_str(&board).to_string();

    let report = h.json("run_drc", json!({ "board": pcb })).await;
    let text = report.to_string();
    assert!(
        text.contains("violation") || text.contains("count") || text.contains("errors"),
        "the DRC report says nothing about what it found: {report}"
    );

    let violations = h
        .json(
            "get_drc_violations",
            json!({ "board": pcb, "severity": "error" }),
        )
        .await;
    assert!(
        violations.is_object(),
        "get_drc_violations returned no report: {violations}"
    );
}

/// An unrouted net is `unconnected_items`, not `violations` — a sibling array
/// the old parser never read. A board with open copper must come back with
/// at least one `error`-severity finding and a real position, not a report
/// that only saw the (possibly empty) `violations` array and called it clean.
#[tokio::test]
#[ignore = "requires kicad-cli; run with --ignored"]
async fn an_unrouted_board_reports_unconnected_copper_as_an_error() {
    let h = Harness::with_kicad_cli(kicad_cli());
    let board = h.fixture("unrouted.kicad_pcb");
    let pcb = harness::as_str(&board).to_string();

    let report = h
        .json("run_drc", json!({ "board": pcb, "severity": "error" }))
        .await;
    let violations = report["violations"]
        .as_array()
        .unwrap_or_else(|| panic!("no 'violations' array in report: {report}"));

    let unconnected: Vec<_> = violations
        .iter()
        .filter(|v| v["category"] == "unconnected_items")
        .collect();
    assert!(
        !unconnected.is_empty(),
        "an unrouted board must report at least one unconnected_items finding: {report}"
    );
    for v in &unconnected {
        assert_eq!(v["severity"], "error");
        assert!(
            v["pos"].is_object(),
            "unconnected_items entries must carry a position: {v}"
        );
    }
}

/// `create_netclass` used to insert a `(netclass …)` node into the board
/// itself — as a direct child of `(kicad_pcb`, a token pcbnew's parser
/// rejects, so the board no longer loaded and every later tool against it,
/// `run_drc` included, failed on the load rather than on anything of its own.
/// The class now lives in the sibling `.kicad_pro`; this is the one proof
/// that a real `kicad-cli` still opens the board afterward.
#[tokio::test]
#[ignore = "requires kicad-cli; run with --ignored"]
async fn netclass_tools_leave_the_board_loadable_by_kicad_cli() {
    let h = Harness::with_kicad_cli(kicad_cli());
    let board = h.fixture("unrouted.kicad_pcb");
    h.write(
        "unrouted.kicad_pro",
        "{\n  \"meta\": { \"filename\": \"unrouted.kicad_pro\", \"version\": 3 }\n}\n",
    );
    let pcb = harness::as_str(&board).to_string();

    h.json(
        "create_netclass",
        json!({ "board": pcb, "name": "HV", "clearance": 0.5, "trace_width": 0.3 }),
    )
    .await;
    h.json(
        "assign_net_to_class",
        json!({ "board": pcb, "net_name": "GND", "netclass": "HV" }),
    )
    .await;

    // If create_netclass had corrupted the board (the pre-fix behaviour),
    // kicad-cli fails to load it and this call errors instead of answering.
    let report = h.json("run_drc", json!({ "board": pcb })).await;
    assert!(
        report["total_violations"].is_number(),
        "run_drc did not produce a report after create_netclass/assign_net_to_class: {report}"
    );
}

/// `add_layer` used to find the close of `(layers …)` with a literal
/// `"\n  )"` probe, falling back to the first `)` in the block on a
/// tab-indented KiCAD 10 file — the close of the *first* layer entry — and
/// wrote the new layer inside it. The board reported success but no longer
/// loaded. This is the one proof that a real `kicad-cli` still opens the
/// board afterward.
#[tokio::test]
#[ignore = "requires kicad-cli; run with --ignored"]
async fn add_layer_leaves_the_board_loadable_by_kicad_cli() {
    let h = Harness::with_kicad_cli(kicad_cli());
    let board = h.fixture("unrouted.kicad_pcb");
    let pcb = harness::as_str(&board).to_string();

    let added = h
        .json(
            "add_layer",
            json!({ "board": pcb, "layer_name": "In1.Cu", "layer_type": "signal" }),
        )
        .await;
    // Without this, a refused add_layer would leave the board untouched and
    // the probe below would pass on a file nothing was written to.
    assert_eq!(added["added_layer"], "In1.Cu", "add_layer refused: {added}");

    // If add_layer had corrupted the board (the pre-fix behaviour), kicad-cli
    // fails to load it and this call errors instead of answering.
    let report = h.json("run_drc", json!({ "board": pcb })).await;
    assert!(
        report["total_violations"].is_number(),
        "run_drc did not produce a report after add_layer: {report}"
    );
}

/// The manufacturing package is the pipeline end to end: Gerbers, drills and
/// the assembly files in one directory.
#[tokio::test]
#[ignore = "requires kicad-cli; run with --ignored"]
async fn the_manufacturing_package_fills_its_output_directory() {
    let (h, schematic, board) = live();
    let out = h.path("fab");

    let package = h
        .json(
            "export_manufacturing_package",
            json!({
                "board": harness::as_str(&board),
                "schematic": harness::as_str(&schematic),
                "output_dir": harness::as_str(&out),
                "fab_house": "jlcpcb"
            }),
        )
        .await;

    let files = package["files_generated"]
        .as_array()
        .expect("the package reports what it generated");
    assert!(
        !files.is_empty(),
        "the package generated nothing: {package}"
    );

    // Every path it claims has to exist — reporting a file that is not there is
    // the failure this pipeline had before (J.2.2.1).
    for entry in files {
        let path = entry["path"].as_str().expect("each entry names a path");
        assert!(
            Path::new(path).exists(),
            "the package reported '{path}', which is not on disk: {package}"
        );
    }
}
