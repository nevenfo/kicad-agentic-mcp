//! Per-call observability: ring buffer of recent calls, per-tool counters,
//! and optional JSONL log file at `<konnect config dir>/logs/calls.jsonl`.
//!
//! Wired into `McpHandler::execute_tool` — every tool invocation (meta-tool or
//! domain tool) lands a `CallRecord` via `CallObserver::record`. The LLM can
//! introspect via the `get_recent_calls` and `server_stats` meta-tools.
//!
//! Goals:
//! - Zero cost on the hot path: recording is lock-then-push-then-write, no
//!   serialization overhead beyond the single JSON line.
//! - Failure isolation: if the log file can't be opened or written, the tool
//!   call still succeeds; we `tracing::warn!` and move on.
//! - Bounded memory: ring buffer caps at `MAX_RECENT_CALLS`.

use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use tracing::warn;

/// Keep the last N calls in memory for `get_recent_calls`.
pub const MAX_RECENT_CALLS: usize = 100;

// ─── Types ───────────────────────────────────────────────────────────────────

/// Outcome of a single tool call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CallStatus {
    /// Tool ran to completion and reported success.
    Ok,
    /// Tool ran to completion but reported `is_error: true`, or the handler
    /// returned an `anyhow::Error`.
    Error,
    /// Tool not found or in an unloaded toolset.
    NotFound,
}

impl CallStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            CallStatus::Ok => "ok",
            CallStatus::Error => "error",
            CallStatus::NotFound => "not_found",
        }
    }
}

/// One recorded tool invocation. Keep fields compact — this is serialized to
/// JSONL on every call.
#[derive(Debug, Clone, Serialize)]
pub struct CallRecord {
    pub call_id: String,
    /// Unix epoch milliseconds when the call started.
    pub ts: u64,
    pub tool: String,
    /// `None` for meta-tools; otherwise the owning toolset name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub toolset: Option<String>,
    pub dur_ms: u64,
    pub status: CallStatus,
    /// Short error identifier (e.g., "missing_argument", "ipc_unavailable").
    /// Free-form for now; promoted to a real enum in a follow-up change.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_kind: Option<String>,
    /// Serialized size of the arguments JSON in bytes.
    pub args_bytes: usize,
    /// Serialized size of the result in bytes (content text + image data).
    pub result_bytes: usize,
}

/// Aggregated counts for a single tool across the session.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ToolStats {
    pub total: u64,
    pub errors: u64,
    pub total_duration_ms: u64,
    /// Last call's status for quick "is it healthy right now?" checks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

// ─── Observer ────────────────────────────────────────────────────────────────

/// Centralized call sink. Cheap to clone — all state is behind `Arc`s.
#[derive(Clone)]
pub struct CallObserver {
    inner: Arc<Inner>,
}

struct Inner {
    recent: Mutex<VecDeque<CallRecord>>,
    per_tool: Mutex<HashMap<String, ToolStats>>,
    total_calls: AtomicU64,
    error_calls: AtomicU64,
    started_at_ms: u64,
    started_instant: Instant,
    log_path: Option<PathBuf>,
    log_handle: Mutex<Option<std::fs::File>>,
}

impl CallObserver {
    /// Create a new observer. If `log_path` is provided and its parent directory
    /// can be created, new records are also appended as JSONL; otherwise records
    /// are in-memory only.
    pub fn new(log_path: Option<PathBuf>) -> Self {
        let now_ms = unix_ms();

        // Best-effort directory setup. If this fails we fall back to in-memory only.
        let resolved_log_path = log_path.and_then(|p| match p.parent() {
            Some(dir) if std::fs::create_dir_all(dir).is_ok() => Some(p),
            Some(dir) => {
                warn!(
                    "[observability] could not create log directory '{}' — JSONL logging disabled",
                    dir.display()
                );
                None
            }
            None => None,
        });

        CallObserver {
            inner: Arc::new(Inner {
                recent: Mutex::new(VecDeque::with_capacity(MAX_RECENT_CALLS)),
                per_tool: Mutex::new(HashMap::new()),
                total_calls: AtomicU64::new(0),
                error_calls: AtomicU64::new(0),
                started_at_ms: now_ms,
                started_instant: Instant::now(),
                log_path: resolved_log_path,
                log_handle: Mutex::new(None),
            }),
        }
    }

