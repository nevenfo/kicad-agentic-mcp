//! kicad-cli subprocess wrapper for KiCAD 10.
//!
//! All exports, ERC, DRC, and annotation operations shell out to kicad-cli.
//! This module provides a typed interface to those commands.
//!
//! VERIFIED against: kicad-cli from KiCAD 10.0 (C:\Program Files\KiCad\10.0\bin\kicad-cli.exe)
//! Commands validated: sch erc, sch export (bom/netlist/pdf/svg), pcb drc,
//!   pcb export (gerbers/drill/pdf/svg/step/vrml/pos/ipcd356/dxf/gencad/ipc2581/odb),
//!   pcb render

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;
use tracing::{debug, info, warn};

/// Extended timeout for long operations (export, ERC, DRC).
const LONG_TIMEOUT: Duration = Duration::from_secs(600);

// ─── Result Types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErcViolation {
    pub severity: String,
    pub description: String,
    pub sheet: Option<String>,
    /// The first item's position, kept for backward compatibility with
    /// callers that only ever cared about "where". Derived from `items[0]`.
    pub pos: Option<ErcPos>,
    /// KiCAD's own rule name (`pin_not_connected`, …). Prose is reworded
    /// between versions; the rule name is what a stable finding id can be
    /// built on, so it is kept rather than folded into the description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    /// Every item KiCAD named for this violation. A `pin_to_pin` conflict
    /// names both pins that conflict; `items[0]` alone loses the pin that
    /// explains the finding.
    #[serde(default)]
    pub items: Vec<ReportItem>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErcPos {
    pub x: f64,
    pub y: f64,
}

/// One entry of a violation's `items` array, shared by ERC and DRC reports
/// (schema `{ description, pos: { x, y }, uuid }`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReportItem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pos: Option<ErcPos>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uuid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DrcViolation {
    pub severity: String,
    pub description: String,
    /// The first item's position, kept for backward compatibility with
    /// callers that only ever cared about "where". Derived from `items[0]`.
    pub pos: Option<ErcPos>,
    /// KiCAD's own rule name (`clearance`, `courtyards_overlap`, …). See
    /// [`ErcViolation::rule`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    /// Which top-level array of the DRC report this came from. A `clearance`
    /// violation and an unrouted net are both "errors", but they are found by
    /// different passes of `kicad-cli`, live under different JSON keys, and a
    /// caller ventilating a report by category needs to tell them apart.
    pub category: DrcCategory,
    /// Every item KiCAD named for this violation. Two `unconnected_items`
    /// entries can share rule, description and first position while naming
    /// different pads on the second item — `items[0]` alone made them
    /// indistinguishable.
    #[serde(default)]
    pub items: Vec<ReportItem>,
}

/// The three top-level arrays `kicad-cli pcb drc --format json` may emit.
///
/// `violations` covers board-geometry rules (clearance, courtyard overlap,
/// …); `unconnected_items` is unrouted copper — nets with a ratsnest line
/// still open, always severity `error`; `schematic_parity` is a netlist
/// mismatch between the PCB and its schematic. All three are siblings in the
/// report, not nested under `violations` — reading only `violations` made
/// open copper invisible to every caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrcCategory {
    Violations,
    UnconnectedItems,
    SchematicParity,
}

impl DrcCategory {
    /// The report's own key for this category.
    #[must_use]
    pub fn json_key(self) -> &'static str {
        match self {
            Self::Violations => "violations",
            Self::UnconnectedItems => "unconnected_items",
            Self::SchematicParity => "schematic_parity",
        }
    }

    const ALL: [Self; 3] = [
        Self::Violations,
        Self::UnconnectedItems,
        Self::SchematicParity,
    ];
}

/// The parsed shape of a `kicad-cli pcb drc --format json` report.
///
/// Each field is `None` when the key is absent from the JSON — not measured —
/// and `Some(vec![])` when the key is present but empty — measured, clean.
/// Collapsing the two would turn "this pass did not run" into "this pass
/// found nothing", which is exactly the false-clean report this type exists
/// to rule out.
#[derive(Debug, Clone, Default)]
pub struct DrcReport {
    pub violations: Option<Vec<DrcViolation>>,
    pub unconnected_items: Option<Vec<DrcViolation>>,
    pub schematic_parity: Option<Vec<DrcViolation>>,
}

impl DrcReport {
    fn category(&self, category: DrcCategory) -> &Option<Vec<DrcViolation>> {
        match category {
            DrcCategory::Violations => &self.violations,
            DrcCategory::UnconnectedItems => &self.unconnected_items,
            DrcCategory::SchematicParity => &self.schematic_parity,
        }
    }

    /// Every finding across all three categories, in report order.
    #[must_use]
    pub fn all(&self) -> Vec<&DrcViolation> {
        DrcCategory::ALL
            .iter()
            .filter_map(|c| self.category(*c).as_ref())
            .flatten()
            .collect()
    }

    /// How many findings, across all categories, are severity `error`.
    #[must_use]
    pub fn error_count(&self) -> usize {
        self.all().iter().filter(|v| v.severity == "error").count()
    }

    /// Categories the report did not include a key for. A validator that
    /// found this non-empty ran an incomplete check and must say so, rather
    /// than let the missing category read as "zero findings".
    #[must_use]
    pub fn missing_categories(&self) -> Vec<DrcCategory> {
        DrcCategory::ALL
            .into_iter()
            .filter(|c| self.category(*c).is_none())
            .collect()
    }
}

// ─── KiCAD CLI Runner ─────────────────────────────────────────────────────────

