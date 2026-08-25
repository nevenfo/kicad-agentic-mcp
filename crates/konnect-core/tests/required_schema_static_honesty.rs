//! P.6.9.21: `required_schema_honesty.rs` (P.6.9.16) calls every handler with
//! `{}`. Because `{}` is missing *every* required key at once, it only ever
//! proves the first one a handler happens to check — a schema listing
//! `["board", "uuid"]` whose handler only ever reads `uuid` passes that pass
//! exactly like one that reads both. `query_traces` and `get_nets_list`
//! (fixed by P.6.9.16) were only caught because they happened to check no
//! *other* required key first — a schema with an unread key listed after a
//! checked one would sail through `{}` undetected. This pass found five more
//! of exactly that shape in `pcb_routing.rs`.
//!
//! P.6.9.22 measured what those five actually were: not five schemas that
//! forgot to read `board`, but eight handlers (`route_trace`,
//! `route_pad_to_pad`, `add_via`, `delete_trace`, `query_traces`,
//! `get_nets_list`, `modify_trace`, `route_differential_pair`) routing IPC
//! calls through a `pcb_routing.rs`-local `ipc!` that never resolved `board`
//! or confirmed KiCAD held it — so with the wrong board merely open behind
//! the target, six of those eight would have written copper onto it
//! (`konnect_ipc::client::find_open_board`'s doc comment records the live
//! symptom). `pcb_routing.rs` now shares `pcb_components.rs`'s guarded
//! `ipc!`, from `ipc_boundary::guarded_ipc`, imported under alias so both
//! files still write `ipc!(ctx, args, |c| ..)` — see "Indirection" below for
//! how this pass follows that alias across files to find the shared body.
//! `query_traces` and `get_nets_list` had `board` removed from their schemas
//! entirely by P.6.9.16, on the reasoning that they read the open session,
//! not a file; P.6.9.22 restores `board` to both, `required` included, now
//! that the guarded macro actually honors it — the P.6.9.16 comment that used
//! to sit on those two schemas described the bug as the intent, and is
//! removed with the schemas it was attached to.
//!
//! This pass does not call anything. It reads this repository's own source: for
//! every tool whose schema has a non-empty top-level `required` list, it finds
//! the `tool!(...)` (or hand-built `ToolDef { .. }`) call that registers it,
//! the `handle_*` function it names, and checks — as plain text, the same way
//! `capability::coverage` already scans its own test sources — that every
//! required key is actually read somewhere in that function's body, under one
//! of the forms this codebase is measured to use: `args["key"]`,
//! `args.get("key")`, and the `require_*`/`opt_*`/`get_path` helper family
//! (`require_str`, `opt_str`, `require_f64`, `opt_f64`, `require_array`,
//! `require_u64`, `get_path`), all called as `helper(args, "key")`.
//!
//! A required key absent from the handler body under every one of those forms
//! is caught here regardless of how many other keys precede it in `required`
//! — the `{}` dynamic pass cannot see past the first.
//!
//! ## Indirection: one level, followed generically
//!
//! A handler that delegates reads its keys somewhere other than its own body,
//! and this pass follows one level of that rather than keeping a hand-picked
//! exception list — three shapes are measured to matter:
//!
//! * a call to another function defined anywhere under
//!   `crates/konnect-core/src/tools` or `crates/konnect-core/src/router/meta_tools.rs`
//!   (`name(...)`);
//! * a call to a `macro_rules!` invoked in the handler's own file
//!   (`name!(...)`) — either defined there, or imported under an alias from
//!   another scanned file (`use path::real_name as alias;`), the shape
//!   `pcb_components.rs` and `pcb_routing.rs` both use for `ipc!`, aliasing
//!   `ipc_boundary::guarded_ipc` (P.6.9.22: the two files used to define
//!   independent `ipc!` bodies, one of them unguarded — see this test's
//!   module-level history above). Fourteen handlers across the two files
//!   (`add_via`, `delete_component`, `delete_trace`, `edit_component`,
//!   `find_component`, `get_component_list`, `get_nets_list`, `modify_trace`,
//!   `move_component`, `query_traces`, `rotate_component`,
//!   `route_differential_pair`, `route_pad_to_pad`, `route_trace`) are only
//!   honest about `board` through that shared macro;
//! * a `require_*`/`opt_*` helper called with a variable key inside a loop
//!   over a literal key array — `sch_buses.rs`'s `handle_add_bus` reads `x1`,
//!   `y1`, `x2`, `y2` as `for (slot, key) in coords.iter_mut().zip(["x1",
//!   "y1", "x2", "y2"]) { require_f64(args, key) }`, so no call site ever
//!   names `"x1"` next to `args`. Credited when the body calls a helper with
//!   a non-literal second argument *and* the required key appears quoted
//!   somewhere in the same body — the measured shape, not a general
//!   "assume it's fine" escape hatch.
//!
//! No second level for the function/macro case: a key still missing after
//! that is reported, not chased further — see the module-level report this
//! test prints for how often each of these three mattered.
//!
//! ## What this cannot see
//!
//! * Whether a key that *is* read actually reaches every IPC call the
//!   handler makes. `route_pad_to_pad` (`pcb_routing.rs`) is the measured
//!   case: it reads `board` directly (`get_path(args, "board")?`, to look up
//!   pad positions in the file) before P.6.9.22, so this pass never flagged
//!   it — but it then routed over IPC through the file's unguarded `ipc!`
//!   without that value ever reaching KiCAD, so it could read pad A's
//!   position from the named board and route onto whichever board KiCAD had
//!   open, unrelated to it. A key *read* is not a key *honored*; this pass
//!   only ever checks the former, as plain text presence, and cannot see
//!   what a handler's IPC calls do with the value afterward.
//! * A key read through a variable holding `args` under a different name
//!   outside the three shapes above, or reconstructed (e.g. built from
//!   `args["a"]` and `args["b"]` combined) — none of this codebase's handlers
//!   do that for a `required` key, per the greps this test's own doc comment
//!   was written against, but a future one that did would false-positive
//!   here. That is the same risk `capability/coverage.rs`'s lexical scan
//!   already accepts for its own purpose.
//! * A key read only inside a second level of delegation. Measured count of
//!   how often one level was not enough is in the printed report; if that
//!   count is ever non-zero this test's failures must be read against that
//!   number, not assumed to be real lies.
//! * D133: like `required_schema_honesty.rs`, this file must never write a
//!   tool name as a quoted literal — `capability/coverage.rs`'s `mentions`
//!   would then credit every one of them as `SUPPORTED` in
//!   `docs/capability-matrix.md`, which is never regenerated to match. All
//!   tool names and keys here come from the registry and from parsing other
//!   files' text at runtime, never as literals in this source.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates| crates.parent())
        .expect("crates/konnect-core has a workspace root two levels up")
        .to_path_buf()
}

