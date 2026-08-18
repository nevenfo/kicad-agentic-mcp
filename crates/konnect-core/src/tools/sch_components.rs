//! `sch_components` toolset — add, edit, move, rotate, delete schematic symbols.
//!
//! Simple CRUD operations use `konnect_schematic_editor` (cse) for structured
//! round-trip parsing.  Pin coordinate math still delegates to
//! `konnect_sexp::geometry::transform_pin`.

use crate::mcp::protocol::CallToolResult;
use crate::tool;
use crate::tools::{
    find_symbol_instance_block, get_path, opt_f64, opt_str, project_name_for, require_f64,
    require_str, ToolContext, ToolDef,
};
use konnect_schematic_editor as cse;
use konnect_sexp::{
    commit_command,
    geometry::snap_point,
    parse_sexp,
    schematic::{
        extract_lib_pins_for_unit, extract_symbol_instances, pin_endpoint, read_schematic,
    },
    writer::{
        apply_edits, new_uuid, read_consistent, write_atomic_if_unchanged, write_new_atomic,
        SexpEdit,
    },
    ItemId, SchematicCommand,
};
use serde_json::json;

pub fn tools() -> Vec<ToolDef> {
    vec![
        tool!(
            "create_schematic",
            "Create a new blank .kicad_sch schematic file.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Full path for the new .kicad_sch file" }
                },
                "required": ["path"]
            }),
            |args, ctx| async move { handle_create_schematic(args, ctx).await }
        ),
        tool!(
            "add_schematic_component",
            "Add a symbol from a KiCAD library to the schematic. The symbol is snapped \
             to the 1.27mm schematic grid. Specify position in schematic mm coordinates.",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string", "description": "Path to .kicad_sch file" },
                    "lib_id": { "type": "string", "description": "Library:Symbol (e.g. 'Device:R')" },
                    "x": { "type": "number", "description": "X position in mm" },
                    "y": { "type": "number", "description": "Y position in mm" },
                    "rotation": { "type": "number", "description": "Rotation in degrees (0/90/180/270)", "default": 0 },
                    "reference": { "type": "string", "description": "Optional override for reference designator" },
                    "value": { "type": "string", "description": "Optional override for value field" },
                    "unit": { "type": "integer", "description": "Unit number for multi-unit symbols (gate/part selection). Default 1.", "default": 1 }
                },
                "required": ["schematic", "lib_id", "x", "y"]
            }),
            |args, ctx| async move { handle_add_schematic_component(args, ctx).await }
        ),
        tool!(
            "delete_schematic_component",
            "Remove a symbol instance from the schematic by its reference designator.",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string" },
                    "reference": { "type": "string", "description": "Reference designator (e.g. 'R1')" }
                },
                "required": ["schematic", "reference"]
            }),
            |args, ctx| async move { handle_delete_schematic_component(args, ctx).await }
        ),
        tool!(
            "edit_schematic_component",
            "Update fields (Reference, Value, Footprint, custom properties) of a symbol instance.",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string" },
                    "reference": { "type": "string", "description": "Current reference designator" },
                    "new_reference": { "type": "string", "description": "New reference designator (optional)" },
                    "value": { "type": "string", "description": "New value (optional)" },
                    "footprint": { "type": "string", "description": "New footprint (optional)" },
                    "datasheet": { "type": "string", "description": "New datasheet URL (optional)" },
                    "fields": {
                        "type": "object",
                        "description": "Additional property fields to set as key:value pairs"
                    }
                },
                "required": ["schematic", "reference"]
            }),
            |args, ctx| async move { handle_edit_schematic_component(args, ctx).await }
        ),
        tool!(
            "get_schematic_component",
            "Get all properties, position, and pin locations for a single schematic \
             component, that is one symbol instance, looked up by its reference \
             designator.",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string" },
                    "reference": { "type": "string" }
                },
                "required": ["schematic", "reference"]
            }),
            |args, ctx| async move { handle_get_schematic_component(args, ctx).await }
        ),
        tool!(
            "list_schematic_components",
            "List all symbol instances in a schematic with their positions, values, \
             footprints, and pin locations.",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string" }
                },
                "required": ["schematic"]
            }),
            |args, ctx| async move { handle_list_schematic_components(args, ctx).await }
        ),
        tool!(
            "move_schematic_component",
            "Move a symbol to a new position. Does NOT adjust connected wires.",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string" },
                    "reference": { "type": "string" },
                    "x": { "type": "number", "description": "New X position in mm" },
                    "y": { "type": "number", "description": "New Y position in mm" }
                },
                "required": ["schematic", "reference", "x", "y"]
            }),
            |args, ctx| async move { handle_move_schematic_component(args, ctx).await }
        ),
        tool!(
            "rotate_schematic_component",
            "Rotate a symbol by setting its absolute rotation angle (0/90/180/270).",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string" },
                    "reference": { "type": "string" },
                    "rotation": { "type": "number", "description": "Absolute rotation in degrees" }
                },
                "required": ["schematic", "reference", "rotation"]
            }),
            |args, ctx| async move { handle_rotate_schematic_component(args, ctx).await }
        ),
        tool!(
            "move_connected",
            "Move a symbol and drag the wire ends that were sitting on its pins along with it, \
             so the connections survive the move. Reports how many wire ends were dragged; \
             wires that touched nothing are left where they were drawn.",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string" },
                    "reference": { "type": "string" },
                    "x": { "type": "number" },
                    "y": { "type": "number" }
                },
                "required": ["schematic", "reference", "x", "y"]
            }),
            |args, ctx| async move { handle_move_connected(args, ctx).await }
        ),
        tool!(
            "move_region",
            "Move all symbols within a bounding box by a given offset.",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string" },
                    "x1": { "type": "number", "description": "Region bounding box min X" },
                    "y1": { "type": "number", "description": "Region bounding box min Y" },
                    "x2": { "type": "number", "description": "Region bounding box max X" },
                    "y2": { "type": "number", "description": "Region bounding box max Y" },
                    "dx": { "type": "number", "description": "X offset to move by" },
                    "dy": { "type": "number", "description": "Y offset to move by" }
                },
                "required": ["schematic", "x1", "y1", "x2", "y2", "dx", "dy"]
            }),
            |args, ctx| async move { handle_move_region(args, ctx).await }
        ),
        tool!(
            "annotate_schematic",
            "Run kicad-cli to auto-assign reference designators (R? → R1, U? → U1, etc.).",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string" }
                },
                "required": ["schematic"]
            }),
            |args, ctx| async move { handle_annotate_schematic(args, ctx).await }
        ),
        tool!(
            "get_schematic_pin_locations",
            "Get the exact schematic-space (X,Y) coordinates showing where every pin \
             of a component symbol is located, accounting for rotation and mirroring. \
             Uses the canonical pin transform.",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string" },
                    "reference": { "type": "string" }
                },
                "required": ["schematic", "reference"]
            }),
            |args, ctx| async move { handle_get_schematic_pin_locations(args, ctx).await }
        ),
        tool!(
            "batch_get_schematic_pin_locations",
            "Get pin locations for multiple components in a single file read.",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string" },
                    "references": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "List of reference designators"
                    }
                },
                "required": ["schematic", "references"]
            }),
            |args, ctx| async move { handle_batch_get_pin_locations(args, ctx).await }
        ),
        tool!(
            "add_component_annotation",
            "Add a custom property (annotation) to a symbol instance in the schematic.",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string", "description": "Path to .kicad_sch file" },
                    "reference": { "type": "string", "description": "Component reference designator (e.g. 'R1')" },
                    "key": { "type": "string", "description": "Property name" },
                    "value": { "type": "string", "description": "Property value" }
                },
                "required": ["schematic", "reference", "key", "value"]
            }),
            |args, ctx| async move { handle_add_component_annotation(args, ctx).await }
        ),
        tool!(
            "group_components",
            "Add a group property to multiple components in the schematic.",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string", "description": "Path to .kicad_sch file" },
                    "references": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "List of reference designators to group"
                    },
                    "group_name": { "type": "string", "description": "Group name to assign" }
                },
                "required": ["schematic", "references", "group_name"]
            }),
            |args, ctx| async move { handle_group_components(args, ctx).await }
        ),
        tool!(
            "replace_component",
            "Replace a component's lib_id with a new library symbol (swap the component type).",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string", "description": "Path to .kicad_sch file" },
                    "reference": { "type": "string", "description": "Component reference designator (e.g. 'U1')" },
                    "new_lib_id": { "type": "string", "description": "New Library:Symbol identifier (e.g. 'Device:C')" },
                    "unit": { "type": "integer", "description": "Optional unit number for multi-unit symbols; validated against the new symbol's unit count. When omitted the existing unit is kept." }
                },
                "required": ["schematic", "reference", "new_lib_id"]
            }),
            |args, ctx| async move { handle_replace_component(args, ctx).await }
        ),
        tool!(
            "get_schematic_view",
            "Render the schematic to a PNG image (base64-encoded) via kicad-cli.",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string" }
                },
                "required": ["schematic"]
            }),
            |args, ctx| async move { handle_get_schematic_view(args, ctx).await }
        ),
    ]
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

