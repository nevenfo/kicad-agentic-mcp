//! `pcb_routing` toolset — traces, vias, copper pours, nets, netclasses, and diff pairs.
//!
//! Routing operations use the KiCAD IPC API; `add_net` and `add_copper_pour`
//! use S-expression file manipulation. `create_netclass` and
//! `assign_net_to_class` edit the sibling `.kicad_pro` — KiCAD has kept net
//! classes in the project's `net_settings` since v7, and the board file has no
//! container for them at all.

use crate::mcp::error::ToolErrorKind;
use crate::mcp::protocol::CallToolResult;
use crate::tool;
use crate::tools::ipc_boundary::{ipc_error_result, with_ipc};
use crate::tools::{get_path, opt_f64, require_f64, require_str, ToolContext, ToolDef};
use konnect_sexp::writer::{apply_edits, new_uuid, write_atomic, SexpEdit};
use serde_json::json;

// ─── IPC helper ───────────────────────────────────────────────────────────────

macro_rules! ipc {
    ($ctx:expr, |$c:ident| $body:expr) => {{
        let addr = $ctx.config.ipc_address.clone();
        match with_ipc(addr, move |$c| $body).await? {
            Ok(v) => v,
            Err(failure) => return Ok(ipc_error_result(&failure)),
        }
    }};
}

// ─── S-expression helpers ─────────────────────────────────────────────────────

fn format_zone(
    net_id: i32,
    net_name: &str,
    layer: &str,
    clearance: f64,
    min_w: f64,
    pts: &[(f64, f64)],
) -> String {
    let uuid = new_uuid();
    let pt_str: String = pts
        .iter()
        .map(|(x, y)| format!("\n      (xy {x} {y})"))
        .collect();
    format!(
        "\n  (zone (net {net_id}) (net_name \"{net_name}\") (layer \"{layer}\") (uuid \"{uuid}\")\n    \
         (hatch edge 0.508)\n    (connect_pads (clearance {clearance}))\n    \
         (min_thickness {min_w})\n    (fill yes)\n    \
         (polygon (pts{pt_str}\n    ))\n  )"
    )
}

