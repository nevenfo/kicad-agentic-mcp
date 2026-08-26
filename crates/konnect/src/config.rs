use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Path to the kicad-cli binary
    #[serde(default = "default_kicad_cli")]
    pub kicad_cli: String,

    /// Path to the KiCAD binary (for launching the UI)
    #[serde(default = "default_kicad_binary")]
    pub kicad_binary: String,

    /// Default project directory
    #[serde(default)]
    pub project_dir: Option<PathBuf>,

    /// KiCAD IPC socket path (NNG). Auto-detected from KICAD_API_SOCKET env var if empty.
    #[serde(default = "default_ipc_address")]
    #[serde(alias = "ipc_socket_path")]
    pub ipc_address: String,

    /// MCP server transport mode
    #[serde(default)]
    pub transport: TransportMode,

    /// HTTP server bind address (used when transport includes HTTP)
    #[serde(default = "default_http_address")]
    pub http_address: String,

    /// JLCPCB database cache path
    #[serde(default)]
    pub jlcpcb_db_path: Option<PathBuf>,

    /// Log level (error, warn, info, debug, trace)
    #[serde(default = "default_log_level")]
    pub log_level: String,

    /// Auto-load a tool's toolset on call instead of returning
    /// `toolset_not_loaded`. Off by default: toolsets accumulate monotonically
    /// once loaded, so auto-load trades one recoverable error for permanent
    /// context growth -- opt in only if that trade is worth it for your client.
    #[serde(default)]
    pub auto_load_toolsets: bool,

    /// Explicit loopback OpenAI-compatible endpoint for Agent mode. When
    /// absent, LOCAL returns `local_provider_unavailable` and Direct is
    /// unaffected.
    #[serde(default)]
    pub local_llm_base_url: Option<String>,

    /// Backend model id; defaults to the D38 selection.
    #[serde(default = "default_local_llm_model")]
    pub local_llm_model: String,

    /// Process-wide execution-risk mode (plan.md D.8), orthogonal to which
    /// toolset a client has loaded. `#[serde(skip)]`: this is a process
    /// startup decision from `KONNECT_MODE`, not a config-file setting — a
    /// stale `mode: "read-only"` left in a saved `settings.json` must not
    /// silently lock a server that the operator meant to run writable.
    #[serde(skip, default)]
    pub mode: kam_state::OperatingMode,
}

/// Which tier resolved `ipc_address` — logged once at startup, the same way
/// [`crate::install::KicadCliSource`] is, so a user can tell whether the
/// server picked a real config value, the env var KiCAD sets when it
/// launches the plugin itself, or guessed the platform default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpcAddressSource {
    /// An explicit non-empty `ipc_address` from the config file — never
    /// replaced, exactly like a configured `kicad_cli` value.
    Configured,
    /// The `KICAD_API_SOCKET` environment variable, set by KiCad itself when
    /// it launches the plugin (or by the invoking process otherwise).
    EnvVar,
    /// The compiled-in per-platform default: `<temp_dir>\kicad\api.sock` on
    /// Windows, `/tmp/kicad/api.sock` on macOS/Linux.
    PlatformDefault,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TransportMode {
    #[default]
    Stdio,
    Http,
    Both,
}

fn default_kicad_cli() -> String {
    if cfg!(target_os = "windows") {
        "kicad-cli.exe".to_string()
    } else {
        "kicad-cli".to_string()
    }
}

fn default_kicad_binary() -> String {
    if cfg!(target_os = "windows") {
        "kicad.exe".to_string()
    } else {
        "kicad".to_string()
    }
}

fn default_ipc_address() -> String {
    // Empty = auto-detect from KICAD_API_SOCKET env var at runtime
    std::env::var("KICAD_API_SOCKET").unwrap_or_default()
}

