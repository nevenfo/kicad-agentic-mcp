//! Library symbol resolution — loads symbol definitions from KiCAD's installed libraries.
//!
//! KiCAD 10 stores symbols in `.kicad_symdir` directories:
//! ```text
//! C:\KiCad\10.0\share\kicad\symbols\Device.kicad_symdir\R.kicad_sym
//! C:\KiCad\10.0\share\kicad\symbols\power.kicad_symdir\VCC.kicad_sym
//! ```
//!
//! This module resolves a `lib_id` like `"Device:R"` to the full symbol S-expression
//! definition, and can inject it into a Schematic's `lib_symbols` section.

use crate::sexp::{parser, SexpNode};
use crate::Schematic;
use std::path::PathBuf;

/// Resolve a lib_id (e.g. "Device:R") to the full symbol S-expression string.
/// The returned string is the raw content of the `(symbol "R" ...)` block,
/// with the name prefixed as `"Device:R"`.
pub fn resolve_lib_symbol(lib_id: &str) -> Option<String> {
    let parts: Vec<&str> = lib_id.splitn(2, ':').collect();
    if parts.len() != 2 {
        return None;
    }
    let (library_name, symbol_name) = (parts[0], parts[1]);

    for base_dir in find_symbol_dirs() {
        // KiCAD 10: Library.kicad_symdir/SymbolName.kicad_sym
        let symdir = base_dir.join(format!("{}.kicad_symdir", library_name));
        let sym_file = symdir.join(format!("{}.kicad_sym", symbol_name));

        if sym_file.exists() {
            if let Ok(content) = std::fs::read_to_string(&sym_file) {
                if let Some(block) = extract_symbol_block(&content, symbol_name) {
                    // Rename symbol to include library prefix
                    let mut renamed = block.replacen(
                        &format!("(symbol \"{}\"", symbol_name),
                        &format!("(symbol \"{}:{}\"", library_name, symbol_name),
                        1,
                    );
                    // Also fix (extends "ParentName") to use prefixed name
                    if let Some(ext_pos) = renamed.find("(extends \"") {
                        let after = &renamed[ext_pos + 10..];
                        if let Some(end) = after.find('"') {
                            let parent = after[..end].to_string();
                            renamed = renamed.replace(
                                &format!("(extends \"{}\")", parent),
                                &format!("(extends \"{}:{}\")", library_name, parent),
                            );
                        }
                    }
                    // Unit sub-symbols ("Name_0_1", "Name_1_1") must stay
                    // UNPREFIXED: eeschema names only the outer symbol with
                    // the library prefix and refuses to load a schematic
                    // whose units carry it ("Failed to load schematic" —
                    // verified against kicad-cli 10.0 and the KiCAD demo
                    // corpus, which embeds units without the prefix).
                    return Some(renamed);
                }
            }
        }

        // Fallback: KiCAD 8/9 format — single Library.kicad_sym file
        let legacy = base_dir.join(format!("{}.kicad_sym", library_name));
        if legacy.exists() {
            if let Ok(content) = std::fs::read_to_string(&legacy) {
                if let Some(block) = extract_symbol_block(&content, symbol_name) {
                    let mut renamed = block.replacen(
                        &format!("(symbol \"{}\"", symbol_name),
                        &format!("(symbol \"{}:{}\"", library_name, symbol_name),
                        1,
                    );
                    if let Some(ext_pos) = renamed.find("(extends \"") {
                        let after = &renamed[ext_pos + 10..];
                        if let Some(end) = after.find('"') {
                            let parent = after[..end].to_string();
                            renamed = renamed.replace(
                                &format!("(extends \"{}\")", parent),
                                &format!("(extends \"{}:{}\")", library_name, parent),
                            );
                        }
                    }
                    // Unit sub-symbols stay UNPREFIXED here too — same rule
                    // as the symdir branch above (eeschema refuses prefixed
                    // unit names; hit in CI where KiCAD ships single-file
                    // libraries and this legacy branch handles the embed).
                    return Some(renamed);
                }
            }
        }
    }
    None
}

/// Resolve a lib_id to a parsed SexpNode tree.
pub fn resolve_lib_symbol_node(lib_id: &str) -> Option<SexpNode> {
    let raw = resolve_lib_symbol(lib_id)?;
    parser::parse(&raw).ok()
}

