//! `sch_components` toolset — add, edit, move, rotate, delete schematic symbols.
//!
//! Simple CRUD operations use `konnect_schematic_editor` (cse) for structured
//! round-trip parsing.  Pin coordinate math still delegates to
//! `konnect_sexp::geometry::transform_pin`.

use crate::mcp::error::ToolErrorKind;
use crate::mcp::protocol::CallToolResult;
use crate::tool;
use crate::tools::{
    find_all_symbol_instance_blocks, find_symbol_instance_block, get_path, opt_f64, opt_str,
    require_f64, require_str, set_symbol_property, set_symbol_property_on_all_units,
    SetPropertyError, SetPropertyOutcome, ToolContext, ToolDef, RESERVED_PROPERTY_KEYS,
};
use konnect_schematic_editor as cse;
use konnect_sexp::{
    commit_command,
    geometry::snap_point,
    parse_sexp,
    schematic::{
        extract_lib_pins_for_unit, extract_symbol_instances, find_lib_symbol, pin_endpoint,
        read_schematic,
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
            "Remove a symbol instance from the schematic by reference designator or UUID.",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string" },
                    "reference": { "type": "string", "description": "Reference designator (e.g. 'R1')" },
                    "uuid": { "type": "string", "description": "Symbol UUID; pass this or 'reference'" }
                },
                "required": ["schematic"]
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
                    "uuid": { "type": "string", "description": "Symbol UUID; pass this or 'reference'" },
                    "new_reference": { "type": "string", "description": "New reference designator (optional)" },
                    "value": { "type": "string", "description": "New value (optional)" },
                    "footprint": { "type": "string", "description": "New footprint (optional)" },
                    "datasheet": { "type": "string", "description": "New datasheet URL (optional)" },
                    "fields": {
                        "type": "object",
                        "description": "Additional property fields to set as key:value pairs"
                    }
                },
                "required": ["schematic"]
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
                    "reference": { "type": "string" },
                    "uuid": { "type": "string", "description": "Symbol UUID; pass this or 'reference'" }
                },
                "required": ["schematic"]
            }),
            |args, ctx| async move { handle_get_schematic_component(args, ctx).await }
        ),
        tool!(
            "list_schematic_components",
            "List all symbol instances in a schematic with their positions, values, \
             footprints, uuids, and pin locations. A symbol's uuid is what addresses \
             it in every editing tool here, alongside its reference.",
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
                    "uuid": { "type": "string", "description": "Symbol UUID; pass this or 'reference'" },
                    "x": { "type": "number", "description": "New X position in mm" },
                    "y": { "type": "number", "description": "New Y position in mm" }
                },
                "required": ["schematic", "x", "y"]
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
                    "uuid": { "type": "string", "description": "Symbol UUID; pass this or 'reference'" },
                    "rotation": { "type": "number", "description": "Absolute rotation in degrees" }
                },
                "required": ["schematic", "rotation"]
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
                    "uuid": { "type": "string", "description": "Symbol UUID; pass this or 'reference'" },
                    "x": { "type": "number" },
                    "y": { "type": "number" }
                },
                "required": ["schematic", "x", "y"]
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
                    "reference": { "type": "string" },
                    "uuid": { "type": "string", "description": "Symbol UUID; pass this or 'reference'" }
                },
                "required": ["schematic"]
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
                    },
                    "uuids": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Symbol UUIDs; pass these or 'references', or both"
                    }
                },
                "required": ["schematic"]
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
                    "uuid": { "type": "string", "description": "Symbol UUID; pass this or 'reference'" },
                    "key": { "type": "string", "description": "Property name" },
                    "value": { "type": "string", "description": "Property value" }
                },
                "required": ["schematic", "key", "value"]
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
                    "uuids": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Symbol UUIDs; pass these or 'references', or both"
                    },
                    "group_name": { "type": "string", "description": "Group name to assign" }
                },
                "required": ["schematic", "group_name"]
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
                    "uuid": { "type": "string", "description": "Symbol UUID; pass this or 'reference'" },
                    "new_lib_id": { "type": "string", "description": "New Library:Symbol identifier (e.g. 'Device:C')" },
                    "unit": { "type": "integer", "description": "Optional unit number for multi-unit symbols; validated against the new symbol's unit count. When omitted the existing unit is kept." }
                },
                "required": ["schematic", "new_lib_id"]
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

/// One `NotFound` for "this schematic has no component with that reference".
///
/// Five sites in this file said it in prose, and the prose differs between
/// them ("not found in schematic" vs "not found"), which is exactly why the
/// classification lives here and the message stays theirs.
fn component_not_found(
    sch_path: &std::path::Path,
    reference: &str,
    message: String,
) -> CallToolResult {
    CallToolResult::error_kind(
        ToolErrorKind::NotFound {
            document: sch_path.display().to_string(),
            item_kind: "component".to_string(),
            key: reference.to_string(),
            candidates: Vec::new(),
        },
        message,
    )
}

/// The one symbol a call addresses, in whichever shape the handler holding it
/// can look up: a position in a `cse::Schematic`, a position among the parsed
/// instances, or a byte range in the document text.
///
/// Multi-unit symbols are why this is a target and not a designator (D.4.1.7):
/// the units of one symbol are separate top-level `(symbol …)` blocks sharing
/// one designator, so a handler that redescends by designator lands on the
/// first unit whatever the call named. The uuid is kept beside the designator
/// and every lookup below goes through it, so the unit named is the unit
/// edited. A call that gave a designator resolves to the first symbol carrying
/// it — exactly what it always did (INV8).
struct ComponentTarget {
    /// The designator: what the results and the error messages say, and what
    /// the designator path looks up by. Read out of the block on the uuid path
    /// (D80), never echoed back from the request.
    reference: String,
    /// `Some` only when the call addressed the symbol by uuid.
    uuid: Option<String>,
}

impl ComponentTarget {
    /// Position of the addressed symbol in `symbols`.
    ///
    /// A position, not a second address: the caller reaches the symbol with
    /// `get_mut` / `remove_at` from here rather than re-resolving it (D81).
    fn index_in(&self, symbols: &cse::SymbolCollection) -> Option<usize> {
        match &self.uuid {
            Some(uuid) => symbols.iter().position(|s| s.uuid == *uuid),
            None => symbols
                .iter()
                .position(|s| s.reference() == Some(self.reference.as_str())),
        }
    }

    /// Position of the addressed symbol among `extract_symbol_instances`, for
    /// the handlers that hold the parsed tree instead of a `cse::Schematic`.
    fn instance_index(
        &self,
        instances: &[konnect_sexp::schematic::SymbolInstance],
    ) -> Option<usize> {
        match &self.uuid {
            Some(uuid) => instances
                .iter()
                .position(|i| i.uuid.as_deref() == Some(uuid.as_str())),
            None => instances.iter().position(|i| i.reference == self.reference),
        }
    }

    /// Byte range of the addressed symbol's own block in `content`, for the
    /// handlers that edit the document text.
    fn block_in(&self, content: &str) -> Option<(usize, usize)> {
        self.block_by(content, &self.reference)
    }

    /// [`block_in`](Self::block_in) for a caller that has just renamed the
    /// symbol: on the designator path the block now answers to `reference`,
    /// while a uuid is untouched by a rename.
    fn block_by(&self, content: &str, reference: &str) -> Option<(usize, usize)> {
        let Some(uuid) = &self.uuid else {
            return find_symbol_instance_block(content, reference);
        };
        konnect_sexp::item_locations(content)
            .ok()?
            .iter()
            .find(|item| {
                item.id.as_str() == uuid.as_str() && item.kind.as_deref() == Some("symbol")
            })
            .map(|item| (item.start, item.end))
    }

    /// The uuid of every placed unit sharing this target's designator, in
    /// document order — for a "not found" or refusal message that names the
    /// units a plain designator lookup would otherwise silently collapse to
    /// one of.
    fn sibling_unit_uuids(&self, content: &str) -> Vec<String> {
        find_all_symbol_instance_blocks(content, &self.reference)
            .iter()
            .filter_map(|&(start, end)| crate::tools::symbol_block_uuid(&content[start..end]))
            .collect()
    }
}

/// Refuse a geometry call (move / rotate) addressed by `reference` when the
/// symbol has more than one placed unit.
///
/// Moving or rotating one unit of a multi-unit symbol without the others is
/// legitimate — it is the only thing eeschema itself does, since a symbol's
/// units can sit anywhere on the sheet — so writing to "every unit" the way
/// a property edit does (P.6.8.2) would be wrong here. But silently picking
/// the first unit, as a plain designator lookup does, is wrong too: it moves
/// a unit the caller never named and leaves the others exactly where they
/// were, with no sign anything but `reference` was addressed. Refusing names
/// the units and their uuids and says to address one by `uuid` instead.
///
/// A `uuid`-addressed target already names one specific unit and a
/// single-unit symbol has nothing to disambiguate, so both are let through
/// unchanged.
fn refuse_ambiguous_multiunit_geometry(
    content: &str,
    target: &ComponentTarget,
) -> Result<(), CallToolResult> {
    if target.uuid.is_some() {
        return Ok(());
    }
    let blocks = find_all_symbol_instance_blocks(content, &target.reference);
    if blocks.len() <= 1 {
        return Ok(());
    }
    let uuids: Vec<String> = blocks
        .iter()
        .filter_map(|&(start, end)| crate::tools::symbol_block_uuid(&content[start..end]))
        .collect();
    let reference = &target.reference;
    Err(CallToolResult::error_kind(
        ToolErrorKind::InvalidArgument {
            field: "reference".to_string(),
            reason: format!(
                "'{reference}' has {} units ({}); address one unit by its 'uuid' instead of by 'reference'",
                blocks.len(),
                uuids.join(", ")
            ),
        },
        format!(
            "'{reference}' has {} placed units ({}); move/rotate needs one 'uuid', not a shared 'reference'",
            blocks.len(),
            uuids.join(", ")
        ),
    ))
}

