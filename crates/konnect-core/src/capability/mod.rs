//! What this server can actually do, per KiCAD domain, and what proves it.
//!
//! The manifest below classifies every registered domain tool by the domain it
//! serves, the backend that runs it, and any limitation that is a fact rather
//! than an intention. It is deliberately *not* a place to claim coverage: a
//! capability is `SUPPORTED` only when [`coverage`] finds an automated proof
//! for it in this repository, and the proof is named in the generated document
//! next to the claim.
//!
//! Three rules keep the number honest:
//!
//! * **Nothing is supported because someone wrote it down.** The status comes
//!   from a scan of the test suite and the golden benchmark; a tool nobody
//!   exercises reads `NOT_TESTED` however good its code looks.
//! * **A test that does not run is not a proof.** `#[ignore]`d tests are
//!   reported as `gated` and do not make a capability `SUPPORTED`.
//! * **What KiCAD cannot do is not our gap.** `GUI_ONLY_NO_API` and
//!   `REQUIRES_CUSTOM_KICAD` are excluded from the coverage denominator, so
//!   the percentage measures what we chose not to do, not what no API exists
//!   for.
//!
//! `docs/capability-matrix.md` is rendered from here by
//! `crates/konnect-core/tests/capability_matrix.rs`, which also fails if the
//! committed document has drifted.

pub mod baseline;
pub mod coverage;
pub mod render;

use std::fmt;

// ─── Domains ─────────────────────────────────────────────────────────────────

/// A KiCAD problem area, used as the row axis of the matrix.
///
/// The first 28 are the domains the project brief requires to be reported on.
/// The rest exist because this server has capabilities that are not KiCAD
/// domains at all (its own task state, its plan compiler, its config).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Domain {
    Project,
    Schematic,
    Symbols,
    Wires,
    Nets,
    Labels,
    Buses,
    Hierarchy,
    Libraries,
    Footprints,
    Pcb,
    Placement,
    Routing,
    Vias,
    Zones,
    Stackup,
    Rules,
    Erc,
    Drc,
    Bom,
    ThreeD,
    Simulation,
    Manufacturing,
    Gerber,
    Drill,
    PickPlace,
    Datasheet,
    Sourcing,
    // Not KiCAD domains — capabilities of the server itself.
    Export,
    Review,
    Config,
    Templates,
    Task,
    Plan,
    Ui,
    Graph,
}

impl Domain {
    pub fn slug(self) -> &'static str {
        use Domain::*;
        match self {
            Project => "project",
            Schematic => "schematic",
            Symbols => "symbols",
            Wires => "wires",
            Nets => "nets",
            Labels => "labels",
            Buses => "buses",
            Hierarchy => "hierarchy",
            Libraries => "libraries",
            Footprints => "footprints",
            Pcb => "pcb",
            Placement => "placement",
            Routing => "routing",
            Vias => "vias",
            Zones => "zones",
            Stackup => "stackup",
            Rules => "rules",
            Erc => "erc",
            Drc => "drc",
            Bom => "bom",
            ThreeD => "3d",
            Simulation => "simulation",
            Manufacturing => "manufacturing",
            Gerber => "gerber",
            Drill => "drill",
            PickPlace => "pick_place",
            Datasheet => "datasheet",
            Sourcing => "sourcing",
            Export => "export",
            Review => "review",
            Config => "config",
            Templates => "templates",
            Task => "task",
            Plan => "plan",
            Ui => "ui",
            Graph => "graph",
        }
    }

    /// Domains the brief names explicitly. Reported separately from the
    /// server's own domains so the headline coverage number answers "how much
    /// of KiCAD", not "how much of everything including our own bookkeeping".
    pub fn is_kicad_domain(self) -> bool {
        !matches!(
            self,
            Domain::Review
                | Domain::Config
                | Domain::Templates
                | Domain::Task
                | Domain::Plan
                | Domain::Ui
                | Domain::Export
                | Domain::Graph
        )
    }
}

impl fmt::Display for Domain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.slug())
    }
}

pub static ALL_DOMAINS: &[Domain] = &[
    Domain::Project,
    Domain::Schematic,
    Domain::Symbols,
    Domain::Wires,
    Domain::Nets,
    Domain::Labels,
    Domain::Buses,
    Domain::Hierarchy,
    Domain::Libraries,
    Domain::Footprints,
    Domain::Pcb,
    Domain::Placement,
    Domain::Routing,
    Domain::Vias,
    Domain::Zones,
    Domain::Stackup,
    Domain::Rules,
    Domain::Erc,
    Domain::Drc,
    Domain::Bom,
    Domain::ThreeD,
    Domain::Simulation,
    Domain::Manufacturing,
    Domain::Gerber,
    Domain::Drill,
    Domain::PickPlace,
    Domain::Datasheet,
    Domain::Sourcing,
    Domain::Export,
    Domain::Review,
    Domain::Config,
    Domain::Templates,
    Domain::Task,
    Domain::Plan,
    Domain::Ui,
    Domain::Graph,
];

