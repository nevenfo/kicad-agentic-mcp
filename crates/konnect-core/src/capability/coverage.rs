//! What proves a capability works, discovered rather than declared.
//!
//! The matrix would be worth nothing if `SUPPORTED` were a field somebody sets.
//! This module goes and looks: it reads the repository's own test sources and
//! golden benchmark tasks and reports, per tool, the strongest proof it can
//! find. A tool nobody exercises comes back [`Proof::None`] and is published as
//! `NOT_TESTED` however finished its code looks.
//!
//! Three deliberate choices about what counts:
//!
//! * **A test that does not run is not a proof.** `#[ignore]`d tests — the ones
//!   needing a live KiCAD GUI or the symbol libraries — are reported as
//!   [`Proof::Gated`]. They are real evidence for a human and no evidence at
//!   all for a claim of coverage, so they are shown and do not count.
//! * **The registry's own tests are excluded.** `router/` enumerates every tool
//!   name to check the catalogue, so scanning it would "prove" all 196 tools at
//!   once. The scan skips it; proof has to come from a test that exercises the
//!   behaviour or a benchmark that runs it against KiCAD.
//! * **A tool is recognised by its name or its handler.** Unit tests here call
//!   `handle_add_wire(..)` directly while protocol tests send `"add_wire"` over
//!   MCP; both are the tool being exercised.

use std::collections::BTreeMap;
use std::path::Path;

/// How strongly a tool is exercised by this repository.
///
/// The order is the strength order, and it is the point: `record` keeps the
/// strongest proof found, and [`super::Capability::status`] compares what was
/// found against what the capability *requires* ([`super::Bar`]). The split
/// that matters is between the proofs our own code can produce alone
/// ([`Proof::Test`], [`Proof::Bench`]) and the ones where KiCAD is the judge
/// ([`Proof::Arbitrated`], [`Proof::Live`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Proof {
    /// Nothing found.
    None,
    /// Only by a test that is `#[ignore]`d — it needs a live KiCAD, a GUI
    /// session, or the installed symbol libraries.
    Gated,
    /// An automated test that runs in the default gate. It proves our code
    /// does what our code claims, and nothing about what KiCAD accepts.
    Test,
    /// A golden benchmark task or probe, with committed results. Stronger
    /// than a unit test because it runs the tool end to end, but it still does
    /// not, on its own, put the resulting document in front of KiCAD.
    Bench,
    /// KiCAD read the result back and accepted it: a test that submits the
    /// mutated document to `kicad-cli` through [`ARBITER`] and fails when
    /// KiCAD refuses it. This is the weakest proof that can back a claim about
    /// a file KiCAD has to load.
    Arbitrated,
    /// A live KiCAD session performed the operation and the result was read
    /// back from it — the only proof available for a tool that has no file to
    /// parse, because its whole effect is on the running editor.
    Live,
}

impl Proof {
    pub fn label(self) -> &'static str {
        match self {
            Proof::None => "—",
            Proof::Gated => "gated",
            Proof::Test => "test",
            Proof::Bench => "bench",
            Proof::Arbitrated => "kicad-parsed",
            Proof::Live => "live",
        }
    }

    /// Whether this proof may support a claim of coverage *at all*. Clearing
    /// this bar is necessary and no longer sufficient: the capability's own
    /// [`super::Bar`] decides how much more it takes.
    pub fn is_evidence(self) -> bool {
        self >= Proof::Test
    }
}

/// The test helper that makes KiCAD the judge, and the marker the scan looks
/// for. A test function that calls it has handed its result to `kicad-cli`;
/// the tools that same function names are credited [`Proof::Arbitrated`].
///
/// Naming it here, rather than matching a pattern, keeps the credit narrow: a
/// helper renamed without updating this constant silently *loses* proofs,
/// which fails safe.
pub const ARBITER: &str = "kicad_reloads";

/// The same idea for the operations no file can settle: a helper that asks the
/// running KiCAD what it now holds, so the assertion is made against the
/// editor's own answer rather than against the request we sent it.
pub const LIVE_ARBITER: &str = "kicad_reads_back";

/// The strongest proof found for a tool, and where it was found.
#[derive(Debug, Clone)]
pub struct Evidence {
    pub proof: Proof,
    /// Repository-relative path of the file that provides it. Lexicographically
    /// smallest among the files at that strength, so the document is stable.
    pub source: Option<String>,
}

impl Default for Evidence {
    fn default() -> Self {
        Evidence {
            proof: Proof::None,
            source: None,
        }
    }
}

/// Proof for every tool named in [`super::MANIFEST`].
#[derive(Debug, Default)]
pub struct Coverage {
    by_tool: BTreeMap<String, Evidence>,
}

