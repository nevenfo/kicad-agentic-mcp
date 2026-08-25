//! P.6.9.16: every `required` list in the registry is honest, checked in one
//! pass over all of it rather than wherever a test happened to exercise a
//! tool.
//!
//! P.6.9.10 put the enforcement in (`missing_required_refusal` at dispatch)
//! and found no lying schema — but it only looked where a test already called
//! the tool, and only proved *omission* (a handler that needs a key its
//! schema never lists), because dispatch refuses before a handler runs: any
//! key `required` names is enforced there regardless of what the handler
//! underneath would have done with it, so calling through dispatch can never
//! show a schema *over*-promising.
//!
//! This pass calls each `ToolDef`'s handler directly — `(tool.handler)(&{},
//! ctx)`, bypassing `McpHandler::dispatch_tool` and its
//! `missing_required_refusal` gate entirely — so a tool with a non-empty
//! `required` list is called with `{}` and its handler's own answer is what
//! gets checked, both directions, in one call:
//!
//! * the handler succeeds despite `required` being non-empty — the schema
//!   over-promises, or the handler ignores a key it claims to need;
//! * the handler refuses `invalid_argument` on a field *absent* from its own
//!   `required` list — the omission class P.6.9.7 and P.6.9.9 fixed by hand,
//!   site by site, now visible without a dispatch gate standing in front of
//!   it;
//! * the handler refuses on a field present in `required` — consistent,
//!   nothing to report;
//! * the handler refuses `invalid_argument` on a field despite declaring no
//!   `required` list at all — also an omission, and now checked even though
//!   there is no `required` list to score the refusal against.
//!
//! What it does *not* cover:
//!
//! * tools with no `required` list whose handler answers something other
//!   than a missing-field refusal on `{}`: `{}` may legitimately succeed
//!   (every argument optional) or fail on its own terms, so there is nothing
//!   to assert beyond the missing-field case above. They are counted and
//!   named in the printed report only.
//! * `kicad_invoke`'s batch *entries*: D131 exempts them from
//!   `first_missing_required` on purpose, and
//!   `required_args::the_gateway_envelope_is_checked_but_not_its_entries`
//!   freezes that. Only the gateway's own envelope (`calls`) is checked here,
//!   like any other schema — and `handle_kicad_invoke` validates `calls`
//!   itself before touching a batch entry, so calling it directly still
//!   proves the envelope honest.
//! * argument *types*: this is presence only, exactly like
//!   `first_missing_required`. A key present with the wrong type is the
//!   `require_*` helpers' business, inside the handler.
//! * meta-tools' internal use of `ctx.router`: the context built here starts
//!   with no toolset loaded, since nothing here dispatches through it. A
//!   meta-tool that needs a loaded toolset for some *other* argument would
//!   not be exercised that way here — irrelevant on an empty call, where the
//!   only question is what a missing required key does.
//! * a `required` list naming more than one key: `{}` is missing every key
//!   at once, so it only ever proves the *first* one the handler happens to
//!   check — a schema listing `["board", "uuid"]` whose handler only ever
//!   reads `uuid` passes this pass exactly like one that reads both. Measured
//!   in `pcb_routing.rs`: `"board"` appears in 33 tool schemas there, either
//!   in `properties` or `required`, and is actually read by only 5 handlers
//!   (`get_path(args, "board")` at lines 267, 343, 495, 729, 809). The class
//!   this pass caught for `query_traces` and `get_nets_list` (P.6.9.16) is
//!   therefore narrower than the class that exists — those two happened to
//!   check no other required key first. A schema with `board` unread but
//!   listed *after* another, checked key in `required` would not be caught
//!   by this pass at all.
//!
//! Not cheap by construction any more: calling a handler directly forgoes
//! dispatch's before-the-handler refusal, so a handler that does I/O, spawns
//! a process, or reaches the network on `{}` now actually would. `EXCLUDED`
//! is the measured list of tools that do.

mod harness;

use harness::body;
use konnect_core::mcp::error::ToolErrorKind;
use konnect_core::mcp::handler::McpHandler;
use konnect_core::mcp::protocol::CallToolResult;
use konnect_core::router::{meta_tools, registry, ToolRouter};
use konnect_core::tools::{ServerConfig, ToolContext, ToolDef};
use serde_json::{json, Value};
use std::sync::Arc;

/// Tools deliberately not called with `{}`, with the reason. A name here that
/// is no longer registered fails the test, so the list cannot rot into a
/// silent hole in the pass.
///
/// All three declare no `required` list, so excluding them costs the pass no
/// assertion at all — they would only have been counted and reported. Each is
/// excluded for what an empty call *does*, not for what it answers.
///
/// D132/P.6.9.19: the three names are built with `concat!` rather than
/// written as plain string literals. `capability/coverage.rs`'s scanner
/// (`mentions`, a lexical `contains(line, "\"{tool}\"")`) treats any line
/// holding the quoted tool name as proof of a test — with the literals
/// written out, this file would flip `save_project`, `launch_kicad_ui`, and
/// `download_jlcpcb_database` from `NOT_TESTED` to `SUPPORTED` in
/// `docs/capability-matrix.md`, which the matrix is never regenerated to
/// reflect: this test refuses, on purpose, to call any of the three.
const EXCLUDED: &[(&str, &str)] = &[
    (
        concat!("download_", "jlcpcb_database"),
        "an empty call falls back to the machine-wide default database path \
         (`resolve_db_path`); where that file is absent it fetches hundreds of \
         megabytes over the network",
    ),
    (
        concat!("launch_", "kicad_ui"),
        "spawns the KiCAD GUI; only the empty `kicad_binary` in this config stops \
         it, which is too thin a guard to rely on",
    ),
    (
        concat!("save_", "project"),
        "writes the board of a live KiCAD session over IPC — a test must not be \
         able to save someone's open design",
    ),
];

