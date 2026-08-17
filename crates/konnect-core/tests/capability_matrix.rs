//! The capability matrix is generated here, and these tests are what make it
//! worth reading.
//!
//! `docs/capability-matrix.md` is rendered from `konnect_core::capability` plus
//! a scan of this repository's own tests. Three failure modes are guarded
//! against, because each is a way a coverage document quietly becomes fiction:
//!
//! * a tool exists and the matrix never mentions it — silent under-reporting;
//! * the matrix mentions a tool that no longer exists — silent over-reporting;
//! * the committed document drifts from what the code says — the whole point.
//!
//! Regenerate with `KAM_UPDATE_MATRIX=1 cargo test -p konnect-core --test
//! capability_matrix`.

use std::collections::BTreeSet;
use std::path::PathBuf;

use konnect_core::capability::{
    baseline, coverage, is_advisory_tool, render, Adapter, Limitation, Status, ADVISORY_SUFFIX,
    ALL_DOMAINS, MANIFEST, MISSING,
};
use konnect_core::router::registry;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates| crates.parent())
        .expect("crates/konnect-core has a workspace root two levels up")
        .to_path_buf()
}

fn registry_tools() -> BTreeSet<&'static str> {
    let mut names = BTreeSet::new();
    for meta in registry::ALL_TOOLSETS {
        let tools = registry::tools_for(meta.name)
            .unwrap_or_else(|| panic!("toolset '{}' is listed but has no tools", meta.name));
        for tool in tools {
            names.insert(tool.name);
        }
    }
    names
}

fn manifest_tools() -> BTreeSet<&'static str> {
    MANIFEST.iter().map(|capability| capability.tool).collect()
}

/// Every registered tool is classified. Without this the matrix reports on
/// whatever subset someone remembered, and a new toolset lands unmeasured.
#[test]
fn the_manifest_covers_every_registered_tool() {
    let unclassified: Vec<_> = registry_tools()
        .difference(&manifest_tools())
        .copied()
        .collect();
    assert!(
        unclassified.is_empty(),
        "these registered tools are missing from konnect_core::capability::MANIFEST: {unclassified:?}"
    );
}

/// And nothing is classified that does not exist. This is the check that kept
/// `kicad-mcp-pro`'s parity matrix honest and the one a hand-maintained
/// document always loses first.
#[test]
fn the_manifest_names_no_tool_that_does_not_exist() {
    let phantom: Vec<_> = manifest_tools()
        .difference(&registry_tools())
        .copied()
        .collect();
    assert!(
        phantom.is_empty(),
        "these tools are in the manifest and not in the registry: {phantom:?}"
    );
}

#[test]
fn no_tool_is_classified_twice() {
    let mut seen = BTreeSet::new();
    for capability in MANIFEST {
        assert!(
            seen.insert(capability.tool),
            "'{}' appears twice in the manifest",
            capability.tool
        );
    }
    assert_eq!(seen.len(), MANIFEST.len());
}

/// Every limitation says why. A `PARTIAL` with no reason is a status nobody
/// can act on.
#[test]
fn every_limitation_carries_its_reason() {
    for capability in MANIFEST {
        if let Some(reason) = capability.limitation.reason() {
            assert!(
                reason.len() > 20,
                "'{}' has a limitation with no usable reason",
                capability.tool
            );
        }
    }
    for missing in MISSING {
        assert!(
            missing.limitation.reason().is_some_and(|r| r.len() > 20),
            "missing capability '{}' has no usable reason",
            missing.capability
        );
    }
}

/// The rule the whole document rests on: a claim of support requires a proof
/// that runs. Asserted directly so it cannot be softened by an edit to the
/// renderer.
#[test]
fn supported_requires_a_proof_that_runs() {
    for capability in MANIFEST {
        for proof in [coverage::Proof::None, coverage::Proof::Gated] {
            assert_ne!(
                capability.status(proof),
                Status::Supported,
                "'{}' would read SUPPORTED on a proof of '{}'",
                capability.tool,
                proof.label()
            );
        }
    }
}

/// A limitation that is a fact about KiCAD outranks any amount of testing:
/// exercising a GUI-only capability does not create an API for it.
#[test]
fn a_kicad_limitation_survives_a_passing_test() {
    let gui_only = MANIFEST
        .iter()
        .find(|capability| matches!(capability.limitation, Limitation::GuiOnlyNoApi(_)))
        .expect("at least one GUI-only capability is recorded");
    assert_eq!(
        gui_only.status(coverage::Proof::Bench),
        Status::GuiOnlyNoApi
    );
}

