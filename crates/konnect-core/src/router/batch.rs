//! Working out which directories a batch is about to touch.
//!
//! `kicad_invoke` protects a batch by snapshotting before it runs, which means
//! it has to know *what* to snapshot before any tool has executed. Nothing in
//! the MCP request declares that: the information is scattered through each
//! call's arguments as `schematic`, `board`, `path`, `output_dir` and friends.
//!
//! So it is inferred, from the two things that are reliable — a filename with a
//! KiCAD suffix, and a small set of argument names that conventionally hold a
//! directory. Inference that misses is safe in one direction only, and the
//! direction matters: a root we fail to find means no rollback for files under
//! it, so `kicad_invoke` reports the roots it used rather than leaving the
//! caller to assume coverage. A root we find wrongly costs a directory read.

use kam_state::TRACKED_EXTENSIONS;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Argument names that conventionally carry a directory rather than a file.
/// `path` is included because `create_project(path, name)` — the one call that
/// makes a whole project appear — puts the parent directory there.
const DIR_KEYS: &[&str] = &[
    "path",
    "dir",
    "directory",
    "project_dir",
    "project_path",
    "output_dir",
    "output_path",
    "workdir",
];

/// How deep to look inside a call's arguments. Batch payloads nest components
/// and pin lists a few levels down; nothing legitimate goes deeper.
const MAX_ARG_DEPTH: usize = 6;

/// More roots than this and the batch is not a coherent unit of work any more.
const MAX_ROOTS: usize = 8;

/// Directories a batch of `{tool, args}` calls may write to.
///
/// `explicit` is the caller's own `documents` argument, and wins in the sense
/// that it is always included — it exists for the case where the inference
/// cannot see a path, such as a tool that resolves a project from config.
pub fn discover_roots(calls: &[Value], explicit: Option<&Value>) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();

    if let Some(Value::Array(items)) = explicit {
        for item in items {
            if let Some(s) = item.as_str() {
                push_root(&mut roots, root_for(Path::new(s)));
            }
        }
    }

    for call in calls {
        if let Some(args) = call.get("args") {
            walk(args, None, 0, &mut roots);
        }
    }

    roots.truncate(MAX_ROOTS);
    roots
}

fn walk(value: &Value, key: Option<&str>, depth: usize, roots: &mut Vec<PathBuf>) {
    if depth > MAX_ARG_DEPTH {
        return;
    }
    match value {
        Value::String(s) => {
            if let Some(root) = root_from_string(s, key) {
                push_root(roots, Some(root));
            }
        }
        Value::Array(items) => {
            for item in items {
                walk(item, key, depth + 1, roots);
            }
        }
        Value::Object(map) => {
            for (k, v) in map {
                walk(v, Some(k.as_str()), depth + 1, roots);
            }
        }
        _ => {}
    }
}

fn root_from_string(s: &str, key: Option<&str>) -> Option<PathBuf> {
    if s.trim().is_empty() {
        return None;
    }
    let path = Path::new(s);

    // A KiCAD suffix is self-describing: it is a document wherever it appears,
    // under whichever argument name.
    if has_tracked_extension(path) {
        return path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(std::path::Path::to_path_buf);
    }

    // Otherwise only conventional directory arguments are trusted, and only
    // when they name something that really is a directory. A value like "GND"
    // under `path` would otherwise turn a net name into a snapshot root.
    let key = key?;
    if !DIR_KEYS.contains(&key) {
        return None;
    }
    if path.is_dir() {
        return Some(path.to_path_buf());
    }
    None
}

/// Root for an explicitly named document or directory.
fn root_for(path: &Path) -> Option<PathBuf> {
    if path.is_dir() {
        return Some(path.to_path_buf());
    }
    path.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(std::path::Path::to_path_buf)
}

fn push_root(roots: &mut Vec<PathBuf>, candidate: Option<PathBuf>) {
    let Some(candidate) = candidate else {
        return;
    };
    if roots.len() >= MAX_ROOTS || roots.contains(&candidate) {
        return;
    }
    roots.push(candidate);
}