/// Files this pass reads as source, relative to the workspace root: every
/// toolset module (where `tool!` registers a domain tool) plus the meta-tools
/// module (which registers its own handlers through a different macro,
/// `define_meta_tools!`, checked separately below).
fn source_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let tools_dir = root.join("crates/konnect-core/src/tools");
    let entries = std::fs::read_dir(&tools_dir)
        .unwrap_or_else(|e| panic!("{} is readable: {e}", tools_dir.display()));
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
    files.sort();
    files.push(root.join("crates/konnect-core/src/router/meta_tools.rs"));
    files
}

/// Extract a balanced-brace body starting at the first `{` at or after
/// `from`, aware of string/char literals and `//` / `/* */` comments so a
/// stray brace inside a JSON literal or an error message cannot desync the
/// count. Returns the text strictly between the outer braces.
fn brace_body(text: &str, from: usize) -> Option<&str> {
    let bytes = text.as_bytes();
    let mut i = from;
    while i < bytes.len() && bytes[i] != b'{' {
        i += 1;
    }
    if i >= bytes.len() {
        return None;
    }
    let start = i + 1;
    let mut depth = 1i32;
    let mut j = start;
    while j < bytes.len() {
        match bytes[j] {
            b'"' => {
                j += 1;
                while j < bytes.len() && bytes[j] != b'"' {
                    if bytes[j] == b'\\' {
                        j += 1;
                    }
                    j += 1;
                }
            }
            b'\'' => {
                // Char literal or lifetime; a char literal is at most
                // `'\\''`/`'x'` — skip up to the next `'` within a few bytes,
                // otherwise treat as a lifetime (no skip needed).
                if j + 2 < bytes.len() && bytes[j + 1] == b'\\' {
                    j += 3; // '\x' plus the closing quote handled below
                } else if j + 2 < bytes.len() && bytes[j + 2] == b'\'' {
                    j += 2;
                }
            }
            b'/' if j + 1 < bytes.len() && bytes[j + 1] == b'/' => {
                while j < bytes.len() && bytes[j] != b'\n' {
                    j += 1;
                }
                continue;
            }
            b'/' if j + 1 < bytes.len() && bytes[j + 1] == b'*' => {
                j += 2;
                while j + 1 < bytes.len() && !(bytes[j] == b'*' && bytes[j + 1] == b'/') {
                    j += 1;
                }
                j += 1;
            }
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return std::str::from_utf8(&bytes[start..j]).ok();
                }
            }
            _ => {}
        }
        j += 1;
    }
    None
}

