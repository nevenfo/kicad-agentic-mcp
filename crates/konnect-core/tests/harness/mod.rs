//! Calling a tool the way an agent does, from an integration test.
//!
//! The capability matrix counts a tool as proved when a test that runs
//! exercises it. Going through [`ToolRouter`] rather than a private handler
//! makes that proof the real path: the tool has to be registered, findable by
//! name, and take the arguments its schema advertises.
//!
//! No `kicad-cli` and no running KiCAD — `kicad_cli` is empty, so a tool that
//! shells out fails cleanly and a test asserting on that failure is honest
//! about what it proved.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use konnect_core::mcp::protocol::{CallToolResult, ToolContent};
use konnect_core::router::ToolRouter;
use konnect_core::tools::{ServerConfig, ToolContext};
use serde_json::Value;

/// Keep `konnect_sexp::writer::document_lock_path` off `HOME`/`APPDATA` for
/// the lifetime of this test binary.
///
/// `redirected_user_config` (in `config_and_rules.rs`) repoints
/// `HOME`/`APPDATA` to a short-lived `TempDir`, under a mutex that only the
/// config tests take. A design-rules test never takes that guard, but its
/// write still resolves its lock file through `dirs::data_local_dir()` — on
/// macOS, `$HOME/Library/Application Support` — which lands it inside
/// whichever config test's `TempDir` `HOME` points at right now. That
/// `TempDir` is deleted the moment its owning test returns, out from under a
/// lock file it never knew about: `'set_design_rules' failed: IO error:
/// Invalid argument (os error 22)`. Windows never sees this, because its
/// equivalent lookup uses `LOCALAPPDATA`, which nothing here redirects.
///
/// Pointing `KONNECT_STATE_DIR` at a directory under `CARGO_TARGET_TMPDIR` —
/// stable for the binary's run, outside the user's profile, and never
/// repointed by any test — takes `HOME` out of that lookup entirely, so the
/// two no longer share a directory to race over.
fn ensure_state_dir() {
    static INIT: OnceLock<()> = OnceLock::new();
    INIT.get_or_init(|| {
        let state_dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("konnect-state");
        std::fs::create_dir_all(&state_dir).expect("state dir is creatable");
        std::env::set_var("KONNECT_STATE_DIR", &state_dir);
    });
}

/// A JLCPCB database path that is guaranteed not to exist, under this test
/// binary's own temp directory.
///
/// P.6.9.20: `jlcpcb_db_path: None` does not mean "no database". It means
/// "fall back to the machine-wide default" — `resolve_db_path`
/// (`tools/integration.rs:248`) then returns `default_jlcpcb_db_path()`,
/// `%APPDATA%\konnect\jlcpcb.db` on Windows. So
/// `the_jlcpcb_tools_say_the_database_is_missing_rather_than_finding_nothing`
/// asserted a fact about the machine while its message claimed a fact about
/// the harness, and started failing the day a real database was downloaded
/// here. Naming a path that is never created makes absence a property of the
/// fixture, the way `kicad_cli: ""` already makes "no kicad-cli" one.
fn absent_jlcpcb_db() -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR")).join("no-such-jlcpcb.db")
}

/// The `ServerConfig` every `Harness` constructor shares, parameterised only
/// by the one field a caller has ever needed to vary.
fn config(kicad_cli: String) -> ServerConfig {
    ServerConfig {
        kicad_cli,
        kicad_binary: String::new(),
        ipc_address: String::new(),
        project_dir: None,
        jlcpcb_db_path: Some(absent_jlcpcb_db()),
        auto_load_toolsets: false,
        mode: kam_state::OperatingMode::Write,
    }
}

/// A router with every toolset reachable, and a context with no KiCAD behind
/// it.
pub struct Harness {
    router: Arc<ToolRouter>,
    ctx: Arc<ToolContext>,
    pub dir: tempfile::TempDir,
}

impl Harness {
    pub fn new() -> Self {
        Self::with_kicad_cli(String::new())
    }

    /// Same, with a `kicad-cli` path — for a probe that has one.
    pub fn with_kicad_cli(kicad_cli: String) -> Self {
        ensure_state_dir();
        let router = Arc::new(ToolRouter::new());
        let ctx = Arc::new(ToolContext::new(config(kicad_cli), router.clone()));
        Harness {
            router,
            ctx,
            dir: tempfile::tempdir().expect("tempdir"),
        }
    }

    /// Same as [`Self::new`], but with `ctx.journal` open against a directory
    /// this harness owns — `ToolContext::new`'s journal is always `None`
    /// (it must stay IO-free for the tests that don't care), so this rebuilds
    /// the context rather than patching the one `new` made.
    ///
    /// For D.7.1's replay probe, which needs a real journal to read back.
    pub fn with_journal() -> Self {
        ensure_state_dir();
        let router = Arc::new(ToolRouter::new());
        let mut ctx = ToolContext::new(config(String::new()), router.clone());
        let dir = tempfile::tempdir().expect("tempdir");
        let journal = kam_state::RunJournal::open(dir.path().join("journal"))
            .expect("journal dir is creatable");
        ctx.journal = Some(Arc::new(journal));
        Harness {
            router,
            ctx: Arc::new(ctx),
            dir,
        }
    }

    /// The context this harness calls tools through — for a probe that needs
    /// to reach a meta-tool handler directly (`router::meta_tools::handle_meta_tool`)
    /// rather than through [`Self::call`]'s toolset lookup.
    pub fn ctx(&self) -> Arc<ToolContext> {
        self.ctx.clone()
    }

    /// Call `tool` by name, as `tools/call` does.
    pub async fn call(&self, tool: &str, args: Value) -> anyhow::Result<CallToolResult> {
        let def = self
            .router
            .find_tool_def(tool)
            .unwrap_or_else(|| panic!("'{tool}' is not registered in any toolset"));
        (def.handler)(&args, self.ctx.clone()).await
    }