fn find_net_id(content: &str, net_name: &str) -> i32 {
    let search = format!(r#" "{net_name}")"#);
    if let Some(pos) = content.find(&search) {
        let before = &content[..pos];
        let net_pos = before.rfind("(net ").unwrap_or(0);
        let num_str = &before[net_pos + 5..];
        let num_end = num_str.find(' ').unwrap_or(0);
        num_str[..num_end].parse().unwrap_or(0)
    } else {
        0
    }
}

// ─── Tool definitions ─────────────────────────────────────────────────────────

pub fn tools() -> Vec<ToolDef> {
    vec![
        tool!(
            "add_net",
            "Add a new net entry to the PCB file (S-expression insert, no KiCAD IPC required).",
            json!({
                "type": "object",
                "properties": {
                    "board":    { "type": "string" },
                    "net_name": { "type": "string" }
                },
                "required": ["board", "net_name"]
            }),
            |args, ctx| async move { handle_add_net(args, ctx).await }
        ),
        tool!(
            "route_trace",
            "Route a trace segment between two points on a copper layer via KiCAD IPC.",
            json!({
                "type": "object",
                "properties": {
                    "board":    { "type": "string" },
                    "net_name": { "type": "string" },
                    "layer":    { "type": "string", "description": "Copper layer (e.g. 'F.Cu')" },
                    "x1": { "type": "number" }, "y1": { "type": "number" },
                    "x2": { "type": "number" }, "y2": { "type": "number" },
                    "width": { "type": "number", "default": 0.25 }
                },
                "required": ["board", "net_name", "layer", "x1", "y1", "x2", "y2"]
            }),
            |args, ctx| async move { handle_route_trace(args, ctx).await }
        ),
        tool!(
            "route_pad_to_pad",
            "Route a direct trace between two pads of named components (L-bend routing) via KiCAD IPC.",
            json!({
                "type": "object",
                "properties": {
                    "board":       { "type": "string" },
                    "net_name":    { "type": "string" },
                    "ref1":        { "type": "string", "description": "First component reference" },
                    "pad1":        { "type": "string", "description": "First pad number" },
                    "ref2":        { "type": "string", "description": "Second component reference" },
                    "pad2":        { "type": "string", "description": "Second pad number" },
                    "layer":       { "type": "string", "default": "F.Cu" },
                    "width":       { "type": "number", "default": 0.25 }
                },
                "required": ["board", "net_name", "ref1", "pad1", "ref2", "pad2"]
            }),
            |args, ctx| async move { handle_route_pad_to_pad(args, ctx).await }
        ),
        tool!(
            "add_via",
            "Add a through-hole via at a given position and assign it to a net via KiCAD IPC.",
            json!({
                "type": "object",
                "properties": {
                    "board":     { "type": "string" },
                    "net_name":  { "type": "string" },
                    "x":         { "type": "number" },
                    "y":         { "type": "number" },
                    "drill":     { "type": "number", "description": "Drill diameter in mm", "default": 0.4 },
                    "pad_size":  { "type": "number", "description": "Via pad diameter in mm", "default": 0.8 }
                },
                "required": ["board", "net_name", "x", "y"]
            }),
            |args, ctx| async move { handle_add_via(args, ctx).await }
        ),
        tool!(
            "add_copper_pour",
            "Add a copper fill zone polygon on a layer/net via S-expression file insert.",
            json!({
                "type": "object",
                "properties": {
                    "board":     { "type": "string" },
                    "net_name":  { "type": "string" },
                    "layer":     { "type": "string", "description": "Copper layer (e.g. 'F.Cu')" },
                    "points": {
                        "type": "array",
                        "items": { "type": "object", "properties": { "x": { "type": "number" }, "y": { "type": "number" } } }
                    },
                    "clearance": { "type": "number", "default": 0.2 },
                    "min_width": { "type": "number", "default": 0.25 }
                },
                "required": ["board", "net_name", "layer", "points"]
            }),
            |args, ctx| async move { handle_add_copper_pour(args, ctx).await }
        ),
        tool!(
            "delete_trace",
            "Delete a trace segment identified by its UUID via KiCAD IPC.",
            json!({
                "type": "object",
                "properties": {
                    "board": { "type": "string" },
                    "uuid":  { "type": "string", "description": "UUID of the track segment to delete" }
                },
                "required": ["board", "uuid"]
            }),
            |args, ctx| async move { handle_delete_trace(args, ctx).await }
        ),
        tool!(
            "query_traces",
            "List trace segments on the board, optionally filtered by net and/or layer.",
            json!({
                "type": "object",
                "properties": {
                    "board":    { "type": "string" },
                    "net_name": { "type": "string", "description": "Filter by net (optional)" },
                    "layer":    { "type": "string", "description": "Filter by layer (optional)" }
                },
                "required": ["board"]
            }),
            |args, ctx| async move { handle_query_traces(args, ctx).await }
        ),
        tool!(
            "get_nets_list",
            "Return all nets defined on the PCB via KiCAD IPC.",
            json!({
                "type": "object",
                "properties": {
                    "board": { "type": "string" }
                },
                "required": ["board"]
            }),
            |args, ctx| async move { handle_get_nets_list(args, ctx).await }
        ),
        tool!(
            "modify_trace",
            "Modify a trace segment by deleting and re-adding it with new parameters.",
            json!({
                "type": "object",
                "properties": {
                    "board":     { "type": "string" },
                    "uuid":      { "type": "string" },
                    "net_name":  { "type": "string" },
                    "layer":     { "type": "string" },
                    "x1": { "type": "number" }, "y1": { "type": "number" },
                    "x2": { "type": "number" }, "y2": { "type": "number" },
                    "width":     { "type": "number", "default": 0.25 }
                },
                "required": ["board", "uuid", "net_name", "layer", "x1", "y1", "x2", "y2"]
            }),
            |args, ctx| async move { handle_modify_trace(args, ctx).await }
        ),
        tool!(
            "create_netclass",
            "Create or update a netclass in the project's design rules. Writes \
             net_settings.classes in the sibling .kicad_pro (where KiCad has kept \
             netclasses since v7); the board file is never touched. An existing \
             class with the same name is updated in place, and only the fields \
             named here move — a repeat call that names one field leaves the rest \
             of the class as it was. Requires the project file to exist.",
            json!({
                "type": "object",
                "properties": {
                    "board":        { "type": "string", "description": "Path to .kicad_pcb file; the sibling .kicad_pro is edited" },
                    "name":         { "type": "string", "description": "Netclass name (e.g. 'Power')" },
                    "clearance":    { "type": "number", "description": "Clearance in mm", "default": 0.2 },
                    "trace_width":  { "type": "number", "description": "Default trace width in mm", "default": 0.25 },
                    "via_drill":    { "type": "number", "description": "Via drill diameter in mm", "default": 0.4 },
                    "via_diameter": { "type": "number", "description": "Via pad diameter in mm", "default": 0.8 }
                },
                "required": ["board", "name"]
            }),
            |args, ctx| async move { handle_create_netclass(args, ctx).await }
        ),
        tool!(
            "assign_net_to_class",
            "Assign a net to an existing netclass, as a netclass_patterns entry in \
             the sibling .kicad_pro. The class must already exist (create_netclass). \
             Reassigning a net moves its pattern to the new class rather than adding \
             a competing one.",
            json!({
                "type": "object",
                "properties": {
                    "board":     { "type": "string", "description": "Path to .kicad_pcb file; the sibling .kicad_pro is edited" },
                    "net_name":  { "type": "string", "description": "Net name to assign" },
                    "netclass":  { "type": "string", "description": "Netclass name to assign the net to" }
                },
                "required": ["board", "net_name", "netclass"]
            }),
            |args, ctx| async move { handle_assign_net_to_class(args, ctx).await }
        ),
        tool!(
            "route_differential_pair",
            "Route a differential pair (two parallel traces with a specified gap).",
            json!({
                "type": "object",
                "properties": {
                    "board":    { "type": "string" },
                    "net_pos":  { "type": "string", "description": "Positive net name" },
                    "net_neg":  { "type": "string", "description": "Negative net name" },
                    "layer":    { "type": "string", "default": "F.Cu" },
                    "x1": { "type": "number" }, "y1": { "type": "number" },
                    "x2": { "type": "number" }, "y2": { "type": "number" },
                    "width": { "type": "number", "default": 0.1 },
                    "gap":   { "type": "number", "description": "Gap between pair traces in mm", "default": 0.1 }
                },
                "required": ["board", "net_pos", "net_neg", "x1", "y1", "x2", "y2"]
            }),
            |args, ctx| async move { handle_route_diff_pair(args, ctx).await }
        ),
    ]
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

async fn handle_add_net(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let board_path = get_path(args, "board")?;
    let net_name = match require_str(args, "net_name") {
        Ok(v) => v.to_string(),
        Err(e) => return Ok(e),
    };

    let content = std::fs::read_to_string(&board_path)?;
    let tree = konnect_sexp::parser::parse_sexp(&content)?;
    if !konnect_sexp::net::board_uses_net_table(&tree) {
        return Ok(CallToolResult::error_kind(
            ToolErrorKind::MalformedDocument {
                path: board_path.display().to_string(),
                detail: "this board has no (net <id> …) table (KiCAD 20260206+ writes net \
                    names directly on items); a net cannot be added by inserting a table \
                    entry here — create it by connecting an item to that net name instead"
                    .to_string(),
            },
            "Board has no net table; add_net only works on boards with a legacy net table.",
        ));
    }
    let net_id = konnect_sexp::net::next_net_id(&tree);
    let net_sexp = format!("\n  (net {net_id} \"{net_name}\")");
    // Insert before the last closing paren
    let close_pos = content.rfind(')').unwrap_or(content.len());
    let new_content = apply_edits(content, vec![SexpEdit::insert(close_pos, net_sexp)]);
    write_atomic(&board_path, &new_content)?;

    Ok(CallToolResult::json(
        &json!({ "net_id": net_id, "net_name": net_name }),
    ))
}

async fn handle_route_trace(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let net_name = match require_str(args, "net_name") {
        Ok(v) => v.to_string(),
        Err(e) => return Ok(e),
    };
    let layer = match require_str(args, "layer") {
        Ok(v) => v.to_string(),
        Err(e) => return Ok(e),
    };
    let x1 = match require_f64(args, "x1") {
        Ok(v) => v,
        Err(e) => return Ok(e),
    };
    let y1 = match require_f64(args, "y1") {
        Ok(v) => v,
        Err(e) => return Ok(e),
    };
    let x2 = match require_f64(args, "x2") {
        Ok(v) => v,
        Err(e) => return Ok(e),
    };
    let y2 = match require_f64(args, "y2") {
        Ok(v) => v,
        Err(e) => return Ok(e),
    };
    let width = args["width"].as_f64().unwrap_or(0.25);

    let net_ipc = net_name.clone();
    let layer_ipc = layer.clone();
    ipc!(ctx, |c| c
        .add_track(&net_ipc, &layer_ipc, width, x1, y1, x2, y2));
    Ok(CallToolResult::json(&json!({
        "net": net_name, "layer": layer, "width": width,
        "from": { "x": x1, "y": y1 }, "to": { "x": x2, "y": y2 }
    })))
}

async fn handle_route_pad_to_pad(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let board_path = get_path(args, "board")?;
    let net_name = match require_str(args, "net_name") {
        Ok(v) => v.to_string(),
        Err(e) => return Ok(e),
    };
    let ref1 = match require_str(args, "ref1") {
        Ok(v) => v.to_string(),
        Err(e) => return Ok(e),
    };
    let pad1 = match require_str(args, "pad1") {
        Ok(v) => v.to_string(),
        Err(e) => return Ok(e),
    };
    let ref2 = match require_str(args, "ref2") {
        Ok(v) => v.to_string(),
        Err(e) => return Ok(e),
    };
    let pad2 = match require_str(args, "pad2") {
        Ok(v) => v.to_string(),
        Err(e) => return Ok(e),
    };
    let layer = args["layer"].as_str().unwrap_or("F.Cu").to_string();
    let width = args["width"].as_f64().unwrap_or(0.25);

    // Look up pad positions from the PCB S-expression file
    let content = std::fs::read_to_string(&board_path)?;
    let tree = konnect_sexp::parser::parse_sexp(&content)?;

    let pos1 = find_pad_board_position(&tree, &ref1, &pad1)?;
    let pos2 = find_pad_board_position(&tree, &ref2, &pad2)?;

    // Route an L-bend: horizontal first, then vertical
    let (x1, y1) = pos1;
    let (x2, y2) = pos2;
    let net_ipc = net_name.clone();
    let layer_ipc = layer.clone();

    if (x1 - x2).abs() < 0.01 || (y1 - y2).abs() < 0.01 {
        // Already axis-aligned: single segment
        ipc!(ctx, |c| c
            .add_track(&net_ipc, &layer_ipc, width, x1, y1, x2, y2));
    } else {
        // L-bend: horizontal then vertical
        let mid_x = x2;
        let mid_y = y1;
        let net_a = net_name.clone();
        let net_b = net_name.clone();
        let layer_a = layer.clone();
        let layer_b = layer.clone();
        ipc!(ctx, |c| {
            c.add_tracks(&[
                konnect_ipc::TrackSpec {
                    net_name: net_a.clone(),
                    layer: layer_a.clone(),
                    width,
                    x1,
                    y1,
                    x2: mid_x,
                    y2: mid_y,
                },
                konnect_ipc::TrackSpec {
                    net_name: net_b.clone(),
                    layer: layer_b.clone(),
                    width,
                    x1: mid_x,
                    y1: mid_y,
                    x2,
                    y2,
                },
            ])
        });
    }

    Ok(CallToolResult::json(&json!({
        "routed": true,
        "net": net_name, "layer": layer, "width": width,
        "from": { "ref": ref1, "pad": pad1, "x": x1, "y": y1 },
        "to":   { "ref": ref2, "pad": pad2, "x": x2, "y": y2 }
    })))
}

/// Look up a pad's board-space (x, y) position from the parsed PCB S-expression tree.
fn find_pad_board_position(
    tree: &konnect_sexp::parser::SexpNode,
    reference: &str,
    pad_number: &str,
) -> anyhow::Result<(f64, f64)> {
    let fp_node = tree
        .find_all("footprint")
        .into_iter()
        .find(|fp| {
            fp.find_all("property").iter().any(|p| {
                p.get(1).and_then(|n| n.as_str()) == Some("Reference")
                    && p.get(2).and_then(|n| n.as_str()) == Some(reference)
            })
        })
        .ok_or_else(|| anyhow::anyhow!("Footprint '{}' not found on board", reference))?;

    let fp_at = fp_node.find("at");
    let fp_x = fp_at.and_then(|a| a.get_f64(1)).unwrap_or(0.0);
    let fp_y = fp_at.and_then(|a| a.get_f64(2)).unwrap_or(0.0);
    let fp_rot = fp_at.and_then(|a| a.get_f64(3)).unwrap_or(0.0);

    let pad = fp_node
        .find_all("pad")
        .into_iter()
        .find(|p| p.get(1).and_then(|n| n.as_str()) == Some(pad_number))
        .ok_or_else(|| anyhow::anyhow!("Pad '{}' not found on '{}'", pad_number, reference))?;

    let pad_at = pad
        .find("at")
        .ok_or_else(|| anyhow::anyhow!("Pad has no (at) node"))?;
    let local_x = pad_at.get_f64(1).unwrap_or(0.0);
    let local_y = pad_at.get_f64(2).unwrap_or(0.0);

    // Transform local pad coords to board space (rotation only).
    // Uses the canonical KiCAD transform — see konnect_sexp::geometry.
    Ok(konnect_sexp::geometry::transform_pad(
        local_x, local_y, fp_x, fp_y, fp_rot,
    ))
}

async fn handle_add_via(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let net_name = match require_str(args, "net_name") {
        Ok(v) => v.to_string(),
        Err(e) => return Ok(e),
    };
    let x = match require_f64(args, "x") {
        Ok(v) => v,
        Err(e) => return Ok(e),
    };
    let y = match require_f64(args, "y") {
        Ok(v) => v,
        Err(e) => return Ok(e),
    };
    let drill = args["drill"].as_f64().unwrap_or(0.4);
    let pad_size = args["pad_size"].as_f64().unwrap_or(0.8);

    let net_ipc = net_name.clone();
    ipc!(ctx, |c| c.add_via(&net_ipc, x, y, drill, pad_size));
    Ok(CallToolResult::json(
        &json!({ "net": net_name, "x": x, "y": y, "drill": drill, "pad_size": pad_size }),
    ))
}

async fn handle_add_copper_pour(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let board_path = get_path(args, "board")?;
    let net_name = match require_str(args, "net_name") {
        Ok(v) => v.to_string(),
        Err(e) => return Ok(e),
    };
    let layer = match require_str(args, "layer") {
        Ok(v) => v.to_string(),
        Err(e) => return Ok(e),
    };
    let clearance = args["clearance"].as_f64().unwrap_or(0.2);
    let min_w = args["min_width"].as_f64().unwrap_or(0.25);
    let pts_arr = match args["points"].as_array() {
        Some(a) => a.clone(),
        None => {
            return Ok(CallToolResult::error_kind(
                ToolErrorKind::InvalidArgument {
                    field: "points".to_string(),
                    reason: "must be an array".to_string(),
                },
                "Missing 'points' array",
            ))
        }
    };

    let pts: Vec<(f64, f64)> = pts_arr
        .iter()
        .filter_map(|p| Some((p["x"].as_f64()?, p["y"].as_f64()?)))
        .collect();
    if pts.len() < 3 {
        return Ok(CallToolResult::error_kind(
            ToolErrorKind::InvalidArgument {
                field: "points".to_string(),
                reason: "a zone outline needs at least 3 points".to_string(),
            },
            "Zone requires at least 3 points",
        ));
    }

    let content = std::fs::read_to_string(&board_path)?;
    let net_id = find_net_id(&content, &net_name);
    let zone_s = format_zone(net_id, &net_name, &layer, clearance, min_w, &pts);
    let close = content.rfind(')').unwrap_or(content.len());
    let new_content = apply_edits(content, vec![SexpEdit::insert(close, zone_s)]);
    write_atomic(&board_path, &new_content)?;

    Ok(CallToolResult::json(
        &json!({ "net": net_name, "layer": layer, "points": pts.len() }),
    ))
}

async fn handle_delete_trace(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let uuid = match require_str(args, "uuid") {
        Ok(v) => v.to_string(),
        Err(e) => return Ok(e),
    };

    let uuid_ipc = uuid.clone();
    ipc!(ctx, |c| c.delete_track(&uuid_ipc));
    Ok(CallToolResult::json(&json!({ "deleted_uuid": uuid })))
}

async fn handle_query_traces(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let net = args["net_name"].as_str().map(String::from);
    let layer = args["layer"].as_str().map(String::from);

    let tracks = ipc!(ctx, |c| { c.get_tracks(net.as_deref(), layer.as_deref()) });

    let items: Vec<serde_json::Value> = tracks
        .iter()
        .map(|t| {
            json!({
                "net": t.net_name, "layer": t.layer, "width": t.width,
                "x1": t.start.x, "y1": t.start.y,
                "x2": t.end.x,   "y2": t.end.y
            })
        })
        .collect();

    Ok(CallToolResult::json(
        &json!({ "count": items.len(), "traces": items }),
    ))
}

async fn handle_get_nets_list(
    _args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let nets = ipc!(ctx, |c| c.get_nets());
    let items: Vec<serde_json::Value> = nets
        .iter()
        .map(|n| json!({ "name": n.name, "netcode": n.netcode }))
        .collect();
    Ok(CallToolResult::json(
        &json!({ "count": items.len(), "nets": items }),
    ))
}

async fn handle_modify_trace(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let uuid = match require_str(args, "uuid") {
        Ok(v) => v.to_string(),
        Err(e) => return Ok(e),
    };
    let net_name = match require_str(args, "net_name") {
        Ok(v) => v.to_string(),
        Err(e) => return Ok(e),
    };
    let layer = match require_str(args, "layer") {
        Ok(v) => v.to_string(),
        Err(e) => return Ok(e),
    };
    let x1 = match require_f64(args, "x1") {
        Ok(v) => v,
        Err(e) => return Ok(e),
    };
    let y1 = match require_f64(args, "y1") {
        Ok(v) => v,
        Err(e) => return Ok(e),
    };
    let x2 = match require_f64(args, "x2") {
        Ok(v) => v,
        Err(e) => return Ok(e),
    };
    let y2 = match require_f64(args, "y2") {
        Ok(v) => v,
        Err(e) => return Ok(e),
    };
    let width = args["width"].as_f64().unwrap_or(0.25);

    let uuid_ipc = uuid.clone();
    let net_ipc = net_name.clone();
    let layer_ipc = layer.clone();
    ipc!(ctx, |c| c.replace_track(
        &uuid_ipc,
        &konnect_ipc::TrackSpec {
            net_name: net_ipc.clone(),
            layer: layer_ipc.clone(),
            width,
            x1,
            y1,
            x2,
            y2,
        }
    ));
    Ok(CallToolResult::json(&json!({
        "modified_uuid": uuid,
        "net": net_name, "layer": layer, "width": width,
        "from": { "x": x1, "y": y1 }, "to": { "x": x2, "y": y2 }
    })))
}

/// The sibling `<project>.kicad_pro`, which is where KiCad ≥ 7 keeps net
/// classes. The board file has no netclass container at all — the previous
/// handler inserted `(netclass …)` as a direct child of `(kicad_pcb`, a token
/// pcbnew's parser rejects, so the board no longer loaded.
fn project_settings_path(board_path: &std::path::Path) -> std::path::PathBuf {
    board_path.with_extension("kicad_pro")
}

/// Load the project JSON, refusing — rather than inventing a file KiCad never
/// reads — when it is absent. A netclass with nowhere KiCad looks for it
/// would be the same silent no-op this handler replaces.
fn load_project_settings(
    board_path: &std::path::Path,
) -> anyhow::Result<Result<(std::path::PathBuf, serde_json::Value), CallToolResult>> {
    let pro = project_settings_path(board_path);
    if !pro.exists() {
        return Ok(Err(CallToolResult::error_kind(
            ToolErrorKind::FileNotFound {
                path: pro.display().to_string(),
            },
            format!(
                "No project file at {} — net classes live in the .kicad_pro since KiCad 7, \
                 and a class written anywhere else is never read. Create the project \
                 (KiCad: File > Save a Copy, or place the board inside a project) and retry.",
                pro.display()
            ),
        )));
    }
    let settings: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&pro)?)
        .map_err(|e| anyhow::anyhow!("{} is not valid JSON: {e}", pro.display()))?;
    Ok(Ok((pro, settings)))
}

