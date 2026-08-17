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

/// The one library whose symbol names carry a leading polarity sign as a
/// naming convention rather than as part of the part name (`power:+5V`,
/// `power:-12V`, `power:GND`). [`canonical_lib_id`]'s second rule is scoped to
/// it, so toggling a sign can never rewrite a real part number elsewhere.
const POWER_LIBRARY: &str = "power";

/// Deterministically rewrite a `lib_id` that does not resolve into the one
/// installed `lib_id` it can only have meant — or `None`.
///
/// This is **not** [`suggest_lib_ids`]'s fuzzy ranking promoted to an
/// auto-substitution. Nothing here scores, ranks or approximates: a rewrite is
/// returned only when the installed libraries admit exactly one answer, and
/// any ambiguity at all yields `None` so the caller still fails with its
/// did-you-mean list. Two rules, both measured on the E26 model-fit run
/// (H.6.1), where 16 of 60 attempts failed to apply on an unresolvable
/// `lib_id`:
///
/// 1. **Wrong library, right symbol.** The caller wrote the symbol name
///    correctly and invented the library around it — `regulator/AMS1117-3.3`,
///    `MICROCHIP/AMS1117-3.3`, `regulators/linear/AMS1117`, `Device:PWR_FLAG`,
///    bare `R`. Split on the last `:` or `/`, then look the stem up across
///    every installed library: exactly one owner means the library name was
///    never carrying information, and 8 of the 16 failures are that.
/// 2. **Power-symbol polarity sign.** `power:5V` for `power:+5V`, `power:+GND`
///    for `power:GND` — 2 more. Only tried after (1) misses, only inside
///    [`POWER_LIBRARY`], and still only on a unique match.
///
/// The remaining 6 are not deterministic and stay failures: `Resistor_SMD:R_0805`
/// asks for a footprint as a symbol, `Regulator:LDO_AMS1117-3.3` invents the
/// symbol name too, `power:VPU` matches nothing. Guessing at those is exactly
/// what this function refuses to do.
///
/// Case is compared insensitively but the *returned* id is the installed
/// spelling, so the result is always directly placeable.
pub fn canonical_lib_id(lib_id: &str) -> Option<String> {
    if resolve_lib_symbol(lib_id).is_some() {
        return None; // already valid — nothing to canonicalize
    }
    canonical_lib_id_from(&all_libraries_with_symbols(), lib_id)
}

/// Pure core of [`canonical_lib_id`], taking the installed-library listing as
/// data — unit-testable against a fixed fixture, like [`suggest_lib_ids_from`].
///
/// Assumes the caller already knows `lib_id` does not resolve; it does not
/// re-check the filesystem.
fn canonical_lib_id_from(libraries: &[(String, Vec<String>)], lib_id: &str) -> Option<String> {
    let stem = lib_id.rsplit([':', '/']).next()?;
    if stem.is_empty() {
        return None;
    }

    // Rule 1 — the stem names exactly one installed symbol, anywhere.
    if let Some(unique) = sole_owner(libraries, stem, None) {
        return Some(unique);
    }

    // Rule 2 — the same stem with its polarity sign toggled, power library only.
    let toggled = match stem.strip_prefix(['+', '-']) {
        Some(rest) => rest.to_string(),
        None => format!("+{stem}"),
    };
    if toggled.is_empty() {
        return None;
    }
    sole_owner(libraries, &toggled, Some(POWER_LIBRARY))
}

/// `Library:Symbol` when `stem` names exactly one symbol across `libraries`
/// (optionally restricted to `only_library`), `None` when it names none or
/// several. Distinct libraries spelling the same symbol name count as several:
/// that is the ambiguity this whole function exists to refuse.
fn sole_owner(
    libraries: &[(String, Vec<String>)],
    stem: &str,
    only_library: Option<&str>,
) -> Option<String> {
    let wanted = stem.to_lowercase();
    let mut found: Option<String> = None;
    for (lib, syms) in libraries {
        if only_library.is_some_and(|only| !lib.eq_ignore_ascii_case(only)) {
            continue;
        }
        for sym in syms {
            if sym.to_lowercase() != wanted {
                continue;
            }
            let hit = format!("{lib}:{sym}");
            match &found {
                Some(first) if *first == hit => {}
                Some(_) => return None, // two different owners — ambiguous
                None => found = Some(hit),
            }
        }
    }
    found
}