async fn handle_create_schematic(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let path = get_path(args, "path")?;
    // Build a minimal valid schematic and save via cse's atomic writer.
    let template = crate::tools::blank_schematic_template();
    // Write the template then immediately load/save through cse so the file
    // is normalised to cse's writer output format.
    write_new_atomic(&path, &template)?;
    let sch = cse::Schematic::load(&path)?;
    sch.overwrite()?;
    Ok(CallToolResult::json(
        &json!({ "created": path.display().to_string() }),
    ))
}

async fn handle_add_schematic_component(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let lib_id = match require_str(args, "lib_id") {
        Ok(s) => s.to_string(),
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
    let rotation = opt_f64(args, "rotation").unwrap_or(0.0);
    let reference = opt_str(args, "reference");
    let value = opt_str(args, "value");
    let unit = opt_f64(args, "unit").unwrap_or(1.0) as u32;
    let ref_str = reference.unwrap_or("?");

    // Load via konnect-schematic-editor
    let mut sch = cse::Schematic::load(&sch_path)?;

    // The instance path below must be "/<root-uuid>" — KiCAD's netlister
    // resolves instances against the root sheet UUID and silently forms no
    // wire-only nets for symbols whose path doesn't resolve.
    let root_uuid = crate::tools::ensure_root_uuid(&mut sch);
    let project_name = project_name_for(&sch_path);

    let result = match place_one_component(
        &mut sch,
        &root_uuid,
        &project_name,
        &lib_id,
        x,
        y,
        rotation,
        ref_str,
        value,
        unit,
    ) {
        Ok(v) => v,
        Err(e) => return Ok(e),
    };

    sch.overwrite()?;

    // A pin landing mid-segment on an existing wire needs a junction dot, or
    // KiCad's netlister treats it as unconnected. Runs after the write because
    // it re-reads the saved file; `place_one_component` stays pure so the batch
    // path can do one junction pass for the whole batch instead of one per part.
    let mut result = result;
    let junctions = crate::tools::add_pin_midwire_junctions(&sch_path, ref_str)?;
    result["junctions_added"] = json!(junctions
        .iter()
        .map(|(x, y)| json!({ "x": x, "y": y }))
        .collect::<Vec<_>>());

    Ok(CallToolResult::json(&result))
}

/// Place one symbol into `sch`: embeds the lib_symbols definition, validates
/// the unit, and adds the positioned instance. Does not write the file --
/// callers own the read/write cycle (single-add and batch-add alike).
#[allow(clippy::too_many_arguments)]
pub(crate) fn place_one_component(
    sch: &mut cse::Schematic,
    root_uuid: &str,
    project_name: &str,
    lib_id: &str,
    x: f64,
    y: f64,
    rotation: f64,
    reference: &str,
    value: Option<&str>,
    unit: u32,
) -> Result<serde_json::Value, CallToolResult> {
    // Snap to 1.27mm grid
    let (x, y) = snap_point(x, y, 1.27);

    // Embed the library symbol definition. A lib_id that names the right symbol
    // in a library that doesn't exist (`regulator/AMS1117-3.3`, bare `R`) is not
    // a failure worth reporting back and paying another model call for — the
    // installed libraries answer it outright when they answer it uniquely
    // (H.6.1). The direct attempt runs first, so a valid lib_id pays nothing for
    // this: `canonical_lib_id` is only ever reached once placement has failed.
    // `ensure_lib_symbol` leaves the schematic untouched when it returns false,
    // so the second attempt starts from the same state as the first.
    let requested = lib_id;
    let mut canonicalized_from = None;
    let mut lib_id = std::borrow::Cow::Borrowed(lib_id);
    if !cse::library::ensure_lib_symbol(sch, &lib_id) {
        match cse::library::canonical_lib_id(&lib_id) {
            Some(canonical) if cse::library::ensure_lib_symbol(sch, &canonical) => {
                canonicalized_from = Some(requested.to_string());
                lib_id = std::borrow::Cow::Owned(canonical);
            }
            // Report the id the caller actually asked for: its did-you-mean
            // list is the one they can act on.
            _ => return Err(crate::tools::lib_symbol_not_found_error(requested)),
        }
    }
    let lib_id = lib_id.as_ref();
    let val_str = value.unwrap_or(lib_id.split(':').next_back().unwrap_or("?"));

    // Validate the unit against the resolved symbol BEFORE writing anything:
    // eeschema silently renders an out-of-range unit as unit 1 and the
    // netlister mis-assigns its pins (#35).
    let unit_count = cse::library::symbol_unit_count(lib_id).unwrap_or(1);
    if unit < 1 || unit > unit_count {
        return Err(CallToolResult::error(format!(
            "Invalid unit {} for '{}': the symbol has {} unit(s) (valid: 1..={}).",
            unit, lib_id, unit_count, unit_count
        )));
    }

    // Build the Symbol struct
    let mut sym = cse::Symbol::new(lib_id, x, y);
    sym.at.rotation = Some(rotation);
    sym.unit = unit;

    // Reference above the component, Value below; Footprint/Datasheet hidden.
    // Power symbols get their Reference hidden too, matching eeschema: a
    // #PWR designator is never shown on the sheet.
    let hide_reference = lib_id.starts_with("power:") || reference.starts_with("#PWR");
    let positioned = crate::tools::positioned_property;
    sym.properties.push(positioned(
        "Reference",
        reference,
        x,
        y - 3.81,
        0.0,
        hide_reference,
    ));
    sym.properties
        .push(positioned("Value", val_str, x, y + 3.81, 0.0, false));
    sym.properties
        .push(positioned("Footprint", "", x, y, 0.0, true));
    sym.properties
        .push(positioned("Datasheet", "", x, y, 0.0, true));

    // Instance entry, keyed to the root sheet UUID like eeschema writes it:
    // (instances (project "<name>" (path "/<root-uuid>" (reference ...) (unit 1))))
    sym.set_instance_path(project_name, &format!("/{}", root_uuid), reference, unit);

    let uuid = sym.uuid.clone();
    sch.add_symbol(sym);

    let mut result = json!({
        "added": lib_id,
        "reference": reference,
        "value": val_str,
        "x": x, "y": y,
        "unit": unit,
        "uuid": uuid
    });
    // Substituting silently would let a caller believe a lib_id it invented is
    // real and reuse it. `added` is already the placed id; this says what it
    // replaced.
    if let Some(from) = canonicalized_from {
        result["lib_id_canonicalized_from"] = json!(from);
    }
    Ok(result)
}

async fn handle_delete_schematic_component(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let reference = match require_str(args, "reference") {
        Ok(r) => r.to_string(),
        Err(e) => return Ok(e),
    };

    let mut sch = cse::Schematic::load(&sch_path)?;

    match sch.symbols.remove_by_reference(&reference) {
        Some(_) => {
            sch.overwrite()?;
            Ok(CallToolResult::json(&json!({ "deleted": reference })))
        }
        None => Ok(CallToolResult::error(format!(
            "Component '{}' not found in schematic",
            reference
        ))),
    }
}

async fn handle_edit_schematic_component(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let reference = match require_str(args, "reference") {
        Ok(r) => r.to_string(),
        Err(e) => return Ok(e),
    };

    let mut content = read_consistent(&sch_path)?;
    let expected = content.clone();
    let mut changed = Vec::new();

    let mut errors: Vec<String> = Vec::new();
    // Set a property, adding it when the symbol has none.
    //
    // A symbol carries only the properties it was given: a part placed without
    // a footprint has no `Footprint` property at all, and refusing to set one
    // made the tool unable to do the single most common edit after placement
    // (J.2.4.1). A missing property is now created, exactly as
    // `add_component_annotation` creates one.
    let mut apply = |content: &mut String, field: &str, new_val: &str| match update_field(
        content, &reference, field, new_val,
    ) {
        Ok(updated) => {
            *content = updated;
            changed.push(format!("{} → {}", field, new_val));
        }
        Err(FieldError::MissingProperty) => {
            match insert_property(content, &reference, field, new_val) {
                Ok(updated) => {
                    *content = updated;
                    changed.push(format!("{} → {} (added)", field, new_val));
                }
                Err(why) => errors.push(format!("{field}: {why}")),
            }
        }
        Err(other) => errors.push(format!("{field}: {other}")),
    };

    // Every field is located by looking the symbol up by `reference`, so the
    // rename has to go last: renaming first made the symbol unfindable and
    // every other field in the same call came back "symbol 'R2' not found".
    if let Some(val) = opt_str(args, "value") {
        apply(&mut content, "Value", val);
    }
    if let Some(fp) = opt_str(args, "footprint") {
        apply(&mut content, "Footprint", fp);
    }
    if let Some(ds) = opt_str(args, "datasheet") {
        apply(&mut content, "Datasheet", ds);
    }
    if let Some(new_ref) = opt_str(args, "new_reference") {
        apply(&mut content, "Reference", new_ref);
        // KiCAD resolves a symbol's designator from its `instances` block, not
        // from the Reference property. Renaming only the property left
        // `kicad-cli sch export netlist` still emitting the old designator
        // while this tool reported success (J.2.3.2).
        match update_instance_reference(&content, new_ref, &reference) {
            Ok(updated) => content = updated,
            Err(why) => errors.push(format!("instances: {why}")),
        }
    }

    // A request that changed nothing is a failure, not a success — silently
    // reporting `"changes": []` is what let the tab-indentation bug hide.
    if changed.is_empty() && !errors.is_empty() {
        return Ok(CallToolResult::error(format!(
            "No fields were updated on '{}': {}",
            reference,
            errors.join("; ")
        )));
    }

    if !changed.is_empty() {
        let item_id = symbol_item_id(&expected, &reference)?;
        let command = SchematicCommand::replace_item_from_document(
            &expected,
            &content,
            item_id,
            format!("Edit {reference}"),
        )?;
        commit_command(&sch_path, &command)?;
    }

    let mut result = json!({
        "reference": reference,
        "changes": changed
    });
    if !errors.is_empty() {
        result["errors"] = json!(errors);
    }
    Ok(CallToolResult::json(&result))
}

/// Why a property edit could not be applied. Separated from a plain string so
/// the caller can tell "the symbol has no such property" — which is fixable by
/// adding one — from a failure that is not.
enum FieldError {
    SymbolNotFound(String),
    MissingProperty,
    Malformed(String),
}

impl std::fmt::Display for FieldError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FieldError::SymbolNotFound(reference) => {
                write!(f, "symbol '{reference}' not found in this schematic")
            }
            FieldError::MissingProperty => write!(f, "the symbol has no such property"),
            FieldError::Malformed(field) => write!(f, "'{field}' property is malformed"),
        }
    }
}