// ─── Adapters ────────────────────────────────────────────────────────────────

/// Which backend actually executes a capability.
///
/// This is the "adapter matrix": it makes a fallback observable instead of
/// implicit, and it is the only column that says whether a tool needs a live
/// KiCAD. `Ipc` does — `ipc!` has no file fallback and returns "KiCAD must be
/// running with the board loaded" when the socket is silent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Adapter {
    /// The S-expression document engine (`konnect-sexp`,
    /// `konnect-schematic-editor`): reads and writes project files directly.
    Sexpr,
    /// KiCAD's IPC API over NNG. Requires a running KiCAD with the API
    /// enabled; there is no file fallback.
    Ipc,
    /// IPC when KiCAD is running, the file engine otherwise.
    IpcOrSexpr,
    /// `kicad-cli`, spawned as a subprocess.
    Cli,
    /// In-process: server-owned state, pure computation, or a derived view.
    Internal,
    /// Spawns or inspects the KiCAD GUI process itself.
    Process,
    /// A third party: the JLCPCB parts database, Freerouting, the web.
    External,
}

impl Adapter {
    pub fn label(self) -> &'static str {
        match self {
            Adapter::Sexpr => "sexpr",
            Adapter::Ipc => "ipc",
            Adapter::IpcOrSexpr => "ipc→sexpr",
            Adapter::Cli => "cli",
            Adapter::Internal => "internal",
            Adapter::Process => "process",
            Adapter::External => "external",
        }
    }

    /// Whether a call can only succeed with a KiCAD GUI session up.
    pub fn requires_gui(self) -> bool {
        matches!(self, Adapter::Ipc | Adapter::Process)
    }
}

// ─── Limitations ─────────────────────────────────────────────────────────────

/// A fact about a capability that no amount of testing changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Limitation {
    /// Works as advertised.
    None,
    /// Works, with a stated restriction.
    Partial(&'static str),
    /// We have not built it, or built only part of the underlying capability.
    Gap(&'static str),
    /// KiCAD exposes it only through its GUI.
    GuiOnlyNoApi(&'static str),
    /// Would need a patched KiCAD.
    RequiresCustomKiCad(&'static str),
}

impl Limitation {
    pub fn reason(self) -> Option<&'static str> {
        match self {
            Limitation::None => None,
            Limitation::Partial(r)
            | Limitation::Gap(r)
            | Limitation::GuiOnlyNoApi(r)
            | Limitation::RequiresCustomKiCad(r) => Some(r),
        }
    }
}

// ─── Status ──────────────────────────────────────────────────────────────────

/// The published status of a capability. Derived, never declared: a
/// [`Limitation`] and a [`coverage::Proof`] decide it together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Supported,
    Partial,
    Gap,
    GuiOnlyNoApi,
    RequiresCustomKiCad,
    ExternalTool,
    NotTested,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Status::Supported => "SUPPORTED",
            Status::Partial => "PARTIAL",
            Status::Gap => "GAP",
            Status::GuiOnlyNoApi => "GUI_ONLY_NO_API",
            Status::RequiresCustomKiCad => "REQUIRES_CUSTOM_KICAD",
            Status::ExternalTool => "EXTERNAL_TOOL",
            Status::NotTested => "NOT_TESTED",
        }
    }

    /// Whether the status counts against us. `GUI_ONLY_NO_API` and
    /// `REQUIRES_CUSTOM_KICAD` are KiCAD's limits, not ours, so they leave the
    /// denominator entirely rather than being scored as failures.
    pub fn in_denominator(self) -> bool {
        !matches!(self, Status::GuiOnlyNoApi | Status::RequiresCustomKiCad)
    }

    /// Whether the status counts as covered.
    pub fn is_covered(self) -> bool {
        matches!(self, Status::Supported | Status::ExternalTool)
    }
}

// ─── The manifest ────────────────────────────────────────────────────────────

