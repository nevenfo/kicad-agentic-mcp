//! D.6.4: makes the plain-text error debt visible and non-regressive.
//!
//! `konnect-core` has two ways to fail a tool call: `CallToolResult::error(
//! "some prose")`, which a client can only render, and
//! `CallToolResult::error_kind(ToolErrorKind::Foo { .. }, "message")`
//! (`crates/konnect-core/src/mcp/error.rs`), which a recovery loop can branch
//! on. Converting the ~157 plain-text sites to catalogued kinds is D.6.1, and
//! it is explicitly out of scope for this change — what belongs here is the
//! instrument that keeps that number from drifting upward unnoticed while
//! D.6.1 is not yet done, and that turns each paid-down site into a number
//! someone has to consciously lower.
//!
//! [`KAM_ERROR_CATALOG_DEBT_CEILING`] must equal the current total exactly.
//! Both directions are a failure:
//!
//! * the total rises — a new plain-text error site landed uncatalogued;
//!   catalog it with a [`konnect_core::mcp::error::ToolErrorKind`] variant,
//!   or, if the increase is deliberate and already tracked as D.6.1 debt,
//!   raise the ceiling to match and say why in the commit;
//! * the total falls — D.6.1 progress happened and the ceiling is stale;
//!   lower it to the new total so the number can never silently drift back
//!   up to the old, larger one.
//!
//! Regenerate the exact value with `cargo test -p konnect-core --test
//! error_catalog_debt -- --nocapture`, which prints the total and the five
//! worst files — read that breakdown to pick which file D.6.1 attacks next.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The number of `CallToolResult::error(` call sites in
/// `crates/konnect-core/src` on 2026-08-18, measured by this file's own
/// scanner (`rg -c` on the same pattern gives the same order of magnitude,
/// but is not what this ceiling is defined against — the scanner is).
const KAM_ERROR_CATALOG_DEBT_CEILING: usize = 71;

fn src_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Counts plain-text `CallToolResult::error(` call sites in one source
/// string. A "site" is a line containing the literal call syntax whose
/// trimmed text does not start with `//` — so a doc comment that *mentions*
/// `CallToolResult::error(` (this crate's own error-taxonomy docs do, more
/// than once) is not counted as a debt site, only an actual call is.
///
/// Deliberately line-based, not a real parser: a call spanning multiple
/// lines (rare — these calls take one short string argument) would be missed
/// by one count and be exactly what makes the file worth reading. Simplicity
/// here is what keeps this scanner itself trustworthy enough to gate on.
fn count_uncatalogued_call_sites(source: &str) -> usize {
    source
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                return false;
            }
            // Two shapes of the same debt. The plain-text call is the obvious
            // one. `ToolErrorKind::HandlerError` written out at a call site is
            // the other, and counting it is what keeps this number honest:
            // `HandlerError` is documented as the catch-all "hasn't been
            // migrated yet", so converting plain text into it moves a site out
            // of this count while telling the caller nothing new — and worse,
            // asserts `TransientClass::None`, i.e. "do not retry", on failures
            // where starting KiCAD and retrying is exactly the fix. A metric
            // that rewards that is a metric that will get gamed.
            //
            // `ToolErrorKind::from_anyhow` is deliberately NOT counted: it
            // classifies at runtime from the error chain, so it lands on `Io`
            // or `Conflict` when it can and only falls back to the catch-all
            // when the chain really carries nothing more.
            line.contains("CallToolResult::error(") || line.contains("ToolErrorKind::HandlerError")
        })
        .count()
}

/// Per-file counts across every `.rs` file under `crates/konnect-core/src`,
/// recursively — this is what the scanner "counts" means in practice.
fn scan(root: &Path) -> BTreeMap<PathBuf, usize> {
    // `mcp/error.rs` defines the catalogue and fixtures every variant in its
    // own tests, including the catch-all. Counting those would make the
    // catalogue's own existence look like debt.
    let catalogue = root.join("mcp").join("error.rs");
    let mut counts = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in
            std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()))
        {
            let entry = entry.expect("dir entry");
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            if path == catalogue {
                continue;
            }
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
            let n = count_uncatalogued_call_sites(&source);
            if n > 0 {
                counts.insert(path, n);
            }
        }
    }
    counts
}

#[test]
fn plain_text_error_sites_do_not_drift_from_the_ceiling() {
    let counts = scan(&src_root());
    let total: usize = counts.values().sum();

    let mut by_count: Vec<(&PathBuf, &usize)> = counts.iter().collect();
    by_count.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    let breakdown: String = by_count
        .iter()
        .take(5)
        .map(|(path, n)| format!("\n  {n:>3}  {}", path.display()))
        .collect();

    if total > KAM_ERROR_CATALOG_DEBT_CEILING {
        panic!(
            "plain-text CallToolResult::error( sites rose from {KAM_ERROR_CATALOG_DEBT_CEILING} \
             to {total} — catalog the new site(s) with a ToolErrorKind variant (D.6.1), or raise \
             KAM_ERROR_CATALOG_DEBT_CEILING in tests/error_catalog_debt.rs to {total} if the \
             increase is deliberate. Worst files:{breakdown}"
        );
    }
    if total < KAM_ERROR_CATALOG_DEBT_CEILING {
        panic!(
            "plain-text CallToolResult::error( sites fell from {KAM_ERROR_CATALOG_DEBT_CEILING} \
             to {total} — D.6.1 progress happened. Lower KAM_ERROR_CATALOG_DEBT_CEILING in \
             tests/error_catalog_debt.rs to {total} so it cannot silently drift back up. Worst \
             files:{breakdown}"
        );
    }
}

#[cfg(test)]
mod scanner_self_check {
    use super::count_uncatalogued_call_sites;

    /// The scanner counts what it claims to count: real call sites, not
    /// mentions inside comments, and it does not undercount adjacent lines.
    #[test]
    fn counts_real_call_sites_and_skips_comment_mentions() {
        let fixture = r#"
// CallToolResult::error("mentioned in a line comment, not a call")
/// Docs sometimes say `CallToolResult::error()` in prose.
fn handler() -> anyhow::Result<CallToolResult> {
    if true {
        return Ok(CallToolResult::error("first real site"));
    }
    Ok(CallToolResult::error("second real site"))
}
"#;
        assert_eq!(count_uncatalogued_call_sites(fixture), 2);
    }

    #[test]
    fn empty_source_counts_zero() {
        assert_eq!(count_uncatalogued_call_sites(""), 0);
    }
}