fn is_fn_decl(trimmed: &str) -> Option<&str> {
    let mut rest = trimmed;
    loop {
        let stripped = [
            "pub(crate) ",
            "pub ",
            "async ",
            "unsafe ",
            "const ",
            "extern ",
        ]
        .iter()
        .find_map(|prefix| rest.strip_prefix(prefix));
        match stripped {
            Some(next) => rest = next,
            None => break,
        }
    }
    let rest = rest.strip_prefix("fn ")?;
    let end = rest.find(['(', '<'])?;
    Some(rest[..end].trim())
}

/// Every function body in `text`, keyed by name. A name defined more than
/// once (there are a few short, generically-named helpers) accumulates every
/// body under that name — an over-approximation that only ever helps find a
/// key that is really read, never hides one that is not.
fn collect_fn_bodies(text: &str, into: &mut HashMap<String, String>) {
    let mut pos = 0usize;
    while let Some(nl) = text[pos..].find('\n') {
        let line = &text[pos..pos + nl];
        let trimmed = line.trim_start();
        if !trimmed.starts_with("//") {
            if let Some(name) = is_fn_decl(trimmed) {
                if let Some(body) = brace_body(text, pos) {
                    into.entry(name.to_string()).or_default().push_str(body);
                }
            }
        }
        pos += nl + 1;
    }
}

/// `tool!("name", desc, schema, |args, ctx| async move { handle_x(args, ctx).await })`
/// and the one hand-built exception, `ToolDef { name: "apply_plan", ..,
/// handler: Arc::new(|args, ctx| { .. Box::pin(async move { handle_apply_plan(..) }) }) }`
/// (`plan.rs`, by its own comment: "Built by hand rather than through `tool!`:
/// the macro hands the handler a `&ToolContext`, and this one dispatches other
/// tools, whose handlers take the `Arc` by value.") — both forms carry the
/// tool's name as a `"literal"` and its handler as `handle_<ident>` reached
/// through `async move { handle_`, so one scan over that marker finds both.
fn tool_handler_pairs(text: &str) -> Vec<(String, String)> {
    let mut names = Vec::new();
    let mut idx = 0;
    while let Some(p) = text[idx..].find("tool!(") {
        let start = idx + p + "tool!(".len();
        if let Some((name, after)) = next_quoted(text, start) {
            names.push((start, name));
            idx = after;
        } else {
            idx = start;
        }
    }
    idx = 0;
    while let Some(p) = text[idx..].find("ToolDef {") {
        let abs = idx + p;
        let before = text[..abs].trim_end();
        // Excludes `pub struct ToolDef {`, `impl .. for ToolDef {`, and the
        // macro's own `$crate::tools::ToolDef {` — none register a tool.
        if !(before.ends_with("struct") || before.ends_with("for") || before.ends_with("::")) {
            if let Some(name_at) = text[abs..].find("name: \"") {
                let (name, _) = next_quoted(text, abs + name_at + "name: ".len())
                    .unwrap_or_else(|| (String::new(), abs));
                if !name.is_empty() {
                    names.push((abs, name));
                }
            }
        }
        idx = abs + "ToolDef {".len();
    }
    names.sort_by_key(|(pos, _)| *pos);

    let mut handlers = Vec::new();
    let mut idx = 0;
    while let Some(p) = text[idx..].find("async move { handle_") {
        let start = idx + p + "async move { handle_".len();
        let rest = &text[start..];
        if let Some(paren) = rest.find('(') {
            handlers.push((start, format!("handle_{}", &rest[..paren])));
            idx = start + paren;
        } else {
            idx = start;
        }
    }

    // Pair each name with the next handler that follows it in the file — the
    // registration order both forms are written in.
    let mut out = Vec::new();
    let mut hi = 0;
    for (pos, name) in names {
        while hi < handlers.len() && handlers[hi].0 < pos {
            hi += 1;
        }
        if hi < handlers.len() {
            out.push((name, handlers[hi].1.clone()));
            hi += 1;
        }
    }
    out
}

