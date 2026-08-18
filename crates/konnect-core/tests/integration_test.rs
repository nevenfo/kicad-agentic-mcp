//! Integration tests that run against real .kicad_sch and .kicad_pcb fixture files.
//!
//! These tests do NOT require kicad-cli or a running KiCAD instance.
//! They test S-expression parsing and file manipulation only.

use std::path::PathBuf;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

/// Copy a fixture to a temp file so we can mutate it without affecting other tests.
fn temp_copy(fixture_name: &str) -> tempfile::NamedTempFile {
    let src = fixtures_dir().join(fixture_name);
    let content = std::fs::read_to_string(&src)
        .unwrap_or_else(|e| panic!("Failed to read fixture {}: {}", fixture_name, e));
    let ext = if fixture_name.ends_with(".kicad_sch") {
        ".kicad_sch"
    } else {
        ".kicad_pcb"
    };
    let tmp = tempfile::Builder::new()
        .suffix(ext)
        .tempfile()
        .expect("create temp file");
    std::fs::write(tmp.path(), &content).expect("write temp file");
    tmp
}

// ─── konnect-sexp parsing ──────────────────────────────────────────────────────

#[test]
fn parse_schematic_finds_symbols() {
    let path = fixtures_dir().join("test.kicad_sch");
    let content = std::fs::read_to_string(&path).unwrap();
    let tree = konnect_sexp::parser::parse_sexp(&content).unwrap();

    // Find top-level symbol instances (have Reference property)
    let symbols = tree.find_all("symbol");
    let instances: Vec<_> = symbols
        .iter()
        .filter(|s| {
            s.find_all("property")
                .iter()
                .any(|p| p.get(1).and_then(|n| n.as_str()) == Some("Reference"))
        })
        .collect();
    assert_eq!(instances.len(), 2, "Expected R1 and R2");
}

#[test]
fn parse_pcb_finds_footprints() {
    let path = fixtures_dir().join("test.kicad_pcb");
    let content = std::fs::read_to_string(&path).unwrap();
    let tree = konnect_sexp::parser::parse_sexp(&content).unwrap();

    let fps = tree.find_all("footprint");
    assert_eq!(fps.len(), 2);

    // Check R1 pads
    let r1_pads = fps[0].find_all("pad");
    assert_eq!(r1_pads.len(), 2, "R1 should have 2 pads");
}

#[test]
fn pcb_pad_board_position() {
    let path = fixtures_dir().join("test.kicad_pcb");
    let content = std::fs::read_to_string(&path).unwrap();
    let tree = konnect_sexp::parser::parse_sexp(&content).unwrap();

    let fps = tree.find_all("footprint");
    let r1 = &fps[0];

    let fp_at = r1.find("at").unwrap();
    let fp_x = fp_at.get_f64(1).unwrap();
    let fp_y = fp_at.get_f64(2).unwrap();

    let pads = r1.find_all("pad");
    let pad1_at = pads[0].find("at").unwrap();
    let local_x = pad1_at.get_f64(1).unwrap();
    let local_y = pad1_at.get_f64(2).unwrap();

    let board_x = fp_x + local_x;
    let board_y = fp_y + local_y;
    assert!(
        (board_x - 99.5).abs() < 0.01,
        "R1 pad 1 board X should be 99.5, got {}",
        board_x
    );
    // Both axes, or "board position" is half-checked: a transform that dropped
    // the footprint's own y would still pass on x alone.
    assert!(
        (board_y - 50.0).abs() < 0.01,
        "R1 pad 1 board Y should be 50.0, got {}",
        board_y
    );
}

#[test]
fn pcb_nets_parsed() {
    let path = fixtures_dir().join("test.kicad_pcb");
    let content = std::fs::read_to_string(&path).unwrap();

    // Count net declarations
    let net_count = content.matches("\n  (net ").count();
    assert!(net_count >= 3, "Should have at least 3 net declarations");
    assert!(content.contains("\"VCC\""));
    assert!(content.contains("\"GND\""));
}

// ─── S-expression writer ─────────────────────────────────────────────────────

#[test]
fn atomic_write_roundtrip() {
    let tmp = temp_copy("test.kicad_sch");
    let original = std::fs::read_to_string(tmp.path()).unwrap();

    let modified = original.replace("10k", "22k");
    konnect_sexp::writer::write_atomic(tmp.path(), &modified).unwrap();

    let readback = std::fs::read_to_string(tmp.path()).unwrap();
    assert!(readback.contains("22k"));
    assert!(!readback.contains("10k"));
}

#[test]
fn insert_sexp_before_close() {
    let tmp = temp_copy("test.kicad_sch");
    let content = std::fs::read_to_string(tmp.path()).unwrap();

    let close = content.rfind(')').unwrap();
    let wire_sexp = "\n  (wire (pts (xy 0 0) (xy 10 0)) (stroke (width 0) (type default)) (uuid \"test-wire\"))";
    let edits = vec![konnect_sexp::writer::SexpEdit::insert(
        close,
        wire_sexp.to_string(),
    )];
    let new_content = konnect_sexp::writer::apply_edits(content, edits);
    konnect_sexp::writer::write_atomic(tmp.path(), &new_content).unwrap();

    let readback = std::fs::read_to_string(tmp.path()).unwrap();
    assert!(readback.contains("test-wire"));
}