/// The socket path KiCad opens when nothing else says otherwise, so a
/// standalone MCP client (Claude Desktop/Code — the launch path the README
/// documents) still gets a working `ipc_address` without the user copying
/// one out of KiCad's Preferences dialog by hand. KiCad only ever sets
/// `KICAD_API_SOCKET` itself, when *it* launches the plugin — a client
/// spawning `konnect` standalone never inherits it.
///
/// Windows: `<std::env::temp_dir()>\kicad\api.sock`. This is *constructed*,
/// never checked with an existence test — on Windows an `ipc://` address is
/// an NNG named pipe, not a filesystem entry, so `%LOCALAPPDATA%\Temp\kicad\`
/// stays empty even while KiCad is listening on it.
///
/// macOS: `/tmp/kicad/api.sock` — matching what `README.md` documents.
/// This is deliberately *not* `std::env::temp_dir()`, which on macOS
/// resolves to `$TMPDIR` (a per-user directory under `/var/folders/...`),
/// not `/tmp`.
///
/// Linux: `/tmp/kicad/api.sock`.
fn platform_default_ipc_address() -> String {
    #[cfg(target_os = "windows")]
    {
        let socket = std::env::temp_dir().join("kicad").join("api.sock");
        format!("ipc://{}", socket.display())
    }
    #[cfg(not(target_os = "windows"))]
    {
        "ipc:///tmp/kicad/api.sock".to_string()
    }
}

