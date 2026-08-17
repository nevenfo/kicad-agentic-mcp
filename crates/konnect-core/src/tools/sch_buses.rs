//! `sch_buses` toolset — bus segments, bus entries, bus aliases, and reading a
//! bus name back as the nets it stands for (J.2.2.2).
//!
//! Before this, `MISSING` recorded the whole domain as a gap: the schematic
//! engine handled wires and labels only, `bus` nodes survived a round-trip
//! untouched, and a bus-based design could not be authored here at all.
//!
//! What a caller has to know, because KiCAD does not forgive it:
//!
//! * **A bus is not a thick wire.** A wire joins a bus only through a
//!   `bus_entry`; drawing a wire onto a bus does nothing. `add_bus_entry`
//!   exists for exactly that join.
//! * **The bus's name comes from a label on it**, not from the segment. The
//!   segment carries geometry; `add_schematic_net_label` names it, and the
//!   name's syntax is what decides its members.
//! * **Members are derived, never stored.** `expand_bus` reads the same
//!   syntax KiCAD's connectivity does, against the aliases the sheet declares.
//!
//! File-editing only: KiCAD 10 registers no schematic-editing IPC commands
//! (D3), so there is no live-GUI path to keep in step.

use crate::mcp::protocol::CallToolResult;
use crate::tool;
use crate::tools::{get_path, opt_f64, require_f64, require_str, ToolContext, ToolDef};
use konnect_schematic_editor as cse;
use konnect_schematic_editor::schematic::bus::{expand_members, BusKind};
use konnect_sexp::geometry::{snap_point, SCHEMATIC_GRID_MM};
use serde_json::json;

pub fn tools() -> Vec<ToolDef> {
    vec![
        tool!(
            "add_bus",
            "Draw a bus segment on a schematic. A bus is not a thick wire: a wire connects to \
             it only through a bus entry (add_bus_entry), and the bus takes its name from a \
             label placed on it (add_schematic_net_label), whose syntax — DATA[0..7], \
             {SDA SCL}, or an alias name — decides which nets it carries.",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string", "description": "Path to the .kicad_sch file" },
                    "x1": { "type": "number", "description": "Start X in mm" },
                    "y1": { "type": "number", "description": "Start Y in mm" },
                    "x2": { "type": "number", "description": "End X in mm" },
                    "y2": { "type": "number", "description": "End Y in mm" }
                },
                "required": ["schematic", "x1", "y1", "x2", "y2"]
            }),
            |args, ctx| async move { handle_add_bus(args, ctx).await }
        ),
        tool!(
            "add_bus_entry",
            "Add a bus entry — the short diagonal stub that taps one member out of a bus. \
             Place it at the point on the bus; `dx`/`dy` are a delta, not a corner, and either \
             may be negative to choose the diagonal. The wire for that member must start at \
             the stub's far end, which this returns as `end`.",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string", "description": "Path to the .kicad_sch file" },
                    "x": { "type": "number", "description": "X of the point on the bus, in mm" },
                    "y": { "type": "number", "description": "Y of the point on the bus, in mm" },
                    "dx": { "type": "number", "description": "Stub delta X in mm (default 2.54, may be negative)", "default": 2.54 },
                    "dy": { "type": "number", "description": "Stub delta Y in mm (default 2.54, may be negative)", "default": 2.54 }
                },
                "required": ["schematic", "x", "y"]
            }),
            |args, ctx| async move { handle_add_bus_entry(args, ctx).await }
        ),
        tool!(
            "add_bus_alias",
            "Declare a bus alias: one name standing for an explicit list of member nets, so a \
             bus of unrelated signals can be labelled with a single name. Re-declaring an \
             existing alias replaces its members — a sheet cannot mean two things by one name.",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string", "description": "Path to the .kicad_sch file" },
                    "name": { "type": "string", "description": "Alias name, e.g. 'USB'" },
                    "members": {
                        "type": "array",
                        "description": "Member net names, e.g. ['USB_DP', 'USB_DM']",
                        "items": { "type": "string" }
                    }
                },
                "required": ["schematic", "name", "members"]
            }),
            |args, ctx| async move { handle_add_bus_alias(args, ctx).await }
        ),
        tool!(
            "list_buses",
            "List the bus segments, bus entries, and bus aliases on a schematic, with the \
             labels that name each bus and the members those names expand to.",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string", "description": "Path to the .kicad_sch file" }
                },
                "required": ["schematic"]
            }),
            |args, ctx| async move { handle_list_buses(args, ctx).await }
        ),
        tool!(
            "expand_bus",
            "Expand a bus name into the member nets it stands for, the way KiCAD's \
             connectivity does: DATA[0..7] is a vector, {SDA SCL} (optionally prefixed, \
             MEM{A0 A1} → MEM.A0) is a group, and a bare name may be an alias the schematic \
             declares. A name that is none of these is reported as 'plain' and expands to \
             itself, so any label can be handed over and the kind read back.",
            json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Bus name to expand, e.g. 'DATA[0..7]'" },
                    "schematic": { "type": "string", "description": "Optional .kicad_sch whose bus_alias declarations are consulted. Without it, only vector and group syntax resolve." }
                },
                "required": ["name"]
            }),
            |args, ctx| async move { handle_expand_bus(args, ctx).await }
        ),
    ]
}