impl Coverage {
    pub fn get(&self, tool: &str) -> Evidence {
        self.by_tool.get(tool).cloned().unwrap_or_default()
    }

    fn record(&mut self, tool: &str, proof: Proof, source: &str) {
        let entry = self.by_tool.entry(tool.to_string()).or_default();
        let better = proof > entry.proof;
        let same_but_earlier = proof == entry.proof
            && entry
                .source
                .as_deref()
                .map(|existing| source < existing)
                .unwrap_or(true);
        if better || same_but_earlier {
            entry.proof = proof;
            entry.source = Some(source.to_string());
        }
    }
}

/// Directories whose test code is scanned for proofs, relative to the
/// workspace root. `crates/konnect-core/src/router` is deliberately absent:
/// its tests enumerate the catalogue rather than exercise it.
const SRC_TEST_ROOTS: &[&str] = &[
    "crates/konnect-core/src/tools",
    "crates/konnect-core/src/plan",
    "crates/konnect-core/src/evidence",
    "crates/konnect-core/src/mcp",
];

/// Walk the repository at `root` and collect the proof for each tool in
/// `tools`.
pub fn scan(root: &Path, tools: &[&str]) -> Coverage {
    let mut coverage = Coverage::default();

    for dir in ["bench/tasks", "bench/probes"] {
        for (path, text) in read_dir_sorted(&root.join(dir), "yaml") {
            let rel = format!("{dir}/{path}");
            for tool in yaml_tools(&text) {
                if tools.contains(&tool.as_str()) {
                    coverage.record(&tool, Proof::Bench, &rel);
                }
            }
        }
    }

    for crate_dir in read_subdirs_sorted(&root.join("crates")) {
        let tests = root.join("crates").join(&crate_dir).join("tests");
        for (path, text) in read_dir_sorted(&tests, "rs") {
            let rel = format!("crates/{crate_dir}/tests/{path}");
            collect(&mut coverage, tools, &text, &rel);
        }
    }

    for dir in SRC_TEST_ROOTS {
        for (path, text) in read_dir_sorted(&root.join(dir), "rs") {
            let Some(idx) = text.find("#[cfg(test)]") else {
                continue;
            };
            let rel = format!("{dir}/{path}");
            collect(&mut coverage, tools, &text[idx..], &rel);
        }
    }

    coverage
}

/// Attribute every tool mention in `text` to the function that contains it,
/// so an `#[ignore]`d test cannot pass as a proof.
///
/// Functions are delimited by their declaration lines rather than by brace
/// matching: a `fn` line ends the previous block. That is exact for test files
/// (a mention lives inside some function) and errs toward attributing
/// module-level constants to the function above them, which is why the scan is
/// only ever pointed at test code.
fn collect(coverage: &mut Coverage, tools: &[&str], text: &str, source: &str) {
    let mut pending_ignore = false;
    let mut ignored = false;
    let mut block = String::new();

    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            continue;
        }
        if trimmed.starts_with("#[ignore") {
            pending_ignore = true;
            continue;
        }
        if is_fn_declaration(trimmed) {
            credit(coverage, tools, &block, ignored, source);
            block.clear();
            ignored = pending_ignore;
            pending_ignore = false;
        }
        block.push_str(line);
        block.push('\n');
    }
    credit(coverage, tools, &block, ignored, source);
}

/// Score one function body and credit every tool it names.
///
/// The whole body is scored at once, not line by line, because the fact that
/// decides the strength — did this test hand its result to KiCAD? — is a
/// property of the function, not of the line that happens to name the tool.
///
/// Calling the arbiter outranks `#[ignore]` rather than being cancelled by it.
/// A test that submits a document to `kicad-cli` *has* to be opt-in, because
/// CI has no KiCAD to run it against (`.github/workflows/ci.yml` installs
/// none); treating it as no evidence would make the strongest proof in the
/// repository unreachable and leave the weakest — our own code agreeing with
/// itself — as the ceiling. What it costs is stated where it is published:
/// these proofs come from suites `gate.ps1` runs on a machine with KiCAD, not
/// from CI.
fn credit(coverage: &mut Coverage, tools: &[&str], block: &str, ignored: bool, source: &str) {
    if block.is_empty() {
        return;
    }
    let proof = if contains(block, LIVE_ARBITER) {
        Proof::Live
    } else if contains(block, ARBITER) {
        Proof::Arbitrated
    } else if ignored {
        Proof::Gated
    } else {
        Proof::Test
    };
    for tool in tools {
        if block.lines().any(|line| mentions(line, tool)) {
            coverage.record(tool, proof, source);
        }
    }
}

fn is_fn_declaration(trimmed: &str) -> bool {
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
    rest.starts_with("fn ")
}