/// Process-wide answer cache for [`suggest_symbols`], same shape and same
/// invalidation rule as [`SUGGESTION_CACHE`] above: keyed on
/// `(installed dirs, lib_id, limit)`, so a `find_symbol_dirs()` change (env
/// var, tests) invalidates it honestly instead of leaking stale results.
/// Listing a single library's `.kicad_symdir` is one `read_dir`, but on this
/// host that one call still costs ~300ms per invocation (real KiCAD `Device`
/// ships ~700+ per-symbol files; each `DirEntry` pays antivirus/filesystem
/// overhead) — repeating that scan for the identical `lib_id` on a retry is
/// exactly the E26 waste, one library at a time instead of every library at
/// once.
static SUGGEST_SYMBOLS_CACHE: std::sync::OnceLock<std::sync::Mutex<Option<SuggestionCacheEntry>>> =
    std::sync::OnceLock::new();

/// Symbol names similar to the one in `lib_id`, for did-you-mean hints when a
/// lib_id doesn't resolve (#34: LLM callers habitually reach for KiCAD ≤9
/// names like `Device:CP` that KiCAD 10 renamed). Returns full `Library:Name`
/// ids, closest first, at most `limit`.
pub fn suggest_symbols(lib_id: &str, limit: usize) -> Vec<String> {
    let parts: Vec<&str> = lib_id.splitn(2, ':').collect();
    if parts.len() != 2 {
        return Vec::new();
    }
    let dirs = find_symbol_dirs();
    let key = (lib_id.to_string(), limit);
    let cache = SUGGEST_SYMBOLS_CACHE.get_or_init(|| std::sync::Mutex::new(None));
    if let Some((cached_dirs, map)) = cache.lock().unwrap().as_ref() {
        if *cached_dirs == dirs {
            if let Some(hit) = map.get(&key) {
                return hit.clone();
            }
        }
    }

    let (library_name, symbol_name) = (parts[0], parts[1]);
    let wanted = symbol_name.to_lowercase();

    let mut candidates: Vec<String> = Vec::new();
    for base in &dirs {
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

    let result: Vec<String> = rank_candidates(&wanted, candidates, limit)
        .into_iter()
        .map(|name| format!("{}:{}", library_name, name))
        .collect();

    let mut guard = cache.lock().unwrap();
    match guard.as_mut() {
        Some((cached_dirs, map)) if *cached_dirs == dirs => {
            map.insert(key, result.clone());
        }
        _ => {
            let mut map = std::collections::HashMap::new();
            map.insert(key, result.clone());
            *guard = Some((dirs, map));
        }
    }
    result
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
///
/// The candidate scan behind [`all_libraries_with_symbols`] is cached
/// per-process (E26), but the fuzzy-ranking work in [`suggest_lib_ids_from`]
/// over the whole installed corpus is still real CPU per distinct `lib_id`
/// (owner-map build + edit distance against every symbol name). A caller that
/// retries the exact same bad `lib_id` — the documented E26 case, and the
/// realistic one: an LLM repair loop re-submitting an unresolved id — pays
/// that a second time for no new information, so the *answer* is memoized
/// too, keyed on `(installed dirs, lib_id, limit)`. Same honesty rule as the
/// index cache: a `find_symbol_dirs()` change invalidates the whole answer
/// cache along with the index. Entries are a handful of short `String`s each;
/// unbounded growth over a long-lived process is a non-issue for a
/// human-scale set of distinct failing lookups, so no eviction.
type SuggestionCacheEntry = (
    Vec<PathBuf>,
    std::collections::HashMap<(String, usize), Vec<String>>,
);
static SUGGESTION_CACHE: std::sync::OnceLock<std::sync::Mutex<Option<SuggestionCacheEntry>>> =
    std::sync::OnceLock::new();

pub fn suggest_lib_ids(lib_id: &str, limit: usize) -> Vec<String> {
    let dirs = find_symbol_dirs();
    let key = (lib_id.to_string(), limit);
    let cache = SUGGESTION_CACHE.get_or_init(|| std::sync::Mutex::new(None));
    if let Some((cached_dirs, map)) = cache.lock().unwrap().as_ref() {
        if *cached_dirs == dirs {
            if let Some(hit) = map.get(&key) {
                return hit.clone();
            }
        }
    }

    let parts: Vec<&str> = lib_id.splitn(2, ':').collect();
    let (library_name, symbol_name) = match parts.as_slice() {
        [lib, sym] => (*lib, Some(*sym)),
        _ => return Vec::new(),
    };
    let libraries = all_libraries_with_symbols();
    let result = suggest_lib_ids_from(
        &libraries,
        library_exists(library_name),
        library_name,
        symbol_name,
        limit,
    );

    let mut guard = cache.lock().unwrap();
    match guard.as_mut() {
        Some((cached_dirs, map)) if *cached_dirs == dirs => {
            map.insert(key, result.clone());
        }
        _ => {
            let mut map = std::collections::HashMap::new();
            map.insert(key, result.clone());
            *guard = Some((dirs, map));
        }
    }
    result
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

/// Process-wide cache of [`all_libraries_with_symbols`], keyed on the
/// resolved directory list rather than assumed constant for the process
/// lifetime (#E26).
///
/// The installed-library set is determined by `find_symbol_dirs()`, which in
/// turn depends on `KICAD10_SYMBOL_DIR` / `KICAD9_SYMBOL_DIR` /
/// `KICAD8_SYMBOL_DIR` and the bundled install locations. In a real server
/// process those are fixed at launch and never change, so one scan per
/// process is correct. Tests, however, call `std::env::set_var` on those vars
/// mid-process (see `ENV_LOCK` below) — keying the cache on the *resolved
/// dirs* rather than "first call wins" means a changed env var invalidates
/// the cache honestly instead of leaking one test's library listing into
/// another's assertions. The cache holds the full name index (every library
/// name plus every symbol stem it contains, original case, as `String`s) —
/// measured on a stock KiCAD 10 Windows install: 222 libraries, 46 315 symbol
/// names, ~660 KB of raw characters; with one heap allocation per `String`
/// and per `Vec`, resident size is a low single-digit MB, not held across an
/// `await` (this whole module
/// is synchronous filesystem code, called from sync tool handlers). Stored
/// behind an `Arc` so a cache hit is a refcount bump, not a deep clone of
/// every library/symbol `String` on each lookup — the fuzzy-ranking work in
/// [`suggest_lib_ids_from`] over the full corpus already dominates a cached
/// call; re-cloning the whole index on top of it would keep that call
/// hundreds of ms instead of the few ms a scan-free lookup should cost.
type LibraryIndexEntry = (Vec<PathBuf>, std::sync::Arc<Vec<(String, Vec<String>)>>);
static LIBRARY_INDEX_CACHE: std::sync::OnceLock<std::sync::Mutex<Option<LibraryIndexEntry>>> =
    std::sync::OnceLock::new();

/// `(library_name, symbol names)` for every installed library, original
/// case preserved so a returned candidate is a real, placeable `lib_id`.
///
/// KiCAD 10 `.kicad_symdir` libraries cost one `read_dir` per library (file
/// names only, no file reads). Legacy single-file `.kicad_sym` libraries are
/// read once each — the same string scan [`suggest_symbols`] already does per
/// library, just run across all of them here. Both are failure-path only —
/// but the failure path itself is what E26 caches: the scan runs once per
/// distinct `find_symbol_dirs()` result and is reused after that, so a
/// repeated miss on the same bad `lib_id` (or any other unresolved one) no
/// longer re-walks every installed library.
fn all_libraries_with_symbols() -> std::sync::Arc<Vec<(String, Vec<String>)>> {
    let dirs = find_symbol_dirs();
    let cache = LIBRARY_INDEX_CACHE.get_or_init(|| std::sync::Mutex::new(None));
    if let Some((cached_dirs, libs)) = cache.lock().unwrap().as_ref() {
        if *cached_dirs == dirs {
            return libs.clone();
        }
    }

    // Second, colder tier: an on-disk cache from a PREVIOUS process (E26
    // second pass). The in-memory cache above already makes every retry
    // within one process free; this is for the very first miss of a fresh
    // process, which the benchmark's per-run server spawn pays every time.
    // See [`load_symbol_index_cache`] for the invalidation key and the
    // corrupt/partial-file fallback.
    let libs = match load_symbol_index_cache(&dirs) {
        Some(from_disk) => std::sync::Arc::new(from_disk),
        None => {
            let (fingerprint, scanned) = scan_libraries_with_symbols(&dirs);
            write_symbol_index_cache(&dirs, &fingerprint, &scanned);
            std::sync::Arc::new(scanned)
        }
    };
    *cache.lock().unwrap() = Some((dirs, libs.clone()));
    libs
}

/// A cheap-to-recompute fingerprint of one resolved symbol directory, used to
/// validate the on-disk index cache without re-walking every library inside
/// it: the directory's own mtime plus its immediate entry count (number of
/// `.kicad_symdir`/`.kicad_sym` top-level entries). Both come from the same
/// top-level `read_dir(base)` + one `metadata()` call the scan already makes,
/// so computing this costs nothing extra on the scan path.
///
/// Deliberately shallow: this catches a library being added/removed/replaced
/// (a KiCAD version change, or the test suite's `KICAD10_SYMBOL_DIR` swap),
/// which is the E26 case (a fresh process, same install, repeated lookup). It
/// does NOT notice a symbol file added inside an *existing* `.kicad_symdir`
/// without touching the parent directory's mtime — accepted because this
/// cache only ever feeds [`suggest_lib_ids`]'s did-you-mean candidates, never
/// `resolve_lib_symbol`'s success path: a stale entry here can at worst omit
/// or miss a very recently added symbol from a suggestion list, never return
/// a wrong "resolved" answer (the resolve path always reads the real file).
#[derive(Debug, Clone, PartialEq, Eq)]
struct DirFingerprint {
    path: PathBuf,
    mtime_secs: u64,
    entry_count: u64,
}

fn dir_fingerprint(base: &PathBuf, entry_count: u64) -> DirFingerprint {
    let mtime_secs = std::fs::metadata(base)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    DirFingerprint {
        path: base.clone(),
        mtime_secs,
        entry_count,
    }
}

/// Where the on-disk symbol index cache lives, and how its filename is keyed:
/// `%LOCALAPPDATA%\konnect-mcp\symbol-index\<hash>.cache` (falling back to
/// `std::env::temp_dir()` if `LOCALAPPDATA` isn't set — e.g. non-Windows dev
/// runs). `<hash>` is a `DefaultHasher` digest of the resolved directory
/// *paths* (order-sensitive, matching `find_symbol_dirs()`'s deterministic
/// order) — NOT their mtimes/counts, which live inside the file and are
/// re-checked on every read. This means:
/// - two different installs (or two tests using two different
///   `tempfile::tempdir()` fixtures via `KICAD10_SYMBOL_DIR`) never share a
///   cache file, so a test's synthetic library set can't leak into another
///   test or into a real install's cache;
/// - the same install across process restarts reliably finds the same file.
fn symbol_index_cache_path(dirs: &[PathBuf]) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for d in dirs {
        d.hash(&mut hasher);
    }
    let key = hasher.finish();
    let root = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("konnect-mcp")
        .join("symbol-index");
    root.join(format!("{key:016x}.cache"))
}

const SYMBOL_INDEX_CACHE_MAGIC: &str = "KAM_SYM_INDEX_V1";

/// Reads and validates the on-disk index cache for `dirs`. Returns `None` on
/// ANY of: no cache file, wrong magic, malformed content, or a fingerprint
/// mismatch against the live directories (stale) — every one of those
/// degrades to a full rescan in the caller, never to a wrong answer; this
/// function itself never fabricates data, it only parses what's on disk or
/// gives up.
fn load_symbol_index_cache(dirs: &[PathBuf]) -> Option<Vec<(String, Vec<String>)>> {
    let path = symbol_index_cache_path(dirs);
    let content = std::fs::read_to_string(&path).ok()?;
    let mut lines = content.split('\n');

    if lines.next()? != SYMBOL_INDEX_CACHE_MAGIC {
        return None;
    }

    let n_dirs: usize = lines.next()?.parse().ok()?;
    if n_dirs != dirs.len() {
        return None;
    }
    for base in dirs {
        let line = lines.next()?;
        let mut fields = line.splitn(3, '\t');
        let mtime_secs: u64 = fields.next()?.parse().ok()?;
        let entry_count: u64 = fields.next()?.parse().ok()?;
        let stored_path = fields.next()?;
        let live = dir_fingerprint(base, {
            // Recompute the live entry count the same cheap way the scan
            // does — one `read_dir` per base dir, of which there are only a
            // handful (never the 222-library count this cache exists to
            // avoid).
            std::fs::read_dir(base)
                .map(|e| e.flatten().count() as u64)
                .unwrap_or(0)
        });
        if stored_path != live.path.to_string_lossy()
            || mtime_secs != live.mtime_secs
            || entry_count != live.entry_count
        {
            return None; // stale: install changed since the cache was written
        }
    }

    let mut libs: Vec<(String, Vec<String>)> = Vec::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let mut fields = line.split('\t');
        let lib_name = fields.next()?.to_string();
        let syms: Vec<String> = fields.map(str::to_string).collect();
        libs.push((lib_name, syms));
    }
    Some(libs)
}

/// Writes the on-disk index cache. Best-effort: any I/O failure (unwritable
/// dir, race with another process, disk full) is silently swallowed — the
/// in-memory result the caller already computed is still returned and used,
/// a missing/failed cache write just means the next fresh process pays the
/// scan again, exactly like today.
///
/// Written to a per-process temp file then renamed into place, so a reader
/// racing a writer (two server processes launched close together) only ever
/// sees either the old complete file or the new complete file, never a
/// half-written one.
fn write_symbol_index_cache(
    dirs: &[PathBuf],
    fingerprints: &[DirFingerprint],
    libs: &[(String, Vec<String>)],
) {
    let path = symbol_index_cache_path(dirs);
    let Some(parent) = path.parent() else { return };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }

    let mut out = String::with_capacity(64 * 1024);
    out.push_str(SYMBOL_INDEX_CACHE_MAGIC);
    out.push('\n');
    out.push_str(&fingerprints.len().to_string());
    out.push('\n');
    for fp in fingerprints {
        out.push_str(&format!(
            "{}\t{}\t{}\n",
            fp.mtime_secs,
            fp.entry_count,
            fp.path.to_string_lossy()
        ));
    }
    for (lib_name, syms) in libs {
        out.push_str(lib_name);
        for s in syms {
            out.push('\t');
            out.push_str(s);
        }
        out.push('\n');
    }

    let tmp_path = path.with_extension(format!("tmp-{}", std::process::id()));
    if std::fs::write(&tmp_path, &out).is_err() {
        return;
    }
    let _ = std::fs::rename(&tmp_path, &path);
}