// ─── Handlers ────────────────────────────────────────────────────────────────

async fn handle_add_bus(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let mut coords = [0.0f64; 4];
    for (slot, key) in coords.iter_mut().zip(["x1", "y1", "x2", "y2"]) {
        match require_f64(args, key) {
            Ok(value) => *slot = value,
            Err(e) => return Ok(e),
        }
    }
    let (x1, y1) = snap_point(coords[0], coords[1], SCHEMATIC_GRID_MM);
    let (x2, y2) = snap_point(coords[2], coords[3], SCHEMATIC_GRID_MM);

    let mut sch = cse::Schematic::load(&sch_path)?;
    sch.add_bus(x1, y1, x2, y2);
    sch.overwrite()?;

    Ok(CallToolResult::json(&json!({
        "added_bus": { "x1": x1, "y1": y1, "x2": x2, "y2": y2 },
        "bus_count": sch.buses.len()
    })))
}

async fn handle_add_bus_entry(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let x = match require_f64(args, "x") {
        Ok(v) => v,
        Err(e) => return Ok(e),
    };
    let y = match require_f64(args, "y") {
        Ok(v) => v,
        Err(e) => return Ok(e),
    };
    let dx = opt_f64(args, "dx").unwrap_or(2.54);
    let dy = opt_f64(args, "dy").unwrap_or(2.54);

    let (x, y) = snap_point(x, y, SCHEMATIC_GRID_MM);

    let mut sch = cse::Schematic::load(&sch_path)?;
    let entry = sch.add_bus_entry(x, y, dx, dy);
    let end = entry.end();
    sch.overwrite()?;

    Ok(CallToolResult::json(&json!({
        "added_bus_entry": { "x": x, "y": y, "dx": dx, "dy": dy },
        "end": { "x": end.0, "y": end.1 },
        "bus_entry_count": sch.bus_entries.len()
    })))
}

