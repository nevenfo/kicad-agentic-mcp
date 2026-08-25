//! `sch_batch` toolset — bulk/batch operations on schematic elements.
//!
//! **Critical invariant**: every write handler performs a single file read,
//! collects ALL mutations as `SexpEdit` values against the original content,
//! then calls `write_atomic` exactly once. This fixes the Python bug where
//! `batch_connect_to_net` did N separate read/write cycles.

use crate::mcp::error::ToolErrorKind;
use crate::mcp::protocol::CallToolResult;
use crate::tool;
use crate::tools::{
    find_all_symbol_instance_blocks, find_symbol_instance_block, get_path, opt_str, require_f64,
    require_str, set_symbol_property_on_all_units, symbol_property_at_spans, SetPropertyOutcome,
    ToolDef, RESERVED_PROPERTY_KEYS,
};
use konnect_schematic_editor as cse;
use konnect_sexp::{
    geometry::{point_on_segment, points_coincident, snap_point},
    schematic::{
        extract_labels, extract_lib_pins_for_unit, extract_symbol_instances, extract_wires,
        find_lib_symbol, format_net_label, format_wire, pin_endpoint, read_schematic,
    },
    writer::{
        apply_edits, find_block_with_leading_whitespace, find_enclosing_direct_child_block,
        new_uuid, read_consistent, write_atomic_if_unchanged, SexpEdit,
    },
};
use serde_json::json;
use std::collections::HashSet;

// Re-use the crate-internal net-graph primitives from sch_analysis.
use super::sch_analysis::build_net_graph;
// Re-use the single-item component placer and pin-to-pin router.
use super::sch_components::place_one_component;
use super::sch_wiring::{resolve_pin_endpoint, route_between};

// ─── Tool definitions ─────────────────────────────────────────────────────────

pub fn tools() -> Vec<ToolDef> {
    vec![
        tool!(
            "batch_connect_to_net",
            "Connect multiple component pins to a named net by adding net labels at each pin \
             endpoint. Single file read → all labels inserted → single file write.",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string", "description": "Path to .kicad_sch file" },
                    "net_name": { "type": "string", "description": "Name of the net to connect pins to" },
                    "pins": {
                        "type": "array",
                        "description": "List of {reference, pin_number} objects to connect",
                        "items": {
                            "type": "object",
                            "properties": {
                                "reference": { "type": "string" },
                                "pin_number": { "type": "string" }
                            },
                            "required": ["reference", "pin_number"]
                        }
                    }
                },
                "required": ["schematic", "net_name", "pins"]
            }),
            |args, ctx| async move { handle_batch_connect_to_net(args, ctx).await }
        ),
        tool!(
            "batch_place_components",
            "Place multiple symbols from KiCAD libraries in a single file read/write cycle. \
             Pass explicit references -- there is no auto-numbering; an omitted reference \
             becomes '?' like an eeschema-unannotated symbol, same as add_schematic_component.",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string", "description": "Path to .kicad_sch file" },
                    "components": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "lib_id": { "type": "string" },
                                "x": { "type": "number" }, "y": { "type": "number" },
                                "rotation": { "type": "number", "default": 0 },
                                "reference": { "type": "string" },
                                "value": { "type": "string" },
                                "unit": { "type": "integer", "default": 1 }
                            },
                            "required": ["lib_id", "x", "y"]
                        }
                    }
                },
                "required": ["schematic", "components"]
            }),
            |args, ctx| async move { handle_batch_place_components(args, ctx).await }
        ),
        tool!(
            "batch_connect_pins",
            "Connect multiple component pin pairs by reference and pin number, in a single \
             file read/write cycle.",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string", "description": "Path to .kicad_sch file" },
                    "connections": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "ref1": { "type": "string" }, "pin1": { "type": "string" },
                                "ref2": { "type": "string" }, "pin2": { "type": "string" }
                            },
                            "required": ["ref1", "pin1", "ref2", "pin2"]
                        }
                    }
                },
                "required": ["schematic", "connections"]
            }),
            |args, ctx| async move { handle_batch_connect_pins(args, ctx).await }
        ),
        tool!(
            "batch_delete",
            "Delete multiple schematic items (wires, labels, junctions, components) by UUID \
             or component reference designator — single file write.",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string", "description": "Path to .kicad_sch file" },
                    "uuids": {
                        "type": "array",
                        "description": "UUIDs of items to delete",
                        "items": { "type": "string" }
                    },
                    "references": {
                        "type": "array",
                        "description": "Component reference designators to delete",
                        "items": { "type": "string" }
                    }
                },
                "required": ["schematic"]
            }),
            |args, ctx| async move { handle_batch_delete(args, ctx).await }
        ),
        tool!(
            "bulk_move_schematic_components",
            "Move multiple components by a uniform dx/dy offset in a single atomic file write.",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string", "description": "Path to .kicad_sch file" },
                    "references": {
                        "type": "array",
                        "description": "Reference designators to move",
                        "items": { "type": "string" }
                    },
                    "uuids": {
                        "type": "array",
                        "description": "Symbol UUIDs; pass these or 'references', or both",
                        "items": { "type": "string" }
                    },
                    "dx": { "type": "number", "description": "X offset in mm" },
                    "dy": { "type": "number", "description": "Y offset in mm" }
                },
                "required": ["schematic", "dx", "dy"]
            }),
            |args, ctx| async move { handle_bulk_move(args, ctx).await }
        ),
        tool!(
            "batch_edit_schematic_components",
            "Apply field updates (Value, Footprint, custom properties) to multiple components \
             in a single atomic file write.",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string", "description": "Path to .kicad_sch file" },
                    "edits": {
                        "type": "array",
                        "description": "List of {reference|uuid, value?, footprint?, fields?} edit objects",
                        "items": {
                            "type": "object",
                            "properties": {
                                "reference": { "type": "string" },
                                "uuid": { "type": "string", "description": "Symbol UUID; pass this or 'reference'" },
                                "value": { "type": "string" },
                                "footprint": { "type": "string" },
                                "fields": {
                                    "type": "object",
                                    "description": "Additional property fields as key:value pairs"
                                }
                            }
                        }
                    }
                },
                "required": ["schematic", "edits"]
            }),
            |args, ctx| async move { handle_batch_edit(args, ctx).await }
        ),
        tool!(
            "batch_delete_schematic_components",
            "Delete multiple components by reference designator or UUID in a single atomic file write.",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string", "description": "Path to .kicad_sch file" },
                    "references": {
                        "type": "array",
                        "description": "Reference designators to delete",
                        "items": { "type": "string" }
                    },
                    "uuids": {
                        "type": "array",
                        "description": "Symbol UUIDs; pass these or 'references', or both",
                        "items": { "type": "string" }
                    }
                },
                "required": ["schematic"]
            }),
            |args, ctx| async move { handle_batch_delete_components(args, ctx).await }
        ),
        tool!(
            "connect_passthrough",
            "Add a wire stub and matching net label at a point to route a signal through \
             a region without drawing a full wire path. Direction controls stub orientation.",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string", "description": "Path to .kicad_sch file" },
                    "net_name": { "type": "string", "description": "Net name for the passthrough label" },
                    "x": { "type": "number", "description": "X position of the stub root in mm" },
                    "y": { "type": "number", "description": "Y position of the stub root in mm" },
                    "direction": {
                        "type": "string",
                        "description": "Stub direction: 'left', 'right', 'up', 'down'",
                        "default": "right"
                    }
                },
                "required": ["schematic", "net_name", "x", "y"]
            }),
            |args, ctx| async move { handle_connect_passthrough(args, ctx).await }
        ),
        tool!(
            "add_schematic_text",
            "Add a text annotation (non-net label) to the schematic at a given position.",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string", "description": "Path to .kicad_sch file" },
                    "text": { "type": "string", "description": "Text content to add" },
                    "x": { "type": "number", "description": "X position in mm" },
                    "y": { "type": "number", "description": "Y position in mm" },
                    "size": { "type": "number", "description": "Font size in mm", "default": 1.27 },
                    "rotation": { "type": "number", "description": "Rotation in degrees", "default": 0 }
                },
                "required": ["schematic", "text", "x", "y"]
            }),
            |args, ctx| async move { handle_add_schematic_text(args, ctx).await }
        ),
        tool!(
            "get_schematic_layout",
            "Return a compact spatial summary of the schematic: component positions, \
             bounding box, and optionally wire segments, label locations, junction \
             dots and no-connect flags. Wires, labels, junctions and no-connects come \
             with the uuid that addresses them.",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string", "description": "Path to .kicad_sch file" },
                    "include_wires": { "type": "boolean", "description": "Include wire data", "default": true },
                    "include_labels": { "type": "boolean", "description": "Include label data", "default": true },
                    "include_junctions": { "type": "boolean", "description": "Include junction dots with their uuids", "default": false },
                    "include_no_connects": { "type": "boolean", "description": "Include no-connect flags with their uuids", "default": false }
                },
                "required": ["schematic"]
            }),
            |args, ctx| async move { handle_get_layout(args, ctx).await }
        ),
        tool!(
            "validate_wire_connections",
            "Check all wire endpoints for floating ends (not connected to a pin, label, \
             or another wire). Reports each floating endpoint with its coordinates.",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string", "description": "Path to .kicad_sch file" },
                    "tolerance": { "type": "number", "description": "Snap tolerance in mm", "default": 0.01 }
                },
                "required": ["schematic"]
            }),
            |args, ctx| async move { handle_validate_wire_connections(args, ctx).await }
        ),
        tool!(
            "validate_component_connections",
            "Check that every non-passive pin on every component has at least one wire \
             or label connected. Reports unconnected pins with reference, pin number, \
             and schematic position.",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string", "description": "Path to .kicad_sch file" },
                    "ignore_power_pins": {
                        "type": "boolean",
                        "description": "Skip power-type pins in the check",
                        "default": false
                    },
                    "references": {
                        "type": "array",
                        "description": "Limit check to these reference designators (empty = all)",
                        "items": { "type": "string" }
                    }
                },
                "required": ["schematic"]
            }),
            |args, ctx| async move { handle_validate_component_connections(args, ctx).await }
        ),
    ]
}