    /// Call `tool` and read its JSON body. Panics if the tool errored — use
    /// [`call`](Self::call) when the error is the point.
    pub async fn json(&self, tool: &str, args: Value) -> Value {
        let result = self
            .call(tool, args)
            .await
            .unwrap_or_else(|e| panic!("'{tool}' failed: {e}"));
        body(&result)
    }

    /// Copy a fixture into this harness's directory and return the copy.
    pub fn fixture(&self, name: &str) -> PathBuf {
        let src = fixtures_dir().join(name);
        let dst = self.dir.path().join(name);
        std::fs::copy(&src, &dst).unwrap_or_else(|e| panic!("fixture {name} is copyable: {e}"));
        dst
    }

    /// Write `content` into this harness's directory under `name`.
    pub fn write(&self, name: &str, content: &str) -> PathBuf {
        let path = self.dir.path().join(name);
        std::fs::write(&path, content).expect("the file is writable");
        path
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    /// A file from the repository itself, copied into this harness's
    /// directory. `path` is relative to the workspace root, e.g.
    /// `"bench/fixtures/divider.kicad_sch"`.
    ///
    /// For a probe that must run against a whole project rather than a
    /// hand-written fixture. Copying means the repository's own file is never
    /// edited by a test.
    pub fn repo_file(&self, path: &str) -> PathBuf {
        let src = workspace_root().join(path);
        let name = Path::new(path).file_name().expect("the path names a file");
        let dst = self.dir.path().join(name);
        std::fs::copy(&src, &dst).unwrap_or_else(|e| panic!("{path} is copyable: {e}"));
        dst
    }
}

/// The workspace root: two levels above this crate.
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

pub fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

/// The JSON a tool returned. Tools answer with one text block holding JSON.
pub fn body(result: &CallToolResult) -> Value {
    let text = match result.content.first() {
        Some(ToolContent::Text { text }) => text.clone(),
        _ => panic!("the result carries no text"),
    };
    serde_json::from_str(&text).unwrap_or(Value::String(text))
}

pub fn as_str(path: &Path) -> &str {
    path.to_str().expect("the path is UTF-8")
}

/// A schematic KiCAD 10 loads, with two `Device:R` symbols wired together and
/// their library symbol embedded, so no installed libraries are needed.
///
/// R1 sits at (101.6, 50.8) and R2 at (114.3, 50.8); each pin 1 is 3.81 mm
/// above its symbol and each pin 2 the same distance below.
pub const TWO_RESISTORS: &str = "bus_two_resistors.kicad_sch";

/// [`TWO_RESISTORS`] with R2 marked `(dnp yes)` — the only fixture that makes
/// `export_bom`'s `exclude_dnp` observable: `kicad-cli` is the one doing the
/// filtering, so the oracle is the CSV it writes, not our own JSON.
pub const TWO_RESISTORS_ONE_DNP: &str = "bus_two_resistors_dnp.kicad_sch";

/// A real `Amplifier_Operational:LM2904` (dual op-amp) placed as `U1`, unit 1
/// at x = 100 and unit 2 at x = 160 — two top-level `(symbol …)` blocks
/// sharing one designator, each with its own uuid and its own copy of every
/// property (P.6.8.1). Loads clean in KiCad 10; edited only through copies.
pub const MULTIUNIT_LM2904: &str = "multiunit_lm2904.kicad_sch";

/// A real `Connector_Generic:Conn_02x05_Odd_Even` placed as `J1` at
/// (101.6, 96.52) — a double-row connector whose two rows face opposite ways
/// and share y coordinates, so a wire stub drawn the wrong way genuinely
/// crosses another pin instead of only looking like it might (P.6.8.5).
/// Pin positions as `kicad-cli sch erc` reports them: odd pins 1..9 on the
/// left at x = 96.52, even pins 2..10 on the right at x = 109.22, both rows
/// stepping 2.54 from y = 91.44 down to y = 101.6 — so pin 9 at
/// `(96.52, 101.6)` sits directly across from pin 10 at `(109.22, 101.6)`.
/// Loads clean in KiCad 10 (10 `pin_not_connected` errors and nothing else,
/// as an unwired connector should); edited only through copies.
pub const CONN_DOUBLE_ROW: &str = "conn_double_row.kicad_sch";

/// A board with layers and nothing on them — the same skeleton
/// `create_project` writes. Use it when the fixture's own `Edge.Cuts` outline
/// would be measured together with whatever the test draws.
pub const BLANK_BOARD: &str = "(kicad_pcb\n\t(version 20250610)\n\t(generator \"konnect\")\n\t(generator_version \"10.0\")\n\t(general\n\t\t(thickness 1.6)\n\t)\n\t(paper \"A4\")\n\t(layers\n\t\t(0 \"F.Cu\" signal)\n\t\t(31 \"B.Cu\" signal)\n\t\t(36 \"B.SilkS\" user \"B.Silkscreen\")\n\t\t(37 \"F.SilkS\" user \"F.Silkscreen\")\n\t\t(44 \"Edge.Cuts\" user)\n\t)\n\t(setup\n\t\t(pad_to_mask_clearance 0.05)\n\t)\n\t(net 0 \"\")\n)\n";

/// Pin coordinates of the [`TWO_RESISTORS`] fixture, as (x, y) in mm.
pub mod pins {
    pub const R1_PIN1: (f64, f64) = (101.6, 46.99);
    pub const R1_PIN2: (f64, f64) = (101.6, 54.61);
    pub const R2_PIN1: (f64, f64) = (114.3, 46.99);
    pub const R2_PIN2: (f64, f64) = (114.3, 54.61);
}