/// `GUI_ONLY_NO_API` and `REQUIRES_CUSTOM_KICAD` leave the denominator; nothing
/// else does. This is the arithmetic that separates "we didn't" from "KiCAD
/// can't", so it is pinned rather than left to the renderer.
#[test]
fn only_kicad_limits_leave_the_denominator() {
    for status in [
        Status::Supported,
        Status::Partial,
        Status::Gap,
        Status::ExternalTool,
        Status::NotTested,
    ] {
        assert!(
            status.in_denominator(),
            "{} left the denominator",
            status.label()
        );
    }
    assert!(!Status::GuiOnlyNoApi.in_denominator());
    assert!(!Status::RequiresCustomKiCad.in_denominator());
}

/// Every domain the brief names is either populated or explicitly reported as
/// having nothing — the domains with no tool at all are the ones worth
/// knowing about.
#[test]
fn every_domain_is_reachable_from_the_manifest_or_the_gap_list() {
    for domain in ALL_DOMAINS {
        let has_tool = MANIFEST.iter().any(|c| c.domain == *domain);
        let has_gap = MISSING.iter().any(|m| m.domain == *domain);
        assert!(
            has_tool || has_gap,
            "domain '{}' has neither a tool nor a recorded gap — it is reported as empty with no explanation",
            domain.slug()
        );
    }
}

/// `ipc!` returns "KiCAD must be running with the board loaded" and never
/// falls back to the file engine, so an `Ipc` adapter is a hard GUI
/// dependency. Pinned because the fallback question decides whether the PCB
/// path can ever be benchmarked unattended.
#[test]
fn the_ipc_adapter_is_declared_as_needing_a_gui() {
    assert!(Adapter::Ipc.requires_gui());
    assert!(Adapter::Process.requires_gui());
    assert!(!Adapter::IpcOrSexpr.requires_gui());
    assert!(!Adapter::Cli.requires_gui());
}

/// The document on disk is what the code says it is.
#[test]
fn the_committed_matrix_is_up_to_date() {
    let root = workspace_root();
    let tools: Vec<&str> = MANIFEST.iter().map(|c| c.tool).collect();
    let coverage = coverage::scan(&root, &tools);
    let rendered = render::render(&coverage);

    let path = root.join("docs").join("capability-matrix.md");
    if std::env::var("KAM_UPDATE_MATRIX").is_ok() {
        std::fs::write(&path, &rendered).expect("docs/ is writable");
        return;
    }

    let committed = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "{} is missing ({e}); regenerate with KAM_UPDATE_MATRIX=1",
            path.display()
        )
    });
    assert_eq!(
        committed.replace("\r\n", "\n"),
        rendered.replace("\r\n", "\n"),
        "docs/capability-matrix.md is stale — regenerate with KAM_UPDATE_MATRIX=1 cargo test -p konnect-core --test capability_matrix"
    );
}

/// The MCP `description` an agent actually reads — not just the matrix —
/// carries the advisory caveat, for exactly the tools `is_advisory_tool` (the
/// single source both this test and `router::registry::tools_for` consult)
/// flags in `MANIFEST`. Regression this guards: a fifteenth ADVISORY tool
/// added to the matrix without the registry suffix picking it up, or a
/// non-advisory tool acquiring the caveat by accident.
#[test]
fn advisory_tools_and_only_advisory_tools_carry_the_caveat_in_their_description() {
    let advisory_in_manifest: BTreeSet<&str> = MANIFEST
        .iter()
        .filter(|c| is_advisory_tool(c.tool))
        .map(|c| c.tool)
        .collect();
    assert_eq!(
        advisory_in_manifest.len(),
        15,
        "expected 15 ADVISORY tools, found {}: {advisory_in_manifest:?}",
        advisory_in_manifest.len()
    );

    for meta in registry::ALL_TOOLSETS {
        for tool in registry::tools_for(meta.name).unwrap() {
            let carries_caveat = tool.description.ends_with(ADVISORY_SUFFIX);
            let should = advisory_in_manifest.contains(tool.name);
            assert_eq!(
                carries_caveat, should,
                "'{}': description carries the advisory caveat = {carries_caveat}, \
                 but is_advisory_tool() = {should} — description and manifest disagree",
                tool.name
            );
        }
    }
}