/// What to say when `kicad-cli` fails.
///
/// Argument errors go to **stdout**, not stderr — `--layers F.Cu --layers B.Cu`
/// prints "Duplicate argument --layers" there and leaves stderr empty
/// (measured on 10.0.3). Reporting stderr alone therefore produced a failure
/// with no message at all, which is the worst kind: the caller cannot tell a
/// rejected argument from a crash. Stdout is trimmed to its first lines
/// because kicad-cli follows the message with its whole usage screen.
fn cli_failure_diagnostics(stdout: &[u8], stderr: &[u8]) -> String {
    let stderr = String::from_utf8_lossy(stderr);
    let stderr = stderr.trim();
    if !stderr.is_empty() {
        return stderr.to_string();
    }
    let stdout = String::from_utf8_lossy(stdout);
    let message: Vec<&str> = stdout
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .take_while(|line| !line.starts_with("Usage:"))
        .take(4)
        .collect();
    if message.is_empty() {
        "no diagnostic on stdout or stderr".to_string()
    } else {
        message.join("; ")
    }
}

/// Run a kicad-cli command with arguments and capture stdout.
async fn run_cli(cli: &str, args: &[&str], timeout_dur: Duration) -> Result<String> {
    info!("[BETA] kicad-cli {} {}", cli, args.join(" "));

    let mut cmd = Command::new(cli);
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());

    let child = cmd
        .spawn()
        .with_context(|| format!("Failed to spawn kicad-cli: {}", cli))?;

    let output = timeout(timeout_dur, child.wait_with_output())
        .await
        .with_context(|| format!("kicad-cli timed out after {:?}", timeout_dur))?
        .with_context(|| "kicad-cli process failed")?;

    if !output.stderr.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        for line in stderr.lines() {
            if line.contains("Error") || line.contains("error") {
                warn!("[BETA] kicad-cli: {}", line);
            } else {
                debug!("[BETA] kicad-cli stderr: {}", line);
            }
        }
    }

    if !output.status.success() {
        anyhow::bail!(
            "kicad-cli exited with {}: {}",
            output.status.code().unwrap_or(-1),
            cli_failure_diagnostics(&output.stdout, &output.stderr)
        );
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

// ─── ERC ─────────────────────────────────────────────────────────────────────

/// Run ERC on a schematic and return parsed violations.
/// KiCAD 10: `sch erc --output <path> --format json <input>`
pub async fn run_erc(cli: &str, schematic: &Path) -> Result<Vec<ErcViolation>> {
    let out_path = schematic.with_extension("erc.json");
    let args = [
        "sch",
        "erc",
        "--output",
        out_path.to_str().unwrap(),
        "--format",
        "json",
        schematic.to_str().unwrap(),
    ];
    run_cli(cli, &args, LONG_TIMEOUT).await?;

    let json_str = tokio::fs::read_to_string(&out_path)
        .await
        .context("ERC output file not found")?;
    let raw: serde_json::Value = serde_json::from_str(&json_str)?;

    let violations = parse_erc_json(&raw);
    let _ = tokio::fs::remove_file(&out_path).await;
    Ok(violations)
}

/// Decode a violation's `items` array (`[{ description, pos: { x, y }, uuid }]`),
/// shared by the ERC and DRC parsers — the shape is identical in both
/// reports, and duplicating this per-parser is how the second item was lost.
fn parse_report_items(v: &serde_json::Value) -> Vec<ReportItem> {
    let Some(items) = v.get("items").and_then(|i| i.as_array()) else {
        return Vec::new();
    };
    items
        .iter()
        .map(|item| ReportItem {
            description: item
                .get("description")
                .and_then(|d| d.as_str())
                .map(String::from),
            pos: item.get("pos").and_then(|p| {
                Some(ErcPos {
                    x: p["x"].as_f64()?,
                    y: p["y"].as_f64()?,
                })
            }),
            uuid: item.get("uuid").and_then(|u| u.as_str()).map(String::from),
        })
        .collect()
}

fn parse_erc_json(raw: &serde_json::Value) -> Vec<ErcViolation> {
    // KiCAD's ERC report (https://schemas.kicad.org/erc.v1.json) nests
    // violations per sheet — { "sheets": [ { "path": …, "violations": […] } ] }
    // — with positions on the affected items. There is no top-level
    // "violations" key (that's the DRC report's shape), so reading one here
    // silently returned zero violations for every schematic.
    let Some(sheets) = raw.get("sheets").and_then(|s| s.as_array()) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for sheet in sheets {
        let sheet_path = sheet.get("path").and_then(|p| p.as_str()).map(String::from);
        let Some(violations) = sheet.get("violations").and_then(|v| v.as_array()) else {
            continue;
        };
        for v in violations {
            let items = parse_report_items(v);
            let first_item = items.first();
            let mut description = v["description"].as_str().unwrap_or("").to_string();
            // The per-item description names the offender ("Symbol R1 Pin 1…")
            // — without it "Pin not connected" is unactionable.
            if let Some(detail) = first_item.and_then(|item| item.description.as_deref()) {
                if !detail.is_empty() {
                    description = format!("{}: {}", description, detail);
                }
            }
            out.push(ErcViolation {
                severity: v["severity"].as_str().unwrap_or("error").to_string(),
                rule: v["type"].as_str().map(str::to_string),
                description,
                sheet: sheet_path.clone(),
                pos: first_item.and_then(|item| item.pos.clone()),
                items,
            });
        }
    }
    out
}

// ─── DRC ─────────────────────────────────────────────────────────────────────

/// Run DRC on a PCB and return the parsed report.
/// KiCAD 10: `pcb drc --output <path> --format json [--refill-zones] <input>`
pub async fn run_drc(cli: &str, pcb: &Path, refill_zones: bool) -> Result<DrcReport> {
    let out_path = pcb.with_extension("drc.json");
    let mut args = vec![
        "pcb",
        "drc",
        "--output",
        out_path.to_str().unwrap(),
        "--format",
        "json",
    ];
    if refill_zones {
        args.push("--refill-zones");
    }
    args.push(pcb.to_str().unwrap());
    run_cli(cli, &args, LONG_TIMEOUT).await?;

    let json_str = tokio::fs::read_to_string(&out_path)
        .await
        .context("DRC output file not found")?;
    let raw: serde_json::Value = serde_json::from_str(&json_str)?;
    let _ = tokio::fs::remove_file(&out_path).await;

    Ok(parse_drc_json(&raw))
}

/// Parse a `kicad-cli pcb drc --format json` report (schema
/// https://schemas.kicad.org/drc.v1.json). `violations`, `unconnected_items`
/// and `schematic_parity` are sibling top-level arrays — none nested under
/// another — each holding entries shaped like `{ description, items: [{
/// description, pos, uuid }], severity, type }`. Positions live on the item,
/// never on the violation itself: KiCAD never writes a violation-level `pos`.
fn parse_drc_json(raw: &serde_json::Value) -> DrcReport {
    let category = |key: &str, category: DrcCategory| -> Option<Vec<DrcViolation>> {
        let arr = raw.get(key)?.as_array()?;
        Some(
            arr.iter()
                .map(|v| {
                    let items = parse_report_items(v);
                    let first_item = items.first();
                    let mut description = v["description"].as_str().unwrap_or("").to_string();
                    // The per-item description names the offender ("Pad 1
                    // [VCC] on R1"); without it "Missing connection between
                    // items" is unactionable.
                    if let Some(detail) = first_item.and_then(|item| item.description.as_deref()) {
                        if !detail.is_empty() {
                            description = format!("{}: {}", description, detail);
                        }
                    }
                    DrcViolation {
                        severity: v["severity"].as_str().unwrap_or("error").to_string(),
                        rule: v["type"].as_str().map(str::to_string),
                        description,
                        pos: first_item.and_then(|item| item.pos.clone()),
                        category,
                        items,
                    }
                })
                .collect(),
        )
    };

    DrcReport {
        violations: category("violations", DrcCategory::Violations),
        unconnected_items: category("unconnected_items", DrcCategory::UnconnectedItems),
        schematic_parity: category("schematic_parity", DrcCategory::SchematicParity),
    }
}

// ─── Annotation ───────────────────────────────────────────────────────────────

/// KiCAD 10: `sch annotate` is NOT in the CLI.
/// We implement annotation ourselves by parsing the schematic and assigning
/// sequential reference designators to unannotated symbols (those with "?" suffix).
pub async fn annotate_schematic(_cli: &str, schematic: &Path) -> Result<()> {
    use std::collections::HashMap;

    let read_path = schematic.to_path_buf();
    let content =
        tokio::task::spawn_blocking(move || konnect_sexp::read_consistent(&read_path)).await??;
    let mut new_content = content.clone();
    let mut counters: HashMap<String, usize> = HashMap::new();

    // First pass: find all existing numbered references to avoid conflicts
    let mut pos = 0;
    while let Some(ref_pos) = new_content[pos..].find("(reference \"") {
        let abs = pos + ref_pos + 12;
        if let Some(end) = new_content[abs..].find('"') {
            let reference = &new_content[abs..abs + end];
            // Extract prefix and number: "R1" → ("R", 1)
            let prefix: String = reference
                .chars()
                .take_while(|c| c.is_alphabetic() || *c == '#')
                .collect();
            let num_str: String = reference.chars().skip(prefix.len()).collect();
            if let Ok(num) = num_str.parse::<usize>() {
                let counter = counters.entry(prefix).or_insert(0);
                if num >= *counter {
                    *counter = num + 1;
                }
            }
        }
        pos = abs + 1;
    }

    // Second pass: replace "?" references with sequential numbers
    let mut replacements: Vec<(usize, usize, String)> = Vec::new();
    pos = 0;
    while let Some(ref_pos) = new_content[pos..].find("(reference \"") {
        let abs = pos + ref_pos + 12;
        if let Some(end) = new_content[abs..].find('"') {
            let reference = &new_content[abs..abs + end];
            if reference.ends_with('?') {
                let prefix = reference.trim_end_matches('?').to_string();
                let counter = counters.entry(prefix.clone()).or_insert(1);
                let new_ref = format!("{}{}", prefix, counter);
                *counter += 1;
                replacements.push((abs, abs + end, new_ref));
            }
        }
        pos = abs + 1;
    }

    // Apply replacements in reverse order to preserve offsets
    for (start, end, new_ref) in replacements.into_iter().rev() {
        new_content.replace_range(start..end, &new_ref);
    }

    if new_content != content {
        let write_path = schematic.to_path_buf();
        tokio::task::spawn_blocking(move || {
            konnect_sexp::write_atomic_if_unchanged(&write_path, &content, &new_content)
        })
        .await??;
    }

    Ok(())
}

// ─── Schematic Export ────────────────────────────────────────────────────────

/// KiCAD 10: `sch export svg --output <dir> <input>`
pub async fn export_schematic_svg(
    cli: &str,
    schematic: &Path,
    output_dir: &Path,
) -> Result<PathBuf> {
    let args = [
        "sch",
        "export",
        "svg",
        "--output",
        output_dir.to_str().unwrap(),
        schematic.to_str().unwrap(),
    ];
    run_cli(cli, &args, LONG_TIMEOUT).await?;
    let stem = schematic.file_stem().unwrap_or_default().to_string_lossy();
    Ok(output_dir.join(format!("{}.svg", stem)))
}

/// KiCAD 10: `sch export pdf --output <path> <input>`
pub async fn export_schematic_pdf(cli: &str, schematic: &Path, output: &Path) -> Result<()> {
    let args = [
        "sch",
        "export",
        "pdf",
        "--output",
        output.to_str().unwrap(),
        schematic.to_str().unwrap(),
    ];
    run_cli(cli, &args, LONG_TIMEOUT).await?;
    Ok(())
}

/// Filtering options for `sch export bom`. `exclude_dnp: false` reproduces
/// kicad-cli's own default (Do-Not-Populate symbols included).
#[derive(Debug, Default, Clone, Copy)]
pub struct BomOptions {
    /// Drop Do-Not-Populate symbols via `--exclude-dnp`.
    pub exclude_dnp: bool,
}

/// Argument vector for the BOM export, factored out so the `--exclude-dnp`
/// flag can be asserted without a kicad-cli on the machine.
fn bom_args<'a>(output: &'a str, schematic: &'a str, options: &BomOptions) -> Vec<&'a str> {
    let mut args = vec!["sch", "export", "bom", "--output", output];
    if options.exclude_dnp {
        args.push("--exclude-dnp");
    }
    args.push(schematic);
    args
}

/// KiCAD 10: `sch export bom --output <path> [--exclude-dnp] <input>`
///
/// Note: v10 BOM does NOT take `--format` — `kicad-cli sch export bom --help`
/// has no such flag. Output is always the fixed
/// `Reference,Value,Footprint,QUANTITY,DNP` CSV-like set (customizable via
/// `--fields`/`--labels`, not exposed here).
pub async fn export_bom(
    cli: &str,
    schematic: &Path,
    output: &Path,
    options: &BomOptions,
) -> Result<()> {
    let args = bom_args(
        output.to_str().unwrap_or(""),
        schematic.to_str().unwrap_or(""),
        options,
    );
    run_cli(cli, &args, LONG_TIMEOUT).await?;
    Ok(())
}

#[cfg(test)]
mod pcb_plot_args_tests {
    use super::*;

    /// KiCad 10 takes one comma-separated `--layers`; repeating the option is
    /// refused with "Duplicate argument --layers", so every layer-filtered
    /// export failed.
    #[test]
    fn layers_are_one_comma_separated_value_not_a_repeated_option() {
        let args = single_file_pcb_export_args("pdf", "/out/b.pdf", "F.Cu,B.Cu", "/b.kicad_pcb");
        assert_eq!(args.iter().filter(|a| **a == "--layers").count(), 1);
        assert!(args.contains(&"F.Cu,B.Cu"));
    }

    /// Without `--mode-single`, `--output` is read as a directory and KiCad
    /// plots one file per layer instead of the file the caller named.
    #[test]
    fn a_single_file_plot_is_actually_requested() {
        for subcommand in ["pdf", "svg"] {
            let args =
                single_file_pcb_export_args(subcommand, "/out/b.out", "F.Cu", "/b.kicad_pcb");
            assert!(
                args.contains(&"--mode-single"),
                "{subcommand} did not ask for a single file: {args:?}"
            );
        }
    }

    /// No layers means "use the board's own plot settings"; `--layers ""`
    /// would ask for nothing at all.
    #[test]
    fn an_empty_layer_list_passes_no_layers_option() {
        let args = single_file_pcb_export_args("svg", "/out/b.svg", "", "/b.kicad_pcb");
        assert!(!args.contains(&"--layers"), "{args:?}");
        assert_eq!(args.last(), Some(&"/b.kicad_pcb"));
    }

    /// kicad-cli prints argument errors on stdout and leaves stderr empty, so
    /// reporting stderr alone produced a failure with no message at all.
    #[test]
    fn an_argument_error_on_stdout_still_reaches_the_caller() {
        let stdout = b"Duplicate argument --layers\nUsage: export pdf [--help] [--output OUTPUT_DIR]\n  -h, --help  Shows help\n";
        let diagnostics = cli_failure_diagnostics(stdout, b"");
        assert_eq!(diagnostics, "Duplicate argument --layers");
    }

    /// stderr still wins when KiCad does use it.
    #[test]
    fn stderr_is_preferred_when_kicad_writes_there() {
        let diagnostics =
            cli_failure_diagnostics(b"some progress chatter\n", b"Fatal: bad board\n");
        assert_eq!(diagnostics, "Fatal: bad board");
    }

    /// Silence on both streams must not read as success-shaped emptiness.
    #[test]
    fn a_silent_failure_says_so_rather_than_returning_nothing() {
        assert_eq!(
            cli_failure_diagnostics(b"", b""),
            "no diagnostic on stdout or stderr"
        );
    }
}

#[cfg(test)]
mod bom_export_tests {
    use super::*;

    /// `exclude_dnp` has been in the `export_bom` schema (default `true`)
    /// since the tool shipped, but the handler never read it and the flag
    /// was never sent to `kicad-cli` — every BOM included DNP parts.
    #[test]
    fn exclude_dnp_is_passed_only_when_asked_for() {
        let on = BomOptions { exclude_dnp: true };
        assert!(bom_args("/out/bom.csv", "/s.kicad_sch", &on).contains(&"--exclude-dnp"));

        let off = BomOptions::default();
        assert!(!bom_args("/out/bom.csv", "/s.kicad_sch", &off).contains(&"--exclude-dnp"));
    }

    /// Defaults must reproduce the previous argv exactly, so a caller that
    /// wants KiCAD's own BOM keeps getting it.
    #[test]
    fn default_options_are_the_bare_kicad_cli_invocation() {
        let args = bom_args("/out/bom.csv", "/s.kicad_sch", &BomOptions::default());
        assert_eq!(
            args,
            [
                "sch",
                "export",
                "bom",
                "--output",
                "/out/bom.csv",
                "/s.kicad_sch"
            ]
        );
    }
}

/// KiCAD 10: `sch export netlist --output <path> --format <fmt> <input>`
/// Valid formats: kicadsexpr, kicadxml, cadstar, orcadpcb2, spice, spicemodel, pads, allegro
pub async fn export_netlist(
    cli: &str,
    schematic: &Path,
    output: &Path,
    format: &str,
) -> Result<()> {
    // Map friendly names to v10 format values
    let lower = format.to_lowercase();
    let v10_format = match lower.as_str() {
        "kicad" | "kicadsexpr" | "sexp" => "kicadsexpr",
        "xml" | "kicadxml" => "kicadxml",
        "spice" => "spice",
        "cadstar" => "cadstar",
        "orcad" | "orcadpcb2" => "orcadpcb2",
        "pads" => "pads",
        "allegro" => "allegro",
        _ => &lower,
    };
    let args = [
        "sch",
        "export",
        "netlist",
        "--output",
        output.to_str().unwrap(),
        "--format",
        v10_format,
        schematic.to_str().unwrap(),
    ];
    run_cli(cli, &args, LONG_TIMEOUT).await?;
    Ok(())
}

// ─── PCB Export ──────────────────────────────────────────────────────────────

/// KiCAD 10: `pcb export gerbers --output <dir> <input>` (PLURAL!)
pub async fn export_gerber(cli: &str, pcb: &Path, output_dir: &Path) -> Result<()> {
    let args = [
        "pcb",
        "export",
        "gerbers",
        "--output",
        output_dir.to_str().unwrap(),
        pcb.to_str().unwrap(),
    ];
    run_cli(cli, &args, LONG_TIMEOUT).await?;
    Ok(())
}

/// Everything `pcb export drill` can be told, with KiCAD 10's own defaults.
///
/// The names are KiCAD's, so a caller reading `kicad-cli pcb export drill
/// --help` finds the same words. Verified against KiCAD 10.0.
#[derive(Debug, Clone)]
pub struct DrillOptions<'a> {
    /// `excellon` (default) or `gerber`.
    pub format: &'a str,
    /// Excellon coordinate units: `mm` (default) or `in`. Ignored by the
    /// Gerber writer, which is always metric.
    pub units: &'a str,
    /// `absolute` (default) or `plot` — the drill origin the coordinates are
    /// measured from.
    pub origin: &'a str,
    /// Write plated and non-plated holes to separate files.
    pub separate_th: bool,
    /// Also write a drill map.
    pub generate_map: bool,
    /// Map format when `generate_map` is set: `pdf` (default), `gerberx2`,
    /// `ps`, `dxf`, or `svg`.
    pub map_format: &'a str,
}

