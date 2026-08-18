//! `graph` toolset — query the indexed world model instead of dumping a
//! document.
//!
//! Three tools, not a gateway verb per document kind (D20, same reasoning as
//! `task`): `graph_query`, `graph_neighbors`, `graph_stats`. As a registry
//! toolset reached through `kicad_invoke`, they cost nothing in `tools/list`
//! until a caller actually loads them — a graph is an optimisation over tools
//! that already exist (`list_schematic_components` and friends), so it must
//! not be baseline weight on every session that never queries it.
//!
//! `crate::graph` builds and caches the [`kam_graph::Graph`] itself; this
//! module is only the MCP skin — argument parsing, the compact inline
//! schemas (D7: no `$defs`/`$ref`), and mapping [`QueryError`] onto this
//! server's stable error kinds (E9: never a Debug-formatted string).
//!
//! ## E7, repeated in every description on purpose
//!
//! An agent reads a tool's description, not `docs/capability-matrix.md`. The
//! limitation that makes a schematic's `net` attribute never appear —
//! Konnect's own connectivity analysis has already disagreed with
//! `kicad-cli sch erc` on a real board — has to be said here, in each of the
//! three descriptions below, or a caller who only ever sees this toolset has
//! no way to know it.

use crate::mcp::error::ToolErrorKind;
use crate::mcp::protocol::CallToolResult;
use crate::tools::{opt_str, require_f64, require_str, ToolContext, ToolDef};
use crate::{tool, try_arg};
use kam_graph::{Attr, Query, QueryError, QueryResult};
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub fn tools() -> Vec<ToolDef> {
    vec![
        tool!(
            "graph_query",
            "Filter indexed items across a project's schematic and PCB documents by kind, label, \
             document, indexed attribute, or spatial region — an intersection of indices instead \
             of a document dump. Returns `total` matched plus a capped `items` sample; a narrower \
             query is the way to see more, not a bigger limit. Indexed attributes: net, lib_id, \
             layer, value, footprint, text. `fields` (default 'compact') controls how much of each \
             item comes back: 'compact' is `document`/`kind`/`key`/`label` plus only the indexed \
             attributes above — enough to filter further or address `graph_neighbors` with `key` \
             as-is, no round-trip needed. 'full' adds every other attribute the document carries \
             (geometry like `at`, `angle`, `unit`), at real token cost — ask for it only when the \
             extra fields are actually needed. An unrecognised `fields` value is refused, not \
             defaulted: a typo must never silently look like 'compact' ran. E7 limitation: Indexes \
             only what the documents state. \
             On a .kicad_pcb, `net` comes from the file's own pads and is a fact. On a .kicad_sch, \
             no item ever carries a `net` — labels and power symbols are indexed as `text`, not as \
             proof of a connection — because Konnect's own connectivity analysis has previously \
             disagreed with kicad-cli ERC on a real schematic. The connectivity verdict comes from \
             run_erc, never from this tool.",
            json!({
                "type": "object",
                "properties": {
                    "project": {
                        "type": "string",
                        "description": "Path to the .kicad_pro file, a design document in the \
                             project, or the project directory"
                    },
                    "kind": { "type": "string", "description": "e.g. 'symbol', 'footprint', 'wire', 'net', 'label'" },
                    "label": { "type": "string" },
                    "document": { "type": "string", "description": "Restrict to one document by file name, e.g. 'power.kicad_sch'" },
                    "attrs": {
                        "type": "array",
                        "description": "Indexed attribute filters, ANDed together. Indexed: net, lib_id, layer, value, footprint, text",
                        "items": {
                            "type": "object",
                            "properties": {
                                "name": { "type": "string" },
                                "value": { "description": "A string, a number, or {\"x\":.., \"y\":..}" }
                            },
                            "required": ["name", "value"]
                        }
                    },
                    "region": {
                        "type": "object",
                        "description": "Axis-aligned box in document units, bounds inclusive",
                        "properties": {
                            "min_x": { "type": "number" },
                            "min_y": { "type": "number" },
                            "max_x": { "type": "number" },
                            "max_y": { "type": "number" }
                        },
                        "required": ["min_x", "min_y", "max_x", "max_y"]
                    },
                    "limit": { "type": "integer", "description": "Default 20, hard-capped at 200" },
                    "fields": {
                        "type": "string",
                        "enum": ["compact", "full"],
                        "description": "'compact' (default): identity plus only the indexed attributes. \
                             'full': every attribute the document carries, at real token cost."
                    }
                },
                "required": ["project"]
            }),
            |args, ctx| async move { handle_graph_query(args, ctx).await }
        ),
        tool!(
            "graph_neighbors",
            "Items whose indexed position is near another item's, nearest first, distance included \
             — never across documents, since two files index unrelated coordinate spaces. E7 \
             limitation: Indexes only what the documents state. On a .kicad_pcb, `net` comes from \
             the file's own pads and is a fact. On a .kicad_sch, no item ever carries a `net` — \
             labels and power symbols are indexed as `text`, not as proof of a connection — \
             because Konnect's own connectivity analysis has previously disagreed with kicad-cli \
             ERC on a real schematic; proximity here is geometric only, never a connection claim. \
             The connectivity verdict comes from run_erc, never from this tool.",
            json!({
                "type": "object",
                "properties": {
                    "project": { "type": "string", "description": "Path to the .kicad_pro file, a design document, or the project directory" },
                    "document": { "type": "string", "description": "File name of the document the target item is in, e.g. 'board.kicad_pcb'" },
                    "kind": { "type": "string" },
                    "key": { "type": "string", "description": "The item's stable key, e.g. its KiCAD uuid" },
                    "radius": { "type": "number" },
                    "limit": { "type": "integer", "description": "Default 20, hard-capped at 200" }
                },
                "required": ["project", "document", "kind", "key", "radius"]
            }),
            |args, ctx| async move { handle_graph_neighbors(args, ctx).await }
        ),
        tool!(
            "graph_stats",
            "Item counts per document and per kind for a project's graph — the cheap orientation \
             query to run before graph_query or graph_neighbors, no items fetched. E7 limitation: \
             Indexes only what the documents state. On a .kicad_pcb, `net` comes from the file's \
             own pads and is a fact. On a .kicad_sch, no item ever carries a `net` — labels and \
             power symbols are indexed as `text`, not as proof of a connection — because Konnect's \
             own connectivity analysis has previously disagreed with kicad-cli ERC on a real \
             schematic. The connectivity verdict comes from run_erc, never from this tool.",
            json!({
                "type": "object",
                "properties": {
                    "project": { "type": "string", "description": "Path to the .kicad_pro file, a design document, or the project directory" }
                },
                "required": ["project"]
            }),
            |args, ctx| async move { handle_graph_stats(args, ctx).await }
        ),
    ]
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

/// How much of each matched item `graph_query` hands back.
///
/// `Compact` (the default) is deliberately not "everything except a
/// blocklist": it is identity — `document`/`kind`/`key`/`label` — plus only
/// the attributes [`crate::graph::INDEXED_ATTRS`] names, because those are
/// exactly the attributes a caller can also filter on. Geometry
/// (`at`/`angle`/`unit`) is real token cost that a caller filtering by kind
/// or value did not ask to pay for; `key` stays the full address either way,
/// since a compact item that `graph_neighbors` could not resolve without a
/// second, `full` round-trip would defeat the reason a graph exists at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fields {
    Compact,
    Full,
}