    /// Record a completed call. Never fails — logs warnings on IO issues.
    pub async fn record(&self, rec: CallRecord) {
        // Update counters.
        self.inner.total_calls.fetch_add(1, Ordering::Relaxed);
        if matches!(rec.status, CallStatus::Error) {
            self.inner.error_calls.fetch_add(1, Ordering::Relaxed);
        }

        // Update per-tool stats.
        {
            let mut per_tool = self.inner.per_tool.lock().await;
            let entry = per_tool.entry(rec.tool.clone()).or_default();
            entry.total += 1;
            entry.total_duration_ms += rec.dur_ms;
            entry.last_status = Some(rec.status.as_str().to_string());
            if matches!(rec.status, CallStatus::Error) {
                entry.errors += 1;
                entry.last_error = rec.error_kind.clone();
            }
        }

        // Push to ring buffer.
        {
            let mut ring = self.inner.recent.lock().await;
            if ring.len() >= MAX_RECENT_CALLS {
                ring.pop_front();
            }
            ring.push_back(rec.clone());
        }

        // Append to JSONL (best effort).
        if let Some(path) = self.inner.log_path.as_ref() {
            if let Err(e) = self.append_jsonl(path, &rec).await {
                warn!("[observability] JSONL write failed: {}", e);
            }
        }
    }

    async fn append_jsonl(&self, path: &std::path::Path, rec: &CallRecord) -> std::io::Result<()> {
        let line = serde_json::to_string(rec)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let mut guard = self.inner.log_handle.lock().await;
        if guard.is_none() {
            *guard = Some(OpenOptions::new().create(true).append(true).open(path)?);
        }
        if let Some(file) = guard.as_mut() {
            writeln!(file, "{}", line)?;
            file.flush()?;
        }
        Ok(())
    }

    /// Return up to `limit` most-recent calls (newest first). `limit == 0` means
    /// the ring's current capacity (up to `MAX_RECENT_CALLS`).
    pub async fn recent(&self, limit: usize) -> Vec<CallRecord> {
        let ring = self.inner.recent.lock().await;
        let n = if limit == 0 {
            ring.len()
        } else {
            limit.min(ring.len())
        };
        ring.iter().rev().take(n).cloned().collect()
    }

    /// Snapshot of current stats for the `server_stats` meta-tool.
    pub async fn snapshot(&self) -> StatsSnapshot {
        let per_tool = self.inner.per_tool.lock().await.clone();
        let uptime_ms = self.inner.started_instant.elapsed().as_millis() as u64;
        StatsSnapshot {
            started_at_ms: self.inner.started_at_ms,
            uptime_ms,
            total_calls: self.inner.total_calls.load(Ordering::Relaxed),
            error_calls: self.inner.error_calls.load(Ordering::Relaxed),
            log_path: self
                .inner
                .log_path
                .as_ref()
                .map(|p| p.display().to_string()),
            per_tool,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct StatsSnapshot {
    pub started_at_ms: u64,
    pub uptime_ms: u64,
    pub total_calls: u64,
    pub error_calls: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_path: Option<String>,
    pub per_tool: HashMap<String, ToolStats>,
}

// ─── Path helpers ────────────────────────────────────────────────────────────

/// Platform-specific path for konnect state files (config, logs, cache).
/// Matches the convention in `tools/config.rs::user_config_dir`.
pub fn konnect_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var("APPDATA").unwrap_or_default();
        PathBuf::from(appdata).join("konnect")
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").unwrap_or_default();
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("konnect")
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let home = std::env::var("HOME").unwrap_or_default();
        PathBuf::from(home).join(".konnect")
    }
}

/// Default JSONL path for per-call logs.
pub fn default_calls_log_path() -> PathBuf {
    konnect_dir().join("logs").join("calls.jsonl")
}

/// Directory the run journal (`kam_state::RunJournal`) is opened under.
///
/// `KONNECT_STATE_DIR`, when set, wins outright — a test binary shares one
/// process-wide environment, so the harness sets it once
/// (`crates/konnect-core/tests/harness/mod.rs::ensure_state_dir`) to keep
/// every test's on-disk state under `CARGO_TARGET_TMPDIR` rather than letting
/// this resolve `APPDATA`/`HOME` and write into the real user profile that
/// happens to be running the test suite. Anything else falls back to
/// `konnect_dir()/journal`, the same convention `default_calls_log_path` uses
/// for `logs`.
pub fn journal_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("KONNECT_STATE_DIR") {
        let dir = PathBuf::from(dir);
        // Absolute only, like `konnect_sexp::writer`'s use of the same
        // variable: a relative state directory would put the journal wherever
        // the server happened to be started from, and one variable answering
        // to two different rules is worse than either rule.
        if dir.is_absolute() {
            return dir.join("journal");
        }
    }
    konnect_dir().join("journal")
}