impl Default for DrillOptions<'_> {
    fn default() -> Self {
        DrillOptions {
            format: "excellon",
            units: "mm",
            origin: "absolute",
            separate_th: false,
            generate_map: false,
            map_format: "pdf",
        }
    }
}

/// KiCAD 10: `pcb export drill --output <dir> <input>`
///
/// `--output` is a **directory**, not a file: KiCAD names the files after the
/// board (`<board>.drl`, or `<board>-PTH.drl` / `<board>-NPTH.drl` with
/// `separate_th`, plus `<board>-*-drl_map.<ext>` for a map). Passing a file
/// path makes KiCAD create a directory of that name and write the real file
/// inside it — verified against KiCAD 10.0, and the reason this takes
/// `output_dir`.
///
/// Every option is validated here rather than passed through, so a typo comes
/// back naming the valid values instead of as `kicad-cli` exiting non-zero.
pub async fn export_drill(
    cli: &str,
    pcb: &Path,
    output_dir: &Path,
    options: &DrillOptions<'_>,
) -> Result<()> {
    let format = one_of(options.format, &["excellon", "gerber"], "drill format")?;
    let units = one_of(options.units, &["mm", "in"], "drill units")?;
    let origin = one_of(options.origin, &["absolute", "plot"], "drill origin")?;
    let map_format = one_of(
        options.map_format,
        &["pdf", "gerberx2", "ps", "dxf", "svg"],
        "drill map format",
    )?;

    let mut args = vec![
        "pcb",
        "export",
        "drill",
        "--output",
        output_dir.to_str().unwrap(),
        "--format",
        format,
        "--drill-origin",
        origin,
        "--excellon-units",
        units,
    ];
    if options.separate_th {
        args.push("--excellon-separate-th");
    }
    if options.generate_map {
        args.push("--generate-map");
        args.push("--map-format");
        args.push(map_format);
    }
    args.push(pcb.to_str().unwrap());

    run_cli(cli, &args, LONG_TIMEOUT).await?;
    Ok(())
}