/// The scan finds real proofs in this repository. A silent regression in the
/// scanner would otherwise show up as an honest-looking collapse in coverage.
#[test]
fn the_scan_finds_the_benchmark_and_the_tests() {
    let root = workspace_root();
    let tools: Vec<&str> = MANIFEST.iter().map(|c| c.tool).collect();
    let coverage = coverage::scan(&root, &tools);

    // `create_project` is the first step of every golden task.
    assert_eq!(coverage.get("create_project").proof, coverage::Proof::Bench);
    // `run_erc` is how the golden suite proves a schematic is clean.
    assert_eq!(coverage.get("run_erc").proof, coverage::Proof::Bench);

    let with_proof = MANIFEST
        .iter()
        .filter(|c| coverage.get(c.tool).proof.is_evidence())
        .count();
    assert!(
        with_proof > 40,
        "only {with_proof} tools have a running proof — the scanner is probably broken, not the suite"
    );
}

/// This fork is measured against the baseline every time the matrix renders;
/// the baseline side is a frozen list, and a frozen measurement is a claim
/// until something re-derives it. This test does, from the tree itself.
///
/// It runs in the default gate: the criterion would otherwise be the one
/// number in the document nothing checks.
#[test]
fn the_frozen_baseline_measurement_still_holds() {
    let Some(tree) = extract_baseline() else {
        panic!(
            "commit {} is not in this clone — the baseline measurement cannot be re-derived. \
             Fetch the full history (a shallow clone will not do) and re-run.",
            baseline::BASELINE_COMMIT
        );
    };

    let registered = baseline_registered_tools(&tree);
    assert_eq!(
        registered,
        baseline::BASELINE_TOOLS.iter().copied().collect::<BTreeSet<_>>(),
        "the baseline's registered surface is not what capability::baseline froze"
    );

    let names: Vec<&str> = baseline::BASELINE_TOOLS.to_vec();
    let scanned = coverage::scan(&tree, &names);
    let proved: BTreeSet<&str> = MANIFEST
        .iter()
        .filter(|capability| baseline::is_scored(capability.tool))
        .filter(|capability| {
            capability
                .status(scanned.get(capability.tool).proof)
                .is_covered()
        })
        .map(|capability| capability.tool)
        .collect();
    assert_eq!(
        proved,
        baseline::BASELINE_COVERED
            .iter()
            .copied()
            .collect::<BTreeSet<_>>(),
        "the baseline proves a different set of tools than capability::baseline records"
    );
}

/// Export the baseline commit into a scratch directory, or `None` when this
/// clone does not have the object. Uses `git archive` + `tar` rather than a
/// worktree so the developer's repository state is never touched.
fn extract_baseline() -> Option<PathBuf> {
    let root = workspace_root();
    let out = std::env::temp_dir().join(format!("kam-baseline-{}", &baseline::BASELINE_COMMIT[..7]));
    let archive = out.with_extension("tar");
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).ok()?;

    let archived = std::process::Command::new("git")
        .current_dir(&root)
        .args(["archive", "--format=tar", "--output"])
        .arg(&archive)
        .arg(baseline::BASELINE_COMMIT)
        .status()
        .ok()?;
    if !archived.success() {
        return None;
    }
    let extracted = std::process::Command::new("tar")
        .arg("-xf")
        .arg(&archive)
        .arg("-C")
        .arg(&out)
        .status()
        .ok()?;
    let _ = std::fs::remove_file(&archive);
    extracted.success().then_some(out)
}

/// The tool names the baseline registers, read from its `tool!(` declarations —
/// the same source its own `tool-directory.md` is generated from.
fn baseline_registered_tools(tree: &std::path::Path) -> BTreeSet<&'static str> {
    let dir = tree.join("crates").join("konnect-core").join("src").join("tools");
    let entries = std::fs::read_dir(&dir).unwrap_or_else(|e| {
        panic!("the extracted baseline has no {}: {e}", dir.display())
    });

    let mut names = BTreeSet::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("the extracted file is readable");
        let mut lines = text.lines();
        while let Some(line) = lines.next() {
            if !line.trim_end().ends_with("tool!(") {
                continue;
            }
            let Some(next) = lines.next() else { break };
            let quoted = next.trim().trim_end_matches(',');
            let Some(name) = quoted
                .strip_prefix('"')
                .and_then(|rest| rest.strip_suffix('"'))
            else {
                continue;
            };
            // Borrow the manifest's `'static` name so the sets compare directly;
            // a baseline tool this fork dropped would fail the surface check in
            // `capability::baseline` first.
            if let Some(capability) = MANIFEST.iter().find(|c| c.tool == name) {
                names.insert(capability.tool);
            } else {
                panic!("the baseline registers '{name}' and this fork does not classify it");
            }
        }
    }
    names
}