/// One registered tool, classified.
///
/// The toolset is *not* stored here: it is read from the registry at render
/// time, so the two cannot drift. A test asserts the manifest and the registry
/// name exactly the same tools.
#[derive(Debug, Clone, Copy)]
pub struct Capability {
    pub tool: &'static str,
    pub domain: Domain,
    pub adapter: Adapter,
    pub limitation: Limitation,
}

impl Capability {
    /// Combine the declared limitation with the discovered proof.
    ///
    /// A limitation that is a fact about KiCAD wins outright — an untested
    /// GUI-only capability is still GUI-only. Otherwise no proof means
    /// `NOT_TESTED`, whatever the code does.
    pub fn status(&self, proof: coverage::Proof) -> Status {
        match self.limitation {
            Limitation::GuiOnlyNoApi(_) => return Status::GuiOnlyNoApi,
            Limitation::RequiresCustomKiCad(_) => return Status::RequiresCustomKiCad,
            Limitation::Gap(_) => return Status::Gap,
            _ => {}
        }
        if !proof.is_evidence() {
            return Status::NotTested;
        }
        match self.limitation {
            Limitation::Partial(_) => Status::Partial,
            _ if self.adapter == Adapter::External => Status::ExternalTool,
            _ => Status::Supported,
        }
    }
}

const fn cap(tool: &'static str, domain: Domain, adapter: Adapter) -> Capability {
    Capability {
        tool,
        domain,
        adapter,
        limitation: Limitation::None,
    }
}

const fn cap_lim(
    tool: &'static str,
    domain: Domain,
    adapter: Adapter,
    limitation: Limitation,
) -> Capability {
    Capability {
        tool,
        domain,
        adapter,
        limitation,
    }
}

/// Konnect's own connectivity analysis disagreed with `kicad-cli sch erc` on a
/// schematic that had six unconnected pins (progress.md, E7): it reported zero
/// single-pin nets and zero nets. Every tool that derives connectivity from
/// the document rather than asking KiCAD carries this, so the matrix says
/// which answers are advisory and which are a verdict.
const ADVISORY: Limitation = Limitation::Partial(
    "advisory: connectivity derived in-process, and it has disagreed with kicad-cli ERC (E7) — the verdict comes from run_erc / verify",
);

/// Whether `tool` carries the shared [`ADVISORY`] limitation in [`MANIFEST`].
///
/// This is the single source of truth for "which tools need the advisory
/// caveat": `docs/capability-matrix.md` reads it from `MANIFEST` via
/// [`Capability::status`], and the MCP tool descriptions read it from here
/// (`router::registry::tools_for`, which appends [`ADVISORY_SUFFIX`]). A tool
/// cannot gain or lose the caveat in one place without moving in the other,
/// because both walk the same `MANIFEST` entry.
pub fn is_advisory_tool(tool: &str) -> bool {
    MANIFEST
        .iter()
        .any(|c| c.tool == tool && c.limitation == ADVISORY)
}

/// Caveat appended to an `ADVISORY` tool's MCP `description`, read on every
/// `tools/list` an agent sees — unlike [`ADVISORY`]'s longer reason string,
/// which is archival prose read once when someone opens the matrix. Kept to
/// ~20 tokens: says what to call instead, not just what is wrong.
pub const ADVISORY_SUFFIX: &str = " Advisory: connectivity is derived in-process and has disagreed with kicad-cli ERC. For a verdict, use run_erc.";

/// A heuristic audit, not a validator: useful as a checklist, never as a
/// sign-off.
const HEURISTIC: Limitation = Limitation::Partial(
    "heuristic audit, not a validator — ERC/DRC decide whether a design is sound",
);

/// The `graph_*` tools index only what a document states (`konnect_core::graph`,
/// E7): a `.kicad_pcb` footprint's `net` is a fact copied from the file, but no
/// `.kicad_sch` item is ever given a derived `net` attribute, because this
/// project's own connectivity analysis has previously disagreed with
/// `kicad-cli sch erc` on a real schematic.
const GRAPH_E7: Limitation = Limitation::Partial(
    "indexes only what the documents state — no .kicad_sch item ever carries a derived `net`; \
     the connectivity verdict comes from run_erc, never from this tool (E7)",
);

