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
use std::sync::Arc;

use konnect_core::mcp::protocol::{CallToolResult, ToolContent};
use konnect_core::router::ToolRouter;
use konnect_core::tools::{ServerConfig, ToolContext};
use serde_json::Value;

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
        let router = Arc::new(ToolRouter::new());
        let ctx = Arc::new(ToolContext::new(
            ServerConfig {
                kicad_cli,
                kicad_binary: String::new(),
                ipc_address: String::new(),
                project_dir: None,
                jlcpcb_db_path: None,
                auto_load_toolsets: false,
            },
            router.clone(),
        ));
        Harness {
            router,
            ctx,
            dir: tempfile::tempdir().expect("tempdir"),
        }
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
        std::fs::copy(&src, &dst)
            .unwrap_or_else(|e| panic!("fixture {name} is copyable: {e}"));
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
    serde_json::from_str(&text).unwrap_or_else(|_| Value::String(text))
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

/// Pin coordinates of the [`TWO_RESISTORS`] fixture, as (x, y) in mm.
pub mod pins {
    pub const R1_PIN1: (f64, f64) = (101.6, 46.99);
    pub const R1_PIN2: (f64, f64) = (101.6, 54.61);
    pub const R2_PIN1: (f64, f64) = (114.3, 46.99);
    pub const R2_PIN2: (f64, f64) = (114.3, 54.61);
}
