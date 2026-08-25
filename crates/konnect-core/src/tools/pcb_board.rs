//! `pcb_board` toolset — board setup, layers, outlines, zones, and board-level items.
//!
//! Most operations use S-expression file manipulation so they work without a running
//! KiCAD instance. `get_board_extents` tries the IPC API first, falling back to
//! parsing the file for coordinate bounds.

use crate::mcp::error::ToolErrorKind;
use crate::mcp::protocol::CallToolResult;
use crate::tool;
use crate::tools::ipc_boundary::{ipc_error_result, with_ipc};
use crate::tools::{get_path, require_f64, require_str, ToolContext, ToolDef};
use konnect_ipc::builders;
use konnect_sexp::{
    parser::parse_sexp,
    writer::{apply_edits, new_uuid, write_atomic, SexpEdit},
};
use serde_json::json;

// Build the 4 Edge.Cuts segments forming a rectangle, packed as Any for create_items.
fn rect_outline_items(x1: f64, y1: f64, x2: f64, y2: f64, w: f64) -> Vec<prost_types::Any> {
    let sides = [
        (x1, y1, x2, y1),
        (x2, y1, x2, y2),
        (x2, y2, x1, y2),
        (x1, y2, x1, y1),
    ];
    sides
        .iter()
        .map(|&(a, b, c, d)| {
            builders::pack_any(
                &builders::board_segment("Edge.Cuts", w, a, b, c, d),
                "kiapi.board.types.BoardGraphicShape",
            )
        })
        .collect()
}

// ─── S-expression format helpers ──────────────────────────────────────────────

fn format_gr_line(x1: f64, y1: f64, x2: f64, y2: f64, layer: &str, width: f64) -> String {
    let uuid = new_uuid();
    format!(
        "\n  (gr_line\n    (start {x1} {y1})\n    (end {x2} {y2})\n    \
         (stroke (width {width}) (type solid))\n    (layer \"{layer}\")\n    (uuid \"{uuid}\")\n  )"
    )
}

fn format_gr_text(text: &str, x: f64, y: f64, rot: f64, layer: &str, size: f64) -> String {
    let uuid = new_uuid();
    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        "\n  (gr_text \"{escaped}\"\n    (at {x} {y} {rot})\n    (layer \"{layer}\")\n    \
         (effects (font (size {size} {size}) (thickness 0.15)))\n    (uuid \"{uuid}\")\n  )"
    )
}

fn format_npth_footprint(x: f64, y: f64, drill_d: f64, reference: &str) -> String {
    let fp_uuid = new_uuid();
    let ref_uuid = new_uuid();
    let val_uuid = new_uuid();
    let pad_uuid = new_uuid();
    let pad_size = drill_d + 0.5;
    format!(
        "\n  (footprint \"MountingHole:MountingHole_{drill_d:.1}mm\"\n    \
         (layer \"F.Cu\")\n    (at {x} {y})\n    \
         (attr exclude_from_pos_files)\n    \
         (property \"Reference\" \"{reference}\"\n      (at 0 {offset} 0)\n      (layer \"F.SilkS\")\n      (uuid \"{ref_uuid}\")\n    )\n    \
         (property \"Value\" \"MountingHole\"\n      (at 0 -{offset} 0)\n      (layer \"F.Fab\")\n      (uuid \"{val_uuid}\")\n    )\n    \
         (pad \"\" np_thru_hole circle (at 0 0) (size {pad_size} {pad_size})\n      \
         (drill {drill_d})\n      (layers \"*.Cu\" \"*.Mask\")\n      (uuid \"{pad_uuid}\")\n    )\n    \
         (uuid \"{fp_uuid}\")\n  )",
        offset = drill_d + 1.5
    )
}

/// A zone, with its net reference written in whichever form the board itself
/// uses — see [`konnect_sexp::net::NetRef::zone_tokens`].
fn format_zone_polygon(
    net_tokens: &str,
    layer: &str,
    clearance: f64,
    min_width: f64,
    points: &[(f64, f64)],
) -> String {
    let uuid = new_uuid();
    let pts: String = points
        .iter()
        .map(|(x, y)| format!("\n      (xy {x} {y})"))
        .collect();
    format!(
        "\n  (zone {net_tokens} (layer \"{layer}\") (uuid \"{uuid}\")\n    \
         (hatch edge 0.508)\n    (connect_pads (clearance {clearance}))\n    \
         (min_thickness {min_width})\n    (fill yes (thermal_gap 0.5) (thermal_bridge_width 0.5))\n    \
         (polygon (pts{pts}\n    ))\n  )"
    )
}

/// A standalone filled polygon graphic (`gr_poly`), not tied to a net or zone
/// fill — used for imported artwork rather than copper pours.
fn format_gr_poly(points: &[(f64, f64)], layer: &str) -> String {
    let uuid = new_uuid();
    let pts: String = points
        .iter()
        .map(|(x, y)| format!("\n      (xy {x} {y})"))
        .collect();
    format!(
        "\n  (gr_poly\n    (pts{pts}\n    )\n    \
         (stroke (width 0) (type solid))\n    (fill solid)\n    \
         (layer \"{layer}\")\n    (uuid \"{uuid}\")\n  )"
    )
}

/// Byte offset of the `)` that closes the block opening at `open_pos`.
///
/// Balances parens while skipping quoted strings, so it is independent of how
/// the file is indented — KiCAD 9 writes two spaces, KiCAD 10 writes tabs, and
/// a probe for either is wrong on the other.
fn close_of_block(content: &str, open_pos: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_str = false;
    let mut escaped = false;
    for (i, c) in content[open_pos..].char_indices() {
        if in_str {
            match c {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_str = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open_pos + i);
                }
            }
            _ => {}
        }
    }
    None
}

