//! Render the capability matrix as Markdown.
//!
//! The output is deterministic — sorted domains, manifest order inside a
//! domain, one proof source per tool chosen lexicographically — so a diff of
//! `docs/capability-matrix.md` shows a change in capability and never a change
//! in iteration order.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::coverage::{Coverage, Proof};
use super::{Adapter, Capability, Domain, Status, ALL_DOMAINS, MANIFEST, MISSING};

/// Which toolset each tool belongs to, read from the registry rather than
/// stored in the manifest, so the two cannot disagree.
pub fn toolset_index() -> BTreeMap<&'static str, &'static str> {
    let mut index = BTreeMap::new();
    for meta in crate::router::registry::ALL_TOOLSETS {
        if let Some(tools) = crate::router::registry::tools_for(meta.name) {
            for tool in tools {
                index.insert(tool.name, meta.name);
            }
        }
    }
    index
}

#[derive(Default)]
struct Counts {
    total: usize,
    supported: usize,
    external: usize,
    partial: usize,
    not_tested: usize,
    gap: usize,
    out_of_scope: usize,
}

impl Counts {
    fn add(&mut self, status: Status) {
        self.total += 1;
        match status {
            Status::Supported => self.supported += 1,
            Status::ExternalTool => self.external += 1,
            Status::Partial => self.partial += 1,
            Status::NotTested => self.not_tested += 1,
            Status::Gap => self.gap += 1,
            Status::GuiOnlyNoApi | Status::RequiresCustomKiCad => self.out_of_scope += 1,
        }
    }

    fn denominator(&self) -> usize {
        self.total - self.out_of_scope
    }

    fn covered(&self) -> usize {
        self.supported + self.external
    }

    fn percent(&self) -> String {
        let denominator = self.denominator();
        if denominator == 0 {
            return "—".to_string();
        }
        format!(
            "{:.1} %",
            100.0 * self.covered() as f64 / denominator as f64
        )
    }
}

/// Render the whole document.
pub fn render(coverage: &Coverage) -> String {
    let toolsets = toolset_index();
    let mut out = String::new();

    out.push_str(HEADER);

    // ── Per-domain counts ───────────────────────────────────────────────────
    let mut per_domain: BTreeMap<Domain, Counts> = BTreeMap::new();
    for capability in MANIFEST {
        let status = capability.status(coverage.get(capability.tool).proof);
        per_domain.entry(capability.domain).or_default().add(status);
    }
    for missing in MISSING {
        per_domain
            .entry(missing.domain)
            .or_default()
            .add(missing.status());
    }

    let mut kicad = Counts::default();
    let mut server = Counts::default();
    for (domain, counts) in &per_domain {
        let target = if domain.is_kicad_domain() {
            &mut kicad
        } else {
            &mut server
        };
        target.total += counts.total;
        target.supported += counts.supported;
        target.external += counts.external;
        target.partial += counts.partial;
        target.not_tested += counts.not_tested;
        target.gap += counts.gap;
        target.out_of_scope += counts.out_of_scope;
    }

    let _ = writeln!(out, "## Headline\n");
    let _ = writeln!(
        out,
        "| | entries | supported | partial | not tested | gap | KiCAD has no API | coverage |"
    );
    let _ = writeln!(out, "|---|---|---|---|---|---|---|---|");
    row(&mut out, "KiCAD domains", &kicad);
    row(&mut out, "server's own", &server);
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "Coverage is `(supported + external) / (entries − entries KiCAD has no API for)`. \
         An entry is `supported` only when a test that actually runs, or a golden benchmark \
         task, exercises it; the proof is named in the tables below.\n"
    );

    // ── Domain summary ──────────────────────────────────────────────────────
    let _ = writeln!(out, "## By domain\n");
    let _ = writeln!(
        out,
        "| domain | entries | supported | partial | not tested | gap | no API | coverage |"
    );
    let _ = writeln!(out, "|---|---|---|---|---|---|---|---|");
    for domain in ALL_DOMAINS {
        let counts = per_domain.get(domain);
        match counts {
            Some(counts) => {
                let _ = writeln!(
                    out,
                    "| [`{}`](#{}) | {} | {} | {} | {} | {} | {} | {} |",
                    domain.slug(),
                    domain.slug(),
                    counts.total,
                    counts.supported + counts.external,
                    counts.partial,
                    counts.not_tested,
                    counts.gap,
                    counts.out_of_scope,
                    counts.percent()
                );
            }
            None => {
                let _ = writeln!(out, "| `{}` | 0 | 0 | 0 | 0 | 0 | 0 | — |", domain.slug());
            }
        }
    }
    let _ = writeln!(out);

    // ── Adapters ────────────────────────────────────────────────────────────
    let _ = writeln!(out, "## Adapters\n");
    let _ = writeln!(
        out,
        "Which backend actually runs a call, and whether it needs KiCAD open. \
         `ipc` has no file fallback: with no live KiCAD the call fails rather than \
         editing the document, which is why unattended coverage of the PCB path is \
         limited to what `kicad-cli` and the file engine can do.\n"
    );
    let _ = writeln!(out, "| adapter | tools | needs a running KiCAD |");
    let _ = writeln!(out, "|---|---|---|");
    for adapter in [
        Adapter::Sexpr,
        Adapter::Cli,
        Adapter::Ipc,
        Adapter::IpcOrSexpr,
        Adapter::Internal,
        Adapter::External,
        Adapter::Process,
    ] {
        let count = MANIFEST.iter().filter(|c| c.adapter == adapter).count();
        let _ = writeln!(
            out,
            "| `{}` | {} | {} |",
            adapter.label(),
            count,
            if adapter.requires_gui() { "yes" } else { "no" }
        );
    }
    let _ = writeln!(out);

    // ── Per-domain detail ───────────────────────────────────────────────────
    let _ = writeln!(out, "## Detail\n");
    for domain in ALL_DOMAINS {
        let tools: Vec<&Capability> = MANIFEST
            .iter()
            .filter(|capability| capability.domain == *domain)
            .collect();
        let missing: Vec<_> = MISSING
            .iter()
            .filter(|entry| entry.domain == *domain)
            .collect();
        if tools.is_empty() && missing.is_empty() {
            continue;
        }

        let _ = writeln!(out, "### {}\n", domain.slug());

        if !tools.is_empty() {
            let _ = writeln!(
                out,
                "| tool | toolset | adapter | status | proof | evidence | note |"
            );
            let _ = writeln!(out, "|---|---|---|---|---|---|---|");
            for capability in tools {
                let evidence = coverage.get(capability.tool);
                let status = capability.status(evidence.proof);
                let _ = writeln!(
                    out,
                    "| `{}` | `{}` | `{}` | {} | {} | {} | {} |",
                    capability.tool,
                    toolsets.get(capability.tool).copied().unwrap_or("—"),
                    capability.adapter.label(),
                    status.label(),
                    evidence.proof.label(),
                    evidence
                        .source
                        .as_deref()
                        .map(|source| format!("`{source}`"))
                        .unwrap_or_else(|| "—".to_string()),
                    escape(capability.limitation.reason().unwrap_or(""))
                );
            }
            let _ = writeln!(out);
        }

        if !missing.is_empty() {
            let _ = writeln!(out, "Not covered by any tool:\n");
            let _ = writeln!(out, "| capability | status | why |");
            let _ = writeln!(out, "|---|---|---|");
            for entry in missing {
                let _ = writeln!(
                    out,
                    "| {} | {} | {} |",
                    escape(entry.capability),
                    entry.status().label(),
                    escape(entry.limitation.reason().unwrap_or(""))
                );
            }
            let _ = writeln!(out);
        }
    }

    // ── Untested tools, gathered ────────────────────────────────────────────
    let untested: Vec<&Capability> = MANIFEST
        .iter()
        .filter(|capability| {
            capability.status(coverage.get(capability.tool).proof) == Status::NotTested
        })
        .collect();
    let _ = writeln!(out, "## Not tested\n");
    let _ = writeln!(
        out,
        "{} of {} registered tools have no proof that runs. \
         `gated` means a test exists and is `#[ignore]`d — it needs a live KiCAD GUI, \
         its IPC socket, or the installed libraries.\n",
        untested.len(),
        MANIFEST.len()
    );
    if untested.is_empty() {
        let _ = writeln!(out, "None.\n");
    } else {
        let _ = writeln!(out, "| tool | domain | adapter | proof found |");
        let _ = writeln!(out, "|---|---|---|---|");
        for capability in untested {
            let evidence = coverage.get(capability.tool);
            let _ = writeln!(
                out,
                "| `{}` | `{}` | `{}` | {} |",
                capability.tool,
                capability.domain.slug(),
                capability.adapter.label(),
                match evidence.proof {
                    Proof::Gated => format!(
                        "gated — `{}`",
                        evidence.source.as_deref().unwrap_or("unknown")
                    ),
                    _ => "none".to_string(),
                }
            );
        }
        let _ = writeln!(out);
    }

    out
}