pub static MANIFEST: &[Capability] = &[
    // ── project ─────────────────────────────────────────────────────────────
    cap("create_project", Domain::Project, Adapter::Sexpr),
    cap("open_project", Domain::Project, Adapter::Ipc),
    cap("save_project", Domain::Project, Adapter::Ipc),
    cap("get_project_info", Domain::Project, Adapter::Sexpr),
    cap("snapshot_project", Domain::Project, Adapter::Internal),
    cap("open_schematic_viewer", Domain::Ui, Adapter::Process),
    // ── sch_components ──────────────────────────────────────────────────────
    cap("create_schematic", Domain::Schematic, Adapter::Sexpr),
    cap("add_schematic_component", Domain::Symbols, Adapter::Sexpr),
    cap("delete_schematic_component", Domain::Symbols, Adapter::Sexpr),
    cap("edit_schematic_component", Domain::Symbols, Adapter::Sexpr),
    cap("get_schematic_component", Domain::Symbols, Adapter::Sexpr),
    cap("list_schematic_components", Domain::Symbols, Adapter::Sexpr),
    cap("move_schematic_component", Domain::Symbols, Adapter::Sexpr),
    cap("rotate_schematic_component", Domain::Symbols, Adapter::Sexpr),
    cap_lim(
        "move_connected",
        Domain::Symbols,
        Adapter::Sexpr,
        Limitation::Partial(
            "moves the symbol only — wire stubs are not stretched, so wires attached to the moved pins are left behind and the connection is lost (J.2.3.2)",
        ),
    ),
    cap("move_region", Domain::Symbols, Adapter::Sexpr),
    cap("annotate_schematic", Domain::Symbols, Adapter::Cli),
    cap("get_schematic_pin_locations", Domain::Symbols, Adapter::Sexpr),
    cap(
        "batch_get_schematic_pin_locations",
        Domain::Symbols,
        Adapter::Sexpr,
    ),
    cap("add_component_annotation", Domain::Schematic, Adapter::Sexpr),
    cap("group_components", Domain::Symbols, Adapter::Sexpr),
    cap("replace_component", Domain::Symbols, Adapter::Sexpr),
    cap("get_schematic_view", Domain::Schematic, Adapter::Cli),
    // ── sch_wiring ──────────────────────────────────────────────────────────
    cap("add_wire", Domain::Wires, Adapter::Sexpr),
    cap("batch_add_wire", Domain::Wires, Adapter::Sexpr),
    cap("delete_schematic_wire", Domain::Wires, Adapter::Sexpr),
    cap("batch_delete_schematic_wire", Domain::Wires, Adapter::Sexpr),
    cap("split_wire_at_point", Domain::Wires, Adapter::Sexpr),
    cap("add_schematic_net_label", Domain::Labels, Adapter::Sexpr),
    cap("delete_schematic_net_label", Domain::Labels, Adapter::Sexpr),
    cap("rotate_schematic_label", Domain::Labels, Adapter::Sexpr),
    cap("move_labels_by_offset", Domain::Labels, Adapter::Sexpr),
    cap("batch_rotate_labels", Domain::Labels, Adapter::Sexpr),
    cap_lim(
        "add_power_symbol",
        Domain::Symbols,
        Adapter::Sexpr,
        Limitation::Partial(
            "does not snap to the 1.27 mm grid (E6): a power symbol placed at a component's nominal coordinate lands 0.33 mm off the pin and ERC reports it unconnected. A plan's `power` operation snaps before calling it; the direct path does not",
        ),
    ),
    cap("add_no_connect", Domain::Nets, Adapter::Sexpr),
    cap("delete_no_connect", Domain::Nets, Adapter::Sexpr),
    cap("batch_delete_no_connect", Domain::Nets, Adapter::Sexpr),
    cap("add_junction", Domain::Wires, Adapter::Sexpr),
    cap("batch_add_junction", Domain::Wires, Adapter::Sexpr),
    cap("connect_to_net", Domain::Nets, Adapter::Sexpr),
    cap("connect_pins", Domain::Nets, Adapter::Sexpr),
    cap("add_schematic_connection", Domain::Nets, Adapter::Sexpr),
    // ── sch_analysis ────────────────────────────────────────────────────────
    cap("list_schematic_wires", Domain::Wires, Adapter::Sexpr),
    cap_lim("list_schematic_nets", Domain::Nets, Adapter::Sexpr, ADVISORY),
    cap("list_schematic_labels", Domain::Labels, Adapter::Sexpr),
    cap_lim("get_net_connections", Domain::Nets, Adapter::Sexpr, ADVISORY),
    cap_lim(
        "get_net_connectivity",
        Domain::Nets,
        Adapter::Sexpr,
        ADVISORY,
    ),
    cap_lim("get_pin_connections", Domain::Nets, Adapter::Sexpr, ADVISORY),
    cap_lim("get_pin_net_name", Domain::Nets, Adapter::Sexpr, ADVISORY),
    cap_lim("get_component_nets", Domain::Nets, Adapter::Sexpr, ADVISORY),
    cap_lim("get_net_components", Domain::Nets, Adapter::Sexpr, ADVISORY),
    cap_lim("trace_from_point", Domain::Nets, Adapter::Sexpr, ADVISORY),
    cap_lim(
        "find_orphan_items",
        Domain::Schematic,
        Adapter::Sexpr,
        ADVISORY,
    ),
    cap_lim("find_shorted_nets", Domain::Nets, Adapter::Sexpr, ADVISORY),
    cap_lim(
        "find_single_pin_nets",
        Domain::Nets,
        Adapter::Sexpr,
        ADVISORY,
    ),
    cap_lim("get_connected_items", Domain::Nets, Adapter::Sexpr, ADVISORY),
    cap(
        "check_schematic_overlaps",
        Domain::Schematic,
        Adapter::Sexpr,
    ),
    // ── sch_batch ───────────────────────────────────────────────────────────
    cap("batch_connect_to_net", Domain::Nets, Adapter::Sexpr),
    cap("batch_place_components", Domain::Symbols, Adapter::Sexpr),
    cap("batch_connect_pins", Domain::Nets, Adapter::Sexpr),
    cap("batch_delete", Domain::Schematic, Adapter::Sexpr),
    cap(
        "bulk_move_schematic_components",
        Domain::Symbols,
        Adapter::Sexpr,
    ),
    cap(
        "batch_edit_schematic_components",
        Domain::Symbols,
        Adapter::Sexpr,
    ),
    cap(
        "batch_delete_schematic_components",
        Domain::Symbols,
        Adapter::Sexpr,
    ),
    cap("connect_passthrough", Domain::Nets, Adapter::Sexpr),
    cap("add_schematic_text", Domain::Schematic, Adapter::Sexpr),
    cap("get_schematic_layout", Domain::Schematic, Adapter::Sexpr),
    cap_lim(
        "validate_wire_connections",
        Domain::Nets,
        Adapter::Sexpr,
        ADVISORY,
    ),
    cap_lim(
        "validate_component_connections",
        Domain::Nets,
        Adapter::Sexpr,
        ADVISORY,
    ),
    // ── sch_export ──────────────────────────────────────────────────────────
    cap("export_schematic_svg", Domain::Export, Adapter::Cli),
    cap("export_schematic_pdf", Domain::Export, Adapter::Cli),
    cap("generate_netlist", Domain::Export, Adapter::Cli),
    cap_lim(
        "export_netlist_summary",
        Domain::Export,
        Adapter::Sexpr,
        ADVISORY,
    ),
    cap("run_erc", Domain::Erc, Adapter::Cli),
    cap("fix_connectivity", Domain::Nets, Adapter::Sexpr),
    cap("export_bom", Domain::Bom, Adapter::Cli),
    // ── sch_hierarchy ───────────────────────────────────────────────────────
    cap("add_hierarchical_sheet", Domain::Hierarchy, Adapter::Sexpr),
    cap("edit_sheet", Domain::Hierarchy, Adapter::Sexpr),
    cap("move_sheet", Domain::Hierarchy, Adapter::Sexpr),
    cap("delete_sheet", Domain::Hierarchy, Adapter::Sexpr),
    cap("duplicate_sheet", Domain::Hierarchy, Adapter::Sexpr),
    cap("get_sheet_hierarchy", Domain::Hierarchy, Adapter::Sexpr),
    cap("renumber_sheet_pages", Domain::Hierarchy, Adapter::Sexpr),
    cap("import_sheet_pins", Domain::Hierarchy, Adapter::Sexpr),
    cap("add_sheet_pin", Domain::Hierarchy, Adapter::Sexpr),
    cap("edit_sheet_pin", Domain::Hierarchy, Adapter::Sexpr),
    cap("delete_sheet_pin", Domain::Hierarchy, Adapter::Sexpr),
    cap("validate_sheet_pins", Domain::Hierarchy, Adapter::Sexpr),
    // ── pcb_board ───────────────────────────────────────────────────────────
    cap("set_board_size", Domain::Pcb, Adapter::IpcOrSexpr),
    cap("get_board_info", Domain::Pcb, Adapter::Sexpr),
    cap("get_board_extents", Domain::Pcb, Adapter::IpcOrSexpr),
    cap("get_layer_list", Domain::Stackup, Adapter::Sexpr),
    cap("add_layer", Domain::Stackup, Adapter::Sexpr),
    cap("set_active_layer", Domain::Stackup, Adapter::Sexpr),
    cap("add_board_outline", Domain::Pcb, Adapter::IpcOrSexpr),
    cap("add_mounting_hole", Domain::Pcb, Adapter::Sexpr),
    cap("add_board_text", Domain::Pcb, Adapter::IpcOrSexpr),
    cap("add_zone", Domain::Zones, Adapter::Sexpr),
    cap("import_svg_logo", Domain::Pcb, Adapter::IpcOrSexpr),
    // ── pcb_components ──────────────────────────────────────────────────────
    cap("place_component", Domain::Placement, Adapter::Ipc),
    cap("move_component", Domain::Placement, Adapter::Ipc),
    cap("rotate_component", Domain::Placement, Adapter::Ipc),
    cap("delete_component", Domain::Placement, Adapter::Ipc),
    cap("edit_component", Domain::Placement, Adapter::Ipc),
    cap("find_component", Domain::Placement, Adapter::Ipc),
    cap("get_component_pads", Domain::Footprints, Adapter::Sexpr),
    cap("get_pad_position", Domain::Footprints, Adapter::Sexpr),
    cap("get_component_list", Domain::Placement, Adapter::Ipc),
    cap("place_component_array", Domain::Placement, Adapter::Ipc),
    cap("align_components", Domain::Placement, Adapter::Ipc),
    cap("duplicate_component", Domain::Placement, Adapter::Ipc),
    cap("get_board_2d_view", Domain::Pcb, Adapter::Cli),
    // ── pcb_routing ─────────────────────────────────────────────────────────
    cap("add_net", Domain::Nets, Adapter::Sexpr),
    cap("route_trace", Domain::Routing, Adapter::Ipc),
    cap("route_pad_to_pad", Domain::Routing, Adapter::Ipc),
    cap("add_via", Domain::Vias, Adapter::Ipc),
    cap("add_copper_pour", Domain::Zones, Adapter::Sexpr),
    cap("delete_trace", Domain::Routing, Adapter::Ipc),
    cap("query_traces", Domain::Routing, Adapter::Ipc),
    cap("get_nets_list", Domain::Nets, Adapter::Ipc),
    cap("modify_trace", Domain::Routing, Adapter::Ipc),
    cap("create_netclass", Domain::Rules, Adapter::Sexpr),
    cap("assign_net_to_class", Domain::Rules, Adapter::Sexpr),
    cap_lim(
        "route_differential_pair",
        Domain::Routing,
        Adapter::Ipc,
        Limitation::Partial(
            "one straight segment per net, offset perpendicular by (gap + width) / 2: no length matching, no skew budget, no impedance target and no vias",
        ),
    ),
    // ── sch_buses ───────────────────────────────────────────────────────────
    cap("add_bus", Domain::Buses, Adapter::Sexpr),
    cap("add_bus_entry", Domain::Buses, Adapter::Sexpr),
    cap("add_bus_alias", Domain::Buses, Adapter::Sexpr),
    cap("list_buses", Domain::Buses, Adapter::Sexpr),
    cap("expand_bus", Domain::Buses, Adapter::Internal),
    // ── pcb_export ──────────────────────────────────────────────────────────
    cap("export_gerber", Domain::Gerber, Adapter::Cli),
    cap("export_drill", Domain::Drill, Adapter::Cli),
    cap("export_pdf", Domain::Export, Adapter::Cli),
    cap("export_svg", Domain::Export, Adapter::Cli),
    cap("export_3d", Domain::ThreeD, Adapter::Cli),
    cap("export_netlist", Domain::Export, Adapter::Cli),
    cap("export_position_file", Domain::PickPlace, Adapter::Cli),
    cap("export_dxf", Domain::Export, Adapter::Cli),
    cap("export_gencad", Domain::Export, Adapter::Cli),
    cap("export_ipc2581", Domain::Export, Adapter::Cli),
    cap("export_odb", Domain::Export, Adapter::Cli),
    cap("refill_zones", Domain::Zones, Adapter::Ipc),
    cap("get_drc_violations", Domain::Drc, Adapter::Cli),
    // ── library ─────────────────────────────────────────────────────────────
    cap("create_footprint", Domain::Footprints, Adapter::Sexpr),
    cap("edit_footprint_pad", Domain::Footprints, Adapter::Sexpr),
    cap("register_footprint_library", Domain::Libraries, Adapter::Sexpr),
    cap("list_footprint_libraries", Domain::Libraries, Adapter::Sexpr),
    cap("create_symbol", Domain::Libraries, Adapter::Sexpr),
    cap("delete_symbol", Domain::Libraries, Adapter::Sexpr),
    cap("list_symbols_in_library", Domain::Libraries, Adapter::Sexpr),
    cap("register_symbol_library", Domain::Libraries, Adapter::Sexpr),
    cap("list_symbol_libraries", Domain::Libraries, Adapter::Sexpr),
    cap("search_symbols", Domain::Libraries, Adapter::Sexpr),
    cap("list_library_footprints", Domain::Footprints, Adapter::Sexpr),
    cap("get_footprint_info", Domain::Footprints, Adapter::Sexpr),
    cap("search_footprints", Domain::Footprints, Adapter::Sexpr),
    cap("get_symbol_info", Domain::Libraries, Adapter::Sexpr),
    // ── integration ─────────────────────────────────────────────────────────
    cap("download_jlcpcb_database", Domain::Sourcing, Adapter::External),
    cap("search_jlcpcb_parts", Domain::Sourcing, Adapter::External),
    cap("get_jlcpcb_part", Domain::Sourcing, Adapter::External),
    cap(
        "suggest_jlcpcb_alternatives",
        Domain::Sourcing,
        Adapter::External,
    ),
    cap(
        "get_jlcpcb_database_stats",
        Domain::Sourcing,
        Adapter::External,
    ),
    cap("enrich_datasheets", Domain::Datasheet, Adapter::Sexpr),
    cap("get_datasheet_url", Domain::Datasheet, Adapter::External),
    cap_lim(
        "autoroute",
        Domain::Routing,
        Adapter::External,
        Limitation::GuiOnlyNoApi(
            "kicad-cli 10 dropped Specctra DSN export and SES import, so the Freerouting round trip exists only in the PCB editor; the handler always fails and names the GUI steps",
        ),
    ),
    cap("check_freerouting", Domain::Routing, Adapter::External),
    // ── verification ────────────────────────────────────────────────────────
    cap("run_drc", Domain::Drc, Adapter::Cli),
    cap("set_design_rules", Domain::Rules, Adapter::Sexpr),
    cap("get_design_rules", Domain::Rules, Adapter::Sexpr),
    cap("check_kicad_ui", Domain::Ui, Adapter::Process),
    cap("launch_kicad_ui", Domain::Ui, Adapter::Process),
    cap("copy_routing_pattern", Domain::Routing, Adapter::Sexpr),
    cap("set_layer_constraints", Domain::Rules, Adapter::Sexpr),
    cap_lim(
        "check_clearance",
        Domain::Drc,
        Adapter::Sexpr,
        Limitation::Partial(
            "geometric clearance computed in-process from the file, against no rule set — kicad-cli DRC is the verdict",
        ),
    ),
    // ── config ──────────────────────────────────────────────────────────────
    cap("load_user_config", Domain::Config, Adapter::Internal),
    cap("save_user_config", Domain::Config, Adapter::Internal),
    cap("load_project_config", Domain::Config, Adapter::Internal),
    cap("save_project_config", Domain::Config, Adapter::Internal),
    cap("get_effective_config", Domain::Config, Adapter::Internal),
    cap("add_design_rule", Domain::Config, Adapter::Internal),
    cap("list_design_rules", Domain::Config, Adapter::Internal),
    // ── design_review ───────────────────────────────────────────────────────
    cap_lim("audit_decoupling", Domain::Review, Adapter::Sexpr, HEURISTIC),
    cap_lim("audit_connections", Domain::Review, Adapter::Sexpr, HEURISTIC),
    cap_lim("audit_power_rails", Domain::Review, Adapter::Sexpr, HEURISTIC),
    cap_lim(
        "audit_manufacturing",
        Domain::Review,
        Adapter::Sexpr,
        HEURISTIC,
    ),
    cap_lim("run_design_review", Domain::Review, Adapter::Sexpr, HEURISTIC),
    cap_lim("check_bom_health", Domain::Review, Adapter::Sexpr, HEURISTIC),
    // ── templates ───────────────────────────────────────────────────────────
    cap("search_templates", Domain::Templates, Adapter::Internal),
    cap("get_template", Domain::Templates, Adapter::Internal),
    cap("apply_template", Domain::Templates, Adapter::Sexpr),
    cap("list_template_categories", Domain::Templates, Adapter::Internal),
    // ── manufacturing ───────────────────────────────────────────────────────
    cap(
        "export_manufacturing_package",
        Domain::Manufacturing,
        Adapter::Cli,
    ),
    cap_lim(
        "validate_for_manufacturing",
        Domain::Manufacturing,
        Adapter::Sexpr,
        HEURISTIC,
    ),
    cap_lim(
        "estimate_cost",
        Domain::Manufacturing,
        Adapter::Internal,
        Limitation::Partial(
            "an order-of-magnitude estimate from stored per-fab-house rates, not a quote",
        ),
    ),
    // ── plan ────────────────────────────────────────────────────────────────
    cap("preview_plan", Domain::Plan, Adapter::Internal),
    cap("apply_plan", Domain::Plan, Adapter::Internal),
    // ── task ────────────────────────────────────────────────────────────────
    cap("start_task", Domain::Task, Adapter::Internal),
    cap("update_task", Domain::Task, Adapter::Internal),
    cap("get_task", Domain::Task, Adapter::Internal),
    cap("list_tasks", Domain::Task, Adapter::Internal),
    // ── graph ───────────────────────────────────────────────────────────────
    cap_lim("graph_query", Domain::Graph, Adapter::Sexpr, GRAPH_E7),
    cap_lim("graph_neighbors", Domain::Graph, Adapter::Sexpr, GRAPH_E7),
    cap_lim("graph_stats", Domain::Graph, Adapter::Sexpr, GRAPH_E7),
];