fn has_tracked_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| TRACKED_EXTENSIONS.contains(&ext))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_schematic_argument_yields_its_directory() {
        let calls = vec![json!({
            "tool": "add_schematic_component",
            "args": { "schematic": "C:/work/divider/divider.kicad_sch", "reference": "R1" }
        })];
        assert_eq!(
            discover_roots(&calls, None),
            vec![PathBuf::from("C:/work/divider")]
        );
    }

    #[test]
    fn an_absolute_board_argument_yields_its_project_directory() {
        let calls = vec![json!({
            "tool": "place_footprint",
            "args": { "board": "C:/work/Test/Test.kicad_pcb" }
        })];
        assert_eq!(
            discover_roots(&calls, None),
            vec![PathBuf::from("C:/work/Test")]
        );
    }

    #[test]
    fn an_absolute_project_file_is_recognised_under_any_argument_name() {
        let calls = vec![json!({
            "tool": "get_project_info",
            "args": { "source": "C:/work/Test/Test.kicad_pro" }
        })];
        assert_eq!(
            discover_roots(&calls, None),
            vec![PathBuf::from("C:/work/Test")]
        );
    }

    #[test]
    fn a_board_and_a_schematic_in_one_batch_share_one_root() {
        let calls = vec![
            json!({"tool": "a", "args": {"schematic": "/p/x.kicad_sch"}}),
            json!({"tool": "b", "args": {"board": "/p/x.kicad_pcb"}}),
        ];
        assert_eq!(discover_roots(&calls, None), vec![PathBuf::from("/p")]);
    }

    #[test]
    fn nested_arguments_are_searched() {
        let calls = vec![json!({
            "tool": "batch_place_components",
            "args": {
                "components": [{"lib_id": "Device:R", "sheet": "/p/sub/child.kicad_sch"}]
            }
        })];
        assert_eq!(discover_roots(&calls, None), vec![PathBuf::from("/p/sub")]);
    }

    #[test]
    fn a_non_path_value_under_a_directory_key_is_ignored() {
        // `path: "GND"` is not a root, and treating it as one would snapshot
        // the process working directory.
        let calls = vec![json!({"tool": "x", "args": {"path": "GND"}})];
        assert!(discover_roots(&calls, None).is_empty());
    }

    #[test]
    fn an_existing_directory_under_a_directory_key_is_a_root() {
        let dir = tempfile::tempdir().unwrap();
        let calls = vec![json!({
            "tool": "create_project",
            "args": { "path": dir.path().to_string_lossy(), "name": "divider" }
        })];
        assert_eq!(discover_roots(&calls, None), vec![dir.path().to_path_buf()]);
    }

    #[test]
    fn explicit_documents_are_always_included() {
        let calls = vec![json!({"tool": "x", "args": {}})];
        let explicit = json!(["/elsewhere/board.kicad_pcb"]);
        assert_eq!(
            discover_roots(&calls, Some(&explicit)),
            vec![PathBuf::from("/elsewhere")]
        );
    }

    #[test]
    fn a_pathless_ipc_call_requires_explicit_documents_for_protection() {
        let calls = vec![json!({"tool": "save_project", "args": {}})];
        assert!(discover_roots(&calls, None).is_empty());

        let explicit = json!(["C:/work/Test/Test.kicad_sch"]);
        assert_eq!(
            discover_roots(&calls, Some(&explicit)),
            vec![PathBuf::from("C:/work/Test")]
        );
    }

    #[test]
    fn generated_output_paths_are_not_roots() {
        // A gerber directory holds nothing the snapshot tracks; making it a
        // root only costs a directory walk, but a .gbr file must never be one.
        let calls =
            vec![json!({"tool": "export_gerbers", "args": {"output": "/p/gerbers/top.gbr"}})];
        assert!(discover_roots(&calls, None).is_empty());
    }

    #[test]
    fn the_root_count_is_bounded() {
        let calls: Vec<Value> = (0..20)
            .map(|i| json!({"tool": "x", "args": {"schematic": format!("/p{i}/a.kicad_sch")}}))
            .collect();
        assert_eq!(discover_roots(&calls, None).len(), MAX_ROOTS);
    }

    #[test]
    fn a_bare_filename_has_no_root_rather_than_the_working_directory() {
        let calls = vec![json!({"tool": "x", "args": {"schematic": "a.kicad_sch"}})];
        assert!(discover_roots(&calls, None).is_empty());
    }
}