fn row(out: &mut String, label: &str, counts: &Counts) {
    let _ = writeln!(
        out,
        "| {} | {} | {} | {} | {} | {} | {} | {} |",
        label,
        counts.total,
        counts.supported + counts.external,
        counts.partial,
        counts.not_tested,
        counts.gap,
        counts.out_of_scope,
        counts.percent()
    );
}

fn escape(text: &str) -> String {
    text.replace('|', "\\|")
}

const HEADER: &str = r#"# Capability matrix

**Generated — do not edit by hand.** Rendered from `konnect_core::capability` by
`crates/konnect-core/tests/capability_matrix.rs`, which fails if this file has
drifted. Regenerate with:

```
KAM_UPDATE_MATRIX=1 cargo test -p konnect-core --test capability_matrix
```

Three rules decide what appears here, and they exist to stop the percentage
from becoming decoration:

* **`SUPPORTED` is discovered, not declared.** The status comes from scanning
  this repository's tests and golden benchmark tasks for something that
  exercises the tool. Code that looks finished and is exercised by nothing
  reads `NOT_TESTED`.
* **A test that does not run is not a proof.** `#[ignore]`d tests — the ones
  needing a live KiCAD GUI or the installed symbol libraries — appear as
  `gated` and never make a capability `SUPPORTED`.
* **What KiCAD has no API for leaves the denominator.** `GUI_ONLY_NO_API` and
  `REQUIRES_CUSTOM_KICAD` are not scored as our failures, so the coverage
  number measures what we chose not to build rather than what cannot be built.

The last section lists what no tool covers at all, because a matrix keyed on
the tools that exist is exactly how a document reports full coverage of a
partial feature set.

The scan recognises a tool by its name in quotes or by a call to
`handle_<tool>`. A tool whose handler is named differently — `route_differential_pair`
is handled by `handle_route_diff_pair` — is therefore under-reported unless a
test names it. That direction is deliberate: a scanner that guesses wide
inflates the number it exists to keep honest.

"#;