/// One unit of per-library filesystem work discovered by the cheap top-level
/// `read_dir(base)` pass in [`scan_libraries_with_symbols`]: either a KiCAD 10
/// `.kicad_symdir` (needs its own `read_dir`) or a legacy single-file
/// `.kicad_sym` (needs a `read_to_string`).
enum LibraryWorkItem {
    SymDir { lib_name: String, path: PathBuf },
    LegacyFile { lib_name: String, path: PathBuf },
}

/// Filesystem scan behind [`all_libraries_with_symbols`], separated out so
/// the cache above wraps it without holding its lock during the scan itself.
///
/// The expensive part is not file *content* — `.kicad_symdir` libraries only
/// need their directory entries' names, never a file read — it's the sheer
/// *count* of `read_dir` calls: one per installed library (222 on a stock
/// KiCAD 10 install), each paying independent filesystem/antivirus latency
/// (measured ~300ms/call on this host, see [`SUGGEST_SYMBOLS_CACHE`]'s doc).
/// That's I/O-bound wait, not CPU work, so a first top-level `read_dir(base)`
/// collects the list of per-library work cheaply, then a small thread pool
/// (std, not rayon — this crate has no dependency on it and one hot path
/// doesn't justify adding one) fires the 222 `read_dir`/`read_to_string`
/// calls concurrently: wall time collapses from ~(N × per-call latency) to
/// ~(N / worker_count × per-call latency) since each is waiting on I/O, not
/// competing for CPU.
fn scan_libraries_with_symbols(
    dirs: &[PathBuf],
) -> (Vec<DirFingerprint>, Vec<(String, Vec<String>)>) {
    let mut work: Vec<LibraryWorkItem> = Vec::new();
    let mut fingerprints: Vec<DirFingerprint> = Vec::new();
    for base in dirs {
        let Ok(entries) = std::fs::read_dir(base) else {
            fingerprints.push(dir_fingerprint(base, 0));
            continue;
        };
        let mut entry_count: u64 = 0;
        for entry in entries.flatten() {
            entry_count += 1;
            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str());
            match ext {
                Some("kicad_symdir") if path.is_dir() => {
                    let Some(lib_name) = path.file_stem().and_then(|s| s.to_str()) else {
                        continue;
                    };
                    work.push(LibraryWorkItem::SymDir {
                        lib_name: lib_name.to_string(),
                        path,
                    });
                }
                Some("kicad_sym") if path.is_file() => {
                    let Some(lib_name) = path.file_stem().and_then(|s| s.to_str()) else {
                        continue;
                    };
                    work.push(LibraryWorkItem::LegacyFile {
                        lib_name: lib_name.to_string(),
                        path,
                    });
                }
                _ => {}
            }
        }
        fingerprints.push(dir_fingerprint(base, entry_count));
    }

    // I/O-bound, not CPU-bound: oversubscribe cores heavily, since each
    // thread spends nearly all its time blocked on a `read_dir`/read, not
    // competing for CPU. Capped so a pathological number of libraries
    // doesn't spawn unbounded OS threads.
    let worker_count = work.len().clamp(1, 256);
    let chunks: Vec<Vec<LibraryWorkItem>> = {
        let mut buckets: Vec<Vec<LibraryWorkItem>> =
            (0..worker_count).map(|_| Vec::new()).collect();
        for (i, item) in work.into_iter().enumerate() {
            buckets[i % worker_count].push(item);
        }
        buckets
    };

    let mut libs: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    std::thread::scope(|scope| {
        let handles: Vec<_> = chunks
            .into_iter()
            .map(|chunk| scope.spawn(move || scan_library_work_chunk(chunk)))
            .collect();
        for handle in handles {
            if let Ok(partial) = handle.join() {
                for (lib_name, syms) in partial {
                    libs.entry(lib_name).or_default().extend(syms);
                }
            }
        }
    });
    (fingerprints, libs.into_iter().collect())
}