/// Accept `value` if KiCAD does, case-insensitively, and return the spelling
/// `kicad-cli` expects.
fn one_of<'a>(value: &str, valid: &[&'a str], what: &str) -> Result<&'a str> {
    let lower = value.to_lowercase();
    valid
        .iter()
        .find(|candidate| **candidate == lower)
        .copied()
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Unsupported {what}: '{value}'. Valid options: {}",
                valid.join(", ")
            )
        })
}

/// KiCAD 10: `pcb export pdf --output <path> [--layers <layer>]... <input>`
pub async fn export_pdf(cli: &str, pcb: &Path, output: &Path, layers: &[&str]) -> Result<()> {
    let joined = layers.join(",");
    let args = single_file_pcb_export_args(
        "pdf",
        output.to_str().unwrap_or(""),
        &joined,
        pcb.to_str().unwrap_or(""),
    );
    run_cli(cli, &args, LONG_TIMEOUT).await?;
    Ok(())
}

/// Arguments for a `pcb export pdf|svg` that writes **one** file.
///
/// Two things were wrong. `--layers` was pushed once per layer, but KiCad 10
/// takes a single comma-separated value and rejects the repeat outright
/// ("Duplicate argument --layers"), so every layer-filtered PDF or SVG export
/// failed. And `--mode-single` was never passed, so KiCad treated `--output`
/// as a directory and plotted one file per layer instead of the single file
/// the caller named.
fn single_file_pcb_export_args<'a>(
    subcommand: &'a str,
    output: &'a str,
    layers: &'a str,
    pcb: &'a str,
) -> Vec<&'a str> {
    let mut args = vec!["pcb", "export", subcommand, "--output", output];
    // An empty list means "whatever the board's plot settings say"; passing
    // `--layers ""` would ask for nothing at all.
    if !layers.is_empty() {
        args.push("--layers");
        args.push(layers);
    }
    args.push("--mode-single");
    args.push(pcb);
    args
}