// ─── Private helpers ──────────────────────────────────────────────────────────

/// Find the `(symbol ...)` block for every unit of a reference designator,
/// plus each block's leading whitespace so deletion leaves clean formatting.
/// Returns `(block_start, block_end)` byte offset pairs in `content`.
///
/// Every unit, not just the first: deleting a component must drop every unit
/// sharing its designator, or the surviving units are a half-deleted
/// component (P.6.8.2). Used by `batch_delete` and `batch_delete_components`,
/// whose deletes have always been reference-wide by contract, unlike
/// `move`/`rotate`.
fn find_all_symbol_blocks(content: &str, reference: &str) -> Vec<(usize, usize)> {
    find_all_symbol_instance_blocks(content, reference)
        .into_iter()
        .filter_map(|(sym_start, _)| find_block_with_leading_whitespace(content, sym_start))
        .collect()
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

async fn handle_batch_connect_to_net(
    args: &serde_json::Value,
    _ctx: &crate::tools::ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let net_name = match require_str(args, "net_name") {
        Ok(v) => v.to_string(),
        Err(e) => return Ok(e),
    };
    let pins = match args["pins"].as_array() {
        Some(a) => a.clone(),
        None => {
            return Ok(CallToolResult::error_kind(
                ToolErrorKind::InvalidArgument {
                    field: "pins".to_string(),
                    reason: "must be an array".to_string(),
                },
                "Missing 'pins' array",
            ))
        }
    };

    let (content, tree) = read_schematic(&sch_path)?;
    let instances = extract_symbol_instances(&tree);
    let lib_syms = tree
        .find("lib_symbols")
        .map(|n| n.find_all("symbol"))
        .unwrap_or_default();

    let mut inserts = String::new();
    let mut added: Vec<serde_json::Value> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for pin_spec in &pins {
        let reference = match pin_spec["reference"].as_str() {
            Some(r) => r,
            None => {
                errors.push("Missing 'reference' in pin spec".into());
                continue;
            }
        };
        let pin_number = match pin_spec["pin_number"].as_str() {
            Some(p) => p,
            None => {
                errors.push("Missing 'pin_number' in pin spec".into());
                continue;
            }
        };

        // A multi-unit symbol places one `SymbolInstance` per unit under the
        // same `Reference` (e.g. two `U1` entries for an LM2904's two
        // op-amps). Picking the first match ignored `unit` entirely, so a
        // pin that only exists on unit 2 resolved through unit 1's placement
        // — silently landing on whatever pin unit 1 happens to have at the
        // same local coordinates (P.6.8.1). Try every instance carrying
        // this reference and keep the one whose own unit actually declares
        // the requested pin.
        let candidates: Vec<&konnect_sexp::schematic::SymbolInstance> = instances
            .iter()
            .filter(|i| i.reference == reference)
            .collect();
        if candidates.is_empty() {
            errors.push(format!("Component '{}' not found", reference));
            continue;
        }

        let mut pin_ep = None;
        let mut tried_units: Vec<u32> = Vec::new();
        for inst in &candidates {
            let Some(sym) = find_lib_symbol(&lib_syms, inst) else {
                continue;
            };
            tried_units.push(inst.unit);
            if let Some(p) = extract_lib_pins_for_unit(sym, inst.unit)
                .into_iter()
                .find(|p| p.number == pin_number)
            {
                pin_ep = Some(pin_endpoint(&p, inst.pin_transform()));
                break;
            }
        }

        match pin_ep {
            Some((px, py)) => {
                inserts.push_str(&format_net_label(&net_name, px, py, 0.0));
                added.push(json!({
                    "reference": reference,
                    "pin": pin_number,
                    "x": px,
                    "y": py
                }));
            }
            // Two different failures, and telling them apart is the whole
            // point of naming the units: the pin is on no unit, or no unit's
            // library symbol resolved at all — in which case saying "units
            // tried: []" would describe the wrong problem.
            None if tried_units.is_empty() => errors.push(format!(
                "No library symbol resolved for '{}', so pin '{}' could not be located",
                reference, pin_number
            )),
            None => errors.push(format!(
                "Pin '{}' not found on '{}' (units tried: {:?})",
                pin_number, reference, tried_units
            )),
        }
    }

    if !inserts.is_empty() {
        let expected = content.clone();
        let close_pos = content.rfind(')').unwrap_or(content.len());
        let edits = vec![SexpEdit::insert(close_pos, inserts)];
        let new_content = apply_edits(content, edits);
        write_atomic_if_unchanged(&sch_path, &expected, &new_content)?;
    }

    Ok(CallToolResult::json(&json!({
        "net": net_name,
        "added": added,
        "added_count": added.len(),
        "errors": errors
    })))
}

/// Extract the message text out of a `CallToolResult` error, for folding a
/// single-item handler's structured error into a batch tool's `errors` list.
fn error_text(result: &CallToolResult) -> String {
    match result.content.first() {
        Some(crate::mcp::protocol::ToolContent::Text { text }) => text.clone(),
        _ => "unknown error".to_string(),
    }
}

async fn handle_batch_place_components(
    args: &serde_json::Value,
    _ctx: &crate::tools::ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let components = match args["components"].as_array() {
        Some(a) => a.clone(),
        None => {
            return Ok(CallToolResult::error_kind(
                ToolErrorKind::InvalidArgument {
                    field: "components".to_string(),
                    reason: "must be an array".to_string(),
                },
                "Missing 'components' array",
            ))
        }
    };

    let mut sch = cse::Schematic::load(&sch_path)?;
    let (project_name, instance_paths) = crate::tools::instance_targets(&sch_path, &mut sch);

    let mut placed: Vec<serde_json::Value> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for comp in &components {
        let Some(lib_id) = comp["lib_id"].as_str() else {
            errors.push("Missing 'lib_id' in component spec".into());
            continue;
        };
        let (Some(x), Some(y)) = (comp["x"].as_f64(), comp["y"].as_f64()) else {
            errors.push(format!("Missing 'x'/'y' for '{}'", lib_id));
            continue;
        };
        let rotation = comp["rotation"].as_f64().unwrap_or(0.0);
        let reference = comp["reference"].as_str().unwrap_or("?");
        let value = comp["value"].as_str();
        let unit = comp["unit"].as_f64().unwrap_or(1.0) as u32;

        match place_one_component(
            &mut sch,
            &instance_paths,
            &project_name,
            lib_id,
            x,
            y,
            rotation,
            reference,
            value,
            unit,
        ) {
            Ok(v) => placed.push(v),
            Err(e) => errors.push(error_text(&e)),
        }
    }

    if !placed.is_empty() {
        sch.overwrite()?;
    }

    let mut result = CallToolResult::json(&json!({
        "placed": placed,
        "placed_count": placed.len(),
        "errors": errors
    }));
    result.is_error = placed.is_empty() && !errors.is_empty();
    Ok(result)
}

async fn handle_batch_connect_pins(
    args: &serde_json::Value,
    _ctx: &crate::tools::ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let connections = match args["connections"].as_array() {
        Some(a) => a.clone(),
        None => {
            return Ok(CallToolResult::error_kind(
                ToolErrorKind::InvalidArgument {
                    field: "connections".to_string(),
                    reason: "must be an array".to_string(),
                },
                "Missing 'connections' array",
            ))
        }
    };

    let (content, tree) = read_schematic(&sch_path)?;
    let expected = content.clone();
    let instances = extract_symbol_instances(&tree);
    let lib_syms = tree
        .find("lib_symbols")
        .map(|n| n.find_all("symbol"))
        .unwrap_or_default();

    // Resolve every endpoint from the initial tree before any wire is
    // inserted -- symbols/lib_symbols never change as wires are added, so
    // this is safe to do up front instead of re-resolving per connection.
    let mut resolved: Vec<(f64, f64, f64, f64)> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    for conn in &connections {
        let (Some(ref1), Some(pin1), Some(ref2), Some(pin2)) = (
            conn["ref1"].as_str(),
            conn["pin1"].as_str(),
            conn["ref2"].as_str(),
            conn["pin2"].as_str(),
        ) else {
            errors.push("Missing ref1/pin1/ref2/pin2 in connection spec".into());
            continue;
        };
        match (
            resolve_pin_endpoint(&instances, &lib_syms, ref1, pin1),
            resolve_pin_endpoint(&instances, &lib_syms, ref2, pin2),
        ) {
            (Ok((x1, y1)), Ok((x2, y2))) => resolved.push((x1, y1, x2, y2)),
            (Err(e), _) | (_, Err(e)) => errors.push(e.to_string()),
        }
    }

    // ponytail: re-parses content per wire; incremental tree edits if batches get huge.
    let mut new_content = content;
    for (x1, y1, x2, y2) in &resolved {
        new_content = route_between(new_content, *x1, *y1, *x2, *y2);
    }

    if !resolved.is_empty() {
        write_atomic_if_unchanged(&sch_path, &expected, &new_content)?;
    }

    let mut result = CallToolResult::json(&json!({
        "connected_count": resolved.len(),
        "errors": errors
    }));
    result.is_error = resolved.is_empty() && !errors.is_empty();
    Ok(result)
}

async fn handle_batch_delete(
    args: &serde_json::Value,
    _ctx: &crate::tools::ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let content = read_consistent(&sch_path)?;
    let expected = content.clone();

    let mut edits: Vec<SexpEdit> = Vec::new();
    let mut deleted: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let mut delete_ranges: HashSet<(usize, usize)> = HashSet::new();

    // Delete by UUID — walk back from uuid node to enclosing top-level block.
    //
    // Deliberately NOT migrated to `find_schematic_item_by_uuid`/
    // `konnect_sexp::item_locations` (D.4.1.1): those only match an item's own
    // direct-child `(uuid …)`, but this handler has always accepted a UUID
    // *nested* inside a top-level item — most notably a `(sheet …)`'s own
    // `(pin …)` UUID — and deleted the enclosing item, per
    // `is_deletable_schematic_item` below. Switching to the direct-child index
    // would turn that accepted input into `NotFound` (INV8 regression).
    if let Some(uuids) = args["uuids"].as_array() {
        for uuid_val in uuids {
            let uuid = match uuid_val.as_str() {
                Some(u) => u,
                None => continue,
            };
            let pattern = format!(r#"(uuid "{}")"#, uuid);
            match content.find(&pattern) {
                Some(uuid_pos) => {
                    match find_enclosing_direct_child_block(&content, "kicad_sch", uuid_pos) {
                        Some((block_start, block_end)) => {
                            let item = &content[block_start..block_end];
                            if !is_deletable_schematic_item(item) {
                                errors.push(format!(
                                    "UUID '{}' belongs to protected schematic structure '{}'",
                                    uuid,
                                    sexp_tag(item)
                                ));
                                continue;
                            }
                            match find_block_with_leading_whitespace(&content, block_start) {
                                Some((del_start, del_end)) => {
                                    if delete_ranges.insert((del_start, del_end)) {
                                        edits.push(SexpEdit::delete(del_start, del_end));
                                        deleted.push(uuid.to_string());
                                    }
                                }
                                None => {
                                    errors.push(format!("Cannot parse block for UUID '{}'", uuid))
                                }
                            }
                        }
                        None => errors.push(format!("Cannot locate block for UUID '{}'", uuid)),
                    }
                }
                None => errors.push(format!("UUID '{}' not found", uuid)),
            }
        }
    }

    // Delete by reference designator
    if let Some(refs) = args["references"].as_array() {
        for ref_val in refs {
            let reference = match ref_val.as_str() {
                Some(r) => r,
                None => continue,
            };
            // Every unit of the designator, not just the first — a
            // reference-addressed delete must not leave a half-deleted
            // multi-unit component behind (P.6.8.2).
            let blocks = find_all_symbol_blocks(&content, reference);
            if blocks.is_empty() {
                errors.push(format!("Component '{}' not found", reference));
                continue;
            }
            let mut any_new = false;
            for (del_start, del_end) in blocks {
                if delete_ranges.insert((del_start, del_end)) {
                    edits.push(SexpEdit::delete(del_start, del_end));
                    any_new = true;
                }
            }
            if any_new {
                deleted.push(reference.to_string());
            }
        }
    }

    let new_content = apply_edits(content, edits);
    write_atomic_if_unchanged(&sch_path, &expected, &new_content)?;

    Ok(CallToolResult::json(&json!({
        "deleted_count": deleted.len(),
        "deleted": deleted,
        "errors": errors
    })))
}

fn sexp_tag(block: &str) -> &str {
    let Some(after_open) = block.strip_prefix('(') else {
        return "";
    };
    let end = after_open
        .find(|c: char| c.is_whitespace() || c == '(' || c == ')')
        .unwrap_or(after_open.len());
    &after_open[..end]
}

// Blocklist of structural forms, not an allowlist of item kinds: deleting a
// drawing item (text, bus, sheet, image, polyline, …) by UUID has always
// worked and must keep working — only the schematic's skeleton is protected.
fn is_deletable_schematic_item(block: &str) -> bool {
    !matches!(
        sexp_tag(block),
        "version"
            | "generator"
            | "generator_version"
            | "uuid"
            | "paper"
            | "title_block"
            | "lib_symbols"
            | "sheet_instances"
            | "symbol_instances"
            | "embedded_fonts"
    )
}

/// Byte range of the symbol's own `(at x y rot)` coordinate text (not a
/// property's) inside `content[sym_start..sym_end]` — the first `(at ` in
/// the block, since a symbol's non-property children always precede its
/// first `(property …)`.
///
/// A block this malformed should not occur in practice: `sym_start..sym_end`
/// always comes from a balanced-block finder, so the block's own text always
/// ends in `)`, guaranteeing a `(at ` found inside it is eventually followed
/// by one. But the scan does not rely on that guarantee to avoid panicking:
/// before this, `close_rel` defaulted to 0 on a missing `)`, so `at_end`
/// came out less than `at_abs` and slicing `content[at_abs..at_end]` panicked.
fn symbol_own_at_span(
    content: &str,
    sym_start: usize,
    sym_end: usize,
) -> Result<(usize, usize), String> {
    let sym_block = &content[sym_start..sym_end];
    let at_pat = "(at ";
    let at_rel = sym_block
        .find(at_pat)
        .ok_or_else(|| "No (at) in symbol".to_string())?;
    let at_abs = sym_start + at_rel + at_pat.len();
    let close_rel = sym_block[at_rel..]
        .find(')')
        .ok_or_else(|| "Unterminated (at) in symbol".to_string())?;
    let at_end = sym_start + at_rel + close_rel;
    Ok((at_abs, at_end))
}

/// Round a millimetre coordinate to the precision KiCAD actually writes,
/// so adding a delta does not leak binary-float noise into the file.
///
/// The symbol's own anchor is protected by `snap_point`; a property anchor is
/// a plain addition, and `139.7 → 144.78` moves a field at `241.3` to
/// `246.38000000000002` and one at `3.556` to `8.636000000000013`. Writing
/// that is the same class of damage P.6.9.4 removed — a byte changing for no
/// reason the caller asked for.
///
/// Six decimals, measured: across 126 933 `(at …)` coordinates in the KiCad 10
/// demo schematics, every one but a single value carries at most **four**
/// decimals. The exception, `59.209102362204725`, is an inch conversion rather
/// than float noise, and six decimals moves it by 0.4 nm — under KiCAD's own
/// 1 nm internal resolution. Addition noise, meanwhile, shows up around the
/// thirteenth decimal. Six separates the two with room on both sides.
fn mm(value: f64) -> f64 {
    (value * 1e6).round() / 1e6
}

async fn handle_bulk_move(
    args: &serde_json::Value,
    _ctx: &crate::tools::ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let dx = match require_f64(args, "dx") {
        Ok(v) => v,
        Err(e) => return Ok(e),
    };
    let dy = match require_f64(args, "dy") {
        Ok(v) => v,
        Err(e) => return Ok(e),
    };

    let content = read_consistent(&sch_path)?;
    let expected = content.clone();
    let batch = match crate::tools::resolve_component_batch(&content, args, &sch_path) {
        Ok(batch) => batch,
        Err(result) => return Ok(*result),
    };
    let mut edits: Vec<SexpEdit> = Vec::new();
    let mut moved: Vec<serde_json::Value> = Vec::new();
    // A uuid naming no symbol joins the per-entry errors this handler already
    // collects for a missing designator: the rest of the batch still moves.
    let mut errors: Vec<String> = batch.unresolved;

    for reference in &batch.references {
        let reference = reference.as_str();

        // A multi-unit designator has no single "the" position to move by
        // `reference` — moving unit 1 and leaving unit 2 behind (or vice
        // versa) is legitimate on its own but silently picking the first
        // unit here would move one the caller never named (P.6.8.2, see
        // `refuse_ambiguous_multiunit_geometry` in sch_components.rs, whose
        // uuid-addressed path this batch tool has no way to take). Refused,
        // not guessed at.
        let units = find_all_symbol_instance_blocks(&content, reference);
        if units.len() > 1 {
            let uuids: Vec<String> = units
                .iter()
                .filter_map(|&(s, e)| crate::tools::symbol_block_uuid(&content[s..e]))
                .collect();
            errors.push(format!(
                "'{}' has {} units ({}); bulk_move cannot address one unit of a \
                 multi-unit symbol — move it with move_schematic_component and a 'uuid'",
                reference,
                units.len(),
                uuids.join(", ")
            ));
            continue;
        }

        // Locate symbol block for this reference
        let (sym_start, sym_end) = match find_symbol_instance_block(&content, reference) {
            Some(r) => r,
            None => {
                errors.push(format!("'{}' not found", reference));
                continue;
            }
        };

        // Find the symbol's own (at X Y [ROT]) inside this symbol block.
        let (at_abs, at_end) = match symbol_own_at_span(&content, sym_start, sym_end) {
            Ok(r) => r,
            Err(why) => {
                errors.push(format!("{why} '{}'", reference));
                continue;
            }
        };

        let at_str = &content[at_abs..at_end];
        let parts: Vec<&str> = at_str.split_whitespace().collect();
        let x = parts
            .first()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);
        let y = parts
            .get(1)
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);
        let rot = parts
            .get(2)
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.0);

        let (new_x, new_y) = snap_point(x + dx, y + dy, 1.27);
        edits.push(SexpEdit::replace(
            at_abs,
            at_end,
            format!("{new_x} {new_y} {rot}"),
        ));

        // Every field's text carries its own absolute `(at …)`; moving only
        // the symbol's own anchor above leaves field text sitting where the
        // symbol used to be. Shift each by the delta actually applied to the
        // symbol — after `snap_point`, not the raw `dx`/`dy` — and leave its
        // rotation alone: a hidden field's `(at … 0)` does not track the
        // symbol's own rotation (measured on KiCad's demo schematics; see
        // `symbol_insertion_site`'s doc comment).
        let applied_dx = new_x - x;
        let applied_dy = new_y - y;
        // A move that snapped back to where the symbol already was must not
        // rewrite a single field byte: an edit that changes nothing still
        // shows up in the file (a `(at x y)` with no rotation would come back
        // as `(at x y 0)`), which is the reformatting P.6.9.4 just removed.
        let symbol_actually_moved = applied_dx != 0.0 || applied_dy != 0.0;
        for (val_start, val_end) in symbol_property_at_spans(&content, sym_start, sym_end) {
            if !symbol_actually_moved {
                break;
            }
            let field_at = &content[val_start..val_end];
            let field_parts: Vec<&str> = field_at.split_whitespace().collect();
            let fx = field_parts
                .first()
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0);
            let fy = field_parts
                .get(1)
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0);
            let f_rot = field_parts
                .get(2)
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0);
            edits.push(SexpEdit::replace(
                val_start,
                val_end,
                format!("{} {} {f_rot}", mm(fx + applied_dx), mm(fy + applied_dy)),
            ));
        }

        moved.push(json!({
            "reference": reference,
            "old_x": x, "old_y": y,
            "new_x": new_x, "new_y": new_y
        }));
    }

    let new_content = apply_edits(content, edits);
    write_atomic_if_unchanged(&sch_path, &expected, &new_content)?;

    Ok(CallToolResult::json(&json!({
        "moved_count": moved.len(),
        "moved": moved,
        "dx": dx, "dy": dy,
        "errors": errors
    })))
}

