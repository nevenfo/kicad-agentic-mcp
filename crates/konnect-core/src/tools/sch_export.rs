//! `sch_export` toolset — export, netlist, ERC, connectivity fix, board sync.
//!
//! All export operations delegate to `kicad-cli` via the `cli` module.
//! `export_netlist_summary` and `fix_connectivity` operate directly on
//! S-expression file content so they work without a running KiCAD instance.

use crate::mcp::error::ToolErrorKind;
use crate::mcp::protocol::CallToolResult;
use crate::tool;
use crate::tools::{get_path, ToolContext, ToolDef};
use konnect_sexp::{
    geometry::{point_on_segment, points_coincident},
    schematic::{
        extract_labels, extract_lib_pins_for_unit, extract_symbol_instances, extract_wires,
        find_lib_symbol, pin_endpoint, read_schematic,
    },
    writer::{
        apply_edits, find_block_with_leading_whitespace, write_atomic_if_unchanged, SexpEdit,
    },
};
use serde_json::json;
use std::path::{Path, PathBuf};

use super::cli;
use super::sch_analysis::build_net_graph;

// ─── Tool definitions ─────────────────────────────────────────────────────────

pub fn tools() -> Vec<ToolDef> {
    vec![
        tool!(
            "export_schematic_svg",
            "Export a schematic sheet to an SVG file using kicad-cli.",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string", "description": "Path to .kicad_sch file" },
                    "output":    { "type": "string", "description": "Output SVG file path (directory used as output dir)" },
                    "black_and_white": { "type": "boolean", "description": "Render in black and white", "default": false },
                    "theme": { "type": "string", "description": "KiCAD colour theme name (optional)" }
                },
                "required": ["schematic", "output"]
            }),
            |args, ctx| async move { handle_export_svg(args, ctx).await }
        ),
        tool!(
            "export_schematic_pdf",
            "Export a schematic sheet to a PDF file using kicad-cli.",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string", "description": "Path to .kicad_sch file" },
                    "output":    { "type": "string", "description": "Output PDF file path" },
                    "black_and_white": { "type": "boolean", "description": "Render in black and white", "default": false },
                    "all_sheets": { "type": "boolean", "description": "Include all hierarchical sheets", "default": true }
                },
                "required": ["schematic", "output"]
            }),
            |args, ctx| async move { handle_export_pdf(args, ctx).await }
        ),
        tool!(
            "generate_netlist",
            "Generate a KiCAD netlist file from the schematic using kicad-cli.",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string", "description": "Path to .kicad_sch file" },
                    "output":    { "type": "string", "description": "Output .net file path" },
                    "format": {
                        "type": "string",
                        "description": "Netlist format: 'kicad', 'orcadpcb2', 'cadstar', 'spice'",
                        "default": "kicad"
                    }
                },
                "required": ["schematic", "output"]
            }),
            |args, ctx| async move { handle_generate_netlist(args, ctx).await }
        ),
        tool!(
            "export_netlist_summary",
            "Return a human-readable JSON summary of the schematic netlist: all \
             components, their nets, pin counts. Does not require kicad-cli.",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string", "description": "Path to .kicad_sch file" }
                },
                "required": ["schematic"]
            }),
            |args, ctx| async move { handle_export_netlist_summary(args, ctx).await }
        ),
        tool!(
            "run_erc",
            "Run the Electrical Rules Check (ERC) on the schematic via kicad-cli \
             and return a list of violations filtered by severity.",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string", "description": "Path to .kicad_sch file" },
                    "output":    { "type": "string", "description": "Optional path to write ERC report JSON" },
                    "severity":  {
                        "type": "string",
                        "description": "Minimum severity to report: 'error', 'warning', 'info'",
                        "default": "warning"
                    }
                },
                "required": ["schematic"]
            }),
            |args, ctx| async move { handle_run_erc(args, ctx).await }
        ),
        tool!(
            "fix_connectivity",
            "Scan the schematic for near-miss wire endpoints (within snap_tolerance of a \
             pin or label but not exactly on it) and snap them into place. Use dry_run \
             to preview fixes without writing.",
            json!({
                "type": "object",
                "properties": {
                    "schematic":       { "type": "string", "description": "Path to .kicad_sch file" },
                    "snap_tolerance":  { "type": "number", "description": "Snap distance in mm", "default": 0.05 },
                    "dry_run":         { "type": "boolean", "description": "Report fixes without applying them", "default": false }
                },
                "required": ["schematic"]
            }),
            |args, ctx| async move { handle_fix_connectivity(args, ctx).await }
        ),
        // Registered here, not in `pcb_export`, because it reads a `.kicad_sch`
        // and nothing else. In the `pcb_export` toolset an agent holding every
        // schematic toolset still got `toolset_not_loaded` and paid a failed
        // call plus a `load_toolset` round trip to reach a schematic export.
        tool!(
            "export_bom",
            "Generate a Bill of Materials (BOM) CSV from the schematic's component data.",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string", "description": "Path to .kicad_sch file (BOM uses schematic data)" },
                    "output": { "type": "string", "description": "Output CSV file path" },
                    "format": {
                        "type": "string",
                        "description": "BOM format. 'kicad-cli sch export bom' has no --format flag at all — \
                                         output is always its fixed CSV-like column set. The only accepted \
                                         value is 'csv'; anything else is refused rather than silently ignored.",
                        "default": "csv",
                        "enum": ["csv"]
                    },
                    "exclude_dnp": {
                        "type": "boolean",
                        "description": "Exclude 'Do Not Place' components",
                        "default": true
                    },
                    "fields": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Ordered list of fields to export, e.g. ['Reference','Value','MPN','LCSC']. \
                                         Generated fields (QUANTITY, ITEM_NUMBER, DNP, EXCLUDE_FROM_BOM, \
                                         EXCLUDE_FROM_BOARD, EXCLUDE_FROM_SIM) may be listed with or without \
                                         '${}' delimiters. Omitted entirely: kicad-cli falls back to its own \
                                         default 'Reference,Value,Footprint,QUANTITY,DNP'."
                    },
                    "labels": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Ordered column labels applied to 'fields' in order. Measured against \
                                         kicad-cli 10.0.3: fewer labels than fields leaves the remaining columns \
                                         titled with the field name itself; more labels than fields has the extras \
                                         ignored — neither shifts columns silently. Omitted entirely: kicad-cli \
                                         falls back to its own default 'Refs,Value,Footprint,Qty,DNP'."
                    },
                    "group_by": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Fields to group references by when their values match, e.g. ['Value'] to \
                                         merge R1 and R2 into one 'R1,R2' row. Every entry must name a field that \
                                         is actually being exported (in 'fields', or in kicad-cli's default field \
                                         set when 'fields' is omitted) — kicad-cli otherwise accepts the name and \
                                         silently produces no grouping at all. Omitted entirely: no grouping."
                    }
                },
                "required": ["schematic", "output"]
            }),
            |args, ctx| async move { handle_export_bom(args, ctx).await }
        ),
    ]
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

