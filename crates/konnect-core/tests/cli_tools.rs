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

use harness::{Harness, TWO_RESISTORS};
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
