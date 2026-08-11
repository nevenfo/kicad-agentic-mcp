//! Static registry mapping toolset names → ToolDef slices.
//!
//! Each toolset module exposes a `tools()` function returning its Vec<ToolDef>.
//! This registry wires them together by name.

use super::ToolsetMeta;
use crate::tools::ToolDef;

/// Toolsets auto-loaded when the server starts.
///
/// Kept minimal so that baseline `tools/list` context stays small. Every token
/// here is paid again on every `tools/list` refresh a `load_toolset` /
/// `load_tools` call triggers, so this list is the most expensive real estate
/// the server owns — not a convenience list.
///
/// - `project` — needed to open / create / save any project
pub static STARTER_KIT: &[&str] = &["project"];

/// Individual tools auto-loaded at startup without their toolset.
///
/// `config` used to be a whole starter toolset: 7 tools, 625 tokens, of which
/// the golden suite calls zero. The two read paths below are what "load the
/// user's preferences at session start" actually needs; the five write and
/// design-rule tools are one `find_capabilities` away and cost 507 tokens on
/// every refresh if they sit here instead.
pub static STARTER_TOOLS: &[&str] = &["load_user_config", "get_effective_config"];

pub static ALL_TOOLSETS: &[ToolsetMeta] = &[
    ToolsetMeta {
        name: "project",
        description: "Create, open, save, snapshot KiCAD projects, and launch the live schematic viewer",
        category: "project",
        tool_count: 6,
    },
    ToolsetMeta {
        name: "sch_components",
        description: "Add, edit, move, rotate, and delete schematic symbols",
        category: "schematic",
        tool_count: 17,
    },
    ToolsetMeta {
        name: "sch_wiring",
        description: "Wires, net labels, power symbols, junctions, no-connects, pin-to-pin connections",
        category: "schematic",
        tool_count: 19,
    },
    ToolsetMeta {
        name: "sch_analysis",
        description: "Net connectivity, pin queries, trace paths, overlap/orphan detection",
        category: "schematic",
        tool_count: 15,
    },
    ToolsetMeta {
        name: "sch_batch",
        description: "Bulk add, edit, delete, and move schematic elements in one call",
        category: "schematic",
        tool_count: 12,
    },
    ToolsetMeta {
        name: "sch_export",
        description: "Export schematic to SVG/PDF/netlist/BOM, run ERC",
        category: "schematic",
        tool_count: 7,
    },
    ToolsetMeta {
        name: "sch_hierarchy",
        description: "Hierarchical sheets: add/edit/move/delete/duplicate a sheet, hierarchy and page-numbering queries, import/add/edit/delete sheet pins, pin/label sync validation",
        category: "schematic",
        tool_count: 12,
    },
    ToolsetMeta {
        name: "pcb_board",
        description: "Board outline, layers, zones, mounting holes, board text, SVG logo import",
        category: "pcb",
        tool_count: 11,
    },
    ToolsetMeta {
        name: "pcb_components",
        description: "Place, move, rotate, align, and duplicate PCB footprints",
        category: "pcb",
        tool_count: 13,
    },
    ToolsetMeta {
        name: "pcb_routing",
        description: "Traces, vias, copper pours, net classes, differential pairs",
        category: "pcb",
        tool_count: 12,
    },
    ToolsetMeta {
        name: "pcb_export",
        description: "Gerber, PDF, SVG, 3D model, pick-and-place, DRC, DXF/GenCAD/IPC-2581/ODB++",
        category: "pcb",
        tool_count: 12,
    },
    ToolsetMeta {
        name: "library",
        description: "Symbol libraries, footprint libraries, search and registration",
        category: "library",
        tool_count: 14,
    },
    ToolsetMeta {
        name: "integration",
        description: "JLCPCB parts database, Freerouting autoroute, datasheet URLs",
        category: "integration",
        tool_count: 9,
    },
    ToolsetMeta {
        name: "verification",
        description: "ERC, DRC, design rules, KiCAD UI control",
        category: "verification",
        tool_count: 8,
    },
    ToolsetMeta {
        name: "config",
        description: "User preferences, project rules, design rules, fab constraints — call load_user_config at session start",
        category: "config",
        tool_count: 7,
    },
    ToolsetMeta {
        name: "design_review",
        description: "AI-powered design audits: decoupling, connections, power rails, DFM, BOM health",
        category: "review",
        tool_count: 6,
    },
    ToolsetMeta {
        name: "templates",
        description: "Reference circuit library: USB-C, LDO, buck converter, STM32, I2C, LED — verified component values",
        category: "templates",
        tool_count: 4,
    },
    ToolsetMeta {
        name: "plan",
        description: "Compile and run a plan: one operation expands to many grid-snapped tool calls, a later one may use an earlier one's result, and a plan that cannot finish is refused before it starts",
        category: "plan",
        tool_count: 2,
    },
    ToolsetMeta {
        name: "task",
        description: "Track an objective across calls: constraints, success criteria, verified facts, failed attempts, evidence — held outside the conversation so a compaction cannot lose them",
        category: "task",
        tool_count: 4,
    },
    ToolsetMeta {
        name: "manufacturing",
        description: "Design-to-fab pipeline: export Gerber+BOM+positions package, validate for fab house, estimate cost",
        category: "manufacturing",
        tool_count: 3,
    },
];

/// Return the ToolDefs for a given toolset name, or None if unknown.
pub fn tools_for(name: &str) -> Option<Vec<ToolDef>> {
    use crate::tools::*;
    match name {
        "project" => Some(project::tools()),
        "sch_components" => Some(sch_components::tools()),
        "sch_wiring" => Some(sch_wiring::tools()),
        "sch_analysis" => Some(sch_analysis::tools()),
        "sch_batch" => Some(sch_batch::tools()),
        "sch_export" => Some(sch_export::tools()),
        "sch_hierarchy" => Some(sch_hierarchy::tools()),
        "pcb_board" => Some(pcb_board::tools()),
        "pcb_components" => Some(pcb_components::tools()),
        "pcb_routing" => Some(pcb_routing::tools()),
        "pcb_export" => Some(pcb_export::tools()),
        "library" => Some(library::tools()),
        "integration" => Some(integration::tools()),
        "verification" => Some(verification::tools()),
        "config" => Some(config::tools()),
        "design_review" => Some(design_review::tools()),
        "templates" => Some(templates::tools()),
        "manufacturing" => Some(manufacturing::tools()),
        "plan" => Some(plan::tools()),
        "task" => Some(task::tools()),
        _ => None,
    }
}