fn next_quoted(text: &str, from: usize) -> Option<(String, usize)> {
    let q1 = text[from..].find('"')? + from + 1;
    let q2 = text[q1..].find('"')? + q1;
    Some((text[q1..q2].to_string(), q2))
}

/// `define_meta_tools! { args, ctx; "name" => handle_x(args, ctx).await, .. }`
/// — a flat list, unlike the toolset macro's blocks, so a direct `"name" =>
/// handle_` scan is enough.
fn meta_tool_handler_pairs(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut idx = 0;
    while let Some(p) = text[idx..].find("\" => handle_") {
        let name_end = idx + p + 1;
        // Walk back to the opening quote of the name.
        let name_start = text[..name_end - 1].rfind('"').map(|i| i + 1);
        let handler_start = name_end + " => handle_".len();
        if let (Some(ns), Some(paren)) = (name_start, text[handler_start..].find('(')) {
            let name = text[ns..name_end - 1].to_string();
            let handler = format!("handle_{}", &text[handler_start..handler_start + paren]);
            out.push((name, handler));
            idx = handler_start + paren;
        } else {
            idx = name_end;
        }
    }
    out
}

const HELPERS: &[&str] = &[
    "require_str",
    "opt_str",
    "require_f64",
    "opt_f64",
    "require_array",
    "require_u64",
    "get_path",
];

/// Every `key` used as `args["key"]`, `args.get("key")`, or one of the
/// `require_*`/`opt_*`/`get_path` helpers called as `helper(args, "key")`,
/// found by direct substring search against the two spacing variants this
/// codebase's `rustfmt` output actually uses (`helper(args, "key")` with a
/// space after the comma — measured as the only variant present; `helper(args,"key")`
/// checked too so a reformat cannot silently defeat this pass).
///
/// `$args` alongside `args`: a `macro_rules!` body (reached through
/// `local_calls`'s `name!(...)` case) names its parameter `$args` — the
/// `ipc!` macro in `pcb_components.rs` reads `board` as
/// `get_path($args, "board")?`, never as plain `args`, since it is never
/// substituted until the call site.
fn reads_key(body: &str, key: &str) -> bool {
    let forms = [
        format!("args[\"{key}\"]"),
        format!("args.get(\"{key}\")"),
        format!("$args[\"{key}\"]"),
        format!("$args.get(\"{key}\")"),
    ];
    if forms.iter().any(|f| body.contains(f.as_str())) {
        return true;
    }
    if HELPERS.iter().any(|helper| {
        body.contains(&format!("{helper}(args, \"{key}\")"))
            || body.contains(&format!("{helper}(args,\"{key}\")"))
            || body.contains(&format!("{helper}($args, \"{key}\")"))
            || body.contains(&format!("{helper}($args,\"{key}\")"))
    }) {
        return true;
    }
    // `sch_buses.rs::handle_add_bus` is the one measured case of a helper
    // called with a *variable* key inside a loop over a literal key array —
    // `for (slot, key) in coords.iter_mut().zip(["x1", "y1", "x2", "y2"])`
    // then `require_f64(args, key)`. A required key that appears quoted
    // anywhere in a body that also calls a helper with a non-literal second
    // argument is credited as read through that loop, rather than reported as
    // unread because no call site names it directly.
    has_dynamic_key_call(body) && body.contains(&format!("\"{key}\""))
}