/// A context with no `kicad-cli`, no KiCAD binary, no IPC address and no
/// toolset loaded — nothing here may reach a real KiCAD, and nothing here
/// dispatches through `ctx.router`, so an unloaded one costs nothing.
fn context(project_dir: &std::path::Path) -> Arc<ToolContext> {
    let config = ServerConfig {
        kicad_cli: String::new(),
        kicad_binary: String::new(),
        ipc_address: String::new(),
        project_dir: Some(project_dir.to_path_buf()),
        jlcpcb_db_path: None,
        auto_load_toolsets: false,
        // D130: the mode gate lives in `dispatch_tool`, which this pass never
        // reaches — a handler called directly sees no mode check either way.
        // `Write` matches every other field's unrestricted default.
        mode: kam_state::OperatingMode::Write,
    };
    Arc::new(ToolContext::new(config, Arc::new(ToolRouter::new())))
}

/// One entry in the registry: a domain tool's `ToolDef`, or `None` for a
/// meta-tool (which has no `ToolDef` — only `handle_meta_tool` reaches it).
struct Entry {
    name: String,
    schema: Value,
    tool: Option<ToolDef>,
}

/// Every registered domain tool and meta-tool, paired with its schema.
fn every_entry() -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    for meta in registry::ALL_TOOLSETS {
        let tools = registry::tools_for(meta.name)
            .unwrap_or_else(|| panic!("toolset '{}' is listed but has no tools", meta.name));
        for tool in tools {
            out.push(Entry {
                name: tool.name.to_string(),
                schema: tool.input_schema.clone(),
                tool: Some(tool),
            });
        }
    }
    for name in meta_tools::META_TOOL_NAMES {
        let schema = meta_tools::meta_tool_schema(name)
            .unwrap_or_else(|| panic!("meta-tool '{name}' has no schema"));
        out.push(Entry {
            name: name.to_string(),
            schema: schema.clone(),
            tool: None,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out.dedup_by(|a, b| a.name == b.name);
    out
}

/// Call `entry`'s handler on `{}`, bypassing `McpHandler::dispatch_tool`
/// entirely. A domain handler that returns `Err` is turned into the same
/// structured `CallToolResult` `dispatch_tool` itself would build from it
/// (`ToolErrorKind::from_anyhow`) — `get_path` and friends answer a missing
/// key through `anyhow::Result`, downcast back into `InvalidArgument` there,
/// so skipping that step would misreport every one of them as a kind
/// mismatch instead of judging what the client actually sees.
async fn call_empty(entry: &Entry, ctx: &Arc<ToolContext>) -> CallToolResult {
    match &entry.tool {
        Some(tool) => match (tool.handler)(&json!({}), ctx.clone()).await {
            Ok(result) => result,
            Err(e) => {
                let kind = ToolErrorKind::from_anyhow(&e);
                CallToolResult::error_kind(kind, format!("Tool error: {e}"))
            }
        },
        None => meta_tools::handle_meta_tool(&entry.name, &json!({}), ctx)
            .await
            .unwrap_or_else(|| {
                panic!(
                    "'{}' is in META_TOOL_NAMES but handle_meta_tool doesn't recognize it",
                    entry.name
                )
            }),
    }
}

fn required_keys(schema: &Value) -> Vec<String> {
    schema
        .get("required")
        .and_then(Value::as_array)
        .map(|keys| {
            keys.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// P.6.9.16: `required` alone cannot express "one of these" (a conjunction
/// only), so `get_datasheet_url` publishes the disjunction as `anyOf:
/// [{required: [mpn]}, {required: [lcsc_id]}]` alongside `required: []`. A
/// refusal naming a field that appears in *any* `anyOf` branch's own
/// `required` list is honest under that contract, even though `field` is
/// absent from the schema's own (necessarily empty) top-level `required`.
fn any_of_names(schema: &Value, field: &str) -> bool {
    schema
        .get("anyOf")
        .and_then(Value::as_array)
        .is_some_and(|branches| {
            branches
                .iter()
                .any(|branch| required_keys(branch).iter().any(|key| key == field))
        })
}

#[tokio::test]
async fn every_required_list_is_honest() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = context(dir.path());
    let entries = every_entry();
    assert!(
        entries.len() > 190,
        "the registry shrank unexpectedly: {} tools",
        entries.len()
    );

    for (name, reason) in EXCLUDED {
        assert!(
            entries.iter().any(|entry| &entry.name == name),
            "excluded tool '{name}' ({reason}) is no longer registered — drop it from EXCLUDED \
             so the exclusion list cannot rot"
        );
    }

    let mut liars: Vec<String> = Vec::new();
    let mut checked = 0usize;
    // Reported, never asserted on beyond the missing-field check below: see
    // the module docs.
    let mut no_required: Vec<&str> = Vec::new();
    let mut open_and_succeeding: Vec<&str> = Vec::new();

    for entry in &entries {
        if EXCLUDED.iter().any(|(excluded, _)| *excluded == entry.name) {
            continue;
        }
        let required = required_keys(&entry.schema);
        let resp = call_empty(entry, &ctx).await;
        let is_error = resp.is_error;
        let response_body = body(&resp);
        let kind = response_body["error"]["kind"].as_str().unwrap_or_default();
        let field = response_body["error"]["field"].as_str().unwrap_or_default();

        if required.is_empty() {
            no_required.push(&entry.name);
            if !is_error {
                open_and_succeeding.push(&entry.name);
            } else if kind == "invalid_argument" && !any_of_names(&entry.schema, field) {
                // No `required` list, yet the handler refuses a missing key —
                // the omission P.6.9.7/P.6.9.9 fixed by hand, now visible
                // without a `required` list to compare the refusal against.
                // Unless the field is named in an `anyOf` branch instead: an
                // empty top-level `required` is then the honest answer to a
                // disjunctive contract, not an omission.
                liars.push(format!(
                    "{}: declares no required list but refused on field '{field}' as \
                     invalid_argument — the field belongs in required",
                    entry.name
                ));
            }
            continue;
        }

        checked += 1;
        if !is_error {
            liars.push(format!(
                "{}: requires {required:?} but succeeded on an empty argument object — the \
                 schema over-promises, or the handler ignores the key",
                entry.name
            ));
            continue;
        }
        if kind != "invalid_argument" {
            liars.push(format!(
                "{}: requires {required:?} but an empty call answered kind '{kind}' instead of \
                 invalid_argument",
                entry.name
            ));
        } else if !required.iter().any(|key| key == field) {
            liars.push(format!(
                "{}: refused on field '{field}', which is not in its own required list \
                 {required:?}",
                entry.name
            ));
        }
    }

    println!(
        "required-schema pass: {} tools total, {checked} with a required list checked, {} \
         without one, {} of those succeed on an empty call ({open_and_succeeding:?}), {} \
         excluded",
        entries.len(),
        no_required.len(),
        open_and_succeeding.len(),
        EXCLUDED.len()
    );

    assert!(
        liars.is_empty(),
        "these schemas do not tell the truth about their own required list:\n  {}",
        liars.join("\n  ")
    );
}

/// D131: `kicad_invoke` batch entries skip `first_missing_required` on
/// purpose — `required_args::the_gateway_envelope_is_checked_but_not_its_entries`
/// freezes that as intentional for the envelope's own `calls` key. But
/// `run_design_review`'s schema names `schematic` as `required` while its
/// handler only ever *checks* `args["schematic"].is_string()` and silently
/// skips every schematic audit otherwise (`run_design_review_with`,
/// `design_review.rs`) — so a batch entry that omits it used to get a
/// "review complete" success with an empty report instead of the refusal
/// dispatch would have given it directly. `handle_run_design_review` now
/// guards on `schematic` itself, so this proves the batch path answers the
/// same refusal, not the false success P.6.9.16 measured before the fix
/// (recorded in the module docs' history, not reproduced here since the
/// handler no longer allows it).
#[tokio::test]
async fn run_design_review_refuses_a_batch_entry_missing_schematic() {
    let dir = tempfile::tempdir().unwrap();
    let config = ServerConfig {
        kicad_cli: String::new(),
        kicad_binary: String::new(),
        ipc_address: String::new(),
        project_dir: Some(dir.path().to_path_buf()),
        jlcpcb_db_path: None,
        auto_load_toolsets: false,
        mode: kam_state::OperatingMode::Write,
    };
    let handler = McpHandler::new(config).await.expect("handler constructs");

    let req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "kicad_invoke",
            "arguments": {
                "calls": [
                    { "tool": "run_design_review", "args": {} }
                ]
            }
        }
    });
    let resp = handler
        .handle_message(req)
        .await
        .expect("tools/call always answers");
    let resp = serde_json::to_value(resp).expect("response serializes");

    let text = resp["result"]["content"][0]["text"]
        .as_str()
        .expect("tool result carries text content");
    let batch_body: Value = serde_json::from_str(text).unwrap();

    assert_eq!(
        batch_body["results"][0]["ok"], false,
        "a batch entry missing 'schematic' must not report success with an unreviewed \
         report: {batch_body}"
    );
    assert_eq!(
        batch_body["results"][0]["result"]["error"]["kind"], "invalid_argument",
        "{batch_body}"
    );
    assert_eq!(
        batch_body["results"][0]["result"]["error"]["field"], "schematic",
        "{batch_body}"
    );
}
