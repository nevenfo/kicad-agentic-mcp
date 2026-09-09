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

use std::collections::BTreeMap;
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
    config_with_ipc(kicad_cli, String::new())
}

/// The same, for the live suites: `ipc_address` is the one other field that
/// changes what the tools can reach.
fn config_with_ipc(kicad_cli: String, ipc_address: String) -> ServerConfig {
    ServerConfig {
        kicad_cli,
        kicad_binary: String::new(),
        ipc_address,
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

    /// A harness whose tools talk to a real KiCAD, for the live suites.
    ///
    /// Panics when `KICAD_API_SOCKET` is unset: every caller is `#[ignore]`d
    /// and was asked for explicitly, so a silent skip would report a pass for
    /// a suite that never ran.
    pub fn live() -> Self {
        let socket = std::env::var("KICAD_API_SOCKET")
            .expect("KICAD_API_SOCKET is required by the live suite");
        ensure_state_dir();
        let router = Arc::new(ToolRouter::new());
        let ctx = Arc::new(ToolContext::new(
            config_with_ipc(String::new(), socket),
            router.clone(),
        ));
        Harness {
            router,
            ctx,
            dir: tempfile::tempdir().expect("tempdir"),
        }
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
    ///
    /// Both shapes of failure count. A handler that returns `Err` and one that
    /// returns `CallToolResult { is_error: true }` are the same event to a
    /// caller, and the second is the shape almost every handler here uses:
    /// `require_str`, `get_path` and `lib_symbol_not_found_error` all build a
    /// result rather than an error. Reading the body of one and asserting on
    /// the file afterwards is how a test can assert a tool's effect while the
    /// tool refused to act — P.7.1's defect exactly.
    pub async fn json(&self, tool: &str, args: Value) -> Value {
        let result = self
            .call(tool, args)
            .await
            .unwrap_or_else(|e| panic!("'{tool}' failed: {e}"));
        assert!(
            !result.is_error,
            "'{tool}' refused: {}",
            serde_json::to_string(&body(&result)).unwrap_or_default()
        );
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

// ─── KiCAD as the judge ──────────────────────────────────────────────────────

/// Two tracks on `F.Cu`, on different nets, 1 mm apart centre to centre and
/// 0.25 mm wide — so 0.75 mm of copper-to-copper gap, inside a closed
/// `Edge.Cuts` rectangle.
///
/// The gap is the whole point: a `min_clearance` below it produces no
/// `clearance` violation and one above it produces exactly one, which makes
/// "did KiCAD actually apply the rule we wrote?" a question with a countable
/// answer. Measured, not assumed — 0.2 mm gives `{track_dangling: 2}` and
/// 1.5 mm gives `{clearance: 1, track_dangling: 2}` under kicad-cli 10.0.6.
pub const CLEARANCE_BOARD: &str = include_str!("../fixtures/clearance_pair.kicad_pcb");

/// The smallest project file KiCAD accepts beside a board, with no rules of
/// its own — the state a board has before anything sets a constraint.
pub const BLANK_PROJECT: &str =
    "{\n  \"board\": {\n    \"design_settings\": {}\n  },\n  \"meta\": {\n    \"version\": 3\n  }\n}\n";

/// Where `kicad-cli` is, for the suites that need a real one.
///
/// `KONNECT_KICAD_CLI` wins so a machine with several KiCADs can say which.
pub fn kicad_cli_path() -> PathBuf {
    if let Ok(configured) = std::env::var("KONNECT_KICAD_CLI") {
        if !configured.trim().is_empty() {
            return PathBuf::from(configured);
        }
    }
    let name = if cfg!(windows) {
        "kicad-cli.exe"
    } else {
        "kicad-cli"
    };
    let local_appdata = std::env::var("LOCALAPPDATA").ok().map(PathBuf::from);
    konnect_core::kicad_locate::kicad_standard_paths(name, local_appdata.as_deref())
        .into_iter()
        .find(|candidate| candidate.exists())
        .unwrap_or_else(|| PathBuf::from(name))
}

/// What KiCAD found in a board, per violation type.
pub type DrcCounts = BTreeMap<String, usize>;

/// Hand `board` to KiCAD and return what it says about it — the helper that
/// makes a claim about a mutation something other than our own opinion.
///
/// **Calling this is what earns a tool `Proof::Arbitrated`**
/// (`capability::coverage::ARBITER` names this function). The scan credits
/// every tool a calling test mentions, so call it in the test that performed
/// the mutation, not in a separate one.
///
/// It panics rather than returning an error on a board KiCAD will not load:
/// that is the failure the whole contract exists to catch, and a test that
/// could accidentally ignore it would be worse than no test. Note what it
/// does *not* prove: `kicad-cli pcb drc` reads the project file for its
/// constraints but does not validate it — a `.kicad_pro` full of nonsense
/// gives a clean exit and KiCAD's built-in defaults. Proving a *rule* landed
/// therefore means watching the violation counts move, not watching this
/// return.
pub fn kicad_reloads(board: &Path) -> DrcCounts {
    let report = board.with_extension("drc.json");
    let output = std::process::Command::new(kicad_cli_path())
        .args(["pcb", "drc", "--format", "json", "-o"])
        .arg(&report)
        .arg(board)
        .output()
        .unwrap_or_else(|e| {
            panic!("kicad-cli is required by this suite (set KONNECT_KICAD_CLI): {e}")
        });
    assert!(
        output.status.success(),
        "KiCAD refused to load {}: exit {:?}\n{}{}",
        board.display(),
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let text = std::fs::read_to_string(&report).expect("kicad-cli wrote its report");
    let parsed: Value = serde_json::from_str(&text).expect("the DRC report is JSON");
    let mut counts = DrcCounts::new();
    for violation in parsed["violations"].as_array().into_iter().flatten() {
        // The `type` field is a stable identifier; the `description` beside it
        // is translated into the user's language, so nothing here reads it.
        if let Some(kind) = violation["type"].as_str() {
            *counts.entry(kind.to_string()).or_default() += 1;
        }
    }
    counts
}

/// Ask the running KiCAD what it now holds, through a client of our own rather
/// than through the tool that just wrote.
///
/// **Calling this is what earns a tool `Proof::Live`**
/// (`capability::coverage::LIVE_ARBITER` names this function). It is the
/// counterpart of [`kicad_reloads`] for operations with no file to parse: the
/// active layer, for one, lives in the editor's session and nowhere in the
/// board. Take the answer from `f` and assert on that — asserting on what the
/// tool returned would only prove the tool agrees with itself.
pub fn kicad_reads_back<T>(f: impl FnOnce(&konnect_ipc::client::KiCadIpcClient) -> T) -> T {
    let socket =
        std::env::var("KICAD_API_SOCKET").expect("KICAD_API_SOCKET is required by the live suite");
    let client = konnect_ipc::client::KiCadIpcClient::new(socket);
    f(&client)
}

/// One footprint as KiCAD reports it in a position file.
#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    pub x: f64,
    pub y: f64,
    pub rotation: f64,
    /// `top` or `bottom`, KiCAD's own words.
    pub side: String,
}

/// Ask KiCAD where every footprint sits and which side it is on, by exporting
/// a position file and reading it back.
///
/// This is [`kicad_reloads`] for placement, and it is the only oracle
/// available for a flip: KiCAD exposes no flip command to compare against, so
/// the question "did this land?" has to be answered by KiCAD's own reading of
/// the board rather than by re-parsing the bytes we wrote. The CSV's `Side`
/// column is `top`/`bottom` regardless of the user's language, unlike a DRC
/// description.
///
/// Panics on a board KiCAD will not load, for the same reason
/// [`kicad_reloads`] does.
pub fn kicad_places(board: &Path) -> BTreeMap<String, Placement> {
    let report = board.with_extension("pos.csv");
    let output = std::process::Command::new(kicad_cli_path())
        .args([
            "pcb", "export", "pos", "--format", "csv", "--units", "mm", "--side", "both", "-o",
        ])
        .arg(&report)
        .arg(board)
        .output()
        .unwrap_or_else(|e| panic!("kicad-cli is required by this suite: {e}"));
    assert!(
        output.status.success(),
        "KiCAD refused to place {}: exit {:?}\n{}{}",
        board.display(),
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let text = std::fs::read_to_string(&report).expect("kicad-cli wrote its position file");
    let mut out = BTreeMap::new();
    for line in text.lines().skip(1) {
        // Ref,Val,Package,PosX,PosY,Rot,Side — the first three are quoted.
        let cells: Vec<&str> = line.split(',').collect();
        if cells.len() < 7 {
            continue;
        }
        let reference = cells[0].trim().trim_matches('"').to_string();
        let parse = |cell: &str| cell.trim().parse::<f64>().unwrap_or(f64::NAN);
        out.insert(
            reference,
            Placement {
                x: parse(cells[3]),
                y: parse(cells[4]),
                rotation: parse(cells[5]),
                side: cells[6].trim().trim_matches('"').to_string(),
            },
        );
    }
    out
}
