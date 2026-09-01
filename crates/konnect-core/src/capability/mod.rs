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

// ─── Effect ──────────────────────────────────────────────────────────────────

/// Whether a call can leave something behind.
///
/// * [`Effect::Write`] — the call may modify persistent state: a document of
///   the project, a file on disk (an export or a report counts), or the state
///   of the loaded KiCAD application.
/// * [`Effect::Read`] — the call leaves nothing behind. A scratch file the
///   handler deletes before returning is still `Read`, because nothing of it
///   survives the call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    Read,
    Write,
}

impl Effect {
    pub fn label(self) -> &'static str {
        match self {
            Effect::Read => "read",
            Effect::Write => "write",
        }
    }
}

/// The verb a tool name starts with, and what it implies.
///
/// Longest-match is not needed: no entry is a prefix of another. A tool whose
/// verb classifies it wrongly belongs in [`TOOL_EFFECTS`], not here.
const VERB_EFFECTS: &[(&str, Effect)] = &[
    // ── observers ───────────────────────────────────────────────────────────
    ("audit_", Effect::Read),
    ("check_", Effect::Read),
    ("estimate_", Effect::Read),
    ("expand_", Effect::Read),
    ("find_", Effect::Read),
    ("get_", Effect::Read),
    ("graph_", Effect::Read),
    ("list_", Effect::Read),
    ("preview_", Effect::Read),
    ("query_", Effect::Read),
    ("search_", Effect::Read),
    ("suggest_", Effect::Read),
    ("trace_", Effect::Read),
    ("validate_", Effect::Read),
    // ── mutators ────────────────────────────────────────────────────────────
    ("add_", Effect::Write),
    ("align_", Effect::Write),
    ("annotate_", Effect::Write),
    ("apply_", Effect::Write),
    ("assign_", Effect::Write),
    ("batch_", Effect::Write),
    ("bulk_", Effect::Write),
    ("connect_", Effect::Write),
    ("copy_", Effect::Write),
    ("create_", Effect::Write),
    ("delete_", Effect::Write),
    ("download_", Effect::Write),
    ("duplicate_", Effect::Write),
    ("edit_", Effect::Write),
    ("enrich_", Effect::Write),
    ("export_", Effect::Write),
    ("fix_", Effect::Write),
    ("generate_", Effect::Write),
    ("group_", Effect::Write),
    ("import_", Effect::Write),
    ("launch_", Effect::Write),
    ("load_", Effect::Write),
    ("modify_", Effect::Write),
    ("move_", Effect::Write),
    ("open_", Effect::Write),
    ("place_", Effect::Write),
    ("refill_", Effect::Write),
    ("register_", Effect::Write),
    ("renumber_", Effect::Write),
    ("replace_", Effect::Write),
    ("rotate_", Effect::Write),
    ("route_", Effect::Write),
    ("run_", Effect::Write),
    ("save_", Effect::Write),
    ("set_", Effect::Write),
    ("snapshot_", Effect::Write),
    ("split_", Effect::Write),
    ("start_", Effect::Write),
    ("update_", Effect::Write),
];

/// Tools whose verb lies about them. Each entry was decided by reading the
/// handler, not the name; a stale entry is a test failure
/// (`every_exception_names_a_real_tool`).
const TOOL_EFFECTS: &[(&str, Effect)] = &[
    // No verb at all. It drives the Freerouting round trip on the board.
    ("autoroute", Effect::Write),
    // `batch_` is a mutating verb everywhere else; this one only reads pins.
    ("batch_get_schematic_pin_locations", Effect::Read),
    // `get_`, but it runs kicad-cli DRC and writes the report when asked
    // (`tools::pcb_export::handle_get_drc_violations`).
    ("get_drc_violations", Effect::Write),
    // Reads the project config; unlike `load_user_config` it never seeds a file.
    ("load_project_config", Effect::Read),
    // `load_`, but it writes a default user config when none exists
    // (`tools::config::handle_load_user_config`).
    ("load_user_config", Effect::Write),
    // `run_`, but it only aggregates the read-only `audit_*` heuristics.
    ("run_design_review", Effect::Read),
];