/// KiCad's own writer emits 2-space-indented JSON; `serde_json`'s pretty
/// printer matches, so the diff on a round trip stays minimal. Keys not
/// touched here (`text_variables`, `sheets`, `pcbnew`, …) pass through
/// unchanged because `settings` is the whole document read back, edited
/// in place, and re-serialised — nothing is reconstructed from scratch.
fn save_project_settings(
    pro: &std::path::Path,
    settings: &serde_json::Value,
) -> anyhow::Result<()> {
    write_atomic(
        pro,
        &format!("{}\n", serde_json::to_string_pretty(settings)?),
    )?;
    Ok(())
}

/// KiCad's `net_settings.classes[]` field name, this tool's argument name,
/// and the value a *new* class takes when the caller says nothing. The
/// defaults apply to creation only — folding them into an update turned
/// "widen HV's track" into a silent reset of whatever the caller had already
/// tuned and did not name this time (upstream #220).
const NETCLASS_FIELDS: [(&str, &str, f64); 4] = [
    ("clearance", "clearance", 0.2),
    ("track_width", "trace_width", 0.25),
    ("via_drill", "via_drill", 0.4),
    ("via_diameter", "via_diameter", 0.8),
];

async fn handle_create_netclass(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let board_path = get_path(args, "board")?;
    let name = match require_str(args, "name") {
        Ok(v) => v.to_string(),
        Err(e) => return Ok(e),
    };

    let (pro, mut settings) = match load_project_settings(&board_path)? {
        Ok(v) => v,
        Err(refusal) => return Ok(refusal),
    };

    let top = settings
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("{}: top level is not a JSON object", pro.display()))?;
    let net_settings = top.entry("net_settings").or_insert_with(
        || json!({ "classes": [], "meta": { "version": 4 }, "netclass_patterns": [] }),
    );
    let classes = net_settings
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("{}: net_settings is not an object", pro.display()))?
        .entry("classes")
        .or_insert_with(|| json!([]));
    let classes = classes.as_array_mut().ok_or_else(|| {
        anyhow::anyhow!("{}: net_settings.classes is not an array", pro.display())
    })?;

    // KiCad keys classes by name; a second entry with the same name is
    // undefined in its dialog, so an existing class is updated in place, and
    // only the fields the caller actually named move.
    let mut changed = true;
    let updated = if let Some(class) = classes.iter_mut().find(|c| c["name"] == json!(name)) {
        let before = class.clone();
        for (key, arg, _) in NETCLASS_FIELDS {
            if let Some(value) = opt_f64(args, arg) {
                class[key] = json!(value);
            }
        }
        changed = *class != before;
        true
    } else {
        let mut class = json!({ "name": name, "priority": 0 });
        for (key, arg, default) in NETCLASS_FIELDS {
            class[key] = json!(opt_f64(args, arg).unwrap_or(default));
        }
        classes.push(class);
        false
    };

    // Report the class as it now stands, not the arguments that came in — on
    // an update most fields were never named by this call, and echoing the
    // arguments back is what made the #220 reset invisible.
    let stored = classes
        .iter()
        .find(|c| c["name"] == json!(name))
        .cloned()
        .unwrap_or_else(|| json!({}));

    // Naming no value at all, or the values already held, decides nothing.
    // `save_project_settings` re-serialises the whole document rather than
    // patching it, so saving anyway would rewrite the project file for a
    // call that is, in effect, a read.
    if changed {
        save_project_settings(&pro, &settings)?;
    }

    Ok(CallToolResult::json(&json!({
        "created_netclass": name,
        "updated_existing": updated,
        "clearance": stored["clearance"], "trace_width": stored["track_width"],
        "via_drill": stored["via_drill"], "via_diameter": stored["via_diameter"],
        "file": pro.display().to_string(),
        "note": "Netclasses live in the project file; assign nets with assign_net_to_class. \
                 KiCad reads the change on next project open."
    })))
}

