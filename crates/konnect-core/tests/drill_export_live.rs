//! Live probe for the drill and IPC-D-356 exports (J.2.2).
//!
//! The unit tests around these two prove argument handling without KiCAD; the
//! facts they rest on are facts about `kicad-cli`, and only KiCAD can confirm
//! them:
//!
//! * `pcb export drill --output` is a **directory**. The code used to pass
//!   `<dir>/drill.drl`, and KiCAD created a *directory* called `drill.drl` with
//!   the real file inside it — so `export_manufacturing_package` reported a
//!   file path that was not a file.
//! * IPC-D-356 is `pcb export ipcd356`. It is not a value `sch export netlist
//!   --format` accepts, which is what `export_netlist(format: "ipc")` used to
//!   send.
//!
//! `#[ignore]`d because it needs a real `kicad-cli`, and reported as `gated` in
//! the capability matrix for the same reason. Run it with:
//!
//!     KICAD_CLI=<path> cargo test -p konnect-core --test drill_export_live -- --ignored --nocapture

use std::path::{Path, PathBuf};

use konnect_core::tools::cli;

/// A board KiCAD 10 loads: the same skeleton `create_project` writes.
const BLANK_BOARD: &str = "(kicad_pcb\n\t(version 20250610)\n\t(generator \"konnect\")\n\t(generator_version \"10.0\")\n\t(general\n\t\t(thickness 1.6)\n\t)\n\t(paper \"A4\")\n\t(layers\n\t\t(0 \"F.Cu\" signal)\n\t\t(31 \"B.Cu\" signal)\n\t\t(44 \"Edge.Cuts\" user)\n\t)\n\t(setup\n\t\t(pad_to_mask_clearance 0.05)\n\t)\n\t(net 0 \"\")\n)\n";

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

fn board_in(dir: &Path) -> PathBuf {
    let board = dir.join("probe.kicad_pcb");
    std::fs::write(&board, BLANK_BOARD).expect("the board is writable");
    board
}

fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("the output directory exists")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// The defaults write one Excellon file, named after the board, directly into
/// the directory given — nothing nested.
#[tokio::test]
#[ignore = "requires kicad-cli; run with --ignored"]
async fn drill_export_fills_the_directory_it_is_given() {
    let cli_path = kicad_cli();
    let dir = tempfile::tempdir().expect("tempdir");
    let board = board_in(dir.path());
    let out = dir.path().join("drills");
    std::fs::create_dir_all(&out).expect("the output directory is creatable");

    cli::export_drill(&cli_path, &board, &out, &cli::DrillOptions::default())
        .await
        .expect("the default drill export succeeds");

    assert_eq!(
        names_in(&out),
        vec!["probe.drl".to_string()],
        "KiCAD names the drill file after the board and writes it into --output"
    );
}

/// The regression this fixes: a file path as `--output` becomes a *directory*
/// of that name. Asserted against KiCAD itself, because it is the reason
/// `export_drill` takes `output_dir`.
#[tokio::test]
#[ignore = "requires kicad-cli; run with --ignored"]
async fn a_file_path_as_output_would_have_become_a_directory() {
    let cli_path = kicad_cli();
    let dir = tempfile::tempdir().expect("tempdir");
    let board = board_in(dir.path());
    let mistaken = dir.path().join("drill.drl");

    cli::export_drill(&cli_path, &board, &mistaken, &cli::DrillOptions::default())
        .await
        .expect("kicad-cli accepts the path — that is the problem");

    assert!(
        mistaken.is_dir(),
        "if this is ever a file, KiCAD changed and export_drill can take a file path again"
    );
    assert_eq!(names_in(&mistaken), vec!["probe.drl".to_string()]);
}

/// The options the gap said were unreachable: separate plated/non-plated
/// files, a map, and inch units all reach KiCAD and change what is written.
#[tokio::test]
#[ignore = "requires kicad-cli; run with --ignored"]
async fn the_fabricator_options_reach_kicad() {
    let cli_path = kicad_cli();
    let dir = tempfile::tempdir().expect("tempdir");
    let board = board_in(dir.path());
    let out = dir.path().join("drills");
    std::fs::create_dir_all(&out).expect("the output directory is creatable");

    cli::export_drill(
        &cli_path,
        &board,
        &out,
        &cli::DrillOptions {
            units: "in",
            origin: "plot",
            separate_th: true,
            generate_map: true,
            map_format: "svg",
            ..Default::default()
        },
    )
    .await
    .expect("the fabricator options are accepted");

    assert_eq!(
        names_in(&out),
        vec![
            "probe-NPTH-drl_map.svg".to_string(),
            "probe-NPTH.drl".to_string(),
            "probe-PTH-drl_map.svg".to_string(),
            "probe-PTH.drl".to_string(),
        ],
        "separate_th splits the files and generate_map adds the maps"
    );
}

/// IPC-D-356 comes out of its own verb. `sch export netlist --format ipc` is
/// what the tool used to send, and KiCAD rejects it — asserted here so the
/// routing in `handle_export_netlist` is justified by KiCAD's behaviour and
/// not by reading its help text.
#[tokio::test]
#[ignore = "requires kicad-cli; run with --ignored"]
async fn ipc_d356_is_its_own_verb_and_the_netlist_format_is_not() {
    let cli_path = kicad_cli();
    let dir = tempfile::tempdir().expect("tempdir");
    let board = board_in(dir.path());

    let good = dir.path().join("board.ipc");
    cli::export_ipcd356(&cli_path, &board, &good)
        .await
        .expect("pcb export ipcd356 writes the netlist");
    assert!(good.is_file(), "the IPC-D-356 netlist is a file");

    let bad = dir.path().join("board.net");
    let error = cli::export_netlist(&cli_path, &board, &bad, "ipc")
        .await
        .expect_err("sch export netlist has no 'ipc' format");
    assert!(
        !bad.exists(),
        "the rejected call must not leave a netlist behind: {error}"
    );
}