/// Resolve a lib_id to a parsed tree with any `(extends "Parent")` chain
/// FLATTENED, the way eeschema itself saves derived symbols (#35):
///
/// - the parent chain's unit sub-symbols are deep-copied into the child,
///   renamed `Parent_N_M` → `Derived_N_M`;
/// - parent properties and attribute nodes (pin_numbers, pin_names, in_bom,
///   …) are inherited unless the child overrides them;
/// - the `(extends …)` marker is dropped.
///
/// An extends STUB embed (child + separately embedded parent) is a shape
/// kicad-cli cannot resolve — the netlist gets a pinless libpart — and one
/// eeschema never writes. A missing/broken parent stops the walk gracefully,
/// returning the partially flattened child.
pub fn resolve_lib_symbol_flattened_node(lib_id: &str) -> Option<SexpNode> {
    let mut node = resolve_lib_symbol_node(lib_id)?;
    let child_base = lib_id.split_once(':')?.1.to_string();

    let mut parent_id = node.get_value("extends").map(str::to_string);
    if parent_id.is_none() {
        return Some(node); // not derived: nothing to flatten
    }
    if let SexpNode::List(children) = &mut node {
        children.retain(|c| c.tag() != Some("extends"));
    }

    let mut visited: std::collections::HashSet<String> =
        std::collections::HashSet::from([lib_id.to_string()]);
    while let Some(pid) = parent_id {
        if !visited.insert(pid.clone()) {
            break; // cyclic extends: stop, keep what we have
        }
        let Some(parent) = resolve_lib_symbol_node(&pid) else {
            break; // broken library (dangling parent): keep what we have
        };
        let parent_base = pid
            .split_once(':')
            .map_or(pid.as_str(), |x| x.1)
            .to_string();
        merge_parent_into_child(&mut node, &parent, &parent_base, &child_base);
        parent_id = parent.get_value("extends").map(str::to_string);
    }
    Some(node)
}

/// Serialized form of [`resolve_lib_symbol_flattened_node`], for callers that
/// splice raw text into a schematic's `lib_symbols` section.
pub fn resolve_lib_symbol_flattened(lib_id: &str) -> Option<String> {
    resolve_lib_symbol_flattened_node(lib_id).map(|n| crate::sexp::writer::write(&n))
}

/// Copy one parent level into a derived symbol: unit sub-symbols renamed to
/// the child's base name, plus properties / attribute nodes the child does
/// not define itself (most-derived wins, matching eeschema's inheritance).
fn merge_parent_into_child(
    child: &mut SexpNode,
    parent: &SexpNode,
    parent_base: &str,
    child_base: &str,
) {
    let child_subs: std::collections::HashSet<String> = child
        .find_all("symbol")
        .iter()
        .filter_map(|s| s.value())
        .map(String::from)
        .collect();
    let child_props: std::collections::HashSet<String> = child
        .find_all("property")
        .iter()
        .filter_map(|p| p.value())
        .map(String::from)
        .collect();

    let mut inherited: Vec<SexpNode> = Vec::new();
    for item in parent.args() {
        match item.tag() {
            Some("symbol") => {
                let Some(name) = item.value() else { continue };
                let Some(suffix) = unit_suffix_of(name, parent_base) else {
                    continue;
                };
                let new_name = format!("{child_base}{suffix}");
                if child_subs.contains(&new_name) {
                    continue; // child overrides this unit's drawing
                }
                let mut cloned = item.clone();
                if let SexpNode::List(c) = &mut cloned {
                    if c.len() >= 2 {
                        c[1] = SexpNode::Str(new_name);
                    }
                }
                inherited.push(cloned);
            }
            Some("property") => {
                let Some(key) = item.value() else { continue };
                if !child_props.contains(key) {
                    inherited.push(item.clone());
                }
            }
            // extends handled by the caller's chain walk.
            Some("extends") | None => {}
            // Attribute-style nodes (pin_numbers, pin_names, in_bom,
            // on_board, exclude_from_sim, …): inherit unless overridden.
            Some(tag) => {
                if child.find(tag).is_none() {
                    inherited.push(item.clone());
                }
            }
        }
    }
    if let SexpNode::List(c) = child {
        c.extend(inherited);
    }
}

/// The `_N_M` unit suffix of `name` given its base (e.g. `LM2904_1_1` with
/// base `LM2904` → `_1_1`). `None` unless the remainder is exactly two
/// `_`-separated integers.
fn unit_suffix_of<'a>(name: &'a str, base: &str) -> Option<&'a str> {
    let rest = name.strip_prefix(base)?;
    let mut it = rest.rsplitn(3, '_');
    let style = it.next()?;
    let unit = it.next()?;
    let lead = it.next()?;
    (lead.is_empty()
        && !style.is_empty()
        && !unit.is_empty()
        && style.bytes().all(|b| b.is_ascii_digit())
        && unit.bytes().all(|b| b.is_ascii_digit()))
    .then_some(rest)
}

