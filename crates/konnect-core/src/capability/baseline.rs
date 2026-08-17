//! The comparison target the V1 `CAPABILITY_COVERAGE` criterion is measured
//! against (J.2.1).
//!
//! "`CAPABILITY_COVERAGE` > baseline" is meaningless until *baseline* names a
//! number that was produced the same way ours is. This module fixes it:
//!
//! * **The universe is frozen and inherited.** It is the 187 MCP tools
//!   `mixelpixx/Konnect` v0.2.2 registers at [`BASELINE_COMMIT`]. The fork
//!   still registers all 187 — it has removed none — so the two sides compare
//!   tool-for-tool with no name mapping and no judgement call.
//! * **The denominator cannot move.** It is those 187 minus the ones whose
//!   [`Limitation`](super::Limitation) is a fact about KiCAD
//!   (`GUI_ONLY_NO_API`, `REQUIRES_CUSTOM_KICAD`), which is the same subtraction
//!   the headline coverage makes. Nothing this fork *adds* — its own tools, the
//!   `MISSING` entries — can enter it, so the percentage moves only when a test
//!   that runs starts proving a tool.
//! * **Both numerators come from the same scanner.** [`coverage::scan`] is
//!   pointed at each repository in turn and counts a tool only when a test that
//!   actually runs, or a golden benchmark task, exercises it.
//!
//! [`BASELINE_COVERED`] is the baseline side of that measurement, frozen here
//! because the baseline tree is not checked out in a normal build.
//! `the_frozen_baseline_measurement_still_holds` in
//! `crates/konnect-core/tests/capability_matrix.rs` re-derives both lists from
//! `git archive`, in the default gate, and is what keeps them from becoming a
//! claim. [`BASELINE_COMMIT`] is an ancestor of this history, so it needs
//! nothing but a full clone.
//!
//! The comparison is only met when the fork is ahead *and* has no regression:
//! a tool the baseline proved and this fork does not is a loss the percentage
//! alone would hide, so [`Comparison::regressions`] reports it by name.

use super::coverage::{Coverage, Proof};
use super::MANIFEST;

/// The baseline release, as recorded in `docs/benchmark.md`.
pub const BASELINE_VERSION: &str = "mixelpixx/Konnect v0.2.2";

/// The exact tree the baseline numbers were measured on. An ancestor of this
/// fork's history, so any full clone can re-derive them.
pub const BASELINE_COMMIT: &str = "5cd6454969d2d060ff8c65b480651a4341051eed";

