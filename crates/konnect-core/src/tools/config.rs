//! `config` toolset — User preferences, project rules, and effective configuration.
//!
//! Persists user-level config to `~/.konnect/config.json` and project-level
//! config to `<project_dir>/.konnect/project.json`. Claude should call
//! `load_user_config` at the start of every session.

use crate::mcp::error::ToolErrorKind;
use crate::mcp::protocol::CallToolResult;
use crate::tool;
use crate::tools::{require_str, ToolContext, ToolDef};
use serde_json::json;
use std::path::{Path, PathBuf};
use tracing::{debug, info};

// ─── Default config ──────────────────────────────────────────────────────────

fn default_user_config() -> serde_json::Value {
    json!({
        "preferred_manufacturers": [],
        "preferred_distributors": ["JLCPCB", "LCSC"],
        "default_passives": {
            "decoupling_cap": "100nF X7R 0402",
            "pull_up": "10k 0402",
            "bulk_cap": "10uF X5R 0805"
        },
        "fab_constraints": {
            "min_trace_width_mm": 0.15,
            "min_via_drill_mm": 0.3,
            "min_clearance_mm": 0.15,
            "layer_count": 2,
            "fab_house": "JLCPCB"
        },
        "naming_conventions": {
            "net_prefix_power": "VCC_",
            "net_prefix_ground": "GND"
        },
        "design_rules": []
    })
}

fn default_project_config() -> serde_json::Value {
    json!({
        "design_rules": [],
        "fab_constraints": {},
        "naming_conventions": {}
    })
}

// ─── Config file paths ───────────────────────────────────────────────────────

fn user_config_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var("APPDATA").unwrap_or_default();
        PathBuf::from(appdata).join("konnect")
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").unwrap_or_default();
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("konnect")
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let home = std::env::var("HOME").unwrap_or_default();
        PathBuf::from(home).join(".konnect")
    }
}

fn user_config_path() -> PathBuf {
    user_config_dir().join("config.json")
}

fn project_config_path(project_dir: &Path) -> PathBuf {
    project_dir.join(".konnect").join("project.json")
}

// ─── Config I/O helpers ──────────────────────────────────────────────────────

async fn read_config(path: &Path, default: serde_json::Value) -> serde_json::Value {
    match tokio::fs::read_to_string(path).await {
        Ok(content) => serde_json::from_str(&content).unwrap_or(default),
        Err(_) => default,
    }
}

async fn write_config(path: &Path, config: &serde_json::Value) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let content = serde_json::to_string_pretty(config)?;
    tokio::fs::write(path, content).await?;
    Ok(())
}

/// Deep merge: overlay values onto base. overlay takes precedence.
fn deep_merge(base: &serde_json::Value, overlay: &serde_json::Value) -> serde_json::Value {
    match (base, overlay) {
        (serde_json::Value::Object(b), serde_json::Value::Object(o)) => {
            let mut merged = b.clone();
            for (key, val) in o {
                let base_val = merged.get(key).cloned().unwrap_or(serde_json::Value::Null);
                merged.insert(key.clone(), deep_merge(&base_val, val));
            }
            serde_json::Value::Object(merged)
        }
        (_, overlay) if !overlay.is_null() => overlay.clone(),
        (base, _) => base.clone(),
    }
}

/// Set a value at a dot-notation path, e.g. "fab_constraints.fab_house" = "JLCPCB".
///
/// Fails with an error (instead of panicking) if a segment of the path already
/// holds a non-object value, since there is nowhere to insert the child key.
fn set_dot_path(
    config: &mut serde_json::Value,
    key_path: &str,
    value: serde_json::Value,
) -> anyhow::Result<()> {
    let parts: Vec<&str> = key_path.split('.').collect();
    let mut current = config;
    for (i, part) in parts.iter().enumerate() {
        if i == parts.len() - 1 {
            // Last part — set the value
            return match current {
                serde_json::Value::Object(map) => {
                    map.insert(part.to_string(), value);
                    Ok(())
                }
                other => anyhow::bail!(
                    "Cannot set '{key_path}': '{}' is not an object (found {})",
                    parts[..i].join("."),
                    json_type_name(other)
                ),
            };
        }
        // Navigate into nested object, creating it if missing.
        if !current.get(*part).map(|v| v.is_object()).unwrap_or(false) {
            match current {
                serde_json::Value::Object(map) => {
                    map.insert(part.to_string(), json!({}));
                }
                other => anyhow::bail!(
                    "Cannot set '{key_path}': '{}' is not an object (found {})",
                    parts[..i].join("."),
                    json_type_name(other)
                ),
            }
        }
        current = current
            .get_mut(*part)
            .expect("just verified or inserted as an object above");
    }
    Ok(())
}