fn default_http_address() -> String {
    "127.0.0.1:3000".to_string()
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_local_llm_model() -> String {
    "gpt-oss-20b".to_string()
}

impl Config {
    /// Load config from the default search path. Used by the `cdylib`/`rlib`
    /// target (`ffi.rs`, KiCAD's own plugin loader), which doesn't need the
    /// resolution-source log line the `bin` target's `main.rs` prints via
    /// [`Self::load_with_ipc_source`] — `config.rs` is compiled once per
    /// target, so this is genuinely unused from the `bin` target's own POV.
    #[allow(dead_code)]
    pub fn load() -> Result<Self> {
        Self::load_with_ipc_source().map(|(config, _source)| config)
    }

    /// Same as [`Self::load`], but also returns which tier resolved
    /// `ipc_address` — for the one-time startup log line, mirroring
    /// `install::resolve_binary`'s `KicadCliSource`.
    pub fn load_with_ipc_source() -> Result<(Self, IpcAddressSource)> {
        let mut config_paths = vec![
            PathBuf::from("konnect.toml"),
            PathBuf::from("settings.json"),
        ];
        config_paths.extend(exe_relative_settings_paths());
        config_paths.push(dirs_config_path());

        let mut config = None;
        for path in &config_paths {
            if path.exists() {
                config = Some(Self::load_from(path)?);
                break;
            }
        }

        let mut config = config.unwrap_or_default();
        let source = config.apply_env_fallbacks()?;
        Ok((config, source))
    }

    /// Resolve `ipc_address`, first-found-wins: an explicit configured value
    /// (never replaced), then `KICAD_API_SOCKET`, then the compiled-in
    /// per-platform default (see [`platform_default_ipc_address`]). Must run
    /// on every load path — including `--config <file>`, which is how KiCAD
    /// itself launches the server (with `KICAD_API_SOCKET` in the
    /// environment).
    ///
    /// Returns `Err` only for `KONNECT_MODE` (plan.md D.8): an unrecognised
    /// mode is an explicit startup failure, never a silent fallback to the
    /// unrestricted `write` default — see `kam_state::OperatingMode::from_str`.
    pub fn apply_env_fallbacks(&mut self) -> Result<IpcAddressSource> {
        let env_socket = std::env::var("KICAD_API_SOCKET")
            .ok()
            .filter(|s| !s.is_empty());

        let ipc_address_source = if !self.ipc_address.is_empty() {
            // Non-empty already: either an explicit config value, or the
            // env var was already folded in by `default_ipc_address()` (the
            // serde default, for a field entirely absent from the file).
            // Value equality disambiguates for logging only -- the value
            // itself is never touched either way.
            match &env_socket {
                Some(sock) if sock == &self.ipc_address => IpcAddressSource::EnvVar,
                _ => IpcAddressSource::Configured,
            }
        } else if let Some(sock) = env_socket {
            self.ipc_address = sock;
            IpcAddressSource::EnvVar
        } else {
            self.ipc_address = platform_default_ipc_address();
            IpcAddressSource::PlatformDefault
        };

        if self.local_llm_base_url.as_deref().is_none_or(str::is_empty) {
            self.local_llm_base_url = std::env::var("KONNECT_LOCAL_LLM_BASE_URL")
                .ok()
                .filter(|value| !value.is_empty());
        }
        if self.local_llm_model == default_local_llm_model() {
            if let Ok(model) = std::env::var("KONNECT_LOCAL_LLM_MODEL") {
                if !model.is_empty() {
                    self.local_llm_model = model;
                }
            }
        }
        match std::env::var("KONNECT_MODE") {
            Err(_) => {}
            Ok(raw) if raw.is_empty() => {}
            Ok(raw) => {
                self.mode = raw
                    .parse::<kam_state::OperatingMode>()
                    .map_err(|e| anyhow::anyhow!("KONNECT_MODE: {e}"))?;
            }
        }
        Ok(ipc_address_source)
    }

    /// Build the optional loopback-only local provider for Agent mode.
    pub fn local_provider(&self) -> Result<Option<std::sync::Arc<dyn kam_llm::Provider>>> {
        let Some(base_url) = self.local_llm_base_url.as_deref() else {
            return Ok(None);
        };
        let config = kam_llm::OpenAiCompatConfig::new(base_url, &self.local_llm_model)?;
        Ok(Some(std::sync::Arc::new(
            kam_llm::OpenAiCompatProvider::new(config),
        )))
    }

    /// Load config from a specific file path. Auto-detects JSON vs TOML by extension.
    pub fn load_from(path: &std::path::Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

        match ext {
            "json" => {
                let config: Config = serde_json::from_str(&content)?;
                Ok(config)
            }
            _ => {
                // Default: TOML
                let config: Config = toml::from_str(&content)?;
                Ok(config)
            }
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            kicad_cli: default_kicad_cli(),
            kicad_binary: default_kicad_binary(),
            project_dir: None,
            ipc_address: default_ipc_address(),
            transport: TransportMode::default(),
            http_address: default_http_address(),
            jlcpcb_db_path: None,
            log_level: default_log_level(),
            auto_load_toolsets: false,
            local_llm_base_url: None,
            local_llm_model: default_local_llm_model(),
            mode: kam_state::OperatingMode::Write,
        }
    }
}

/// settings.json next to the binary, and one dir up (covers <plugin_dir>/bin/konnect).
fn exe_relative_settings_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            paths.push(exe_dir.join("settings.json"));
            if let Some(parent_dir) = exe_dir.parent() {
                paths.push(parent_dir.join("settings.json"));
            }
        }
    }
    paths
}