// ─── Capabilities with no tool at all ────────────────────────────────────────

/// Something a domain needs that no registered tool provides.
///
/// A tool-keyed matrix can only report on tools that exist, which is the
/// classic way a coverage document reads 100 % while missing whole features.
/// These rows are the counterweight: they are found by reading KiCAD's own
/// surface (`kicad-cli` subcommands, the IPC protos) and asking what nothing
/// here calls.
#[derive(Debug, Clone, Copy)]
pub struct MissingCapability {
    pub domain: Domain,
    pub capability: &'static str,
    pub limitation: Limitation,
}

impl MissingCapability {
    pub fn status(&self) -> Status {
        match self.limitation {
            Limitation::GuiOnlyNoApi(_) => Status::GuiOnlyNoApi,
            Limitation::RequiresCustomKiCad(_) => Status::RequiresCustomKiCad,
            Limitation::Partial(_) => Status::Partial,
            _ => Status::Gap,
        }
    }
}

pub static MISSING: &[MissingCapability] = &[
    MissingCapability {
        domain: Domain::Stackup,
        capability: "write the board stackup (material, thickness, dielectric)",
        limitation: Limitation::GuiOnlyNoApi(
            "`UpdateBoardStackup` is declared in KiCad 10's board protos and marked '**not yet implemented**' there (crates/konnect-ipc/proto/board/board_commands.proto, pinned by that crate's stackup_write_is_unimplemented test); the stackup is read-only over IPC and editable only in the GUI",
        ),
    },
    MissingCapability {
        domain: Domain::Schematic,
        capability: "edit a schematic that is open in the KiCad GUI",
        limitation: Limitation::Gap(
            "KiCad 10 registers only GetOpenDocuments on the schematic API (D3), so edits go to the file and the GUI must reload. Upstream lands this in KiCad 11 — the reason this fork does not patch KiCad",
        ),
    },
    MissingCapability {
        domain: Domain::Simulation,
        capability: "run an ngspice simulation and read results",
        limitation: Limitation::GuiOnlyNoApi(
            "kicad-cli 10 has no simulation verb and the IPC protos expose none; only the GUI simulator runs one. `generate_netlist --format spice` produces the input file",
        ),
    },
    MissingCapability {
        domain: Domain::Placement,
        capability: "automatic component placement",
        limitation: Limitation::Gap(
            "placement is caller-driven: every position is a coordinate someone chose. No autoplacer, and none in KiCad either",
        ),
    },
    MissingCapability {
        domain: Domain::Routing,
        capability: "interactive push-and-shove routing",
        limitation: Limitation::GuiOnlyNoApi(
            "the router lives in the PCB editor; IPC creates track segments but does not drive the interactive router",
        ),
    },
    MissingCapability {
        domain: Domain::ThreeD,
        capability: "3D viewer control and rendered board images",
        limitation: Limitation::GuiOnlyNoApi(
            "export_3d writes STEP/GLB/VRML geometry; rendering a picture of the board is the GUI's 3D viewer",
        ),
    },
    MissingCapability {
        domain: Domain::Erc,
        capability: "ERC on a schematic open in the GUI, or incremental ERC",
        limitation: Limitation::Partial(
            "run_erc is a kicad-cli process over the file on disk: ~1.1 s per call and blind to unsaved GUI state",
        ),
    },
];