/// One batch entry's field work: both the standard fields ("value",
/// "footprint") and the free-form `fields` map, all routed through
/// `set_symbol_property` (P.6.9.18).
struct PendingProperties {
    reference: String,
    /// `(field, stored text)` pairs, standard fields first, then `fields`
    /// map entries in their given order — so a spec naming the same key
    /// both ways (e.g. `"footprint"` and `fields.Footprint`) has the
    /// `fields` entry win, matching `edit_schematic_component`'s order of
    /// named argument then `fields` map (P.6.9.18).
    updates: Vec<(String, String)>,
    /// What this component changed, filled in as `updates` is applied. Every
    /// field write lands here now, standard fields included (P.6.9.18).
    changes: Vec<String>,
}

async fn handle_batch_edit(
    args: &serde_json::Value,
    _ctx: &crate::tools::ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let edits_arr = match args["edits"].as_array() {
        Some(a) => a.clone(),
        None => {
            return Ok(CallToolResult::error_kind(
                ToolErrorKind::InvalidArgument {
                    field: "edits".to_string(),
                    reason: "must be an array".to_string(),
                },
                "Missing 'edits' array",
            ))
        }
    };

    let content = read_consistent(&sch_path)?;
    let expected = content.clone();
    let mut changed: Vec<serde_json::Value> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    // Every spec's field writes (standard fields and `fields` map alike),
    // applied once the whole batch has been walked and validated.
    let mut pending_properties: Vec<PendingProperties> = Vec::new();

    // One index for the whole batch, consulted only by the entries that
    // carry a `uuid` (D.4.1.6); `reference` entries never reach it.
    let by_uuid = if edits_arr
        .iter()
        .any(|spec| spec["reference"].as_str().is_none() && spec["uuid"].as_str().is_some())
    {
        match crate::tools::symbol_references_by_uuid(&content, &sch_path) {
            Ok(map) => map,
            Err(result) => return Ok(*result),
        }
    } else {
        std::collections::HashMap::new()
    };

    for edit_spec in &edits_arr {
        // `reference` wins when an entry carries both, as everywhere else.
        let reference = match (edit_spec["reference"].as_str(), edit_spec["uuid"].as_str()) {
            (Some(r), _) => r,
            (None, Some(uuid)) => match by_uuid.get(uuid) {
                Some(reference) => reference.as_str(),
                None => {
                    errors.push(crate::tools::no_component_with_uuid(uuid));
                    continue;
                }
            },
            (None, None) => {
                errors.push("Missing 'reference' or 'uuid' in edit spec".into());
                continue;
            }
        };

        // Standard fields ("value", "footprint") now go through the same
        // `updates` vector as "fields" below (P.6.9.18): they used to be
        // resolved as byte ranges via `field_value_range` and refused outright
        // when the property did not exist, which made a bare-placed symbol
        // (no `Footprint` property at all) unable to receive one through
        // batch edit — exactly the case J.2.4.1 fixed on the single-component
        // path via `set_symbol_property`'s insert-when-missing behaviour.
        // Pushed first, so a `fields.Footprint` on the same spec is applied
        // after and wins, mirroring `edit_schematic_component`'s order
        // (named argument, then the `fields` map, last write wins — see
        // l.757-765 of sch_components.rs).
        let mut updates: Vec<(String, String)> = Vec::new();
        for (field, key) in &[("Value", "value"), ("Footprint", "footprint")] {
            if let Some(new_val) = edit_spec[key].as_str() {
                updates.push((field.to_string(), new_val.to_string()));
            }
        }

        // Arbitrary extra fields from "fields". This is the same generic
        // property path `edit_schematic_component` has, so it answers the same
        // way (P.6.9.14): a number or a boolean is stored as its text form,
        // anything with no text form is refused out loud rather than silently
        // dropped, a key the symbol does not carry yet is inserted rather than
        // refused (J.2.4.1), and `Reference` is refused outright because its
        // designator lives in `(instances …)` too and this path would only
        // rewrite the property half of it (D124).
        match edit_spec.get("fields") {
            None | Some(serde_json::Value::Null) => {}
            Some(serde_json::Value::Object(map)) => {
                for (key, raw) in map {
                    match crate::tools::sch_components::property_text(raw) {
                        Some(text) => updates.push((key.clone(), text)),
                        None => errors.push(format!(
                            "'{}' on '{}': value must be a string, number or boolean",
                            key, reference
                        )),
                    }
                }
            }
            Some(_) => errors.push(format!(
                "'fields' on '{}' must be an object of key:value pairs",
                reference
            )),
        }

        pending_properties.push(PendingProperties {
            reference: reference.to_string(),
            updates,
            changes: Vec::new(),
        });
    }

    // A single pass now: standard fields ("value", "footprint") and the
    // `fields` map both end up in the same `updates` vector and are both
    // written through `set_symbol_property`, which re-locates the symbol
    // by reference on every call and returns an already-spliced document —
    // exactly what `set_field` does on the single-component path. There is
    // no offset-based phase left to run first (P.6.9.18 removed the last one,
    // which used to resolve "value"/"footprint" as byte ranges via
    // `field_value_range` and refuse them outright when the property did not
    // exist yet). Walking `updates` over the growing string keeps every
    // insertion's position and indentation correct without ever
    // reserialising the document, so a one-field edit is still a one-line
    // diff (P.6.9.4).
    let mut new_content = content;
    for pending in &mut pending_properties {
        for (field, value) in &pending.updates {
            // Every unit sharing `pending.reference`, not just the first
            // (P.6.8.2): `Value`/`Footprint`/custom fields are the
            // component's, not one unit's, and this is the same generic
            // property path `edit_schematic_component`'s `set_field` uses.
            if find_all_symbol_instance_blocks(&new_content, &pending.reference).is_empty() {
                errors.push(format!("Component '{}' not found", pending.reference));
                continue;
            }
            match set_symbol_property_on_all_units(
                &new_content,
                &pending.reference,
                field,
                value,
                RESERVED_PROPERTY_KEYS,
            ) {
                Ok((updated, outcome)) => {
                    new_content = updated;
                    pending.changes.push(match outcome {
                        SetPropertyOutcome::Updated => format!("{} → {}", field, value),
                        SetPropertyOutcome::Inserted => format!("{} → {} (added)", field, value),
                    });
                }
                Err(why) => errors.push(format!("'{}' on '{}': {why}", field, pending.reference)),
            }
        }
        if !pending.changes.is_empty() {
            changed.push(json!({
                "reference": pending.reference,
                "changes": pending.changes
            }));
        }
    }

    write_atomic_if_unchanged(&sch_path, &expected, &new_content)?;

    Ok(CallToolResult::json(&json!({
        "updated_count": changed.len(),
        "updated": changed,
        "errors": errors
    })))
}