/// Look up `tool`'s effect, or `None` when no rule covers it.
///
/// Separate from [`tool_effect`] so a test can tell "classified" from "fell
/// through to the fail-safe" — the fail-safe must never be load-bearing.
fn classify(tool: &str) -> Option<Effect> {
    if let Some((_, effect)) = TOOL_EFFECTS.iter().find(|(name, _)| *name == tool) {
        return Some(*effect);
    }
    VERB_EFFECTS
        .iter()
        .find(|(verb, _)| tool.starts_with(verb))
        .map(|(_, effect)| *effect)
}

/// Whether calling `tool` can change persistent state.
///
/// An unknown tool is [`Effect::Write`]. The two errors are not symmetric:
/// calling a read tool a writer costs a refusal the caller can see and work
/// around, while calling a writer a reader lets a mutation through a context
/// that believed itself safe. The exhaustiveness test keeps this fallback from
/// ever being the answer for a tool in [`MANIFEST`].
pub fn tool_effect(tool: &str) -> Effect {
    classify(tool).unwrap_or(Effect::Write)
}

/// Effect of each always-visible meta-tool
/// (`crates/konnect-core/src/router/meta_tools.rs`), decided by reading its
/// handler — never by [`tool_effect`]'s verb table, which only covers
/// [`MANIFEST`] and would fall back to `Write` for every meta-tool (none of
/// their names carry a verb the table recognises), exactly the false
/// positive that made the `read_only` bench tier unusable for
/// `find_capabilities` and `load_tools`.
///
/// [`Effect`] keeps the meaning [`tool_effect`] gives it: can the call mutate
/// the *project on disk*, the thing `$WORK`'s fingerprint checks
/// independently of this table. A meta-tool that only changes this server's
/// own session state — which tools `tools/list` currently exposes
/// (`load_tools`, `load_toolset`, `unload_toolset`) — moves nothing on disk,
/// so it is `Read` by that measure even though it does mutate *something*.
/// Collapsing that distinction would make `read_only` unusable for exactly
/// the discovery/toolset calls a read-only task has to make.
///
/// Exhaustiveness against `router::meta_tools::META_TOOL_NAMES` (itself
/// generated from the same list that builds `handle_meta_tool`'s dispatch
/// `match`) is asserted by
/// `crates/konnect-core/tests/capability_matrix.rs::every_meta_tool_has_a_declared_effect`.
pub const META_TOOL_EFFECTS: &[(&str, Effect)] = &[
    // ── discovery: ranks or exposes tool names, writes nothing ─────────────
    ("find_capabilities", Effect::Read),
    ("load_tools", Effect::Read),
    ("kicad_describe", Effect::Read),
    ("list_toolboxes", Effect::Read),
    ("load_toolset", Effect::Read),
    ("unload_toolset", Effect::Read),
    ("get_active_toolsets", Effect::Read),
    // ── observability: reads the shared call log / stats ───────────────────
    ("get_recent_calls", Effect::Read),
    ("server_stats", Effect::Read),
    ("changes_since", Effect::Read),
    // Carries an arbitrary batch of inner tool calls by name, including
    // MANIFEST writers (`handle_kicad_invoke`,
    // `router::meta_tools::handle_kicad_invoke`). D57: in gateway mode the
    // audit reads the `tool` field of each *inner* result, not `kicad_invoke`
    // itself, so classifying the envelope `Write` changes nothing about what
    // the audit already sees — verified by grepping the bench's audit path
    // for `kicad_invoke` (there is none; it keys on the batch's `tool`
    // entries).
    ("kicad_invoke", Effect::Write),
    // NO_LLM/ESCALATE and LOCAL without `execute` only touch durable task
    // state (`Supervisor::run`, `kam-runtime/src/lib.rs`). `execute: true`
    // calls `agent_loop::execute`, which applies a compiled Plan IR to the
    // named `document` — a real write, gated by one argument this table
    // cannot see. A tool that *can* write is classified `Write`.
    ("kicad_agent", Effect::Write),
    // Reads or runs kicad-cli ERC/DRC (`VerificationAgent::verify`) and
    // records the verdict in durable task state — no output file is written
    // to the project. `Read` by the disk-mutation measure, even though it is
    // not side-effect-free against task state.
    ("kicad_agent_verify", Effect::Read),
];