/// KiCAD 10: `pcb export svg --output <path> [--layers <layer>]... <input>`
pub async fn export_svg_pcb(cli: &str, pcb: &Path, output: &Path, layers: &[&str]) -> Result<()> {
    let joined = layers.join(",");
    let args = single_file_pcb_export_args(
        "svg",
        output.to_str().unwrap_or(""),
        &joined,
        pcb.to_str().unwrap_or(""),
    );
    run_cli(cli, &args, LONG_TIMEOUT).await?;
    Ok(())
}

/// KiCAD 10: `pcb export <format> --output <path> <input>`
/// Supported 3D formats: step, vrml, glb, brep, stl, ply, stpz, u3d, xao, 3dpdf
pub async fn export_3d(cli: &str, pcb: &Path, output: &Path, format: &str) -> Result<()> {
    let subcommand = match format.to_lowercase().as_str() {
        "step" | "stp" => "step",
        "vrml" | "wrl" => "vrml",
        "glb" | "gltf" => "glb",
        "brep" => "brep",
        "stl" => "stl",
        "ply" => "ply",
        "stpz" => "stpz",
        "u3d" => "u3d",
        "xao" => "xao",
        "3dpdf" | "pdf3d" => "3dpdf",
        other => anyhow::bail!(
            "Unsupported 3D format: '{}'. Supported: step, vrml, glb, brep, stl, ply, stpz, u3d, xao, 3dpdf",
            other
        ),
    };
    let args = vec![
        "pcb",
        "export",
        subcommand,
        "--output",
        output.to_str().unwrap(),
        pcb.to_str().unwrap(),
    ];
    run_cli(cli, &args, LONG_TIMEOUT).await?;
    Ok(())
}