/// Every MCP tool the baseline registers, from its `tool!(…)` declarations.
/// Sorted, so a diff on this list reads as a surface change and nothing else.
pub static BASELINE_TOOLS: &[&str] = &[
    "add_board_outline",
    "add_board_text",
    "add_component_annotation",
    "add_copper_pour",
    "add_design_rule",
    "add_hierarchical_sheet",
    "add_junction",
    "add_layer",
    "add_mounting_hole",
    "add_net",
    "add_no_connect",
    "add_power_symbol",
    "add_schematic_component",
    "add_schematic_connection",
    "add_schematic_net_label",
    "add_schematic_text",
    "add_sheet_pin",
    "add_via",
    "add_wire",
    "add_zone",
    "align_components",
    "annotate_schematic",
    "apply_template",
    "assign_net_to_class",
    "audit_connections",
    "audit_decoupling",
    "audit_manufacturing",
    "audit_power_rails",
    "autoroute",
    "batch_add_junction",
    "batch_add_wire",
    "batch_connect_pins",
    "batch_connect_to_net",
    "batch_delete",
    "batch_delete_no_connect",
    "batch_delete_schematic_components",
    "batch_delete_schematic_wire",
    "batch_edit_schematic_components",
    "batch_get_schematic_pin_locations",
    "batch_place_components",
    "batch_rotate_labels",
    "bulk_move_schematic_components",
    "check_bom_health",
    "check_clearance",
    "check_freerouting",
    "check_kicad_ui",
    "check_schematic_overlaps",
    "connect_passthrough",
    "connect_pins",
    "connect_to_net",
    "copy_routing_pattern",
    "create_footprint",
    "create_netclass",
    "create_project",
    "create_schematic",
    "create_symbol",
    "delete_component",
    "delete_no_connect",
    "delete_schematic_component",
    "delete_schematic_net_label",
    "delete_schematic_wire",
    "delete_sheet",
    "delete_sheet_pin",
    "delete_symbol",
    "delete_trace",
    "download_jlcpcb_database",
    "duplicate_component",
    "duplicate_sheet",
    "edit_component",
    "edit_footprint_pad",
    "edit_schematic_component",
    "edit_sheet",
    "edit_sheet_pin",
    "enrich_datasheets",
    "estimate_cost",
    "export_3d",
    "export_bom",
    "export_dxf",
    "export_gencad",
    "export_gerber",
    "export_ipc2581",
    "export_manufacturing_package",
    "export_netlist",
    "export_netlist_summary",
    "export_odb",
    "export_pdf",
    "export_position_file",
    "export_schematic_pdf",
    "export_schematic_svg",
    "export_svg",
    "find_component",
    "find_orphan_items",
    "find_shorted_nets",
    "find_single_pin_nets",
    "fix_connectivity",
    "generate_netlist",
    "get_board_2d_view",
    "get_board_extents",
    "get_board_info",
    "get_component_list",
    "get_component_nets",
    "get_component_pads",
    "get_connected_items",
    "get_datasheet_url",
    "get_design_rules",
    "get_drc_violations",
    "get_effective_config",
    "get_footprint_info",
    "get_jlcpcb_database_stats",
    "get_jlcpcb_part",
    "get_layer_list",
    "get_net_components",
    "get_net_connections",
    "get_net_connectivity",
    "get_nets_list",
    "get_pad_position",
    "get_pin_connections",
    "get_pin_net_name",
    "get_project_info",
    "get_schematic_component",
    "get_schematic_layout",
    "get_schematic_pin_locations",
    "get_schematic_view",
    "get_sheet_hierarchy",
    "get_symbol_info",
    "get_template",
    "group_components",
    "import_sheet_pins",
    "import_svg_logo",
    "launch_kicad_ui",
    "list_design_rules",
    "list_footprint_libraries",
    "list_library_footprints",
    "list_schematic_components",
    "list_schematic_labels",
    "list_schematic_nets",
    "list_schematic_wires",
    "list_symbol_libraries",
    "list_symbols_in_library",
    "list_template_categories",
    "load_project_config",
    "load_user_config",
    "modify_trace",
    "move_component",
    "move_connected",
    "move_labels_by_offset",
    "move_region",
    "move_schematic_component",
    "move_sheet",
    "open_project",
    "open_schematic_viewer",
    "place_component",
    "place_component_array",
    "query_traces",
    "refill_zones",
    "register_footprint_library",
    "register_symbol_library",
    "renumber_sheet_pages",
    "replace_component",
    "rotate_component",
    "rotate_schematic_component",
    "rotate_schematic_label",
    "route_differential_pair",
    "route_pad_to_pad",
    "route_trace",
    "run_design_review",
    "run_drc",
    "run_erc",
    "save_project",
    "save_project_config",
    "save_user_config",
    "search_footprints",
    "search_jlcpcb_parts",
    "search_symbols",
    "search_templates",
    "set_active_layer",
    "set_board_size",
    "set_design_rules",
    "set_layer_constraints",
    "snapshot_project",
    "split_wire_at_point",
    "suggest_jlcpcb_alternatives",
    "trace_from_point",
    "validate_component_connections",
    "validate_for_manufacturing",
    "validate_sheet_pins",
    "validate_wire_connections",
];

/// The tools the baseline's own repository proves, measured by pointing
/// [`coverage::scan`](super::coverage::scan) at [`BASELINE_COMMIT`]. Frozen
/// because that tree is not present in a build; re-derived by a test rather
/// than trusted.
pub static BASELINE_COVERED: &[&str] = &[
    "add_hierarchical_sheet",
    "add_schematic_component",
    "add_sheet_pin",
    "add_wire",
    "add_zone",
    "batch_connect_pins",
    "batch_delete",
    "batch_delete_no_connect",
    "batch_place_components",
    "connect_pins",
    "create_footprint",
    "create_project",
    "create_schematic",
    "create_symbol",
    "delete_no_connect",
    "delete_sheet",
    "delete_sheet_pin",
    "duplicate_sheet",
    "edit_sheet",
    "edit_sheet_pin",
    "export_dxf",
    "export_gencad",
    "export_ipc2581",
    "export_odb",
    "get_jlcpcb_part",
    "get_project_info",
    "get_schematic_pin_locations",
    "get_sheet_hierarchy",
    "get_symbol_info",
    "import_sheet_pins",
    "import_svg_logo",
    "list_footprint_libraries",
    "list_symbols_in_library",
    "move_labels_by_offset",
    "move_sheet",
    "place_component",
    "renumber_sheet_pages",
    "replace_component",
    "route_trace",
    "search_jlcpcb_parts",
    "split_wire_at_point",
    "validate_sheet_pins",
];

/// One side-by-side measurement of the frozen target.
#[derive(Debug, Clone)]
pub struct Comparison {
    /// Inherited tools that are still scored — the frozen denominator.
    pub denominator: usize,
    /// Of those, the ones the baseline repository proves.
    pub baseline_covered: usize,
    /// Of those, the ones this repository proves.
    pub head_covered: usize,
    /// Tools the baseline proved and this repository does not. Any entry here
    /// fails the criterion however the percentages read.
    pub regressions: Vec<&'static str>,
}

impl Comparison {
    /// Whether the V1 criterion is met: strictly ahead, and nothing lost.
    pub fn is_met(&self) -> bool {
        self.head_covered > self.baseline_covered && self.regressions.is_empty()
    }

    pub fn baseline_percent(&self) -> f64 {
        percent(self.baseline_covered, self.denominator)
    }