impl Fields {
    /// Unlike `limit` or `document`, an unrecognised `fields` value is
    /// refused rather than defaulted to `compact` — silently returning a
    /// narrower reply than the caller asked for would look exactly like a
    /// successful `compact` query (D17/E4).
    fn from_args(args: &Value) -> Result<Self, CallToolResult> {
        match args.get("fields") {
            None | Some(Value::Null) => Ok(Self::Compact),
            Some(Value::String(s)) if s == "compact" => Ok(Self::Compact),
            Some(Value::String(s)) if s == "full" => Ok(Self::Full),
            Some(other) => Err(invalid_argument(
                "fields",
                &format!("expected 'compact' or 'full', got {other}"),
            )),
        }
    }
}

/// Project a [`QueryResult`] down to identity plus indexed attributes.
///
/// Mirrors [`kam_graph::graph::Graph::result_item`]'s shape rather than
/// re-deriving `Serialize` on a second struct: the same four identity fields
/// in the same order, `attrs` filtered to
/// [`crate::graph::INDEXED_ATTRS`] and omitted entirely when that leaves it
/// empty, for the same reason `ResultItem` already omits an empty `attrs` —
/// a compact result does not spend tokens saying nothing.
///
/// `kind` is dropped from every item exactly when `document` already is: a
/// query pinned to `kind: "symbol"` guarantees every match is a symbol, so
/// repeating that on each of the (possibly hundred) items would be the same
/// fact restated as many times as there are matches — the reason
/// [`kam_graph::graph::Graph::result_item`] already omits `document` this
/// way. `full` does not get this treatment: it is the pre-projection shape,
/// unconditionally unchanged.
fn compact_result(result: &QueryResult, kind_pinned: bool) -> Value {
    let items: Vec<Value> = result
        .items
        .iter()
        .map(|item| {
            let mut obj = serde_json::Map::new();
            if let Some(document) = &item.document {
                obj.insert("document".to_string(), json!(document));
            }
            if !kind_pinned {
                obj.insert("kind".to_string(), json!(item.kind));
            }
            obj.insert("key".to_string(), json!(item.key));
            obj.insert("label".to_string(), json!(item.label));
            if let Some(distance) = item.distance {
                obj.insert("distance".to_string(), json!(distance));
            }
            let attrs: BTreeMap<&String, &Attr> = item
                .attrs
                .iter()
                .filter(|(name, _)| crate::graph::INDEXED_ATTRS.contains(&name.as_str()))
                .collect();
            if !attrs.is_empty() {
                obj.insert("attrs".to_string(), json!(attrs));
            }
            Value::Object(obj)
        })
        .collect();
    json!({ "total": result.total, "items": items })
}