/// A line exercises `tool` if it names it as a string — how a protocol test
/// calls it — or calls its handler directly, which is how a unit test does.
fn mentions(line: &str, tool: &str) -> bool {
    contains(line, &format!("\"{tool}\"")) || contains(line, &format!("handle_{tool}"))
}

/// Substring search that refuses a match glued to an identifier character, so
/// `handle_add_wire` is not a proof for `add_wi`.
fn contains(haystack: &str, needle: &str) -> bool {
    let mut from = 0;
    while let Some(idx) = haystack[from..].find(needle) {
        let start = from + idx;
        let end = start + needle.len();
        let before_ok = start == 0 || !is_ident_char(haystack.as_bytes()[start - 1]);
        let after_ok = end == haystack.len() || !is_ident_char(haystack.as_bytes()[end]);
        if before_ok && after_ok {
            return true;
        }
        from = start + 1;
    }
    false
}

fn is_ident_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Tool names from a benchmark task or probe: `- tool: add_wire`.
fn yaml_tools(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start().trim_start_matches("- ").trim_start();
        if let Some(rest) = trimmed.strip_prefix("tool:") {
            let name = rest.trim().trim_matches('"').trim_matches('\'');
            if !name.is_empty() {
                out.push(name.to_string());
            }
        }
    }
    out
}

fn read_dir_sorted(dir: &Path, extension: &str) -> Vec<(String, String)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<_> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|ext| ext == extension))
        .map(|e| e.path())
        .collect();
    files.sort();
    files
        .into_iter()
        .filter_map(|path| {
            let name = path.file_name()?.to_string_lossy().into_owned();
            let text = std::fs::read_to_string(&path).ok()?;
            Some((name, text))
        })
        .collect()
}

fn read_subdirs_sorted(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_running_test_proves_a_tool() {
        let mut coverage = Coverage::default();
        let text = "#[test]\nfn works() {\n    handle_add_wire(&args, &ctx);\n}\n";
        collect(&mut coverage, &["add_wire"], text, "tests/x.rs");
        assert_eq!(coverage.get("add_wire").proof, Proof::Test);
    }

    #[test]
    fn an_ignored_test_does_not() {
        let mut coverage = Coverage::default();
        let text =
            "#[test]\n#[ignore = \"needs kicad\"]\nfn works() {\n    handle_add_wire(&a, &c);\n}\n";
        collect(&mut coverage, &["add_wire"], text, "tests/x.rs");
        assert_eq!(coverage.get("add_wire").proof, Proof::Gated);
        assert!(!coverage.get("add_wire").proof.is_evidence());
    }

    #[test]
    fn the_ignore_flag_stops_at_the_next_function() {
        let mut coverage = Coverage::default();
        let text = "#[ignore]\nfn gated() {\n  handle_run_erc(x);\n}\n#[test]\nfn open() {\n  handle_add_wire(x);\n}\n";
        collect(&mut coverage, &["run_erc", "add_wire"], text, "tests/x.rs");
        assert_eq!(coverage.get("run_erc").proof, Proof::Gated);
        assert_eq!(coverage.get("add_wire").proof, Proof::Test);
    }

    #[test]
    fn the_strongest_proof_wins_and_the_source_is_stable() {
        let mut coverage = Coverage::default();
        coverage.record("run_erc", Proof::Test, "z.rs");
        coverage.record("run_erc", Proof::Test, "a.rs");
        assert_eq!(coverage.get("run_erc").source.as_deref(), Some("a.rs"));
        coverage.record("run_erc", Proof::Bench, "z.yaml");
        assert_eq!(coverage.get("run_erc").proof, Proof::Bench);
        assert_eq!(coverage.get("run_erc").source.as_deref(), Some("z.yaml"));
    }

    #[test]
    fn a_prefix_is_not_a_mention() {
        assert!(mentions("call(\"add_wire\")", "add_wire"));
        assert!(!mentions("call(\"batch_add_wire\")", "add_wire"));
        assert!(!mentions("handle_add_wire_at(x)", "add_wire"));
    }

    #[test]
    fn a_comment_is_not_a_mention() {
        let mut coverage = Coverage::default();
        let text = "#[test]\nfn works() {\n    // handle_add_wire is next\n}\n";
        collect(&mut coverage, &["add_wire"], text, "tests/x.rs");
        assert_eq!(coverage.get("add_wire").proof, Proof::None);
    }

    #[test]
    fn benchmark_steps_are_read_from_yaml() {
        let text = "steps:\n  - tool: create_project\n    args: {}\n  - tool: run_erc\n";
        assert_eq!(yaml_tools(text), vec!["create_project", "run_erc"]);
    }
}