async fn handle_batch_delete_components(
    args: &serde_json::Value,
    _ctx: &crate::tools::ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;

    let content = read_consistent(&sch_path)?;
    let expected = content.clone();
    let batch = match crate::tools::resolve_component_batch(&content, args, &sch_path) {
        Ok(batch) => batch,
        Err(result) => return Ok(*result),
    };
    let mut edits: Vec<SexpEdit> = Vec::new();
    let mut deleted: Vec<String> = Vec::new();
    // Same policy as a designator that names nothing: reported, batch goes on.
    let mut errors: Vec<String> = batch.unresolved;

    for reference in &batch.references {
        let reference = reference.as_str();
        // Every unit of the designator (P.6.8.2) — see `handle_batch_delete`.
        let blocks = find_all_symbol_blocks(&content, reference);
        if blocks.is_empty() {
            errors.push(format!("Component '{}' not found", reference));
            continue;
        }
        for (del_start, del_end) in blocks {
            edits.push(SexpEdit::delete(del_start, del_end));
        }
        deleted.push(reference.to_string());
    }

    let new_content = apply_edits(content, edits);
    write_atomic_if_unchanged(&sch_path, &expected, &new_content)?;

    Ok(CallToolResult::json(&json!({
        "deleted_count": deleted.len(),
        "deleted": deleted,
        "errors": errors
    })))
}

async fn handle_connect_passthrough(
    args: &serde_json::Value,
    _ctx: &crate::tools::ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
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
    let direction = opt_str(args, "direction").unwrap_or("right");
    let ((x, y), requested) = crate::tools::snap_reporting(x, y);

    // Stub is 2.54mm (2×1.27 grid units)
    let stub = 2.54_f64;
    let (wire_end_x, wire_end_y, label_rot) = match direction {
        "left" => (x - stub, y, 180.0),
        "up" => (x, y - stub, 90.0),
        "down" => (x, y + stub, 270.0),
        _ => (x + stub, y, 0.0), // "right" default
    };

    let wire_sexp = format_wire(x, y, wire_end_x, wire_end_y);
    let label_sexp = format_net_label(&net_name, wire_end_x, wire_end_y, label_rot);

    let content = read_consistent(&sch_path)?;
    let expected = content.clone();
    let close_pos = content.rfind(')').unwrap_or(content.len());
    let edits = vec![SexpEdit::insert(
        close_pos,
        format!("{wire_sexp}{label_sexp}"),
    )];
    let new_content = apply_edits(content, edits);
    write_atomic_if_unchanged(&sch_path, &expected, &new_content)?;

    let mut result = json!({
        "net": net_name,
        "stub_root": { "x": x, "y": y },
        "label_position": { "x": wire_end_x, "y": wire_end_y },
        "direction": direction,
        "label_rotation": label_rot
    });
    if let Some(requested) = requested {
        result["requested"] = requested;
        result["snapped_to_grid"] = json!(true);
    }
    Ok(CallToolResult::json(&result))
}