/// Whether `body` calls a `require_*`/`opt_*`/`get_path` helper with a second
/// argument that is a bare identifier rather than a quoted key — the loop
/// pattern `reads_key`'s last branch accounts for. Measured across this
/// codebase (`rg -oP '(require_str|opt_str|require_f64|opt_f64|require_array
/// |require_u64|get_path)\(args,\s*[a-zA-Z_][a-zA-Z0-9_]*\)'`): five call
/// sites, none with a quote as the first character after the comma.
fn has_dynamic_key_call(body: &str) -> bool {
    HELPERS.iter().any(|helper| {
        let marker = format!("{helper}(args, ");
        body.match_indices(&marker).any(|(pos, _)| {
            body[pos + marker.len()..]
                .chars()
                .next()
                .is_some_and(|c| c != '"')
        })
    })
}

/// Every `macro_rules! name { .. }` in `text`, keyed by the name it is
/// *defined* under — collected globally (across every scanned file) so an
/// alias importing that macro elsewhere (see [`collect_macro_aliases`]) can
/// find its body regardless of which file defines it.
fn collect_macro_bodies(text: &str, into: &mut HashMap<String, String>) {
    let marker = "macro_rules! ";
    let mut idx = 0;
    while let Some(p) = text[idx..].find(marker) {
        let start = idx + p + marker.len();
        let rest = &text[start..];
        let Some(brace_or_space) = rest.find(['{', ' ']) else {
            break;
        };
        let name = rest[..brace_or_space].trim().to_string();
        if let Some(body) = brace_body(text, start) {
            into.entry(name).or_default().push_str(body);
        }
        idx = start;
    }
}

/// `use path::real_name as alias;` for every `real_name` this pass already
/// knows as a macro (from [`collect_macro_bodies`]'s global pass) — the shape
/// `pcb_components.rs` and `pcb_routing.rs` both use for `ipc!`
/// (`use crate::tools::ipc_boundary::guarded_ipc as ipc;`, P.6.9.22): neither
/// file defines `ipc!` itself any more, so a handler's `ipc!(..)` call site
/// only resolves to a body at all through this alias. A plain textual search
/// for `"{real_name} as "` is enough — this codebase's only two `use ... as`
/// macro imports are both this one, and a false match inside an unrelated
/// `use` would still only ever point at a real macro body, never invent one.
fn collect_macro_aliases(
    text: &str,
    all_macros: &HashMap<String, String>,
    into: &mut HashMap<String, String>,
) {
    for (name, body) in all_macros {
        let marker = format!("{name} as ");
        let mut idx = 0;
        while let Some(p) = text[idx..].find(marker.as_str()) {
            let start = idx + p + marker.len();
            let bytes = text.as_bytes();
            let mut end = start;
            while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
                end += 1;
            }
            if end > start {
                into.entry(text[start..end].to_string())
                    .or_default()
                    .push_str(body);
            }
            idx = end.max(start + 1);
        }
    }
}

/// Identifiers called as `name(` inside `body` that are keys of `all_fns`, or
/// invoked as `name!(` that are keys of `local_macros` — the one level of
/// indirection this pass follows, functions everywhere in the scanned source
/// and macros named (whether defined or imported under an alias) in the
/// handler's own file.
fn local_calls<'a>(
    body: &str,
    all_fns: &'a HashMap<String, String>,
    local_macros: &'a HashMap<String, String>,
) -> Vec<(&'a str, &'a str)> {
    let mut out = Vec::new();
    let bytes = body.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            if i < bytes.len() && bytes[i] == b'!' && i + 1 < bytes.len() && bytes[i + 1] == b'(' {
                if let Some((name, macro_body)) = local_macros.get_key_value(&body[start..i]) {
                    if macro_body.as_str() != body {
                        out.push((name.as_str(), macro_body.as_str()));
                    }
                }
                continue;
            }
            if i < bytes.len() && bytes[i] == b'(' {
                if let Some((name, fn_body)) = all_fns.get_key_value(&body[start..i]) {
                    if fn_body.as_str() != body {
                        out.push((name.as_str(), fn_body.as_str()));
                    }
                }
            }
        } else {
            i += 1;
        }
    }
    out
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