/// Ensure a library symbol definition is present in the schematic's lib_symbols section.
/// If the symbol is already present (by name), does nothing.
/// If the lib_symbols node doesn't exist in raw_other, creates one.
///
/// Derived symbols (`(extends "Parent")`) are embedded FLATTENED — parent
/// units deep-copied and renamed, no extends stub — the way eeschema saves
/// them. The stub-plus-parent shape this used to write is unresolvable by
/// kicad-cli: its netlist showed a pinless libpart for every derived symbol
/// (#35).
///
/// Returns `false` when `lib_id` cannot be resolved from the installed
/// libraries — callers MUST surface that as an error: a symbol instance
/// without an embedded definition is invisible to KiCAD's netlister and
/// yields empty pin lists downstream (#34).
#[must_use]
pub fn ensure_lib_symbol(schematic: &mut Schematic, lib_id: &str) -> bool {
    // Check if already present
    let check_name = format!("\"{}\"", lib_id);
    let already_present = schematic.raw_other.iter().any(|node| {
        if node.tag() == Some("lib_symbols") {
            let content = format!("{:?}", node);
            content.contains(&check_name)
        } else {
            false
        }
    });
    if already_present {
        return true;
    }

    // Resolve and embed the symbol, flattening any extends chain.
    let sym_node = match resolve_lib_symbol_flattened_node(lib_id) {
        Some(n) => n,
        None => return false,
    };

    // Find or create the lib_symbols node
    let lib_syms_idx = schematic
        .raw_other
        .iter()
        .position(|n| n.tag() == Some("lib_symbols"));

    match lib_syms_idx {
        Some(idx) => {
            // Append the symbol to the existing lib_symbols list
            if let SexpNode::List(ref mut children) = schematic.raw_other[idx] {
                children.push(sym_node);
            }
        }
        None => {
            // Create a new lib_symbols node with this symbol
            let lib_syms =
                SexpNode::List(vec![SexpNode::Atom("lib_symbols".to_string()), sym_node]);
            // Insert at the beginning of raw_other (lib_symbols should come early)
            schematic.raw_other.insert(0, lib_syms);
        }
    }
    true
}

/// Number of units of the symbol `lib_id` resolves to, following the
/// `(extends "Parent")` chain when the symbol has no unit sub-symbols of its
/// own (#35). The count is the maximum `N >= 1` over `Name_N_M` sub-symbol
/// names; symbols with only a `_0_1` body (or none) count as 1. Returns
/// `None` when `lib_id` cannot be resolved at all.
pub fn symbol_unit_count(lib_id: &str) -> Option<u32> {
    let mut current = lib_id.to_string();
    let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
    while visited.insert(current.clone()) {
        let node = resolve_lib_symbol_node(&current)?;
        let max_unit = node
            .find_all("symbol")
            .iter()
            .filter_map(|s| s.value())
            .filter_map(konnect_sexp::schematic::parse_subsymbol_unit)
            .filter(|&n| n >= 1)
            .max();
        if let Some(n) = max_unit {
            return Some(n);
        }
        // No unit sub-symbols: a derived symbol inherits the parent's units.
        match node.get_value("extends") {
            Some(parent) if parent.contains(':') => current = parent.to_string(),
            _ => return Some(1),
        }
    }
    Some(1) // cyclic extends: treat as single-unit rather than erroring
}

/// Whether `library_name` (e.g. "Device") exists in any installed symbol dir,
/// in either the KiCAD 10 symdir layout or the legacy single-file one.
pub fn library_exists(library_name: &str) -> bool {
    find_symbol_dirs().iter().any(|base| {
        base.join(format!("{}.kicad_symdir", library_name)).is_dir()
            || base.join(format!("{}.kicad_sym", library_name)).is_file()
    })
}

/// Symbol names similar to the one in `lib_id`, for did-you-mean hints when a
/// lib_id doesn't resolve (#34: LLM callers habitually reach for KiCAD ≤9
/// names like `Device:CP` that KiCAD 10 renamed). Returns full `Library:Name`
/// ids, closest first, at most `limit`.
pub fn suggest_symbols(lib_id: &str, limit: usize) -> Vec<String> {
    let parts: Vec<&str> = lib_id.splitn(2, ':').collect();
    if parts.len() != 2 {
        return Vec::new();
    }
    let (library_name, symbol_name) = (parts[0], parts[1]);
    let wanted = symbol_name.to_lowercase();

    let mut candidates: Vec<String> = Vec::new();
    for base in find_symbol_dirs() {
        let symdir = base.join(format!("{}.kicad_symdir", library_name));
        if let Ok(entries) = std::fs::read_dir(&symdir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("kicad_sym") {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        candidates.push(stem.to_string());
                    }
                }
            }
        }
        // Legacy single-file library: scan top-level (symbol "NAME" entries.
        let legacy = base.join(format!("{}.kicad_sym", library_name));
        if let Ok(content) = std::fs::read_to_string(&legacy) {
            let mut from = 0usize;
            while let Some(rel) = content[from..].find("(symbol \"") {
                let start = from + rel + 9;
                if let Some(end) = content[start..].find('"') {
                    let name = &content[start..start + end];
                    // Skip unit sub-symbols ("R_0_1") and prefixed names.
                    if !name.contains(':') && extract_symbol_block(&content, name).is_some() {
                        candidates.push(name.to_string());
                    }
                    from = start + end;
                } else {
                    break;
                }
            }
        }
    }
    candidates.sort();
    candidates.dedup();

    rank_candidates(&wanted, candidates, limit)
        .into_iter()
        .map(|name| format!("{}:{}", library_name, name))
        .collect()
}