fn json_type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

// ─── Tool definitions ─────────────────────────────────────────────────────────

pub fn tools() -> Vec<ToolDef> {
    vec![
        tool!(
            "load_user_config",
            "Load the user's global Konnect preferences. Call this at the start of every session \
             to get preferred manufacturers, fab constraints, default passives, and design rules.",
            json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
            |args, ctx| async move { handle_load_user_config(args, ctx).await }
        ),
        tool!(
            "save_user_config",
            "Update a user preference. Use dot-notation for nested keys, e.g. 'fab_constraints.fab_house'. \
             Call this when the user says things like 'always use JLCPCB' or 'I prefer 0402 passives'.",
            json!({
                "type": "object",
                "properties": {
                    "key_path": {
                        "type": "string",
                        "description": "Dot-notation path to the config key, e.g. 'fab_constraints.fab_house' or 'default_passives.decoupling_cap'"
                    },
                    "value": {
                        "description": "New value to set (string, number, array, or object)"
                    }
                },
                "required": ["key_path", "value"]
            }),
            |args, ctx| async move { handle_save_user_config(args, ctx).await }
        ),
        tool!(
            "load_project_config",
            "Load project-specific configuration from <project_dir>/.konnect/project.json. \
             Project config overrides user config where both exist.",
            json!({
                "type": "object",
                "properties": {
                    "project_dir": {
                        "type": "string",
                        "description": "Path to the KiCAD project directory. If omitted, uses the configured project_dir."
                    }
                },
                "required": []
            }),
            |args, ctx| async move { handle_load_project_config(args, ctx).await }
        ),
        tool!(
            "save_project_config",
            "Save a project-specific rule or override. Same dot-notation as save_user_config \
             but writes to the project's .konnect/project.json.",
            json!({
                "type": "object",
                "properties": {
                    "project_dir": { "type": "string", "description": "Project directory (optional, uses configured default)" },
                    "key_path": { "type": "string", "description": "Dot-notation config key" },
                    "value": { "description": "New value to set" }
                },
                "required": ["key_path", "value"]
            }),
            |args, ctx| async move { handle_save_project_config(args, ctx).await }
        ),
        tool!(
            "get_effective_config",
            "Return the merged configuration (user defaults + project overrides). \
             This is the config Claude should use for all design decisions.",
            json!({
                "type": "object",
                "properties": {
                    "project_dir": { "type": "string", "description": "Project directory (optional)" }
                },
                "required": []
            }),
            |args, ctx| async move { handle_get_effective_config(args, ctx).await }
        ),
        tool!(
            "add_design_rule",
            "Add a natural-language design rule that Claude should follow in this project. \
             Examples: 'Always use 100nF X7R for MCU decoupling within 3mm of power pin', \
             'Route USB D+/D- as 90-ohm differential pair'.",
            json!({
                "type": "object",
                "properties": {
                    "rule": { "type": "string", "description": "The design rule in plain English" },
                    "scope": {
                        "type": "string",
                        "description": "'user' (applies to all projects) or 'project' (this project only)",
                        "default": "project"
                    },
                    "project_dir": { "type": "string", "description": "Project directory (for project-scoped rules)" }
                },
                "required": ["rule"]
            }),
            |args, ctx| async move { handle_add_design_rule(args, ctx).await }
        ),
        tool!(
            "list_design_rules",
            "List all active design rules (user-level + project-level).",
            json!({
                "type": "object",
                "properties": {
                    "project_dir": { "type": "string", "description": "Project directory (optional)" }
                },
                "required": []
            }),
            |args, ctx| async move { handle_list_design_rules(args, ctx).await }
        ),
    ]
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

async fn handle_load_user_config(
    _args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let path = user_config_path();
    info!(path = %path.display(), "[BETA] Loading user config");
    let config = read_config(&path, default_user_config()).await;

    // Create default config file if it doesn't exist
    if !path.exists() {
        debug!("[BETA] Creating default user config at {}", path.display());
        let _ = write_config(&path, &config).await;
    }

    Ok(CallToolResult::text(
        serde_json::to_string(&json!({
            "config": config,
            "path": path.to_str().unwrap_or(""),
            "note": "User preferences loaded. Project config may override these values."
        }))
        .unwrap(),
    ))
}

async fn handle_save_user_config(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let key_path = match require_str(args, "key_path") {
        Ok(v) => v.to_string(),
        Err(e) => return Ok(e),
    };
    let value = args["value"].clone();
    if value.is_null() {
        return Ok(CallToolResult::error_kind(
            ToolErrorKind::InvalidArgument {
                field: "value".to_string(),
                reason: "is required".to_string(),
            },
            "Missing required argument: 'value'",
        ));
    }

    let path = user_config_path();
    info!(key_path = %key_path, "[BETA] Saving user config");
    let mut config = read_config(&path, default_user_config()).await;
    set_dot_path(&mut config, &key_path, value.clone())?;
    write_config(&path, &config).await?;

    Ok(CallToolResult::text(
        serde_json::to_string(&json!({
            "updated": key_path,
            "value": value,
            "config": config
        }))
        .unwrap(),
    ))
}

async fn handle_load_project_config(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let project_dir = resolve_project_dir(args, ctx)?;
    let path = project_config_path(&project_dir);
    info!(path = %path.display(), "[BETA] Loading project config");
    let config = read_config(&path, default_project_config()).await;

    Ok(CallToolResult::text(
        serde_json::to_string(&json!({
            "config": config,
            "project_dir": project_dir.to_str().unwrap_or(""),
            "path": path.to_str().unwrap_or("")
        }))
        .unwrap(),
    ))
}

async fn handle_save_project_config(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let project_dir = resolve_project_dir(args, ctx)?;
    let key_path = match require_str(args, "key_path") {
        Ok(v) => v.to_string(),
        Err(e) => return Ok(e),
    };
    let value = args["value"].clone();
    if value.is_null() {
        return Ok(CallToolResult::error_kind(
            ToolErrorKind::InvalidArgument {
                field: "value".to_string(),
                reason: "is required".to_string(),
            },
            "Missing required argument: 'value'",
        ));
    }

    let path = project_config_path(&project_dir);
    let mut config = read_config(&path, default_project_config()).await;
    set_dot_path(&mut config, &key_path, value.clone())?;
    write_config(&path, &config).await?;

    Ok(CallToolResult::text(
        serde_json::to_string(&json!({
            "updated": key_path,
            "value": value,
            "project_dir": project_dir.to_str().unwrap_or("")
        }))
        .unwrap(),
    ))
}

async fn handle_get_effective_config(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let user_config = read_config(&user_config_path(), default_user_config()).await;

    let project_config = if let Ok(project_dir) = resolve_project_dir(args, ctx) {
        let path = project_config_path(&project_dir);
        read_config(&path, default_project_config()).await
    } else {
        default_project_config()
    };

    let effective = deep_merge(&user_config, &project_config);

    Ok(CallToolResult::text(
        serde_json::to_string(&json!({
            "effective_config": effective,
            "note": "Merged user defaults + project overrides. Use these values for all design decisions."
        }))
        .unwrap(),
    ))
}

async fn handle_add_design_rule(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let rule = match require_str(args, "rule") {
        Ok(v) => v.to_string(),
        Err(e) => return Ok(e),
    };
    let scope = args["scope"].as_str().unwrap_or("project");

    if scope == "user" {
        let path = user_config_path();
        let mut config = read_config(&path, default_user_config()).await;
        let rules = config["design_rules"].as_array_mut();
        if let Some(rules) = rules {
            rules.push(json!(rule));
        } else {
            config["design_rules"] = json!([rule]);
        }
        write_config(&path, &config).await?;
    } else {
        let project_dir = resolve_project_dir(args, ctx)?;
        let path = project_config_path(&project_dir);
        let mut config = read_config(&path, default_project_config()).await;
        let rules = config["design_rules"].as_array_mut();
        if let Some(rules) = rules {
            rules.push(json!(rule));
        } else {
            config["design_rules"] = json!([rule]);
        }
        write_config(&path, &config).await?;
    }

    Ok(CallToolResult::text(
        serde_json::to_string(&json!({
            "added_rule": rule,
            "scope": scope
        }))
        .unwrap(),
    ))
}

async fn handle_list_design_rules(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let user_config = read_config(&user_config_path(), default_user_config()).await;
    let user_rules: Vec<String> = user_config["design_rules"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let project_rules: Vec<String> = if let Ok(project_dir) = resolve_project_dir(args, ctx) {
        let path = project_config_path(&project_dir);
        let config = read_config(&path, default_project_config()).await;
        config["design_rules"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    Ok(CallToolResult::text(
        serde_json::to_string(&json!({
            "user_rules": user_rules,
            "project_rules": project_rules,
            "total": user_rules.len() + project_rules.len()
        }))
        .unwrap(),
    ))
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn resolve_project_dir(args: &serde_json::Value, ctx: &ToolContext) -> anyhow::Result<PathBuf> {
    if let Some(dir) = args["project_dir"].as_str() {
        return Ok(PathBuf::from(dir));
    }
    if let Some(ref dir) = ctx.config.project_dir {
        return Ok(dir.clone());
    }
    anyhow::bail!("No project directory specified. Pass 'project_dir' or configure a default.")
}

#[cfg(test)]
mod dot_path_and_merge_tests {
    use super::*;

    #[test]
    fn deep_merge_overlays_nested_object_keys() {
        let base = json!({
            "fab_constraints": { "fab_house": "JLCPCB", "layer_count": 2 },
            "design_rules": []
        });
        let overlay = json!({
            "fab_constraints": { "layer_count": 4 }
        });

        let merged = deep_merge(&base, &overlay);

        assert_eq!(merged["fab_constraints"]["fab_house"], "JLCPCB");
        assert_eq!(merged["fab_constraints"]["layer_count"], 4);
        assert_eq!(merged["design_rules"], json!([]));
    }

    #[test]
    fn deep_merge_null_overlay_value_keeps_base() {
        let base = json!({ "fab_house": "JLCPCB" });
        let overlay = json!({ "fab_house": null });

        let merged = deep_merge(&base, &overlay);

        assert_eq!(merged["fab_house"], "JLCPCB");
    }

    #[test]
    fn set_dot_path_sets_top_level_key() {
        let mut config = json!({});
        set_dot_path(&mut config, "fab_house", json!("JLCPCB")).expect("should succeed");
        assert_eq!(config["fab_house"], "JLCPCB");
    }

    #[test]
    fn set_dot_path_creates_missing_intermediate_objects() {
        let mut config = json!({});
        set_dot_path(&mut config, "fab_constraints.fab_house", json!("JLCPCB"))
            .expect("should succeed");
        assert_eq!(config["fab_constraints"]["fab_house"], "JLCPCB");
    }

    #[test]
    fn set_dot_path_overwrites_existing_nested_value() {
        let mut config = json!({ "fab_constraints": { "fab_house": "PCBWay" } });
        set_dot_path(&mut config, "fab_constraints.fab_house", json!("JLCPCB"))
            .expect("should succeed");
        assert_eq!(config["fab_constraints"]["fab_house"], "JLCPCB");
    }

    #[test]
    fn set_dot_path_errors_instead_of_panicking_on_non_object_root() {
        // Regression test: a corrupted config file that parses as valid JSON
        // but isn't a `{...}` object used to make this function panic via
        // `.unwrap()` on a failed `get_mut`, crashing the whole server.
        let mut config = json!(null);
        let result = set_dot_path(&mut config, "fab_constraints.fab_house", json!("JLCPCB"));
        assert!(result.is_err());
    }

    #[test]
    fn set_dot_path_replaces_scalar_intermediate_segment_with_object() {
        // "fab_constraints" already holds a string, not an object. The parent
        // (root) is still an object, so it's free to replace that key with a
        // fresh nested object rather than erroring — this matches the
        // function's pre-existing "create if needed" behavior.
        let mut config = json!({ "fab_constraints": "JLCPCB" });
        set_dot_path(&mut config, "fab_constraints.fab_house", json!("PCBWay"))
            .expect("should succeed by replacing the scalar with an object");
        assert_eq!(config["fab_constraints"]["fab_house"], "PCBWay");
    }
}