async fn handle_add_bus_alias(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let name = match require_str(args, "name") {
        Ok(v) => v.to_string(),
        Err(e) => return Ok(e),
    };
    let members: Vec<String> = args["members"]
        .as_array()
        .map(|values| {
            values
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    if members.is_empty() {
        anyhow::bail!(
            "'members' must list at least one net name — an alias with no members names nothing"
        );
    }

    let mut sch = cse::Schematic::load(&sch_path)?;
    let replaced = sch.bus_aliases.iter().any(|alias| alias.name == name);
    sch.add_bus_alias(&name, members.clone());
    sch.overwrite()?;

    Ok(CallToolResult::json(&json!({
        "bus_alias": { "name": name, "members": members },
        "replaced_existing": replaced,
        "bus_alias_count": sch.bus_aliases.len()
    })))
}

async fn handle_list_buses(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let sch = cse::Schematic::load(&sch_path)?;

    let buses: Vec<_> = sch
        .buses
        .iter()
        .map(|bus| {
            // A bus is named by a label sitting on one of its endpoints, the
            // same way KiCAD reads it.
            let label = sch
                .labels
                .iter()
                .find(|label| {
                    bus.touches(label.at.x, label.at.y)
                        || konnect_sexp::geometry::point_on_segment(
                            label.at.x,
                            label.at.y,
                            bus.start.0,
                            bus.start.1,
                            bus.end.0,
                            bus.end.1,
                            0.01,
                        )
                })
                .map(|label| label.text.clone());
            let (kind, members) = match &label {
                Some(text) => {
                    let (kind, members) = expand_members(text, &sch.bus_aliases);
                    (Some(kind_label(kind).to_string()), members)
                }
                None => (None, Vec::new()),
            };
            json!({
                "uuid": bus.uuid,
                "start": { "x": bus.start.0, "y": bus.start.1 },
                "end": { "x": bus.end.0, "y": bus.end.1 },
                "label": label,
                "kind": kind,
                "members": members
            })
        })
        .collect();

    let entries: Vec<_> = sch
        .bus_entries
        .iter()
        .map(|entry| {
            let end = entry.end();
            json!({
                "uuid": entry.uuid,
                "at": { "x": entry.x, "y": entry.y },
                "size": { "dx": entry.size.0, "dy": entry.size.1 },
                "end": { "x": end.0, "y": end.1 }
            })
        })
        .collect();

    let aliases: Vec<_> = sch
        .bus_aliases
        .iter()
        .map(|alias| json!({ "name": alias.name, "members": alias.members }))
        .collect();

    Ok(CallToolResult::json(&json!({
        "buses": buses,
        "bus_entries": entries,
        "bus_aliases": aliases,
        "bus_count": sch.buses.len(),
        "bus_entry_count": sch.bus_entries.len(),
        "bus_alias_count": sch.bus_aliases.len()
    })))
}

async fn handle_expand_bus(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let name = match require_str(args, "name") {
        Ok(v) => v.to_string(),
        Err(e) => return Ok(e),
    };

    // Aliases live in the sheet, so resolving one needs the file. Without it,
    // vector and group syntax still resolve — and a name that would have been
    // an alias comes back `plain`, which is the honest answer for what was
    // asked.
    let aliases = match args.get("schematic").and_then(|v| v.as_str()) {
        Some(_) => {
            let sch_path = get_path(args, "schematic")?;
            cse::Schematic::load(&sch_path)?.bus_aliases
        }
        None => Vec::new(),
    };

    let (kind, members) = expand_members(&name, &aliases);
    Ok(CallToolResult::json(&json!({
        "name": name,
        "kind": kind_label(kind),
        "members": members,
        "member_count": members.len()
    })))
}

fn kind_label(kind: BusKind) -> &'static str {
    match kind {
        BusKind::Vector => "vector",
        BusKind::Group => "group",
        BusKind::Alias => "alias",
        BusKind::Plain => "plain",
    }
}

#[cfg(test)]
mod tests {
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
            },
            Arc::new(ToolRouter::new()),
        )
    }

    const BLANK: &str = "(kicad_sch\n\t(version 20250114)\n\t(generator \"konnect\")\n\t(generator_version \"10.0\")\n\t(uuid \"11111111-1111-1111-1111-111111111111\")\n\t(paper \"A4\")\n\t(lib_symbols)\n\t(sheet_instances\n\t\t(path \"/\"\n\t\t\t(page \"1\")\n\t\t)\n\t)\n)\n";

    fn blank_schematic(dir: &std::path::Path) -> std::path::PathBuf {
        let path = dir.join("bus.kicad_sch");
        std::fs::write(&path, BLANK).expect("the schematic is writable");
        path
    }

    fn body(result: &CallToolResult) -> serde_json::Value {
        let text = match result.content.first() {
            Some(crate::mcp::protocol::ToolContent::Text { text }) => text.clone(),
            _ => panic!("the result carries text"),
        };
        serde_json::from_str(&text).expect("the result is JSON")
    }

    /// The whole point of the toolset: a bus, an entry, and an alias survive a
    /// write and come back as themselves rather than as preserved raw nodes.
    #[tokio::test]
    async fn a_bus_an_entry_and_an_alias_round_trip_through_the_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = blank_schematic(dir.path());
        let sch = path.to_str().unwrap();
        let ctx = test_ctx();

        handle_add_bus(
            &json!({ "schematic": sch, "x1": 50.8, "y1": 25.4, "x2": 101.6, "y2": 25.4 }),
            &ctx,
        )
        .await
        .expect("the bus is added");
        handle_add_bus_entry(
            &json!({ "schematic": sch, "x": 63.5, "y": 25.4, "dx": 2.54, "dy": 2.54 }),
            &ctx,
        )
        .await
        .expect("the entry is added");
        handle_add_bus_alias(
            &json!({ "schematic": sch, "name": "USB", "members": ["USB_DP", "USB_DM"] }),
            &ctx,
        )
        .await
        .expect("the alias is added");

        let listed = body(
            &handle_list_buses(&json!({ "schematic": sch }), &ctx)
                .await
                .expect("the buses are listed"),
        );
        assert_eq!(listed["bus_count"], 1);
        assert_eq!(listed["bus_entry_count"], 1);
        assert_eq!(listed["bus_alias_count"], 1);
        assert_eq!(listed["bus_aliases"][0]["members"][1], "USB_DM");
        assert_eq!(listed["bus_entries"][0]["end"]["x"], 66.04);
    }

    /// Coordinates land on KiCAD's schematic grid, like every other tool that
    /// writes a placement — an off-grid bus cannot be connected to.
    #[tokio::test]
    async fn a_bus_is_snapped_to_the_schematic_grid() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = blank_schematic(dir.path());
        let ctx = test_ctx();

        let added = body(
            &handle_add_bus(
                &json!({ "schematic": path.to_str().unwrap(), "x1": 50.9, "y1": 25.3, "x2": 101.7, "y2": 25.3 }),
                &ctx,
            )
            .await
            .expect("the bus is added"),
        );
        assert_eq!(added["added_bus"]["x1"], 50.8);
        assert_eq!(added["added_bus"]["y1"], 25.4);
    }

    /// One name, one meaning: re-declaring an alias replaces it rather than
    /// leaving the sheet with two answers.
    #[tokio::test]
    async fn re_declaring_an_alias_replaces_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = blank_schematic(dir.path());
        let sch = path.to_str().unwrap();
        let ctx = test_ctx();

        handle_add_bus_alias(
            &json!({ "schematic": sch, "name": "CTRL", "members": ["RD"] }),
            &ctx,
        )
        .await
        .expect("the alias is declared");
        let again = body(
            &handle_add_bus_alias(
                &json!({ "schematic": sch, "name": "CTRL", "members": ["RD", "WR"] }),
                &ctx,
            )
            .await
            .expect("the alias is re-declared"),
        );
        assert_eq!(again["replaced_existing"], true);
        assert_eq!(again["bus_alias_count"], 1);

        let listed = body(
            &handle_list_buses(&json!({ "schematic": sch }), &ctx)
                .await
                .expect("the buses are listed"),
        );
        assert_eq!(
            listed["bus_aliases"][0]["members"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }

    /// An alias with no members would name nothing, so it is refused rather
    /// than written.
    #[tokio::test]
    async fn an_alias_with_no_members_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = blank_schematic(dir.path());
        let ctx = test_ctx();
        let error = handle_add_bus_alias(
            &json!({ "schematic": path.to_str().unwrap(), "name": "EMPTY", "members": [] }),
            &ctx,
        )
        .await
        .expect_err("an empty alias is refused");
        assert!(error.to_string().contains("at least one net name"));
    }

    /// `expand_bus` resolves an alias only when it is given the sheet that
    /// declares it — and says `plain` rather than guessing when it is not.
    #[tokio::test]
    async fn an_alias_expands_only_against_the_sheet_that_declares_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = blank_schematic(dir.path());
        let sch = path.to_str().unwrap();
        let ctx = test_ctx();
        handle_add_bus_alias(
            &json!({ "schematic": sch, "name": "USB", "members": ["USB_DP", "USB_DM"] }),
            &ctx,
        )
        .await
        .expect("the alias is declared");

        let with_sheet = body(
            &handle_expand_bus(&json!({ "name": "USB", "schematic": sch }), &ctx)
                .await
                .expect("the alias expands"),
        );
        assert_eq!(with_sheet["kind"], "alias");
        assert_eq!(with_sheet["member_count"], 2);

        let without_sheet = body(
            &handle_expand_bus(&json!({ "name": "USB" }), &ctx)
                .await
                .expect("the name still resolves to something"),
        );
        assert_eq!(without_sheet["kind"], "plain");
        assert_eq!(without_sheet["members"][0], "USB");
    }

    /// Vector and group syntax need no file at all.
    #[tokio::test]
    async fn vector_and_group_syntax_expand_without_a_schematic() {
        let ctx = test_ctx();
        let vector = body(
            &handle_expand_bus(&json!({ "name": "DATA[0..3]" }), &ctx)
                .await
                .expect("the vector expands"),
        );
        assert_eq!(vector["kind"], "vector");
        assert_eq!(vector["members"][3], "DATA3");

        let group = body(
            &handle_expand_bus(&json!({ "name": "MEM{A0 A1}" }), &ctx)
                .await
                .expect("the group expands"),
        );
        assert_eq!(group["kind"], "group");
        assert_eq!(group["members"][0], "MEM.A0");
    }
}