/// Plausible full `lib_id`s / library names for a `lib_id` that failed to
/// resolve — used to give a "not found" error its own candidate list instead
/// of leaving the caller to guess (#lib_id candidates).
///
/// Deterministic lookup only, never a guess the placement itself could act
/// on: this never returns a lib_id that resolves to nothing, and callers must
/// still treat the original `lib_id` as an error. Every candidate — exact or
/// fuzzy — must actually resemble the requested symbol name; an unrelated
/// symbol from the right library is exactly as misleading as a random one, so
/// a wrong-library guess is judged the same way a within-library typo is
/// (#lib_id false candidates: a repair loop that trusts these must never be
/// steered toward the wrong part).
///
/// Three cheap, directory-listing-only passes, closest first, capped at
/// `limit` combined:
/// 1. Exact (case-insensitive) symbol-name match in ANY installed library —
///    covers a plausible-sounding but wrong library for a real symbol
///    (`Resistor:R` / `Sensor:R` → `Device:R`).
/// 2. Fuzzy symbol-name match ranked GLOBALLY across every installed
///    library, not only the one named in `lib_id` — a wrong library guess
///    must not hide a real match elsewhere, and a typo inside a real but
///    small library must not be judged only against its own unrelated
///    siblings (this is the fix for `Sensor:R_0805` surfacing `Sensor:
///    MAX30102` — a pulse-oximeter symbol has nothing to do with a resistor
///    and must fail the shared similarity floor in [`rank_candidates`]).
/// 3. Installed library names close to the one asked for — only useful when
///    the named library itself doesn't exist.
///
/// When nothing clears the floor, this returns empty — the caller's
/// `candidates` field is then omitted rather than populated with a guess.
///
/// Only ever called on the failure path of a `lib_id` resolution — a
/// successful `resolve_lib_symbol`/`ensure_lib_symbol` never calls this, so a
/// correct placement never pays for it.
pub fn suggest_lib_ids(lib_id: &str, limit: usize) -> Vec<String> {
    let parts: Vec<&str> = lib_id.splitn(2, ':').collect();
    let (library_name, symbol_name) = match parts.as_slice() {
        [lib, sym] => (*lib, Some(*sym)),
        _ => return Vec::new(),
    };
    let libraries = all_libraries_with_symbols();
    suggest_lib_ids_from(
        &libraries,
        library_exists(library_name),
        library_name,
        symbol_name,
        limit,
    )
}

/// Pure core of [`suggest_lib_ids`], taking the installed-library listing as
/// data instead of reading the filesystem — unit-testable with a small fixed
/// fixture, independent of what KiCAD (if any) is actually installed on the
/// machine running the test.
fn suggest_lib_ids_from(
    libraries: &[(String, Vec<String>)],
    library_name_exists: bool,
    library_name: &str,
    symbol_name: Option<&str>,
    limit: usize,
) -> Vec<String> {
    let mut candidates: Vec<String> = Vec::new();

    // 1. Exact, case-insensitive symbol match in any library.
    if let Some(sym) = symbol_name {
        let sym_lower = sym.to_lowercase();
        for (lib, syms) in libraries {
            if candidates.len() >= limit {
                break;
            }
            if let Some(real) = syms.iter().find(|s| s.to_lowercase() == sym_lower) {
                candidates.push(format!("{lib}:{real}"));
            }
        }
    }

    // 2. Fuzzy symbol match, ranked globally — every symbol name across
    //    every library is one shared candidate pool, so the similarity floor
    //    in `rank_candidates` is what keeps an unrelated part out, not which
    //    library it happens to live in.
    if let Some(sym) = symbol_name {
        if candidates.len() < limit {
            let mut owners: std::collections::BTreeMap<String, Vec<String>> =
                std::collections::BTreeMap::new();
            for (lib, syms) in libraries {
                for s in syms {
                    owners.entry(s.clone()).or_default().push(lib.clone());
                }
            }
            let names: Vec<String> = owners.keys().cloned().collect();
            let wanted = sym.to_lowercase();
            'ranked: for name in rank_candidates(&wanted, names, limit) {
                for lib in &owners[&name] {
                    if candidates.len() >= limit {
                        break 'ranked;
                    }
                    let candidate = format!("{lib}:{name}");
                    if !candidates.contains(&candidate) {
                        candidates.push(candidate);
                    }
                }
            }
        }
    }

    // 3. Library names close to the one asked for — moot once the library
    //    itself is known to exist, since (1)/(2) already cover a symbol
    //    typo inside it.
    if !library_name_exists && candidates.len() < limit {
        let lib_names: Vec<String> = libraries.iter().map(|(name, _)| name.clone()).collect();
        let wanted = library_name.to_lowercase();
        let remaining = limit - candidates.len();
        for name in rank_candidates(&wanted, lib_names, remaining) {
            let candidate = match symbol_name {
                Some(sym) => format!("{name}:{sym}"),
                None => name,
            };
            if !candidates.contains(&candidate) {
                candidates.push(candidate);
            }
        }
    }

    candidates.truncate(limit);
    candidates
}