/// The leading whitespace of the first entry inside the block at `open_pos`,
/// so an inserted sibling matches the file it is written into.
fn entry_indent(content: &str, open_pos: usize) -> Option<String> {
    let after = &content[open_pos..];
    let nl = after.find('\n')?;
    let line = &after[nl + 1..];
    let indent: String = line
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect();
    (!indent.is_empty() && line[indent.len()..].starts_with('(')).then_some(indent)
}

// ─── Tool definitions ─────────────────────────────────────────────────────────

pub fn tools() -> Vec<ToolDef> {
    vec![
        tool!(
            "set_board_size",
            "Set the PCB board outline to a rectangle of the given dimensions on the Edge.Cuts layer.",
            json!({
                "type": "object",
                "properties": {
                    "board":    { "type": "string", "description": "Path to .kicad_pcb file" },
                    "width":    { "type": "number", "description": "Board width in mm" },
                    "height":   { "type": "number", "description": "Board height in mm" },
                    "origin_x": { "type": "number", "description": "Left edge X coordinate", "default": 0 },
                    "origin_y": { "type": "number", "description": "Top edge Y coordinate", "default": 0 }
                },
                "required": ["board", "width", "height"]
            }),
            |args, ctx| async move { handle_set_board_size(args, ctx).await }
        ),
        tool!(
            "get_board_info",
            "Return metadata about the PCB: title, revision, company, layer count, paper size.",
            json!({
                "type": "object",
                "properties": {
                    "board": { "type": "string", "description": "Path to .kicad_pcb file" }
                },
                "required": ["board"]
            }),
            |args, ctx| async move { handle_get_board_info(args, ctx).await }
        ),
        tool!(
            "get_board_extents",
            "Return the bounding box of all objects on the board (tries KiCAD IPC, falls back to file parse).",
            json!({
                "type": "object",
                "properties": {
                    "board": { "type": "string", "description": "Path to .kicad_pcb file" }
                },
                "required": ["board"]
            }),
            |args, ctx| async move { handle_get_board_extents(args, ctx).await }
        ),
        tool!(
            "get_layer_list",
            "Return all layers defined in the board with their names and types.",
            json!({
                "type": "object",
                "properties": {
                    "board": { "type": "string", "description": "Path to .kicad_pcb file" }
                },
                "required": ["board"]
            }),
            |args, ctx| async move { handle_get_layer_list(args, ctx).await }
        ),
        tool!(
            "add_layer",
            "Add a new inner copper or technical layer to the board layer stack.",
            json!({
                "type": "object",
                "properties": {
                    "board":       { "type": "string" },
                    "layer_name":  { "type": "string", "description": "KiCAD layer name (e.g. 'In1.Cu')" },
                    "layer_type":  { "type": "string", "description": "Type: 'signal', 'power', 'mixed', 'jumper'", "default": "signal" }
                },
                "required": ["board", "layer_name"]
            }),
            |args, ctx| async move { handle_add_layer(args, ctx).await }
        ),
        tool!(
            "set_active_layer",
            "Set the active layer recorded in the board file's setup section.",
            json!({
                "type": "object",
                "properties": {
                    "board":  { "type": "string" },
                    "layer":  { "type": "string", "description": "KiCAD layer name (e.g. 'F.Cu')" }
                },
                "required": ["board", "layer"]
            }),
            |args, ctx| async move { handle_set_active_layer(args, ctx).await }
        ),
        tool!(
            "add_board_outline",
            "Add a rectangular board outline on the Edge.Cuts layer at specified coordinates.",
            json!({
                "type": "object",
                "properties": {
                    "board":          { "type": "string" },
                    "x1":             { "type": "number", "description": "Top-left X in mm" },
                    "y1":             { "type": "number", "description": "Top-left Y in mm" },
                    "x2":             { "type": "number", "description": "Bottom-right X in mm" },
                    "y2":             { "type": "number", "description": "Bottom-right Y in mm" },
                    "corner_radius":  { "type": "number", "description": "Corner radius in mm (0 = sharp)", "default": 0 }
                },
                "required": ["board", "x1", "y1", "x2", "y2"]
            }),
            |args, ctx| async move { handle_add_board_outline(args, ctx).await }
        ),
        tool!(
            "add_mounting_hole",
            "Add an NPTH mounting hole footprint at the specified position.",
            json!({
                "type": "object",
                "properties": {
                    "board":          { "type": "string" },
                    "x":              { "type": "number", "description": "X position in mm" },
                    "y":              { "type": "number", "description": "Y position in mm" },
                    "drill_diameter": { "type": "number", "description": "Drill diameter in mm", "default": 3.2 },
                    "reference":      { "type": "string", "description": "Designator for the hole (e.g. 'H1')", "default": "H1" }
                },
                "required": ["board", "x", "y"]
            }),
            |args, ctx| async move { handle_add_mounting_hole(args, ctx).await }
        ),
        tool!(
            "add_board_text",
            "Add a silkscreen or fabrication text string to the board.",
            json!({
                "type": "object",
                "properties": {
                    "board":     { "type": "string" },
                    "text":      { "type": "string" },
                    "x":         { "type": "number" },
                    "y":         { "type": "number" },
                    "layer":     { "type": "string", "description": "Layer name", "default": "F.SilkS" },
                    "size":      { "type": "number", "description": "Font size in mm", "default": 1.0 },
                    "rotation":  { "type": "number", "description": "Rotation in degrees", "default": 0 }
                },
                "required": ["board", "text", "x", "y"]
            }),
            |args, ctx| async move { handle_add_board_text(args, ctx).await }
        ),
        tool!(
            "add_zone",
            "Add a copper fill zone polygon on a specified layer and net.",
            json!({
                "type": "object",
                "properties": {
                    "board":      { "type": "string" },
                    "net_name":   { "type": "string", "description": "Net name (e.g. 'GND')" },
                    "layer":      { "type": "string", "description": "Copper layer (e.g. 'F.Cu')" },
                    "points": {
                        "type": "array",
                        "description": "Polygon vertices as [{x, y}]",
                        "items": { "type": "object", "properties": { "x": { "type": "number" }, "y": { "type": "number" } } }
                    },
                    "clearance":  { "type": "number", "default": 0.2 },
                    "min_width":  { "type": "number", "default": 0.2 }
                },
                "required": ["board", "net_name", "layer", "points"]
            }),
            |args, ctx| async move { handle_add_zone(args, ctx).await }
        ),
        tool!(
            "import_svg_logo",
            "Import an SVG file as filled silkscreen or copper artwork (a logo, icon, or other \
             graphic). Curved paths are flattened into polygon outlines since KiCAD's board \
             format doesn't support Bezier curves in filled shapes. Tries KiCAD IPC first, \
             falls back to a direct file edit if KiCAD isn't running.",
            json!({
                "type": "object",
                "properties": {
                    "board":     { "type": "string", "description": "Path to .kicad_pcb file" },
                    "svg":       { "type": "string", "description": "Path to the .svg file to import" },
                    "width_mm":  { "type": "number", "description": "Target width in mm (aspect ratio preserved)" },
                    "x":         { "type": "number", "description": "X position of the artwork's top-left corner in mm", "default": 0 },
                    "y":         { "type": "number", "description": "Y position of the artwork's top-left corner in mm", "default": 0 },
                    "layer":     { "type": "string", "description": "Target layer", "default": "F.SilkS" }
                },
                "required": ["board", "svg", "width_mm"]
            }),
            |args, ctx| async move { handle_import_svg_logo(args, ctx).await }
        ),
    ]
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

async fn handle_set_board_size(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let board_path = get_path(args, "board")?;
    let width = match require_f64(args, "width") {
        Ok(v) => v,
        Err(e) => return Ok(e),
    };
    let height = match require_f64(args, "height") {
        Ok(v) => v,
        Err(e) => return Ok(e),
    };
    let ox = args["origin_x"].as_f64().unwrap_or(0.0);
    let oy = args["origin_y"].as_f64().unwrap_or(0.0);

    let x2 = ox + width;
    let y2 = oy + height;
    let w = 0.05_f64;

    // Try IPC first (live board in KiCAD, undo-aware); fall through to file edit.
    // ponytail: 4 segments over a single BoardRectangle keeps one builder path;
    // switch to board_rectangle if a native rect proves less flaky.
    let items = rect_outline_items(ox, oy, x2, y2, w);
    let requested_board = board_path.clone();
    if let Err(failure) = with_ipc(ctx.config.ipc_address.clone(), move |c| {
        // P.6.9.23: confirms the live session actually holds `board` before
        // writing — `create_items` has no board argument of its own to check
        // against, so without this it would silently land on whichever board
        // KiCAD happened to have open first. A `BoardMismatch` here still
        // disallows the file fallback below (`allows_file_fallback`), same as
        // any other proof KiCAD answered.
        c.ensure_board_is_active(&requested_board)?;
        c.create_items(items)
    })
    .await?
    {
        // Only a transport that never delivered the request may fall through
        // to editing the file: a KiCAD that answered — even to refuse — may
        // hold this board open and overwrite the edit on its next save.
        if !failure.allows_file_fallback() {
            return Ok(ipc_error_result(&failure));
        }
    } else {
        return Ok(CallToolResult::json(&json!({
            "width": width, "height": height,
            "x1": ox, "y1": oy, "x2": x2, "y2": y2,
            "source": "ipc"
        })));
    }

    // Append 4 Edge.Cuts lines (top, right, bottom, left)
    let lines = format!(
        "{}{}{}{}",
        format_gr_line(ox, oy, x2, oy, "Edge.Cuts", w),
        format_gr_line(x2, oy, x2, y2, "Edge.Cuts", w),
        format_gr_line(x2, y2, ox, y2, "Edge.Cuts", w),
        format_gr_line(ox, y2, ox, oy, "Edge.Cuts", w),
    );

    let content = std::fs::read_to_string(&board_path)?;
    let close_pos = content.rfind(')').unwrap_or(content.len());
    let new_content = apply_edits(content, vec![SexpEdit::insert(close_pos, lines)]);
    write_atomic(&board_path, &new_content)?;

    Ok(CallToolResult::json(&json!({
        "width": width, "height": height,
        "x1": ox, "y1": oy, "x2": x2, "y2": y2,
        "source": "file"
    })))
}

async fn handle_get_board_info(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let board_path = get_path(args, "board")?;
    let content = std::fs::read_to_string(&board_path)?;
    let tree = parse_sexp(&content)?;

    let tb = tree.find("title_block");
    let title = tb
        .and_then(|t| t.find("title"))
        .and_then(|n| n.get(1))
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .to_string();
    let date = tb
        .and_then(|t| t.find("date"))
        .and_then(|n| n.get(1))
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .to_string();
    let rev = tb
        .and_then(|t| t.find("rev"))
        .and_then(|n| n.get(1))
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .to_string();
    let company = tb
        .and_then(|t| t.find("company"))
        .and_then(|n| n.get(1))
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .to_string();

    // A layer is `(0 "F.Cu" signal)`, keyed by its ordinal rather than by a
    // tag, so find_all("") — which matches on the head — never matched one
    // and this was always 0. See konnect_sexp::layers.
    let stack = konnect_sexp::layers::layers(&tree);
    let layer_count = stack.len();
    let copper_layer_count = konnect_sexp::layers::copper(&stack).len();
    let paper = tree
        .find("paper")
        .and_then(|n| n.get(1))
        .and_then(|n| n.as_str())
        .unwrap_or("A4")
        .to_string();

    // Counts distinct named nets across both KiCAD net-table forms (#142):
    // on the old form this equals the prior `table_entries - 1` (net 0 is the
    // only unnamed entry), so boards on that form report the same count.
    let net_count = konnect_sexp::net::count_distinct_nets(&tree);

    Ok(CallToolResult::json(&json!({
        "file": board_path.display().to_string(),
        "title": title, "date": date, "revision": rev, "company": company,
        "paper": paper,
        "layer_count": layer_count,
        "copper_layer_count": copper_layer_count,
        "net_count": net_count
    })))
}

async fn handle_get_board_extents(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let board_path = get_path(args, "board")?;

    // Try IPC first; fall through to file-based computation on *any* failure.
    // Unconditional here, unlike the write paths: reading the file cannot
    // overwrite anything a live KiCAD holds. `ensure_board_is_active` still
    // runs first (P.6.9.23): without it, a KiCAD holding some *other* board
    // open would answer with that board's extents, reported as "source":
    // "ipc" as if they were `board`'s — a `BoardMismatch` here instead falls
    // through to the file-based computation below, which reads the right
    // file.
    let requested_board = board_path.clone();
    if let Ok(ext) = with_ipc(ctx.config.ipc_address.clone(), move |c| {
        c.ensure_board_is_active(&requested_board)?;
        c.get_board_extents()
    })
    .await?
    {
        return Ok(CallToolResult::json(&json!({
            "x_min": ext.min.x, "y_min": ext.min.y,
            "x_max": ext.max.x, "y_max": ext.max.y,
            "width": ext.max.x - ext.min.x,
            "height": ext.max.y - ext.min.y,
            "source": "ipc"
        })));
    }

    // File-based fallback: collect all coordinates from gr_lines and footprint positions
    let content = std::fs::read_to_string(&board_path)?;
    let tree = parse_sexp(&content)?;

    let (mut min_x, mut min_y) = (f64::MAX, f64::MAX);
    let (mut max_x, mut max_y) = (f64::MIN, f64::MIN);
    let mut update = |x: f64, y: f64| {
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    };

    for line in tree.find_all("gr_line") {
        if let (Some(s), Some(e)) = (line.find("start"), line.find("end")) {
            if let (Some(x1), Some(y1), Some(x2), Some(y2)) =
                (s.get_f64(1), s.get_f64(2), e.get_f64(1), e.get_f64(2))
            {
                update(x1, y1);
                update(x2, y2);
            }
        }
    }
    for fp in tree.find_all("footprint") {
        if let Some(at) = fp.find("at") {
            if let (Some(x), Some(y)) = (at.get_f64(1), at.get_f64(2)) {
                update(x, y);
            }
        }
    }

    if min_x == f64::MAX {
        return Ok(CallToolResult::json(
            &json!({ "x_min": 0, "y_min": 0, "x_max": 0, "y_max": 0, "width": 0, "height": 0, "source": "empty" }),
        ));
    }

    Ok(CallToolResult::json(&json!({
        "x_min": min_x, "y_min": min_y,
        "x_max": max_x, "y_max": max_y,
        "width": max_x - min_x,
        "height": max_y - min_y,
        "source": "file"
    })))
}

async fn handle_get_layer_list(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let board_path = get_path(args, "board")?;
    let content = std::fs::read_to_string(&board_path)?;
    let tree = parse_sexp(&content)?;

    if tree.find("layers").is_none() {
        return Ok(CallToolResult::error_kind(
            ToolErrorKind::MalformedDocument {
                path: board_path.display().to_string(),
                detail: "no (layers) section".to_string(),
            },
            "No (layers) section found in board file",
        ));
    }

    // Each child of layers looks like: (0 "F.Cu" signal). The ordinal is the
    // head of the list, so the fields sit one place earlier than the old
    // accessors assumed — and find_all("") never returned any of them anyway.
    let layers: Vec<serde_json::Value> = konnect_sexp::layers::layers(&tree)
        .into_iter()
        .map(|l| {
            json!({
                "id": l.id,
                "name": l.name,
                "type": l.kind,
                "user_name": l.user_name,
                "copper": l.is_copper(),
            })
        })
        .collect();

    Ok(CallToolResult::json(
        &json!({ "count": layers.len(), "layers": layers }),
    ))
}

async fn handle_add_layer(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let board_path = get_path(args, "board")?;
    let layer_name = match require_str(args, "layer_name") {
        Ok(v) => v.to_string(),
        Err(e) => return Ok(e),
    };
    let layer_type = args["layer_type"].as_str().unwrap_or("signal");

    // Fail closed on a name KiCAD does not define. The layer set is closed,
    // and a board carrying an unknown name does not open at all — so writing
    // one used to return success and hand back a file the caller could not
    // load. Verified against KiCAD 10: `(53 "User.8" user)` loads,
    // `(53 "TestLayer" user)` is refused with "Failed to load board".
    if !konnect_sexp::layers::is_canonical_name(&layer_name) {
        return Ok(CallToolResult::error_kind(
            ToolErrorKind::InvalidArgument {
                field: "layer_name".to_string(),
                reason: format!("'{layer_name}' is not a KiCAD layer name"),
            },
            format!(
                "'{layer_name}' is not a KiCAD layer name, and a board containing one \
                 cannot be opened. Names are fixed: F.Cu, B.Cu, In1.Cu..In30.Cu, \
                 User.1..User.45, and the technical layers (Edge.Cuts, F.Mask, …). \
                 To give a layer your own label, add the canonical layer and set its \
                 user name — `(53 \"User.8\" user \"{layer_name}\")`."
            ),
        ));
    }

    let content = std::fs::read_to_string(&board_path)?;

    // Find the (layers ...) block and insert before its closing paren
    let layers_pos = match content.find("(layers") {
        Some(p) => p,
        None => {
            return Ok(CallToolResult::error_kind(
                ToolErrorKind::MalformedDocument {
                    path: board_path.display().to_string(),
                    detail: "no (layers) section".to_string(),
                },
                "No (layers) section found",
            ))
        }
    };

    // Determine the next available inner copper ID (first unused ID in 1-30
    // range). The ids have to be read by shape — see konnect_sexp::layers.
    let tree = parse_sexp(&content)?;
    let used_ids: std::collections::HashSet<i32> = konnect_sexp::layers::layers(&tree)
        .iter()
        .map(|l| l.id)
        .collect();
    let new_id = match (1..=30).find(|id| !used_ids.contains(id)) {
        Some(id) => id,
        None => {
            return Ok(CallToolResult::error_kind(
                ToolErrorKind::InvalidArgument {
                    field: "layer_name".to_string(),
                    reason: "no free inner copper layer id: 1-30 are all in use".to_string(),
                },
                "No free inner copper layer id: 1-30 are all in use",
            ))
        }
    };

    // Close of the layers block, by paren balance. The previous probe looked
    // for a literal "\n  )", which a tab-indented KiCAD 10 file never
    // contains; the fallback then found the first ')' in the block — the
    // close of the *first layer entry* — and the new layer was written inside
    // it, producing a board KiCAD refuses to open.
    let close = match close_of_block(&content, layers_pos) {
        Some(p) => p,
        None => {
            return Ok(CallToolResult::error_kind(
                ToolErrorKind::MalformedDocument {
                    path: board_path.display().to_string(),
                    detail: "unbalanced (layers) block".to_string(),
                },
                "Unbalanced (layers) block; refusing to write",
            ))
        }
    };
    // Insert after the last entry rather than immediately before the close,
    // so the newline and indent already sitting in front of `)` stay there
    // and the block keeps KiCAD's own layout.
    let insert_pos = content[..close].trim_end().len();

    // Match whatever the file already indents entries with, rather than
    // hardcoding spaces into a file that may be tab-indented.
    let indent = entry_indent(&content, layers_pos).unwrap_or_else(|| "    ".to_string());
    let new_layer = format!("\n{indent}({new_id} \"{layer_name}\" {layer_type})");
    let new_content = apply_edits(content, vec![SexpEdit::insert(insert_pos, new_layer)]);
    write_atomic(&board_path, &new_content)?;

    Ok(CallToolResult::json(&json!({
        "added_layer": layer_name, "id": new_id, "type": layer_type
    })))
}

async fn handle_set_active_layer(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let board_path = get_path(args, "board")?;
    let layer = match require_str(args, "layer") {
        Ok(v) => v.to_string(),
        Err(e) => return Ok(e),
    };

    let content = std::fs::read_to_string(&board_path)?;
    let new_content = if let Some(pos) = content.find("(active_layer ") {
        let after = pos + "(active_layer ".len();
        let close = content[after..].find(')').unwrap_or(0);
        let layer_end = after + close;
        apply_edits(
            content,
            vec![SexpEdit::replace(after, layer_end, format!("\"{layer}\""))],
        )
    } else {
        // Insert into setup block
        let setup_close = content
            .find("(setup")
            .and_then(|p| content[p..].find('\n').map(|off| p + off))
            .unwrap_or(content.rfind(')').unwrap_or(content.len()));
        apply_edits(
            content,
            vec![SexpEdit::insert(
                setup_close,
                format!("\n    (active_layer \"{layer}\")"),
            )],
        )
    };
    write_atomic(&board_path, &new_content)?;

    Ok(CallToolResult::json(&json!({ "active_layer": layer })))
}

async fn handle_add_board_outline(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let board_path = get_path(args, "board")?;
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
    let w = 0.05_f64;

    // Try IPC first; fall through to file edit if KiCAD is not reachable.
    let items = rect_outline_items(x1, y1, x2, y2, w);
    let requested_board = board_path.clone();
    if let Err(failure) = with_ipc(ctx.config.ipc_address.clone(), move |c| {
        // P.6.9.23: see handle_set_board_size's identical guard above.
        c.ensure_board_is_active(&requested_board)?;
        c.create_items(items)
    })
    .await?
    {
        // Fail closed on anything that proves KiCAD answered (see above).
        if !failure.allows_file_fallback() {
            return Ok(ipc_error_result(&failure));
        }
    } else {
        return Ok(CallToolResult::json(&json!({
            "x1": x1, "y1": y1, "x2": x2, "y2": y2,
            "width": (x2-x1).abs(), "height": (y2-y1).abs(),
            "source": "ipc"
        })));
    }

    let lines = format!(
        "{}{}{}{}",
        format_gr_line(x1, y1, x2, y1, "Edge.Cuts", w),
        format_gr_line(x2, y1, x2, y2, "Edge.Cuts", w),
        format_gr_line(x2, y2, x1, y2, "Edge.Cuts", w),
        format_gr_line(x1, y2, x1, y1, "Edge.Cuts", w),
    );

    let content = std::fs::read_to_string(&board_path)?;
    let close_pos = content.rfind(')').unwrap_or(content.len());
    let new_content = apply_edits(content, vec![SexpEdit::insert(close_pos, lines)]);
    write_atomic(&board_path, &new_content)?;

    Ok(CallToolResult::json(&json!({
        "x1": x1, "y1": y1, "x2": x2, "y2": y2,
        "width": (x2-x1).abs(), "height": (y2-y1).abs(),
        "source": "file"
    })))
}

async fn handle_add_mounting_hole(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let board_path = get_path(args, "board")?;
    let x = match require_f64(args, "x") {
        Ok(v) => v,
        Err(e) => return Ok(e),
    };
    let y = match require_f64(args, "y") {
        Ok(v) => v,
        Err(e) => return Ok(e),
    };
    let drill_d = args["drill_diameter"].as_f64().unwrap_or(3.2);
    let reference = args["reference"].as_str().unwrap_or("H1");

    let fp_sexp = format_npth_footprint(x, y, drill_d, reference);
    let content = std::fs::read_to_string(&board_path)?;
    let close_pos = content.rfind(')').unwrap_or(content.len());
    let new_content = apply_edits(content, vec![SexpEdit::insert(close_pos, fp_sexp)]);
    write_atomic(&board_path, &new_content)?;

    Ok(CallToolResult::json(&json!({
        "reference": reference, "x": x, "y": y, "drill_diameter": drill_d
    })))
}

async fn handle_add_board_text(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let board_path = get_path(args, "board")?;
    let text = match require_str(args, "text") {
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
    let layer = args["layer"].as_str().unwrap_or("F.SilkS").to_string();
    let size = args["size"].as_f64().unwrap_or(1.0);
    let rotation = args["rotation"].as_f64().unwrap_or(0.0);

    // Try IPC first; fall through to file edit if KiCAD isn't reachable.
    let text_ipc = text.clone();
    let layer_ipc = layer.clone();
    let requested_board = board_path.clone();
    if let Err(failure) = with_ipc(ctx.config.ipc_address.clone(), move |c| {
        // P.6.9.23: see handle_set_board_size's identical guard above.
        c.ensure_board_is_active(&requested_board)?;
        let bt = builders::board_text(&layer_ipc, &text_ipc, x, y, size, rotation, false);
        let any = builders::pack_any(&bt, "kiapi.board.types.BoardText");
        c.create_items(vec![any])
    })
    .await?
    {
        // Fail closed on anything that proves KiCAD answered (see above).
        if !failure.allows_file_fallback() {
            return Ok(ipc_error_result(&failure));
        }
    } else {
        return Ok(CallToolResult::json(&json!({
            "text": text, "x": x, "y": y, "layer": layer, "size": size,
            "source": "ipc"
        })));
    }

    let gr_text = format_gr_text(&text, x, y, rotation, &layer, size);
    let content = std::fs::read_to_string(&board_path)?;
    let close_pos = content.rfind(')').unwrap_or(content.len());
    let new_content = apply_edits(content, vec![SexpEdit::insert(close_pos, gr_text)]);
    write_atomic(&board_path, &new_content)?;

    Ok(CallToolResult::json(&json!({
        "text": text, "x": x, "y": y, "layer": layer, "size": size,
        "source": "file"
    })))
}

async fn handle_add_zone(
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
    let min_width = args["min_width"].as_f64().unwrap_or(0.2);
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

    let points: Vec<(f64, f64)> = pts_arr
        .iter()
        .filter_map(|p| Some((p["x"].as_f64()?, p["y"].as_f64()?)))
        .collect();

    if points.len() < 3 {
        return Ok(CallToolResult::error_kind(
            ToolErrorKind::InvalidArgument {
                field: "points".to_string(),
                reason: "a zone outline needs at least 3 points".to_string(),
            },
            "Zone requires at least 3 points",
        ));
    }

    let content = std::fs::read_to_string(&board_path)?;
    let tree = parse_sexp(&content)?;
    let net_ref = match crate::tools::zone_net_ref(&tree, &net_name) {
        Ok(r) => r,
        Err(e) => return Ok(e),
    };
    let zone_sexp = format_zone_polygon(
        &net_ref.zone_tokens(&net_name),
        &layer,
        clearance,
        min_width,
        &points,
    );

    let close_pos = content.rfind(')').unwrap_or(content.len());
    let new_content = apply_edits(content, vec![SexpEdit::insert(close_pos, zone_sexp)]);
    write_atomic(&board_path, &new_content)?;

    let mut result = json!({
        "net": net_name, "layer": layer,
        "point_count": points.len()
    });
    // Only a board that keeps a net table has an id to report. Reporting 0 on
    // a KiCad 10 board was worse than reporting nothing: it named the
    // unconnected pseudo-net as if the zone had landed there on purpose.
    if let konnect_sexp::net::NetRef::ById(id) = net_ref {
        result["net_id"] = json!(id);
    }
    Ok(CallToolResult::json(&result))
}

async fn handle_import_svg_logo(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let board_path = get_path(args, "board")?;
    let svg_path = get_path(args, "svg")?;
    let width_mm = match require_f64(args, "width_mm") {
        Ok(v) => v,
        Err(e) => return Ok(e),
    };
    let x = args["x"].as_f64().unwrap_or(0.0);
    let y = args["y"].as_f64().unwrap_or(0.0);
    let layer = args["layer"].as_str().unwrap_or("F.SilkS").to_string();

    let svg_content = std::fs::read_to_string(&svg_path)?;
    let logo = crate::tools::svg_import::extract_polygons(&svg_content)?;
    if logo.polygons.is_empty() {
        // The file parsed as SVG and holds nothing this importer can place:
        // the document is what has to change, not the call.
        return Ok(CallToolResult::error_kind(
            ToolErrorKind::MalformedDocument {
                path: svg_path.display().to_string(),
                detail: "no fillable <path> elements".to_string(),
            },
            "No fillable paths found in the SVG (only <path> elements are supported).",
        ));
    }

    let placed =
        crate::tools::svg_import::scale_and_place(&logo.polygons, logo.width, width_mm, x, y);

    // Try IPC first; fall through to a direct file edit if KiCAD isn't reachable.
    let layer_ipc = layer.clone();
    let placed_ipc = placed.clone();
    let requested_board = board_path.clone();
    if let Err(failure) = with_ipc(ctx.config.ipc_address.clone(), move |c| {
        // P.6.9.23: see handle_set_board_size's identical guard above.
        c.ensure_board_is_active(&requested_board)?;
        let shape = builders::board_polygon(&layer_ipc, 0.0, true, &placed_ipc);
        let any = builders::pack_any(&shape, "kiapi.board.types.BoardGraphicShape");
        c.create_items(vec![any])
    })
    .await?
    {
        // Fail closed on anything that proves KiCAD answered (see above).
        if !failure.allows_file_fallback() {
            return Ok(ipc_error_result(&failure));
        }
    } else {
        return Ok(CallToolResult::json(&json!({
            "polygon_count": placed.len(),
            "layer": layer,
            "width_mm": width_mm,
            "source": "ipc"
        })));
    }

    let mut sexp = String::new();
    for polygon in &placed {
        sexp.push_str(&format_gr_poly(polygon, &layer));
    }
    let content = std::fs::read_to_string(&board_path)?;
    let close_pos = content.rfind(')').unwrap_or(content.len());
    let new_content = apply_edits(content, vec![SexpEdit::insert(close_pos, sexp)]);
    write_atomic(&board_path, &new_content)?;

    Ok(CallToolResult::json(&json!({
        "polygon_count": placed.len(),
        "layer": layer,
        "width_mm": width_mm,
        "source": "file"
    })))
}

#[cfg(test)]
mod svg_logo_tests {
    use super::*;
    use crate::router::ToolRouter;
    use crate::tools::ServerConfig;
    use std::sync::Arc;

    fn test_ctx() -> ToolContext {
        ToolContext::new(
            // Deliberately empty ipc_address: with_ipc fails fast against it,
            // exercising the file-fallback path without needing live KiCAD.
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

    fn blank_board() -> &'static str {
        "(kicad_pcb\n  (version 20250610)\n  (generator \"konnect\")\n  (paper \"A4\")\n  (net 0 \"\")\n)\n"
    }

    fn rect_svg() -> &'static str {
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100">
            <path d="M0 0 L100 0 L100 100 L0 100 Z" fill="black"/>
        </svg>"##
    }

    #[test]
    fn format_gr_poly_contains_layer_fill_and_points() {
        let sexp = format_gr_poly(&[(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)], "F.SilkS");
        assert!(sexp.contains("(gr_poly"));
        assert!(sexp.contains("(fill solid)"));
        assert!(sexp.contains("(layer \"F.SilkS\")"));
        assert!(sexp.contains("(xy 1 0)") || sexp.contains("(xy 1.0 0)"));
    }

    #[tokio::test]
    async fn import_svg_logo_file_fallback_places_polygon() {
        let dir = tempfile::tempdir().expect("tempdir");
        let board_path = dir.path().join("board.kicad_pcb");
        let svg_path = dir.path().join("logo.svg");
        std::fs::write(&board_path, blank_board()).unwrap();
        std::fs::write(&svg_path, rect_svg()).unwrap();

        let ctx = test_ctx();
        let args = json!({
            "board": board_path.to_str().unwrap(),
            "svg": svg_path.to_str().unwrap(),
            "width_mm": 10.0
        });

        let result = handle_import_svg_logo(&args, &ctx)
            .await
            .expect("handler should succeed");
        assert!(!result.is_error);

        let body = match &result.content[0] {
            crate::mcp::protocol::ToolContent::Text { text } => text.clone(),
            _ => panic!("expected text content"),
        };
        let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed["polygon_count"], json!(1));
        assert_eq!(parsed["source"], json!("file"));
        assert_eq!(parsed["layer"], json!("F.SilkS"));

        let updated = std::fs::read_to_string(&board_path).unwrap();
        assert!(updated.contains("(gr_poly"));
    }

    #[tokio::test]
    async fn import_svg_logo_rejects_svg_with_no_fillable_paths() {
        let dir = tempfile::tempdir().expect("tempdir");
        let board_path = dir.path().join("board.kicad_pcb");
        let svg_path = dir.path().join("empty.svg");
        std::fs::write(&board_path, blank_board()).unwrap();
        std::fs::write(
            &svg_path,
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"></svg>"##,
        )
        .unwrap();

        let ctx = test_ctx();
        let args = json!({
            "board": board_path.to_str().unwrap(),
            "svg": svg_path.to_str().unwrap(),
            "width_mm": 10.0
        });

        let result = handle_import_svg_logo(&args, &ctx).await.unwrap();
        assert!(result.is_error);
    }

    #[tokio::test]
    async fn import_svg_logo_missing_width_mm_returns_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let board_path = dir.path().join("board.kicad_pcb");
        let svg_path = dir.path().join("logo.svg");
        std::fs::write(&board_path, blank_board()).unwrap();
        std::fs::write(&svg_path, rect_svg()).unwrap();

        let ctx = test_ctx();
        let args = json!({
            "board": board_path.to_str().unwrap(),
            "svg": svg_path.to_str().unwrap()
        });

        let result = handle_import_svg_logo(&args, &ctx).await.unwrap();
        assert!(result.is_error);
    }
}