async fn handle_export_bom(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let schematic = get_path(args, "schematic")?;
    let output = get_path(args, "output")?;

    // `kicad-cli sch export bom --help` (KiCAD 10.0.3) has no --format flag —
    // the BOM is always the fixed Reference,Value,Footprint,QUANTITY,DNP set.
    // The schema still advertises the argument for backward compatibility, so
    // it is validated as a closed set rather than accepted and silently
    // dropped: a caller asking for a format kicad-cli cannot produce should
    // be told, not handed a CSV they didn't ask for.
    let format = args["format"].as_str().unwrap_or("csv");
    if format != "csv" {
        return Ok(CallToolResult::error_kind(
            ToolErrorKind::InvalidArgument {
                field: "format".to_string(),
                reason: format!("'{format}' is not a supported BOM format"),
            },
            format!(
                "'{format}' is not a supported BOM format. kicad-cli's BOM export has no \
                 --format flag; the only value 'export_bom' accepts is 'csv'."
            ),
        ));
    }

    // The schema has promised `exclude_dnp` (default true) since this tool
    // shipped; the handler never read it, so every BOM included DNP parts
    // regardless of what the caller asked for.
    let exclude_dnp = args["exclude_dnp"].as_bool().unwrap_or(true);
    let fields = string_array(args, "fields");
    let labels = string_array(args, "labels");
    let group_by = string_array(args, "group_by");

    // kicad-cli 10.0.3, measured: `--group-by` naming a field absent from the
    // effective field list (the caller's `--fields`, or kicad-cli's own
    // default set when `--fields` is omitted) is accepted and silently
    // produces no grouping at all — no error, no warning, just a BOM that
    // looks grouped-and-isn't. That is exactly the kind of trap this tool
    // refuses rather than reproduces.
    const DEFAULT_FIELDS: [&str; 5] = ["Reference", "Value", "Footprint", "QUANTITY", "DNP"];
    let effective_fields: Vec<&str> = if fields.is_empty() {
        DEFAULT_FIELDS.to_vec()
    } else {
        fields.iter().map(|f| f.as_str()).collect()
    };
    fn normalize(f: &str) -> &str {
        f.trim_start_matches("${").trim_end_matches('}')
    }
    for g in &group_by {
        let g_norm = normalize(g);
        if !effective_fields.iter().any(|f| normalize(f) == g_norm) {
            let reason = format!(
                "'{g}' is not among the exported fields ({}); kicad-cli would accept it and \
                 silently group nothing.",
                effective_fields.join(",")
            );
            return Ok(CallToolResult::error_kind(
                ToolErrorKind::InvalidArgument {
                    field: "group_by".to_string(),
                    reason: reason.clone(),
                },
                reason,
            ));
        }
    }

    let options = cli::BomOptions {
        exclude_dnp,
        fields,
        labels,
        group_by,
    };

    let cli = &ctx.config.kicad_cli;
    cli::export_bom(cli, &schematic, &output, &options).await?;

    Ok(CallToolResult::text(
        serde_json::to_string(&json!({
            "success": true,
            "output": output.to_str().unwrap_or(""),
            "exclude_dnp": exclude_dnp
        }))
        .unwrap(),
    ))
}