    pub fn head_percent(&self) -> f64 {
        percent(self.head_covered, self.denominator)
    }
}

fn percent(covered: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        return 0.0;
    }
    100.0 * covered as f64 / denominator as f64
}

/// Whether `tool` is one of the inherited tools the comparison scores.
///
/// Membership is the frozen list; the exclusion is the manifest's, so a tool
/// KiCAD gives no API for leaves both sides at once. Which side of the
/// denominator a tool falls on never depends on its proof, so `Proof::None`
/// asks the question without a scan.
pub fn is_scored(tool: &str) -> bool {
    BASELINE_TOOLS.contains(&tool)
        && MANIFEST
            .iter()
            .find(|capability| capability.tool == tool)
            .is_some_and(|capability| capability.status(Proof::None).in_denominator())
}

/// Measure this repository against the frozen target, using a scan of it.
pub fn compare(coverage: &Coverage) -> Comparison {
    let mut denominator = 0;
    let mut head_covered = 0;
    let mut proved_here = Vec::new();

    for capability in MANIFEST {
        if !is_scored(capability.tool) {
            continue;
        }
        denominator += 1;
        if capability.status(coverage.get(capability.tool).proof).is_covered() {
            head_covered += 1;
            proved_here.push(capability.tool);
        }
    }

    let regressions = BASELINE_COVERED
        .iter()
        .copied()
        .filter(|tool| is_scored(tool) && !proved_here.contains(tool))
        .collect();

    Comparison {
        denominator,
        baseline_covered: BASELINE_COVERED
            .iter()
            .filter(|tool| is_scored(tool))
            .count(),
        head_covered,
        regressions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The claim the whole comparison rests on: the fork removed none of the
    /// baseline's tools, so the two surfaces are the same 187 names and no
    /// mapping is needed.
    #[test]
    fn every_inherited_tool_is_still_registered() {
        let missing: Vec<&str> = BASELINE_TOOLS
            .iter()
            .copied()
            .filter(|tool| !MANIFEST.iter().any(|c| c.tool == *tool))
            .collect();
        assert!(
            missing.is_empty(),
            "the baseline registers these and this fork no longer classifies them: {missing:?}"
        );
        assert_eq!(BASELINE_TOOLS.len(), 187);
    }

    /// The frozen baseline result can only name inherited tools, and only ones
    /// the comparison actually scores.
    #[test]
    fn the_frozen_baseline_result_stays_inside_the_universe() {
        for tool in BASELINE_COVERED {
            assert!(
                BASELINE_TOOLS.contains(tool),
                "'{tool}' is recorded as proved by the baseline and is not one of its tools"
            );
            assert!(
                is_scored(tool),
                "'{tool}' is counted for the baseline and is outside the denominator"
            );
        }
    }

    /// Both lists are sorted and free of duplicates, so a review diff is
    /// readable and a name cannot be counted twice.
    #[test]
    fn both_lists_are_sorted_and_unique() {
        for list in [BASELINE_TOOLS, BASELINE_COVERED] {
            for pair in list.windows(2) {
                assert!(pair[0] < pair[1], "{:?} is out of order or repeated", pair);
            }
        }
    }

    /// The denominator is the frozen 187 minus what KiCAD gives no API for, and
    /// nothing this fork added can enter it. Pinned because a criterion whose
    /// denominator drifts measures nothing.
    #[test]
    fn the_denominator_is_frozen_and_admits_nothing_new() {
        let scored = MANIFEST.iter().filter(|c| is_scored(c.tool)).count();
        assert_eq!(scored, 186, "the frozen denominator moved");

        let fork_only: Vec<&str> = MANIFEST
            .iter()
            .map(|c| c.tool)
            .filter(|tool| !BASELINE_TOOLS.contains(tool))
            .collect();
        assert!(
            !fork_only.is_empty(),
            "this fork adds tools of its own; if it stops, this test is the wrong shape"
        );
        for tool in fork_only {
            assert!(!is_scored(tool), "'{tool}' is this fork's own and is scored");
        }
    }

    /// A tool the baseline proved and this repository no longer does is a
    /// regression the percentage would hide, so it fails the criterion by
    /// itself.
    #[test]
    fn a_regression_fails_the_criterion_even_when_ahead() {
        let comparison = Comparison {
            denominator: 186,
            baseline_covered: 42,
            head_covered: 55,
            regressions: vec!["add_wire"],
        };
        assert!(!comparison.is_met());
        assert!(Comparison {
            regressions: Vec::new(),
            ..comparison.clone()
        }
        .is_met());
    }

    /// Matching the baseline is not beating it.
    #[test]
    fn a_tie_does_not_meet_the_criterion() {
        let comparison = Comparison {
            denominator: 186,
            baseline_covered: 42,
            head_covered: 42,
            regressions: Vec::new(),
        };
        assert!(!comparison.is_met());
        assert_eq!(format!("{:.1}", comparison.head_percent()), "22.6");
    }
}