#[cfg(test)]
mod layers_block_tests {
    use super::*;
    use crate::router::ToolRouter;
    use crate::tools::ServerConfig;
    use std::sync::Arc;

    fn test_ctx() -> ToolContext {
        ToolContext::new(
            // Deliberately empty ipc_address: add_layer never touches IPC.
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

    // Both indent styles, same content: KiCAD 9 writes two spaces, 10 writes tabs.
    const SPACES: &str =
        "(kicad_pcb\n  (layers\n    (0 \"F.Cu\" signal)\n    (2 \"B.Cu\" signal)\n  )\n)";
    const TABS: &str =
        "(kicad_pcb\n\t(layers\n\t\t(0 \"F.Cu\" signal)\n\t\t(2 \"B.Cu\" signal)\n\t)\n)";

    fn layers_close(content: &str) -> usize {
        close_of_block(content, content.find("(layers").unwrap()).unwrap()
    }

    #[test]
    fn close_of_block_finds_the_same_close_under_either_indent() {
        for content in [SPACES, TABS] {
            let close = layers_close(content);
            // Everything up to the close balances, and the block ends after the
            // last entry rather than inside the first one.
            assert_eq!(&content[close..close + 1], ")");
            assert!(content[..close].contains("B.Cu"));
        }
    }

    #[test]
    fn close_of_block_is_not_the_first_paren_in_the_block() {
        // The old probe fell back to the first ')' — the close of entry one —
        // and wrote the new layer inside it.
        let content = TABS;
        let start = content.find("(layers").unwrap();
        let first = start + content[start..].find(')').unwrap();
        assert_ne!(layers_close(content), first);
    }

    #[test]
    fn close_of_block_ignores_parens_inside_strings() {
        let content = "(kicad_pcb\n\t(layers\n\t\t(0 \"F.Cu)(\" signal)\n\t)\n)";
        let close = layers_close(content);
        assert!(content[..close].contains("F.Cu)("));
    }

    #[test]
    fn close_of_block_refuses_an_unbalanced_block() {
        assert_eq!(close_of_block("(layers\n\t(0 \"F.Cu\" signal)", 0), None);
    }

    #[test]
    fn entry_indent_matches_the_file() {
        assert_eq!(
            entry_indent(SPACES, SPACES.find("(layers").unwrap()).as_deref(),
            Some("    ")
        );
        assert_eq!(
            entry_indent(TABS, TABS.find("(layers").unwrap()).as_deref(),
            Some("\t\t")
        );
    }

    #[test]
    fn entry_indent_declines_an_empty_block_rather_than_guessing() {
        let empty = "(kicad_pcb\n\t(layers\n\t)\n)";
        assert_eq!(entry_indent(empty, empty.find("(layers").unwrap()), None);
    }

    #[test]
    fn layers_canonical_names_match_kicads_own_enum() {
        // Guards konnect_sexp::layers::is_canonical_name against drift: the
        // authority is KiCAD's BoardLayer enum, shipped in the API protos.
        // Variant name -> file name is `BL_` off, remaining `_` to `.`.
        use konnect_ipc::gen::kiapi::board::types::BoardLayer;
        let sentinels = ["BL_UNKNOWN", "BL_UNDEFINED", "BL_UNSELECTED"];
        let mut checked = 0;
        for i in 0..=200i32 {
            let Ok(layer) = BoardLayer::try_from(i) else {
                continue;
            };
            let variant = layer.as_str_name();
            if sentinels.contains(&variant) {
                continue;
            }
            let name = variant.trim_start_matches("BL_").replacen('_', ".", 1);
            assert!(
                konnect_sexp::layers::is_canonical_name(&name),
                "{variant} maps to '{name}', which is_canonical_name rejects"
            );
            checked += 1;
        }
        // Cheap guard against the loop silently matching nothing.
        assert!(checked > 90, "only {checked} layers checked");
    }

    #[test]
    fn ids_in_use_are_seen_so_a_new_layer_does_not_collide() {
        // The regression this module guards: with the ids unreadable, the
        // free-id search always returned 1 and would have duplicated an
        // existing In1.Cu.
        let four_layer = "(kicad_pcb\n\t(layers\n\t\t(0 \"F.Cu\" signal)\n\t\t(1 \"In1.Cu\" signal)\n\t\t(2 \"B.Cu\" signal)\n\t)\n)";
        let tree = parse_sexp(four_layer).unwrap();
        let used: std::collections::HashSet<i32> = konnect_sexp::layers::layers(&tree)
            .iter()
            .map(|l| l.id)
            .collect();
        assert!(used.contains(&1));
        let next_free = (1..=30).find(|id| !used.contains(id));
        assert_eq!(next_free, Some(3));
    }

    /// `add_layer` on a tab-indented board (KiCAD 10's own style) writes the
    /// new entry as a sibling of the existing ones, not as a child of the
    /// first entry — the corruption this item exists to fix.
    #[tokio::test]
    async fn add_layer_on_a_tab_indented_board_inserts_a_sibling_not_a_child() {
        let dir = tempfile::tempdir().expect("tempdir");
        let board_path = dir.path().join("board.kicad_pcb");
        let content = "(kicad_pcb\n\t(version 20240108)\n\t(layers\n\t\t(0 \"F.Cu\" signal)\n\t\t(31 \"B.Cu\" signal)\n\t)\n)\n";
        std::fs::write(&board_path, content).unwrap();

        let ctx = test_ctx();
        let args = json!({
            "board": board_path.to_str().unwrap(),
            "layer_name": "In1.Cu",
            "layer_type": "signal"
        });
        let result = handle_add_layer(&args, &ctx).await.unwrap();
        assert!(!result.is_error, "add_layer failed: {result:?}");

        let new_content = std::fs::read_to_string(&board_path).unwrap();
        let tree = parse_sexp(&new_content).unwrap();
        let stack = konnect_sexp::layers::layers(&tree);
        // A layer written as a child of F.Cu instead of a sibling of the
        // `(layers …)` block would not show up here at all.
        assert!(
            stack.iter().any(|l| l.name == "In1.Cu"),
            "In1.Cu missing from the reparsed stackup: {stack:?}"
        );
        assert_eq!(stack.len(), 3, "stackup: {stack:?}");
    }
}