#[test]
fn delete_sexp_block() {
    let tmp = temp_copy("test.kicad_sch");
    let content = std::fs::read_to_string(tmp.path()).unwrap();

    // Delete the first wire
    let wire_start = content.find("(wire ").unwrap();
    let (block_start, block_end) =
        konnect_sexp::writer::find_block_with_leading_whitespace(&content, wire_start).unwrap();

    let edits = vec![konnect_sexp::writer::SexpEdit::delete(
        block_start,
        block_end,
    )];
    let new_content = konnect_sexp::writer::apply_edits(content, edits);
    konnect_sexp::writer::write_atomic(tmp.path(), &new_content).unwrap();

    let readback = std::fs::read_to_string(tmp.path()).unwrap();
    assert!(!readback.contains("wire-uuid-1111"));
}

// ─── Router ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn router_loads_all_toolsets() {
    let router = konnect_core::router::ToolRouter::new();
    assert!(router.load("project").await.is_some());
    assert!(router.load("sch_wiring").await.is_some());
    assert!(router.load("pcb_board").await.is_some());
    assert!(router.load("integration").await.is_some());
    assert!(router.load("verification").await.is_some());
    assert!(router.load("no_such_toolset").await.is_none());
}

// ─── Structured error taxonomy ──────────────────────────────────────────────

#[test]
fn structured_error_round_trips_through_extract() {
    use konnect_core::mcp::error::{extract_error_kind, ToolErrorKind};
    use konnect_core::mcp::protocol::{CallToolResult, ToolContent};

    let result = CallToolResult::error_kind(
        ToolErrorKind::ToolsetNotLoaded {
            toolset: "pcb_components".into(),
            tool: "place_component".into(),
        },
        "toolset 'pcb_components' not loaded",
    );
    assert!(result.is_error);
    assert_eq!(
        extract_error_kind(&result).as_deref(),
        Some("toolset_not_loaded")
    );

    // Client parses the body and branches on kind.
    let body = match &result.content[0] {
        ToolContent::Text { text } => text.clone(),
        _ => panic!(),
    };
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(parsed["error"]["kind"], "toolset_not_loaded");
    assert_eq!(parsed["error"]["toolset"], "pcb_components");
    assert_eq!(parsed["error"]["tool"], "place_component");
    assert!(parsed["message"].as_str().unwrap().contains("not loaded"));
}

// ─── Observability meta-tools ────────────────────────────────────────────────
//
// End-to-end test for the observer wiring: construct a ToolContext with a
// shared CallObserver, simulate a handful of tool calls via direct record,
// then confirm that the `get_recent_calls` and `server_stats` meta-tools
// surface the data.

#[tokio::test]
async fn observability_meta_tools_surface_recorded_calls() {
    use konnect_core::observability::{new_call_id, unix_ms, CallObserver, CallRecord, CallStatus};
    use konnect_core::router::{meta_tools, ToolRouter};
    use konnect_core::tools::{ServerConfig, ToolContext};
    use std::sync::Arc;

    let router = Arc::new(ToolRouter::new());
    router.load_starter_kit().await;
    let observer = CallObserver::new(None);
    let ctx = Arc::new(ToolContext::new_with_observer(
        ServerConfig {
            kicad_cli: String::new(),
            kicad_binary: String::new(),
            ipc_address: String::new(),
            project_dir: None,
            jlcpcb_db_path: None,
            auto_load_toolsets: false,
            mode: kam_state::OperatingMode::Write,
        },
        router,
        observer.clone(),
    ));

    // Simulate four calls.
    for (tool, status) in [
        ("add_wire", CallStatus::Ok),
        ("add_wire", CallStatus::Ok),
        ("add_wire", CallStatus::Error),
        ("route_trace", CallStatus::NotFound),
    ] {
        observer
            .record(CallRecord {
                call_id: new_call_id(),
                ts: unix_ms(),
                tool: tool.to_string(),
                toolset: Some("sch_wiring".to_string()),
                dur_ms: 5,
                status,
                error_kind: if matches!(status, CallStatus::Ok) {
                    None
                } else {
                    Some("x".to_string())
                },
                args_bytes: 1,
                result_bytes: 1,
            })
            .await;
    }

    // get_recent_calls with default limit
    let recent = meta_tools::handle_meta_tool("get_recent_calls", &serde_json::json!({}), &ctx)
        .await
        .expect("meta-tool should exist");
    assert!(!recent.is_error);
    let text = match &recent.content[0] {
        konnect_core::mcp::protocol::ToolContent::Text { text } => text.clone(),
        _ => panic!("expected text"),
    };
    let body: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(body["count"], 4);
    assert_eq!(body["calls"][0]["tool"], "route_trace"); // newest first
    assert_eq!(body["calls"][0]["status"], "not_found");

    // server_stats
    let stats = meta_tools::handle_meta_tool("server_stats", &serde_json::json!({}), &ctx)
        .await
        .expect("meta-tool should exist");
    let stats_text = match &stats.content[0] {
        konnect_core::mcp::protocol::ToolContent::Text { text } => text.clone(),
        _ => panic!("expected text"),
    };
    let stats_body: serde_json::Value = serde_json::from_str(&stats_text).unwrap();
    assert_eq!(stats_body["total_calls"], 4);
    assert_eq!(stats_body["error_calls"], 1); // only true `Error` counts
    let add_wire_stats = &stats_body["per_tool"]["add_wire"];
    assert_eq!(add_wire_stats["total"], 3);
    assert_eq!(add_wire_stats["errors"], 1);
}