async fn handle_graph_query(args: &Value, ctx: &ToolContext) -> anyhow::Result<CallToolResult> {
    let project = try_arg!(require_str(args, "project"));
    let fields = match Fields::from_args(args) {
        Ok(fields) => fields,
        Err(rejection) => return Ok(rejection),
    };
    let graph = ctx.graph.get(&crate::graph::project_dir(project))?;

    let mut query = Query::new();
    if let Some(kind) = opt_str(args, "kind") {
        query = query.kind(kind);
    }
    if let Some(label) = opt_str(args, "label") {
        query = query.label(label);
    }
    if let Some(document) = opt_str(args, "document") {
        query = query.document(document);
    }
    if let Some(limit) = args["limit"].as_u64() {
        query = query.limit(limit as usize);
    }
    if let Some(entries) = args["attrs"].as_array() {
        for entry in entries {
            let Some(name) = entry["name"].as_str() else {
                return Ok(invalid_argument(
                    "attrs",
                    "each entry needs a string 'name'",
                ));
            };
            let Some(raw) = entry.get("value") else {
                return Ok(invalid_argument("attrs", "each entry needs a 'value'"));
            };
            let attr: Attr = match serde_json::from_value(raw.clone()) {
                Ok(attr) => attr,
                Err(_) => {
                    return Ok(invalid_argument(
                        "attrs",
                        &format!("attrs['{name}'].value must be a string, a number, or {{x,y}}"),
                    ))
                }
            };
            query = query.attr(name, attr);
        }
    }
    if let Some(region) = args.get("region") {
        let bound = |k: &str| region[k].as_f64();
        match (
            bound("min_x"),
            bound("min_y"),
            bound("max_x"),
            bound("max_y"),
        ) {
            (Some(min_x), Some(min_y), Some(max_x), Some(max_y)) => {
                query = query.region(min_x, min_y, max_x, max_y);
            }
            _ => {
                return Ok(invalid_argument(
                    "region",
                    "region needs numeric min_x, min_y, max_x, max_y",
                ))
            }
        }
    }

    match graph.query(&query) {
        Ok(result) => Ok(match fields {
            Fields::Full => CallToolResult::json(&result),
            Fields::Compact => CallToolResult::json(&compact_result(&result, query.kind.is_some())),
        }),
        Err(e @ QueryError::AttributeNotIndexed(_)) => Ok(invalid_argument(
            "attrs",
            &format!("{} ({})", e.message(), e.code()),
        )),
        Err(e @ QueryError::NoSpatialIndex) => Ok(invalid_argument(
            "region",
            &format!("{} ({})", e.message(), e.code()),
        )),
        // graph_query never resolves a single item address, so neither of
        // these can be returned by Graph::query.
        Err(e @ (QueryError::ItemNotFound | QueryError::ItemHasNoPoint)) => {
            unreachable!("Graph::query cannot return {e:?}")
        }
    }
}