async fn handle_add_schematic_text(
    args: &serde_json::Value,
    _ctx: &crate::tools::ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
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
    let size = args["size"].as_f64().unwrap_or(1.27);
    let rotation = args["rotation"].as_f64().unwrap_or(0.0);
    let uuid = new_uuid();
    // Free-floating annotation text isn't electrically significant, but a
    // tool must not write off-grid coordinates silently — snap it like every
    // other schematic placement (E6).
    let ((x, y), requested) = crate::tools::snap_reporting(x, y);

    // Escape quotes in text content
    let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");

    let text_sexp = format!(
        "\n  (text \"{escaped}\"\n    (at {x} {y} {rotation})\n    \
         (effects (font (size {size} {size})))\n    (uuid \"{uuid}\")\n  )"
    );

    let content = read_consistent(&sch_path)?;
    let expected = content.clone();
    let close_pos = content.rfind(')').unwrap_or(content.len());
    let edits = vec![SexpEdit::insert(close_pos, text_sexp)];
    let new_content = apply_edits(content, edits);
    write_atomic_if_unchanged(&sch_path, &expected, &new_content)?;

    let mut result = json!({
        "added": text,
        "x": x, "y": y,
        "size": size,
        "rotation": rotation,
        "uuid": uuid
    });
    if let Some(requested) = requested {
        result["requested"] = requested;
        result["snapped_to_grid"] = json!(true);
    }
    Ok(CallToolResult::json(&result))
}

async fn handle_get_layout(
    args: &serde_json::Value,
    _ctx: &crate::tools::ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let include_wires = args["include_wires"].as_bool().unwrap_or(true);
    let include_labels = args["include_labels"].as_bool().unwrap_or(true);
    let include_junctions = args["include_junctions"].as_bool().unwrap_or(false);
    let include_no_connects = args["include_no_connects"].as_bool().unwrap_or(false);

    let (_, tree) = read_schematic(&sch_path)?;
    let instances = extract_symbol_instances(&tree);

    let components: Vec<serde_json::Value> = instances
        .iter()
        .map(|i| {
            json!({
                "reference": i.reference,
                "value": i.value,
                "lib_id": i.lib_id,
                "x": i.x, "y": i.y,
                "rotation": i.rotation,
                "mirror_x": i.mirror_x,
                "mirror_y": i.mirror_y
            })
        })
        .collect();

    // Bounding box over component origins
    let (mut min_x, mut min_y) = (f64::MAX, f64::MAX);
    let (mut max_x, mut max_y) = (f64::MIN, f64::MIN);
    for i in &instances {
        min_x = min_x.min(i.x);
        min_y = min_y.min(i.y);
        max_x = max_x.max(i.x);
        max_y = max_y.max(i.y);
    }
    let bbox = if instances.is_empty() {
        json!({ "x_min": 0, "y_min": 0, "x_max": 0, "y_max": 0 })
    } else {
        json!({ "x_min": min_x, "y_min": min_y, "x_max": max_x, "y_max": max_y })
    };

    let mut result = json!({
        "component_count": components.len(),
        "components": components,
        "bounding_box": bbox
    });

    if include_wires {
        let wires = extract_wires(&tree);
        let wire_data: Vec<serde_json::Value> = wires
            .iter()
            .map(|w| json!({ "x1": w.x1, "y1": w.y1, "x2": w.x2, "y2": w.y2, "uuid": w.uuid }))
            .collect();
        result["wire_count"] = json!(wire_data.len());
        result["wires"] = json!(wire_data);
    }

    if include_labels {
        let labels = extract_labels(&tree);
        let label_data: Vec<serde_json::Value> = labels
            .iter()
            .map(
                |l| json!({ "net": l.net, "type": format!("{:?}", l.kind), "x": l.x, "y": l.y, "uuid": l.uuid }),
            )
            .collect();
        result["label_count"] = json!(label_data.len());
        result["labels"] = json!(label_data);
    }

    // Junctions and no-connects are off by default: most callers of a layout
    // summary do not want them, and the reason they can be asked for at all is
    // that nothing else publishes their uuids — which is the only way to
    // address one on a document this caller did not just write (D.4.1.8).
    // Read through `cse`, which already models both with their identity, and
    // only when asked, so the default path still parses the document once.
    if include_junctions || include_no_connects {
        let sch = cse::Schematic::load(&sch_path)?;
        if include_junctions {
            let data: Vec<serde_json::Value> = sch
                .junctions
                .iter()
                .map(|j| json!({ "x": j.x, "y": j.y, "uuid": j.uuid }))
                .collect();
            result["junction_count"] = json!(data.len());
            result["junctions"] = json!(data);
        }
        if include_no_connects {
            let data: Vec<serde_json::Value> = sch
                .no_connects
                .iter()
                .map(|n| json!({ "x": n.x, "y": n.y, "uuid": n.uuid }))
                .collect();
            result["no_connect_count"] = json!(data.len());
            result["no_connects"] = json!(data);
        }
    }

    Ok(CallToolResult::json(&result))
}

async fn handle_validate_wire_connections(
    args: &serde_json::Value,
    _ctx: &crate::tools::ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let tol = args["tolerance"].as_f64().unwrap_or(0.01);

    let (_, tree) = read_schematic(&sch_path)?;
    let wires = extract_wires(&tree);
    let labels = extract_labels(&tree);
    let instances = extract_symbol_instances(&tree);
    let lib_syms = tree
        .find("lib_symbols")
        .map(|n| n.find_all("symbol"))
        .unwrap_or_default();

    // Collect all valid pin endpoints
    let mut pin_points: Vec<(f64, f64)> = Vec::new();
    for inst in &instances {
        let lib_sym = find_lib_symbol(&lib_syms, inst);
        if let Some(sym) = lib_sym {
            let t = inst.pin_transform();
            for pin in extract_lib_pins_for_unit(sym, inst.unit) {
                pin_points.push(pin_endpoint(&pin, t));
            }
        }
    }

    let label_points: Vec<(f64, f64)> = labels.iter().map(|l| (l.x, l.y)).collect();
    // All wire endpoints as a flat list (for quick counting)
    let all_wire_eps: Vec<(f64, f64)> = wires
        .iter()
        .flat_map(|w| [(w.x1, w.y1), (w.x2, w.y2)])
        .collect();

    let is_connected = |px: f64, py: f64| -> bool {
        // Another wire endpoint at the same position (count >= 2 because px/py itself is in the list)
        let same_ep_count = all_wire_eps
            .iter()
            .filter(|(wx, wy)| points_coincident(px, py, *wx, *wy, tol))
            .count();
        if same_ep_count >= 2 {
            return true;
        }

        // T-junction: lies on the INTERIOR of another wire
        if wires.iter().any(|w| {
            point_on_segment(px, py, w.x1, w.y1, w.x2, w.y2, tol)
                && !points_coincident(px, py, w.x1, w.y1, tol)
                && !points_coincident(px, py, w.x2, w.y2, tol)
        }) {
            return true;
        }

        // Label at this point
        if label_points
            .iter()
            .any(|(lx, ly)| points_coincident(px, py, *lx, *ly, tol))
        {
            return true;
        }

        // Pin endpoint at this point
        if pin_points
            .iter()
            .any(|(ppx, ppy)| points_coincident(px, py, *ppx, *ppy, tol))
        {
            return true;
        }

        false
    };

    let mut floating: Vec<serde_json::Value> = Vec::new();
    for w in &wires {
        for (px, py) in [(w.x1, w.y1), (w.x2, w.y2)] {
            if !is_connected(px, py) {
                floating.push(json!({ "x": px, "y": py, "wire_uuid": w.uuid }));
            }
        }
    }

    Ok(CallToolResult::json(&json!({
        "valid": floating.is_empty(),
        "floating_count": floating.len(),
        "floating_endpoints": floating
    })))
}