/// KiCAD 10: `pcb export pos --output <path> --format <fmt> <input>`
/// Formats: ascii (default), csv, gerber
pub async fn export_position_file(
    cli: &str,
    pcb: &Path,
    output: &Path,
    format: &str,
) -> Result<()> {
    let args = [
        "pcb",
        "export",
        "pos",
        "--output",
        output.to_str().unwrap(),
        "--format",
        format,
        pcb.to_str().unwrap(),
    ];
    run_cli(cli, &args, LONG_TIMEOUT).await?;
    Ok(())
}

/// KiCAD 10: `pcb export ipcd356 --output <path> <input>`
pub async fn export_ipcd356(cli: &str, pcb: &Path, output: &Path) -> Result<()> {
    let args = [
        "pcb",
        "export",
        "ipcd356",
        "--output",
        output.to_str().unwrap(),
        pcb.to_str().unwrap(),
    ];
    run_cli(cli, &args, LONG_TIMEOUT).await?;
    Ok(())
}

/// KiCAD 10: `pcb export dxf --output <dir> [--layers <csv>] --mode-multi <input>`
///
/// Unlike `pdf`/`svg`, DXF's `--layers` takes a single comma-separated value
/// rather than a repeatable flag, and one file per requested layer is written
/// into `output_dir` (verified against KiCAD 10.0).
pub async fn export_dxf(cli: &str, pcb: &Path, output_dir: &Path, layers: &[&str]) -> Result<()> {
    let output_str = output_dir.to_str().unwrap();
    let pcb_str = pcb.to_str().unwrap();
    let layers_csv = layers.join(",");

    let mut args: Vec<&str> = vec!["pcb", "export", "dxf", "--output", output_str];
    if !layers_csv.is_empty() {
        args.push("--layers");
        args.push(&layers_csv);
    }
    args.push("--mode-multi");
    args.push(pcb_str);

    run_cli(cli, &args, LONG_TIMEOUT).await?;
    Ok(())
}

/// KiCAD 10: `pcb export gencad --output <path> <input>`
pub async fn export_gencad(cli: &str, pcb: &Path, output: &Path) -> Result<()> {
    let args = [
        "pcb",
        "export",
        "gencad",
        "--output",
        output.to_str().unwrap(),
        pcb.to_str().unwrap(),
    ];
    run_cli(cli, &args, LONG_TIMEOUT).await?;
    Ok(())
}

/// KiCAD 10: `pcb export ipc2581 --output <path> --units <mm|in> [--compress] <input>`
pub async fn export_ipc2581(
    cli: &str,
    pcb: &Path,
    output: &Path,
    units: &str,
    compress: bool,
) -> Result<()> {
    let output_str = output.to_str().unwrap();
    let pcb_str = pcb.to_str().unwrap();

    let mut args: Vec<&str> = vec![
        "pcb", "export", "ipc2581", "--output", output_str, "--units", units,
    ];
    if compress {
        args.push("--compress");
    }
    args.push(pcb_str);

    run_cli(cli, &args, LONG_TIMEOUT).await?;
    Ok(())
}

/// KiCAD 10: `pcb export odb --output <path> --units <mm|in> --compression <mode> <input>`
/// Compression modes (verified against KiCAD 10.0): `zip`, `none`, `tgz`.
pub async fn export_odb(
    cli: &str,
    pcb: &Path,
    output: &Path,
    units: &str,
    compression: &str,
) -> Result<()> {
    let args = [
        "pcb",
        "export",
        "odb",
        "--output",
        output.to_str().unwrap(),
        "--units",
        units,
        "--compression",
        compression,
        pcb.to_str().unwrap(),
    ];
    run_cli(cli, &args, LONG_TIMEOUT).await?;
    Ok(())
}