/// The symbol a call addresses by `reference` or `uuid` — whichever it
/// carries.
///
/// A call carrying `reference` is resolved without reading anything: the
/// existing path, its "not found" errors included, stays exactly what it was
/// (INV8). Only a `uuid` call pays for a read *here* — the geometry handlers
/// read on their own account afterwards, for
/// [`refuse_ambiguous_multiunit_geometry`], which cannot answer without
/// seeing the document.
///
/// # Errors
///
/// The outer `Err` is a document read failure, as everywhere else in this
/// file; the inner one is the [`CallToolResult`] to hand back.
fn resolve_component_target(
    args: &serde_json::Value,
    sch_path: &std::path::Path,
) -> anyhow::Result<Result<ComponentTarget, Box<CallToolResult>>> {
    if let Some(reference) = opt_str(args, "reference") {
        return Ok(Ok(ComponentTarget {
            reference: reference.to_string(),
            uuid: None,
        }));
    }
    let content = read_consistent(sch_path)?;
    let resolved = match crate::tools::resolve_component(&content, args, sch_path) {
        Ok(resolved) => resolved,
        Err(e) => return Ok(Err(e)),
    };
    Ok(Ok(ComponentTarget {
        reference: resolved.reference,
        // Past the `reference` branch `resolve_component` only succeeds on a
        // call that carries a `uuid`, so this is the address it matched.
        uuid: opt_str(args, "uuid").map(str::to_string),
    }))
}

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
    let (project_name, instance_paths) = crate::tools::instance_targets(&sch_path, &mut sch);

    let result = match place_one_component(
        &mut sch,
        &instance_paths,
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
    instance_paths: &[String],
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
        return Err(CallToolResult::error_kind(
            ToolErrorKind::InvalidArgument {
                field: "unit".to_string(),
                reason: format!("valid range is 1..={unit_count} for '{lib_id}'"),
            },
            format!(
                "Invalid unit {} for '{}': the symbol has {} unit(s) (valid: 1..={}).",
                unit, lib_id, unit_count, unit_count
            ),
        ));
    }

    // Build the Symbol struct
    let mut sym = cse::Symbol::new(lib_id, x, y);
    sym.at.rotation = Some(rotation);
    sym.unit = unit;

    // The four fields go where the library symbol puts them, transformed by
    // this placement (P.6.8.8). Power symbols get their Reference hidden too,
    // matching eeschema: a #PWR designator is never shown on the sheet, and
    // the `power:` library already hides its own.
    let hide_reference = lib_id.starts_with("power:") || reference.starts_with("#PWR");
    crate::tools::push_placed_fields(sch, &mut sym, reference, val_str, hide_reference);

    // Instance entry, keyed to the root sheet UUID like eeschema writes it:
    // (instances (project "<name>" (path "/<root-uuid>" (reference ...) (unit 1))))
    // One entry per placement of this sheet in the hierarchy: a sheet
    // instantiated twice carries two paths, and a symbol written with only one
    // of them is annotated in one instance and invisible in the other.
    for path in instance_paths {
        sym.set_instance_path(project_name, path, reference, unit);
    }

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
    let target = match resolve_component_target(args, &sch_path)? {
        Ok(t) => t,
        Err(e) => return Ok(*e),
    };
    let reference = target.reference.clone();

    let mut sch = cse::Schematic::load(&sch_path)?;

    // A reference-addressed delete drops every unit sharing that designator:
    // leaving one behind is a half-deleted component. A uuid names one unit
    // specifically and only that one is removed, matching `move`/`rotate`'s
    // uuid path.
    let deleted_units: Option<usize> = if target.uuid.is_some() {
        target
            .index_in(&sch.symbols)
            .and_then(|i| sch.symbols.remove_at(i))
            .map(|_| 1)
    } else {
        let before = sch.symbols.len();
        sch.symbols
            .retain(|s| s.reference() != Some(reference.as_str()));
        let removed = before - sch.symbols.len();
        (removed > 0).then_some(removed)
    };

    match deleted_units {
        Some(units_deleted) => {
            sch.overwrite()?;
            Ok(CallToolResult::json(&json!({
                "deleted": reference,
                "units_deleted": units_deleted
            })))
        }
        None => Ok(component_not_found(
            &sch_path,
            &reference,
            format!("Component '{}' not found in schematic", reference),
        )),
    }
}

async fn handle_edit_schematic_component(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let target = match resolve_component_target(args, &sch_path)? {
        Ok(t) => t,
        Err(e) => return Ok(*e),
    };
    let reference = target.reference.clone();

    let mut content = read_consistent(&sch_path)?;
    let expected = content.clone();
    let mut changed = Vec::new();

    let mut errors: Vec<String> = Vec::new();

    // The `fields` map was declared in the schema and never read, so a call
    // carrying only custom properties wrote nothing and still reported success
    // (P.6.9.6). Values are turned into their stored text here, before the
    // `apply` closure below borrows `errors`: a rejected value and a rejected
    // key both belong in the same list, and doing the conversion first is what
    // keeps one mutable borrow of `errors` live at a time.
    let mut field_updates: Vec<(&str, String)> = Vec::new();
    match args.get("fields") {
        None | Some(serde_json::Value::Null) => {}
        Some(serde_json::Value::Object(map)) => {
            for (key, raw) in map {
                // KiCAD stores every property as text, so a JSON number or
                // boolean is written as its text form; anything with no text
                // form is refused rather than stringified into nonsense.
                match property_text(raw) {
                    Some(text) => field_updates.push((key.as_str(), text)),
                    None => {
                        errors.push(format!("{key}: value must be a string, number or boolean"))
                    }
                }
            }
        }
        Some(_) => {
            let reason = "'fields' must be an object of key:value pairs";
            return Ok(CallToolResult::error_kind(
                ToolErrorKind::InvalidArgument {
                    field: "fields".to_string(),
                    reason: reason.to_string(),
                },
                reason.to_string(),
            ));
        }
    }

    // Set a property, adding it when the symbol has none.
    //
    // A symbol carries only the properties it was given: a part placed without
    // a footprint has no `Footprint` property at all, and refusing to set one
    // made the tool unable to do the single most common edit after placement
    // (J.2.4.1). A missing property is now created, exactly as
    // `add_component_annotation` creates one.
    //
    // `reject` is empty for the named arguments — they are exactly the fields
    // this handler is meant to set this way, `Reference` included: it goes
    // through `update_instance_reference` right after. The free-form `fields`
    // map passes `RESERVED_PROPERTY_KEYS`, because that generic path rewrites
    // only the property and would desynchronise the designator copy that lives
    // in `(instances …)`.
    let mut apply =
        |content: &mut String, field: &str, new_val: &str, reject: &[&str]| match set_field(
            content, &target, field, new_val, reject,
        ) {
            Ok((updated, outcome)) => {
                *content = updated;
                changed.push(match outcome {
                    SetPropertyOutcome::Updated => format!("{} → {}", field, new_val),
                    SetPropertyOutcome::Inserted => format!("{} → {} (added)", field, new_val),
                });
            }
            Err(why) => errors.push(format!("{field}: {why}")),
        };

    // On the designator path every field is located by looking the symbol up
    // by `reference`, so the rename has to go last: renaming first made the
    // symbol unfindable and every other field in the same call came back
    // "symbol 'R2' not found".
    if let Some(val) = opt_str(args, "value") {
        apply(&mut content, "Value", val, &[]);
    }
    if let Some(fp) = opt_str(args, "footprint") {
        apply(&mut content, "Footprint", fp, &[]);
    }
    if let Some(ds) = opt_str(args, "datasheet") {
        apply(&mut content, "Datasheet", ds, &[]);
    }
    for (key, value) in &field_updates {
        apply(&mut content, key, value, RESERVED_PROPERTY_KEYS);
    }
    if let Some(new_ref) = opt_str(args, "new_reference") {
        apply(&mut content, "Reference", new_ref, &[]);
        // KiCAD resolves a symbol's designator from its `instances` block, not
        // from the Reference property. Renaming only the property left
        // `kicad-cli sch export netlist` still emitting the old designator
        // while this tool reported success (J.2.3.2).
        match update_instance_reference(&content, &target, new_ref, &reference) {
            Ok(updated) => content = updated,
            Err(why) => errors.push(format!("instances: {why}")),
        }
    }

    // A request that changed nothing is a failure, not a success — silently
    // reporting `"changes": []` is what let the tab-indentation bug hide, and
    // what let an edit with no editable argument at all look like it worked.
    if changed.is_empty() {
        let reason = if errors.is_empty() {
            "no editable field was given".to_string()
        } else {
            errors.join("; ")
        };
        return Ok(CallToolResult::error_kind(
            ToolErrorKind::InvalidArgument {
                field: "fields".to_string(),
                reason: reason.clone(),
            },
            format!("No fields were updated on '{reference}': {reason}"),
        ));
    }

    // Past the guard above, `changed` is never empty: something was written.
    //
    // A reference-addressed target has every one of its units' properties
    // rewritten in `content` above (P.6.8.2), so committing only the first
    // unit's `ItemId` here would drop the other units' writes on the floor —
    // present in `content`, never reaching disk. Every sibling unit's own
    // `ItemId` (read from `expected`, before any edit) is committed in the
    // same atomic change instead. A uuid-addressed target still names one.
    let item_ids: Vec<ItemId> = if target.uuid.is_none() {
        find_all_symbol_instance_blocks(&expected, &reference)
            .iter()
            .map(|&(s, e)| symbol_block_item_id(&expected[s..e]))
            .collect::<anyhow::Result<Vec<_>>>()?
    } else {
        let (start, end) = target
            .block_in(&expected)
            .ok_or_else(|| anyhow::anyhow!("component '{reference}' not found"))?;
        vec![symbol_block_item_id(&expected[start..end])?]
    };
    let command = SchematicCommand::replace_items_from_document(
        &expected,
        &content,
        item_ids,
        format!("Edit {reference}"),
    )?;
    commit_command(&sch_path, &command)?;

    let mut result = json!({
        "reference": reference,
        "changes": changed
    });
    if !errors.is_empty() {
        result["errors"] = json!(errors);
    }
    Ok(CallToolResult::json(&result))
}

/// The text KiCAD would store for a JSON property value: a string as-is, a
/// number or boolean as its text form, nothing at all for `null`, an object or
/// an array, which have no single-line text form a property could hold.
///
/// Shared with `batch_edit_schematic_components` (P.6.9.14): the batch path's
/// `fields` map is the same generic property path and has to accept, and
/// refuse, exactly the same values.
pub(crate) fn property_text(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Why a property edit could not be applied.
enum FieldError {
    SymbolNotFound(String),
    Set(SetPropertyError),
}

impl std::fmt::Display for FieldError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FieldError::SymbolNotFound(reference) => {
                write!(f, "symbol '{reference}' not found in this schematic")
            }
            FieldError::Set(e) => write!(f, "{e}"),
        }
    }
}