async fn handle_validate_component_connections(
    args: &serde_json::Value,
    _ctx: &crate::tools::ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let filter_refs: Vec<String> = args["references"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let tol = 0.01_f64;

    let (_, tree) = read_schematic(&sch_path)?;
    let instances = extract_symbol_instances(&tree);
    let wires = extract_wires(&tree);
    let labels = extract_labels(&tree);
    let lib_syms = tree
        .find("lib_symbols")
        .map(|n| n.find_all("symbol"))
        .unwrap_or_default();

    // No-connect positions (pins with intentional no-connect markers are exempt)
    let no_connect_pts: Vec<(f64, f64)> = tree
        .find_all("no_connect")
        .iter()
        .filter_map(|n| {
            let at = n.find("at")?;
            Some((at.get_f64(1)?, at.get_f64(2)?))
        })
        .collect();

    // Build net graph so we can check connectivity. Junctions matter: a pin
    // sitting mid-wire is connected only through a junction dot, so without
    // them this validator reports false "not connected" (#104).
    let junction_pts = konnect_sexp::schematic::extract_junction_points(&tree);
    let mut g = build_net_graph(&wires, &labels, &junction_pts);
    // Also build flat wire-endpoint list for direct presence checks
    let all_wire_eps: Vec<(f64, f64)> = wires
        .iter()
        .flat_map(|w| [(w.x1, w.y1), (w.x2, w.y2)])
        .collect();

    // `g.net_at` requires &mut self, so we need a `mut` closure.
    let mut has_connection = |px: f64, py: f64| -> bool {
        // Connected to a wire endpoint
        if all_wire_eps
            .iter()
            .any(|(wx, wy)| points_coincident(px, py, *wx, *wy, tol))
        {
            return true;
        }
        // A pin landing mid-wire connects only through a junction dot — KiCad's
        // netlister registers the unsplit wire at a junction point, so a dot
        // alone is enough and no wire split is required (#104).
        if junction_pts
            .iter()
            .any(|(jx, jy)| points_coincident(px, py, *jx, *jy, tol))
            && wires
                .iter()
                .any(|w| point_on_segment(px, py, w.x1, w.y1, w.x2, w.y2, tol))
        {
            return true;
        }
        // Or has a named net (label at or reachable from pin via wires)
        g.net_at(px, py).is_some()
    };

    let mut unconnected: Vec<serde_json::Value> = Vec::new();

    for inst in &instances {
        if !filter_refs.is_empty() && !filter_refs.contains(&inst.reference) {
            continue;
        }
        let lib_sym = find_lib_symbol(&lib_syms, inst);
        if let Some(sym) = lib_sym {
            let t = inst.pin_transform();
            for pin in extract_lib_pins_for_unit(sym, inst.unit) {
                let (px, py) = pin_endpoint(&pin, t);

                // Skip intentional no-connects
                if no_connect_pts
                    .iter()
                    .any(|(nx, ny)| points_coincident(px, py, *nx, *ny, tol))
                {
                    continue;
                }

                if !has_connection(px, py) {
                    unconnected.push(json!({
                        "reference": inst.reference,
                        "value": inst.value,
                        "pin": pin.number,
                        "pin_name": pin.name,
                        "x": px,
                        "y": py
                    }));
                }
            }
        }
    }

    Ok(CallToolResult::json(&json!({
        "valid": unconnected.is_empty(),
        "unconnected_count": unconnected.len(),
        "unconnected_pins": unconnected
    })))
}

#[cfg(test)]
mod batch_delete_tests {
    use super::*;
    use crate::router::ToolRouter;
    use crate::tools::{ServerConfig, ToolContext};
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
    async fn batch_delete_uuid_is_tab_indentation_safe_and_deduplicated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("batch-delete.kicad_sch");
        let uuid = "11111111-1111-1111-1111-111111111111";
        std::fs::write(
            &path,
            format!(
                "(kicad_sch\n\t(version 20260306)\n\t(generator \"eeschema\")\n\t(uuid \"root\")\n\t(wire\n\t\t(pts (xy 0 0) (xy 10 0))\n\t\t(uuid \"{uuid}\")\n\t)\n\t(text \"keep me\" (at 5 5 0) (uuid \"text\"))\n\t(sheet_instances (path \"/\" (page \"1\")))\n)\n"
            ),
        )
        .unwrap();

        let result = handle_batch_delete(
            &json!({
                "schematic": path.display().to_string(),
                "uuids": [uuid, "root", uuid]
            }),
            &test_ctx(),
        )
        .await
        .unwrap();
        assert!(!result.is_error);

        let after = std::fs::read_to_string(&path).unwrap();
        assert!(!after.contains(uuid));
        assert!(after.contains("(uuid \"root\")"));
        assert!(after.contains("keep me"));
        assert!(after.contains("(sheet_instances"));
        assert!(konnect_sexp::parse_sexp(&after).is_ok());
    }

    #[tokio::test]
    async fn batch_delete_uuid_removes_top_level_text_but_preserves_structure() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("batch-delete-text.kicad_sch");
        let text_uuid = "22222222-2222-2222-2222-222222222222";
        std::fs::write(
            &path,
            format!(
                "(kicad_sch\n  (version 20260306)\n  (generator \"eeschema\")\n  (uuid \"root\")\n  (text \"obsolete caption\"\n    (at 5 5 0)\n    (effects (font (size 1.27 1.27)))\n    (uuid \"{text_uuid}\")\n  )\n  (sheet_instances (path \"/\" (page \"1\")))\n)\n"
            ),
        )
        .unwrap();

        let result = handle_batch_delete(
            &json!({
                "schematic": path.display().to_string(),
                "uuids": [text_uuid]
            }),
            &test_ctx(),
        )
        .await
        .unwrap();
        assert!(!result.is_error);

        let after = std::fs::read_to_string(&path).unwrap();
        assert!(!after.contains("obsolete caption"));
        assert!(after.contains("(uuid \"root\")"));
        assert!(after.contains("(sheet_instances"));
        assert!(konnect_sexp::parse_sexp(&after).is_ok());
    }

    /// Locks the reason this handler keeps its textual UUID search instead of
    /// the direct-child index of D.4.1.1: a UUID *nested* inside a top-level
    /// item — here a sheet pin's own — has always deleted the enclosing item,
    /// and the index would answer `NotFound` for it (INV8).
    #[tokio::test]
    async fn batch_delete_accepts_a_uuid_nested_inside_the_deleted_item() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("batch-delete-nested.kicad_sch");
        let pin_uuid = "33333333-3333-3333-3333-333333333333";
        std::fs::write(
            &path,
            format!(
                "(kicad_sch
	(version 20260306)
	(generator \"eeschema\")
	(uuid \"root\")
	(sheet
		(at 10 10 0)
		(uuid \"44444444-4444-4444-4444-444444444444\")
		(pin \"CLK\" input (at 0 0 0)
			(uuid \"{pin_uuid}\")
		)
	)
	(sheet_instances (path \"/\" (page \"1\")))
)
"
            ),
        )
        .unwrap();

        let result = handle_batch_delete(
            &json!({
                "schematic": path.display().to_string(),
                "uuids": [pin_uuid]
            }),
            &test_ctx(),
        )
        .await
        .unwrap();
        assert!(!result.is_error);

        let after = std::fs::read_to_string(&path).unwrap();
        assert!(!after.contains("44444444-4444-4444-4444-444444444444"));
        assert!(!after.contains(pin_uuid));
        assert!(after.contains("(uuid \"root\")"));
        assert!(after.contains("(sheet_instances"));
        assert!(konnect_sexp::parse_sexp(&after).is_ok());
    }
}

#[cfg(test)]
mod batch_place_and_connect_tests {
    use super::*;
    use crate::router::ToolRouter;
    use crate::tools::{ServerConfig, ToolContext};
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

    // Pre-seed lib_symbols so ensure_lib_symbol short-circuits without KiCad
    // (precedent: sch_components.rs add_schematic_component_hides_power_reference).
    const DEVICE_R: &str = "    (symbol \"Device:R\"\n      (property \"Reference\" \"R\" (at 0 0 0))\n      (property \"Value\" \"R\" (at 0 0 0))\n    )\n";

    fn seeded_schematic() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("place.kicad_sch");
        std::fs::write(
            &path,
            format!(
                "(kicad_sch\n  (version 20250610)\n  (generator \"konnect\")\n  (uuid \"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee\")\n  (paper \"A4\")\n  (lib_symbols\n{DEVICE_R}  )\n)\n"
            ),
        )
        .unwrap();
        (dir, path)
    }

    #[tokio::test]
    async fn batch_place_components_dedupes_lib_symbols() {
        let (_d, path) = seeded_schematic();
        let result = handle_batch_place_components(
            &json!({
                "schematic": path.display().to_string(),
                "components": [
                    { "lib_id": "Device:R", "x": 100.0, "y": 100.0, "reference": "R1" },
                    { "lib_id": "Device:R", "x": 110.0, "y": 100.0, "reference": "R2" }
                ]
            }),
            &test_ctx(),
        )
        .await
        .unwrap();
        assert!(!result.is_error, "{result:?}");

        let sch = cse::Schematic::load(&path).unwrap();
        assert!(sch.symbols.by_reference("R1").is_some());
        assert!(sch.symbols.by_reference("R2").is_some());

        let after = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            after.matches("(symbol \"Device:R\"").count(),
            1,
            "lib_symbols entry must not be duplicated: {after}"
        );
    }

    #[tokio::test]
    async fn batch_place_components_collects_per_item_errors() {
        let (_d, path) = seeded_schematic();
        let result = handle_batch_place_components(
            &json!({
                "schematic": path.display().to_string(),
                "components": [
                    { "lib_id": "Device:R", "x": 100.0, "y": 100.0, "reference": "R1" },
                    { "lib_id": "Nonexistent_xyzzy:Foo", "x": 110.0, "y": 100.0, "reference": "R2" },
                    { "lib_id": "Device:R", "x": 120.0, "y": 100.0, "reference": "R3" }
                ]
            }),
            &test_ctx(),
        )
        .await
        .unwrap();
        assert!(!result.is_error, "{result:?}");

        let body = match &result.content[0] {
            crate::mcp::protocol::ToolContent::Text { text } => text.clone(),
            _ => panic!("expected text"),
        };
        let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed["placed_count"], 2);
        assert_eq!(parsed["errors"].as_array().unwrap().len(), 1);

        let sch = cse::Schematic::load(&path).unwrap();
        assert!(sch.symbols.by_reference("R1").is_some());
        assert!(sch.symbols.by_reference("R3").is_some());
        assert!(sch.symbols.by_reference("R2").is_none());
    }

    #[tokio::test]
    async fn batch_place_components_total_failure_sets_is_error() {
        let (_d, path) = seeded_schematic();
        let result = handle_batch_place_components(
            &json!({
                "schematic": path.display().to_string(),
                "components": [
                    { "lib_id": "Nonexistent_xyzzy:Foo", "x": 100.0, "y": 100.0, "reference": "R1" }
                ]
            }),
            &test_ctx(),
        )
        .await
        .unwrap();
        assert!(result.is_error, "{result:?}");
    }

    /// Six single-pin instances of a synthetic part, positioned so that
    /// connecting them by pin pairs produces a T-junction on the second pair.
    fn multi_point_schematic() -> (tempfile::TempDir, std::path::PathBuf) {
        let pin_def = "\t\t\t(pin passive line (at 0 0 0) (length 0)\n\t\t\t\t(name \"~\" (effects (font (size 1.27 1.27))))\n\t\t\t\t(number \"1\" (effects (font (size 1.27 1.27))))\n\t\t\t)\n";
        let lib_sym = format!("\t\t(symbol \"Test:PT\"\n{pin_def}\t\t)\n");
        let inst = |reference: &str, x: f64, y: f64, uuid: &str| {
            format!(
                "\t(symbol\n\t\t(lib_id \"Test:PT\")\n\t\t(at {x} {y} 0)\n\t\t(uuid \"{uuid}\")\n\t\t(property \"Reference\" \"{reference}\"\n\t\t\t(at {x} {y} 0)\n\t\t)\n\t)\n"
            )
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("points.kicad_sch");
        std::fs::write(
            &path,
            format!(
                "(kicad_sch\n\t(version 20250610)\n\t(generator \"konnect\")\n\t(uuid \"3af69a4c-1faa-40bd-91dc-c4fc245c4cbd\")\n\t(lib_symbols\n{}\t)\n{}{}{}{}{}{})\n",
                lib_sym,
                inst("R1", 100.0, 100.0, "aaaaaaaa-0000-0000-0000-000000000001"),
                inst("R2", 120.0, 100.0, "aaaaaaaa-0000-0000-0000-000000000002"),
                inst("R3", 110.0, 80.0, "aaaaaaaa-0000-0000-0000-000000000003"),
                inst("R4", 110.0, 100.0, "aaaaaaaa-0000-0000-0000-000000000004"),
                inst("R5", 200.0, 100.0, "aaaaaaaa-0000-0000-0000-000000000005"),
                inst("R6", 220.0, 100.0, "aaaaaaaa-0000-0000-0000-000000000006"),
            ),
        )
        .unwrap();
        (dir, path)
    }

    #[tokio::test]
    async fn batch_connect_pins_dedupes_junction_and_collects_errors() {
        // R3-R4's wire T-lands on R1-R2's wire at (110, 100) -- without the
        // STEP 1 fix, processing the third connection re-detects that same
        // T-junction from the raw wire list and inserts a second dot.
        let (_d, path) = multi_point_schematic();
        let result = handle_batch_connect_pins(
            &json!({
                "schematic": path.display().to_string(),
                "connections": [
                    { "ref1": "R1", "pin1": "1", "ref2": "R2", "pin2": "1" },
                    { "ref1": "R3", "pin1": "1", "ref2": "R4", "pin2": "1" },
                    { "ref1": "R5", "pin1": "1", "ref2": "R6", "pin2": "1" },
                    { "ref1": "Rbad", "pin1": "1", "ref2": "R6", "pin2": "1" }
                ]
            }),
            &test_ctx(),
        )
        .await
        .unwrap();
        assert!(!result.is_error, "{result:?}");

        let after = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            after.matches("(junction").count(),
            1,
            "the T-junction at (110, 100) must not be re-inserted: {after}"
        );

        let body = match &result.content[0] {
            crate::mcp::protocol::ToolContent::Text { text } => text.clone(),
            _ => panic!("expected text"),
        };
        let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed["connected_count"], 3);
        assert_eq!(parsed["errors"].as_array().unwrap().len(), 1);
    }
}

