//! `manufacturing` toolset — Design-to-fab pipeline: export packages, cost estimation, validation.
//!
//! Orchestrates gerber export, BOM generation, and pick-and-place file creation
//! into a single manufacturing-ready package for a specific fab house.

use crate::mcp::protocol::CallToolResult;
use crate::tool;
use crate::tools::{get_path, ToolContext, ToolDef};
use serde_json::json;
use std::path::PathBuf;
use tracing::{debug, error, info, warn};

use super::{cli, drc_gate};

// ─── Tool definitions ─────────────────────────────────────────────────────────

pub fn tools() -> Vec<ToolDef> {
    vec![
        tool!(
            "export_manufacturing_package",
            "Generate ALL files needed for PCB fabrication and assembly in one call: \
             Gerbers, drill files, BOM (fab-house format), and pick-and-place positions. \
             Targets a specific fab house (JLCPCB, PCBWay, etc.).",
            json!({
                "type": "object",
                "properties": {
                    "board": { "type": "string", "description": "Path to .kicad_pcb file" },
                    "schematic": { "type": "string", "description": "Path to .kicad_sch file (for BOM generation)" },
                    "output_dir": { "type": "string", "description": "Directory to write all output files" },
                    "fab_house": {
                        "type": "string",
                        "description": "Target manufacturer: 'jlcpcb' (default), 'pcbway', 'oshpark', 'generic'",
                        "default": "jlcpcb"
                    },
                    "include_assembly": {
                        "type": "boolean",
                        "description": "Include BOM + pick-and-place files for SMT assembly",
                        "default": true
                    },
                    "quantity": {
                        "type": "integer",
                        "description": "Production quantity (for BOM pricing context)",
                        "default": 5
                    }
                },
                "required": ["board", "output_dir"]
            }),
            |args, ctx| async move { handle_export_manufacturing_package(args, ctx).await }
        ),
        tool!(
            "validate_for_manufacturing",
            "Pre-flight check before ordering: verifies the design is ready for the target \
             fab house. Checks board outline, design rules, BOM completeness, and assembly constraints.",
            json!({
                "type": "object",
                "properties": {
                    "board": { "type": "string", "description": "Path to .kicad_pcb file" },
                    "schematic": { "type": "string", "description": "Path to .kicad_sch file (optional, for BOM checks)" },
                    "fab_house": {
                        "type": "string",
                        "description": "Target manufacturer: 'jlcpcb', 'pcbway', 'oshpark'",
                        "default": "jlcpcb"
                    }
                },
                "required": ["board"]
            }),
            |args, ctx| async move { handle_validate_for_manufacturing(args, ctx).await }
        ),
        tool!(
            "estimate_cost",
            "Estimate the total manufacturing cost for PCB fabrication and assembly at a given fab house. \
             Returns itemized breakdown: PCB, components, assembly, and total.",
            json!({
                "type": "object",
                "properties": {
                    "board": { "type": "string", "description": "Path to .kicad_pcb file" },
                    "schematic": { "type": "string", "description": "Path to .kicad_sch file (for component count)" },
                    "fab_house": {
                        "type": "string",
                        "description": "'jlcpcb' (default), 'pcbway'",
                        "default": "jlcpcb"
                    },
                    "quantity": {
                        "type": "integer",
                        "description": "Number of boards to manufacture",
                        "default": 5
                    },
                    "layers": {
                        "type": "integer",
                        "description": "Layer count (2, 4, 6). Auto-detected from board if omitted."
                    }
                },
                "required": ["board"]
            }),
            |args, ctx| async move { handle_estimate_cost(args, ctx).await }
        ),
    ]
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

async fn handle_export_manufacturing_package(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let board = get_path(args, "board")?;
    let output_dir = get_path(args, "output_dir")?;
    let fab_house = args["fab_house"].as_str().unwrap_or("jlcpcb");
    let include_assembly = args["include_assembly"].as_bool().unwrap_or(true);
    let schematic = args["schematic"].as_str().map(PathBuf::from);

    info!(
        board = %board.display(),
        output_dir = %output_dir.display(),
        fab_house = %fab_house,
        include_assembly = include_assembly,
        "[BETA] Generating manufacturing package"
    );

    tokio::fs::create_dir_all(&output_dir).await?;

    let cli_path = &ctx.config.kicad_cli;
    let mut files_generated = Vec::new();
    let mut warnings = Vec::new();

    // 1. Export Gerbers
    let gerber_dir = output_dir.join("gerbers");
    tokio::fs::create_dir_all(&gerber_dir).await?;
    match cli::export_gerber(cli_path, &board, &gerber_dir).await {
        Ok(()) => {
            info!("[BETA] Gerber export succeeded");
            files_generated.push(json!({
                "type": "gerber",
                "path": gerber_dir.to_str().unwrap_or("")
            }));
        }
        Err(e) => {
            error!(error = %e, "[BETA] Gerber export failed");
            warnings.push(format!("Gerber export failed: {}", e));
        }
    }

    // 2. Export drill files. `--output` is a directory and KiCAD names the
    //    files after the board, so this reports the directory it filled.
    let drill_dir = output_dir.join("drill");
    tokio::fs::create_dir_all(&drill_dir).await?;
    match cli::export_drill(cli_path, &board, &drill_dir, &cli::DrillOptions::default()).await {
        Ok(()) => {
            info!("[BETA] Drill export succeeded");
            files_generated.push(json!({
                "type": "drill",
                "path": drill_dir.to_str().unwrap_or("")
            }));
        }
        Err(e) => {
            warn!(error = %e, "[BETA] Drill export failed (may be included in gerbers)");
            // Not critical — some gerber exports include drill
        }
    }

    // 3. Assembly files (BOM + pick-and-place)
    if include_assembly {
        // Pick-and-place (position file)
        let pos_format = match fab_house {
            "jlcpcb" => "csv",
            _ => "csv",
        };
        let pos_path = output_dir.join(format!("positions.{}", pos_format));
        match cli::export_position_file(cli_path, &board, &pos_path, pos_format).await {
            Ok(()) => {
                info!("[BETA] Position file export succeeded");
                files_generated.push(json!({
                    "type": "pick_and_place",
                    "path": pos_path.to_str().unwrap_or(""),
                    "format": pos_format
                }));
            }
            Err(e) => {
                error!(error = %e, "[BETA] Position file export failed");
                warnings.push(format!("Position file export failed: {}", e));
            }
        }

        // BOM
        if let Some(ref sch) = schematic {
            let bom_path = output_dir.join("bom.csv");
            // No exclude_dnp knob is exposed on this tool's schema, so this
            // reproduces the manufacturing package's prior behaviour: every
            // symbol, DNP or not.
            match cli::export_bom(cli_path, sch, &bom_path, &cli::BomOptions::default()).await {
                Ok(()) => {
                    info!("[BETA] BOM export succeeded");
                    files_generated.push(json!({
                        "type": "bom",
                        "path": bom_path.to_str().unwrap_or(""),
                        "format": "csv"
                    }));
                }
                Err(e) => {
                    error!(error = %e, "[BETA] BOM export failed");
                    warnings.push(format!("BOM export failed: {}", e));
                }
            }
        } else {
            warnings.push("No schematic provided — BOM not generated. Pass 'schematic' for full assembly package.".to_string());
        }
    }

    // List all files in output dir
    let mut all_files = Vec::new();
    if let Ok(mut rd) = tokio::fs::read_dir(&output_dir).await {
        while let Ok(Some(entry)) = rd.next_entry().await {
            all_files.push(entry.file_name().to_string_lossy().to_string());
        }
    }
    // Also list gerber subdir
    if let Ok(mut rd) = tokio::fs::read_dir(&gerber_dir).await {
        while let Ok(Some(entry)) = rd.next_entry().await {
            all_files.push(format!("gerbers/{}", entry.file_name().to_string_lossy()));
        }
    }
    all_files.sort();

    let summary = format!(
        "Generated for {}. {} files total. {}",
        fab_house.to_uppercase(),
        all_files.len(),
        if warnings.is_empty() {
            "No warnings.".to_string()
        } else {
            format!("{} warnings.", warnings.len())
        }
    );

    info!(
        files = all_files.len(),
        warnings = warnings.len(),
        "[BETA] Manufacturing package complete"
    );

    Ok(CallToolResult::text(
        serde_json::to_string(&json!({
            "fab_house": fab_house,
            "output_dir": output_dir.to_str().unwrap_or(""),
            "files": all_files,
            "files_generated": files_generated,
            "warnings": warnings,
            "summary": summary,
            "next_steps": format!(
                "Upload the contents of {} to {}'s order page. Gerbers go in the PCB order, BOM + positions go in the assembly order.",
                output_dir.display(),
                fab_house.to_uppercase()
            )
        }))
        .unwrap(),
    ))
}

/// Distinct nets and routed items on the board, read from the parsed tree.
///
/// Both counts used to be substring probes — `"\n  (net "`, `"(segment "`,
/// `"(via "` — which depend on indentation KiCad controls, miss KiCad 10's
/// per-item net shape entirely, and never saw an `(arc …)` track at all. A
/// fully routed KiCad 10 board and one never routed both read zero, so the
/// `net_count > 3 && track_count == 0` guard could not fire: false success on
/// the last check before fabrication.
///
/// Nets go through [`konnect_sexp::net::count_distinct_nets`], which reads
/// both forms by shape and counts a KiCad 9 net once rather than through its
/// declaration *and* each reference.
///
/// Tracks are counted among the direct children of `(kicad_pcb …)`, where
/// routed copper always sits: `(arc …)` also appears inside a zone outline's
/// `(pts …)`, and that is a polygon corner, not copper.
fn count_nets_and_tracks(tree: &konnect_sexp::SexpNode) -> (usize, usize) {
    let track_count =
        tree.find_all("segment").len() + tree.find_all("via").len() + tree.find_all("arc").len();
    (konnect_sexp::net::count_distinct_nets(tree), track_count)
}

async fn handle_validate_for_manufacturing(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let board = get_path(args, "board")?;
    let evidence = drc_gate::gather(&ctx.config.kicad_cli, &board, false).await;
    validate_for_manufacturing_with(args, &evidence).await
}

/// The fab check itself, against a DRC report someone else gathered.
///
/// Split from the handler so the verdict can be exercised over a report
/// KiCAD never had to produce, and so a caller that already ran DRC does not
/// run it twice.
async fn validate_for_manufacturing_with(
    args: &serde_json::Value,
    evidence: &drc_gate::DrcEvidence,
) -> anyhow::Result<CallToolResult> {
    let board = get_path(args, "board")?;
    let fab_house = args["fab_house"].as_str().unwrap_or("jlcpcb");

    info!(
        board = %board.display(),
        fab_house = %fab_house,
        "[BETA] Running manufacturing validation"
    );

    let content = tokio::fs::read_to_string(&board).await?;
    let tree = konnect_sexp::parser::parse_sexp(&content)?;

    let mut issues = Vec::new();

    // Check board outline
    let has_outline = content.contains("Edge.Cuts");
    if !has_outline {
        issues.push(json!({
            "severity": "error",
            "issue": "No board outline found on Edge.Cuts layer",
            "fix": "Add a board outline using add_board_outline before ordering"
        }));
    }

    // Check that footprints exist
    let fp_count = tree.find_all("footprint").len();
    if fp_count == 0 {
        issues.push(json!({
            "severity": "error",
            "issue": "No footprints found on the board",
            "fix": "Open the PCB in KiCAD and run Tools > Update PCB from Schematic (kicad-cli 'pcb sync' was removed in v10)"
        }));
    }

    // Check layer count
    let _layers = tree
        .find("layers")
        .map(|l| l.find_all("*"))
        .unwrap_or_default();
    let copper_layers = content.matches("signal)").count() + content.matches("signal \"").count();
    debug!(
        copper_layers = copper_layers,
        "[BETA] Detected copper layers"
    );

    // Fab-specific checks
    let (min_trace, _min_drill, _max_layers) = match fab_house {
        "jlcpcb" => (0.127, 0.3, 32),
        "oshpark" => (0.152, 0.254, 4),
        "pcbway" => (0.1, 0.2, 32),
        _ => (0.15, 0.3, 32),
    };

    // Check design rules
    if let Some(min_tw) = find_setup_value(&content, "min_trace_width") {
        if min_tw < min_trace {
            issues.push(json!({
                "severity": "error",
                "issue": format!("Trace width {:.3}mm is below {}'s minimum ({:.3}mm)", min_tw, fab_house, min_trace),
                "fix": format!("Increase minimum trace width to {:.3}mm in design rules", min_trace)
            }));
        }
    }

    // DRC is what actually answers "is this board ready": its
    // `unconnected_items` pass names every net still on the ratsnest, one by
    // one, on a board that is otherwise fully routed.
    let gate = drc_gate::assess(evidence);
    for finding in &gate.findings {
        issues.push(json!({
            "severity": finding.severity,
            "issue": finding.issue,
            "fix": finding.fix,
        }));
    }

    // Check for unrouted nets (ratsnest).
    //
    // Kept only for the case where DRC could not measure connectivity: it is
    // the crudest possible proxy — nets exist, no copper anywhere — and once
    // `unconnected_items` is in hand it is strictly subsumed by it (a board
    // with nets and no tracks cannot have an empty unconnected list). Running
    // it anyway would let a heuristic contradict a measurement, which is the
    // opposite of what this item is for.
    let (net_count, track_count) = count_nets_and_tracks(&tree);
    if !gate.connectivity_measured && net_count > 3 && track_count == 0 {
        issues.push(json!({
            "severity": "error",
            "issue": format!("{} nets defined but no traces routed", net_count),
            "fix": "Route traces using route_trace or autoroute before manufacturing"
        }));
    }

    let verdict = if issues.iter().any(|i| i["severity"] == "error") {
        "NOT READY"
    } else if gate.incomplete {
        // No error found — but nobody looked with the full set of eyes, so
        // this is not a pass. Reporting READY here is the false clean the
        // whole check exists to prevent.
        "INCOMPLETE"
    } else if !issues.is_empty() {
        "NEEDS REVIEW"
    } else {
        "READY"
    };

    info!(
        verdict = verdict,
        issues = issues.len(),
        "[BETA] Manufacturing validation complete"
    );

    Ok(CallToolResult::text(
        serde_json::to_string(&json!({
            "verdict": verdict,
            "fab_house": fab_house,
            "board_info": {
                "footprint_count": fp_count,
                "copper_layers": copper_layers,
                "net_count": net_count,
                "track_count": track_count
            },
            // `null` when DRC did not run: a counters object reading zero
            // would be indistinguishable from a board that passed.
            "drc": gate.summary,
            "issues": issues,
            "summary": format!(
                "{}: {} issues found. {} footprints, {} copper layers.",
                verdict, issues.len(), fp_count, copper_layers
            )
        }))
        .unwrap(),
    ))
}

async fn handle_estimate_cost(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let board = get_path(args, "board")?;
    let fab_house = args["fab_house"].as_str().unwrap_or("jlcpcb");
    let quantity = args["quantity"].as_u64().unwrap_or(5) as usize;

    info!(
        board = %board.display(),
        fab_house = %fab_house,
        quantity = quantity,
        "[BETA] Estimating manufacturing cost"
    );

    let content = tokio::fs::read_to_string(&board).await?;
    let tree = konnect_sexp::parser::parse_sexp(&content)?;

    // Count components
    let fps = tree.find_all("footprint");
    let component_count = fps.len();

    // Detect layers
    let copper_layers = args["layers"].as_u64().unwrap_or_else(|| {
        let count = content.matches("signal)").count() + content.matches("signal \"").count();
        (count as u64).max(2)
    }) as usize;

    // Estimate board dimensions from Edge.Cuts
    let (width_mm, height_mm) = estimate_board_dimensions(&content);

    // Rough cost estimation based on fab house pricing models
    let (pcb_cost, assembly_cost, component_est) = match fab_house {
        "jlcpcb" => {
            let pcb = match copper_layers {
                2 => 2.0 + (quantity as f64 - 5.0).max(0.0) * 0.40,
                4 => 7.0 + (quantity as f64 - 5.0).max(0.0) * 1.40,
                6 => 15.0 + (quantity as f64 - 5.0).max(0.0) * 3.00,
                _ => 30.0 + (quantity as f64 - 5.0).max(0.0) * 5.00,
            };
            let smt_setup = if component_count > 0 { 8.0 } else { 0.0 };
            let smt_per_board = component_count as f64 * 0.003 * quantity as f64;
            let comp_est = component_count as f64 * 0.05; // rough avg per component
            (pcb, smt_setup + smt_per_board, comp_est * quantity as f64)
        }
        "pcbway" => {
            let pcb = match copper_layers {
                2 => 5.0 + (quantity as f64 - 5.0).max(0.0) * 0.50,
                4 => 12.0 + (quantity as f64 - 5.0).max(0.0) * 2.00,
                _ => 25.0 + (quantity as f64 - 5.0).max(0.0) * 4.00,
            };
            let smt = component_count as f64 * 0.005 * quantity as f64;
            let comp_est = component_count as f64 * 0.08 * quantity as f64;
            (pcb, smt, comp_est)
        }
        _ => {
            let pcb = 10.0 + quantity as f64 * 2.0;
            (pcb, 0.0, 0.0)
        }
    };

    let total = pcb_cost + assembly_cost + component_est;

    debug!(
        pcb_cost = pcb_cost,
        assembly_cost = assembly_cost,
        component_est = component_est,
        total = total,
        "[BETA] Cost estimate calculated"
    );

    Ok(CallToolResult::text(
        serde_json::to_string(&json!({
            "fab_house": fab_house,
            "quantity": quantity,
            "board": {
                "width_mm": width_mm,
                "height_mm": height_mm,
                "copper_layers": copper_layers,
                "component_count": component_count
            },
            "cost_estimate": {
                "pcb_fabrication": format!("${:.2}", pcb_cost),
                "smt_assembly": format!("${:.2}", assembly_cost),
                "components_estimate": format!("${:.2}", component_est),
                "total_estimate": format!("${:.2}", total),
                "per_board": format!("${:.2}", total / quantity as f64)
            },
            "notes": [
                "Estimates are approximate — actual cost depends on board size, finish, and specific components",
                "Component costs are rough averages — use generate_bom with supply chain data for accurate pricing",
                format!("Based on {} quantity from {}", quantity, fab_house.to_uppercase())
            ],
            "disclaimer": "BETA: Cost estimates are indicative only. Always confirm with the fab house's online quoting tool."
        }))
        .unwrap(),
    ))
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn find_setup_value(content: &str, key: &str) -> Option<f64> {
    let pat = format!("({} ", key);
    let pos = content.find(&pat)?;
    let after = &content[pos + pat.len()..];
    let end = after.find(')')?;
    after[..end].trim().parse().ok()
}

fn estimate_board_dimensions(content: &str) -> (f64, f64) {
    let mut min_x = f64::MAX;
    let mut max_x = f64::MIN;
    let mut min_y = f64::MAX;
    let mut max_y = f64::MIN;
    let mut found = false;

    // Scan gr_line on Edge.Cuts for board outline coordinates
    let mut pos = 0;
    while let Some(line_pos) = content[pos..].find("(gr_line") {
        let abs = pos + line_pos;
        let block_end = content[abs..].find(")\n").unwrap_or(300) + abs;
        let block = &content[abs..block_end.min(content.len())];

        if block.contains("Edge.Cuts") {
            // Extract start and end coordinates
            if let (Some(sx), Some(sy)) = (
                extract_coord(block, "start", 0),
                extract_coord(block, "start", 1),
            ) {
                if sx < min_x {
                    min_x = sx;
                }
                if sx > max_x {
                    max_x = sx;
                }
                if sy < min_y {
                    min_y = sy;
                }
                if sy > max_y {
                    max_y = sy;
                }
                found = true;
            }
            if let (Some(ex), Some(ey)) = (
                extract_coord(block, "end", 0),
                extract_coord(block, "end", 1),
            ) {
                if ex < min_x {
                    min_x = ex;
                }
                if ex > max_x {
                    max_x = ex;
                }
                if ey < min_y {
                    min_y = ey;
                }
                if ey > max_y {
                    max_y = ey;
                }
            }
        }
        pos = abs + 1;
    }

    if found {
        ((max_x - min_x).abs(), (max_y - min_y).abs())
    } else {
        (0.0, 0.0) // Unknown
    }
}

fn extract_coord(block: &str, keyword: &str, index: usize) -> Option<f64> {
    let pat = format!("({} ", keyword);
    let pos = block.find(&pat)? + pat.len();
    let rest = &block[pos..];
    let parts: Vec<&str> = rest.split([' ', ')']).collect();
    parts.get(index)?.parse().ok()
}

#[cfg(test)]
mod net_track_count_tests {
    use super::*;
    use crate::router::ToolRouter;
    use crate::tools::ServerConfig;
    use konnect_sexp::parser::parse_sexp;
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

    /// KiCad 10 (file format 20260206) indents with tabs, writes each track on
    /// its own multi-line form, and has no top-level net table — every item
    /// names its net instead. The old probes (`"\n  (net "`, `"(segment "`,
    /// `"(via "`) match none of that.
    const KICAD_10_BOARD: &str = "(kicad_pcb\n\t(version 20260206)\n\t(generator \"pcbnew\")\n\t(segment\n\t\t(start 110 110)\n\t\t(end 120 110)\n\t\t(width 0.2)\n\t\t(layer \"F.Cu\")\n\t\t(net \"GND\")\n\t)\n\t(segment\n\t\t(start 120 110)\n\t\t(end 130 110)\n\t\t(width 0.2)\n\t\t(layer \"F.Cu\")\n\t\t(net \"VCC\")\n\t)\n\t(arc\n\t\t(start 130 110)\n\t\t(mid 135 115)\n\t\t(end 130 120)\n\t\t(width 0.2)\n\t\t(layer \"F.Cu\")\n\t\t(net \"VCC\")\n\t)\n\t(via\n\t\t(at 130 120)\n\t\t(size 0.6)\n\t\t(drill 0.3)\n\t\t(layers \"F.Cu\" \"B.Cu\")\n\t\t(net \"VCC\")\n\t)\n)\n";

    /// KiCad 9 keeps the table and refers to it by number from each item, so
    /// counting `(net …)` nodes double-counts every net.
    const KICAD_9_BOARD: &str = "(kicad_pcb\n\t(version 20250114)\n\t(generator \"pcbnew\")\n\t(net 0 \"\")\n\t(net 1 \"GND\")\n\t(net 2 \"VCC\")\n\t(segment\n\t\t(start 110 110)\n\t\t(end 120 110)\n\t\t(width 0.2)\n\t\t(layer \"F.Cu\")\n\t\t(net 1)\n\t)\n\t(segment\n\t\t(start 120 110)\n\t\t(end 130 110)\n\t\t(width 0.2)\n\t\t(layer \"F.Cu\")\n\t\t(net 2)\n\t)\n\t(arc\n\t\t(start 130 110)\n\t\t(mid 135 115)\n\t\t(end 130 120)\n\t\t(width 0.2)\n\t\t(layer \"F.Cu\")\n\t\t(net 2)\n\t)\n\t(via\n\t\t(at 130 120)\n\t\t(size 0.6)\n\t\t(drill 0.3)\n\t\t(layers \"F.Cu\" \"B.Cu\")\n\t\t(net 2)\n\t)\n)\n";

    #[test]
    fn counts_a_kicad_10_board_with_no_top_level_net_table() {
        let tree = parse_sexp(KICAD_10_BOARD).unwrap();
        assert_eq!(count_nets_and_tracks(&tree), (2, 4));
    }

    #[test]
    fn counts_a_kicad_9_board_without_double_counting_declarations() {
        // Two named nets across three declarations and four references.
        let tree = parse_sexp(KICAD_9_BOARD).unwrap();
        assert_eq!(count_nets_and_tracks(&tree), (2, 4));
    }

    #[test]
    fn the_unconnected_pseudo_net_does_not_count() {
        let tree = parse_sexp("(kicad_pcb\n\t(net 0 \"\")\n)\n").unwrap();
        assert_eq!(count_nets_and_tracks(&tree), (0, 0));
    }

    /// A zone outline may carry `(arc …)` inside its `(pts …)`; that is a
    /// polygon corner, not routed copper. Counting arcs anywhere in the tree
    /// would call an unrouted board routed.
    #[test]
    fn zone_outline_arcs_are_not_routed_copper() {
        let tree = parse_sexp(
            "(kicad_pcb\n\t(zone\n\t\t(net \"GND\")\n\t\t(polygon\n\t\t\t(pts\n\t\t\t\t(arc (start 0 0) (mid 5 5) (end 10 0))\n\t\t\t)\n\t\t)\n\t)\n)\n",
        )
        .unwrap();
        assert_eq!(count_nets_and_tracks(&tree), (1, 0));
    }

    async fn validate(board_text: &str) -> serde_json::Value {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("board.kicad_pcb");
        std::fs::write(&path, board_text).unwrap();
        let result = handle_validate_for_manufacturing(
            &json!({ "board": path.display().to_string() }),
            &test_ctx(),
        )
        .await
        .unwrap();
        let crate::mcp::protocol::ToolContent::Text { text } = &result.content[0] else {
            panic!("expected text");
        };
        serde_json::from_str(text).unwrap()
    }

    #[tokio::test]
    async fn a_routed_kicad_10_board_reports_its_nets_and_tracks() {
        let report = validate(KICAD_10_BOARD).await;
        assert_eq!(report["board_info"]["net_count"], json!(2));
        assert_eq!(report["board_info"]["track_count"], json!(4));
    }

    /// The symptom that surfaced this: an unrouted board came back clean on
    /// the routing check because both counts read zero, so the
    /// `net_count > 3 && track_count == 0` guard could never fire.
    #[tokio::test]
    async fn an_unrouted_kicad_10_board_is_flagged() {
        let unrouted = "(kicad_pcb\n\t(version 20260206)\n\t(generator \"pcbnew\")\n\t(footprint \"R_0805\"\n\t\t(pad \"1\" smd rect\n\t\t\t(net \"GND\")\n\t\t)\n\t\t(pad \"2\" smd rect\n\t\t\t(net \"VCC\")\n\t\t)\n\t)\n\t(footprint \"C_0805\"\n\t\t(pad \"1\" smd rect\n\t\t\t(net \"SDA\")\n\t\t)\n\t\t(pad \"2\" smd rect\n\t\t\t(net \"SCL\")\n\t\t)\n\t)\n)\n";
        let report = validate(unrouted).await;
        assert_eq!(report["board_info"]["net_count"], json!(4));
        assert_eq!(report["board_info"]["track_count"], json!(0));
        assert!(
            report["issues"]
                .as_array()
                .unwrap()
                .iter()
                .any(|i| i["issue"]
                    .as_str()
                    .unwrap_or("")
                    .contains("no traces routed")),
            "the unrouted-net issue is missing: {report}"
        );
    }
}

#[cfg(test)]
mod drc_gate_tests {
    use super::*;
    use crate::router::ToolRouter;
    use crate::tools::drc_gate::{DrcEvidence, DrcFinding};
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

    /// Everything a fab check looks at without DRC is satisfied: an outline,
    /// two footprints, four nets, and copper on three of them. Only the
    /// fourth net, `SCL`, is unrouted — the case
    /// `net_count > 3 && track_count == 0` cannot see, because there *are*
    /// tracks.
    const ROUTED_EXCEPT_ONE_NET: &str = "(kicad_pcb
	(version 20260206)
	(generator \"pcbnew\")
	(layers
		(0 \"F.Cu\" signal)
		(44 \"Edge.Cuts\" user)
	)
	(footprint \"R_0805\"
		(pad \"1\" smd rect (net \"GND\"))
		(pad \"2\" smd rect (net \"VCC\"))
	)
	(footprint \"C_0805\"
		(pad \"1\" smd rect (net \"SDA\"))
		(pad \"2\" smd rect (net \"SCL\"))
	)
	(segment (start 110 110) (end 120 110) (width 0.2) (layer \"F.Cu\") (net \"GND\"))
	(segment (start 110 120) (end 120 120) (width 0.2) (layer \"F.Cu\") (net \"VCC\"))
	(segment (start 110 130) (end 120 130) (width 0.2) (layer \"F.Cu\") (net \"SDA\"))
)
";

    async fn validate_with(board_text: &str, drc: DrcEvidence) -> serde_json::Value {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("board.kicad_pcb");
        std::fs::write(&path, board_text).unwrap();
        let result =
            validate_for_manufacturing_with(&json!({ "board": path.display().to_string() }), &drc)
                .await
                .unwrap();
        let crate::mcp::protocol::ToolContent::Text { text } = &result.content[0] else {
            panic!("expected text");
        };
        serde_json::from_str(text).unwrap()
    }

    fn unconnected(description: &str) -> crate::tools::cli::DrcViolation {
        crate::tools::cli::DrcViolation {
            severity: "error".to_string(),
            description: description.to_string(),
            pos: None,
            rule: Some("unconnected_items".to_string()),
            category: crate::tools::cli::DrcCategory::UnconnectedItems,
            items: Vec::new(),
        }
    }

    /// The heart of the item: 99% routed is not routed, and the old guard
    /// could only fire on a board with no copper at all.
    #[tokio::test]
    async fn a_board_routed_except_one_net_is_not_ready() {
        let report = crate::tools::cli::DrcReport {
            violations: Some(vec![]),
            unconnected_items: Some(vec![unconnected(
                "Missing connection between items: Pad 2 [SCL] on C1",
            )]),
            schematic_parity: Some(vec![]),
        };
        let out = validate_with(ROUTED_EXCEPT_ONE_NET, DrcEvidence::Measured(report)).await;
        assert_eq!(
            out["verdict"], "NOT READY",
            "an unrouted net is a fab blocker, not a detail: {out}"
        );
        assert_eq!(out["drc"]["unconnected_items"], json!(1), "{out}");
    }

    /// A DRC that could not run is not a DRC that found nothing.
    #[tokio::test]
    async fn drc_that_could_not_run_is_incomplete_and_summarised_as_null() {
        let out = validate_with(
            ROUTED_EXCEPT_ONE_NET,
            DrcEvidence::Unavailable("Failed to spawn kicad-cli".to_string()),
        )
        .await;
        assert_eq!(
            out["verdict"], "INCOMPLETE",
            "a board nobody checked is not READY: {out}"
        );
        assert!(
            out["drc"].is_null(),
            "an unmeasured DRC must be null, never a counters object reading zero: {out}"
        );
        assert!(
            out["issues"].as_array().unwrap().iter().any(|i| i["issue"]
                .as_str()
                .unwrap_or("")
                .contains("Failed to spawn kicad-cli")),
            "the missing evidence has to be named: {out}"
        );
    }

    /// `missing_categories()` non-empty: the report is short a whole pass.
    #[tokio::test]
    async fn a_missing_category_does_not_read_as_zero_findings() {
        let report = crate::tools::cli::DrcReport {
            violations: Some(vec![]),
            unconnected_items: None,
            schematic_parity: Some(vec![]),
        };
        let out = validate_with(ROUTED_EXCEPT_ONE_NET, DrcEvidence::Measured(report)).await;
        assert_eq!(out["verdict"], "INCOMPLETE", "{out}");
        assert!(out["drc"]["unconnected_items"].is_null(), "{out}");
        assert!(
            out["drc"]["missing_categories"]
                .as_array()
                .unwrap()
                .contains(&json!("unconnected_items")),
            "{out}"
        );
    }

    /// With connectivity measured, the track-count heuristic must not add a
    /// second, cruder voice: DRC already answers "is this routed".
    #[tokio::test]
    async fn the_track_count_heuristic_yields_to_a_measured_drc() {
        let unrouted = "(kicad_pcb
	(version 20260206)
	(footprint \"R\" (pad \"1\" smd rect (net \"GND\")) (pad \"2\" smd rect (net \"VCC\")))
	(footprint \"C\" (pad \"1\" smd rect (net \"SDA\")) (pad \"2\" smd rect (net \"SCL\")))
)
";
        let report = crate::tools::cli::DrcReport {
            violations: Some(vec![]),
            unconnected_items: Some(vec![]),
            schematic_parity: Some(vec![]),
        };
        let out = validate_with(unrouted, DrcEvidence::Measured(report)).await;
        assert!(
            !out["issues"].as_array().unwrap().iter().any(|i| i["issue"]
                .as_str()
                .unwrap_or("")
                .contains("no traces routed")),
            "the heuristic contradicted a DRC that measured connectivity: {out}"
        );
    }

    /// The handler still reaches for kicad-cli itself when nobody hands it a
    /// report: an empty `kicad_cli` is missing evidence, not a clean board.
    #[tokio::test]
    async fn the_handler_gathers_its_own_evidence_and_says_when_it_cannot() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("board.kicad_pcb");
        std::fs::write(&path, ROUTED_EXCEPT_ONE_NET).unwrap();
        let result = handle_validate_for_manufacturing(
            &json!({ "board": path.display().to_string() }),
            &test_ctx(),
        )
        .await
        .unwrap();
        let crate::mcp::protocol::ToolContent::Text { text } = &result.content[0] else {
            panic!("expected text");
        };
        let out: serde_json::Value = serde_json::from_str(text).unwrap();
        assert_eq!(out["verdict"], "INCOMPLETE", "{out}");
        assert!(out["drc"].is_null(), "{out}");
    }

    #[test]
    fn an_unavailable_drc_produces_one_named_finding() {
        let gate = crate::tools::drc_gate::assess(&DrcEvidence::Unavailable("no binary".into()));
        assert!(gate.incomplete);
        assert!(gate.summary.is_null());
        assert!(!gate.connectivity_measured);
        let DrcFinding { issue, .. } = &gate.findings[0];
        assert!(issue.contains("no binary"), "{issue}");
    }
}