/// Set a property on the addressed symbol: update it in place if it already
/// has one, insert a new hidden one at the symbol's own position otherwise
/// ([`set_symbol_property`]). `reject` is the keys this call refuses to
/// touch even if the symbol already carries one — empty for the
/// `edit_schematic_component` fields, which are exactly the fields meant to
/// be set this way; [`RESERVED_PROPERTY_KEYS`] for `add_component_annotation`,
/// which is not.
///
/// Locates the symbol fresh in `content` every call rather than once up
/// front: on the designator path a preceding call in the same batch (e.g. a
/// rename) can have moved the block, and re-resolving by `target` here is
/// what keeps every subsequent field call landing on the right bytes.
///
/// A designator-addressed target (`target.uuid.is_none()`) writes `field` to
/// every unit sharing that designator, not just the first: `Value` /
/// `Footprint` / `Datasheet` and any custom field belong to the component,
/// not to one unit's placement (P.6.8.2, see
/// [`set_symbol_property_on_all_units`]). A uuid-addressed target names one
/// specific unit and only that one is written, unchanged from before.
fn set_field(
    content: &str,
    target: &ComponentTarget,
    field: &str,
    value: &str,
    reject: &[&str],
) -> Result<(String, SetPropertyOutcome), FieldError> {
    if target.uuid.is_none() {
        if find_all_symbol_instance_blocks(content, &target.reference).is_empty() {
            return Err(FieldError::SymbolNotFound(target.reference.clone()));
        }
        return set_symbol_property_on_all_units(content, &target.reference, field, value, reject)
            .map_err(FieldError::Set);
    }
    let (sym_start, sym_end) = target
        .block_in(content)
        .ok_or_else(|| FieldError::SymbolNotFound(target.reference.clone()))?;
    set_symbol_property(content, sym_start, sym_end, field, value, reject).map_err(FieldError::Set)
}

/// Point a renamed symbol's `instances` entries at its new designator.
///
/// `new_ref` is what the symbol's Reference property now says — that is how the
/// block is located — and `old_ref` is the designator the instance entries still
/// carry. Every entry is rewritten, because a symbol placed on several sheets
/// has one per sheet and leaving any of them behind means KiCAD reports two
/// different designators for the same symbol.
///
/// A designator-addressed target repoints every unit's own `instances`
/// entry, not just the first block that carries `new_ref`: `set_field`
/// already renamed the `Reference` property on every unit above, and leaving
/// unit 2's `instances` entry pointed at `old_ref` would desync it from its
/// own now-renamed property the same way the pre-P.6.8.2 single-block write
/// did for `Value`. A uuid-addressed target still repoints only its one unit.
fn update_instance_reference(
    content: &str,
    target: &ComponentTarget,
    new_ref: &str,
    old_ref: &str,
) -> Result<String, String> {
    let needle = format!(r#"(reference "{old_ref}")"#);
    let replacement = format!(r#"(reference "{new_ref}")"#);

    if target.uuid.is_none() {
        let mut blocks = find_all_symbol_instance_blocks(content, new_ref);
        if blocks.is_empty() {
            return Err(format!("symbol '{new_ref}' not found after the rename"));
        }
        blocks.sort_by_key(|b| std::cmp::Reverse(b.0));
        let mut updated = content.to_string();
        for (start, end) in blocks {
            let block = &updated[start..end];
            if !block.contains(&needle) {
                continue;
            }
            updated = format!(
                "{}{}{}",
                &updated[..start],
                block.replace(&needle, &replacement),
                &updated[end..]
            );
        }
        return Ok(updated);
    }

    let (start, end) = target
        .block_by(content, new_ref)
        .ok_or_else(|| format!("symbol '{new_ref}' not found after the rename"))?;
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
    let target = match resolve_component_target(args, &sch_path)? {
        Ok(t) => t,
        Err(e) => return Ok(*e),
    };
    let reference = target.reference.clone();
    // A designator-addressed call resolves to the first unit's block
    // (`find_symbol_instance_block`'s documented behaviour); the sibling
    // uuids say so explicitly instead of leaving the caller to assume there
    // is only one.
    let sibling_units = target.sibling_unit_uuids(&read_consistent(&sch_path)?);

    let sch = cse::Schematic::load(&sch_path)?;

    match target
        .index_in(&sch.symbols)
        .and_then(|i| sch.symbols.get(i))
    {
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
                "uuid": sym.uuid,
                "unit": sym.unit,
                "sibling_unit_uuids": sibling_units
                    .into_iter()
                    .filter(|u| u != &sym.uuid)
                    .collect::<Vec<_>>()
            })))
        }
        None => Ok(component_not_found(
            &sch_path,
            &reference,
            format!("Component '{}' not found", reference),
        )),
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
                // The address every editing tool in this toolset accepts
                // (D.4.1.2). Without it here, obtaining one costs a second call.
                "uuid": sym.uuid,
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
    let target = match resolve_component_target(args, &sch_path)? {
        Ok(t) => t,
        Err(e) => return Ok(*e),
    };
    let reference = target.reference.clone();
    let guard_content = read_consistent(&sch_path)?;
    if let Err(e) = refuse_ambiguous_multiunit_geometry(&guard_content, &target) {
        return Ok(e);
    }
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

    match target
        .index_in(&sch.symbols)
        .and_then(|i| sch.symbols.get_mut(i))
    {
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
    let target = match resolve_component_target(args, &sch_path)? {
        Ok(t) => t,
        Err(e) => return Ok(*e),
    };
    let reference = target.reference.clone();
    let guard_content = read_consistent(&sch_path)?;
    if let Err(e) = refuse_ambiguous_multiunit_geometry(&guard_content, &target) {
        return Ok(e);
    }
    let rotation = match require_f64(args, "rotation") {
        Ok(v) => v,
        Err(e) => return Ok(e),
    };

    let mut sch = cse::Schematic::load(&sch_path)?;

    match target
        .index_in(&sch.symbols)
        .and_then(|i| sch.symbols.get_mut(i))
    {
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

/// Absolute pin positions of the addressed symbol, keyed by pin number.
///
/// Returns an empty map rather than an error when the component or its
/// embedded definition cannot be resolved: the caller uses this to *decide
/// whether* a wire end belongs to a pin, and having no pins to match simply
/// means nothing moves.
fn pin_positions(
    sch_path: &std::path::Path,
    target: &ComponentTarget,
) -> Vec<(String, (f64, f64))> {
    let Ok((_, tree)) = read_schematic(sch_path) else {
        return Vec::new();
    };
    let instances = extract_symbol_instances(&tree);
    let Some(inst) = target.instance_index(&instances).map(|i| &instances[i]) else {
        return Vec::new();
    };
    let lib_syms = tree
        .find("lib_symbols")
        .map(|n| n.find_all("symbol"))
        .unwrap_or_default();
    let Some(sym) = find_lib_symbol(&lib_syms, inst) else {
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
    let target = match resolve_component_target(args, &sch_path)? {
        Ok(t) => t,
        Err(e) => return Ok(*e),
    };

    let reference = target.reference.clone();

    let before = pin_positions(&sch_path, &target);
    let moved = handle_move_schematic_component(args, ctx).await?;
    if moved.is_error {
        return Ok(moved);
    }
    let after = pin_positions(&sch_path, &target);

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
    let target = match resolve_component_target(args, &sch_path)? {
        Ok(t) => t,
        Err(e) => return Ok(*e),
    };
    let reference = target.reference.clone();

    let (_, tree) = read_schematic(&sch_path)?;
    let instances = extract_symbol_instances(&tree);
    let inst = match target.instance_index(&instances).map(|i| &instances[i]) {
        Some(i) => i,
        None => {
            return Ok(component_not_found(
                &sch_path,
                &reference,
                format!("Component '{}' not found", reference),
            ))
        }
    };

    // Find the library symbol definition within the schematic's lib_symbols section
    let lib_syms = tree
        .find("lib_symbols")
        .map(|n| n.find_all("symbol"))
        .unwrap_or_default();
    let lib_sym = find_lib_symbol(&lib_syms, inst);

    // A missing embedded definition is an error, not an empty pin list —
    // silently returning [] hid every bad-lib_id component until wiring or
    // netlisting failed much later (#34).
    let Some(sym) = lib_sym else {
        return Ok(CallToolResult::error_kind(
            // NotFound, not MalformedDocument: the addressed item is what is
            // absent, and `lib_symbols` is exactly the place it should be.
            ToolErrorKind::NotFound {
                document: sch_path.display().to_string(),
                item_kind: "lib_symbols definition".to_string(),
                key: inst.lib_id.clone(),
                candidates: Vec::new(),
            },
            format!(
                "Component '{}' has no embedded definition for '{}' in this \
                 schematic's lib_symbols — it was likely added with a lib_id that \
                 doesn't exist in the installed libraries, so it is invisible to \
                 KiCAD's netlister. Re-add it with a valid lib_id \
                 (delete_schematic_component + add_schematic_component).",
                reference,
                inst.lib_symbol_name()
            ),
        ));
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
            return Ok(CallToolResult::error_kind(
                ToolErrorKind::MalformedDocument {
                    path: sch_path.display().to_string(),
                    detail: format!(
                        "the embedded definition for '{}' is an (extends \"{}\") \
                         stub with no pins of its own",
                        inst.lib_id, parent
                    ),
                },
                format!(
                    "Component '{}': the embedded definition for '{}' is an \
                 (extends \"{}\") stub with no pins of its own. kicad-cli \
                 cannot resolve extends stubs (the netlist gets a pinless \
                 part). Re-add the component (delete_schematic_component + \
                 add_schematic_component) so the definition is embedded in \
                 full, or place the parent symbol '{}' directly.",
                    reference,
                    inst.lib_symbol_name(),
                    parent,
                    parent
                ),
            ));
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
        // A designator-addressed call resolves to the first unit's block; this
        // says which unit (and which uuid) the pins below actually belong to,
        // since a multi-unit symbol's units sit at different positions.
        "unit": inst.unit,
        "uuid": inst.uuid,
        "pins": pins
    })))
}