// ─── Render to image ─────────────────────────────────────────────────────────

/// Render schematic to SVG (no bitmap export in KiCAD 10 CLI).
/// KiCAD 10: `sch export svg --output <dir> <input>`
pub async fn render_schematic_svg(cli: &str, schematic: &Path, output: &Path) -> Result<PathBuf> {
    let output_dir = output.parent().unwrap_or(Path::new("."));
    export_schematic_svg(cli, schematic, output_dir).await
}

/// KiCAD 10: `pcb render --output <path> --width <w> --height <h> <input>`
///
/// `pcb render` is the 3-D renderer and takes **no** `--layers`: passing it
/// makes kicad-cli exit non-zero with `Unknown argument: --layers`, which is
/// how this was broken from 1ec5b81 (2026-07-08) through v0.2.0 and v0.2.1 —
/// every call failed, and nothing tested it. Layer-aware 2-D output is
/// `pcb export svg`, tracked separately.
pub async fn render_pcb_png(
    cli: &str,
    pcb: &Path,
    output: &Path,
    width: u32,
    height: u32,
) -> Result<()> {
    let width_str = width.to_string();
    let height_str = height.to_string();
    let args = vec![
        "pcb",
        "render",
        "--output",
        output.to_str().unwrap(),
        "--width",
        &width_str,
        "--height",
        &height_str,
        pcb.to_str().unwrap(),
    ];
    run_cli(cli, &args, LONG_TIMEOUT).await?;
    Ok(())
}

#[cfg(test)]
mod erc_parse_tests {
    use super::*;

    /// Shape produced by `kicad-cli sch erc --format json` (KiCAD 10.0.3,
    /// schema https://schemas.kicad.org/erc.v1.json), trimmed to the fields
    /// the parser touches. Captured from a real run on a 2-resistor divider.
    fn real_report() -> serde_json::Value {
        serde_json::json!({
            "$schema": "https://schemas.kicad.org/erc.v1.json",
            "coordinate_units": "mm",
            "kicad_version": "10.0.3",
            "sheets": [
                {
                    "path": "/",
                    "uuid_path": "/14ad3364-2bf7-4e0f-ab6e-27bd0021e859",
                    "violations": [
                        {
                            "description": "Pin not connected",
                            "items": [
                                {
                                    "description": "Symbol R1 Pin 1 [Passive, Line]",
                                    "pos": { "x": 1.0033, "y": 0.762 },
                                    "uuid": "bf26e4e8-972e-4f6c-8144-fe6b3fdd68ad"
                                }
                            ],
                            "severity": "error",
                            "type": "pin_not_connected"
                        },
                        {
                            "description": "Pin not connected",
                            "items": [
                                {
                                    "description": "Symbol R2 Pin 2 [Passive, Line]",
                                    "pos": { "x": 1.0033, "y": 1.143 },
                                    "uuid": "da98d3c5-aa74-4df3-8151-0d6e1e166975"
                                }
                            ],
                            "severity": "warning",
                            "type": "pin_not_connected"
                        }
                    ]
                }
            ]
        })
    }

    #[test]
    fn parses_violations_nested_under_sheets() {
        let violations = parse_erc_json(&real_report());
        assert_eq!(
            violations.len(),
            2,
            "must flatten sheets[].violations — a top-level 'violations' key does not exist in ERC reports"
        );
        assert_eq!(violations[0].severity, "error");
        assert!(violations[0].description.contains("Pin not connected"));
        assert!(
            violations[0].description.contains("R1"),
            "description should name the offending item"
        );
        assert_eq!(violations[0].sheet.as_deref(), Some("/"));
        let pos = violations[0].pos.as_ref().expect("position from items[0]");
        assert!((pos.x - 1.0033).abs() < 1e-9);
        assert_eq!(violations[1].severity, "warning");
    }

    #[test]
    fn empty_or_alien_reports_yield_no_violations() {
        assert!(parse_erc_json(&serde_json::json!({})).is_empty());
        assert!(parse_erc_json(&serde_json::json!({ "sheets": [] })).is_empty());
        // DRC-shaped input (top-level violations) is not an ERC report.
        assert!(
            parse_erc_json(&serde_json::json!({ "violations": [{ "severity": "error" }] }))
                .is_empty()
        );
    }

    #[test]
    fn a_pin_to_pin_conflict_keeps_both_items() {
        // A `pin_to_pin` conflict names two pins — the one driving and the
        // one it conflicts with. Reading only items[0] loses the pin that
        // explains the finding.
        let report = serde_json::json!({
            "sheets": [{
                "path": "/",
                "violations": [{
                    "description": "Pin conflict",
                    "items": [
                        {
                            "description": "Symbol U1 Pin 1 [Output]",
                            "pos": { "x": 1.0, "y": 2.0 },
                            "uuid": "p1"
                        },
                        {
                            "description": "Symbol U2 Pin 3 [Output]",
                            "pos": { "x": 3.0, "y": 4.0 },
                            "uuid": "p2"
                        }
                    ],
                    "severity": "error",
                    "type": "pin_to_pin"
                }]
            }]
        });
        let violations = parse_erc_json(&report);
        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0].items.len(),
            2,
            "must keep every item, not just items[0]"
        );
        assert_eq!(
            violations[0].items[0].description.as_deref(),
            Some("Symbol U1 Pin 1 [Output]")
        );
        assert_eq!(
            violations[0].items[1].description.as_deref(),
            Some("Symbol U2 Pin 3 [Output]")
        );
        let pos1 = violations[0].items[1]
            .pos
            .as_ref()
            .expect("second item's own position");
        assert!((pos1.x - 3.0).abs() < 1e-9);

        // `pos` remains the first item's position, unchanged behavior.
        let pos0 = violations[0]
            .pos
            .as_ref()
            .expect("pos derived from items[0]");
        assert!((pos0.x - 1.0).abs() < 1e-9);
    }
}