/// `(library_name, symbol names)` for every installed library, original
/// case preserved so a returned candidate is a real, placeable `lib_id`.
///
/// KiCAD 10 `.kicad_symdir` libraries cost one `read_dir` per library (file
/// names only, no file reads). Legacy single-file `.kicad_sym` libraries are
/// read once each — the same string scan [`suggest_symbols`] already does per
/// library, just run across all of them here. Both are failure-path only.
fn all_libraries_with_symbols() -> Vec<(String, Vec<String>)> {
    let mut libs: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for base in find_symbol_dirs() {
        let Ok(entries) = std::fs::read_dir(&base) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str());
            match ext {
                Some("kicad_symdir") if path.is_dir() => {
                    let Some(lib_name) = path.file_stem().and_then(|s| s.to_str()) else {
                        continue;
                    };
                    let syms = libs.entry(lib_name.to_string()).or_default();
                    if let Ok(sym_entries) = std::fs::read_dir(&path) {
                        for se in sym_entries.flatten() {
                            let sp = se.path();
                            if sp.extension().and_then(|e| e.to_str()) == Some("kicad_sym") {
                                if let Some(stem) = sp.file_stem().and_then(|s| s.to_str()) {
                                    syms.push(stem.to_string());
                                }
                            }
                        }
                    }
                }
                Some("kicad_sym") if path.is_file() => {
                    let Some(lib_name) = path.file_stem().and_then(|s| s.to_str()) else {
                        continue;
                    };
                    let syms = libs.entry(lib_name.to_string()).or_default();
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        let mut from = 0usize;
                        while let Some(rel) = content[from..].find("(symbol \"") {
                            let start = from + rel + 9;
                            let Some(end) = content[start..].find('"') else {
                                break;
                            };
                            let name = &content[start..start + end];
                            if !name.contains(':') && extract_symbol_block(&content, name).is_some()
                            {
                                syms.push(name.to_string());
                            }
                            from = start + end;
                        }
                    }
                }
                _ => {}
            }
        }
    }
    libs.into_iter().collect()
}

/// Rank `candidates` by similarity to `wanted` (already lowercased), keeping
/// at most `limit`, closest first. Pure so it's unit-testable without an
/// installed KiCAD.
fn rank_candidates(wanted: &str, candidates: Vec<String>, limit: usize) -> Vec<String> {
    let mut scored: Vec<(usize, String)> = candidates
        .into_iter()
        .filter_map(|name| {
            let lower = name.to_lowercase();
            // Stylized matches cover the classic KiCAD ≤9 shorthands the
            // renames expanded (CP → C_Polarized, R_POT_TRIM →
            // R_Potentiometer_Trim); substring containment covers truncations;
            // otherwise edit distance, capped so unrelated names don't surface.
            let dist = if stylized_match(wanted, &lower)
                || lower.contains(wanted)
                || wanted.contains(&lower)
            {
                1
            } else {
                edit_distance(wanted, &lower)
            };
            // Floor: at least half the longer name's characters must
            // actually agree, or the candidate is dropped outright rather
            // than merely ranked low. Loosened to 2/3 before, "MAX30102"
            // (dist 6, len 8) and "RPR-0521RS" (dist 7, len 10) both cleared
            // it for a request like "R_0805" — an unrelated symbol offered
            // as a fix is worse than no fix. At 1/2 both are rejected (need
            // <=4 and <=5) while every existing stylized/substring/typo case
            // this module handles is unaffected — those set `dist = 1` and
            // clear any cutoff down to `wanted.len() == 1`.
            let cutoff = wanted.len().max(lower.len()).div_ceil(2);
            (dist <= cutoff).then_some((dist, name))
        })
        .collect();
    scored.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    scored.into_iter().take(limit).map(|(_, n)| n).collect()
}

