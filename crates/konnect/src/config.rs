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
    /// Load config from the default search path.
    pub fn load() -> Result<Self> {
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
        config.apply_env_fallbacks();
        Ok(config)
    }

    /// Env var wins over an unset/blank ipc_address either way. Must run on
    /// every load path — including `--config <file>`, which is how KiCAD
    /// itself launches the server (with KICAD_API_SOCKET in the environment).
    pub fn apply_env_fallbacks(&mut self) {
        if self.ipc_address.is_empty() {
            if let Ok(sock) = std::env::var("KICAD_API_SOCKET") {
                if !sock.is_empty() {
                    self.ipc_address = sock;
                }
            }
        }
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

    // Mutates the process-wide env var, so these two run serially.
    static ENV_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn empty_ipc_address_falls_back_to_env_var_when_no_config_found() {
        let _guard = ENV_GUARD.lock().unwrap();
        std::env::set_var("KICAD_API_SOCKET", "ipc://env-fallback.sock");
        let c = Config::default();
        assert_eq!(c.ipc_address, "ipc://env-fallback.sock");
        std::env::remove_var("KICAD_API_SOCKET");
    }

    #[test]
    fn explicit_empty_ipc_address_in_config_file_does_not_block_env_var() {
        // A present-but-blank field must not out-rank the env var the way
        // a merely-missing field would (#39).
        let _guard = ENV_GUARD.lock().unwrap();
        std::env::set_var("KICAD_API_SOCKET", "ipc://env-wins.sock");

        let f = write_temp("json", r#"{"ipc_socket_path": ""}"#);
        let mut c = Config::load_from(f.path()).unwrap();
        assert_eq!(c.ipc_address, "", "sanity: file's blank value loaded as-is");

        c.apply_env_fallbacks();
        assert_eq!(c.ipc_address, "ipc://env-wins.sock");

        // But an explicit file value must out-rank the env var.
        let f = write_temp("json", r#"{"ipc_socket_path": "ipc://file-wins.sock"}"#);
        let mut c = Config::load_from(f.path()).unwrap();
        c.apply_env_fallbacks();
        assert_eq!(c.ipc_address, "ipc://file-wins.sock");

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

        let mut local = Config::default();
        local.local_llm_base_url = Some("http://127.0.0.1:1234/v1".to_string());
        assert!(local.local_provider().unwrap().is_some());

        local.local_llm_base_url = Some("https://models.example.com/v1".to_string());
        assert!(local.local_provider().is_err());
    }
}