async fn handle_batch_get_pin_locations(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let string_array = |field: &str| {
        args[field]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    if args["references"].is_null() && args["uuids"].is_null() {
        return Ok(CallToolResult::error_kind(
            ToolErrorKind::InvalidArgument {
                field: "references".to_string(),
                reason: "one of 'references' or 'uuids' is required to address components"
                    .to_string(),
            },
            "Missing component addresses: pass 'references' or 'uuids'",
        ));
    }
    let mut refs = string_array("references");
    let uuids = string_array("uuids");

    let (_, tree) = read_schematic(&sch_path)?; // single read
    let instances = extract_symbol_instances(&tree);
    // The uuids resolve against the instances this read already produced, so
    // the second address form costs no second read (D.4.1.6). An item of
    // another kind is not among them, so it lands in the same per-entry
    // "not found" a missing designator gets.
    let mut unresolved: Vec<serde_json::Value> = Vec::new();
    for uuid in &uuids {
        match instances
            .iter()
            .find(|i| i.uuid.as_deref() == Some(uuid.as_str()))
        {
            Some(instance) if !refs.contains(&instance.reference) => {
                refs.push(instance.reference.clone())
            }
            // Already named by a `references` entry: answered once.
            Some(_) => {}
            None => unresolved.push(json!({ "uuid": uuid, "error": "not found" })),
        }
    }
    let lib_syms = tree
        .find("lib_symbols")
        .map(|n| n.find_all("symbol"))
        .unwrap_or_default();

    let mut results: Vec<serde_json::Value> = refs
        .iter()
        .map(|reference| {
            let inst = match instances.iter().find(|i| &i.reference == reference) {
                Some(i) => i,
                None => return json!({ "reference": reference, "error": "not found" }),
            };
            let lib_sym = find_lib_symbol(&lib_syms, inst);
            // Per-entry error rather than a silent empty pin list (#34).
            let Some(sym) = lib_sym else {
                return json!({
                    "reference": reference,
                    "error": format!(
                        "no embedded definition for '{}' in lib_symbols — \
                         likely added with a nonexistent lib_id",
                        inst.lib_symbol_name()
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
                            inst.lib_symbol_name(), parent
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
    results.extend(unresolved);

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

    // Find the symbol block for this address. `reference` keeps its own lookup
    // and its own "not found" message (INV8); `uuid` goes through the shared
    // resolver, which hands back the exact unit's byte range.
    let is_reference_addressed = opt_str(args, "reference").is_some();
    let (reference, sym_start, sym_end) = match opt_str(args, "reference") {
        Some(reference) => match find_symbol_instance_block(&content, reference) {
            Some((start, end)) => (reference.to_string(), start, end),
            None => {
                return Ok(component_not_found(
                    &sch_path,
                    reference,
                    format!("Component '{}' not found", reference),
                ))
            }
        },
        None => match crate::tools::resolve_component(&content, args, &sch_path) {
            Ok(resolved) => (resolved.reference, resolved.start, resolved.end),
            Err(e) => return Ok(*e),
        },
    };

    // `Reference` is refused — and it alone. Its designator lives twice in
    // the file, in the property and in `(instances …)`, and only
    // `edit_schematic_component`'s `new_reference` path repoints the second
    // one; setting it here would leave KiCAD reading the stale designator.
    // `Value` / `Footprint` / `Datasheet` have a dedicated argument there too
    // but carry no such second copy, so writing one through this tool is
    // legitimate — the BOM audit already does exactly that with `Footprint`.
    // See `RESERVED_PROPERTY_KEYS`.
    if RESERVED_PROPERTY_KEYS.contains(&key.as_str()) {
        return Ok(CallToolResult::error_kind(
            ToolErrorKind::InvalidArgument {
                field: "key".to_string(),
                reason: format!(
                    "'{key}' is a reserved property; use edit_schematic_component to change it"
                ),
            },
            format!("'{key}' is a reserved property; use edit_schematic_component to change it"),
        ));
    }

    // Update the existing property in place if the symbol already has one
    // named `key` — a second call with the same key must not leave two
    // fields of the same name — otherwise insert a new hidden one at the
    // symbol's own position.
    //
    // A reference-addressed call writes every unit of that designator, for
    // the same reason `edit_schematic_component`'s `set_field` does
    // (P.6.8.2): this custom property is a component-level fact, not a
    // per-unit one. A uuid-addressed call still means one specific unit.
    let (new_content, outcome) = if is_reference_addressed {
        match set_symbol_property_on_all_units(&content, &reference, &key, &value, &[]) {
            Ok(r) => r,
            Err(why) => {
                return Ok(CallToolResult::error_kind(
                    ToolErrorKind::InvalidArgument {
                        field: "key".to_string(),
                        reason: why.to_string(),
                    },
                    why.to_string(),
                ))
            }
        }
    } else {
        match set_symbol_property(&content, sym_start, sym_end, &key, &value, &[]) {
            Ok(r) => r,
            Err(why) => {
                return Ok(CallToolResult::error_kind(
                    ToolErrorKind::InvalidArgument {
                        field: "key".to_string(),
                        reason: why.to_string(),
                    },
                    why.to_string(),
                ))
            }
        }
    };
    // From the resolved blocks in `expected` (pre-edit), not from a second
    // text search of `key` in `new_content`: the units of a multi-unit
    // symbol share a designator, and only these are the ones this call
    // meant to touch.
    let item_ids: Vec<ItemId> = if is_reference_addressed {
        find_all_symbol_instance_blocks(&expected, &reference)
            .iter()
            .map(|&(s, e)| symbol_block_item_id(&expected[s..e]))
            .collect::<anyhow::Result<Vec<_>>>()?
    } else {
        vec![symbol_block_item_id(&expected[sym_start..sym_end])?]
    };
    let verb = match outcome {
        SetPropertyOutcome::Updated => "Update",
        SetPropertyOutcome::Inserted => "Add",
    };
    let command = SchematicCommand::replace_items_from_document(
        &expected,
        &new_content,
        item_ids,
        format!("{verb} {key} property on {reference}"),
    )?;
    commit_command(&sch_path, &command)?;

    // `added_property` keeps its historical name — nothing in this repo reads
    // it, but it is part of a published tool's answer. What it can no longer
    // do is imply a creation that did not happen: this call now updates a
    // property the symbol already had instead of duplicating it, so `created`
    // says which of the two occurred.
    Ok(CallToolResult::json(&json!({
        "reference": reference,
        "added_property": key,
        "created": matches!(outcome, SetPropertyOutcome::Inserted),
        "value": value
    })))
}

fn symbol_item_id(content: &str, reference: &str) -> anyhow::Result<ItemId> {
    let (start, end) = find_symbol_instance_block(content, reference)
        .ok_or_else(|| anyhow::anyhow!("component '{reference}' not found"))?;
    symbol_block_item_id(&content[start..end])
}

/// The [`ItemId`] of one already-located symbol block, for callers that hold
/// the block and must not re-find it by designator.
fn symbol_block_item_id(block: &str) -> anyhow::Result<ItemId> {
    let symbol = parse_sexp(block)?;
    let uuid = symbol
        .find_str("uuid")
        .ok_or_else(|| anyhow::anyhow!("symbol block carries no UUID"))?;
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
    let mut content = read_consistent(&sch_path)?;
    let expected = content.clone();
    let batch = match crate::tools::resolve_component_batch(&content, args, &sch_path) {
        Ok(batch) => batch,
        Err(result) => return Ok(*result),
    };
    let refs = batch.references;

    if refs.is_empty() && batch.unresolved.is_empty() {
        return Ok(CallToolResult::error_kind(
            ToolErrorKind::InvalidArgument {
                field: "references".to_string(),
                reason: "must name at least one component".to_string(),
            },
            "No references provided",
        ));
    }

    let mut grouped = Vec::new();
    let mut item_ids = Vec::new();
    let mut batch_errors = batch.unresolved.clone();

    for reference in &refs {
        let (sym_start, sym_end) = match find_symbol_instance_block(&content, reference) {
            Some(r) => r,
            None => continue,
        };

        // The same shared helper the other two text paths use (P.6.9.5): it
        // updates an existing `Group` instead of stacking a second one,
        // anchors a new property on the symbol's own position rather than the
        // sheet origin, and reads its indentation off a sibling. `reject` is
        // empty because the key is the literal "Group" — there is no
        // caller-supplied name here that could collide with a reserved one.
        content = match set_symbol_property(&content, sym_start, sym_end, "Group", &group_name, &[])
        {
            Ok((updated, _)) => updated,
            Err(why) => {
                batch_errors.push(format!("{reference}: {why}"));
                continue;
            }
        };
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
        "grouped": grouped,
        "errors": batch_errors
    })))
}

async fn handle_replace_component(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let new_lib_id = match require_str(args, "new_lib_id") {
        Ok(v) => v.to_string(),
        Err(e) => return Ok(e),
    };
    let new_unit = opt_f64(args, "unit").map(|u| u as u32);

    let mut content = read_consistent(&sch_path)?;
    let expected = content.clone();

    // Find the symbol block for this address. `reference` keeps its own lookup
    // and its own "not found" message (INV8); `uuid` goes through the shared
    // resolver, which hands back the exact unit's byte range.
    let is_reference_addressed = opt_str(args, "reference").is_some();
    let (reference, sym_start, sym_end) = match opt_str(args, "reference") {
        Some(reference) => match find_symbol_instance_block(&content, reference) {
            Some((start, end)) => (reference.to_string(), start, end),
            None => {
                return Ok(component_not_found(
                    &sch_path,
                    reference,
                    format!("Component '{}' not found", reference),
                ))
            }
        },
        None => match crate::tools::resolve_component(&content, args, &sch_path) {
            Ok(resolved) => (resolved.reference, resolved.start, resolved.end),
            Err(e) => return Ok(*e),
        },
    };

    // Find the (lib_id "OLD") and replace it — searching only within this
    // symbol's block, so a malformed instance can't reach into the next one.
    let sym_block = &content[sym_start..sym_end];
    let lib_id_pat = "(lib_id \"";
    let lib_id_rel = match sym_block.find(lib_id_pat) {
        Some(o) => o,
        None => {
            return Ok(CallToolResult::error_kind(
                ToolErrorKind::MalformedDocument {
                    path: sch_path.display().to_string(),
                    detail: format!("the symbol block for '{reference}' has no (lib_id …)"),
                },
                "Could not find lib_id in symbol block",
            ))
        }
    };
    let lib_id_abs = sym_start + lib_id_rel + lib_id_pat.len();
    let lib_id_end = match content[lib_id_abs..].find('"') {
        Some(o) => lib_id_abs + o,
        None => {
            return Ok(CallToolResult::error_kind(
                ToolErrorKind::MalformedDocument {
                    path: sch_path.display().to_string(),
                    detail: format!("the (lib_id …) of '{reference}' has no closing quote"),
                },
                "Malformed lib_id",
            ))
        }
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
            return Ok(CallToolResult::error_kind(
                ToolErrorKind::InvalidArgument {
                    field: "unit".to_string(),
                    reason: format!("valid range is 1..={unit_count} for '{new_lib_id}'"),
                },
                format!(
                    "Invalid unit {} for '{}': the symbol has {} unit(s) (valid: 1..={}).",
                    unit, new_lib_id, unit_count, unit_count
                ),
            ));
        }
        // Shift the block's end by the lib_id edit rather than re-finding it by
        // designator: the units of a multi-unit symbol share one, and this call
        // named a single unit. The block starts before the lib_id, so its start
        // did not move. Then update every `(unit N)` inside it — the symbol's
        // own and the one in its (instances …) entry.
        let s = sym_start;
        let e = (sym_end + new_lib_id.len()).saturating_sub(old_lib_id.len());
        {
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

    // A reference-addressed replace repoints every sibling unit's `(lib_id
    // …)` too, not just the primary block edited above: every unit of a
    // multi-unit symbol is placed from the same library part, so leaving a
    // sibling on `old_lib_id` would point it at a definition this call just
    // replaced (P.6.8.2). `unit` is deliberately left alone on siblings — it
    // selects *which* library unit each placed block shows and is a
    // per-block fact, unlike `lib_id`. The already-updated primary block no
    // longer contains `old_lib_id`'s text, so this pass never touches it
    // twice.
    if is_reference_addressed {
        let mut sibling_edits = Vec::new();
        for (s, e) in find_all_symbol_instance_blocks(&content, &reference) {
            let block = &content[s..e];
            if let Some(rel) = block.find(lib_id_pat) {
                let abs = s + rel + lib_id_pat.len();
                if let Some(close_rel) = content[abs..].find('"') {
                    let old_here = &content[abs..abs + close_rel];
                    if old_here == old_lib_id {
                        sibling_edits.push(SexpEdit::replace(
                            abs,
                            abs + close_rel,
                            new_lib_id.clone(),
                        ));
                    }
                }
            }
        }
        content = apply_edits(content, sibling_edits);
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
    async fn pin_locations_resolve_through_lib_name_not_lib_id() {
        // eeschema stores a locally edited library symbol under a derived name
        // and points the instance at it with (lib_name …). Resolving on lib_id
        // alone picks the *base* definition, whose pins sit elsewhere — the
        // wrong answer is returned silently, and every wire placed from it
        // lands off-pin (#143).
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("derived.kicad_sch");
        std::fs::write(
            &path,
            "(kicad_sch\n\t(version 20250114)\n\t(generator \"eeschema\")\n\t(uuid \"11111111-2222-3333-4444-555555555555\")\n\t(lib_symbols\n\t\t(symbol \"Device:R\"\n\t\t\t(symbol \"R_1_1\"\n\t\t\t\t(pin passive line (at 0 3.81 270) (length 1.27) (name \"~\") (number \"1\"))\n\t\t\t)\n\t\t)\n\t\t(symbol \"R_1\"\n\t\t\t(symbol \"R_1_1_1\"\n\t\t\t\t(pin passive line (at 0 6.35 270) (length 1.27) (name \"~\") (number \"1\"))\n\t\t\t)\n\t\t)\n\t\t(symbol \"C_1\"\n\t\t\t(symbol \"C_1_1_1\"\n\t\t\t\t(pin passive line (at 0 3.81 270) (length 3.048) (name \"~\") (number \"1\"))\n\t\t\t)\n\t\t)\n\t)\n\t(symbol\n\t\t(lib_name \"R_1\")\n\t\t(lib_id \"Device:R\")\n\t\t(at 88.9 63.5 0)\n\t\t(unit 1)\n\t\t(uuid \"aaaaaaaa-bbbb-cccc-dddd-000000000001\")\n\t\t(property \"Reference\" \"R2\" (at 91.44 62.23 0))\n\t)\n\t(symbol\n\t\t(lib_name \"C_1\")\n\t\t(lib_id \"Device:C\")\n\t\t(at 139.7 63.5 0)\n\t\t(unit 1)\n\t\t(uuid \"aaaaaaaa-bbbb-cccc-dddd-000000000002\")\n\t\t(property \"Reference\" \"C1\" (at 142.24 62.23 0))\n\t)\n)\n",
        )
        .unwrap();
        let ctx = test_ctx();

        let res = handle_get_schematic_pin_locations(
            &json!({ "schematic": path.display().to_string(), "reference": "R2" }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(!res.is_error, "{}", content_text(&res));
        let out: serde_json::Value = serde_json::from_str(&content_text(&res)).unwrap();
        // R_1's pin sits at local +6.35 => 63.5 - 6.35; Device:R's would be
        // 63.5 - 3.81 = 59.69.
        assert_eq!(out["pins"][0]["y"].as_f64().unwrap(), 57.15);

        // Device:C is not embedded at all — only the derived C_1 is. Matching
        // on lib_id reported "no embedded definition ... nonexistent lib_id",
        // which is both wrong and dangerous advice.
        let batch = handle_batch_get_pin_locations(
            &json!({
                "schematic": path.display().to_string(),
                "references": ["C1"]
            }),
            &ctx,
        )
        .await
        .unwrap();
        let out: serde_json::Value = serde_json::from_str(&content_text(&batch)).unwrap();
        assert!(
            out["components"][0]["error"].is_null(),
            "C1 must resolve through C_1: {out}"
        );
        assert_eq!(
            out["components"][0]["pins"][0]["y"].as_f64().unwrap(),
            59.69
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

    /// A sheet carrying one library symbol whose fields sit somewhere other
    /// than the old hard-coded offsets: `Reference` right and above the
    /// origin, `Value` right and below, both left-justified, and the whole
    /// thing in the library's Y-up space.
    fn sheet_with_fielded_symbol(dir: &std::path::Path) -> std::path::PathBuf {
        let path = dir.join("fielded.kicad_sch");
        std::fs::write(
            &path,
            concat!(
                "(kicad_sch\n  (version 20250610)\n  (generator \"konnect\")\n",
                "  (uuid \"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee\")\n  (paper \"A4\")\n",
                "  (lib_symbols\n    (symbol \"Test:FIELDED\"\n",
                "      (property \"Reference\" \"U\" (at 2.54 3.81 0)",
                " (effects (font (size 1.27 1.27)) (justify left)))\n",
                "      (property \"Value\" \"FIELDED\" (at 2.54 -3.81 0)",
                " (effects (font (size 1.27 1.27)) (justify left)))\n",
                "    )\n  )\n)\n"
            ),
        )
        .unwrap();
        path
    }

    async fn place_fielded(path: &std::path::Path, rotation: f64) -> cse::Symbol {
        let result = handle_add_schematic_component(
            &json!({
                "schematic": path.display().to_string(),
                "lib_id": "Test:FIELDED",
                "x": 101.6,
                "y": 88.9,
                "rotation": rotation,
                "reference": "U1",
                "value": "FIELDED"
            }),
            &test_ctx(),
        )
        .await
        .unwrap();
        assert!(!result.is_error, "{result:?}");
        cse::Schematic::load(path)
            .unwrap()
            .symbols
            .iter()
            .find(|s| s.reference() == Some("U1"))
            .expect("the placed symbol")
            .clone()
    }

    fn field_at(sym: &cse::Symbol, name: &str) -> String {
        let prop = sym
            .properties
            .iter()
            .find(|p| p.name == name)
            .unwrap_or_else(|| panic!("no {name} property"));
        cse::sexp::writer::write(&prop.to_sexp())
    }

    /// P.6.8.8: the fields go where the library symbol says, not to a fixed
    /// ±3.81 under the placement. At rotation 0 that is the anchor flipped
    /// from Y-up to Y-down and translated — including its x, which the old
    /// hard-coded rule dropped entirely.
    #[tokio::test]
    async fn placed_fields_take_the_library_anchor() {
        let dir = tempfile::tempdir().unwrap();
        let path = sheet_with_fielded_symbol(dir.path());
        let sym = place_fielded(&path, 0.0).await;

        // (2.54, 3.81) Y-up → (101.6 + 2.54, 88.9 - 3.81).
        assert!(
            field_at(&sym, "Reference").contains("(at 104.14 85.09 0)"),
            "{}",
            field_at(&sym, "Reference")
        );
        assert!(
            field_at(&sym, "Value").contains("(at 104.14 92.71 0)"),
            "{}",
            field_at(&sym, "Value")
        );
    }

    /// The rotated case, which is the one the old rule could never get right:
    /// it wrote the same two points whatever the placement rotation. The
    /// anchor goes through the pin transform, so (2.54, 3.81) at 90° lands at
    /// (x - 3.81, y - 2.54).
    #[tokio::test]
    async fn placed_fields_rotate_with_the_symbol() {
        let dir = tempfile::tempdir().unwrap();
        let path = sheet_with_fielded_symbol(dir.path());
        let sym = place_fielded(&path, 90.0).await;

        assert!(
            field_at(&sym, "Reference").contains("(at 97.79 86.36"),
            "{}",
            field_at(&sym, "Reference")
        );
        assert!(
            field_at(&sym, "Value").contains("(at 105.41 86.36"),
            "{}",
            field_at(&sym, "Value")
        );
    }

    /// The text angle is the library's, not the placement's — measured on the
    /// demo corpus — and the library's justification comes across with it: a
    /// `left`-justified reference written without its justify shifts by half
    /// its own width.
    #[tokio::test]
    async fn a_placed_field_keeps_the_library_text_angle_and_justification() {
        let dir = tempfile::tempdir().unwrap();
        let path = sheet_with_fielded_symbol(dir.path());
        let sym = place_fielded(&path, 90.0).await;

        let reference = field_at(&sym, "Reference");
        assert!(
            reference.contains("(at 97.79 86.36 0)"),
            "the field angle is the library's 0, not the placement's 90: {reference}"
        );
        assert!(
            reference.contains("(justify left)"),
            "the library justification must survive the placement: {reference}"
        );
    }

    /// A library entry that declares no `Reference`/`Value` of its own keeps
    /// the historical offsets: this is the fallback, and it must stay, because
    /// a symbol with no anchor to read is exactly where the old rule was the
    /// only rule available.
    #[tokio::test]
    async fn a_symbol_without_field_anchors_keeps_the_old_offsets() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("anchorless.kicad_sch");
        std::fs::write(
            &path,
            concat!(
                "(kicad_sch\n  (version 20250610)\n  (generator \"konnect\")\n",
                "  (uuid \"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee\")\n  (paper \"A4\")\n",
                "  (lib_symbols\n    (symbol \"Test:BARE\"\n    )\n  )\n)\n"
            ),
        )
        .unwrap();

        let result = handle_add_schematic_component(
            &json!({
                "schematic": path.display().to_string(),
                "lib_id": "Test:BARE",
                "x": 101.6,
                "y": 88.9,
                "reference": "U1",
                "value": "BARE"
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
            .find(|s| s.reference() == Some("U1"))
            .expect("the placed symbol");
        assert!(
            field_at(sym, "Reference").contains("(at 101.6 85.09 0)"),
            "{}",
            field_at(sym, "Reference")
        );
        assert!(
            field_at(sym, "Value").contains("(at 101.6 92.71 0)"),
            "{}",
            field_at(sym, "Value")
        );
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
    // ─── D.4.1.2: `uuid` is accepted wherever `reference` is ─────────────────

    /// Two byte-identical schematics, one per directory so the file stem — and
    /// with it the project name written into every instance path — stays the
    /// same. Each holds R1 and R2; the returned uuid is R1's.
    fn ok_body(result: &CallToolResult) -> serde_json::Value {
        let crate::mcp::protocol::ToolContent::Text { text } = result.content.first().unwrap()
        else {
            panic!("text content expected");
        };
        serde_json::from_str::<serde_json::Value>(text).unwrap()
    }

    /// The `fields` argument was declared in the schema and never read, so a
    /// call carrying only custom properties reported `"changes": []` as a
    /// success and wrote nothing (P.6.9.6).
    #[tokio::test]
    async fn a_fields_only_edit_writes_the_property_the_symbol_lacks() {
        let (_symdir, _env) = stub_symbol_dir().await;
        let ctx = test_ctx();
        let (_dir, path, _b, _uuid) = twin_schematics(&ctx).await;

        let edited = handle_edit_schematic_component(
            &json!({
                "schematic": path.display().to_string(),
                "reference": "R1",
                "fields": { "MPN": "RC0805FR-074K7L" }
            }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(!edited.is_error, "{:?}", edited.content);
        let body = ok_body(&edited);
        assert_eq!(
            body["changes"],
            json!(["MPN → RC0805FR-074K7L (added)"]),
            "the fields edit must be reported as a change: {body}"
        );
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("(property \"MPN\" \"RC0805FR-074K7L\""),
            "the property was not written:\n{text}"
        );
    }

    #[tokio::test]
    async fn a_fields_key_the_symbol_already_has_is_updated_in_place() {
        let (_symdir, _env) = stub_symbol_dir().await;
        let ctx = test_ctx();
        let (_dir, path, _b, _uuid) = twin_schematics(&ctx).await;

        for value in ["RC0805", "RC0603"] {
            let edited = handle_edit_schematic_component(
                &json!({
                    "schematic": path.display().to_string(),
                    "reference": "R1",
                    "fields": { "MPN": value }
                }),
                &ctx,
            )
            .await
            .unwrap();
            assert!(!edited.is_error, "{:?}", edited.content);
        }

        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            text.matches("(property \"MPN\"").count(),
            1,
            "the second write duplicated the property instead of updating it:\n{text}"
        );
        assert!(
            text.contains("\"RC0603\"") && !text.contains("\"RC0805\""),
            "the property still carries the first value:\n{text}"
        );
    }

    /// `Reference` is stored twice — in the property and in `(instances …)` —
    /// so the generic `fields` path, which rewrites only the property, must
    /// refuse it and leave `new_reference` as the one way to rename.
    #[tokio::test]
    async fn reference_in_fields_is_refused_and_writes_nothing() {
        let (_symdir, _env) = stub_symbol_dir().await;
        let ctx = test_ctx();
        let (_dir, path, _b, _uuid) = twin_schematics(&ctx).await;
        let before = std::fs::read_to_string(&path).unwrap();

        let edited = handle_edit_schematic_component(
            &json!({
                "schematic": path.display().to_string(),
                "reference": "R1",
                "fields": { "Reference": "R9" }
            }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(edited.is_error, "{:?}", edited.content);
        assert_eq!(error_body(&edited)["kind"], json!("invalid_argument"));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            before,
            "a reserved key must not reach the file"
        );
    }

    #[tokio::test]
    async fn an_edit_that_changes_nothing_is_a_failure() {
        let (_symdir, _env) = stub_symbol_dir().await;
        let ctx = test_ctx();
        let (_dir, path, _b, _uuid) = twin_schematics(&ctx).await;

        for args in [
            json!({ "schematic": path.display().to_string(), "reference": "R1", "fields": {} }),
            json!({ "schematic": path.display().to_string(), "reference": "R1" }),
        ] {
            let edited = handle_edit_schematic_component(&args, &ctx).await.unwrap();
            assert!(
                edited.is_error,
                "an empty edit reported success: {:?}",
                edited.content
            );
            assert_eq!(error_body(&edited)["kind"], json!("invalid_argument"));
        }
    }

    #[tokio::test]
    async fn a_non_object_fields_argument_is_an_invalid_argument() {
        let (_symdir, _env) = stub_symbol_dir().await;
        let ctx = test_ctx();
        let (_dir, path, _b, _uuid) = twin_schematics(&ctx).await;

        let edited = handle_edit_schematic_component(
            &json!({
                "schematic": path.display().to_string(),
                "reference": "R1",
                "fields": "MPN=RC0805"
            }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(edited.is_error, "{:?}", edited.content);
        let body = error_body(&edited);
        assert_eq!(body["kind"], json!("invalid_argument"));
        assert_eq!(body["field"], json!("fields"));
    }

    /// KiCAD stores every property as text, so a JSON number or boolean is
    /// written as its text form; a value with no text form is reported.
    #[tokio::test]
    async fn numeric_and_boolean_field_values_are_stored_as_text() {
        let (_symdir, _env) = stub_symbol_dir().await;
        let ctx = test_ctx();
        let (_dir, path, _b, _uuid) = twin_schematics(&ctx).await;

        let edited = handle_edit_schematic_component(
            &json!({
                "schematic": path.display().to_string(),
                "reference": "R1",
                "fields": { "Tolerance": 1, "DNP": true, "Bad": null }
            }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(!edited.is_error, "{:?}", edited.content);
        let body = ok_body(&edited);
        assert_eq!(
            body["errors"],
            json!(["Bad: value must be a string, number or boolean"]),
            "an unrepresentable value must be reported: {body}"
        );
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("(property \"Tolerance\" \"1\"")
                && text.contains("(property \"DNP\" \"true\""),
            "scalar values were not stored as text:\n{text}"
        );
    }

    async fn twin_schematics(
        ctx: &ToolContext,
    ) -> (
        tempfile::TempDir,
        std::path::PathBuf,
        std::path::PathBuf,
        String,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let build = dir.path().join("build");
        std::fs::create_dir_all(&build).unwrap();
        let source = build.join("twin.kicad_sch");
        handle_create_schematic(&json!({ "path": source.display().to_string() }), ctx)
            .await
            .unwrap();
        for (reference, x) in [("R1", 100.0), ("R2", 120.0)] {
            let res = handle_add_schematic_component(
                &json!({
                    "schematic": source.display().to_string(),
                    "lib_id": "Device:R",
                    "x": x, "y": 80.0,
                    "reference": reference
                }),
                ctx,
            )
            .await
            .unwrap();
            assert!(!res.is_error, "placing {reference}: {:?}", res.content);
        }

        let (a_dir, b_dir) = (dir.path().join("a"), dir.path().join("b"));
        std::fs::create_dir_all(&a_dir).unwrap();
        std::fs::create_dir_all(&b_dir).unwrap();
        let (a, b) = (a_dir.join("twin.kicad_sch"), b_dir.join("twin.kicad_sch"));
        std::fs::copy(&source, &a).unwrap();
        std::fs::copy(&source, &b).unwrap();

        let uuid = symbol_uuid(&a, "R1");
        (dir, a, b, uuid)
    }

    fn symbol_uuid(path: &std::path::Path, reference: &str) -> String {
        cse::Schematic::load(path)
            .unwrap()
            .symbols
            .by_reference(reference)
            .unwrap_or_else(|| panic!("{reference} present"))
            .uuid
            .clone()
    }

    /// The structured `error` object of a failed result.
    fn error_body(result: &CallToolResult) -> serde_json::Value {
        let crate::mcp::protocol::ToolContent::Text { text } = result.content.first().unwrap()
        else {
            panic!("text content expected");
        };
        serde_json::from_str::<serde_json::Value>(text).unwrap()["error"].clone()
    }

    #[tokio::test]
    async fn get_component_by_uuid_matches_by_reference() {
        let (_symdir, _env) = stub_symbol_dir().await;
        let ctx = test_ctx();
        let (_dir, a, b, uuid) = twin_schematics(&ctx).await;

        let by_ref = handle_get_schematic_component(
            &json!({ "schematic": a.display().to_string(), "reference": "R1" }),
            &ctx,
        )
        .await
        .unwrap();
        let by_uuid = handle_get_schematic_component(
            &json!({ "schematic": b.display().to_string(), "uuid": uuid }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(!by_ref.is_error && !by_uuid.is_error);
        assert_eq!(
            format!("{:?}", by_ref.content),
            format!("{:?}", by_uuid.content)
        );
    }

    #[tokio::test]
    async fn move_component_by_uuid_matches_by_reference() {
        let (_symdir, _env) = stub_symbol_dir().await;
        let ctx = test_ctx();
        let (_dir, a, b, uuid) = twin_schematics(&ctx).await;

        handle_move_schematic_component(
            &json!({ "schematic": a.display().to_string(), "reference": "R1", "x": 60.96, "y": 55.88 }),
            &ctx,
        )
        .await
        .unwrap();
        handle_move_schematic_component(
            &json!({ "schematic": b.display().to_string(), "uuid": uuid, "x": 60.96, "y": 55.88 }),
            &ctx,
        )
        .await
        .unwrap();

        assert_eq!(
            std::fs::read_to_string(&a).unwrap(),
            std::fs::read_to_string(&b).unwrap(),
            "a uuid-addressed move must write the same document as a reference-addressed one"
        );
    }

    #[tokio::test]
    async fn edit_component_by_uuid_matches_by_reference() {
        let (_symdir, _env) = stub_symbol_dir().await;
        let ctx = test_ctx();
        let (_dir, a, b, uuid) = twin_schematics(&ctx).await;

        let fields = json!({ "value": "4k7", "footprint": "Resistor_SMD:R_0805_2012Metric" });
        let mut by_ref = fields.clone();
        by_ref["schematic"] = json!(a.display().to_string());
        by_ref["reference"] = json!("R1");
        let mut by_uuid = fields;
        by_uuid["schematic"] = json!(b.display().to_string());
        by_uuid["uuid"] = json!(uuid);

        let ra = handle_edit_schematic_component(&by_ref, &ctx)
            .await
            .unwrap();
        let rb = handle_edit_schematic_component(&by_uuid, &ctx)
            .await
            .unwrap();
        assert!(
            !ra.is_error && !rb.is_error,
            "{:?} {:?}",
            ra.content,
            rb.content
        );
        assert_eq!(
            std::fs::read_to_string(&a).unwrap(),
            std::fs::read_to_string(&b).unwrap()
        );
    }

    #[tokio::test]
    async fn add_annotation_by_uuid_matches_by_reference() {
        let (_symdir, _env) = stub_symbol_dir().await;
        let ctx = test_ctx();
        let (_dir, a, b, uuid) = twin_schematics(&ctx).await;

        let ra = handle_add_component_annotation(
            &json!({ "schematic": a.display().to_string(), "reference": "R1", "key": "MPN", "value": "RC0805" }),
            &ctx,
        )
        .await
        .unwrap();
        let rb = handle_add_component_annotation(
            &json!({ "schematic": b.display().to_string(), "uuid": uuid, "key": "MPN", "value": "RC0805" }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(
            !ra.is_error && !rb.is_error,
            "{:?} {:?}",
            ra.content,
            rb.content
        );
        assert_eq!(
            std::fs::read_to_string(&a).unwrap(),
            std::fs::read_to_string(&b).unwrap()
        );
    }

    #[tokio::test]
    async fn replace_component_by_uuid_matches_by_reference() {
        let (_symdir, _env) = stub_symbol_dir().await;
        let ctx = test_ctx();
        let (_dir, a, b, uuid) = twin_schematics(&ctx).await;

        let ra = handle_replace_component(
            &json!({ "schematic": a.display().to_string(), "reference": "R1", "new_lib_id": "Device:C_Polarized" }),
            &ctx,
        )
        .await
        .unwrap();
        let rb = handle_replace_component(
            &json!({ "schematic": b.display().to_string(), "uuid": uuid, "new_lib_id": "Device:C_Polarized" }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(
            !ra.is_error && !rb.is_error,
            "{:?} {:?}",
            ra.content,
            rb.content
        );
        assert_eq!(
            std::fs::read_to_string(&a).unwrap(),
            std::fs::read_to_string(&b).unwrap()
        );
    }

    /// The point of D.4: a uuid is an address that survives what the document
    /// does to the thing it names. Position changes, designator changes; the
    /// uuid keeps resolving to the same symbol.
    #[tokio::test]
    async fn a_uuid_still_addresses_the_symbol_after_it_moved() {
        let (_symdir, _env) = stub_symbol_dir().await;
        let ctx = test_ctx();
        let (_dir, path, _b, uuid) = twin_schematics(&ctx).await;

        let moved = handle_move_schematic_component(
            &json!({ "schematic": path.display().to_string(), "uuid": uuid, "x": 60.96, "y": 55.88 }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(!moved.is_error);

        let again = handle_get_schematic_component(
            &json!({ "schematic": path.display().to_string(), "uuid": uuid }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(!again.is_error, "the address must outlive the move");
        let crate::mcp::protocol::ToolContent::Text { text } = again.content.first().unwrap()
        else {
            panic!("text content expected");
        };
        let body: serde_json::Value = serde_json::from_str(text).unwrap();
        assert_eq!(body["uuid"], json!(uuid));
        assert_eq!(body["x"], json!(60.96));
        assert_eq!(body["y"], json!(55.88));
    }

    #[tokio::test]
    async fn an_unknown_uuid_is_not_found_and_lists_the_symbols() {
        let (_symdir, _env) = stub_symbol_dir().await;
        let ctx = test_ctx();
        let (_dir, path, _b, r1) = twin_schematics(&ctx).await;
        let r2 = symbol_uuid(&path, "R2");

        let result = handle_get_schematic_component(
            &json!({
                "schematic": path.display().to_string(),
                "uuid": "00000000-0000-4000-8000-000000000000"
            }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(result.is_error);
        let error = error_body(&result);
        assert_eq!(error["kind"], json!("not_found"));
        assert_eq!(error["item_kind"], json!("component"));
        let candidates: Vec<String> = serde_json::from_value(error["candidates"].clone()).unwrap();
        assert!(
            candidates.contains(&r1) && candidates.contains(&r2),
            "candidates must be the symbols that are there: {candidates:?}"
        );
    }

    /// A real uuid of the wrong kind of item is `NotFound`, not an edit landing
    /// on the wire that happened to carry it.
    #[tokio::test]
    async fn a_wires_uuid_does_not_address_a_component() {
        let (_symdir, _env) = stub_symbol_dir().await;
        let ctx = test_ctx();
        let (_dir, path, _b, _uuid) = twin_schematics(&ctx).await;

        let wire_uuid = {
            let mut sch = cse::Schematic::load(&path).unwrap();
            let wire = cse::Wire::new(50.8, 50.8, 63.5, 50.8);
            let uuid = wire.uuid.clone();
            sch.wires.push(wire);
            sch.overwrite().unwrap();
            uuid
        };
        let before = std::fs::read_to_string(&path).unwrap();

        let result = handle_edit_schematic_component(
            &json!({ "schematic": path.display().to_string(), "uuid": wire_uuid, "value": "4k7" }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(result.is_error);
        assert_eq!(error_body(&result)["kind"], json!("not_found"));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            before,
            "a wrong-kind address must not edit anything"
        );
    }

    #[tokio::test]
    async fn neither_reference_nor_uuid_is_an_invalid_argument() {
        let (_symdir, _env) = stub_symbol_dir().await;
        let ctx = test_ctx();
        let (_dir, path, _b, _uuid) = twin_schematics(&ctx).await;

        for result in [
            handle_get_schematic_component(
                &json!({ "schematic": path.display().to_string() }),
                &ctx,
            )
            .await
            .unwrap(),
            handle_add_component_annotation(
                &json!({ "schematic": path.display().to_string(), "key": "MPN", "value": "X" }),
                &ctx,
            )
            .await
            .unwrap(),
        ] {
            assert!(result.is_error);
            let error = error_body(&result);
            assert_eq!(error["kind"], json!("invalid_argument"));
            assert_eq!(error["field"], json!("reference"));
        }
    }
    /// A multi-unit symbol as KiCad writes one: two top-level `(symbol …)`
    /// blocks sharing the designator `U1`, each with its own `(uuid …)` and
    /// `(unit N)`, at different positions. Built through
    /// `add_schematic_component` so the `lib_symbols` definition is embedded
    /// exactly the way every other fixture here gets it.
    ///
    /// Returns the schematic path and the two uuids, unit 1 first.
    async fn multi_unit_schematic(
        dir: &tempfile::TempDir,
        ctx: &ToolContext,
    ) -> (std::path::PathBuf, String, String) {
        let path = dir.path().join("dual.kicad_sch");
        handle_create_schematic(&json!({ "path": path.display().to_string() }), ctx)
            .await
            .unwrap();
        let mut uuids = Vec::new();
        for (unit, x) in [(1u32, 100.0), (2, 150.0)] {
            let res = handle_add_schematic_component(
                &json!({
                    "schematic": path.display().to_string(),
                    "lib_id": "Device:OPAMP_DUAL",
                    "x": x, "y": 80.0,
                    "reference": "U1",
                    "unit": unit
                }),
                ctx,
            )
            .await
            .unwrap();
            assert!(!res.is_error, "placing unit {unit}: {:?}", res.content);
            let out: serde_json::Value = serde_json::from_str(&content_text(&res)).unwrap();
            uuids.push(out["uuid"].as_str().unwrap().to_string());
        }
        (path, uuids.remove(0), uuids.remove(0))
    }

    /// Position and footprint of the symbol at `uuid`, straight from
    /// `get_schematic_component` — which is itself one of the handlers under
    /// test, so a lookup that landed on the wrong unit could not hide here.
    async fn unit_state(
        path: &std::path::Path,
        uuid: &str,
        ctx: &ToolContext,
    ) -> (f64, f64, String) {
        let res = handle_get_schematic_component(
            &json!({ "schematic": path.display().to_string(), "uuid": uuid }),
            ctx,
        )
        .await
        .unwrap();
        assert!(!res.is_error, "get {uuid}: {:?}", res.content);
        let out: serde_json::Value = serde_json::from_str(&content_text(&res)).unwrap();
        assert_eq!(
            out["uuid"].as_str(),
            Some(uuid),
            "get returned another unit"
        );
        (
            out["x"].as_f64().unwrap(),
            out["y"].as_f64().unwrap(),
            out["footprint"].as_str().unwrap_or("").to_string(),
        )
    }

    #[tokio::test]
    async fn move_by_uuid_moves_the_named_unit() {
        // D.4.1.7: the units of one symbol share a designator, so a uuid
        // naming unit 2 used to move unit 1 in silence.
        let (_symdir, _env) = stub_symbol_dir().await;
        let dir = tempfile::tempdir().unwrap();
        let ctx = test_ctx();
        let (path, u1, u2) = multi_unit_schematic(&dir, &ctx).await;

        let before_1 = unit_state(&path, &u1, &ctx).await;
        let moved = handle_move_schematic_component(
            &json!({ "schematic": path.display().to_string(), "uuid": u2, "x": 200.0, "y": 120.0 }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(!moved.is_error, "{:?}", moved.content);

        let after_2 = unit_state(&path, &u2, &ctx).await;
        assert!(
            (after_2.0 - 200.0).abs() < 1.27 && (after_2.1 - 120.0).abs() < 1.27,
            "unit 2 must be at the requested point, is {after_2:?}"
        );
        assert_eq!(
            unit_state(&path, &u1, &ctx).await,
            before_1,
            "unit 1 must not have moved"
        );

        // And the other way round: unit 1 by its own uuid.
        let before_2 = unit_state(&path, &u2, &ctx).await;
        let moved = handle_move_schematic_component(
            &json!({ "schematic": path.display().to_string(), "uuid": u1, "x": 60.0, "y": 40.0 }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(!moved.is_error, "{:?}", moved.content);
        let after_1 = unit_state(&path, &u1, &ctx).await;
        assert!(
            (after_1.0 - 60.0).abs() < 1.27 && (after_1.1 - 40.0).abs() < 1.27,
            "unit 1 must be at the requested point, is {after_1:?}"
        );
        assert_eq!(
            unit_state(&path, &u2, &ctx).await,
            before_2,
            "unit 2 must not have moved"
        );
    }

    #[tokio::test]
    async fn move_by_reference_on_a_multiunit_symbol_is_refused_not_silently_the_first_unit() {
        // Superseded by P.6.8.2: this test used to assert INV8's "the
        // designator means the first symbol carrying it" for a *geometry*
        // call, moving unit 1 while silently leaving unit 2 exactly where it
        // was. That is not a safe reading of a multi-unit `reference` — the
        // caller never said "unit 1", and eeschema itself only ever moves
        // one unit at a time by picking it directly, never by designator.
        // INV8's own first clause is what decides it: an input with two
        // meanings is genuine ambiguity and stays refused. Its second clause
        // ("a widened acceptance must never turn a previously compiling input
        // into a failure") governs widenings, and this is the opposite — an
        // acceptance that should never have been granted. A
        // reference-addressed geometry call on a multi-unit symbol is now
        // refused, naming the units so the caller can retry with `uuid`; the
        // uuid-addressed path (`move_by_uuid_moves_the_named_unit` above and
        // `rotate_and_edit_by_uuid_reach_the_named_unit` below) is
        // unaffected and still moves exactly the named unit.
        let (_symdir, _env) = stub_symbol_dir().await;
        let dir = tempfile::tempdir().unwrap();
        let ctx = test_ctx();
        let (path, u1, u2) = multi_unit_schematic(&dir, &ctx).await;

        let before_1 = unit_state(&path, &u1, &ctx).await;
        let before_2 = unit_state(&path, &u2, &ctx).await;
        let moved = handle_move_schematic_component(
            &json!({ "schematic": path.display().to_string(), "reference": "U1", "x": 200.0, "y": 120.0 }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(
            moved.is_error,
            "a multi-unit reference move must be refused"
        );

        assert_eq!(
            unit_state(&path, &u1, &ctx).await,
            before_1,
            "a refused move must not touch unit 1"
        );
        assert_eq!(
            unit_state(&path, &u2, &ctx).await,
            before_2,
            "a refused move must not touch unit 2"
        );
    }

    #[tokio::test]
    async fn rotate_and_edit_by_uuid_reach_the_named_unit() {
        let (_symdir, _env) = stub_symbol_dir().await;
        let dir = tempfile::tempdir().unwrap();
        let ctx = test_ctx();
        let (path, u1, u2) = multi_unit_schematic(&dir, &ctx).await;

        let edited = handle_edit_schematic_component(
            &json!({
                "schematic": path.display().to_string(),
                "uuid": u2,
                "footprint": "Package_SO:SOIC-8_3.9x4.9mm_P1.27mm"
            }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(!edited.is_error, "{:?}", edited.content);
        assert_eq!(
            unit_state(&path, &u2, &ctx).await.2,
            "Package_SO:SOIC-8_3.9x4.9mm_P1.27mm",
            "the edited unit is the one the uuid named"
        );
        assert_eq!(
            unit_state(&path, &u1, &ctx).await.2,
            "",
            "unit 1 must be untouched"
        );

        let rotated = handle_rotate_schematic_component(
            &json!({ "schematic": path.display().to_string(), "uuid": u2, "rotation": 90.0 }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(!rotated.is_error, "{:?}", rotated.content);

        let angle = |uuid: &str| {
            let path = path.clone();
            let uuid = uuid.to_string();
            let ctx = test_ctx();
            async move {
                let res = handle_get_schematic_component(
                    &json!({ "schematic": path.display().to_string(), "uuid": uuid }),
                    &ctx,
                )
                .await
                .unwrap();
                let out: serde_json::Value = serde_json::from_str(&content_text(&res)).unwrap();
                out["rotation"].as_f64().unwrap()
            }
        };
        assert_eq!(angle(&u2).await, 90.0, "unit 2 rotated");
        assert_eq!(angle(&u1).await, 0.0, "unit 1 must not have rotated");
    }

    #[tokio::test]
    async fn delete_and_pin_locations_by_uuid_reach_the_named_unit() {
        let (_symdir, _env) = stub_symbol_dir().await;
        let dir = tempfile::tempdir().unwrap();
        let ctx = test_ctx();
        let (path, u1, u2) = multi_unit_schematic(&dir, &ctx).await;

        // Unit 2's pins, not unit 1's superimposed or substituted.
        let pins = handle_get_schematic_pin_locations(
            &json!({ "schematic": path.display().to_string(), "uuid": u2 }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(!pins.is_error, "{:?}", pins.content);
        let out: serde_json::Value = serde_json::from_str(&content_text(&pins)).unwrap();
        let mut nums: Vec<String> = out["pins"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["number"].as_str().unwrap().to_string())
            .collect();
        nums.sort();
        assert_eq!(nums, vec!["5", "6", "7"], "unit 2 pins");

        let deleted = handle_delete_schematic_component(
            &json!({ "schematic": path.display().to_string(), "uuid": u2 }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(!deleted.is_error, "{:?}", deleted.content);

        let listed = handle_list_schematic_components(
            &json!({ "schematic": path.display().to_string() }),
            &ctx,
        )
        .await
        .unwrap();
        let out: serde_json::Value = serde_json::from_str(&content_text(&listed)).unwrap();
        let remaining: Vec<&str> = out["components"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["uuid"].as_str().unwrap())
            .collect();
        assert_eq!(remaining, vec![u1.as_str()], "unit 2 is the one deleted");
    }

    // ─── P.6.9.5: add_component_annotation dedup / reserved keys ───────────

    /// Two calls with the same key must leave exactly one property of that
    /// name, not two — the bug being that `handle_add_component_annotation`
    /// used to insert unconditionally, with no lookup for an existing one.
    #[tokio::test]
    async fn add_annotation_same_key_twice_yields_one_property() {
        let (_symdir, _env) = stub_symbol_dir().await;
        let ctx = test_ctx();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dup.kicad_sch");
        handle_create_schematic(&json!({ "path": path.display().to_string() }), &ctx)
            .await
            .unwrap();
        handle_add_schematic_component(
            &json!({ "schematic": path.display().to_string(), "lib_id": "Device:R", "x": 100.0, "y": 80.0, "reference": "R1" }),
            &ctx,
        )
        .await
        .unwrap();

        for (value, expect_created) in [("RC0805-first", true), ("RC0805-second", false)] {
            let r = handle_add_component_annotation(
                &json!({ "schematic": path.display().to_string(), "reference": "R1", "key": "MPN", "value": value }),
                &ctx,
            )
            .await
            .unwrap();
            assert!(!r.is_error, "{:?}", r.content);
            // The answer must say which of the two happened: the second call
            // updated a property that already existed, and reporting that as
            // a creation is how a caller ends up believing it has two.
            let text = format!("{:?}", r.content);
            assert!(
                text.contains(&format!(r#"created\":{expect_created}"#)),
                "expected created={expect_created} for {value}: {text}"
            );
        }

        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            content.matches("(property \"MPN\" ").count(),
            1,
            "two calls with the same key must leave one property, not two:\n{content}"
        );
        assert!(
            content.contains("(property \"MPN\" \"RC0805-second\""),
            "the second call's value must win"
        );
    }

    /// `Reference` has a dedicated, invariant-preserving path
    /// (`edit_schematic_component`'s `new_reference`, which also repoints
    /// `(instances …)`); `add_component_annotation` must refuse it rather
    /// than write around that path. `Value`/`Footprint`/`Datasheet` carry no
    /// such invariant and are not reserved — setting them through this tool
    /// is exactly what `design_review.rs`'s BOM audit test already relies
    /// on, unchanged by this fix.
    #[tokio::test]
    async fn add_annotation_refuses_reserved_keys() {
        let (_symdir, _env) = stub_symbol_dir().await;
        let ctx = test_ctx();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("reserved.kicad_sch");
        handle_create_schematic(&json!({ "path": path.display().to_string() }), &ctx)
            .await
            .unwrap();
        handle_add_schematic_component(
            &json!({ "schematic": path.display().to_string(), "lib_id": "Device:R", "x": 100.0, "y": 80.0, "reference": "R1" }),
            &ctx,
        )
        .await
        .unwrap();
        let before = std::fs::read_to_string(&path).unwrap();

        for key in ["Reference"] {
            let r = handle_add_component_annotation(
                &json!({ "schematic": path.display().to_string(), "reference": "R1", "key": key, "value": "X" }),
                &ctx,
            )
            .await
            .unwrap();
            assert!(r.is_error, "'{key}' must be refused, not silently accepted");
            assert_eq!(
                error_body(&r)["kind"],
                json!("invalid_argument"),
                "key={key}"
            );
            assert_eq!(error_body(&r)["field"], json!("key"), "key={key}");
        }
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            before,
            "a refused key must not touch the file"
        );

        // Value/Footprint/Datasheet are not reserved: they must still work.
        let r = handle_add_component_annotation(
            &json!({ "schematic": path.display().to_string(), "reference": "R1", "key": "Footprint", "value": "Resistor_SMD:R_0603_1608Metric" }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(
            !r.is_error,
            "Footprint must not be reserved: {:?}",
            r.content
        );
    }

    /// A property inserted into an eeschema-style, tab-indented schematic
    /// must itself be tab-indented, not the crate's own hard-coded
    /// two/six-space literal.
    #[tokio::test]
    async fn add_annotation_matches_tab_indentation_of_the_file() {
        let (_symdir, _env) = stub_symbol_dir().await;
        let ctx = test_ctx();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tabs.kicad_sch");
        handle_create_schematic(&json!({ "path": path.display().to_string() }), &ctx)
            .await
            .unwrap();
        handle_add_schematic_component(
            &json!({ "schematic": path.display().to_string(), "lib_id": "Device:R", "x": 100.0, "y": 80.0, "reference": "R1" }),
            &ctx,
        )
        .await
        .unwrap();

        // Re-tab the file this crate wrote (two-space indent) into the shape
        // eeschema itself writes (tabs, one per nesting level, P.6.9.4),
        // without changing its structure.
        let original = std::fs::read_to_string(&path).unwrap();
        let retabbed: String = original
            .lines()
            .map(|line| {
                let spaces = line.len() - line.trim_start_matches(' ').len();
                format!(
                    "{}{}\n",
                    "\t".repeat(spaces / 2),
                    line.trim_start_matches(' ')
                )
            })
            .collect();
        std::fs::write(&path, &retabbed).unwrap();

        let r = handle_add_component_annotation(
            &json!({ "schematic": path.display().to_string(), "reference": "R1", "key": "MPN", "value": "RC0805" }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(!r.is_error, "{:?}", r.content);

        let content = std::fs::read_to_string(&path).unwrap();
        let idx = content
            .find("(property \"MPN\" \"RC0805\"")
            .expect("inserted property");
        let line_start = content[..idx].rfind('\n').map(|p| p + 1).unwrap_or(0);
        assert!(
            !content[line_start..idx].is_empty()
                && content[line_start..idx].chars().all(|c| c == '\t'),
            "inserted property must be tab-indented to match the file: {:?}",
            &content[line_start..idx]
        );
    }
}