#[cfg(test)]
mod midwire_pin_tests {
    use super::*;
    use crate::router::ToolRouter;
    use crate::tools::{ServerConfig, ToolContext};
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

    /// U1 has a single pin at (100,80), sitting strictly mid-segment on a wire
    /// from (90,80) to (110,80).
    fn midwire_schematic(with_junction: bool) -> (tempfile::TempDir, std::path::PathBuf) {
        let junction = if with_junction {
            "\t(junction (at 100 80) (diameter 0) (color 0 0 0 0) (uuid \"j1\"))\n"
        } else {
            ""
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("midwire.kicad_sch");
        std::fs::write(
            &path,
            format!(
                "(kicad_sch\n\t(version 20260306)\n\t(generator \"eeschema\")\n\t(uuid \"root\")\n\t(lib_symbols\n\t\t(symbol \"Test:P1\"\n\t\t\t(symbol \"P1_1_1\"\n\t\t\t\t(pin passive line (at 0 0 0) (length 2.54)\n\t\t\t\t\t(name \"~\" (effects (font (size 1.27 1.27))))\n\t\t\t\t\t(number \"1\" (effects (font (size 1.27 1.27))))\n\t\t\t\t)\n\t\t\t)\n\t\t)\n\t)\n\t(wire\n\t\t(pts (xy 90 80) (xy 110 80))\n\t\t(uuid \"w1\")\n\t)\n{junction}\t(symbol\n\t\t(lib_id \"Test:P1\")\n\t\t(at 100 80 0)\n\t\t(unit 1)\n\t\t(uuid \"u1\")\n\t\t(property \"Reference\" \"U1\"\n\t\t\t(at 100 75 0)\n\t\t)\n\t)\n\t(sheet_instances (path \"/\" (page \"1\")))\n)\n"
            ),
        )
        .unwrap();
        (dir, path)
    }

    /// KiCad connects a pin mid-wire only through a junction dot; the
    /// validator must mirror that instead of demanding a wire endpoint.
    #[tokio::test]
    async fn midwire_pin_connects_with_junction_only() {
        for (with_junction, expect_valid) in [(true, true), (false, false)] {
            let (_d, path) = midwire_schematic(with_junction);
            let result = handle_validate_component_connections(
                &json!({ "schematic": path.display().to_string() }),
                &test_ctx(),
            )
            .await
            .unwrap();
            let crate::mcp::protocol::ToolContent::Text { text } = &result.content[0] else {
                panic!("expected text content");
            };
            let body: serde_json::Value = serde_json::from_str(text).unwrap();
            assert_eq!(
                body["valid"].as_bool(),
                Some(expect_valid),
                "with_junction={with_junction}: {body}"
            );
        }
    }
}

#[cfg(test)]
mod bulk_move_property_tests {
    use super::*;
    use crate::router::ToolRouter;
    use crate::tools::{ServerConfig, ToolContext};
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

    const DEVICE_R: &str = "    (symbol \"Device:R\"\n      (property \"Reference\" \"R\" (at 0 0 0))\n      (property \"Value\" \"R\" (at 0 0 0))\n    )\n";

    fn seeded_schematic() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("move.kicad_sch");
        std::fs::write(
            &path,
            format!(
                "(kicad_sch\n  (version 20250610)\n  (generator \"konnect\")\n  (uuid \"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee\")\n  (paper \"A4\")\n  (lib_symbols\n{DEVICE_R}  )\n)\n"
            ),
        )
        .unwrap();
        (dir, path)
    }

    /// Byte range (relative to `content`) of the coordinate text inside the
    /// `(at …)` of the direct-child property named `field` on symbol
    /// `reference`, for asserting on where a field's text ended up.
    fn property_at_text<'a>(content: &'a str, reference: &str, field: &str) -> &'a str {
        let (start, end) = find_symbol_instance_block(content, reference).expect("symbol");
        let prop = crate::tools::find_symbol_property(content, start, end, field)
            .unwrap_or_else(|| panic!("{field} property"));
        // find_symbol_property gives the *value* span; walk forward to this
        // property's own `(at …)` instead.
        let spans = crate::tools::symbol_property_at_spans(content, start, end);
        let value_line_end = content[prop.value_end..]
            .find('\n')
            .map(|o| prop.value_end + o)
            .unwrap_or(prop.value_end);
        spans
            .into_iter()
            .find(|&(s, _)| s > prop.value_end && s < value_line_end + 200)
            .map(|(s, e)| &content[s..e])
            .unwrap_or_else(|| panic!("{field} has no (at …) child"))
    }

    /// Before the fix, `handle_bulk_move` rewrote only the symbol's own `(at
    /// …)`; every property's field text — which carries its own absolute
    /// `(at x y rot)` — stayed where the symbol used to be.
    #[tokio::test]
    async fn bulk_move_shifts_property_anchors_by_the_snapped_delta() {
        let (_d, path) = seeded_schematic();
        handle_batch_place_components(
            &json!({
                "schematic": path.display().to_string(),
                "components": [
                    { "lib_id": "Device:R", "x": 100.3, "y": 100.3, "reference": "R1" }
                ]
            }),
            &test_ctx(),
        )
        .await
        .unwrap();

        let before = std::fs::read_to_string(&path).unwrap();
        let value_before = property_at_text(&before, "R1", "Value");
        let value_before_xy: Vec<f64> = value_before
            .split_whitespace()
            .take(2)
            .map(|s| s.parse().unwrap())
            .collect();
        let value_rot_before = value_before.split_whitespace().nth(2).unwrap().to_string();

        let sch = cse::Schematic::load(&path).unwrap();
        let sym = sch.symbols.by_reference("R1").unwrap();
        let (sym_x_before, sym_y_before) = sym.position();

        let result = handle_bulk_move(
            &json!({ "schematic": path.display().to_string(), "references": ["R1"], "dx": 5.0, "dy": 5.0 }),
            &test_ctx(),
        )
        .await
        .unwrap();
        assert!(!result.is_error, "{result:?}");

        let sch = cse::Schematic::load(&path).unwrap();
        let sym = sch.symbols.by_reference("R1").unwrap();
        let (sym_x_after, sym_y_after) = sym.position();
        let applied_dx = sym_x_after - sym_x_before;
        let applied_dy = sym_y_after - sym_y_before;
        // Grid-snapped, so not exactly (5.0, 5.0) — the point of using the
        // *applied* delta rather than the raw dx/dy.
        assert_ne!((applied_dx, applied_dy), (0.0, 0.0));

        let after = std::fs::read_to_string(&path).unwrap();
        let value_after = property_at_text(&after, "R1", "Value");
        let value_after_xy: Vec<f64> = value_after
            .split_whitespace()
            .take(2)
            .map(|s| s.parse().unwrap())
            .collect();
        let value_rot_after = value_after.split_whitespace().nth(2).unwrap().to_string();

        assert!(
            (value_after_xy[0] - (value_before_xy[0] + applied_dx)).abs() < 1e-6,
            "Value field x must move by the applied delta: before={value_before_xy:?} after={value_after_xy:?} applied_dx={applied_dx}"
        );
        assert!(
            (value_after_xy[1] - (value_before_xy[1] + applied_dy)).abs() < 1e-6,
            "Value field y must move by the applied delta"
        );
        assert_eq!(
            value_rot_after, value_rot_before,
            "a field's own rotation must not change on a move"
        );
        assert_ne!(
            (sym_x_before, sym_y_before),
            (0.0, 0.0),
            "sanity: symbol actually has a nonzero starting position"
        );
    }

    /// A symbol block whose `(at` is never closed with `)` must produce an
    /// error, not panic on a slice with `end < start`.
    ///
    /// `symbol_own_at_span` is exercised directly (not through
    /// `handle_bulk_move`) because reaching this from the handler would
    /// require a `sym_start..sym_end` range whose text has no `)` at all —
    /// impossible for a range `find_symbol_instance_block` itself hands
    /// back, since that range always comes from a balanced-block finder and
    /// therefore always ends in `)`. The scan must not rely on that
    /// guarantee to stay panic-free, which is exactly what this proves.
    #[test]
    fn symbol_own_at_span_reports_unterminated_at_instead_of_panicking() {
        let content = "(symbol\n  (lib_id \"Device:R\")\n  (at 10 10 0";
        let result = symbol_own_at_span(content, 0, content.len());
        assert!(
            matches!(&result, Err(msg) if msg.contains("Unterminated")),
            "{result:?}"
        );
    }

    #[test]
    fn symbol_own_at_span_finds_the_symbols_own_at_not_a_propertys() {
        let content = "(symbol\n  (lib_id \"Device:R\")\n  (at 10 10 90)\n  (property \"Reference\" \"R1\"\n    (at 12 8 0)\n  )\n)";
        let (start, end) = symbol_own_at_span(content, 0, content.len()).unwrap();
        assert_eq!(&content[start..end], "10 10 90");
    }

    /// A property anchor is moved by a plain addition, so unlike the symbol's
    /// own anchor it is not protected by `snap_point`. Adding the applied
    /// delta to a field at 241.3 yields `246.38000000000002` in binary
    /// floating point, and writing that puts noise in the file for a byte the
    /// caller never asked to change — the class of damage P.6.9.4 removed.
    #[tokio::test]
    async fn bulk_move_does_not_leak_float_noise_into_property_anchors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("noise.kicad_sch");
        // Written by hand rather than placed through the handler: the noise
        // only appears for particular coordinate/delta pairs, and the point
        // of this test is to pin one of them rather than hope placement
        // happens to produce one. 139.7 + 5 snaps to 144.78, an applied
        // delta of 5.0800000000000125.
        let sheet = "(kicad_sch\n\t(version 20250114)\n\t(generator \"konnect\")\n\t(uuid \"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee\")\n\t(paper \"A4\")\n\t(lib_symbols\n\t)\n\t(symbol\n\t\t(lib_id \"Device:R\")\n\t\t(at 139.7 100.33 0)\n\t\t(unit 1)\n\t\t(uuid \"bbbbbbbb-cccc-dddd-eeee-ffffffffffff\")\n\t\t(property \"Reference\" \"R1\"\n\t\t\t(at 241.3 3.556 0)\n\t\t)\n\t\t(property \"Value\" \"10k\"\n\t\t\t(at 100.33 241.3 0)\n\t\t)\n\t)\n)\n";
        std::fs::write(&path, sheet).unwrap();

        let result = handle_bulk_move(
            &json!({ "schematic": path.display().to_string(), "references": ["R1"], "dx": 5.0, "dy": 0.0 }),
            &test_ctx(),
        )
        .await
        .unwrap();
        assert!(!result.is_error, "{result:?}");

        let after = std::fs::read_to_string(&path).unwrap();
        let noisy: Vec<&str> = after
            .split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
            .filter(|tok| {
                tok.split_once('.').is_some_and(|(_, frac)| {
                    frac.len() > 6 && frac.chars().all(|c| c.is_ascii_digit())
                })
            })
            .collect();
        assert!(
            noisy.is_empty(),
            "float noise written into the sheet: {noisy:?}\n{after}"
        );
        // And the move itself still happened, so the assertion above is not
        // passing because nothing was written.
        assert!(
            after.contains("(at 144.78 100.33 0)"),
            "the symbol did not move as expected:\n{after}"
        );
    }

    /// A move that snaps back to the symbol's current position must leave the
    /// file byte-identical: an edit that changes nothing still shows up, since
    /// a `(at x y)` with no rotation would be rewritten as `(at x y 0)`.
    #[tokio::test]
    async fn a_move_that_snaps_to_a_standstill_rewrites_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("standstill.kicad_sch");
        let sheet = "(kicad_sch\n\t(version 20250114)\n\t(generator \"konnect\")\n\t(uuid \"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee\")\n\t(paper \"A4\")\n\t(lib_symbols\n\t)\n\t(symbol\n\t\t(lib_id \"Device:R\")\n\t\t(at 139.7 100.33 0)\n\t\t(unit 1)\n\t\t(uuid \"bbbbbbbb-cccc-dddd-eeee-ffffffffffff\")\n\t\t(property \"Reference\" \"R1\"\n\t\t\t(at 241.3 3.556)\n\t\t)\n\t)\n)\n";
        std::fs::write(&path, sheet).unwrap();

        // Well under half a grid step, so `snap_point` returns the symbol
        // where it already is.
        let result = handle_bulk_move(
            &json!({ "schematic": path.display().to_string(), "references": ["R1"], "dx": 0.1, "dy": 0.0 }),
            &test_ctx(),
        )
        .await
        .unwrap();
        assert!(!result.is_error, "{result:?}");

        let after = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            after, sheet,
            "a standstill move rewrote the sheet instead of leaving it alone"
        );
    }
}