/// Runs one worker's slice of [`LibraryWorkItem`]s. Pure I/O, no shared
/// state — each thread returns its own partial map for the caller to merge,
/// so there's no lock to hold (across an `await` or otherwise; this whole
/// path is synchronous).
fn scan_library_work_chunk(chunk: Vec<LibraryWorkItem>) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    for item in chunk {
        match item {
            LibraryWorkItem::SymDir { lib_name, path } => {
                let mut syms = Vec::new();
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
                out.push((lib_name, syms));
            }
            LibraryWorkItem::LegacyFile { lib_name, path } => {
                let mut syms = Vec::new();
                if let Ok(content) = std::fs::read_to_string(&path) {
                    let mut from = 0usize;
                    while let Some(rel) = content[from..].find("(symbol \"") {
                        let start = from + rel + 9;
                        let Some(end) = content[start..].find('"') else {
                            break;
                        };
                        let name = &content[start..start + end];
                        if !name.contains(':') && extract_symbol_block(&content, name).is_some() {
                            syms.push(name.to_string());
                        }
                        from = start + end;
                    }
                }
                out.push((lib_name, syms));
            }
        }
    }
    out
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

    /// Fixture for [`canonical_lib_id_from`]: the libraries the E26 failures
    /// actually reached for, plus a deliberate duplicate symbol name across two
    /// libraries to pin the ambiguity refusal.
    fn canonicalization_libraries() -> Vec<(String, Vec<String>)> {
        vec![
            ("Device".to_string(), vec!["R".to_string(), "C".to_string()]),
            (
                "Regulator_Linear".to_string(),
                vec!["AMS1117".to_string(), "AMS1117-3.3".to_string()],
            ),
            (
                "power".to_string(),
                vec!["+5V".to_string(), "GND".to_string(), "PWR_FLAG".to_string()],
            ),
            // Same name in two libraries: an unresolvable lib_id naming it must
            // stay unresolvable.
            ("Amplifier_Operational".to_string(), vec!["U".to_string()]),
            ("Simulation_SPICE".to_string(), vec!["U".to_string()]),
        ]
    }

    #[test]
    fn canonicalization_fixes_an_invented_library_around_a_real_symbol() {
        let libs = canonicalization_libraries();
        // Verbatim from the E26 model-fit run: four spellings of a library that
        // does not exist, wrapped around a symbol name that is exactly right.
        for asked in [
            "regulator/AMS1117-3.3",
            "MICROCHIP/AMS1117-3.3",
            "regulators/AMS1117-3.3",
            "device/AMS1117-3.3",
        ] {
            assert_eq!(
                canonical_lib_id_from(&libs, asked).as_deref(),
                Some("Regulator_Linear:AMS1117-3.3"),
                "expected the one installed owner of the symbol for {asked}"
            );
        }
        // A nested path and a bare symbol name with no library at all.
        assert_eq!(
            canonical_lib_id_from(&libs, "regulators/linear/AMS1117").as_deref(),
            Some("Regulator_Linear:AMS1117")
        );
        assert_eq!(
            canonical_lib_id_from(&libs, "R").as_deref(),
            Some("Device:R")
        );
        // Right symbol, wrong-but-real library — the library name was never
        // carrying information once the symbol name is globally unique.
        assert_eq!(
            canonical_lib_id_from(&libs, "Device:PWR_FLAG").as_deref(),
            Some("power:PWR_FLAG")
        );
    }

    #[test]
    fn canonicalization_toggles_the_power_polarity_sign_both_ways() {
        let libs = canonicalization_libraries();
        assert_eq!(
            canonical_lib_id_from(&libs, "power:5V").as_deref(),
            Some("power:+5V")
        );
        assert_eq!(
            canonical_lib_id_from(&libs, "power:+GND").as_deref(),
            Some("power:GND")
        );
    }

    #[test]
    fn canonicalization_refuses_everything_it_cannot_prove() {
        let libs = canonicalization_libraries();
        // Two libraries own "U": no unique answer, so no rewrite — the caller
        // gets its did-you-mean list instead.
        assert_eq!(canonical_lib_id_from(&libs, "MCU:U"), None);
        // The rest of the E26 residue, which is not a naming slip at all:
        // a footprint asked for as a symbol, an invented symbol name, and a
        // net name that matches nothing.
        assert_eq!(canonical_lib_id_from(&libs, "Resistor_SMD:R_0805"), None);
        assert_eq!(
            canonical_lib_id_from(&libs, "Regulator:LDO_AMS1117-3.3"),
            None
        );
        assert_eq!(canonical_lib_id_from(&libs, "power:VPU"), None);
        assert_eq!(canonical_lib_id_from(&libs, ""), None);
        // The sign rule is scoped to `power`: toggling must not reach a part
        // number in another library.
        assert_eq!(canonical_lib_id_from(&libs, "Device:+R"), None);
    }

    #[test]
    fn canonical_lib_id_leaves_a_resolvable_lib_id_alone() {
        let _env_guard = ENV_LOCK.lock().unwrap();
        let _fixture = write_device_r_fixture();
        // Already valid: no rewrite, and no claim of one.
        assert_eq!(canonical_lib_id("Device:R"), None);
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