// ─── Utilities ───────────────────────────────────────────────────────────────

pub fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Generate a short call id. We don't pull in `uuid` here — 16 hex chars of
/// randomness from the running thread's nanosecond + atomic counter is plenty
/// for disambiguating a few hundred calls per session.
pub fn new_call_id() -> String {
    static CTR: AtomicU64 = AtomicU64::new(0);
    let seq = CTR.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64)
        .unwrap_or(0);
    format!("{:08x}{:08x}", nanos, seq)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_record(tool: &str, status: CallStatus) -> CallRecord {
        CallRecord {
            call_id: new_call_id(),
            ts: unix_ms(),
            tool: tool.to_string(),
            toolset: Some("test".to_string()),
            dur_ms: 5,
            status,
            error_kind: if matches!(status, CallStatus::Error) {
                Some("boom".to_string())
            } else {
                None
            },
            args_bytes: 10,
            result_bytes: 20,
        }
    }

    #[tokio::test]
    async fn ring_buffer_bounds_at_max() {
        let obs = CallObserver::new(None);
        for _ in 0..(MAX_RECENT_CALLS + 50) {
            obs.record(sample_record("t", CallStatus::Ok)).await;
        }
        let recent = obs.recent(0).await;
        assert_eq!(recent.len(), MAX_RECENT_CALLS);
    }

    #[tokio::test]
    async fn per_tool_stats_track_errors() {
        let obs = CallObserver::new(None);
        obs.record(sample_record("add_wire", CallStatus::Ok)).await;
        obs.record(sample_record("add_wire", CallStatus::Ok)).await;
        obs.record(sample_record("add_wire", CallStatus::Error))
            .await;
        obs.record(sample_record("route_trace", CallStatus::Error))
            .await;

        let snap = obs.snapshot().await;
        assert_eq!(snap.total_calls, 4);
        assert_eq!(snap.error_calls, 2);

        let aw = snap.per_tool.get("add_wire").unwrap();
        assert_eq!(aw.total, 3);
        assert_eq!(aw.errors, 1);
        assert_eq!(aw.last_status.as_deref(), Some("error"));
        assert_eq!(aw.last_error.as_deref(), Some("boom"));

        let rt = snap.per_tool.get("route_trace").unwrap();
        assert_eq!(rt.errors, 1);
    }

    #[tokio::test]
    async fn jsonl_append_roundtrip() {
        // Use a tempdir so the observer can freely create + append to a file
        // inside it without fighting Windows delete-on-close semantics.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("calls.jsonl");
        let obs = CallObserver::new(Some(path.clone()));

        obs.record(sample_record("add_wire", CallStatus::Ok)).await;
        obs.record(sample_record("add_wire", CallStatus::Error))
            .await;

        let contents = std::fs::read_to_string(&path).expect("log read");
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("\"status\":\"ok\""));
        assert!(lines[1].contains("\"status\":\"error\""));
        assert!(lines[1].contains("\"error_kind\":\"boom\""));
    }

    #[test]
    fn call_ids_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for _ in 0..1000 {
            assert!(seen.insert(new_call_id()));
        }
    }
}