#[test]
fn every_required_key_is_read_by_its_handler() {
    let root = workspace_root();
    let files = source_files(&root);

    let texts: Vec<(PathBuf, String)> = files
        .iter()
        .map(|file| {
            let text = std::fs::read_to_string(file)
                .unwrap_or_else(|e| panic!("{} is readable: {e}", file.display()));
            (file.clone(), text)
        })
        .collect();

    let mut all_fns: HashMap<String, String> = HashMap::new();
    // Global, across every scanned file — see `collect_macro_aliases` for why
    // a macro's own definition site no longer has to be the handler's file.
    let mut all_macros: HashMap<String, String> = HashMap::new();
    for (_, text) in &texts {
        collect_fn_bodies(text, &mut all_fns);
        collect_macro_bodies(text, &mut all_macros);
    }

    let mut tool_handler: HashMap<String, String> = HashMap::new();
    let mut handler_file: HashMap<String, PathBuf> = HashMap::new();
    let mut macros_by_file: HashMap<PathBuf, HashMap<String, String>> = HashMap::new();
    for (file, text) in &texts {
        let mut file_macros = HashMap::new();
        collect_macro_bodies(text, &mut file_macros);
        collect_macro_aliases(text, &all_macros, &mut file_macros);
        macros_by_file.insert(file.clone(), file_macros);
        for (name, handler) in tool_handler_pairs(text) {
            handler_file.insert(handler.clone(), file.clone());
            tool_handler.insert(name, handler);
        }
        if file.ends_with("meta_tools.rs") {
            for (name, handler) in meta_tool_handler_pairs(text) {
                handler_file.insert(handler.clone(), file.clone());
                tool_handler.insert(name, handler);
            }
        }
    }
    let empty_macros: HashMap<String, String> = HashMap::new();

    // Every registered tool and meta-tool, with its schema — same source
    // `required_schema_honesty.rs` uses, without calling anything.
    let mut entries: Vec<(String, Value)> = Vec::new();
    for meta in konnect_core::router::registry::ALL_TOOLSETS {
        let tools = konnect_core::router::registry::tools_for(meta.name)
            .unwrap_or_else(|| panic!("toolset '{}' is listed but has no tools", meta.name));
        for tool in tools {
            entries.push((tool.name.to_string(), tool.input_schema.clone()));
        }
    }
    for name in konnect_core::router::meta_tools::META_TOOL_NAMES {
        let schema = konnect_core::router::meta_tools::meta_tool_schema(name)
            .unwrap_or_else(|| panic!("meta-tool '{name}' has no schema"));
        entries.push((name.to_string(), schema.clone()));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries.dedup_by(|a, b| a.0 == b.0);
    assert!(
        entries.len() > 190,
        "the registry shrank unexpectedly: {} tools",
        entries.len()
    );

    let mut unmapped: Vec<String> = Vec::new();
    let mut no_handler_body: Vec<String> = Vec::new();
    let mut checked_tools = 0usize;
    let mut checked_keys = 0usize;
    let mut indirection_used = 0usize;
    let mut liars: Vec<String> = Vec::new();

    for (name, schema) in &entries {
        let required = required_keys(schema);
        if required.is_empty() {
            continue;
        }
        let Some(handler) = tool_handler.get(name) else {
            unmapped.push(name.clone());
            continue;
        };
        let Some(body) = all_fns.get(handler) else {
            no_handler_body.push(format!("{name} -> {handler}"));
            continue;
        };
        checked_tools += 1;
        let local_macros = handler_file
            .get(handler)
            .and_then(|file| macros_by_file.get(file))
            .unwrap_or(&empty_macros);
        for key in &required {
            checked_keys += 1;
            if reads_key(body, key) {
                continue;
            }
            let mut found_indirect = false;
            for (callee_name, callee_body) in local_calls(body, &all_fns, local_macros) {
                if callee_name == handler {
                    continue;
                }
                if reads_key(callee_body, key) {
                    found_indirect = true;
                    break;
                }
            }
            if found_indirect {
                indirection_used += 1;
                continue;
            }
            liars.push(format!(
                "{name}: schema requires '{key}' but handler '{handler}' (and the functions it \
                 calls, one level deep) never reads it as args[\"{key}\"], args.get(\"{key}\"), \
                 or a require_*/opt_*/get_path helper on that key"
            ));
        }
    }

    println!(
        "static required-key pass: {} tools with a non-empty required list, {checked_keys} \
         required keys checked, {} resolved only via one-level indirection, {} liars, {} tools \
         with a required list this pass could not map to a handler, {} mapped handlers whose \
         body could not be found",
        checked_tools,
        indirection_used,
        liars.len(),
        unmapped.len(),
        no_handler_body.len()
    );

    assert!(
        unmapped.is_empty(),
        "these tools declare a non-empty required list but this pass could not find their \
         registration (tool!/ToolDef or define_meta_tools! entry) in source — fix the parser, \
         not the code: {unmapped:?}"
    );
    assert!(
        no_handler_body.is_empty(),
        "these tools were mapped to a handler function name this pass could not find a body \
         for — fix the parser, not the code: {no_handler_body:?}"
    );
    assert!(
        liars.is_empty(),
        "these schemas require a key their handler never reads:\n  {}",
        liars.join("\n  ")
    );
}

/// Balanced-paren span starting at the `(` this pass has already located at
/// or after `open`, string/char/comment-aware like [`brace_body`] (parens
/// inside a string or a `//`/`/* */` comment cannot desync the count).
/// Returns the text strictly between the outer parens — the full argument
/// list of a call, including any closure passed as one of its arguments.
fn paren_body(text: &str, open: usize) -> Option<&str> {
    let bytes = text.as_bytes();
    let mut i = open;
    while i < bytes.len() && bytes[i] != b'(' {
        i += 1;
    }
    if i >= bytes.len() {
        return None;
    }
    let start = i + 1;
    let mut depth = 1i32;
    let mut j = start;
    while j < bytes.len() {
        match bytes[j] {
            b'"' => {
                j += 1;
                while j < bytes.len() && bytes[j] != b'"' {
                    if bytes[j] == b'\\' {
                        j += 1;
                    }
                    j += 1;
                }
            }
            b'\'' => {
                if j + 2 < bytes.len() && bytes[j + 1] == b'\\' {
                    j += 3;
                } else if j + 2 < bytes.len() && bytes[j + 2] == b'\'' {
                    j += 2;
                }
            }
            b'/' if j + 1 < bytes.len() && bytes[j + 1] == b'/' => {
                while j < bytes.len() && bytes[j] != b'\n' {
                    j += 1;
                }
                continue;
            }
            b'/' if j + 1 < bytes.len() && bytes[j + 1] == b'*' => {
                j += 2;
                while j + 1 < bytes.len() && !(bytes[j] == b'*' && bytes[j + 1] == b'/') {
                    j += 1;
                }
                j += 1;
            }
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return std::str::from_utf8(&bytes[start..j]).ok();
                }
            }
            _ => {}
        }
        j += 1;
    }
    None
}