/// Effect of a meta-tool, or `None` when `tool` is not one of
/// [`META_TOOL_EFFECTS`]'s names.
///
/// Kept separate from a fallback (unlike [`tool_effect`], which has one by
/// design) so a caller cannot mistake "not a meta-tool" for "classified
/// read": [`render::render`] and the bench both need to tell the two apart.
pub fn meta_tool_effect(tool: &str) -> Option<Effect> {
    META_TOOL_EFFECTS
        .iter()
        .find(|(name, _)| *name == tool)
        .map(|(_, effect)| *effect)
}

// ─── Tool annotations (MCP `tools/list`) ──────────────────────────────────────

/// Tools whose write is irreversible — not simply "writes", but a write
/// neither the batch rollback (D12, `kicad_invoke`'s pre-call [`kam_state::Snapshot`])
/// nor a project snapshot (D.5) can undo.
///
/// Empty today, on purpose: [`kam_state::TRACKED_EXTENSIONS`] covers every
/// design-document suffix a MANIFEST write touches (`kicad_sch`, `kicad_pcb`,
/// `kicad_sym`, `kicad_mod`, …), and `router::batch::discover_roots` derives a
/// snapshot root from *any* argument carrying such a suffix regardless of its
/// key — so even a tool like `delete_symbol`, which edits a library file by
/// its `library_path` argument rather than a `schematic`/`board` one, still
/// gets captured when called through `kicad_invoke`. Every real deletion this
/// crate performs was read (`sch_hierarchy::handle_delete_sheet` preserves the
/// child schematic file; the rest are scratch renders/archives already gone
/// before the caller sees a result) or reduces to editing a tracked file in
/// place. An empty list is that finding, not an oversight — the moment a tool
/// is added whose write reaches outside `TRACKED_EXTENSIONS` or outside any
/// root `discover_roots` can infer, it belongs here, pinned by
/// `destructive_tools_list_is_pinned`.
pub const DESTRUCTIVE_TOOLS: &[&str] = &[];

/// Build the `tools/list` annotation hints for a tool with this [`Effect`],
/// whose name may or may not be in [`DESTRUCTIVE_TOOLS`].
///
/// Only fields that differ from the MCP defaults
/// (`readOnlyHint=false, destructiveHint=true, idempotentHint=false,
/// openWorldHint=true`) are worth sending, except `readOnlyHint`, which is
/// always set so a client that filters on it (the problem K.2 exists to fix)
/// sees an explicit answer for every tool rather than inheriting the default
/// `false`. `destructiveHint` is set for every `Write` tool, including one
/// whose value matches the MCP default of `true`, precisely because
/// [`DESTRUCTIVE_TOOLS`] being empty must never be indistinguishable, on the
/// wire, from `destructiveHint` never having been considered.
#[must_use]
pub fn tool_annotations(effect: Effect, tool: &str) -> crate::mcp::protocol::ToolAnnotations {
    use crate::mcp::protocol::ToolAnnotations;
    match effect {
        Effect::Read => ToolAnnotations {
            read_only_hint: Some(true),
            destructive_hint: None,
            open_world_hint: Some(false),
        },
        Effect::Write => ToolAnnotations {
            read_only_hint: Some(false),
            destructive_hint: Some(DESTRUCTIVE_TOOLS.contains(&tool)),
            open_world_hint: Some(false),
        },
    }
}

// ─── WriteTarget ────────────────────────────────────────────────────────────

/// *What* a [`Effect::Write`] call writes, orthogonal to [`Effect`] itself.
///
/// `Effect` says whether a call can leave something behind at all;
/// `WriteTarget` says whether what it leaves behind is a source of the
/// design or something derived from it. That distinction is what makes
/// [`kam_state::OperatingMode::Manufacturing`] — the design freeze —
/// implementable: a fabrication export writes to disk exactly like a
/// schematic edit does, so "does it write" cannot be the axis that
/// separates them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteTarget {
    /// The call can modify a source document of the design: a `.kicad_sch`,
    /// `.kicad_pcb`, `.kicad_pro`, or a project library. Refused under
    /// `Manufacturing`. The fail-safe: a write tool with no explicit
    /// classification is `DesignDocument`, so a tool added tomorrow is
    /// refused under `Manufacturing` rather than allowed by accident.
    DesignDocument,
    /// The call writes, but never a source document of the design:
    /// fabrication artifacts (gerbers, drill, BOM, position files), reports,
    /// or this server's own durable state (task state, config). Allowed
    /// under `Manufacturing`.
    Derived,
}