#[cfg(test)]
mod drc_parse_tests {
    use super::*;

    /// Shape produced by `kicad-cli pcb drc --format json` (KiCAD 10.0.3),
    /// captured from a real run on a 2-net unrouted board — the
    /// `unrouted.kicad_pcb` fixture, two SMD resistors on nets VCC/GND with
    /// no traces. Keys, types, severities and coordinates are that run's;
    /// only the prose is given in English, since KiCAD writes descriptions in
    /// the locale it ran under. `violations`, `unconnected_items` and
    /// `schematic_parity` are siblings, not nested under `violations`, and
    /// `pos` lives only on each `items[]` entry — never on the violation
    /// itself.
    fn real_report() -> serde_json::Value {
        serde_json::json!({
            "$schema": "https://schemas.kicad.org/drc.v1.json",
            "coordinate_units": "mm",
            "kicad_version": "10.0.3",
            "violations": [
                {
                    "description": "Footprint 'R_0402_1005Metric' does not match copy in library 'Resistor_SMD'",
                    "items": [
                        {
                            "description": "Footprint R1",
                            "pos": { "x": 100.0, "y": 50.0 },
                            "uuid": "a1"
                        }
                    ],
                    "severity": "warning",
                    "type": "lib_footprint_mismatch"
                },
                {
                    "description": "Footprint 'R_0402_1005Metric' does not match copy in library 'Resistor_SMD'",
                    "items": [
                        {
                            "description": "Footprint R2",
                            "pos": { "x": 110.0, "y": 50.0 },
                            "uuid": "a2"
                        }
                    ],
                    "severity": "warning",
                    "type": "lib_footprint_mismatch"
                }
            ],
            "unconnected_items": [
                {
                    "description": "Missing connection between items",
                    "items": [
                        {
                            "description": "Pad 1 [VCC] of R1 on F.Cu",
                            "pos": { "x": 99.5, "y": 50.0 },
                            "uuid": "b1"
                        },
                        {
                            "description": "Pad 2 [VCC] of R2 on F.Cu",
                            "pos": { "x": 110.5, "y": 50.0 },
                            "uuid": "b2"
                        }
                    ],
                    "severity": "error",
                    "type": "unconnected_items"
                },
                {
                    "description": "Missing connection between items",
                    "items": [
                        {
                            "description": "Pad 2 [GND] of R1 on F.Cu",
                            "pos": { "x": 100.5, "y": 50.0 },
                            "uuid": "c1"
                        },
                        {
                            "description": "Pad 1 [GND] of R2 on F.Cu",
                            "pos": { "x": 109.5, "y": 50.0 },
                            "uuid": "c2"
                        }
                    ],
                    "severity": "error",
                    "type": "unconnected_items"
                }
            ],
            "schematic_parity": []
        })
    }

    #[test]
    fn unconnected_items_are_errors_with_a_position() {
        let report = parse_drc_json(&real_report());
        let unconnected = report
            .unconnected_items
            .as_ref()
            .expect("unconnected_items was present in the report");
        assert_eq!(
            unconnected.len(),
            2,
            "must read the top-level 'unconnected_items' array, not just 'violations'"
        );
        for v in unconnected {
            assert_eq!(v.severity, "error");
            assert_eq!(v.category, DrcCategory::UnconnectedItems);
            let pos = v
                .pos
                .as_ref()
                .expect("position must come from items[0].pos, not a violation-level 'pos' that KiCAD never writes");
            assert!(pos.x > 0.0 || pos.y > 0.0);
            assert!(
                v.description.contains("Pad"),
                "description should name the offending item"
            );
        }
        assert_eq!(
            report.error_count(),
            2,
            "the two unconnected nets are the only errors"
        );
        assert!(report.missing_categories().is_empty(), "all three categories were present in the report, even schematic_parity as an empty array");
    }

    #[test]
    fn unconnected_items_are_distinguished_by_their_second_item() {
        // Both `unconnected_items` entries in the fixture share rule
        // (`unconnected_items`), top-level description ("Missing connection
        // between items") and — for the purpose of this check — could in
        // principle share a first position; only the second item names the
        // other end of the missing connection, so it must survive.
        let report = parse_drc_json(&real_report());
        let unconnected = report.unconnected_items.as_ref().unwrap();
        assert_eq!(unconnected[0].items.len(), 2);
        assert_eq!(unconnected[1].items.len(), 2);
        assert_ne!(
            unconnected[0].items[1].description, unconnected[1].items[1].description,
            "second items must distinguish the two unconnected-items violations"
        );
        let p0 = unconnected[0].items[1].pos.as_ref().unwrap();
        let p1 = unconnected[1].items[1].pos.as_ref().unwrap();
        assert!((p0.x - 110.5).abs() < 1e-9);
        assert!((p1.x - 109.5).abs() < 1e-9);

        // `pos` on the violation stays the first item's position.
        assert_eq!(unconnected[0].pos, unconnected[0].items[0].pos);
        assert_eq!(unconnected[1].pos, unconnected[1].items[0].pos);
    }

    #[test]
    fn a_missing_key_is_not_the_same_as_an_empty_array() {
        // schematic_parity present-but-empty means "measured, clean".
        let report = parse_drc_json(&real_report());
        assert_eq!(report.schematic_parity, Some(Vec::new()));

        // A key genuinely absent from the JSON must be reported as missing,
        // not silently treated as zero findings.
        let partial = parse_drc_json(&serde_json::json!({ "violations": [] }));
        assert_eq!(
            partial.missing_categories(),
            vec![DrcCategory::UnconnectedItems, DrcCategory::SchematicParity]
        );
    }
}