/// Replace the value of a property the symbol already carries.
fn update_field(
    content: &str,
    reference: &str,
    field: &str,
    new_val: &str,
) -> Result<String, FieldError> {
    let (sym_start, sym_end) = find_symbol_instance_block(content, reference)
        .ok_or_else(|| FieldError::SymbolNotFound(reference.to_string()))?;
    let sym_block = &content[sym_start..sym_end];
    let field_search = format!(r#"(property "{field}" ""#);
    let field_offset = sym_block
        .find(&field_search)
        .map(|o| sym_start + o + field_search.len())
        .ok_or(FieldError::MissingProperty)?;
    let val_end = content[field_offset..]
        .find('"')
        .map(|o| field_offset + o)
        .ok_or_else(|| FieldError::Malformed(field.to_string()))?;
    Ok(format!(
        "{}{}{}",
        &content[..field_offset],
        new_val,
        &content[val_end..]
    ))
}

/// Add a property the symbol does not carry yet, hidden and at the origin —
/// the same shape `add_component_annotation` writes, so a field added by
/// either tool looks the same in the file.
fn insert_property(
    content: &str,
    reference: &str,
    field: &str,
    value: &str,
) -> Result<String, FieldError> {
    let (sym_start, sym_end) = find_symbol_instance_block(content, reference)
        .ok_or_else(|| FieldError::SymbolNotFound(reference.to_string()))?;
    let sym_block = &content[sym_start..sym_end];
    // Before `instances` if there is one, otherwise before the block's close.
    let insert_rel = sym_block
        .find("(instances")
        .or_else(|| sym_block.rfind(')'))
        .ok_or_else(|| FieldError::Malformed(field.to_string()))?;
    let insert_abs = sym_start + insert_rel;

    let property = format!(
        "(property \"{field}\" \"{value}\"\n      (at 0 0 0)\n      (effects (font (size 1.27 1.27)) (hide yes))\n    )\n    "
    );
    Ok(format!(
        "{}{}{}",
        &content[..insert_abs],
        property,
        &content[insert_abs..]
    ))
}

/// Point a renamed symbol's `instances` entries at its new designator.
///
/// `new_ref` is what the symbol's Reference property now says — that is how the
/// block is located — and `old_ref` is the designator the instance entries still
/// carry. Every entry is rewritten, because a symbol placed on several sheets
/// has one per sheet and leaving any of them behind means KiCAD reports two
/// different designators for the same symbol.
fn update_instance_reference(
    content: &str,
    new_ref: &str,
    old_ref: &str,
) -> Result<String, String> {
    let (start, end) = find_symbol_instance_block(content, new_ref)
        .ok_or_else(|| format!("symbol '{new_ref}' not found after the rename"))?;

    let needle = format!(r#"(reference "{old_ref}")"#);
    let replacement = format!(r#"(reference "{new_ref}")"#);
    let block = &content[start..end];
    if !block.contains(&needle) {
        // Nothing to repoint: a symbol with no instances block is already
        // consistent, and saying so beats reporting a failure.
        return Ok(content.to_string());
    }
    Ok(format!(
        "{}{}{}",
        &content[..start],
        block.replace(&needle, &replacement),
        &content[end..]
    ))
}

async fn handle_get_schematic_component(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let reference = match require_str(args, "reference") {
        Ok(r) => r.to_string(),
        Err(e) => return Ok(e),
    };

    let sch = cse::Schematic::load(&sch_path)?;

    match sch.symbols.by_reference(&reference) {
        Some(sym) => {
            let (x, y) = sym.position();
            let rotation = sym.at.rotation.unwrap_or(0.0);
            let mirror = sym.mirror.as_deref().unwrap_or("");
            Ok(CallToolResult::json(&json!({
                "reference": sym.reference().unwrap_or("?"),
                "value": sym.value_str().unwrap_or(""),
                "footprint": sym.footprint().unwrap_or(""),
                "lib_id": sym.lib_id,
                "x": x,
                "y": y,
                "rotation": rotation,
                "mirror_x": mirror.contains('x'),
                "mirror_y": mirror.contains('y'),
                "uuid": sym.uuid
            })))
        }
        None => Ok(CallToolResult::error(format!(
            "Component '{}' not found",
            reference
        ))),
    }
}

async fn handle_list_schematic_components(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let sch = cse::Schematic::load(&sch_path)?;

    let items: Vec<serde_json::Value> = sch
        .symbols
        .iter()
        .map(|sym| {
            let (x, y) = sym.position();
            let rotation = sym.at.rotation.unwrap_or(0.0);
            let mirror = sym.mirror.as_deref().unwrap_or("");
            json!({
                "reference": sym.reference().unwrap_or("?"),
                "value": sym.value_str().unwrap_or(""),
                "footprint": sym.footprint().unwrap_or(""),
                "lib_id": sym.lib_id,
                "x": x,
                "y": y,
                "rotation": rotation,
                "mirror_x": mirror.contains('x'),
                "mirror_y": mirror.contains('y')
            })
        })
        .collect();

    Ok(CallToolResult::json(&json!({
        "count": items.len(),
        "components": items
    })))
}

async fn handle_move_schematic_component(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let reference = match require_str(args, "reference") {
        Ok(r) => r.to_string(),
        Err(e) => return Ok(e),
    };
    let new_x = match require_f64(args, "x") {
        Ok(v) => v,
        Err(e) => return Ok(e),
    };
    let new_y = match require_f64(args, "y") {
        Ok(v) => v,
        Err(e) => return Ok(e),
    };
    let (new_x, new_y) = snap_point(new_x, new_y, 1.27);

    let mut sch = cse::Schematic::load(&sch_path)?;

    match sch.symbols.by_reference_mut(&reference) {
        Some(sym) => {
            sym.move_to(new_x, new_y);
            sch.overwrite()?;
            Ok(CallToolResult::json(
                &json!({ "moved": reference, "x": new_x, "y": new_y }),
            ))
        }
        None => Err(anyhow::anyhow!("Component '{}' not found", reference)),
    }
}

async fn handle_rotate_schematic_component(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let reference = match require_str(args, "reference") {
        Ok(r) => r.to_string(),
        Err(e) => return Ok(e),
    };
    let rotation = match require_f64(args, "rotation") {
        Ok(v) => v,
        Err(e) => return Ok(e),
    };

    let mut sch = cse::Schematic::load(&sch_path)?;

    match sch.symbols.by_reference_mut(&reference) {
        Some(sym) => {
            sym.set_rotation(rotation);
            sch.overwrite()?;
            Ok(CallToolResult::json(
                &json!({ "rotated": reference, "rotation": rotation }),
            ))
        }
        None => Err(anyhow::anyhow!("Component '{}' not found", reference)),
    }
}

/// Absolute pin positions of `reference`, keyed by pin number.
///
/// Returns an empty map rather than an error when the component or its
/// embedded definition cannot be resolved: the caller uses this to *decide
/// whether* a wire end belongs to a pin, and having no pins to match simply
/// means nothing moves.
fn pin_positions(sch_path: &std::path::Path, reference: &str) -> Vec<(String, (f64, f64))> {
    let Ok((_, tree)) = read_schematic(sch_path) else {
        return Vec::new();
    };
    let instances = extract_symbol_instances(&tree);
    let Some(inst) = instances.iter().find(|i| i.reference == reference) else {
        return Vec::new();
    };
    let lib_syms = tree
        .find("lib_symbols")
        .map(|n| n.find_all("symbol"))
        .unwrap_or_default();
    let Some(sym) = lib_syms
        .iter()
        .find(|n| n.get(1).and_then(|c| c.as_str()) == Some(&inst.lib_id))
    else {
        return Vec::new();
    };
    let transform = inst.pin_transform();
    extract_lib_pins_for_unit(sym, inst.unit)
        .iter()
        .map(|pin| (pin.number.clone(), pin_endpoint(pin, transform)))
        .collect()
}

/// Move a symbol and drag the wire ends that were sitting on its pins along
/// with it (J.2.4.2).
///
/// This used to delegate to the plain move, which left every attached wire
/// behind and silently broke the connection the caller was trying to preserve.
/// The pins are measured before and after the move, and a wire end coincident
/// with an old pin position is rewritten to the new one — matched per pin
/// number, so a rotation that reorders the pins still lands each wire on the
/// pin it was on.
async fn handle_move_connected(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let reference = match require_str(args, "reference") {
        Ok(r) => r.to_string(),
        Err(e) => return Ok(e),
    };

    let before = pin_positions(&sch_path, &reference);
    let moved = handle_move_schematic_component(args, ctx).await?;
    if moved.is_error {
        return Ok(moved);
    }
    let after = pin_positions(&sch_path, &reference);

    let mut sch = cse::Schematic::load(&sch_path)?;
    let mut dragged = 0usize;
    for (number, old) in &before {
        let Some((_, new)) = after.iter().find(|(n, _)| n == number) else {
            continue;
        };
        if konnect_sexp::geometry::points_coincident(old.0, old.1, new.0, new.1, 1e-9) {
            continue;
        }
        for wire in sch.wires.iter_mut() {
            for end in [&mut wire.start, &mut wire.end] {
                if konnect_sexp::geometry::points_coincident(end.0, end.1, old.0, old.1, 0.01) {
                    *end = *new;
                    dragged += 1;
                }
            }
        }
    }
    if dragged > 0 {
        sch.overwrite()?;
    }

    let mut body: serde_json::Value = match moved.content.first() {
        Some(crate::mcp::protocol::ToolContent::Text { text }) => {
            serde_json::from_str(text).unwrap_or_else(|_| json!({ "moved": reference }))
        }
        _ => json!({ "moved": reference }),
    };
    body["wire_ends_dragged"] = json!(dragged);
    Ok(CallToolResult::json(&body))
}

async fn handle_move_region(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
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
    let dx = match require_f64(args, "dx") {
        Ok(v) => v,
        Err(e) => return Ok(e),
    };
    let dy = match require_f64(args, "dy") {
        Ok(v) => v,
        Err(e) => return Ok(e),
    };

    let mut sch = cse::Schematic::load(&sch_path)?;

    // Collect references of symbols within the bounding box
    let refs_to_move: Vec<String> = sch
        .symbols
        .within_rectangle(x1, y1, x2, y2)
        .iter()
        .filter_map(|s| s.reference().map(String::from))
        .collect();

    let mut moved = Vec::new();
    for reference in &refs_to_move {
        if let Some(sym) = sch.symbols.by_reference_mut(reference) {
            let (ox, oy) = sym.position();
            let (nx, ny) = snap_point(ox + dx, oy + dy, 1.27);
            sym.move_to(nx, ny);
            moved.push(reference.clone());
        }
    }

    sch.overwrite()?;

    Ok(CallToolResult::json(&json!({
        "moved_count": moved.len(),
        "moved": moved
    })))
}

async fn handle_annotate_schematic(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    crate::tools::cli::annotate_schematic(&ctx.config.kicad_cli, &sch_path).await?;
    Ok(CallToolResult::text("Annotation complete."))
}

async fn handle_get_schematic_pin_locations(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let reference = match require_str(args, "reference") {
        Ok(r) => r.to_string(),
        Err(e) => return Ok(e),
    };

    let (_, tree) = read_schematic(&sch_path)?;
    let instances = extract_symbol_instances(&tree);
    let inst = match instances.iter().find(|i| i.reference == reference) {
        Some(i) => i,
        None => {
            return Ok(CallToolResult::error(format!(
                "Component '{}' not found",
                reference
            )))
        }
    };

    // Find the library symbol definition within the schematic's lib_symbols section
    let lib_syms = tree
        .find("lib_symbols")
        .map(|n| n.find_all("symbol"))
        .unwrap_or_default();
    let lib_sym = lib_syms
        .iter()
        .find(|n| n.get(1).and_then(|c| c.as_str()) == Some(&inst.lib_id));

    // A missing embedded definition is an error, not an empty pin list —
    // silently returning [] hid every bad-lib_id component until wiring or
    // netlisting failed much later (#34).
    let Some(sym) = lib_sym else {
        return Ok(CallToolResult::error(format!(
            "Component '{}' has no embedded definition for '{}' in this \
             schematic's lib_symbols — it was likely added with a lib_id that \
             doesn't exist in the installed libraries, so it is invisible to \
             KiCAD's netlister. Re-add it with a valid lib_id \
             (delete_schematic_component + add_schematic_component).",
            reference, inst.lib_id
        )));
    };
    // Unit-aware: only this instance's unit (plus _0_1 commons), not every
    // unit's pins superimposed (#35).
    let lib_pins = extract_lib_pins_for_unit(sym, inst.unit);
    // A definition that resolves but has ZERO pins is almost always an
    // `(extends "Parent")` stub — kicad-cli can't resolve those either (the
    // netlist shows a pinless part), so silent pins:[] hides real breakage.
    // The #34 guard above only catches MISSING definitions.
    if lib_pins.is_empty() {
        if let Some(parent) = sym.find_str("extends") {
            return Ok(CallToolResult::error(format!(
                "Component '{}': the embedded definition for '{}' is an \
                 (extends \"{}\") stub with no pins of its own. kicad-cli \
                 cannot resolve extends stubs (the netlist gets a pinless \
                 part). Re-add the component (delete_schematic_component + \
                 add_schematic_component) so the definition is embedded in \
                 full, or place the parent symbol '{}' directly.",
                reference, inst.lib_id, parent, parent
            )));
        }
    }
    let t = inst.pin_transform();
    let pins: Vec<serde_json::Value> = lib_pins
        .iter()
        .map(|p| {
            let (sx, sy) = pin_endpoint(p, t);
            json!({
                "number": p.number,
                "name": p.name,
                "x": sx,
                "y": sy
            })
        })
        .collect();

    Ok(CallToolResult::json(&json!({
        "reference": reference,
        "component_x": inst.x,
        "component_y": inst.y,
        "rotation": inst.rotation,
        "pins": pins
    })))
}

async fn handle_batch_get_pin_locations(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let refs = args["references"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let (_, tree) = read_schematic(&sch_path)?; // single read
    let instances = extract_symbol_instances(&tree);
    let lib_syms = tree
        .find("lib_symbols")
        .map(|n| n.find_all("symbol"))
        .unwrap_or_default();

    let results: Vec<serde_json::Value> = refs
        .iter()
        .map(|reference| {
            let inst = match instances.iter().find(|i| &i.reference == reference) {
                Some(i) => i,
                None => return json!({ "reference": reference, "error": "not found" }),
            };
            let lib_sym = lib_syms
                .iter()
                .find(|n| n.get(1).and_then(|c| c.as_str()) == Some(&inst.lib_id));
            // Per-entry error rather than a silent empty pin list (#34).
            let Some(sym) = lib_sym else {
                return json!({
                    "reference": reference,
                    "error": format!(
                        "no embedded definition for '{}' in lib_symbols — \
                         likely added with a nonexistent lib_id",
                        inst.lib_id
                    )
                });
            };
            let lib_pins = extract_lib_pins_for_unit(sym, inst.unit);
            // Zero pins from a resolving definition = extends stub (#35);
            // mirror the single-component handler's structured error.
            if lib_pins.is_empty() {
                if let Some(parent) = sym.find_str("extends") {
                    return json!({
                        "reference": reference,
                        "error": format!(
                            "embedded definition for '{}' is an (extends \"{}\") \
                             stub with no pins — re-add the component so it is \
                             embedded in full",
                            inst.lib_id, parent
                        )
                    });
                }
            }
            let t = inst.pin_transform();
            let pins: Vec<serde_json::Value> = lib_pins
                .iter()
                .map(|p| {
                    let (sx, sy) = pin_endpoint(p, t);
                    json!({ "number": p.number, "name": p.name, "x": sx, "y": sy })
                })
                .collect();
            json!({ "reference": reference, "x": inst.x, "y": inst.y, "pins": pins })
        })
        .collect();

    Ok(CallToolResult::json(&json!({ "components": results })))
}

async fn handle_get_schematic_view(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let tmp_dir = std::env::temp_dir().join(format!("konnect_{}", new_uuid()));
    tokio::fs::create_dir_all(&tmp_dir).await?;

    // KiCAD 10 CLI only supports SVG export for schematics (no bitmap)
    let svg_path =
        crate::tools::cli::render_schematic_svg(&ctx.config.kicad_cli, &sch_path, &tmp_dir).await?;

    let svg_content = tokio::fs::read_to_string(&svg_path).await?;
    tokio::fs::remove_dir_all(&tmp_dir).await.ok();

    // Return as text content (SVG is XML text, not a raster image)
    Ok(crate::mcp::protocol::CallToolResult {
        content: vec![crate::mcp::protocol::ToolContent::Text {
            text: format!("SVG schematic rendered. {} bytes.\n\nNote: KiCAD 10 CLI exports schematics as SVG only (no bitmap). \
                          The SVG file has been generated. Use export_schematic_pdf for a PDF version.", svg_content.len()),
        }],
        is_error: false,
    })
}

async fn handle_add_component_annotation(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let reference = match require_str(args, "reference") {
        Ok(r) => r.to_string(),
        Err(e) => return Ok(e),
    };
    let key = match require_str(args, "key") {
        Ok(v) => v.to_string(),
        Err(e) => return Ok(e),
    };
    let value = match require_str(args, "value") {
        Ok(v) => v.to_string(),
        Err(e) => return Ok(e),
    };

    let content = read_consistent(&sch_path)?;
    let expected = content.clone();

    // Find the symbol block for this reference
    let (sym_start, sym_end) = match find_symbol_instance_block(&content, &reference) {
        Some(r) => r,
        None => {
            return Ok(CallToolResult::error(format!(
                "Component '{}' not found",
                reference
            )))
        }
    };

    // Find the position just before (instances in the symbol block, or before closing paren
    let sym_block = &content[sym_start..sym_end];
    let insert_rel = sym_block
        .find("(instances")
        .unwrap_or(sym_block.rfind(')').unwrap_or(sym_block.len() - 1));
    let insert_abs = sym_start + insert_rel;

    // Build the property S-expression
    let prop_sexp = format!(
        "    (property \"{key}\" \"{value}\"\n      (at 0 0 0)\n      (effects (font (size 1.27 1.27)) (hide yes))\n    )\n    "
    );

    let new_content = apply_edits(content, vec![SexpEdit::insert(insert_abs, prop_sexp)]);
    let item_id = symbol_item_id(&expected, &reference)?;
    let command = SchematicCommand::replace_item_from_document(
        &expected,
        &new_content,
        item_id,
        format!("Add {key} property to {reference}"),
    )?;
    commit_command(&sch_path, &command)?;

    Ok(CallToolResult::json(&json!({
        "reference": reference,
        "added_property": key,
        "value": value
    })))
}

fn symbol_item_id(content: &str, reference: &str) -> anyhow::Result<ItemId> {
    let (start, end) = find_symbol_instance_block(content, reference)
        .ok_or_else(|| anyhow::anyhow!("component '{reference}' not found"))?;
    let symbol = parse_sexp(&content[start..end])?;
    let uuid = symbol
        .find_str("uuid")
        .ok_or_else(|| anyhow::anyhow!("component '{reference}' has no UUID"))?;
    Ok(ItemId::new(uuid.to_owned())?)
}

async fn handle_group_components(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let group_name = match require_str(args, "group_name") {
        Ok(v) => v.to_string(),
        Err(e) => return Ok(e),
    };
    let refs = args["references"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if refs.is_empty() {
        return Ok(CallToolResult::error("No references provided"));
    }

    let mut content = read_consistent(&sch_path)?;
    let expected = content.clone();
    let mut grouped = Vec::new();
    let mut item_ids = Vec::new();

    for reference in &refs {
        let (sym_start, sym_end) = match find_symbol_instance_block(&content, reference) {
            Some(r) => r,
            None => continue,
        };

        let sym_block = &content[sym_start..sym_end];
        let insert_rel = sym_block
            .find("(instances")
            .unwrap_or(sym_block.rfind(')').unwrap_or(sym_block.len() - 1));
        let insert_abs = sym_start + insert_rel;

        let prop_sexp = format!(
            "    (property \"Group\" \"{group_name}\"\n      (at 0 0 0)\n      (effects (font (size 1.27 1.27)) (hide yes))\n    )\n    "
        );

        content = apply_edits(content, vec![SexpEdit::insert(insert_abs, prop_sexp)]);
        item_ids.push(symbol_item_id(&expected, reference)?);
        grouped.push(reference.clone());
    }

    if !item_ids.is_empty() {
        let command = SchematicCommand::replace_items_from_document(
            &expected,
            &content,
            item_ids,
            format!("Group components as {group_name}"),
        )?;
        commit_command(&sch_path, &command)?;
    }

    Ok(CallToolResult::json(&json!({
        "group_name": group_name,
        "grouped_count": grouped.len(),
        "grouped": grouped
    })))
}

async fn handle_replace_component(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let reference = match require_str(args, "reference") {
        Ok(r) => r.to_string(),
        Err(e) => return Ok(e),
    };
    let new_lib_id = match require_str(args, "new_lib_id") {
        Ok(v) => v.to_string(),
        Err(e) => return Ok(e),
    };
    let new_unit = opt_f64(args, "unit").map(|u| u as u32);

    let mut content = read_consistent(&sch_path)?;
    let expected = content.clone();

    // Find the symbol block for this reference
    let (sym_start, sym_end) = match find_symbol_instance_block(&content, &reference) {
        Some(r) => r,
        None => {
            return Ok(CallToolResult::error(format!(
                "Component '{}' not found",
                reference
            )))
        }
    };

    // Find the (lib_id "OLD") and replace it — searching only within this
    // symbol's block, so a malformed instance can't reach into the next one.
    let sym_block = &content[sym_start..sym_end];
    let lib_id_pat = "(lib_id \"";
    let lib_id_rel = match sym_block.find(lib_id_pat) {
        Some(o) => o,
        None => {
            return Ok(CallToolResult::error(
                "Could not find lib_id in symbol block",
            ))
        }
    };
    let lib_id_abs = sym_start + lib_id_rel + lib_id_pat.len();
    let lib_id_end = match content[lib_id_abs..].find('"') {
        Some(o) => lib_id_abs + o,
        None => return Ok(CallToolResult::error("Malformed lib_id")),
    };

    let old_lib_id = content[lib_id_abs..lib_id_end].to_string();

    let new_content = apply_edits(
        content,
        vec![SexpEdit::replace(
            lib_id_abs,
            lib_id_end,
            new_lib_id.clone(),
        )],
    );
    content = new_content;

    // Optional unit change, validated against the NEW symbol's unit count
    // (#35). Applied before the embed so all edits land in one write.
    if let Some(unit) = new_unit {
        let unit_count = cse::library::symbol_unit_count(&new_lib_id).unwrap_or(1);
        if unit < 1 || unit > unit_count {
            return Ok(CallToolResult::error(format!(
                "Invalid unit {} for '{}': the symbol has {} unit(s) (valid: 1..={}).",
                unit, new_lib_id, unit_count, unit_count
            )));
        }
        // Re-find the block (offsets moved with the lib_id edit), then update
        // every `(unit N)` inside it — the symbol's own and the one in its
        // (instances …) entry.
        if let Some((s, e)) = find_symbol_instance_block(&content, &reference) {
            let block = &content[s..e];
            let mut edits = Vec::new();
            let mut from = 0usize;
            while let Some(rel) = block[from..].find("(unit ") {
                let num_start = from + rel + "(unit ".len();
                let Some(close) = block[num_start..].find(')') else {
                    break;
                };
                edits.push(SexpEdit::replace(
                    s + num_start,
                    s + num_start + close,
                    unit.to_string(),
                ));
                from = num_start + close;
            }
            content = apply_edits(content, edits);
        }
    }

    // Ensure the new library symbol definition is present. Bail BEFORE writing:
    // a replace that can't embed its definition would leave the component
    // netlist-invisible (#34).
    if !super::ensure_lib_symbol_in_schematic(&mut content, &new_lib_id) {
        return Ok(crate::tools::lib_symbol_not_found_error(&new_lib_id));
    }
    write_atomic_if_unchanged(&sch_path, &expected, &content)?;

    Ok(CallToolResult::json(&json!({
        "reference": reference,
        "old_lib_id": old_lib_id,
        "new_lib_id": new_lib_id,
        "unit": new_unit
    })))
}

// Library symbol resolution moved to tools/mod.rs (shared with sch_wiring.rs)

// `pub(crate)` (rather than private): `tools::lib_symbol_not_found_error_tests`
// needs `SYMBOL_DIR_ENV` below to serialize against these tests' env-var use.
#[cfg(test)]
pub(crate) mod tests {
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

    /// Serializes tests that set KICAD10_SYMBOL_DIR (process-wide env).
    /// `pub(crate)` so other test modules in this crate that also fixture a
    /// symbol dir (e.g. `tools::lib_symbol_not_found_error_tests`) serialize
    /// against the same lock instead of racing on a second, independent one.
    ///
    /// A `tokio` mutex rather than `std`'s (E10): every holder below spans an
    /// `.await`, and a `std::sync::MutexGuard` held across one is not `Send`,
    /// so the test future stops being `Send` and could not survive a move to
    /// `#[tokio::test(flavor = "multi_thread")]`. It also cannot be poisoned,
    /// so one panicking test no longer takes the rest of the file with it.
    pub(crate) static SYMBOL_DIR_ENV: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    /// A stub symbol library so component adds resolve without an installed
    /// KiCAD (CI has none): Device:R and Device:C_Polarized in the KiCAD 10
    /// symdir layout. Returns (tempdir guard, env lock).
    async fn stub_symbol_dir() -> (tempfile::TempDir, tokio::sync::MutexGuard<'static, ()>) {
        let guard = SYMBOL_DIR_ENV.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let symdir = dir.path().join("Device.kicad_symdir");
        std::fs::create_dir_all(&symdir).unwrap();
        let symbol = |name: &str| {
            format!(
                "(kicad_symbol_lib\n\t(version 20241209)\n\t(generator \"test\")\n\t(symbol \"{name}\"\n\t\t(property \"Reference\" \"R\" (at 0 0 0))\n\t\t(property \"Value\" \"{name}\" (at 0 0 0))\n\t\t(symbol \"{name}_0_1\"\n\t\t\t(pin passive line (at 0 3.81 270) (length 1.27)\n\t\t\t\t(name \"~\" (effects (font (size 1.27 1.27))))\n\t\t\t\t(number \"1\" (effects (font (size 1.27 1.27))))\n\t\t\t)\n\t\t\t(pin passive line (at 0 -3.81 90) (length 1.27)\n\t\t\t\t(name \"~\" (effects (font (size 1.27 1.27))))\n\t\t\t\t(number \"2\" (effects (font (size 1.27 1.27))))\n\t\t\t)\n\t\t)\n\t)\n)\n"
            )
        };
        std::fs::write(symdir.join("R.kicad_sym"), symbol("R")).unwrap();
        std::fs::write(symdir.join("C_Polarized.kicad_sym"), symbol("C_Polarized")).unwrap();
        // LM2904-style multi-unit part: unit 1 = pins 1-3, unit 2 = pins 5-7,
        // unit 3 = power pins 4/8 (#35 repro shape).
        let pin = |num: &str, x: f64, y: f64, angle: u32| {
            format!(
                "\t\t\t(pin passive line (at {x} {y} {angle}) (length 2.54)\n\t\t\t\t(name \"~\" (effects (font (size 1.27 1.27))))\n\t\t\t\t(number \"{num}\" (effects (font (size 1.27 1.27))))\n\t\t\t)\n"
            )
        };
        let opamp = format!(
            "(kicad_symbol_lib\n\t(version 20241209)\n\t(generator \"test\")\n\t(symbol \"OPAMP_DUAL\"\n\t\t(property \"Reference\" \"U\" (at 0 0 0))\n\t\t(property \"Value\" \"OPAMP_DUAL\" (at 0 0 0))\n\t\t(symbol \"OPAMP_DUAL_1_1\"\n{}{}{}\t\t)\n\t\t(symbol \"OPAMP_DUAL_2_1\"\n{}{}{}\t\t)\n\t\t(symbol \"OPAMP_DUAL_3_1\"\n{}{}\t\t)\n\t)\n)\n",
            pin("1", -7.62, 2.54, 0),
            pin("2", -7.62, -2.54, 0),
            pin("3", 7.62, 0.0, 180),
            pin("5", -7.62, 2.54, 0),
            pin("6", -7.62, -2.54, 0),
            pin("7", 7.62, 0.0, 180),
            pin("4", 0.0, -7.62, 90),
            pin("8", 0.0, 7.62, 270),
        );
        std::fs::write(symdir.join("OPAMP_DUAL.kicad_sym"), opamp).unwrap();
        // Derived symbol: an extends stub with no drawing of its own, like
        // Amplifier_Operational:NE5532 → LM2904.
        std::fs::write(
            symdir.join("OPAMP_DERIVED.kicad_sym"),
            "(kicad_symbol_lib\n\t(version 20241209)\n\t(generator \"test\")\n\t(symbol \"OPAMP_DERIVED\"\n\t\t(extends \"OPAMP_DUAL\")\n\t\t(property \"Reference\" \"U\" (at 0 0 0))\n\t\t(property \"Value\" \"OPAMP_DERIVED\" (at 0 0 0))\n\t)\n)\n",
        )
        .unwrap();
        std::env::set_var("KICAD10_SYMBOL_DIR", dir.path());
        (dir, guard)
    }

    #[tokio::test]
    async fn create_schematic_writes_root_uuid() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fresh.kicad_sch");
        let ctx = test_ctx();

        let result = handle_create_schematic(&json!({ "path": path.display().to_string() }), &ctx)
            .await
            .unwrap();
        assert!(!result.is_error);

        let sch = cse::Schematic::load(&path).unwrap();
        assert!(
            sch.uuid.is_some(),
            "root (uuid ...) is required for KiCAD's netlister to resolve instance paths"
        );
    }

    #[tokio::test]
    async fn add_component_writes_eeschema_style_instance_path() {
        let (_symdir, _env) = stub_symbol_dir().await;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("amp.kicad_sch");
        let ctx = test_ctx();

        handle_create_schematic(&json!({ "path": path.display().to_string() }), &ctx)
            .await
            .unwrap();
        let result = handle_add_schematic_component(
            &json!({
                "schematic": path.display().to_string(),
                "lib_id": "Device:R",
                "x": 100.0, "y": 80.0,
                "reference": "R1"
            }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(!result.is_error);

        let sch = cse::Schematic::load(&path).unwrap();
        let root_uuid = sch.uuid.clone().expect("root uuid present");
        let sym = sch.symbols.by_reference("R1").unwrap();
        // KiCAD only forms wire-only nets when the instance path is exactly
        // "/<root-uuid>"; the project key mirrors eeschema (file stem).
        assert!(
            sym.has_instance_path("amp", &format!("/{}", root_uuid)),
            "instance path must be /<root-uuid> under the file-stem project name"
        );
    }

    /// H.6.1: an invented library around a real symbol name is a naming slip
    /// the installed libraries settle by themselves. 10 of the 16 apply
    /// failures in the E26 model-fit run were this shape; failing them back to
    /// the caller buys a whole extra model call for no new information.
    #[tokio::test]
    async fn add_component_canonicalizes_an_invented_library_and_says_so() {
        let (_symdir, _env) = stub_symbol_dir().await;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("canon.kicad_sch");
        let ctx = test_ctx();

        handle_create_schematic(&json!({ "path": path.display().to_string() }), &ctx)
            .await
            .unwrap();
        let result = handle_add_schematic_component(
            &json!({
                "schematic": path.display().to_string(),
                "lib_id": "resistors/R",
                "x": 100.0, "y": 80.0,
                "reference": "R1"
            }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(
            !result.is_error,
            "'resistors/R' names exactly one installed symbol: {}",
            content_text(&result)
        );

        let out: serde_json::Value = serde_json::from_str(&content_text(&result)).unwrap();
        assert_eq!(out["added"], "Device:R");
        assert_eq!(
            out["lib_id_canonicalized_from"], "resistors/R",
            "a substitution the caller cannot see is one it will repeat"
        );
        let sch = cse::Schematic::load(&path).unwrap();
        assert_eq!(sch.symbols.by_reference("R1").unwrap().lib_id, "Device:R");
    }

    /// The other half of the same rule: no unique answer, no rewrite. A
    /// footprint name asked for as a symbol still fails with its did-you-mean
    /// list rather than landing on whatever ranked first.
    #[tokio::test]
    async fn add_component_refuses_to_guess_an_ambiguous_lib_id() {
        let (_symdir, _env) = stub_symbol_dir().await;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("noguess.kicad_sch");
        let ctx = test_ctx();

        handle_create_schematic(&json!({ "path": path.display().to_string() }), &ctx)
            .await
            .unwrap();
        let result = handle_add_schematic_component(
            &json!({
                "schematic": path.display().to_string(),
                "lib_id": "Resistor_SMD:R_0805",
                "x": 100.0, "y": 80.0,
                "reference": "R1"
            }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(result.is_error, "R_0805 is a footprint, not a symbol");
        assert!(
            content_text(&result).contains("Resistor_SMD:R_0805"),
            "the error must name the id the caller asked for: {}",
            content_text(&result)
        );
    }

    #[tokio::test]
    async fn add_component_writes_requested_unit() {
        let (_symdir, _env) = stub_symbol_dir().await;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("multi.kicad_sch");
        let ctx = test_ctx();

        handle_create_schematic(&json!({ "path": path.display().to_string() }), &ctx)
            .await
            .unwrap();
        let result = handle_add_schematic_component(
            &json!({
                "schematic": path.display().to_string(),
                "lib_id": "Device:OPAMP_DUAL",
                "x": 100.0, "y": 80.0,
                "reference": "U1",
                "unit": 3
            }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(!result.is_error, "unit 3 of a 3-unit part must be accepted");

        let sch = cse::Schematic::load(&path).unwrap();
        let sym = sch.symbols.by_reference("U1").unwrap();
        assert_eq!(sym.unit, 3, "symbol (unit N) must match the requested unit");
        let root_uuid = sch.uuid.clone().unwrap();
        // Instance entry must carry the same unit, not a hardcoded 1.
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains(&format!("/{}", root_uuid)));
        assert!(raw.contains("(unit 3)"), "instance unit must be 3");
    }

    fn content_text(res: &CallToolResult) -> String {
        match res.content.first() {
            Some(crate::mcp::protocol::ToolContent::Text { text }) => text.clone(),
            other => panic!("expected text content, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn add_component_rejects_out_of_range_unit() {
        let (_symdir, _env) = stub_symbol_dir().await;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("units.kicad_sch");
        let ctx = test_ctx();

        handle_create_schematic(&json!({ "path": path.display().to_string() }), &ctx)
            .await
            .unwrap();
        let before = std::fs::read_to_string(&path).unwrap();

        for bad_unit in [0, 99] {
            let result = handle_add_schematic_component(
                &json!({
                    "schematic": path.display().to_string(),
                    "lib_id": "Device:OPAMP_DUAL",
                    "x": 100.0, "y": 80.0,
                    "reference": "U1",
                    "unit": bad_unit
                }),
                &ctx,
            )
            .await
            .unwrap();
            assert!(result.is_error, "unit {bad_unit} must be rejected");
            let text = content_text(&result);
            assert!(
                text.contains("3 unit"),
                "error must state the unit count: {text}"
            );
        }
        // A single-unit symbol only accepts unit 1.
        let result = handle_add_schematic_component(
            &json!({
                "schematic": path.display().to_string(),
                "lib_id": "Device:R",
                "x": 100.0, "y": 80.0,
                "reference": "R1",
                "unit": 2
            }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(
            result.is_error,
            "unit 2 of a 1-unit symbol must be rejected"
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            before,
            "rejected placements must not modify the schematic"
        );
    }

    #[tokio::test]
    async fn pin_locations_are_unit_aware() {
        // The #35 repro: an LM2904-style dual op-amp placed as unit 1 and as
        // unit 2 must report DISJOINT pin sets, not all units superimposed.
        let (_symdir, _env) = stub_symbol_dir().await;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dual.kicad_sch");
        let ctx = test_ctx();

        handle_create_schematic(&json!({ "path": path.display().to_string() }), &ctx)
            .await
            .unwrap();
        for (reference, unit, x) in [("U1", 1, 100.0), ("U2", 2, 150.0)] {
            let res = handle_add_schematic_component(
                &json!({
                    "schematic": path.display().to_string(),
                    "lib_id": "Device:OPAMP_DUAL",
                    "x": x, "y": 80.0,
                    "reference": reference,
                    "unit": unit
                }),
                &ctx,
            )
            .await
            .unwrap();
            assert!(!res.is_error, "placing {reference}: {:?}", res.content);
        }

        let pin_numbers = |res: &CallToolResult| -> Vec<String> {
            let out: serde_json::Value = serde_json::from_str(&content_text(res)).unwrap();
            let mut nums: Vec<String> = out["pins"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| p["number"].as_str().unwrap().to_string())
                .collect();
            nums.sort();
            nums
        };

        let u1 = handle_get_schematic_pin_locations(
            &json!({ "schematic": path.display().to_string(), "reference": "U1" }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(!u1.is_error);
        assert_eq!(pin_numbers(&u1), vec!["1", "2", "3"], "unit 1 pins only");

        let u2 = handle_get_schematic_pin_locations(
            &json!({ "schematic": path.display().to_string(), "reference": "U2" }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(!u2.is_error);
        assert_eq!(pin_numbers(&u2), vec!["5", "6", "7"], "unit 2 pins only");

        // Batch variant agrees.
        let batch = handle_batch_get_pin_locations(
            &json!({
                "schematic": path.display().to_string(),
                "references": ["U1", "U2"]
            }),
            &ctx,
        )
        .await
        .unwrap();
        let out: serde_json::Value = serde_json::from_str(&content_text(&batch)).unwrap();
        let comps = out["components"].as_array().unwrap();
        let nums = |i: usize| -> Vec<String> {
            let mut v: Vec<String> = comps[i]["pins"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| p["number"].as_str().unwrap().to_string())
                .collect();
            v.sort();
            v
        };
        assert_eq!(nums(0), vec!["1", "2", "3"]);
        assert_eq!(nums(1), vec!["5", "6", "7"]);
    }

    #[tokio::test]
    async fn pin_locations_error_on_extends_stub_with_zero_pins() {
        // A pre-flattening schematic: the embedded definition for the derived
        // symbol is an (extends "Parent") stub with no pins. The #34 guard
        // only catches MISSING definitions; a resolving-but-pinless stub must
        // be a structured error too, not pins:[] (#35).
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("stub.kicad_sch");
        std::fs::write(
            &path,
            "(kicad_sch\n\t(version 20250610)\n\t(generator \"konnect\")\n\t(uuid \"11111111-2222-3333-4444-555555555555\")\n\t(lib_symbols\n\t\t(symbol \"Device:OPAMP_DERIVED\"\n\t\t\t(extends \"Device:OPAMP_DUAL\")\n\t\t\t(property \"Reference\" \"U\" (at 0 0 0))\n\t\t)\n\t)\n\t(symbol\n\t\t(lib_id \"Device:OPAMP_DERIVED\")\n\t\t(at 100 80 0)\n\t\t(unit 1)\n\t\t(uuid \"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee\")\n\t\t(property \"Reference\" \"U1\"\n\t\t\t(at 102 78 0)\n\t\t)\n\t)\n)\n",
        )
        .unwrap();
        let ctx = test_ctx();

        let res = handle_get_schematic_pin_locations(
            &json!({ "schematic": path.display().to_string(), "reference": "U1" }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(res.is_error, "extends stub with zero pins must be an error");
        let text = content_text(&res);
        assert!(
            text.contains("Device:OPAMP_DERIVED"),
            "error must name the lib_id: {text}"
        );
        assert!(
            text.contains("Device:OPAMP_DUAL"),
            "error must name the extends target: {text}"
        );

        // Batch variant reports it per-entry.
        let batch = handle_batch_get_pin_locations(
            &json!({
                "schematic": path.display().to_string(),
                "references": ["U1"]
            }),
            &ctx,
        )
        .await
        .unwrap();
        let out: serde_json::Value = serde_json::from_str(&content_text(&batch)).unwrap();
        let err = out["components"][0]["error"].as_str().unwrap_or("");
        assert!(
            err.contains("Device:OPAMP_DUAL"),
            "batch entry must carry the stub error: {out}"
        );
    }

    #[tokio::test]
    async fn replace_component_sets_validated_unit() {
        let (_symdir, _env) = stub_symbol_dir().await;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("swap.kicad_sch");
        let ctx = test_ctx();

        handle_create_schematic(&json!({ "path": path.display().to_string() }), &ctx)
            .await
            .unwrap();
        handle_add_schematic_component(
            &json!({
                "schematic": path.display().to_string(),
                "lib_id": "Device:OPAMP_DUAL",
                "x": 100.0, "y": 80.0,
                "reference": "U1",
                "unit": 1
            }),
            &ctx,
        )
        .await
        .unwrap();

        // Out-of-range unit on the new symbol is rejected before any write.
        let before = std::fs::read_to_string(&path).unwrap();
        let bad = handle_replace_component(
            &json!({
                "schematic": path.display().to_string(),
                "reference": "U1",
                "new_lib_id": "Device:OPAMP_DUAL",
                "unit": 99
            }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(bad.is_error, "unit 99 must be rejected");
        assert!(content_text(&bad).contains("3 unit"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);

        // Valid unit is written to the symbol and its instances entry.
        let ok = handle_replace_component(
            &json!({
                "schematic": path.display().to_string(),
                "reference": "U1",
                "new_lib_id": "Device:OPAMP_DUAL",
                "unit": 2
            }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(!ok.is_error, "{:?}", ok.content);
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(
            raw.contains("(unit 2)"),
            "unit must be updated to 2:\n{raw}"
        );
        assert!(
            !raw.contains("(unit 1)"),
            "no stale (unit 1) may remain in the instance:\n{raw}"
        );
        let sch = cse::Schematic::load(&path).unwrap();
        assert_eq!(sch.symbols.by_reference("U1").unwrap().unit, 2);
    }

    #[tokio::test]
    async fn add_component_repairs_legacy_file_without_root_uuid() {
        let (_symdir, _env) = stub_symbol_dir().await;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("legacy.kicad_sch");
        // File shape produced by Konnect before root UUIDs were written.
        std::fs::write(
            &path,
            "(kicad_sch\n\t(version 20250610)\n\t(generator \"konnect\")\n\t(generator_version \"10.0\")\n\t(paper \"A4\")\n\t(lib_symbols\n\t)\n)\n",
        )
        .unwrap();
        let ctx = test_ctx();

        let result = handle_add_schematic_component(
            &json!({
                "schematic": path.display().to_string(),
                "lib_id": "Device:R",
                "x": 50.0, "y": 50.0,
                "reference": "R1"
            }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(!result.is_error);

        let sch = cse::Schematic::load(&path).unwrap();
        let root_uuid = sch.uuid.clone().expect("legacy file gains a root uuid");
        let sym = sch.symbols.by_reference("R1").unwrap();
        assert!(sym.has_instance_path("legacy", &format!("/{}", root_uuid)));
    }

    #[tokio::test]
    async fn add_component_with_nonexistent_lib_id_errors_with_suggestion() {
        let (_symdir, _env) = stub_symbol_dir().await;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ghost.kicad_sch");
        let ctx = test_ctx();

        handle_create_schematic(&json!({ "path": path.display().to_string() }), &ctx)
            .await
            .unwrap();
        let before = std::fs::read_to_string(&path).unwrap();

        // Device:CP is the KiCAD ≤9 name; 10 renamed it to C_Polarized (#34).
        let result = handle_add_schematic_component(
            &json!({
                "schematic": path.display().to_string(),
                "lib_id": "Device:CP",
                "x": 100.0, "y": 80.0,
                "reference": "C1"
            }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(result.is_error, "nonexistent lib_id must be an error");
        let msg = format!("{:?}", result.content);
        assert!(msg.contains("Device:CP"), "names the bad lib_id: {msg}");
        assert!(
            msg.contains("C_Polarized"),
            "did-you-mean should surface the rename: {msg}"
        );

        // And nothing was written: no ghost instance in the file.
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    }

    #[tokio::test]
    async fn add_component_with_unknown_library_says_so() {
        let (_symdir, _env) = stub_symbol_dir().await;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nolib.kicad_sch");
        let ctx = test_ctx();

        handle_create_schematic(&json!({ "path": path.display().to_string() }), &ctx)
            .await
            .unwrap();
        let result = handle_add_schematic_component(
            &json!({
                "schematic": path.display().to_string(),
                "lib_id": "Transistor_FET_xyzzy:IRF830",
                "x": 100.0, "y": 80.0
            }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(result.is_error);
        let msg = format!("{:?}", result.content);
        assert!(
            msg.contains("Library 'Transistor_FET_xyzzy' not found"),
            "distinguishes missing library from missing symbol: {msg}"
        );
    }

    #[tokio::test]
    async fn pin_locations_error_when_definition_not_embedded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("noembed.kicad_sch");
        // A symbol instance whose lib_id has NO lib_symbols entry — the file
        // shape a ghost lib_id used to leave behind (#34).
        std::fs::write(
            &path,
            "(kicad_sch\n\t(version 20250610)\n\t(generator \"konnect\")\n\t(uuid \"11111111-2222-3333-4444-555555555555\")\n\t(lib_symbols\n\t)\n\t(symbol\n\t\t(lib_id \"Device:CP\")\n\t\t(at 100 80 0)\n\t\t(unit 1)\n\t\t(uuid \"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee\")\n\t\t(property \"Reference\" \"C1\"\n\t\t\t(at 102 78 0)\n\t\t)\n\t)\n)\n",
        )
        .unwrap();
        let ctx = test_ctx();

        let result = handle_get_schematic_pin_locations(
            &json!({
                "schematic": path.display().to_string(),
                "reference": "C1"
            }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(
            result.is_error,
            "missing embedded definition must be an error, not pins: []"
        );
        let msg = format!("{:?}", result.content);
        assert!(msg.contains("Device:CP"));
        assert!(msg.contains("no embedded definition"));
    }

    #[tokio::test]
    async fn add_schematic_component_hides_power_reference() {
        // Pre-seed lib_symbols so ensure_lib_symbol succeeds without KiCad.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("power-via-add.kicad_sch");
        std::fs::write(
            &path,
            "(kicad_sch\n  (version 20250610)\n  (generator \"konnect\")\n  (uuid \"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee\")\n  (paper \"A4\")\n  (lib_symbols\n    (symbol \"power:GND\"\n      (property \"Reference\" \"#PWR\" (at 0 0 0) (hide yes))\n      (property \"Value\" \"GND\" (at 0 0 0))\n    )\n  )\n)\n",
        )
        .unwrap();

        let result = handle_add_schematic_component(
            &json!({
                "schematic": path.display().to_string(),
                "lib_id": "power:GND",
                "x": 50.0,
                "y": 60.0,
                "reference": "#PWR010",
                "value": "GND"
            }),
            &test_ctx(),
        )
        .await
        .unwrap();
        assert!(!result.is_error, "{result:?}");

        let sch = cse::Schematic::load(&path).unwrap();
        let sym = sch
            .symbols
            .iter()
            .find(|s| s.reference() == Some("#PWR010"))
            .expect("power instance");
        let ref_sexp = cse::sexp::writer::write(
            &sym.properties
                .iter()
                .find(|p| p.name == "Reference")
                .unwrap()
                .to_sexp(),
        );
        let hide_at = ref_sexp.find("(hide yes)").expect("property-level hide");
        let effects_at = ref_sexp.find("(effects").expect("effects");
        assert!(
            hide_at < effects_at,
            "power: via add_schematic_component must hide Reference like add_power_symbol: {ref_sexp}"
        );
        let val_sexp = cse::sexp::writer::write(
            &sym.properties
                .iter()
                .find(|p| p.name == "Value")
                .unwrap()
                .to_sexp(),
        );
        assert!(
            !val_sexp.contains("hide"),
            "Value stays visible: {val_sexp}"
        );
    }
}