/// Tools whose `Effect::Write` is [`WriteTarget::Derived`] — decided by
/// reading each handler, not guessed from its name. Every other tool
/// classified `Write` in [`MANIFEST`] is [`WriteTarget::DesignDocument`] by
/// fail-safe (see [`tool_write_target`]); a stale or dead entry here is a
/// test failure (`every_derived_write_names_a_real_tool`).
///
/// Every `export_*` tool is `Derived` too, but that is handled by a verb
/// rule in [`tool_write_target`] rather than by naming all of them here —
/// there is no MANIFEST tool named `export_*` that is a documented
/// exception to it.
const DERIVED_WRITES: &[&str] = &[
    // Runs kicad-cli and writes only the netlist file the caller asked for,
    // never a project source document.
    "generate_netlist",
    // Runs kicad-cli DRC; the only optional file write is the report at the
    // caller's own `output` path (tools::verification::handle_run_drc).
    "run_drc",
    // Same shape as run_drc, for ERC (tools::sch_export::handle_run_erc).
    "run_erc",
    // Writes timestamped PDF snapshots to a caller-chosen output_dir; never
    // the project's own documents (tools::project::handle_snapshot_project).
    "snapshot_project",
    // Durable task state only (kam-runtime), never a project file.
    "start_task",
    "update_task",
    // Writes `.konnect/project.json` / this server's own user config file —
    // server bookkeeping, not a KiCad document
    // (tools::config::{handle_save_project_config, handle_save_user_config}).
    "save_project_config",
    "save_user_config",
    // Pings the KiCad IPC socket; writes nothing at all
    // (tools::project::handle_open_project).
    "open_project",
    // Spawns the viewer subprocess and reads the schematic to show it;
    // writes no project file (tools::project::handle_open_viewer).
    "open_schematic_viewer",
];

/// Which [`WriteTarget`] `tool`'s [`Effect::Write`] carries.
///
/// Meaningless (and unused) for a tool classified [`Effect::Read`] —
/// callers only consult this after checking `tool_effect(tool) ==
/// Effect::Write`. Fail-safe: a tool not named in [`DERIVED_WRITES`] and
/// not an `export_*` verb is [`WriteTarget::DesignDocument`].
#[must_use]
pub fn tool_write_target(tool: &str) -> WriteTarget {
    if tool.starts_with("export_") || DERIVED_WRITES.contains(&tool) {
        WriteTarget::Derived
    } else {
        WriteTarget::DesignDocument
    }
}

/// [`WriteTarget`] for a meta-tool's [`Effect::Write`]
/// ([`META_TOOL_EFFECTS`]). Both `Write` meta-tools (`kicad_invoke`,
/// `kicad_agent`) can reach a handler that writes a project source document
/// — `kicad_invoke` carries an arbitrary batch of MANIFEST writers, and
/// `kicad_agent`'s `execute: true` path applies a compiled Plan IR to a
/// document — so both are `DesignDocument` by the same fail-safe
/// [`tool_write_target`] uses, with no named exception today.
#[must_use]
pub fn meta_tool_write_target(_tool: &str) -> WriteTarget {
    WriteTarget::DesignDocument
}

// ─── Mode gate ──────────────────────────────────────────────────────────────