/// P.6.9.22 closed the blind spot the module docs' "What this cannot see"
/// names — `route_pad_to_pad` (`pcb_routing.rs`) read `board` directly, so
/// this pass's own key scan never flagged it, yet it routed over IPC through
/// that file's old, unguarded `ipc!` without that value ever reaching KiCAD.
/// `pcb_routing.rs` now shares `pcb_components.rs`'s board-guarded `ipc!`
/// (`ipc_boundary::guarded_ipc`), so nothing in it needs to call `with_ipc`
/// — the raw crossing point `guarded_ipc` itself wraps — directly any more.
///
/// P.6.9.23 measured that `pcb_routing.rs` was not the only file naming
/// `with_ipc(` outside `guarded_ipc`'s own definition: `pcb_board.rs` (five
/// sites), `pcb_export.rs` (one), and `pcb_components.rs` (three) all did.
/// Eight of those nine wrote to the board; `pcb_board.rs`'s
/// `handle_get_board_extents` only reads it. Six — five in `pcb_board.rs`,
/// one in `pcb_export.rs` (`handle_refill_zones`, whose own
/// `KiCadIpcClient::refill_zones` resolves the board via
/// `get_board_document`, KiCAD's *first* open document, exactly the P.6.9.22
/// shape) — had never checked the board at all: fixed by either an inline
/// `c.ensure_board_is_active(&requested_board)?` as the first statement of
/// the closure passed to `with_ipc` (`pcb_board.rs`, which also has a file
/// fallback `guarded_ipc` cannot preserve — it returns unconditionally on
/// any IPC failure, where these handlers must keep falling back to a file
/// edit when the failure is a transport that never reached KiCAD) or by
/// routing through `guarded_ipc` itself where there is no fallback to
/// preserve (`pcb_export.rs`). The other three, in `pcb_components.rs`
/// (`handle_place_array`, `handle_align_components`), already called
/// `ensure_board_is_active` inline before this pass existed; a third
/// (`handle_place_component`) never calls it at all, but its one IPC call is
/// `KiCadIpcClient::place_footprint`, which resolves the board itself via
/// `find_open_board` before writing — a different, already-adequate
/// mechanism this pass credits as its own named exception rather than by
/// weakening what it demands of every other call site.
///
/// So a bare `with_ipc(` count is not enough on its own (P.6.9.22's
/// `pcb_routing.rs` check demanded zero, which is still checked below as a
/// specific case of this rule): for every other `with_ipc(` in a tools
/// module, the balanced-paren argument list of that specific call —
/// [`paren_body`], covering the closure passed to it — must itself contain
/// either `ensure_board_is_active(` or, in `pcb_components.rs` only,
/// `place_footprint(`. `ipc_boundary.rs` is excluded outright: it is where
/// `with_ipc` and `guarded_ipc` are defined, the one legitimate site.
///
/// Measured before the P.6.9.23 fix: six violations, one each at
/// `pcb_board.rs:364,475,735,823,961` and `pcb_export.rs:629` (line numbers
/// as of that commit) — `pcb_board.rs calls \`with_ipc(\` without a board
/// check in its own argument list 5 time(s) …` and `pcb_export.rs … 1
/// time(s) …`. After the fix, zero.
#[test]
fn no_ipc_call_bypasses_the_guarded_macro_or_an_inline_board_check() {
    let root = workspace_root();
    let tools_dir = root.join("crates/konnect-core/src/tools");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&tools_dir)
        .unwrap_or_else(|e| panic!("{} is readable: {e}", tools_dir.display()))
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        // The one legitimate site: `with_ipc` and `guarded_ipc` are defined
        // here, so this file necessarily names `with_ipc(` in its own macro
        // body.
        .filter(|path| path.file_name().and_then(|n| n.to_str()) != Some("ipc_boundary.rs"))
        .collect();
    files.sort();

    let mut report = String::new();
    let mut total_violations = 0usize;
    for file in &files {
        let text = std::fs::read_to_string(file)
            .unwrap_or_else(|e| panic!("{} is readable: {e}", file.display()));
        let is_components = file.file_name().and_then(|n| n.to_str()) == Some("pcb_components.rs");

        let mut violations = Vec::new();
        let mut idx = 0;
        while let Some(p) = text[idx..].find("with_ipc(") {
            let match_start = idx + p;
            let open = match_start + "with_ipc".len();
            let span = paren_body(&text, open).unwrap_or_else(|| {
                panic!(
                    "{}: `with_ipc(` at byte {match_start} has no balanced closing paren",
                    file.display()
                )
            });
            let guarded = span.contains("ensure_board_is_active(")
                || (is_components && span.contains("place_footprint("));
            if !guarded {
                let line = text[..match_start].matches('\n').count() + 1;
                violations.push(line);
            }
            idx = open;
        }
        if !violations.is_empty() {
            total_violations += violations.len();
            report.push_str(&format!(
                "  {} calls `with_ipc(` without a board check in its own argument list {} \
                 time(s) — at line(s) {:?}\n",
                file.display(),
                violations.len(),
                violations
            ));
        }
    }

    assert_eq!(
        total_violations, 0,
        "every IPC crossing in a tools module must go through the board-guarded `ipc!` \
         (`ipc_boundary::guarded_ipc`), or — where that macro's unconditional error return would \
         break a deliberate file fallback, or a client method already resolves the board itself \
         (`KiCadIpcClient::place_footprint`, `pcb_components.rs` only) — call \
         `ensure_board_is_active` (or an equivalent board-resolving client method) inline before \
         any write, or a handler can silently land on whichever board KiCAD happens to have open \
         first (P.6.9.22, P.6.9.23):\n{report}"
    );
}