/// Shorthand relationships between a wanted name and a candidate (both
/// lowercase): the wanted name is the candidate's initials ("cp" vs
/// "c_polarized"), or both split into the same number of `_` tokens with each
/// wanted token a prefix of the candidate's ("r_pot_trim" vs
/// "r_potentiometer_trim").
fn stylized_match(wanted: &str, cand: &str) -> bool {
    let toks = |s: &str| -> Vec<String> {
        s.split(['_', '-', '.'])
            .filter(|t| !t.is_empty())
            .map(str::to_string)
            .collect()
    };
    let (w, c) = (toks(wanted), toks(cand));
    if w.len() == 1 && c.len() >= 2 {
        let initials: String = c.iter().filter_map(|t| t.chars().next()).collect();
        if initials == w[0] {
            return true;
        }
    }
    !w.is_empty() && w.len() == c.len() && w.iter().zip(&c).all(|(a, b)| b.starts_with(a.as_str()))
}

/// Plain Levenshtein distance, O(len(a)·len(b)) with a single-row table.
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut prev_diag = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            let val = (prev_diag + cost).min(row[j] + 1).min(row[j + 1] + 1);
            prev_diag = row[j + 1];
            row[j + 1] = val;
        }
    }
    row[b.len()]
}

/// Extract a `(symbol "NAME" ...)` block from file content by balanced-paren matching.
fn extract_symbol_block(content: &str, symbol_name: &str) -> Option<String> {
    let pattern = format!("(symbol \"{}\"", symbol_name);
    let start = content.find(&pattern)?;
    let mut depth = 0i32;
    let mut end = start;
    for (i, ch) in content[start..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    end = start + i + 1;
                    break;
                }
            }
            _ => {}
        }
    }
    if end > start {
        Some(content[start..end].to_string())
    } else {
        None
    }
}

/// Find directories where KiCAD symbol libraries are stored.
///
/// Delegates to [`crate::kicad_paths`] so symbol lookup, footprint lookup and
/// 3D-model lookup can never again disagree about where KiCad is installed.
pub fn find_symbol_dirs() -> Vec<PathBuf> {
    crate::kicad_paths::library_dirs("symbols")
}

#[cfg(test)]
mod suggestion_tests {
    use super::*;

