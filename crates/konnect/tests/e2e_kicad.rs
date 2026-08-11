//! End-to-end test against a real KiCAD installation (kicad-cli).
//!
//! Drives the shipped binary over stdio through a full design loop:
//! create project → place components → wire → ERC → export Gerbers → DRC.
//!
//! Requires kicad-cli and the standard symbol libraries, so it is `#[ignore]`
//! by default and run explicitly by the e2e-kicad workflow (and locally):
//!
//!     cargo test -p konnect --test e2e_kicad -- --ignored --nocapture

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

struct Mcp {
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
    next_id: i64,
}

/// Locate kicad-cli: KICAD_CLI env override, PATH, then platform defaults.
fn find_kicad_cli() -> Option<String> {
    if let Ok(p) = std::env::var("KICAD_CLI") {
        if std::path::Path::new(&p).exists() {
            return Some(p);
        }
    }
    let name = if cfg!(windows) {
        "kicad-cli.exe"
    } else {
        "kicad-cli"
    };
    if Command::new(name)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
    {
        return Some(name.to_string());
    }
    let candidates: &[&str] = if cfg!(windows) {
        &[
            r"C:\KiCad\10.0\bin\kicad-cli.exe",
            r"C:\Program Files\KiCad\10.0\bin\kicad-cli.exe",
        ]
    } else if cfg!(target_os = "macos") {
        &["/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli"]
    } else {
        &["/usr/bin/kicad-cli", "/usr/local/bin/kicad-cli"]
    };
    candidates
        .iter()
        .find(|c| std::path::Path::new(c).exists())
        .map(|c| c.to_string())
}