/// Whether a call with `effect` (and, when it writes, `write_target`) may
/// run under `mode` (plan.md D.8, D.8.3).
///
/// * [`kam_state::OperatingMode::ReadOnly`] refuses every [`Effect::Write`].
/// * [`kam_state::OperatingMode::Manufacturing`] — the design freeze —
///   refuses a [`Effect::Write`] only when its [`WriteTarget`] is
///   [`WriteTarget::DesignDocument`]; a [`WriteTarget::Derived`] write
///   (a fabrication export, a report, this server's own state) passes.
/// * [`kam_state::OperatingMode::Write`] and
///   [`kam_state::OperatingMode::Experimental`] refuse nothing —
///   `Experimental` is a deliberate alias of `Write`, not a mode with its
///   own rule.
/// * Every mode allows every [`Effect::Read`].
#[must_use]
pub fn mode_allows(
    mode: kam_state::OperatingMode,
    effect: Effect,
    write_target: WriteTarget,
) -> bool {
    match (mode, effect) {
        (kam_state::OperatingMode::ReadOnly, Effect::Write) => false,
        (kam_state::OperatingMode::Manufacturing, Effect::Write) => {
            write_target == WriteTarget::Derived
        }
        _ => true,
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

/// The exact GUI step for `tool`, if [`MANIFEST`] carries it as
/// [`Limitation::GuiOnlyNoApi`] — the single source of truth for what KiCAD
/// exposes only through its own interface (D.6.3).
///
/// Callers building a `MANUAL_STEP_REQUIRED` message must go through this
/// rather than writing their own prose: a hand-written GUI-step description
/// drifts from the manifest the moment either one changes and no test would
/// notice. `None` means `tool` has no `GuiOnlyNoApi` entry — say so, do not
/// invent a step.
#[must_use]
pub fn manual_step_for(tool: &str) -> Option<&'static str> {
    MANIFEST.iter().find_map(|capability| {
        if capability.tool != tool {
            return None;
        }
        match capability.limitation {
            Limitation::GuiOnlyNoApi(reason) => Some(reason),
            _ => None,
        }
    })
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
    cap("move_connected", Domain::Symbols, Adapter::Sexpr),
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
    cap("set_footprint_graphics", Domain::Footprints, Adapter::Sexpr),
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The `Write` fallback in [`tool_effect`] is a safety net, not a
    /// classifier: every tool the server registers must be decided by a verb or
    /// by a named exception, so adding a tool with a new verb fails here rather
    /// than silently becoming a writer.
    #[test]
    fn every_manifest_tool_is_classified() {
        let unclassified: Vec<&str> = MANIFEST
            .iter()
            .map(|c| c.tool)
            .filter(|tool| classify(tool).is_none())
            .collect();
        assert!(
            unclassified.is_empty(),
            "tools reaching the Write fail-safe instead of a rule: {unclassified:?} — \
             add a verb to VERB_EFFECTS or a named entry to TOOL_EFFECTS"
        );
    }

    /// D.6.3: `manual_step_for("autoroute")` must return the exact reason
    /// text carried by `autoroute`'s `Limitation::GuiOnlyNoApi` entry —
    /// `handle_autoroute`'s `MANUAL_STEP_REQUIRED` message is built from
    /// nothing else, so drift here is drift a caller would see.
    #[test]
    fn manual_step_for_reads_the_gui_only_reason() {
        let step = manual_step_for("autoroute").expect("autoroute is GuiOnlyNoApi in MANIFEST");
        assert!(
            step.contains("Specctra"),
            "expected the Freerouting DSN/SES reason, got: {step}"
        );
    }

    /// A tool with no `GuiOnlyNoApi` entry has no manual step to report —
    /// `None`, never a made-up sentence.
    #[test]
    fn manual_step_for_is_none_for_a_supported_tool() {
        assert_eq!(manual_step_for("run_drc"), None);
    }

    /// An exception that names no tool is a claim about a handler nobody can
    /// check any more; it would also hide the fact that the rule it overrides
    /// is now unopposed.
    #[test]
    fn every_exception_names_a_real_tool() {
        let dead: Vec<&str> = TOOL_EFFECTS
            .iter()
            .map(|(tool, _)| *tool)
            .filter(|tool| !MANIFEST.iter().any(|c| c.tool == *tool))
            .collect();
        assert!(
            dead.is_empty(),
            "TOOL_EFFECTS entries matching no MANIFEST tool: {dead:?}"
        );
    }

    /// A verb that is a prefix of another would make [`classify`] depend on the
    /// order of the table.
    #[test]
    fn no_verb_shadows_another() {
        for (a, _) in VERB_EFFECTS {
            for (b, _) in VERB_EFFECTS {
                assert!(
                    a == b || !b.starts_with(a),
                    "verb `{a}` shadows `{b}`: classification would depend on table order"
                );
            }
        }
    }

    #[test]
    fn unknown_tool_is_write() {
        assert_eq!(classify("frobnicate_board"), None);
        assert_eq!(tool_effect("frobnicate_board"), Effect::Write);
    }

    // ─── mode_allows (D.8, D.8.3) ───────────────────────────────────────────

    /// Table-driven over the whole MANIFEST: under `ReadOnly`, every tool
    /// classified `Write` is refused and every tool classified `Read` passes.
    #[test]
    fn read_only_refuses_exactly_the_manifest_writers() {
        for capability in MANIFEST {
            let effect = tool_effect(capability.tool);
            let allowed = mode_allows(
                kam_state::OperatingMode::ReadOnly,
                effect,
                tool_write_target(capability.tool),
            );
            assert_eq!(
                allowed,
                effect == Effect::Read,
                "tool `{}` ({:?}) allowed={} under ReadOnly",
                capability.tool,
                effect,
                allowed
            );
        }
    }

    /// Same rule, over the meta-tool table.
    #[test]
    fn read_only_refuses_exactly_the_write_meta_tools() {
        for (tool, effect) in META_TOOL_EFFECTS {
            let allowed = mode_allows(
                kam_state::OperatingMode::ReadOnly,
                *effect,
                meta_tool_write_target(tool),
            );
            assert_eq!(
                allowed,
                *effect == Effect::Read,
                "meta-tool `{tool}` ({effect:?}) allowed={allowed} under ReadOnly"
            );
        }
    }

    /// Table-driven over the whole MANIFEST: under `Manufacturing`, a write
    /// is refused exactly when its `WriteTarget` is `DesignDocument` — a
    /// `Derived` write (a fabrication export, a report, task state) passes.
    #[test]
    fn manufacturing_refuses_exactly_the_design_document_writers() {
        for capability in MANIFEST {
            let effect = tool_effect(capability.tool);
            let target = tool_write_target(capability.tool);
            let allowed = mode_allows(kam_state::OperatingMode::Manufacturing, effect, target);
            let expected = effect == Effect::Read || target == WriteTarget::Derived;
            assert_eq!(
                allowed, expected,
                "tool `{}` ({:?}, {:?}) allowed={} under Manufacturing",
                capability.tool, effect, target, allowed
            );
        }
    }

    /// No `export_*` tool in `MANIFEST` is ever classified `DesignDocument`
    /// — a fabrication export must never be refused under `Manufacturing`.
    #[test]
    fn no_export_tool_is_a_design_document_write() {
        for capability in MANIFEST {
            if capability.tool.starts_with("export_") {
                assert_eq!(
                    tool_write_target(capability.tool),
                    WriteTarget::Derived,
                    "`{}` starts with export_ but is classified DesignDocument",
                    capability.tool
                );
            }
        }
    }

    /// A `DERIVED_WRITES` entry matching no `MANIFEST` tool is a claim about
    /// a handler nobody can check any more.
    #[test]
    fn every_derived_write_names_a_real_tool() {
        let dead: Vec<&str> = DERIVED_WRITES
            .iter()
            .copied()
            .filter(|tool| !MANIFEST.iter().any(|c| c.tool == *tool))
            .collect();
        assert!(
            dead.is_empty(),
            "DERIVED_WRITES entries matching no MANIFEST tool: {dead:?}"
        );
    }

    /// `Write` (the default) changes nothing relative to today's behaviour:
    /// every tool, read or write, passes the gate, whatever its `WriteTarget`.
    #[test]
    fn write_mode_allows_everything() {
        for capability in MANIFEST {
            assert!(mode_allows(
                kam_state::OperatingMode::Write,
                tool_effect(capability.tool),
                tool_write_target(capability.tool)
            ));
        }
        for (tool, effect) in META_TOOL_EFFECTS {
            assert!(mode_allows(
                kam_state::OperatingMode::Write,
                *effect,
                meta_tool_write_target(tool)
            ));
        }
    }

    /// `Experimental` is a deliberate alias of `Write`: this pins that every
    /// tool and every `WriteTarget` behaves identically under both, so a
    /// silent divergence fails a named test.
    #[test]
    fn experimental_behaves_like_write() {
        for effect in [Effect::Read, Effect::Write] {
            for target in [WriteTarget::DesignDocument, WriteTarget::Derived] {
                assert_eq!(
                    mode_allows(kam_state::OperatingMode::Experimental, effect, target),
                    mode_allows(kam_state::OperatingMode::Write, effect, target)
                );
            }
        }
    }

    /// `Manufacturing` is strictly between `ReadOnly` and `Write`: it must
    /// never allow what `ReadOnly` refuses (every `Write`, `Read` is
    /// unaffected) and must never refuse what `Write` allows for a `Derived`
    /// target — pinning the ordering `ReadOnly < Manufacturing < Write`
    /// itself, not just the per-tool table above.
    #[test]
    fn manufacturing_sits_strictly_between_read_only_and_write() {
        for effect in [Effect::Read, Effect::Write] {
            for target in [WriteTarget::DesignDocument, WriteTarget::Derived] {
                let read_only = mode_allows(kam_state::OperatingMode::ReadOnly, effect, target);
                let manufacturing =
                    mode_allows(kam_state::OperatingMode::Manufacturing, effect, target);
                let write = mode_allows(kam_state::OperatingMode::Write, effect, target);
                assert!(
                    read_only <= manufacturing && manufacturing <= write,
                    "effect={effect:?} target={target:?}: read_only={read_only} \
                     manufacturing={manufacturing} write={write}"
                );
            }
        }
    }

    /// Pins [`DESTRUCTIVE_TOOLS`]'s content so a future change to it is a
    /// visible diff in this test, not a silent widening or shrinking of what
    /// `tools/list` calls irreversible.
    #[test]
    fn destructive_tools_list_is_pinned() {
        assert_eq!(DESTRUCTIVE_TOOLS, &[] as &[&str]);
    }

    /// A read tool emits exactly `readOnlyHint` and `openWorldHint`, both
    /// present, nothing else — asserted on the serialized JSON, not the
    /// struct, so an extra field would fail here even if `Option` still
    /// compared equal to `None` elsewhere.
    #[test]
    fn read_tool_annotations_serialize_to_exactly_two_fields() {
        let annotations = tool_annotations(Effect::Read, "get_something");
        let value = serde_json::to_value(annotations).unwrap();
        assert_eq!(
            value,
            serde_json::json!({"readOnlyHint": true, "openWorldHint": false})
        );
    }

    /// A non-destructive write tool emits all three chosen fields.
    #[test]
    fn write_tool_annotations_serialize_to_exactly_three_fields() {
        let annotations = tool_annotations(Effect::Write, "add_something");
        let value = serde_json::to_value(annotations).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "readOnlyHint": false,
                "destructiveHint": false,
                "openWorldHint": false
            })
        );
    }

    /// Every meta-tool listed in `tools/list` carries annotations, and the
    /// discovery/gateway tools that a headless task must be able to call
    /// without a human in the loop are `readOnlyHint: true`, while the tools
    /// that actually apply a plan are `readOnlyHint: false`.
    #[test]
    fn every_meta_tool_carries_annotations() {
        let tools = crate::router::meta_tools::meta_tool_descriptions();
        assert_eq!(
            tools.len(),
            crate::router::meta_tools::META_TOOL_NAMES.len()
        );
        for tool in &tools {
            assert!(
                tool.annotations.is_some(),
                "meta-tool '{}' has no annotations",
                tool.name
            );
        }
        let read_only_hint = |name: &str| -> bool {
            tools
                .iter()
                .find(|t| t.name == name)
                .unwrap()
                .annotations
                .unwrap()
                .read_only_hint
                .unwrap()
        };
        assert!(read_only_hint("find_capabilities"));
        assert!(read_only_hint("load_tools"));
        assert!(!read_only_hint("kicad_invoke"));
        assert!(!read_only_hint("kicad_agent"));
    }
}