async fn handle_assign_net_to_class(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let board_path = get_path(args, "board")?;
    let net_name = match require_str(args, "net_name") {
        Ok(v) => v.to_string(),
        Err(e) => return Ok(e),
    };
    let netclass = match require_str(args, "netclass") {
        Ok(v) => v.to_string(),
        Err(e) => return Ok(e),
    };

    let (pro, mut settings) = match load_project_settings(&board_path)? {
        Ok(v) => v,
        Err(refusal) => return Ok(refusal),
    };

    // The class must exist — a pattern naming an unknown class is silently
    // ignored by KiCad, which is exactly the failure shape this tool must not
    // reintroduce under a different name.
    let known: Vec<String> = settings["net_settings"]["classes"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|c| c["name"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    if !known.iter().any(|n| n == &netclass) {
        return Ok(CallToolResult::error_kind(
            ToolErrorKind::NotFound {
                document: pro.display().to_string(),
                item_kind: "netclass".to_string(),
                key: netclass.clone(),
                candidates: known,
            },
            format!(
                "Netclass '{}' not found in {} — create it with create_netclass.",
                netclass,
                pro.display()
            ),
        ));
    }

    // Membership is a netclass_patterns entry; the exact net name is a valid
    // pattern. One pattern maps to one class, so a re-assignment moves the
    // entry rather than adding a competing one.
    let patterns = settings["net_settings"]
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("{}: net_settings is not an object", pro.display()))?
        .entry("netclass_patterns")
        .or_insert_with(|| json!([]));
    let patterns = patterns.as_array_mut().ok_or_else(|| {
        anyhow::anyhow!(
            "{}: net_settings.netclass_patterns is not an array",
            pro.display()
        )
    })?;

    let mut previous_class: Option<String> = None;
    if let Some(entry) = patterns
        .iter_mut()
        .find(|p| p["pattern"] == json!(net_name))
    {
        if entry["netclass"] == json!(netclass) {
            return Ok(CallToolResult::json(&json!({
                "already_assigned": true,
                "net_name": net_name,
                "netclass": netclass,
                "file": pro.display().to_string()
            })));
        }
        previous_class = entry["netclass"].as_str().map(String::from);
        entry["netclass"] = json!(netclass);
    } else {
        patterns.push(json!({ "netclass": netclass, "pattern": net_name }));
    }
    save_project_settings(&pro, &settings)?;

    Ok(CallToolResult::json(&json!({
        "assigned": true,
        "net_name": net_name,
        "netclass": netclass,
        "previous_class": previous_class,
        "file": pro.display().to_string()
    })))
}

async fn handle_route_diff_pair(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let net_pos = match require_str(args, "net_pos") {
        Ok(v) => v.to_string(),
        Err(e) => return Ok(e),
    };
    let net_neg = match require_str(args, "net_neg") {
        Ok(v) => v.to_string(),
        Err(e) => return Ok(e),
    };
    let layer = args["layer"].as_str().unwrap_or("F.Cu").to_string();
    let x1 = match require_f64(args, "x1") {
        Ok(v) => v,
        Err(e) => return Ok(e),
    };
    let y1 = match require_f64(args, "y1") {
        Ok(v) => v,
        Err(e) => return Ok(e),
    };
    let x2 = match require_f64(args, "x2") {
        Ok(v) => v,
        Err(e) => return Ok(e),
    };
    let y2 = match require_f64(args, "y2") {
        Ok(v) => v,
        Err(e) => return Ok(e),
    };
    let width = args["width"].as_f64().unwrap_or(0.1);
    let gap = args["gap"].as_f64().unwrap_or(0.1);
    let offset = (gap + width) / 2.0;

    // Route two parallel traces offset perpendicular to the direction
    let dx = x2 - x1;
    let dy = y2 - y1;
    let len = (dx * dx + dy * dy).sqrt().max(1e-9);
    let perp_x = -dy / len * offset;
    let perp_y = dx / len * offset;

    let np_ipc = net_pos.clone();
    let nn_ipc = net_neg.clone();
    let layer_ipc = layer.clone();
    ipc!(ctx, |c| {
        c.add_tracks(&[
            konnect_ipc::TrackSpec {
                net_name: np_ipc.clone(),
                layer: layer_ipc.clone(),
                width,
                x1: x1 + perp_x,
                y1: y1 + perp_y,
                x2: x2 + perp_x,
                y2: y2 + perp_y,
            },
            konnect_ipc::TrackSpec {
                net_name: nn_ipc.clone(),
                layer: layer_ipc.clone(),
                width,
                x1: x1 - perp_x,
                y1: y1 - perp_y,
                x2: x2 - perp_x,
                y2: y2 - perp_y,
            },
        ])
    });

    Ok(CallToolResult::json(&json!({
        "net_pos": net_pos, "net_neg": net_neg,
        "layer": layer, "width": width, "gap": gap
    })))
}

/// Netclasses live in `<project>.kicad_pro` since KiCad 7, not the board. The
/// previous handler inserted a `(netclass …)` node into the `.kicad_pcb` — as
/// a direct child of `(kicad_pcb`, a token pcbnew's parser rejects outright,
/// so a real board no longer loaded.
#[cfg(test)]
mod netclass_tests {
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

    const BOARD: &str = "(kicad_pcb\n\t(version 20250610)\n\t(generator \"pcbnew\")\n)\n";

    /// A board plus, optionally, the sibling `.kicad_pro` KiCad writes.
    fn fixture(with_project: bool) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let board = dir.path().join("demo.kicad_pcb");
        std::fs::write(&board, BOARD).unwrap();
        if with_project {
            std::fs::write(
                dir.path().join("demo.kicad_pro"),
                serde_json::to_string_pretty(&json!({
                    "board": { "design_settings": {} },
                    "meta": { "filename": "demo.kicad_pro", "version": 3 }
                }))
                .unwrap(),
            )
            .unwrap();
        }
        (dir, board)
    }

    fn text_of(r: &CallToolResult) -> String {
        match r.content.first() {
            Some(crate::mcp::protocol::ToolContent::Text { text }) => text.clone(),
            other => panic!("expected text, got {other:?}"),
        }
    }

    fn project_json(board: &std::path::Path) -> serde_json::Value {
        let pro = board.with_extension("kicad_pro");
        serde_json::from_str(&std::fs::read_to_string(pro).unwrap()).unwrap()
    }

    async fn create(board: &std::path::Path, args: serde_json::Value) -> CallToolResult {
        let mut args = args;
        args["board"] = json!(board.to_str().unwrap());
        handle_create_netclass(&args, &test_ctx()).await.unwrap()
    }

    async fn assign(board: &std::path::Path, net: &str, class: &str) -> CallToolResult {
        handle_assign_net_to_class(
            &json!({ "board": board.to_str().unwrap(), "net_name": net, "netclass": class }),
            &test_ctx(),
        )
        .await
        .unwrap()
    }

    /// The board file is data pcbnew refuses if a netclass node lands in it;
    /// the class must go into the project file instead, and the board must
    /// not change by a single byte.
    #[tokio::test]
    async fn create_netclass_writes_the_project_file_and_leaves_the_board_alone() {
        let (_dir, board) = fixture(true);
        let result = create(
            &board,
            json!({ "name": "HV", "clearance": 0.5, "trace_width": 0.3 }),
        )
        .await;
        assert!(!result.is_error, "{}", text_of(&result));
        assert_eq!(std::fs::read_to_string(&board).unwrap(), BOARD);

        let pro = project_json(&board);
        let classes = pro["net_settings"]["classes"].as_array().unwrap();
        let hv = classes
            .iter()
            .find(|c| c["name"] == "HV")
            .expect("HV class in net_settings.classes");
        assert_eq!(hv["clearance"], json!(0.5));
        assert_eq!(hv["track_width"], json!(0.3));
        assert_eq!(hv["via_diameter"], json!(0.8));
        assert_eq!(hv["via_drill"], json!(0.4));
        // The existing project content survives the edit.
        assert_eq!(pro["meta"]["filename"], json!("demo.kicad_pro"));
    }

    /// No project file means nowhere KiCad would ever read the class from;
    /// inventing one risks orphan settings, so the tool refuses instead of
    /// writing anything.
    #[tokio::test]
    async fn create_netclass_without_a_project_file_refuses_and_writes_nothing() {
        let (dir, board) = fixture(false);
        let result = create(&board, json!({ "name": "HV" })).await;
        assert!(result.is_error, "{}", text_of(&result));
        assert!(
            text_of(&result).contains("kicad_pro"),
            "{}",
            text_of(&result)
        );
        assert_eq!(std::fs::read_to_string(&board).unwrap(), BOARD);
        assert!(!dir.path().join("demo.kicad_pro").exists());
    }

    /// Same name twice updates in place — KiCad keys classes by name and two
    /// entries with one name is undefined behaviour in its dialog.
    #[tokio::test]
    async fn create_netclass_updates_an_existing_class_in_place() {
        let (_dir, board) = fixture(true);
        create(&board, json!({ "name": "HV", "clearance": 0.3 })).await;
        let second = create(&board, json!({ "name": "HV", "clearance": 0.6 })).await;
        assert!(!second.is_error, "{}", text_of(&second));

        let pro = project_json(&board);
        let classes = pro["net_settings"]["classes"].as_array().unwrap();
        assert_eq!(
            classes.iter().filter(|c| c["name"] == "HV").count(),
            1,
            "{classes:?}"
        );
        assert_eq!(classes[0]["clearance"], json!(0.6));
    }

    /// Re-running the tool is how a caller adjusts one setting of a class it
    /// already tuned. Every argument carries a schema default, so applying
    /// those defaults on an update would silently reset the three settings
    /// the caller did not name this time — the clearance a board was routed
    /// to, gone on a call that only meant to widen a track (#220).
    #[tokio::test]
    async fn create_netclass_leaves_settings_the_caller_did_not_name_alone() {
        let (_dir, board) = fixture(true);
        create(
            &board,
            json!({ "name": "HV", "clearance": 1.5, "trace_width": 0.5,
                    "via_drill": 0.45, "via_diameter": 0.85 }),
        )
        .await;
        let second = create(&board, json!({ "name": "HV", "trace_width": 0.9 })).await;
        assert!(!second.is_error, "{}", text_of(&second));

        let pro = project_json(&board);
        let hv = pro["net_settings"]["classes"][0].clone();
        assert_eq!(hv["track_width"], json!(0.9), "the named value changes");
        assert_eq!(hv["clearance"], json!(1.5), "{hv}");
        assert_eq!(hv["via_drill"], json!(0.45), "{hv}");
        assert_eq!(hv["via_diameter"], json!(0.85), "{hv}");

        // The result echoes the stored class, not the one argument passed.
        let echoed: serde_json::Value = serde_json::from_str(&text_of(&second)).unwrap();
        assert_eq!(echoed["clearance"], json!(1.5));
        assert_eq!(echoed["trace_width"], json!(0.9));
    }

    /// A new class still gets the documented defaults for whatever the caller
    /// leaves out — the fix above must not turn creation into a partial class.
    #[tokio::test]
    async fn a_new_class_is_still_created_with_the_documented_defaults() {
        let (_dir, board) = fixture(true);
        create(&board, json!({ "name": "HV" })).await;

        let hv = project_json(&board)["net_settings"]["classes"][0].clone();
        assert_eq!(hv["clearance"], json!(0.2), "{hv}");
        assert_eq!(hv["track_width"], json!(0.25), "{hv}");
        assert_eq!(hv["via_drill"], json!(0.4), "{hv}");
        assert_eq!(hv["via_diameter"], json!(0.8), "{hv}");
    }

    /// Naming no value at all, or the values already held, decides nothing —
    /// so it must not write. `save_project_settings` re-serialises the whole
    /// document rather than patching it, so saving anyway rewrites every line
    /// of the project file for a call that is, in effect, a read.
    #[tokio::test]
    async fn a_call_that_changes_nothing_leaves_the_project_file_untouched() {
        let (_dir, board) = fixture(true);
        create(&board, json!({ "name": "HV", "clearance": 1.5 })).await;

        // Re-written by hand in a shape the serialiser would not produce, so
        // any save at all is visible in the bytes.
        let pro = board.with_extension("kicad_pro");
        let compact = serde_json::to_string(
            &serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(&pro).unwrap())
                .unwrap(),
        )
        .unwrap();
        std::fs::write(&pro, &compact).unwrap();

        // Naming no value at all: a read.
        let result = create(&board, json!({ "name": "HV" })).await;
        assert!(!result.is_error, "{}", text_of(&result));
        assert_eq!(std::fs::read_to_string(&pro).unwrap(), compact);
        let echoed: serde_json::Value = serde_json::from_str(&text_of(&result)).unwrap();
        assert_eq!(echoed["clearance"], json!(1.5));
        assert_eq!(echoed["updated_existing"], json!(true));

        // Naming the values it already holds: also nothing to decide.
        create(&board, json!({ "name": "HV", "clearance": 1.5 })).await;
        assert_eq!(std::fs::read_to_string(&pro).unwrap(), compact);

        // A real change still writes.
        create(&board, json!({ "name": "HV", "clearance": 0.9 })).await;
        assert_ne!(std::fs::read_to_string(&pro).unwrap(), compact);
    }

    /// Membership is a netclass_patterns entry keyed by the exact net name.
    #[tokio::test]
    async fn assign_net_adds_a_pattern_once_and_can_move_it() {
        let (_dir, board) = fixture(true);
        create(&board, json!({ "name": "HV" })).await;
        create(&board, json!({ "name": "LV" })).await;

        let first = assign(&board, "GND", "HV").await;
        assert!(!first.is_error, "{}", text_of(&first));
        let pro = project_json(&board);
        let patterns = pro["net_settings"]["netclass_patterns"].as_array().unwrap();
        assert_eq!(patterns.len(), 1);
        assert_eq!(patterns[0]["netclass"], json!("HV"));
        assert_eq!(patterns[0]["pattern"], json!("GND"));

        // Idempotent.
        let again = assign(&board, "GND", "HV").await;
        let body: serde_json::Value = serde_json::from_str(&text_of(&again)).unwrap();
        assert_eq!(body["already_assigned"], json!(true));
        let pro = project_json(&board);
        assert_eq!(
            pro["net_settings"]["netclass_patterns"]
                .as_array()
                .unwrap()
                .len(),
            1
        );

        // Reassigning moves the one entry rather than adding a second.
        let moved = assign(&board, "GND", "LV").await;
        let body: serde_json::Value = serde_json::from_str(&text_of(&moved)).unwrap();
        assert_eq!(body["previous_class"], json!("HV"), "{body}");
        let pro = project_json(&board);
        let patterns = pro["net_settings"]["netclass_patterns"].as_array().unwrap();
        assert_eq!(patterns.len(), 1);
        assert_eq!(patterns[0]["netclass"], json!("LV"));

        assert_eq!(std::fs::read_to_string(&board).unwrap(), BOARD);
    }

    /// Assigning to a class that doesn't exist names the ones that do.
    #[tokio::test]
    async fn assign_net_to_a_missing_class_errors_naming_the_available_ones() {
        let (_dir, board) = fixture(true);
        create(&board, json!({ "name": "HV" })).await;
        let result = assign(&board, "GND", "NOPE").await;
        assert!(result.is_error);
        let msg = text_of(&result);
        assert!(msg.contains("HV"), "{msg}");
        assert_eq!(std::fs::read_to_string(&board).unwrap(), BOARD);
    }

    async fn add_net(board: &std::path::Path, name: &str) -> CallToolResult {
        handle_add_net(
            &json!({ "board": board.to_str().unwrap(), "net_name": name }),
            &test_ctx(),
        )
        .await
        .unwrap()
    }

    /// A board written in KiCAD 20260206's form has no `(net <id> …)` table
    /// at all — `add_net` cannot insert a table entry there, and must say why
    /// rather than silently produce an id nothing reads (upstream #142).
    #[tokio::test]
    async fn add_net_refuses_a_board_with_no_net_table() {
        let (_dir, board) = fixture(false); // BOARD has no (net …) table
        let result = add_net(&board, "GND").await;
        assert!(result.is_error);
        let msg = text_of(&result);
        assert!(msg.contains("net table"), "{msg}");
        assert_eq!(std::fs::read_to_string(&board).unwrap(), BOARD);
    }

    /// On a board that does carry a net table, the new id must not collide
    /// with an existing one, including across a gap.
    #[tokio::test]
    async fn add_net_assigns_a_non_colliding_id() {
        let dir = tempfile::tempdir().unwrap();
        let board = dir.path().join("demo.kicad_pcb");
        std::fs::write(
            &board,
            "(kicad_pcb\n\t(version 20250610)\n\t(net 0 \"\")\n\t(net 1 \"GND\")\n\t(net 5 \"VCC\")\n)\n",
        )
        .unwrap();

        let result = add_net(&board, "CLK").await;
        assert!(!result.is_error, "{}", text_of(&result));
        let body: serde_json::Value = serde_json::from_str(&text_of(&result)).unwrap();
        assert_eq!(body["net_id"], json!(6));

        let written = std::fs::read_to_string(&board).unwrap();
        assert!(written.contains("(net 6 \"CLK\")"), "{written}");
    }
}
