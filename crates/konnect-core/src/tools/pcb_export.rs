//! `pcb_export` toolset — Gerber, PDF, SVG, 3D, BOM, netlist, position file, DRC,
//! zone refill, and DXF/GenCAD/IPC-2581/ODB++ interchange formats.
//!
//! All operations delegate to `kicad-cli` via the `cli` module, except `refill_zones`
//! which uses the KiCAD IPC API.

use crate::mcp::protocol::CallToolResult;
use crate::tool;
use crate::tools::ipc_boundary::guarded_ipc as ipc;
use crate::tools::{get_path, require_array, ToolContext, ToolDef};
use crate::try_arg;
use serde_json::json;

use super::cli;

// ─── Severity filter helpers ──────────────────────────────────────────────────

fn severity_rank(s: &str) -> u8 {
    match s {
        "error" => 2,
        "warning" => 1,
        _ => 0,
    }
}

// ─── Tool definitions ─────────────────────────────────────────────────────────

pub fn tools() -> Vec<ToolDef> {
    vec![
        tool!(
            "export_gerber",
            "Export Gerber production files for all copper and mask layers using kicad-cli.",
            json!({
                "type": "object",
                "properties": {
                    "board": { "type": "string", "description": "Path to .kicad_pcb file" },
                    "output_dir": { "type": "string", "description": "Directory to write Gerber files into" },
                    "layers": {
                        "type": "array",
                        "description": "Layer names to export (empty = all fabrication layers)",
                        "items": { "type": "string" }
                    },
                    "drill_file": { "type": "boolean", "description": "Also generate Excellon drill file", "default": true }
                },
                "required": ["board", "output_dir"]
            }),
            |args, ctx| async move { handle_export_gerber(args, ctx).await }
        ),
        tool!(
            "export_drill",
            "Export drill files on their own, with the fabricator's options: Excellon or Gerber, \
             units, drill origin, separate plated/non-plated files, and a drill map. \
             `export_gerber` and `export_manufacturing_package` emit drills with KiCAD's \
             defaults; use this when the fab house asks for anything else. \
             Writes into a directory — KiCAD names the files after the board.",
            json!({
                "type": "object",
                "properties": {
                    "board": { "type": "string", "description": "Path to .kicad_pcb file" },
                    "output_dir": { "type": "string", "description": "Directory to write drill files into" },
                    "format": { "type": "string", "description": "'excellon' (default) or 'gerber'", "default": "excellon" },
                    "units": { "type": "string", "description": "Excellon coordinate units: 'mm' (default) or 'in'", "default": "mm" },
                    "drill_origin": { "type": "string", "description": "'absolute' (default) or 'plot'", "default": "absolute" },
                    "separate_plated": { "type": "boolean", "description": "Write plated (PTH) and non-plated (NPTH) holes to separate files", "default": false },
                    "generate_map": { "type": "boolean", "description": "Also write a drill map", "default": false },
                    "map_format": { "type": "string", "description": "Map format when generate_map is set: 'pdf' (default), 'gerberx2', 'ps', 'dxf', or 'svg'", "default": "pdf" }
                },
                "required": ["board", "output_dir"]
            }),
            |args, ctx| async move { handle_export_drill(args, ctx).await }
        ),
        tool!(
            "export_pdf",
            "Export the PCB layout to a PDF file using kicad-cli.",
            json!({
                "type": "object",
                "properties": {
                    "board": { "type": "string", "description": "Path to .kicad_pcb file" },
                    "output": { "type": "string", "description": "Output PDF file path" },
                    "layers": {
                        "type": "array",
                        "description": "Layer names to include (empty = all visible layers)",
                        "items": { "type": "string" }
                    },
                    "black_and_white": { "type": "boolean", "description": "Render in black and white", "default": false }
                },
                "required": ["board", "output"]
            }),
            |args, ctx| async move { handle_export_pdf(args, ctx).await }
        ),
        tool!(
            "export_svg",
            "Export the PCB layout to an SVG file using kicad-cli.",
            json!({
                "type": "object",
                "properties": {
                    "board": { "type": "string", "description": "Path to .kicad_pcb file" },
                    "output": { "type": "string", "description": "Output SVG file path" },
                    "layers": {
                        "type": "array",
                        "description": "Layer names to include (empty = all visible layers)",
                        "items": { "type": "string" }
                    },
                    "black_and_white": { "type": "boolean", "description": "Render in black and white", "default": false }
                },
                "required": ["board", "output"]
            }),
            |args, ctx| async move { handle_export_svg(args, ctx).await }
        ),
        tool!(
            "export_3d",
            "Export the PCB as a 3D model (STEP or VRML) using kicad-cli.",
            json!({
                "type": "object",
                "properties": {
                    "board": { "type": "string", "description": "Path to .kicad_pcb file" },
                    "output": { "type": "string", "description": "Output file path (.step or .wrl)" },
                    "format": {
                        "type": "string",
                        "description": "Export format: 'step' (default) or 'vrml'",
                        "default": "step"
                    },
                    "include_unspecified": {
                        "type": "boolean",
                        "description": "Include footprints with unspecified 3D models",
                        "default": false
                    }
                },
                "required": ["board", "output"]
            }),
            |args, ctx| async move { handle_export_3d(args, ctx).await }
        ),
        tool!(
            "export_netlist",
            "Export the PCB netlist to a file in KiCAD or IPC-D-356 format.",
            json!({
                "type": "object",
                "properties": {
                    "board": { "type": "string", "description": "Path to .kicad_pcb file (or .kicad_sch for schematic netlist)" },
                    "output": { "type": "string", "description": "Output netlist file path" },
                    "format": {
                        "type": "string",
                        "description": "Netlist format: 'kicad' or 'ipc' (IPC-D-356)",
                        "default": "kicad"
                    }
                },
                "required": ["board", "output"]
            }),
            |args, ctx| async move { handle_export_netlist(args, ctx).await }
        ),
        tool!(
            "export_position_file",
            "Generate a component placement (pick-and-place) position file for SMT assembly.",
            json!({
                "type": "object",
                "properties": {
                    "board": { "type": "string", "description": "Path to .kicad_pcb file" },
                    "output": { "type": "string", "description": "Output position file path" },
                    "format": {
                        "type": "string",
                        "description": "File format: 'csv' (default) or 'gerber'",
                        "default": "csv"
                    },
                    "side": {
                        "type": "string",
                        "description": "Board side: 'front', 'back', or 'both'",
                        "default": "both"
                    },
                    "units": {
                        "type": "string",
                        "description": "Coordinate units: 'mm' (default) or 'in'",
                        "default": "mm"
                    }
                },
                "required": ["board", "output"]
            }),
            |args, ctx| async move { handle_export_position_file(args, ctx).await }
        ),
        tool!(
            "export_dxf",
            "Export the PCB to DXF using kicad-cli, one file per requested layer. \
             Useful for mechanical CAD interchange (enclosures, panelization, laser cutting).",
            json!({
                "type": "object",
                "properties": {
                    "board": { "type": "string", "description": "Path to .kicad_pcb file" },
                    "output_dir": { "type": "string", "description": "Directory to write DXF files into (one per layer)" },
                    "layers": {
                        "type": "array",
                        "description": "Layer names to export, e.g. ['Edge.Cuts', 'F.Cu']",
                        "items": { "type": "string" }
                    }
                },
                "required": ["board", "output_dir", "layers"]
            }),
            |args, ctx| async move { handle_export_dxf(args, ctx).await }
        ),
        tool!(
            "export_gencad",
            "Export the PCB in GenCAD format using kicad-cli. GenCAD is accepted by some \
             CAM and test-fixture tooling as an alternative to a raw Gerber bundle.",
            json!({
                "type": "object",
                "properties": {
                    "board": { "type": "string", "description": "Path to .kicad_pcb file" },
                    "output": { "type": "string", "description": "Output .cad file path" }
                },
                "required": ["board", "output"]
            }),
            |args, ctx| async move { handle_export_gencad(args, ctx).await }
        ),
        tool!(
            "export_ipc2581",
            "Export the PCB in IPC-2581 format using kicad-cli. IPC-2581 is a unified \
             fabrication/assembly/test data format accepted by many contract manufacturers \
             as an alternative to a Gerber + drill + BOM + pick-and-place bundle.",
            json!({
                "type": "object",
                "properties": {
                    "board": { "type": "string", "description": "Path to .kicad_pcb file" },
                    "output": { "type": "string", "description": "Output file path (.xml)" },
                    "units": { "type": "string", "description": "Output units: 'mm' (default) or 'in'", "default": "mm" },
                    "compress": { "type": "boolean", "description": "Compress the output into a zip archive", "default": false }
                },
                "required": ["board", "output"]
            }),
            |args, ctx| async move { handle_export_ipc2581(args, ctx).await }
        ),
        tool!(
            "export_odb",
            "Export the PCB in ODB++ format using kicad-cli. ODB++ is a unified fabrication \
             data format accepted by many fab houses as an alternative to a Gerber + drill bundle.",
            json!({
                "type": "object",
                "properties": {
                    "board": { "type": "string", "description": "Path to .kicad_pcb file" },
                    "output": { "type": "string", "description": "Output file path" },
                    "units": { "type": "string", "description": "Output units: 'mm' (default) or 'in'", "default": "mm" },
                    "compression": { "type": "string", "description": "Compression mode: 'zip' (default), 'none', or 'tgz'", "default": "zip" }
                },
                "required": ["board", "output"]
            }),
            |args, ctx| async move { handle_export_odb(args, ctx).await }
        ),
        tool!(
            "refill_zones",
            "Refill all copper pour zones on the board. Requires a running KiCAD instance with IPC enabled; returns an error with instructions if KiCAD is not open.",
            json!({
                "type": "object",
                "properties": {
                    "board": { "type": "string", "description": "Path to .kicad_pcb file" },
                    "zones": {
                        "type": "array",
                        "description": "Net names of specific zones to refill (empty = all zones, currently not filtered)",
                        "items": { "type": "string" }
                    }
                },
                "required": ["board"]
            }),
            |args, ctx| async move { handle_refill_zones(args, ctx).await }
        ),
        tool!(
            "get_drc_violations",
            "Run the Design Rule Check (DRC) on the PCB and return a list of violations. \
             Provided in `pcb_export` because the output is handy to bundle alongside \
             Gerbers when preparing a build package. For interactive / iterative DRC \
             work, prefer `run_drc` (verification toolset) — same kicad-cli check, \
             cleaner summary with error/warning counts.",
            json!({
                "type": "object",
                "properties": {
                    "board": { "type": "string", "description": "Path to .kicad_pcb file" },
                    "output": { "type": "string", "description": "Optional path to write DRC report JSON" },
                    "severity": {
                        "type": "string",
                        "description": "Minimum severity to include: 'error', 'warning' (default), 'info'",
                        "default": "warning"
                    }
                },
                "required": ["board"]
            }),
            |args, ctx| async move { handle_get_drc_violations(args, ctx).await }
        ),
    ]
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

async fn handle_export_gerber(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let board = get_path(args, "board")?;
    let output_dir = get_path(args, "output_dir")?;
    let drill = args["drill_file"].as_bool().unwrap_or(true);

    // Ensure output dir exists
    tokio::fs::create_dir_all(&output_dir).await?;

    let cli = &ctx.config.kicad_cli;
    cli::export_gerber(cli, &board, &output_dir).await?;

    if drill {
        // kicad-cli also has a dedicated drill export, into the same directory
        // — its `--output` is a directory and it names the file after the
        // board. For anything beyond the defaults, call `export_drill`.
        let _ = cli::export_drill(cli, &board, &output_dir, &cli::DrillOptions::default()).await;
        // best-effort
    }

    // List produced files
    let mut files = Vec::new();
    if let Ok(mut rd) = tokio::fs::read_dir(&output_dir).await {
        while let Ok(Some(entry)) = rd.next_entry().await {
            files.push(entry.file_name().to_string_lossy().to_string());
        }
    }
    files.sort();

    Ok(CallToolResult::text(
        serde_json::to_string(&json!({
            "success": true,
            "output_dir": output_dir.to_str().unwrap_or(""),
            "files": files
        }))
        .unwrap(),
    ))
}

async fn handle_export_pdf(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let board = get_path(args, "board")?;
    let output = get_path(args, "output")?;

    // Collect optional layer list
    let layers: Vec<String> = args["layers"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let layer_refs: Vec<&str> = layers.iter().map(|s| s.as_str()).collect();

    let cli = &ctx.config.kicad_cli;
    cli::export_pdf(cli, &board, &output, &layer_refs).await?;

    Ok(CallToolResult::text(
        serde_json::to_string(&json!({
            "success": true,
            "output": output.to_str().unwrap_or("")
        }))
        .unwrap(),
    ))
}

async fn handle_export_svg(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let board = get_path(args, "board")?;
    let output = get_path(args, "output")?;

    let layers: Vec<String> = args["layers"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let layer_refs: Vec<&str> = layers.iter().map(|s| s.as_str()).collect();

    let cli = &ctx.config.kicad_cli;
    cli::export_svg_pcb(cli, &board, &output, &layer_refs).await?;

    Ok(CallToolResult::text(
        serde_json::to_string(&json!({
            "success": true,
            "output": output.to_str().unwrap_or("")
        }))
        .unwrap(),
    ))
}

async fn handle_export_3d(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let board = get_path(args, "board")?;
    let output = get_path(args, "output")?;
    let format = args["format"].as_str().unwrap_or("step");

    let cli = &ctx.config.kicad_cli;
    cli::export_3d(cli, &board, &output, format).await?;

    Ok(CallToolResult::text(
        serde_json::to_string(&json!({
            "success": true,
            "format": format,
            "output": output.to_str().unwrap_or("")
        }))
        .unwrap(),
    ))
}

/// Whether `format` names IPC-D-356, which is a different `kicad-cli` verb
/// rather than a value `sch export netlist --format` accepts.
fn is_ipc_d356(format: &str) -> bool {
    matches!(
        format.to_lowercase().replace(['-', '_', ' '], "").as_str(),
        "ipc" | "ipcd356"
    )
}

async fn handle_export_netlist(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let board = get_path(args, "board")?;
    let output = get_path(args, "output")?;
    let format = args["format"].as_str().unwrap_or("kicad");

    let cli = &ctx.config.kicad_cli;
    // `sch export netlist` works on both .kicad_sch and .kicad_pcb paths, but
    // its --format list has no IPC-D-356: that is `pcb export ipcd356`, a
    // separate verb. Asking for it here used to reach kicad-cli as an invalid
    // format, so the tool advertised a format it could not produce.
    if is_ipc_d356(format) {
        cli::export_ipcd356(cli, &board, &output).await?;
    } else {
        cli::export_netlist(cli, &board, &output, format).await?;
    }

    Ok(CallToolResult::text(
        serde_json::to_string(&json!({
            "success": true,
            "format": format,
            "output": output.to_str().unwrap_or("")
        }))
        .unwrap(),
    ))
}

async fn handle_export_drill(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let board = get_path(args, "board")?;
    let output_dir = get_path(args, "output_dir")?;

    let options = cli::DrillOptions {
        format: args["format"].as_str().unwrap_or("excellon"),
        units: args["units"].as_str().unwrap_or("mm"),
        origin: args["drill_origin"].as_str().unwrap_or("absolute"),
        separate_th: args["separate_plated"].as_bool().unwrap_or(false),
        generate_map: args["generate_map"].as_bool().unwrap_or(false),
        map_format: args["map_format"].as_str().unwrap_or("pdf"),
    };

    tokio::fs::create_dir_all(&output_dir).await?;
    let cli = &ctx.config.kicad_cli;
    cli::export_drill(cli, &board, &output_dir, &options).await?;

    // KiCAD names the files itself, so report what is actually there rather
    // than a path this tool guessed.
    let mut files = Vec::new();
    if let Ok(mut rd) = tokio::fs::read_dir(&output_dir).await {
        while let Ok(Some(entry)) = rd.next_entry().await {
            files.push(entry.file_name().to_string_lossy().to_string());
        }
    }
    files.sort();

    Ok(CallToolResult::text(
        serde_json::to_string(&json!({
            "success": true,
            "format": options.format,
            "output_dir": output_dir.to_str().unwrap_or(""),
            "files": files
        }))
        .unwrap(),
    ))
}

async fn handle_export_position_file(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let board = get_path(args, "board")?;
    let output = get_path(args, "output")?;
    let format = args["format"].as_str().unwrap_or("csv");
    let side = args["side"].as_str().unwrap_or("both");
    let units = args["units"].as_str().unwrap_or("mm");

    let cli = &ctx.config.kicad_cli;
    cli::export_position_file(cli, &board, &output, format).await?;

    Ok(CallToolResult::text(
        serde_json::to_string(&json!({
            "success": true,
            "format": format,
            "side": side,
            "units": units,
            "output": output.to_str().unwrap_or("")
        }))
        .unwrap(),
    ))
}

async fn handle_export_dxf(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let board = get_path(args, "board")?;
    let output_dir = get_path(args, "output_dir")?;
    // `layers` is `required` in the schema, and an empty list makes
    // `cli::export_dxf` omit `--layers` altogether, leaving the layer set to
    // kicad-cli's own default. Absence is refused; an explicit `[]` is not.
    let layers: Vec<String> = try_arg!(require_array(args, "layers"))
        .iter()
        .filter_map(|v| v.as_str().map(String::from))
        .collect();
    let layer_refs: Vec<&str> = layers.iter().map(|s| s.as_str()).collect();

    tokio::fs::create_dir_all(&output_dir).await?;

    let cli = &ctx.config.kicad_cli;
    cli::export_dxf(cli, &board, &output_dir, &layer_refs).await?;

    let mut files = Vec::new();
    if let Ok(mut rd) = tokio::fs::read_dir(&output_dir).await {
        while let Ok(Some(entry)) = rd.next_entry().await {
            files.push(entry.file_name().to_string_lossy().to_string());
        }
    }
    files.sort();

    Ok(CallToolResult::text(
        serde_json::to_string(&json!({
            "success": true,
            "output_dir": output_dir.to_str().unwrap_or(""),
            "files": files
        }))
        .unwrap(),
    ))
}

async fn handle_export_gencad(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let board = get_path(args, "board")?;
    let output = get_path(args, "output")?;

    let cli = &ctx.config.kicad_cli;
    cli::export_gencad(cli, &board, &output).await?;

    Ok(CallToolResult::text(
        serde_json::to_string(&json!({
            "success": true,
            "output": output.to_str().unwrap_or("")
        }))
        .unwrap(),
    ))
}

async fn handle_export_ipc2581(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let board = get_path(args, "board")?;
    let output = get_path(args, "output")?;
    let units = args["units"].as_str().unwrap_or("mm");
    let compress = args["compress"].as_bool().unwrap_or(false);

    let cli = &ctx.config.kicad_cli;
    cli::export_ipc2581(cli, &board, &output, units, compress).await?;

    Ok(CallToolResult::text(
        serde_json::to_string(&json!({
            "success": true,
            "units": units,
            "compressed": compress,
            "output": output.to_str().unwrap_or("")
        }))
        .unwrap(),
    ))
}

async fn handle_export_odb(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let board = get_path(args, "board")?;
    let output = get_path(args, "output")?;
    let units = args["units"].as_str().unwrap_or("mm");
    let compression = args["compression"].as_str().unwrap_or("zip");

    let cli = &ctx.config.kicad_cli;
    cli::export_odb(cli, &board, &output, units, compression).await?;

    Ok(CallToolResult::text(
        serde_json::to_string(&json!({
            "success": true,
            "units": units,
            "compression": compression,
            "output": output.to_str().unwrap_or("")
        }))
        .unwrap(),
    ))
}

async fn handle_refill_zones(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let board = get_path(args, "board")?;
    let _cli = &ctx.config.kicad_cli;

    // kicad-cli pcb export gerber triggers zone fills as a side-effect,
    // but the proper command is kicad-cli pcb --refill-zones (not in all versions).
    // Refilling zones has no file-level equivalent — the fill geometry is
    // computed by KiCAD — so this always goes over IPC, board-guarded (`ipc!`
    // = `ipc_boundary::guarded_ipc`) so it cannot silently refill whichever
    // board KiCAD happens to have open first: `KiCadIpcClient::refill_zones`
    // itself resolves the board via `get_board_document` (its first open
    // document), not the one this call named (P.6.9.23).
    ipc!(ctx, args, |client| client.refill_zones());

    Ok(CallToolResult::text(
        serde_json::to_string(&json!({
            "success": true,
            "method": "ipc",
            "board": board.to_str().unwrap_or("")
        }))
        .unwrap(),
    ))
}

async fn handle_get_drc_violations(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let board = get_path(args, "board")?;
    let severity_filter = args["severity"].as_str().unwrap_or("warning");
    let min_rank = severity_rank(severity_filter);

    let cli = &ctx.config.kicad_cli;
    let refill = args["refill_zones"].as_bool().unwrap_or(false);
    let report = cli::run_drc(cli, &board, refill).await?;
    let all = report.all();

    // Optionally write report
    if let Some(out_path) = args["output"].as_str() {
        let text = serde_json::to_string_pretty(&all)?;
        tokio::fs::write(out_path, text).await?;
    }

    let filtered: Vec<_> = all
        .iter()
        .filter(|v| severity_rank(&v.severity) >= min_rank)
        .collect();

    // A category the JSON did not include a key for is not "zero findings" —
    // it is a pass kicad-cli did not run; a fabrication package built on this
    // summary needs that distinction, not just a smaller total.
    let missing: Vec<&str> = report
        .missing_categories()
        .iter()
        .map(|c| c.json_key())
        .collect();

    let summary = json!({
        "total": all.len(),
        "filtered_count": filtered.len(),
        "severity_filter": severity_filter,
        "by_category": {
            "violations": report.violations.as_ref().map(Vec::len),
            "unconnected_items": report.unconnected_items.as_ref().map(Vec::len),
            "schematic_parity": report.schematic_parity.as_ref().map(Vec::len),
        },
        "missing_categories": missing,
        "violations": filtered.iter().map(|v| json!({
            "severity": v.severity,
            "description": v.description,
            "category": v.category,
            "pos": v.pos.as_ref().map(|p| json!({ "x": p.x, "y": p.y })),
            "items": v.items.iter().map(|item| json!({
                "description": item.description,
                "pos": item.pos.as_ref().map(|p| json!({ "x": p.x, "y": p.y })),
                "uuid": item.uuid,
            })).collect::<Vec<_>>()
        })).collect::<Vec<_>>()
    });

    Ok(CallToolResult::text(
        serde_json::to_string(&summary).unwrap(),
    ))
}

#[cfg(test)]
mod new_export_format_tests {
    //! Tests for `export_dxf`/`export_gencad`/`export_ipc2581`/`export_odb`.
    //!
    //! These handlers shell out to `kicad-cli`, which isn't available in CI
    //! (see ROADMAP.md's "mocked IPC endpoint" item — no kicad-cli mock exists
    //! yet either), so we can only test what's reachable without it:
    //! argument validation (missing required args fail before ever touching
    //! `kicad-cli`) and that a missing/unconfigured `kicad-cli` binary produces
    //! a clean error instead of a panic.

    use super::*;
    use crate::router::ToolRouter;
    use crate::tools::ServerConfig;
    use std::sync::Arc;

    fn test_ctx() -> ToolContext {
        ToolContext::new(
            ServerConfig {
                kicad_cli: String::new(),
                kicad_binary: String::new(),
                ipc_address: String::new(),
                project_dir: None,
                jlcpcb_db_path: None,
                auto_load_toolsets: false,
                mode: kam_state::OperatingMode::Write,
            },
            Arc::new(ToolRouter::new()),
        )
    }

    #[tokio::test]
    async fn export_dxf_missing_board_returns_error() {
        let ctx = test_ctx();
        let args = json!({ "output_dir": "out", "layers": ["Edge.Cuts"] });
        assert!(handle_export_dxf(&args, &ctx).await.is_err());
    }

    #[tokio::test]
    async fn export_dxf_fails_gracefully_without_kicad_cli() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = test_ctx();
        let args = json!({
            "board": dir.path().join("board.kicad_pcb").to_str().unwrap(),
            "output_dir": dir.path().join("out").to_str().unwrap(),
            "layers": ["Edge.Cuts", "F.Cu"]
        });
        // kicad_cli is "" in test_ctx, so spawning must fail — but as a
        // returned error, not a panic.
        assert!(handle_export_dxf(&args, &ctx).await.is_err());
    }

    /// An absent `layers` used to become an empty list, and an empty list
    /// makes `cli::export_dxf` omit `--layers` entirely — so kicad-cli picked
    /// its own layer set and the caller got files they never asked for.
    #[tokio::test]
    async fn export_dxf_without_layers_is_refused_not_left_to_kicad_cli() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = test_ctx();
        let args = json!({
            "board": dir.path().join("board.kicad_pcb").to_str().unwrap(),
            "output_dir": dir.path().join("out").to_str().unwrap()
        });
        let res = handle_export_dxf(&args, &ctx)
            .await
            .expect("the refusal is a result, not a transport error");
        assert!(res.is_error);
        assert_eq!(
            crate::mcp::error::extract_error_kind(&res).as_deref(),
            Some("invalid_argument")
        );
    }

    #[tokio::test]
    async fn export_gencad_missing_output_returns_error() {
        let ctx = test_ctx();
        let args = json!({ "board": "board.kicad_pcb" });
        assert!(handle_export_gencad(&args, &ctx).await.is_err());
    }

    #[tokio::test]
    async fn export_gencad_fails_gracefully_without_kicad_cli() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = test_ctx();
        let args = json!({
            "board": dir.path().join("board.kicad_pcb").to_str().unwrap(),
            "output": dir.path().join("board.cad").to_str().unwrap()
        });
        assert!(handle_export_gencad(&args, &ctx).await.is_err());
    }

    #[tokio::test]
    async fn export_ipc2581_missing_board_returns_error() {
        let ctx = test_ctx();
        let args = json!({ "output": "board.xml" });
        assert!(handle_export_ipc2581(&args, &ctx).await.is_err());
    }

    #[tokio::test]
    async fn export_ipc2581_fails_gracefully_without_kicad_cli() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = test_ctx();
        let args = json!({
            "board": dir.path().join("board.kicad_pcb").to_str().unwrap(),
            "output": dir.path().join("board.xml").to_str().unwrap(),
            "units": "mm",
            "compress": true
        });
        assert!(handle_export_ipc2581(&args, &ctx).await.is_err());
    }

    #[tokio::test]
    async fn export_odb_missing_output_returns_error() {
        let ctx = test_ctx();
        let args = json!({ "board": "board.kicad_pcb" });
        assert!(handle_export_odb(&args, &ctx).await.is_err());
    }

    #[tokio::test]
    async fn export_drill_missing_output_dir_returns_error() {
        let ctx = test_ctx();
        let args = json!({ "board": "board.kicad_pcb" });
        assert!(handle_export_drill(&args, &ctx).await.is_err());
    }

    /// An option KiCAD does not accept is rejected here, naming the valid
    /// values — rather than reaching `kicad-cli` and coming back as a non-zero
    /// exit. Checked through the handler so the wiring is covered too.
    #[tokio::test]
    async fn export_drill_rejects_an_option_kicad_does_not_accept() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = test_ctx();
        let args = json!({
            "board": dir.path().join("board.kicad_pcb").to_str().unwrap(),
            "output_dir": dir.path().join("drills").to_str().unwrap(),
            "format": "excellon",
            "units": "furlongs"
        });
        let error = handle_export_drill(&args, &ctx)
            .await
            .expect_err("an invalid unit is refused");
        let message = error.to_string();
        assert!(
            message.contains("furlongs") && message.contains("mm, in"),
            "the error should name the value and the valid ones: {message}"
        );
    }

    /// The valid options are passed through, and the failure is `kicad-cli`
    /// being absent rather than validation. Guards against the accepted set
    /// drifting from `kicad-cli pcb export drill --help`.
    #[tokio::test]
    async fn export_drill_accepts_every_option_kicad_documents() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = test_ctx();
        for (format, units, origin, map_format) in [
            ("excellon", "mm", "absolute", "pdf"),
            ("gerber", "in", "plot", "gerberx2"),
            ("EXCELLON", "MM", "Absolute", "svg"),
        ] {
            let args = json!({
                "board": dir.path().join("board.kicad_pcb").to_str().unwrap(),
                "output_dir": dir.path().join("drills").to_str().unwrap(),
                "format": format,
                "units": units,
                "drill_origin": origin,
                "separate_plated": true,
                "generate_map": true,
                "map_format": map_format
            });
            let message = handle_export_drill(&args, &ctx)
                .await
                .expect_err("kicad_cli is empty in test_ctx, so the spawn fails")
                .to_string();
            assert!(
                !message.contains("Valid options"),
                "'{format}/{units}/{origin}/{map_format}' was rejected as invalid: {message}"
            );
        }
    }

    /// IPC-D-356 is `pcb export ipcd356`, not a `sch export netlist --format`
    /// value. The tool advertises it, so the spelling it accepts is pinned.
    #[test]
    fn ipc_d356_is_recognised_by_every_spelling_the_tool_advertises() {
        for spelling in ["ipc", "IPC", "ipcd356", "ipc-d-356", "IPC_D_356"] {
            assert!(
                is_ipc_d356(spelling),
                "'{spelling}' should route to ipcd356"
            );
        }
        for other in ["kicad", "kicadxml", "spice", "orcadpcb2", "pads"] {
            assert!(!is_ipc_d356(other), "'{other}' is a sch netlist format");
        }
    }

    #[tokio::test]
    async fn export_odb_fails_gracefully_without_kicad_cli() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = test_ctx();
        let args = json!({
            "board": dir.path().join("board.kicad_pcb").to_str().unwrap(),
            "output": dir.path().join("board_odb.zip").to_str().unwrap(),
            "units": "mm",
            "compression": "zip"
        });
        assert!(handle_export_odb(&args, &ctx).await.is_err());
    }
}