    /// `KICAD10_SYMBOL_DIR` is process-global; cargo runs tests on multiple
    /// threads by default, so any two tests that both call `set_var` on it
    /// race. Every test in this module that touches the env var takes this
    /// lock first and holds the guard for its whole body — std-only, no new
    /// dependency, and the minimal fix now that a second such test exists
    /// alongside `ensure_lib_symbol_flattens_extends_chain`.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn edit_distance_basics() {
        assert_eq!(edit_distance("", ""), 0);
        assert_eq!(edit_distance("abc", "abc"), 0);
        assert_eq!(edit_distance("abc", "abd"), 1);
        assert_eq!(edit_distance("kitten", "sitting"), 3);
    }

    #[test]
    fn stylized_match_covers_the_kicad10_renames() {
        // The two shorthands from #34's repro.
        assert!(stylized_match("cp", "c_polarized"));
        assert!(stylized_match("r_pot_trim", "r_potentiometer_trim"));
        // Not everything matches.
        assert!(!stylized_match("cp", "resistor"));
        assert!(!stylized_match("irf830", "irf840"));
    }

    #[test]
    fn rank_candidates_surfaces_the_renamed_symbol() {
        let candidates = vec![
            "C".to_string(),
            "C_Polarized".to_string(),
            "C_Polarized_Small".to_string(),
            "R".to_string(),
            "L".to_string(),
        ];
        let ranked = rank_candidates("cp", candidates, 3);
        assert!(
            ranked.contains(&"C_Polarized".to_string()),
            "CP must suggest C_Polarized, got {ranked:?}"
        );
        assert!(!ranked.contains(&"R".to_string()));
    }

    #[test]
    fn rank_candidates_close_typo_and_cap() {
        let candidates = vec![
            "R_Potentiometer".to_string(),
            "R_Potentiometer_Trim".to_string(),
            "Fuse".to_string(),
        ];
        let ranked = rank_candidates("r_pot_trim", candidates, 2);
        assert_eq!(ranked.len().min(2), ranked.len(), "limit respected");
        assert_eq!(ranked[0], "R_Potentiometer_Trim");
        assert!(!ranked.contains(&"Fuse".to_string()));
    }

    #[test]
    fn ensure_lib_symbol_flattens_extends_chain() {
        let _env_guard = ENV_LOCK.lock().unwrap();
        // NE5532-style derived symbol: (extends "LM2904"), no drawing of its
        // own. The embed must copy the parent's unit sub-symbols renamed to
        // the derived name and drop the extends marker — the old stub+parent
        // shape produced a pinless libpart in kicad-cli's netlist (#35).
        let libdir = tempfile::tempdir().unwrap();
        let symdir = libdir.path().join("Amp.kicad_symdir");
        std::fs::create_dir_all(&symdir).unwrap();
        std::fs::write(
            symdir.join("LM2904.kicad_sym"),
            "(kicad_symbol_lib\n\t(version 20241209)\n\t(generator \"test\")\n\t(symbol \"LM2904\"\n\t\t(pin_names (offset 0.127))\n\t\t(in_bom yes)\n\t\t(property \"Reference\" \"U\" (at 0 0 0))\n\t\t(property \"Value\" \"LM2904\" (at 0 0 0))\n\t\t(property \"Datasheet\" \"lm2904.pdf\" (at 0 0 0))\n\t\t(symbol \"LM2904_1_1\"\n\t\t\t(pin output line (at 7.62 0 180) (length 2.54)\n\t\t\t\t(name \"~\" (effects (font (size 1.27 1.27))))\n\t\t\t\t(number \"1\" (effects (font (size 1.27 1.27))))\n\t\t\t)\n\t\t)\n\t\t(symbol \"LM2904_2_1\"\n\t\t\t(pin output line (at 7.62 0 180) (length 2.54)\n\t\t\t\t(name \"~\" (effects (font (size 1.27 1.27))))\n\t\t\t\t(number \"7\" (effects (font (size 1.27 1.27))))\n\t\t\t)\n\t\t)\n\t)\n)\n",
        )
        .unwrap();
        std::fs::write(
            symdir.join("NE5532.kicad_sym"),
            "(kicad_symbol_lib\n\t(version 20241209)\n\t(generator \"test\")\n\t(symbol \"NE5532\"\n\t\t(extends \"LM2904\")\n\t\t(property \"Reference\" \"U\" (at 0 0 0))\n\t\t(property \"Value\" \"NE5532\" (at 0 0 0))\n\t)\n)\n",
        )
        .unwrap();
        std::env::set_var("KICAD10_SYMBOL_DIR", libdir.path());

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("flat.kicad_sch");
        std::fs::write(
            &path,
            "(kicad_sch\n\t(version 20250610)\n\t(generator \"test\")\n\t(lib_symbols\n\t)\n)\n",
        )
        .unwrap();
        let mut sch = Schematic::load(&path).unwrap();
        assert!(ensure_lib_symbol(&mut sch, "Amp:NE5532"));
        sch.overwrite().unwrap();
        let out = std::fs::read_to_string(&path).unwrap();

        assert!(
            out.contains("(symbol \"Amp:NE5532\""),
            "derived symbol embedded:\n{out}"
        );
        assert!(
            out.contains("(symbol \"NE5532_1_1\"") && out.contains("(symbol \"NE5532_2_1\""),
            "parent units must be copied in, renamed to the derived base:\n{out}"
        );
        assert!(
            !out.contains("(extends"),
            "no extends stub may remain:\n{out}"
        );
        assert!(
            !out.contains("(symbol \"Amp:LM2904\""),
            "the parent must not be embedded separately:\n{out}"
        );
        assert!(
            out.contains("\"NE5532\""),
            "the child's own Value wins:\n{out}"
        );
        assert!(
            out.contains("lm2904.pdf"),
            "properties the child lacks are inherited:\n{out}"
        );
        // Pins from both units present exactly once.
        assert_eq!(out.matches("(number \"1\"").count(), 1);
        assert_eq!(out.matches("(number \"7\"").count(), 1);
    }

    #[test]
    fn ensure_lib_symbol_reports_failure_for_bogus_lib_id() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.kicad_sch");
        std::fs::write(
            &path,
            "(kicad_sch\n\t(version 20250610)\n\t(generator \"test\")\n\t(lib_symbols\n\t)\n)\n",
        )
        .unwrap();
        let mut sch = Schematic::load(&path).unwrap();
        // No library named like this exists anywhere.
        assert!(!ensure_lib_symbol(
            &mut sch,
            "Definitely_Not_A_Library_xyzzy:Nope"
        ));
    }

    /// Fixture library listing for [`suggest_lib_ids_from`]: `Device` with a
    /// real `R`; `Sensor` with `MAX30102` (pulse oximeter) and `RPR-0521RS`
    /// (proximity/ALS) — plausible project symbols with NOTHING to do with a
    /// resistor, reproducing the false-candidate bug against a library that
    /// legitimately exists but doesn't have the requested part. Passed as
    /// data (not written to disk / env) so the test is exact and immune to
    /// both parallel-test env races and whatever KiCAD happens to be
    /// installed on the machine running it.
    fn device_and_sensor_libraries() -> Vec<(String, Vec<String>)> {
        vec![
            ("Device".to_string(), vec!["R".to_string()]),
            (
                "Sensor".to_string(),
                vec!["MAX30102".to_string(), "RPR-0521RS".to_string()],
            ),
        ]
    }

    /// Fixture: one installed library, `Device`, with a single `R` symbol —
    /// enough to prove cross-library did-you-mean without depending on a real
    /// KiCAD install.
    fn write_device_r_fixture() -> tempfile::TempDir {
        let libdir = tempfile::tempdir().unwrap();
        let symdir = libdir.path().join("Device.kicad_symdir");
        std::fs::create_dir_all(&symdir).unwrap();
        std::fs::write(
            symdir.join("R.kicad_sym"),
            "(kicad_symbol_lib\n\t(version 20241209)\n\t(generator \"test\")\n\t(symbol \"R\"\n\t\t(property \"Reference\" \"R\" (at 0 0 0))\n\t\t(property \"Value\" \"R\" (at 0 0 0))\n\t)\n)\n",
        )
        .unwrap();
        std::env::set_var("KICAD10_SYMBOL_DIR", libdir.path());
        libdir
    }

    #[test]
    fn suggest_lib_ids_never_offers_an_unrelated_symbol_from_the_right_library() {
        let libraries = device_and_sensor_libraries();
        // Real bug, verbatim from the benchmark: "Sensor" exists but has no
        // "R_0805" — the within-library did-you-mean used to offer
        // Sensor:MAX30102 / Sensor:RPR-0521RS, both nonsense for a resistor.
        let candidates = suggest_lib_ids_from(&libraries, true, "Sensor", Some("R_0805"), 8);
        assert!(
            !candidates.contains(&"Sensor:MAX30102".to_string()),
            "a pulse-oximeter symbol is not a plausible fix for a resistor: {candidates:?}"
        );
        assert!(
            !candidates.contains(&"Sensor:RPR-0521RS".to_string()),
            "an ambient-light sensor is not a plausible fix for a resistor: {candidates:?}"
        );
        // "R_0805" does start with the real base name "R" (a hallucinated
        // footprint-size suffix tacked onto it) — that one genuinely clears
        // the floor globally, in the *other* library, which is the correct
        // fix to surface instead of the two unrelated Sensor symbols.
        assert_eq!(
            candidates,
            vec!["Device:R".to_string()],
            "expected only the genuinely close Device:R, no unrelated symbol riding along: {candidates:?}"
        );
    }

    #[test]
    fn suggest_lib_ids_reaches_the_standard_device_symbol_across_libraries() {
        let libraries = device_and_sensor_libraries();
        // "Sensor" exists (unlike the invented-library case) but a caller
        // asking for a plain "R" inside it should still be pointed at the
        // real Device:R — the global symbol-name search must not stop at
        // the named library once it exists.
        let candidates = suggest_lib_ids_from(&libraries, true, "Sensor", Some("R"), 8);
        assert!(
            candidates.contains(&"Device:R".to_string()),
            "expected Device:R among {candidates:?}"
        );
    }

    #[test]
    fn suggest_lib_ids_offers_the_plausible_real_library_for_an_invented_one() {
        let libraries = device_and_sensor_libraries();
        // "Resistor" is not an installed library; "Device" is, and it
        // provides an "R" symbol — the cross-library did-you-mean must
        // surface it as a full lib_id, not just a bare library name.
        let candidates = suggest_lib_ids_from(&libraries, false, "Resistor", Some("R"), 8);
        assert!(
            candidates.contains(&"Device:R".to_string()),
            "expected Device:R among {candidates:?}"
        );
        assert!(candidates.len() <= 8);
    }

    #[test]
    fn ensure_lib_symbol_resolves_a_known_good_lib_id_without_touching_candidates() {
        let _env_guard = ENV_LOCK.lock().unwrap();
        let _fixture = write_device_r_fixture();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.kicad_sch");
        std::fs::write(
            &path,
            "(kicad_sch\n\t(version 20250610)\n\t(generator \"test\")\n\t(lib_symbols\n\t)\n)\n",
        )
        .unwrap();
        let mut sch = Schematic::load(&path).unwrap();
        // A real lib_id resolves outright: the caller never reaches the
        // not-found path, so no candidate list is ever built for it.
        assert!(ensure_lib_symbol(&mut sch, "Device:R"));
    }
}