fn dirs_config_path() -> PathBuf {
    // Platform-specific config directory
    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var("APPDATA").unwrap_or_default();
        PathBuf::from(appdata).join("konnect").join("config.toml")
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").unwrap_or_default();
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("konnect")
            .join("config.toml")
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let home = std::env::var("HOME").unwrap_or_default();
        PathBuf::from(home)
            .join(".config")
            .join("konnect")
            .join("config.toml")
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp(ext: &str, content: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::Builder::new()
            .suffix(&format!(".{ext}"))
            .tempfile()
            .unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f.flush().unwrap();
        f
    }

    // Malformed input must produce Err, never a panic (the class of bug
    // PR #9 found in the config *tools*; this pins the server config too).

    #[test]
    fn json_non_object_root_is_err_not_panic() {
        for bad in ["[1, 2, 3]", "42", "\"just a string\"", "null", "true"] {
            let f = write_temp("json", bad);
            assert!(Config::load_from(f.path()).is_err(), "input: {bad}");
        }
    }

    #[test]
    fn json_wrong_field_types_are_err() {
        for bad in [
            r#"{"transport": 42}"#,
            r#"{"transport": "carrier-pigeon"}"#,
            r#"{"kicad_cli": ["a", "b"]}"#,
            r#"{"log_level": {"nested": true}}"#,
        ] {
            let f = write_temp("json", bad);
            assert!(Config::load_from(f.path()).is_err(), "input: {bad}");
        }
    }

    #[test]
    fn toml_garbage_is_err_not_panic() {
        for bad in ["= = =", "[unclosed", "transport = ", "\u{0000}\u{FFFF}"] {
            let f = write_temp("toml", bad);
            assert!(Config::load_from(f.path()).is_err(), "input: {bad:?}");
        }
    }

    #[test]
    fn missing_file_is_err() {
        assert!(Config::load_from(std::path::Path::new("does/not/exist.toml")).is_err());
    }

    // Partial configs fill in defaults for everything omitted.

    #[test]
    fn empty_json_object_yields_defaults() {
        let f = write_temp("json", "{}");
        let c = Config::load_from(f.path()).unwrap();
        let d = Config::default();
        assert_eq!(c.kicad_cli, d.kicad_cli);
        assert_eq!(c.http_address, d.http_address);
        assert_eq!(c.log_level, d.log_level);
        assert!(matches!(c.transport, TransportMode::Stdio));
    }

    #[test]
    fn empty_toml_yields_defaults() {
        let f = write_temp("toml", "");
        let c = Config::load_from(f.path()).unwrap();
        assert_eq!(c.log_level, "info");
    }

    #[test]
    fn partial_toml_overrides_only_named_fields() {
        let f = write_temp(
            "toml",
            "transport = \"http\"\nhttp_address = \"127.0.0.1:9999\"\n",
        );
        let c = Config::load_from(f.path()).unwrap();
        assert!(matches!(
            c.transport,
            TransportMode::Both | TransportMode::Http
        ));
        assert!(matches!(c.transport, TransportMode::Http));
        assert_eq!(c.http_address, "127.0.0.1:9999");
        assert_eq!(c.log_level, "info"); // untouched default
    }

    // Mutates process-wide env vars, so env-touching tests run serially.
    static ENV_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// A panic in one env-mutating test (e.g. a failed `assert_eq!` while
    /// holding the lock) must not poison the mutex for every test after it —
    /// that turns one red test into every remaining one in the file (D145).
    /// Same pattern as `konnect-schematic-editor::library::env_lock`.
    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        ENV_GUARD
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn empty_ipc_address_falls_back_to_env_var_when_no_config_found() {
        let _guard = env_lock();
        std::env::set_var("KICAD_API_SOCKET", "ipc://env-fallback.sock");
        let c = Config::default();
        assert_eq!(c.ipc_address, "ipc://env-fallback.sock");
        std::env::remove_var("KICAD_API_SOCKET");
    }

    #[test]
    fn explicit_empty_ipc_address_in_config_file_does_not_block_env_var() {
        // A present-but-blank field must not out-rank the env var the way
        // a merely-missing field would (#39).
        let _guard = env_lock();
        std::env::set_var("KICAD_API_SOCKET", "ipc://env-wins.sock");

        let f = write_temp("json", r#"{"ipc_socket_path": ""}"#);
        let mut c = Config::load_from(f.path()).unwrap();
        assert_eq!(c.ipc_address, "", "sanity: file's blank value loaded as-is");

        c.apply_env_fallbacks().unwrap();
        assert_eq!(c.ipc_address, "ipc://env-wins.sock");

        // But an explicit file value must out-rank the env var.
        let f = write_temp("json", r#"{"ipc_socket_path": "ipc://file-wins.sock"}"#);
        let mut c = Config::load_from(f.path()).unwrap();
        c.apply_env_fallbacks().unwrap();
        assert_eq!(c.ipc_address, "ipc://file-wins.sock");

        std::env::remove_var("KICAD_API_SOCKET");
    }

    #[test]
    fn empty_ipc_address_and_no_env_var_falls_back_to_platform_default() {
        let _guard = env_lock();
        std::env::remove_var("KICAD_API_SOCKET");
        let mut c = Config {
            ipc_address: String::new(),
            ..Config::default()
        };
        let source = c.apply_env_fallbacks().unwrap();
        assert_eq!(source, IpcAddressSource::PlatformDefault);
        assert!(!c.ipc_address.is_empty(), "must not stay blank");
        assert_eq!(c.ipc_address, platform_default_ipc_address());
        assert!(c.ipc_address.starts_with("ipc://"));
    }

    #[test]
    fn explicit_configured_ipc_address_is_never_replaced() {
        let _guard = env_lock();
        std::env::set_var("KICAD_API_SOCKET", "ipc://env-should-lose.sock");
        let mut c = Config {
            ipc_address: "ipc://explicit-config-value.sock".to_string(),
            ..Config::default()
        };
        let source = c.apply_env_fallbacks().unwrap();
        assert_eq!(source, IpcAddressSource::Configured);
        assert_eq!(c.ipc_address, "ipc://explicit-config-value.sock");
        std::env::remove_var("KICAD_API_SOCKET");
    }

    #[test]
    fn env_var_wins_over_platform_default() {
        let _guard = env_lock();
        std::env::set_var("KICAD_API_SOCKET", "ipc://env-wins-over-default.sock");
        let mut c = Config {
            ipc_address: String::new(),
            ..Config::default()
        };
        let source = c.apply_env_fallbacks().unwrap();
        assert_eq!(source, IpcAddressSource::EnvVar);
        assert_eq!(c.ipc_address, "ipc://env-wins-over-default.sock");
        std::env::remove_var("KICAD_API_SOCKET");
    }

    #[test]
    fn legacy_ipc_socket_path_alias_still_works() {
        // settings.json written by the KiCAD plugin dialog uses the alias.
        let f = write_temp("json", r#"{"ipc_socket_path": "ipc://test.sock"}"#);
        let c = Config::load_from(f.path()).unwrap();
        assert_eq!(c.ipc_address, "ipc://test.sock");
    }

    #[test]
    fn unknown_extension_parses_as_toml() {
        let f = write_temp("conf", "log_level = \"debug\"\n");
        let c = Config::load_from(f.path()).unwrap();
        assert_eq!(c.log_level, "debug");
    }

    #[test]
    fn local_agent_provider_is_opt_in_and_loopback_only() {
        assert!(Config::default().local_provider().unwrap().is_none());

        let mut local = Config {
            local_llm_base_url: Some("http://127.0.0.1:1234/v1".to_string()),
            ..Config::default()
        };
        assert!(local.local_provider().unwrap().is_some());

        local.local_llm_base_url = Some("https://models.example.com/v1".to_string());
        assert!(local.local_provider().is_err());
    }

    #[test]
    fn konnect_mode_env_var_sets_mode() {
        let _guard = env_lock();
        std::env::set_var("KONNECT_MODE", "read-only");
        let mut c = Config::default();
        c.apply_env_fallbacks().unwrap();
        assert_eq!(c.mode, kam_state::OperatingMode::ReadOnly);
        std::env::remove_var("KONNECT_MODE");
    }

    #[test]
    fn unset_konnect_mode_defaults_to_write() {
        let _guard = env_lock();
        std::env::remove_var("KONNECT_MODE");
        let mut c = Config::default();
        c.apply_env_fallbacks().unwrap();
        assert_eq!(c.mode, kam_state::OperatingMode::Write);
    }

    #[test]
    fn unknown_konnect_mode_is_a_startup_error() {
        let _guard = env_lock();
        std::env::set_var("KONNECT_MODE", "yolo");
        let mut c = Config::default();
        let result = c.apply_env_fallbacks();
        std::env::remove_var("KONNECT_MODE");
        assert!(result.is_err(), "an unrecognised mode must fail startup");
        // Never silently left at (or defaulted to) the unrestricted mode.
        assert_eq!(c.mode, kam_state::OperatingMode::Write);
    }
}