async fn handle_graph_neighbors(args: &Value, ctx: &ToolContext) -> anyhow::Result<CallToolResult> {
    let project = try_arg!(require_str(args, "project"));
    let document = try_arg!(require_str(args, "document"));
    let kind = try_arg!(require_str(args, "kind"));
    let key = try_arg!(require_str(args, "key"));
    let radius = try_arg!(require_f64(args, "radius"));
    let limit = args["limit"].as_u64().unwrap_or(20) as usize;

    let graph = ctx.graph.get(&crate::graph::project_dir(project))?;

    match graph.neighbors(document, kind, key, radius, limit) {
        Ok(result) => Ok(CallToolResult::json(&result)),
        Err(QueryError::ItemNotFound) => Ok(CallToolResult::error_kind(
            ToolErrorKind::NotFound {
                document: document.to_string(),
                item_kind: kind.to_string(),
                key: key.to_string(),
            },
            format!("No item at {document}/{kind}/{key}."),
        )),
        Err(e @ QueryError::ItemHasNoPoint) => Ok(invalid_argument(
            "key",
            &format!("{document}/{kind}/{key}: {} ({})", e.message(), e.code()),
        )),
        // A single-item lookup never filters by attribute or region.
        Err(e @ (QueryError::AttributeNotIndexed(_) | QueryError::NoSpatialIndex)) => {
            unreachable!("Graph::neighbors cannot return {e:?}")
        }
    }
}

async fn handle_graph_stats(args: &Value, ctx: &ToolContext) -> anyhow::Result<CallToolResult> {
    let project = try_arg!(require_str(args, "project"));
    let graph = ctx.graph.get(&crate::graph::project_dir(project))?;
    let stats = graph.stats();
    Ok(CallToolResult::json(&json!({
        "documents": stats.documents,
        "kinds": stats.kinds,
    })))
}