#[cfg(test)]
mod multiunit_connect_tests {
    //! P.6.8.1: `batch_connect_to_net`'s instance lookup picked the *first*
    //! `SymbolInstance` matching a reference, ignoring `unit`. On a
    //! multi-unit symbol (two `U1` entries, one per unit) that always meant
    //! unit 1's placement, even for a pin unit 1 doesn't have. The
    //! `Amplifier_Operational:LM2904` fixture makes this measurable: unit 1's
    //! pin 3 and unit 2's pin 5 sit at the identical local point `(-7.62,
    //! 2.54)`, so the old code silently mislabeled unit 2's pin 5 at unit 1's
    //! pin 3 position — a wrong answer that looked plausible.
    use super::*;
    use crate::router::ToolRouter;
    use crate::tools::{ServerConfig, ToolContext};
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

    const MULTIUNIT_LM2904: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/multiunit_lm2904.kicad_sch"
    );

    fn temp_copy_of_fixture(name: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        std::fs::copy(MULTIUNIT_LM2904, &path).unwrap();
        (dir, path)
    }

    /// Red before the fix: pin 5 (unit 2, at x=160) resolved through unit 1's
    /// placement (x=100) because both `U1` instances tied for the first
    /// `find()` match, and unit 1 happens to declare no pin 5 — except the
    /// unit-blind `extract_lib_pins` would have reported one anyway by
    /// superimposing unit 2's pins onto unit 1. The fixed lookup must place
    /// pin 5's label near x=160 (unit 2's placement), and explicitly not at
    /// unit 1's pin 3 coordinate.
    #[tokio::test]
    async fn pin_5_connects_through_its_own_unit_not_unit_1() {
        let (_dir, path) = temp_copy_of_fixture("pin5.kicad_sch");
        let result = handle_batch_connect_to_net(
            &json!({
                "schematic": path.display().to_string(),
                "net_name": "NET5",
                "pins": [{ "reference": "U1", "pin_number": "5" }]
            }),
            &test_ctx(),
        )
        .await
        .unwrap();
        assert!(!result.is_error, "{result:?}");

        let crate::mcp::protocol::ToolContent::Text { text } = &result.content[0] else {
            panic!("expected text result");
        };
        let out: serde_json::Value = serde_json::from_str(text).unwrap();
        assert_eq!(out["errors"].as_array().unwrap().len(), 0, "{out}");
        let added = &out["added"][0];
        let px = added["x"].as_f64().unwrap();

        // Unit 1 sits at x=100, unit 2 at x=160 (see the fixture). Pin 3
        // (unit 1) and pin 5 (unit 2) share the same *local* point, so a
        // resolution through unit 1 lands near x=100 - 7.62, not near
        // x=160 - 7.62.
        assert!(
            (px - (160.0 - 7.62)).abs() < 0.01,
            "pin 5 must resolve through unit 2's placement (x~=152.38), got x={px} \
             (x~=92.38 would mean it was mislabeled onto unit 1's pin 3, #P.6.8.1)"
        );
        assert!(
            (px - (100.0 - 7.62)).abs() > 1.0,
            "pin 5's label landed on unit 1's pin 3 position (x={px}) — the exact \
             regression #P.6.8.1 describes"
        );
    }

    /// Mirror of the test above: the fix must not disturb what already
    /// worked. Pin 3 (unit 1) still resolves through unit 1's own placement.
    #[tokio::test]
    async fn pin_3_on_unit_1_is_unaffected() {
        let (_dir, path) = temp_copy_of_fixture("pin3.kicad_sch");
        let result = handle_batch_connect_to_net(
            &json!({
                "schematic": path.display().to_string(),
                "net_name": "NET3",
                "pins": [{ "reference": "U1", "pin_number": "3" }]
            }),
            &test_ctx(),
        )
        .await
        .unwrap();
        assert!(!result.is_error, "{result:?}");

        let crate::mcp::protocol::ToolContent::Text { text } = &result.content[0] else {
            panic!("expected text result");
        };
        let out: serde_json::Value = serde_json::from_str(text).unwrap();
        assert_eq!(out["errors"].as_array().unwrap().len(), 0, "{out}");
        let added = &out["added"][0];
        let px = added["x"].as_f64().unwrap();

        assert!(
            (px - (100.0 - 7.62)).abs() < 0.01,
            "pin 3 (unit 1) must still resolve through unit 1's placement (x~=92.38), got x={px}"
        );
    }

    /// A pin number that exists on neither unit still errors by name, not
    /// silently — the multi-candidate loop must not swallow a genuine miss.
    #[tokio::test]
    async fn a_pin_absent_from_every_unit_still_errors() {
        let (_dir, path) = temp_copy_of_fixture("missing.kicad_sch");
        let result = handle_batch_connect_to_net(
            &json!({
                "schematic": path.display().to_string(),
                "net_name": "NETX",
                "pins": [{ "reference": "U1", "pin_number": "99" }]
            }),
            &test_ctx(),
        )
        .await
        .unwrap();
        assert!(!result.is_error, "{result:?}");

        let crate::mcp::protocol::ToolContent::Text { text } = &result.content[0] else {
            panic!("expected text result");
        };
        let out: serde_json::Value = serde_json::from_str(text).unwrap();
        let errors: Vec<&str> = out["errors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e.as_str().unwrap())
            .collect();
        assert_eq!(errors.len(), 1, "{out}");
        assert!(errors[0].contains("99"), "{errors:?}");
        assert!(errors[0].contains("U1"), "{errors:?}");
    }
}
