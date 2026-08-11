//! End-to-end test for plan postconditions (`validators`) against a real
//! KiCAD installation.
//!
//! `apply_plan` can be told a plan is only done if a named validator stays
//! clean. This drives that promise through real `kicad-cli sch erc`: a plan
//! that leaves floating pins and a duplicate reference must fail with
//! `postcondition_failed`, and a plan that connects every pin — the golden
//! suite's own divider — must not.
//!
//! Same gating as `e2e_kicad.rs`: requires kicad-cli and the standard symbol
//! libraries, so it is `#[ignore]` by default and run explicitly by the
//! e2e-kicad workflow (and locally):
//!
//!     cargo test -p konnect --test plan_postconditions_e2e -- --ignored --nocapture

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
/// Duplicated from `e2e_kicad.rs` rather than shared — these are two
/// independent, individually `#[ignore]`d integration tests, and the
/// duplication is a few lines against a new inter-test dependency.
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

    /// Call a tool and return its result, asserting it did not error. Use
    /// [`Mcp::request`] directly for calls expected to fail.
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

/// A plan that leaves two resistor pins floating. Under KiCad 10 that is an
/// **error**, not a warning — measured, and the reason this is the broken case
/// rather than the clean one:
///
/// ```text
/// {"errors":1,"violations":[{"description":"Pin not connected: Symbol R1 Pin 1 [Passive, Line]","severity":"error"}]}
/// ```
///
/// Naming both resistors `R1` adds a duplicate reference on top, so the plan
/// fails for a reason no severity setting can downgrade.
fn floating_pins_plan(sch: &str) -> Value {
    json!({
        "plan_id": "broken",
        "ops": [
            {"op": "place", "with": {"schematic": sch, "components": [
                {"lib_id": "Device:R", "reference": "R1", "value": "10k", "x": 100.33, "y": 80.01},
                {"lib_id": "Device:R", "reference": "R1", "value": "10k", "x": 100.33, "y": 95.25},
            ]}},
            {"op": "connect", "with": {"schematic": sch, "connections": [{"from": "R1.2", "to": "R1.1"}]}},
        ],
        "validators": ["erc_clean"]
    })
}

/// The resistive divider from `bench/tasks/01_sch_divider.yaml`, expressed as a
/// plan. Every pin ends up connected — the two `PWR_FLAG`s and the two power
/// symbols sit exactly on the outer resistor pins — which is what makes it the
/// clean case, and the golden suite asserts `erc_max_errors: 0` on the same
/// design on every run. Coordinates are exact multiples of the 1.27 mm grid.
fn divider_plan(sch: &str) -> Value {
    json!({
        "plan_id": "divider",
        "ops": [
            {"op": "place", "with": {"schematic": sch, "components": [
                {"lib_id": "Device:R", "reference": "R1", "value": "10k", "x": 100.33, "y": 80.01},
                {"lib_id": "Device:R", "reference": "R2", "value": "10k", "x": 100.33, "y": 95.25},
                {"lib_id": "power:PWR_FLAG", "reference": "#FLG01", "x": 100.33, "y": 76.2},
                {"lib_id": "power:PWR_FLAG", "reference": "#FLG02", "x": 100.33, "y": 99.06},
            ]}},
            {"op": "power", "with": {"schematic": sch, "symbols": [
                {"net": "+3V3", "x": 100.33, "y": 76.2},
                {"net": "GND", "x": 100.33, "y": 99.06},
            ]}},
            {"op": "connect", "with": {"schematic": sch, "connections": [{"from": "R1.2", "to": "R2.1"}]}},
            {"op": "label", "with": {"schematic": sch, "labels": [{"net": "VOUT", "x": 100.33, "y": 87.63}]}},
        ],
        "validators": ["erc_clean"]
    })
}

#[test]
#[ignore = "requires kicad-cli + symbol libraries; run via e2e workflow"]
fn erc_clean_fails_on_a_broken_schematic_and_passes_on_a_clean_one() {
    let Some(kicad_cli) = find_kicad_cli() else {
        panic!("kicad-cli not found — set KICAD_CLI or install KiCAD (this test is e2e-only)");
    };
    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path();

    // ── A plan that leaves a duplicate reference must fail ─────────────────
    let broken_proj = base.join("broken");
    let broken_sch = broken_proj.join("broken.kicad_sch");
    let mut p = Mcp::spawn(&kicad_cli);
    p.tool(
        "create_project",
        json!({"name": "broken", "path": broken_proj.to_string_lossy()}),
    );
    p.load("sch_components");
    p.load("sch_wiring");
    p.load("plan");

    let plan = floating_pins_plan(&broken_sch.to_string_lossy());
    let result = p.request(
        "tools/call",
        json!({"name": "apply_plan", "arguments": {"plan": plan}}),
    )["result"]
        .clone();
    assert_eq!(
        result["isError"],
        json!(true),
        "a duplicate-reference schematic must fail erc_clean: {result}"
    );
    let failure = body(&result);
    assert_eq!(failure["error"]["kind"], "postcondition_failed");
    assert_eq!(failure["error"]["check"], "erc_clean");
    assert!(failure["error"]["errors"].as_u64().unwrap_or(0) > 0);
    // The steps summary survives the failure: every step ran before the
    // postcondition was checked, and the reply still says so.
    assert_eq!(failure["ok"], failure["steps"]);
    assert!(failure["steps"].as_u64().unwrap_or(0) > 0);

    // ── A design where every pin is connected must pass ─────────────────────
    let clean_proj = base.join("clean");
    let clean_sch = clean_proj.join("clean.kicad_sch");
    p.tool(
        "create_project",
        json!({"name": "clean", "path": clean_proj.to_string_lossy()}),
    );

    let plan = divider_plan(&clean_sch.to_string_lossy());
    let result = p.request(
        "tools/call",
        json!({"name": "apply_plan", "arguments": {"plan": plan}}),
    )["result"]
        .clone();
    assert_ne!(
        result["isError"],
        json!(true),
        "a clean schematic must pass erc_clean: {}",
        result["content"][0]["text"].as_str().unwrap_or("?")
    );
    let ok = body(&result);
    assert_eq!(ok["ok"], ok["steps"]);
    assert!(ok["error"].is_null(), "clean run must carry no error: {ok}");

    eprintln!("E2E OK: erc_clean postcondition failed on the broken schematic and passed on the clean one");
}