fn invalid_argument(field: &str, reason: &str) -> CallToolResult {
    CallToolResult::error_kind(
        ToolErrorKind::InvalidArgument {
            field: field.to_string(),
            reason: reason.to_string(),
        },
        format!("Argument '{field}' is invalid: {reason}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::error::extract_error_kind;
    use crate::router::ToolRouter;
    use std::sync::Arc;

    fn ctx() -> ToolContext {
        ToolContext::new(
            crate::tools::ServerConfig {
                kicad_cli: "kicad-cli".to_string(),
                kicad_binary: "kicad".to_string(),
                ipc_address: String::new(),
                project_dir: None,
                jlcpcb_db_path: None,
                auto_load_toolsets: false,
                mode: kam_state::OperatingMode::Write,
            },
            Arc::new(ToolRouter::new()),
        )
    }

    fn body(result: &CallToolResult) -> Value {
        let crate::mcp::protocol::ToolContent::Text { text } = &result.content[0] else {
            panic!("expected text content")
        };
        serde_json::from_str(text).unwrap()
    }

    const DIVIDER_SCH: &str = r#"
(kicad_sch
  (version 20260306)
  (symbol (lib_id "Device:R") (at 100 80 0) (unit 1) (uuid "s-1")
    (property "Reference" "R1" (at 102 79 0))
    (property "Value" "10k" (at 102 81 0)))
  (symbol (lib_id "Device:R") (at 100 100 0) (unit 1) (uuid "s-2")
    (property "Reference" "R2" (at 102 99 0))
    (property "Value" "10k" (at 102 101 0)))
  (label "VOUT" (at 100 90 0) (uuid "l-1")))
"#;

    fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("board.kicad_sch"), DIVIDER_SCH).unwrap();
        dir
    }

    #[tokio::test]
    async fn graph_query_finds_symbols_and_drops_the_document_field_when_it_is_pinned() {
        let dir = project();
        let ctx = ctx();
        let result = handle_graph_query(
            &json!({ "project": dir.path().to_str().unwrap(), "kind": "symbol", "document": "board.kicad_sch" }),
            &ctx,
        )
        .await
        .unwrap();
        let body = body(&result);
        assert_eq!(body["total"], 2);
        assert!(body["items"][0].get("document").is_none());
    }

    #[tokio::test]
    async fn graph_query_by_indexed_attribute_matches_the_value() {
        let dir = project();
        let ctx = ctx();
        let result = handle_graph_query(
            &json!({
                "project": dir.path().to_str().unwrap(),
                "attrs": [{ "name": "value", "value": "10k" }]
            }),
            &ctx,
        )
        .await
        .unwrap();
        assert_eq!(body(&result)["total"], 2);
    }

    #[tokio::test]
    async fn a_schematic_symbol_never_reports_a_net_attribute() {
        let dir = project();
        let ctx = ctx();
        let result = handle_graph_query(
            &json!({ "project": dir.path().to_str().unwrap(), "kind": "symbol", "limit": 1 }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(body(&result)["items"][0]["attrs"].get("net").is_none());
    }

    #[tokio::test]
    async fn an_unindexed_attribute_is_invalid_argument_not_an_empty_result() {
        let dir = project();
        let ctx = ctx();
        let result = handle_graph_query(
            &json!({
                "project": dir.path().to_str().unwrap(),
                "attrs": [{ "name": "angle", "value": "0" }]
            }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(result.is_error);
        assert_eq!(
            extract_error_kind(&result).as_deref(),
            Some("invalid_argument")
        );
        assert_eq!(body(&result)["error"]["field"], "attrs");
    }

    #[tokio::test]
    async fn a_region_query_without_a_spatial_build_flag_still_works_since_at_is_always_indexed() {
        let dir = project();
        let ctx = ctx();
        let result = handle_graph_query(
            &json!({
                "project": dir.path().to_str().unwrap(),
                "region": { "min_x": 0.0, "min_y": 0.0, "max_x": 200.0, "max_y": 200.0 }
            }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(!result.is_error);
        assert!(body(&result)["total"].as_u64().unwrap() >= 2);
    }

    #[tokio::test]
    async fn graph_neighbors_finds_the_other_resistor() {
        let dir = project();
        let ctx = ctx();
        let result = handle_graph_neighbors(
            &json!({
                "project": dir.path().to_str().unwrap(),
                "document": "board.kicad_sch",
                "kind": "symbol",
                "key": "s-1",
                "radius": 50.0
            }),
            &ctx,
        )
        .await
        .unwrap();
        let body = body(&result);
        // Neighbors are not filtered by kind, so the "VOUT" label at
        // (100,90) — closer to R1 than R2 is — matches too.
        assert_eq!(body["total"], 2);
        let keys: Vec<_> = body["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["key"].as_str().unwrap())
            .collect();
        assert_eq!(
            keys,
            ["l-1", "s-2"],
            "nearest first: label at distance 10, R2 at distance 20"
        );
    }

    #[tokio::test]
    async fn graph_neighbors_on_a_missing_item_is_a_structured_not_found() {
        let dir = project();
        let ctx = ctx();
        let result = handle_graph_neighbors(
            &json!({
                "project": dir.path().to_str().unwrap(),
                "document": "board.kicad_sch",
                "kind": "symbol",
                "key": "ghost",
                "radius": 5.0
            }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(result.is_error);
        assert_eq!(extract_error_kind(&result).as_deref(), Some("not_found"));
        assert_eq!(body(&result)["error"]["key"], "ghost");
    }

    #[tokio::test]
    async fn graph_stats_counts_documents_and_kinds() {
        let dir = project();
        let ctx = ctx();
        let result = handle_graph_stats(&json!({ "project": dir.path().to_str().unwrap() }), &ctx)
            .await
            .unwrap();
        let body = body(&result);
        assert_eq!(body["documents"]["board.kicad_sch"], 3);
        assert_eq!(body["kinds"]["symbol"], 2);
    }

    #[tokio::test]
    async fn compact_is_the_default_and_drops_geometry_but_keeps_indexed_attrs() {
        let dir = project();
        let ctx = ctx();
        let result = handle_graph_query(
            &json!({ "project": dir.path().to_str().unwrap(), "kind": "symbol", "limit": 1 }),
            &ctx,
        )
        .await
        .unwrap();
        let attrs = &body(&result)["items"][0]["attrs"];
        assert_eq!(attrs["lib_id"], "Device:R");
        assert_eq!(attrs["value"], "10k");
        assert!(attrs.get("angle").is_none());
        assert!(attrs.get("unit").is_none());
        assert!(attrs.get("at").is_none());
    }

    #[tokio::test]
    async fn compact_drops_kind_per_item_when_the_query_already_pinned_it() {
        let dir = project();
        let ctx = ctx();
        let pinned = handle_graph_query(
            &json!({ "project": dir.path().to_str().unwrap(), "kind": "symbol", "limit": 1 }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(body(&pinned)["items"][0].get("kind").is_none());

        let unpinned = handle_graph_query(
            &json!({ "project": dir.path().to_str().unwrap(), "limit": 1 }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(body(&unpinned)["items"][0].get("kind").is_some());
    }

    #[tokio::test]
    async fn fields_full_keeps_the_geometry_compact_drops() {
        let dir = project();
        let ctx = ctx();
        let result = handle_graph_query(
            &json!({ "project": dir.path().to_str().unwrap(), "kind": "symbol", "limit": 1, "fields": "full" }),
            &ctx,
        )
        .await
        .unwrap();
        let attrs = &body(&result)["items"][0]["attrs"];
        assert_eq!(attrs["angle"], 0.0);
        assert_eq!(attrs["unit"], 1.0);
        assert!(attrs.get("at").is_some());
    }

    #[tokio::test]
    async fn an_unrecognised_fields_value_is_invalid_argument_not_a_silent_default() {
        let dir = project();
        let ctx = ctx();
        let result = handle_graph_query(
            &json!({ "project": dir.path().to_str().unwrap(), "kind": "symbol", "fields": "verbose" }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(result.is_error);
        assert_eq!(
            extract_error_kind(&result).as_deref(),
            Some("invalid_argument")
        );
        assert_eq!(body(&result)["error"]["field"], "fields");
    }

    #[tokio::test]
    async fn a_compact_key_is_still_accepted_by_graph_neighbors_with_no_round_trip() {
        let dir = project();
        let ctx = ctx();
        let queried = handle_graph_query(
            &json!({
                "project": dir.path().to_str().unwrap(),
                "kind": "symbol",
                "document": "board.kicad_sch",
                "limit": 1
            }),
            &ctx,
        )
        .await
        .unwrap();
        let key = body(&queried)["items"][0]["key"]
            .as_str()
            .unwrap()
            .to_string();

        let neighbors = handle_graph_neighbors(
            &json!({
                "project": dir.path().to_str().unwrap(),
                "document": "board.kicad_sch",
                "kind": "symbol",
                "key": key,
                "radius": 50.0
            }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(!neighbors.is_error);
    }

    #[tokio::test]
    async fn a_missing_project_argument_is_a_structured_error() {
        let ctx = ctx();
        let result = handle_graph_stats(&json!({}), &ctx).await.unwrap();
        assert!(result.is_error);
        assert_eq!(
            extract_error_kind(&result).as_deref(),
            Some("invalid_argument")
        );
    }
}