impl Mcp {
    fn spawn(kicad_cli: &str) -> Self {
        let mut config = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
        write!(config, "{}", json!({"kicad_cli": kicad_cli})).unwrap();
        config.flush().unwrap();
        let (_persist, config_path) = config.keep().unwrap();

        let mut child = Command::new(env!("CARGO_BIN_EXE_konnect"))
            .arg("--config")
            .arg(&config_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let reader = BufReader::new(child.stdout.take().unwrap());
        let mut p = Mcp {
            child,
            stdin,
            reader,
            next_id: 1,
        };
        p.request(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18", "capabilities": {},
                "clientInfo": {"name": "e2e", "version": "0"}
            }),
        );
        p
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        writeln!(
            self.stdin,
            "{}",
            json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
        )
        .unwrap();
        self.stdin.flush().unwrap();
        loop {
            let mut line = String::new();
            assert!(self.reader.read_line(&mut line).unwrap() > 0, "server died");
            let v: Value = serde_json::from_str(line.trim()).unwrap();
            if v.get("id").and_then(Value::as_i64) == Some(id) {
                return v;
            }
        }
    }

    fn tool(&mut self, name: &str, args: Value) -> Value {
        let r = self.request("tools/call", json!({"name": name, "arguments": args}));
        let result = r["result"].clone();
        assert_ne!(
            result["isError"],
            json!(true),
            "tool {name} failed: {}",
            result["content"][0]["text"].as_str().unwrap_or("?")
        );
        result
    }

    fn load(&mut self, toolset: &str) {
        self.tool("load_toolset", json!({"name": toolset}));
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn body(result: &Value) -> Value {
    serde_json::from_str(result["content"][0]["text"].as_str().unwrap_or("{}"))
        .unwrap_or(Value::Null)
}

#[test]
#[ignore = "requires kicad-cli + symbol libraries; run via e2e workflow"]
fn full_design_loop_with_real_kicad() {
    let Some(kicad_cli) = find_kicad_cli() else {
        panic!("kicad-cli not found — set KICAD_CLI or install KiCAD (this test is e2e-only)");
    };
    // KONNECT_E2E_KEEP_DIR: persist the generated project there (CI uploads
    // it as a failure artifact so file-format rejections can be diagnosed).
    let tmp = tempfile::tempdir().unwrap();
    let base: std::path::PathBuf = match std::env::var("KONNECT_E2E_KEEP_DIR") {
        Ok(d) => {
            std::fs::create_dir_all(&d).unwrap();
            d.into()
        }
        Err(_) => tmp.path().to_path_buf(),
    };
    let proj = base.join("e2e");
    let proj_s = proj.to_string_lossy().to_string();
    let sch = proj.join("e2e.kicad_sch");
    let pcb = proj.join("e2e.kicad_pcb");
    let mut p = Mcp::spawn(&kicad_cli);

    // ── Create ───────────────────────────────────────────────────────────
    p.tool("create_project", json!({"name": "e2e", "path": proj_s}));
    assert!(sch.exists() && pcb.exists());

    // ── Schematic: RC divider ────────────────────────────────────────────
    p.load("sch_components");
    p.load("sch_wiring");
    p.tool(
        "add_schematic_component",
        json!({
            "schematic": sch.to_string_lossy(), "lib_id": "Device:R",
            "reference": "R1", "value": "10k", "x": 100.0, "y": 100.0
        }),
    );
    p.tool(
        "add_schematic_component",
        json!({
            "schematic": sch.to_string_lossy(), "lib_id": "Device:C",
            "reference": "C1", "value": "100n", "x": 120.0, "y": 100.0
        }),
    );
    p.tool(
        "connect_pins",
        json!({
            "schematic": sch.to_string_lossy(),
            "ref1": "R1", "pin1": "2",
            "ref2": "C1", "pin2": "1"
        }),
    );

    // The written schematic must still parse and contain both parts.
    let content = std::fs::read_to_string(&sch).unwrap();
    let tree = konnect_sexp::parse_sexp(&content).expect("tool output must reparse");
    let refs: Vec<_> = konnect_sexp::schematic::extract_symbol_instances(&tree)
        .into_iter()
        .map(|s| s.reference)
        .collect();
    assert!(refs.contains(&"R1".to_string()) && refs.contains(&"C1".to_string()));

    // ── ERC through real eeschema ────────────────────────────────────────
    p.load("sch_export");
    p.load("verification");
    let erc = body(&p.tool("run_erc", json!({"schematic": sch.to_string_lossy()})));
    // This test asserts the shape of the report, not its verdict: what matters
    // here is that eeschema parsed OUR file and produced a structured report.
    // Do not read the loose assertion as "the design is fine" — measured on
    // KiCad 10, a floating passive pin is `severity: "error"`, not a warning
    // (`plan_postconditions_e2e.rs` depends on that being an error). The
    // verdict is checked there and by `kicad_invoke(verify: "auto")`.
    assert!(
        erc.get("errors").is_some()
            || erc.get("violations").is_some()
            || erc.get("summary").is_some(),
        "unexpected ERC shape: {erc}"
    );

    // ── PCB: export Gerbers + DRC through real kicad-cli ─────────────────
    p.load("pcb_export");
    let out_dir = proj.join("gerbers");
    p.tool(
        "export_gerber",
        json!({
            "board": pcb.to_string_lossy(),
            "output_dir": out_dir.to_string_lossy()
        }),
    );
    let produced = std::fs::read_dir(&out_dir).map(|d| d.count()).unwrap_or(0);
    assert!(
        produced > 0,
        "no gerber files produced in {}",
        out_dir.display()
    );

    let drc = body(&p.tool("run_drc", json!({"board": pcb.to_string_lossy()})));
    assert!(
        drc.get("errors").is_some()
            || drc.get("violations").is_some()
            || drc.get("summary").is_some(),
        "unexpected DRC shape: {drc}"
    );

    eprintln!("E2E OK: project created, wired, ERC'd, {produced} gerber files, DRC'd");
}

/// E6 regression: add_power_symbol must snap to the schematic grid like every
/// other placement tool (add_schematic_component, add_wire…), or a power
/// symbol placed at the same nominal coordinate as a resistor pin lands up to
/// 0.635mm off it and `kicad-cli sch erc` reports both as unconnected.
///
/// Reproduces the original failure: build a two-resistor probe divider, then
/// place VCC/GND power symbols using an intentionally off-grid nominal
/// coordinate near each free pin (as a caller unaware of grid-snapping
/// would compute it) — not the pin's already-snapped coordinate. Asserts
/// both that the tool reports the snap instead of hiding it, and that real
/// eeschema ERC reports zero errors.
#[test]
#[ignore = "requires kicad-cli + symbol libraries; run via e2e workflow"]
fn power_symbol_snaps_to_grid_like_components_e6() {
    let Some(kicad_cli) = find_kicad_cli() else {
        panic!("kicad-cli not found — set KICAD_CLI or install KiCAD (this test is e2e-only)");
    };
    let tmp = tempfile::tempdir().unwrap();
    let base: std::path::PathBuf = match std::env::var("KONNECT_E2E_KEEP_DIR") {
        Ok(d) => {
            std::fs::create_dir_all(&d).unwrap();
            d.into()
        }
        Err(_) => tmp.path().to_path_buf(),
    };
    let proj = base.join("e2e_e6");
    let proj_s = proj.to_string_lossy().to_string();
    let sch = proj.join("e2e_e6.kicad_sch");
    let mut p = Mcp::spawn(&kicad_cli);

    p.tool("create_project", json!({"name": "e2e_e6", "path": proj_s}));

    p.load("sch_components");
    p.load("sch_wiring");
    p.load("sch_analysis");
    p.load("sch_export");
    p.load("verification");

    // Probe divider: R1 top → VCC, R1.2–R2.1 in series, R2 bottom → GND.
    p.tool(
        "add_schematic_component",
        json!({
            "schematic": sch.to_string_lossy(), "lib_id": "Device:R",
            "reference": "R1", "value": "10k", "x": 100.0, "y": 80.0
        }),
    );
    p.tool(
        "add_schematic_component",
        json!({
            "schematic": sch.to_string_lossy(), "lib_id": "Device:R",
            "reference": "R2", "value": "10k", "x": 100.0, "y": 100.0
        }),
    );
    p.tool(
        "connect_pins",
        json!({
            "schematic": sch.to_string_lossy(),
            "ref1": "R1", "pin1": "2",
            "ref2": "R2", "pin2": "1"
        }),
    );

    // Real, already-snapped pin coordinates for the two free pins.
    let r1_pin1 = body(&p.tool(
        "get_pin_connections",
        json!({"schematic": sch.to_string_lossy(), "reference": "R1", "pin_number": "1"}),
    ));
    let r2_pin2 = body(&p.tool(
        "get_pin_connections",
        json!({"schematic": sch.to_string_lossy(), "reference": "R2", "pin_number": "2"}),
    ));
    let (r1x, r1y) = (
        r1_pin1["pin_x"].as_f64().unwrap(),
        r1_pin1["pin_y"].as_f64().unwrap(),
    );
    let (r2x, r2y) = (
        r2_pin2["pin_x"].as_f64().unwrap(),
        r2_pin2["pin_y"].as_f64().unwrap(),
    );

    // Off-grid nominal coordinates a caller unaware of grid-snapping would
    // compute — offsets stay well under the half-grid radius (0.635mm) so
    // they must snap back onto the exact pin position.
    let vcc = p.tool(
        "add_power_symbol",
        json!({
            "schematic": sch.to_string_lossy(), "power_net": "VCC",
            "x": r1x - 0.3, "y": r1y - 0.29
        }),
    );
    let gnd = p.tool(
        "add_power_symbol",
        json!({
            "schematic": sch.to_string_lossy(), "power_net": "GND",
            "x": r2x + 0.31, "y": r2y - 0.2
        }),
    );

    // A plain #PWR symbol is an ERC "power input" pin, not an output — KiCad
    // requires a PWR_FLAG (or an output-type power pin) on the net to accept
    // it as driven. Unrelated to the snap defect; without this every power
    // net here would ERC-fail regardless of the fix. Placed at the exact,
    // already-snapped pin coordinate (add_schematic_component's own grid
    // snap is not what's under test here).
    p.tool(
        "add_schematic_component",
        json!({
            "schematic": sch.to_string_lossy(), "lib_id": "power:PWR_FLAG",
            "reference": "#FLG01", "x": r1x, "y": r1y
        }),
    );
    p.tool(
        "add_schematic_component",
        json!({
            "schematic": sch.to_string_lossy(), "lib_id": "power:PWR_FLAG",
            "reference": "#FLG02", "x": r2x, "y": r2y
        }),
    );

    let vcc_body = body(&vcc);
    let gnd_body = body(&gnd);
    // The tool must not lie about what it wrote: an off-grid request is
    // reported, and the written x/y matches the pin exactly (not the
    // off-grid request).
    assert_eq!(vcc_body["snapped_to_grid"].as_bool(), Some(true));
    assert_eq!(vcc_body["x"].as_f64(), Some(r1x));
    assert_eq!(vcc_body["y"].as_f64(), Some(r1y));
    assert_eq!(gnd_body["snapped_to_grid"].as_bool(), Some(true));
    assert_eq!(gnd_body["x"].as_f64(), Some(r2x));
    assert_eq!(gnd_body["y"].as_f64(), Some(r2y));

    let erc = body(&p.tool("run_erc", json!({"schematic": sch.to_string_lossy()})));
    assert_eq!(
        erc["errors"].as_u64(),
        Some(0),
        "E6 regression: power symbols must land exactly on the resistor pins \
         they were nominally placed at — got: {erc}"
    );

    eprintln!("E2E OK (E6): probe divider + power symbols snapped, 0 ERC errors");
}