/// Reads `args[key]` as a `Vec<String>`, treating an absent or non-array
/// value as empty rather than an error — the schema documents these as
/// optional lists whose absence means "let kicad-cli use its own default".
fn string_array(args: &serde_json::Value, key: &str) -> Vec<String> {
    args[key]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

async fn handle_export_svg(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let output_path = get_path(args, "output")?;

    // kicad-cli writes to an output directory and names the file <stem>.svg
    let output_dir = output_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .to_path_buf();
    std::fs::create_dir_all(&output_dir)?;

    let svg_path = cli::export_schematic_svg(&ctx.config.kicad_cli, &sch_path, &output_dir).await?;

    Ok(CallToolResult::json(&json!({
        "exported": svg_path.display().to_string()
    })))
}

async fn handle_export_pdf(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let output_path = get_path(args, "output")?;

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    cli::export_schematic_pdf(&ctx.config.kicad_cli, &sch_path, &output_path).await?;

    Ok(CallToolResult::json(&json!({
        "exported": output_path.display().to_string()
    })))
}

async fn handle_generate_netlist(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let output_path = get_path(args, "output")?;
    let format = args["format"].as_str().unwrap_or("kicad");

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    cli::export_netlist(&ctx.config.kicad_cli, &sch_path, &output_path, format).await?;

    Ok(CallToolResult::json(&json!({
        "exported": output_path.display().to_string(),
        "format": format
    })))
}

async fn handle_export_netlist_summary(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let (_, tree) = read_schematic(&sch_path)?;

    let instances = extract_symbol_instances(&tree);
    let wires = extract_wires(&tree);
    let labels = extract_labels(&tree);
    let lib_syms = tree
        .find("lib_symbols")
        .map(|n| n.find_all("symbol"))
        .unwrap_or_default();

    let mut g = build_net_graph(
        &wires,
        &labels,
        &konnect_sexp::schematic::extract_junction_points(&tree),
    );

    // Collect distinct net names
    let mut net_names: Vec<String> = labels.iter().map(|l| l.net.clone()).collect();
    net_names.sort();
    net_names.dedup();

    // Build per-component net map
    let components: Vec<serde_json::Value> = instances
        .iter()
        .map(|inst| {
            let lib_sym = find_lib_symbol(&lib_syms, inst);

            let pins: Vec<serde_json::Value> = if let Some(sym) = lib_sym {
                let t = inst.pin_transform();
                extract_lib_pins_for_unit(sym, inst.unit)
                    .iter()
                    .map(|p| {
                        let (px, py) = pin_endpoint(p, t);
                        let net = g.net_at(px, py).unwrap_or_else(|| "~".to_string());
                        json!({
                            "number": p.number,
                            "name": p.name,
                            "net": net,
                            "x": px, "y": py
                        })
                    })
                    .collect()
            } else {
                Vec::new()
            };

            json!({
                "reference": inst.reference,
                "value": inst.value,
                "footprint": inst.footprint,
                "lib_id": inst.lib_id,
                "pin_count": pins.len(),
                "pins": pins
            })
        })
        .collect();

    Ok(CallToolResult::json(&json!({
        "component_count": components.len(),
        "net_count": net_names.len(),
        "nets": net_names,
        "components": components
    })))
}

async fn handle_run_erc(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let min_severity = args["severity"].as_str().unwrap_or("warning");

    if let Some(root) = owning_project_root(&sch_path) {
        // Structured, not free text: a caller can react to `invalid_argument`
        // on `schematic` by retrying against the named root, which is exactly
        // what the message says to do.
        let reason = format!(
            "{} is a sheet inside the project rooted at {}, not a project root of its own. \
             kicad-cli treats the file it is handed as the root and looks for a .kicad_pro \
             beside it, so the project's sym-lib-table is never read and every symbol from a \
             project library is reported as an unknown library — violations that describe the \
             invocation, not the design. ERC covers the whole hierarchy in any case: run it on \
             {}.",
            sch_path.display(),
            root.display(),
            root.display()
        );
        return Ok(CallToolResult::error_kind(
            ToolErrorKind::InvalidArgument {
                field: "schematic".to_string(),
                reason: reason.clone(),
            },
            reason,
        ));
    }

    let violations = cli::run_erc(&ctx.config.kicad_cli, &sch_path).await?;

    let severity_rank = |s: &str| match s {
        "error" => 2,
        "warning" => 1,
        _ => 0,
    };
    let min_rank = severity_rank(min_severity);

    let filtered: Vec<serde_json::Value> = violations
        .iter()
        .filter(|v| severity_rank(&v.severity) >= min_rank)
        .map(|v| {
            let mut entry = json!({
                "severity": v.severity,
                "description": v.description,
            });
            if let Some(sheet) = &v.sheet {
                entry["sheet"] = json!(sheet);
            }
            if let Some(pos) = &v.pos {
                entry["x"] = json!(pos.x);
                entry["y"] = json!(pos.y);
            }
            if !v.items.is_empty() {
                entry["items"] = json!(v
                    .items
                    .iter()
                    .map(|item| {
                        let mut i = json!({ "description": item.description, "uuid": item.uuid });
                        if let Some(pos) = &item.pos {
                            i["x"] = json!(pos.x);
                            i["y"] = json!(pos.y);
                        }
                        i
                    })
                    .collect::<Vec<_>>());
            }
            entry
        })
        .collect();

    // Optionally write the report to a file
    if let Some(out_path) = args["output"].as_str() {
        let report = serde_json::to_string_pretty(&filtered)?;
        std::fs::write(out_path, report)?;
    }

    let error_count = filtered.iter().filter(|v| v["severity"] == "error").count();
    let warning_count = filtered
        .iter()
        .filter(|v| v["severity"] == "warning")
        .count();

    Ok(CallToolResult::json(&json!({
        "total": filtered.len(),
        "errors": error_count,
        "warnings": warning_count,
        "violations": filtered
    })))
}

async fn handle_fix_connectivity(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let snap_tol = args["snap_tolerance"].as_f64().unwrap_or(0.05);
    let dry_run = args["dry_run"].as_bool().unwrap_or(false);
    let exact_tol = 0.01_f64;

    let (content, tree) = read_schematic(&sch_path)?;
    let wires = extract_wires(&tree);
    let labels = extract_labels(&tree);
    let instances = extract_symbol_instances(&tree);
    let lib_syms = tree
        .find("lib_symbols")
        .map(|n| n.find_all("symbol"))
        .unwrap_or_default();

    // Collect all valid snap targets: pin endpoints + label positions + wire endpoints
    let mut snap_targets: Vec<(f64, f64)> = Vec::new();

    for inst in &instances {
        let lib_sym = find_lib_symbol(&lib_syms, inst);
        if let Some(sym) = lib_sym {
            let t = inst.pin_transform();
            for pin in extract_lib_pins_for_unit(sym, inst.unit) {
                snap_targets.push(pin_endpoint(&pin, t));
            }
        }
    }
    for l in &labels {
        snap_targets.push((l.x, l.y));
    }
    for w in &wires {
        snap_targets.push((w.x1, w.y1));
        snap_targets.push((w.x2, w.y2));
    }

    let mut fixes: Vec<serde_json::Value> = Vec::new();
    let mut file_edits: Vec<SexpEdit> = Vec::new();

    for w in &wires {
        for (is_start, (px, py)) in &[(true, (w.x1, w.y1)), (false, (w.x2, w.y2))] {
            let px = *px;
            let py = *py;
            // Count how many targets are exactly at this point
            // (count >= 2 → there is at least one other connected thing)
            let exact_count = snap_targets
                .iter()
                .filter(|(tx, ty)| points_coincident(px, py, *tx, *ty, exact_tol))
                .count();

            if exact_count >= 2 {
                continue; // already connected
            }
            // Also consider T-junctions (endpoint in middle of another wire)
            if wires.iter().any(|w2| {
                point_on_segment(px, py, w2.x1, w2.y1, w2.x2, w2.y2, exact_tol)
                    && !points_coincident(px, py, w2.x1, w2.y1, exact_tol)
                    && !points_coincident(px, py, w2.x2, w2.y2, exact_tol)
            }) {
                continue; // T-junction — already connected
            }

            // Look for a near-miss snap target within snap_tol
            let near = snap_targets.iter().find(|(tx, ty)| {
                let dist = ((px - tx).powi(2) + (py - ty).powi(2)).sqrt();
                dist > exact_tol && dist <= snap_tol
            });

            if let Some(&(tx, ty)) = near {
                fixes.push(json!({
                    "wire_uuid": w.uuid,
                    "endpoint": if *is_start { "start" } else { "end" },
                    "from": { "x": px, "y": py },
                    "to":   { "x": tx, "y": ty }
                }));

                if !dry_run {
                    // Find the wire block by UUID and replace the coordinate
                    if let Some(uuid_str) = &w.uuid {
                        let uuid_pat = format!(r#"(uuid "{uuid_str}")"#);
                        if let Some(uuid_pos) = content.find(&uuid_pat) {
                            let before = &content[..uuid_pos];
                            if let Some(ws) = before.rfind("\n  (wire").map(|p| p + 1) {
                                if let Some((wbs, wbe)) =
                                    find_block_with_leading_whitespace(&content, ws)
                                {
                                    let wire_block = &content[wbs..wbe];
                                    let coord_prefix = if *is_start { "(start " } else { "(end " };
                                    if let Some(coord_rel) = wire_block.find(coord_prefix) {
                                        let vals_abs = wbs + coord_rel + coord_prefix.len();
                                        let close_rel =
                                            wire_block[coord_rel..].find(')').unwrap_or(0);
                                        let vals_end = wbs + coord_rel + close_rel;
                                        file_edits.push(SexpEdit::replace(
                                            vals_abs,
                                            vals_end,
                                            format!("{tx} {ty}"),
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let applied_count = if dry_run { 0 } else { file_edits.len() };
    if applied_count > 0 {
        let expected = content.clone();
        let new_content = apply_edits(content, file_edits);
        write_atomic_if_unchanged(&sch_path, &expected, &new_content)?;
    }

    Ok(CallToolResult::json(&json!({
        "fixes_found": fixes.len(),
        "applied": !dry_run && !fixes.is_empty(),
        "dry_run": dry_run,
        "fixes": fixes
    })))
}

// ─── Project-root detection for ERC ───────────────────────────────────────────

/// The root schematic of the project that owns `file` as a sub-sheet, if any.
///
/// `kicad-cli` treats whatever file it is handed as the root of the hierarchy
/// and looks for a `.kicad_pro` named after it. A sub-sheet has no such file,
/// so the project's `sym-lib-table` is never read and every symbol from a
/// project library comes back as an unknown library (measured against
/// KiCad 10.0.3: running `sch erc` on a sub-sheet of `demos/complex_hierarchy`
/// produces 67 violations, 46 of them `lib_symbol_issues` — "the current
/// configuration does not include the symbol library" — against 0 when run on
/// the project's own root sheet). Those violations describe the invocation
/// rather than the design, and the obvious remedy — registering the library
/// again — is the wrong one, so the case is worth naming.
///
/// This looks for the owning project only in `file`'s own directory, not by
/// walking up ancestors: this fork has no ancestor-walking `project_root_for`
/// (unlike upstream's `library.rs`), and every sub-sheet reachable from a
/// hierarchy — including both bundled KiCad demo hierarchies — sits beside
/// its project's `.kicad_pro`. A sheet that has been moved to a different
/// directory than its project is not a case this refusal claims to catch.
///
/// Returns `None` for a schematic that is a root in its own right, one that
/// belongs to no project, and one that sits beside a project without appearing
/// in its sheet tree.
fn owning_project_root(file: &Path) -> Option<PathBuf> {
    if file.with_extension("kicad_pro").is_file() {
        return None;
    }
    let dir = file.parent().unwrap_or_else(|| Path::new("."));
    let root = project_root_schematic(dir)?;
    if same_file(&root, file) {
        return None;
    }
    sheet_tree_contains(&root, file).then_some(root)
}

/// The `<stem>.kicad_sch` beside the single `.kicad_pro` in `dir`. A directory
/// holding more than one project says nothing definite about which root a loose
/// sheet belongs to, so it yields nothing rather than a guess.
pub(crate) fn project_root_schematic(dir: &Path) -> Option<PathBuf> {
    let mut found: Option<PathBuf> = None;
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "kicad_pro") {
            if found.is_some() {
                return None;
            }
            found = Some(path);
        }
    }
    let sch = found?.with_extension("kicad_sch");
    sch.is_file().then_some(sch)
}

/// Whether `target` is reachable as a sheet from `root` — the same question
/// [`crate::tools::reachable_sheets`] answers for the whole tree at once, so
/// this asks it rather than re-walking.
///
/// Note the widening this brought: the walk includes `root` itself, so a
/// `target` that *is* the root now answers true where the old child-only
/// recursion answered false. `owning_project_root`, the one caller, has
/// already returned for that case (`same_file(&root, file)`) before asking.
fn sheet_tree_contains(root: &Path, target: &Path) -> bool {
    crate::tools::reachable_sheets(root)
        .sheets
        .iter()
        .any(|sheet| same_file(sheet, target))
}

/// Path equality that survives `.\foo` versus `foo` and case-insensitive
/// filesystems. Falls back to a literal comparison for paths that do not exist.
pub(crate) fn same_file(a: &Path, b: &Path) -> bool {
    canonical(a) == canonical(b)
}

pub(crate) fn canonical(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

#[cfg(test)]
mod erc_project_root_tests {
    use super::*;
    use tempfile::TempDir;

    /// A root sheet holding one sub-sheet reference. The `(sheet …)` block is
    /// shaped the way KiCad 10 writes one — trimmed to the fields the loader
    /// reads.
    fn root_with_child(dir: &Path, root: &str, child_file: &str) -> PathBuf {
        let path = dir.join(root);
        std::fs::write(
            &path,
            format!(
                r#"(kicad_sch
	(version 20250610)
	(generator "konnect")
	(generator_version "10.0")
	(uuid "00000000-0000-4000-8000-000000000001")
	(paper "A4")
	(sheet
		(at 40 50)
		(size 80 25)
		(uuid "00000000-0000-4000-8000-000000000002")
		(property "Sheetname" "Child"
			(at 40 49.365 0)
		)
		(property "Sheetfile" "{child_file}"
			(at 40 75.635 0)
		)
	)
	(sheet_instances
		(path "/" (page "1"))
	)
)
"#
            ),
        )
        .unwrap();
        path
    }

    fn write(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, body).unwrap();
        path
    }

    fn blank(dir: &Path, name: &str) -> PathBuf {
        write(dir, name, &crate::tools::blank_schematic_template())
    }

    #[test]
    fn child_sheet_resolves_to_its_project_root() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "proj.kicad_pro", "{}");
        let root = root_with_child(tmp.path(), "proj.kicad_sch", "child.kicad_sch");
        let child = blank(tmp.path(), "child.kicad_sch");

        assert_eq!(owning_project_root(&child), Some(root));
    }

    #[test]
    fn a_root_of_its_own_is_left_alone() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "proj.kicad_pro", "{}");
        let root = root_with_child(tmp.path(), "proj.kicad_sch", "child.kicad_sch");
        blank(tmp.path(), "child.kicad_sch");

        assert_eq!(owning_project_root(&root), None);
    }

    #[test]
    fn a_sheet_belonging_to_no_project_is_left_alone() {
        let tmp = TempDir::new().unwrap();
        let loose = blank(tmp.path(), "loose.kicad_sch");

        assert_eq!(owning_project_root(&loose), None);
    }

    /// The refusal is a structured `invalid_argument` naming `schematic`, so a
    /// caller can react by retrying against the root the message names.
    #[tokio::test]
    async fn the_sub_sheet_refusal_is_structured_and_names_the_field() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "proj.kicad_pro", "{}");
        root_with_child(tmp.path(), "proj.kicad_sch", "child.kicad_sch");
        let child = blank(tmp.path(), "child.kicad_sch");

        let ctx = crate::tools::ToolContext::new(
            crate::tools::ServerConfig {
                kicad_cli: String::new(),
                kicad_binary: String::new(),
                ipc_address: String::new(),
                project_dir: None,
                jlcpcb_db_path: None,
                auto_load_toolsets: false,
                mode: kam_state::OperatingMode::Write,
            },
            std::sync::Arc::new(crate::router::ToolRouter::new()),
        );
        let result = handle_run_erc(
            &serde_json::json!({ "schematic": child.display().to_string() }),
            &ctx,
        )
        .await
        .expect("a refusal is a tool error, not a transport error");

        assert!(result.is_error);
        let text = match result.content.first() {
            Some(crate::mcp::protocol::ToolContent::Text { text }) => text.clone(),
            other => panic!("expected text content, got {other:?}"),
        };
        let output: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(output["error"]["kind"], "invalid_argument");
        assert_eq!(output["error"]["field"], "schematic");
        assert!(output["error"]["reason"]
            .as_str()
            .unwrap()
            .contains("proj.kicad_sch"));
    }

    /// Sitting beside a project is not the same as belonging to it — the file
    /// has to appear in the sheet tree.
    #[test]
    fn unrelated_neighbour_is_left_alone() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "proj.kicad_pro", "{}");
        root_with_child(tmp.path(), "proj.kicad_sch", "child.kicad_sch");
        blank(tmp.path(), "child.kicad_sch");
        let stranger = blank(tmp.path(), "stranger.kicad_sch");

        assert_eq!(owning_project_root(&stranger), None);
    }

    /// A sheet cycle must not hang the walk.
    #[test]
    fn a_reference_cycle_terminates() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "proj.kicad_pro", "{}");
        root_with_child(tmp.path(), "proj.kicad_sch", "a.kicad_sch");
        root_with_child(tmp.path(), "a.kicad_sch", "proj.kicad_sch");
        let stranger = blank(tmp.path(), "stranger.kicad_sch");

        assert_eq!(owning_project_root(&stranger), None);
    }

    /// A directory with two candidate `.kicad_pro` files says nothing definite
    /// about which root a loose sheet belongs to; refusing arbitrarily would be
    /// worse than doing nothing, so this stays a pass-through.
    #[test]
    fn a_directory_with_multiple_projects_does_not_refuse_arbitrarily() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "one.kicad_pro", "{}");
        write(tmp.path(), "two.kicad_pro", "{}");
        root_with_child(tmp.path(), "one.kicad_sch", "child.kicad_sch");
        let child = blank(tmp.path(), "child.kicad_sch");

        assert_eq!(owning_project_root(&child), None);
    }
}
