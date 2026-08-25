//! `integration` toolset — JLCPCB parts database, datasheet enrichment, and Freerouting autorouter.
//!
//! JLCPCB tools query a local SQLite cache of the JLCPCB parts database.
//! Freerouting wraps the Freerouting JAR via subprocess.
//! Datasheet enrichment uses the LCSC HTTP API.
//!
//! The three network calls (JLCPCB database download, LCSC datasheet lookups)
//! go through `get_with_backoff`, which retries transient failures (network
//! errors, 429, 5xx) with exponential backoff before giving up.
//!
//! The three JLCPCB query tools (`search_jlcpcb_parts`, `get_jlcpcb_part`,
//! `suggest_jlcpcb_alternatives`) cache results in `ToolContext::jlcpcb_cache`
//! (5-minute TTL) to avoid re-running an identical SQLite query for repeated
//! lookups within a session. Responses carry a `"cached"` field so callers
//! can see whether a given result came from cache.

use crate::mcp::error::{ToolErrorKind, TransientClass};
use crate::mcp::protocol::CallToolResult;
use crate::mcp::retry;
use crate::tool;
use crate::tools::{get_path, require_str, ToolContext, ToolDef};
use crate::try_arg;
use konnect_sexp::writer::{read_consistent, write_atomic_if_unchanged};
use serde_json::json;
use std::path::PathBuf;

// ─── Tool definitions ─────────────────────────────────────────────────────────

pub fn tools() -> Vec<ToolDef> {
    vec![
        tool!(
            "download_jlcpcb_database",
            "Download or update the local JLCPCB component parts database cache (SQLite). \
             Fetches the chunked archive published by kicad-jlcpcb-tools and inflates it. \
             Sizes differ by orders of magnitude between libraries: 'basic-preferred' is ~2 MB, \
             'current-parts' ~780 MB, 'all-parts' several GB.",
            json!({
                "type": "object",
                "properties": {
                    "output_path": { "type": "string", "description": "Local path to store the SQLite database file (optional, uses config default)" },
                    "force": { "type": "boolean", "description": "Force re-download even if cache exists", "default": false },
                    "library": {
                        "type": "string",
                        "description": "Which published parts library to fetch",
                        "enum": ["basic-preferred", "current-parts", "all-parts", "empty"],
                        "default": "basic-preferred"
                    },
                    "base_url": { "type": "string", "description": "Override the upstream base URL, e.g. an internal mirror (optional)" }
                },
                "required": []
            }),
            |args, ctx| async move { handle_download_jlcpcb(args, ctx).await }
        ),
        tool!(
            "search_jlcpcb_parts",
            "Search the local JLCPCB component database by keyword, value, or category.",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search string (MPN, description, or value)" },
                    "category": { "type": "string", "description": "Component category filter (optional)" },
                    "basic_only": { "type": "boolean", "description": "Restrict to JLCPCB Basic Library parts only", "default": false },
                    "in_stock": { "type": "boolean", "description": "Only return parts currently in stock", "default": true },
                    "limit": { "type": "integer", "description": "Maximum number of results", "default": 20 }
                },
                "required": ["query"]
            }),
            |args, ctx| async move { handle_search_jlcpcb_parts(args, ctx).await }
        ),
        tool!(
            "get_jlcpcb_part",
            "Retrieve full details for a single JLCPCB part by its LCSC part number.",
            json!({
                "type": "object",
                "properties": {
                    "lcsc_id": { "type": "string", "description": "LCSC part number (e.g. 'C14663')" }
                },
                "required": ["lcsc_id"]
            }),
            |args, ctx| async move { handle_get_jlcpcb_part(args, ctx).await }
        ),
        tool!(
            "suggest_jlcpcb_alternatives",
            "Suggest JLCPCB-stocked alternative parts for a given component value and footprint.",
            json!({
                "type": "object",
                "properties": {
                    "value": { "type": "string", "description": "Component value (e.g. '100nF')" },
                    "footprint": { "type": "string", "description": "KiCAD footprint identifier" },
                    "max_price_usd": { "type": "number", "description": "Maximum unit price in USD (optional)" },
                    "limit": { "type": "integer", "description": "Maximum number of suggestions", "default": 5 }
                },
                "required": ["value", "footprint"]
            }),
            |args, ctx| async move { handle_suggest_alternatives(args, ctx).await }
        ),
        tool!(
            "get_jlcpcb_database_stats",
            "Return statistics about the local JLCPCB database cache: part count, last updated, file size.",
            json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
            |args, ctx| async move { handle_jlcpcb_stats(args, ctx).await }
        ),
        tool!(
            "enrich_datasheets",
            "Fetch and cache datasheet URLs for all components in a schematic using the LCSC API.",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string", "description": "Path to .kicad_sch file" },
                    "overwrite_existing": { "type": "boolean", "description": "Replace existing Datasheet fields", "default": false }
                },
                "required": ["schematic"]
            }),
            |args, ctx| async move { handle_enrich_datasheets(args, ctx).await }
        ),
        tool!(
            "get_datasheet_url",
            "Retrieve the datasheet URL for a component by MPN or LCSC ID.",
            json!({
                "type": "object",
                "properties": {
                    "mpn": { "type": "string", "description": "Manufacturer part number (optional)" },
                    "lcsc_id": { "type": "string", "description": "LCSC part number (optional)" }
                },
                // P.6.9.16: the handler needs `mpn` *or* `lcsc_id`, a contract
                // `required` alone cannot express (it is a conjunction). The
                // disjunction is published in `anyOf` instead, and `required`
                // stays empty since neither key alone is mandatory.
                "required": [],
                "anyOf": [
                    { "required": ["mpn"] },
                    { "required": ["lcsc_id"] }
                ]
            }),
            |args, ctx| async move { handle_get_datasheet_url(args, ctx).await }
        ),
        tool!(
            "autoroute",
            "Run Freerouting autorouter on the PCB: export DSN → autoroute → import SES result.",
            json!({
                "type": "object",
                "properties": {
                    "board": { "type": "string", "description": "Path to .kicad_pcb file" },
                    "passes": { "type": "integer", "description": "Number of autorouter passes", "default": 3 },
                    "timeout_seconds": { "type": "integer", "description": "Maximum autorouter runtime in seconds", "default": 120 },
                    "jar_path": { "type": "string", "description": "Path to freerouting.jar (optional, uses config default)" }
                },
                // P.6.9.16: `handle_autoroute` takes `_args` and always answers
                // `ManualStepRequired` — the DSN/SES round trip it needs was
                // removed from kicad-cli in KiCAD 10. Nothing reads `board`, so
                // nothing may be required; it stays in `properties` for the day
                // the IPC round trip lands and the tool does real work again.
                "required": []
            }),
            |args, ctx| async move { handle_autoroute(args, ctx).await }
        ),
        tool!(
            "check_freerouting",
            "Verify that the Freerouting JAR is available and return its version.",
            json!({
                "type": "object",
                "properties": {
                    "jar_path": { "type": "string", "description": "Path to freerouting.jar (optional, uses config default)" }
                },
                "required": []
            }),
            |args, ctx| async move { handle_check_freerouting(args, ctx).await }
        ),
    ]
}

// ─── The published JLCPCB parts database ─────────────────────────────────────
//
// `kicad-jlcpcb-tools` publishes the database on GitHub Pages, split into
// 80 MB chunks of one deflate archive: `<db-file>.zip.001`, `.002`, ... The
// chunks are numbered from 1 and a plain-text manifest holds how many there
// are. There is no single-file download and no `jlcpcb_parts.db` — that name,
// which this tool used to fetch, has never existed at this host in this form
// (J.2.4.3).

/// Base of every published artifact. Overridable per call for a mirror.
const JLCPCB_BASE_URL: &str = "https://bouni.github.io/kicad-jlcpcb-tools";

/// The table the published database keeps its parts in (an FTS5 virtual table).
const JLCPCB_PARTS_TABLE: &str = "parts";

/// Fetching the whole catalogue by default would mean ~780 MB of SQLite for a
/// caller who asked for "the database"; the Basic/Preferred subset is ~2 MB and
/// is what an assembly-cost decision actually needs. The larger libraries are a
/// deliberate opt-in through `library`.
const DEFAULT_JLCPCB_LIBRARY: &str = "basic-preferred";

struct JlcpcbLibrary {
    /// Tool-facing name.
    name: &'static str,
    /// Published file name, which is also the archive-entry name.
    db_file_name: &'static str,
    /// Plain-text file holding the chunk count.
    chunk_manifest: &'static str,
}

const JLCPCB_LIBRARIES: [JlcpcbLibrary; 4] = [
    JlcpcbLibrary {
        name: "basic-preferred",
        db_file_name: "basic-parts-fts5.db",
        chunk_manifest: "chunk_num_basic_parts_fts5.txt",
    },
    JlcpcbLibrary {
        name: "current-parts",
        db_file_name: "current-parts-fts5.db",
        chunk_manifest: "chunk_num_current_parts_fts5.txt",
    },
    JlcpcbLibrary {
        name: "all-parts",
        db_file_name: "parts-fts5.db",
        chunk_manifest: "chunk_num_fts5.txt",
    },
    JlcpcbLibrary {
        name: "empty",
        db_file_name: "empty-parts-fts5.db",
        chunk_manifest: "chunk_num_empty_parts_fts5.txt",
    },
];

fn jlcpcb_library(name: &str) -> Option<&'static JlcpcbLibrary> {
    JLCPCB_LIBRARIES.iter().find(|v| v.name == name)
}

// ─── JLCPCB database path helper ─────────────────────────────────────────────

fn default_jlcpcb_db_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var("APPDATA").unwrap_or_default();
        PathBuf::from(appdata).join("konnect").join("jlcpcb.db")
    }
    #[cfg(not(target_os = "windows"))]
    {
        let home = std::env::var("HOME").unwrap_or_default();
        PathBuf::from(home).join(".konnect").join("jlcpcb.db")
    }
}

fn resolve_db_path(args: &serde_json::Value, ctx: &ToolContext) -> PathBuf {
    if let Some(p) = args["output_path"].as_str() {
        return PathBuf::from(p);
    }
    if let Some(p) = &ctx.config.jlcpcb_db_path {
        return p.clone();
    }
    default_jlcpcb_db_path()
}

// ─── Retry/backoff for external HTTP calls ────────────────────────────────────
//
// JLCPCB database download and LCSC datasheet lookups are the only genuinely
// networked calls in this toolset (everything else queries the local SQLite
// cache). Both are prone to transient failures — timeouts, connection resets,
// rate limiting — that a simple retry clears up without any user action.

/// Retry policy: 3 attempts total, exponential backoff starting at 300ms
/// (300ms, then 600ms between attempts).
const RETRY_MAX_ATTEMPTS: u32 = 3;
const RETRY_BASE_DELAY: std::time::Duration = std::time::Duration::from_millis(300);

/// Classifies an HTTP status the way [`crate::mcp::retry`] wants it: 429
/// (rate limited) and 5xx (server-side) are `Network` — the same call may
/// work once the far side recovers. Other 4xx (404, 401, ...) are `None` —
/// the request itself is wrong, and retrying a "not found" or "unauthorized"
/// wastes time without changing the outcome.
fn classify_status(status: reqwest::StatusCode) -> TransientClass {
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
        TransientClass::Network
    } else {
        TransientClass::None
    }
}

/// Whether an HTTP status is worth retrying, per the shared [`retry::decide`]
/// policy rather than a rule reimplemented here.
fn is_transient_status(status: reqwest::StatusCode) -> bool {
    retry::decide(classify_status(status)).should_retry
}

/// Delay before the next attempt, given the attempt number just made (1-based).
fn backoff_delay(attempt: u32) -> std::time::Duration {
    RETRY_BASE_DELAY * 2u32.pow(attempt.saturating_sub(1))
}

/// GET `url` with retry/backoff for transient failures (network-level errors,
/// 429, and 5xx). Returns the last response/error once attempts are exhausted.
async fn get_with_backoff(
    client: &reqwest::Client,
    url: &str,
) -> anyhow::Result<reqwest::Response> {
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        match client.get(url).send().await {
            Ok(resp) => {
                let status = resp.status();
                if !is_transient_status(status) || attempt >= RETRY_MAX_ATTEMPTS {
                    return Ok(resp);
                }
                tracing::warn!(
                    "[BETA] {} returned {} (attempt {}/{}), retrying",
                    url,
                    status,
                    attempt,
                    RETRY_MAX_ATTEMPTS
                );
            }
            Err(e) => {
                let class = if e.is_timeout() {
                    TransientClass::Timeout
                } else {
                    TransientClass::Network
                };
                if !retry::decide(class).should_retry || attempt >= RETRY_MAX_ATTEMPTS {
                    return Err(e.into());
                }
                tracing::warn!(
                    "[BETA] request to {} failed (attempt {}/{}): {}, retrying",
                    url,
                    attempt,
                    RETRY_MAX_ATTEMPTS,
                    e
                );
            }
        }
        tokio::time::sleep(backoff_delay(attempt)).await;
    }
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

async fn handle_download_jlcpcb(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let db_path = resolve_db_path(args, ctx);
    let force = args["force"].as_bool().unwrap_or(false);

    if db_path.exists() && !force {
        let meta = tokio::fs::metadata(&db_path).await?;
        return Ok(CallToolResult::text(
            serde_json::to_string(&json!({
                "status": "already_exists",
                "path": db_path.to_str().unwrap_or(""),
                "size_bytes": meta.len(),
                "note": "Use force=true to re-download"
            }))
            .unwrap(),
        ));
    }

    let library = args["library"].as_str().unwrap_or(DEFAULT_JLCPCB_LIBRARY);
    let Some(variant) = jlcpcb_library(library) else {
        return Ok(CallToolResult::error_kind(
            ToolErrorKind::InvalidArgument {
                field: "library".to_string(),
                reason: format!("unknown library '{library}'"),
            },
            format!(
                "Unknown library '{}'. Available: {}",
                library,
                JLCPCB_LIBRARIES
                    .iter()
                    .map(|v| v.name)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ));
    };
    let base_url = args["base_url"]
        .as_str()
        .unwrap_or(JLCPCB_BASE_URL)
        .trim_end_matches('/')
        .to_string();

    if let Some(parent) = db_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    // Both temporaries sit next to the destination so the final step is a
    // same-filesystem rename, and so a failure never leaves a half-written or
    // un-inflated file where a query tool would find it and read it as the
    // database.
    let archive_path = sibling(&db_path, "download");
    let inflated_path = sibling(&db_path, "inflating");
    let discard = || async {
        let _ = tokio::fs::remove_file(&archive_path).await;
        let _ = tokio::fs::remove_file(&inflated_path).await;
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()?;

    // The archive is split into fixed-size chunks; a plain-text manifest holds
    // how many, and they are numbered from 1.
    let manifest_url = format!("{}/{}", base_url, variant.chunk_manifest);
    let manifest = get_with_backoff(&client, &manifest_url).await?;
    if !manifest.status().is_success() {
        return Ok(upstream_failed(
            &manifest_url,
            http_status_code(manifest.status()),
            format!(
                "Could not read the chunk manifest at {}: HTTP {}",
                manifest_url,
                manifest.status()
            ),
        ));
    }
    let manifest_body = manifest.text().await?;
    let Ok(chunk_count) = manifest_body.trim().parse::<u32>() else {
        return Ok(upstream_failed(
            &manifest_url,
            "unexpected_response",
            format!(
                "The chunk manifest at {} is not a chunk count: {:?}",
                manifest_url,
                manifest_body.chars().take(80).collect::<String>()
            ),
        ));
    };
    if chunk_count == 0 {
        return Ok(upstream_failed(
            &manifest_url,
            "unexpected_response",
            format!(
                "The chunk manifest at {manifest_url} reports 0 chunks, so there is nothing to fetch"
            ),
        ));
    }

    let mut downloaded_bytes: u64 = 0;
    {
        let mut archive = tokio::fs::File::create(&archive_path).await?;
        for index in 1..=chunk_count {
            let chunk_url = format!("{}/{}.zip.{:03}", base_url, variant.db_file_name, index);
            let mut resp = match get_with_backoff(&client, &chunk_url).await {
                Ok(resp) => resp,
                Err(e) => {
                    drop(archive);
                    discard().await;
                    return Ok(upstream_failed(
                        &chunk_url,
                        "unreachable",
                        format!(
                            "Chunk {index}/{chunk_count} could not be fetched from {chunk_url}: {e}"
                        ),
                    ));
                }
            };
            if !resp.status().is_success() {
                let status = resp.status();
                drop(archive);
                discard().await;
                return Ok(upstream_failed(
                    &chunk_url,
                    http_status_code(status),
                    format!("Chunk {index}/{chunk_count} failed: HTTP {status} for {chunk_url}"),
                ));
            }
            while let Some(bytes) = resp.chunk().await? {
                downloaded_bytes += bytes.len() as u64;
                tokio::io::AsyncWriteExt::write_all(&mut archive, &bytes).await?;
            }
        }
        tokio::io::AsyncWriteExt::flush(&mut archive).await?;
    }

    let inflate = tokio::task::spawn_blocking({
        let archive_path = archive_path.clone();
        let inflated_path = inflated_path.clone();
        let library = variant.name.to_string();
        let source = format!("{}/{}", base_url, variant.db_file_name);
        move || inflate_and_validate(&archive_path, &inflated_path, &library, &source)
    })
    .await?;

    let part_count = match inflate {
        Ok(count) => count,
        Err(e) => {
            discard().await;
            // The transfer succeeded, so this is the service's answer being
            // wrong rather than the network: replaying the download fetches
            // the same bytes.
            return Ok(upstream_failed(
                &base_url,
                "unexpected_response",
                format!("The downloaded archive is not a usable JLCPCB database: {e}"),
            ));
        }
    };

    let size_bytes = tokio::fs::metadata(&inflated_path).await?.len();
    tokio::fs::rename(&inflated_path, &db_path).await?;
    let _ = tokio::fs::remove_file(&archive_path).await;

    Ok(CallToolResult::text(
        serde_json::to_string(&json!({
            "success": true,
            "path": db_path.to_str().unwrap_or(""),
            "library": variant.name,
            "chunks": chunk_count,
            "downloaded_bytes": downloaded_bytes,
            "size_bytes": size_bytes,
            "part_count": part_count
        }))
        .unwrap(),
    ))
}

/// A temporary beside `path`, distinguished by suffix rather than by
/// `with_extension` so `jlcpcb.db` and `jlcpcb.sqlite3` cannot collide.
fn sibling(path: &std::path::Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{suffix}"));
    path.with_file_name(name)
}

/// Inflate the single database entry out of the concatenated chunks, then prove
/// the result is really the parts database before anything is renamed into
/// place: a 200 that served an error page, a truncated chunk set, or a schema
/// change upstream all land here rather than in a query tool later.
///
/// Returns the part count.
fn inflate_and_validate(
    archive_path: &std::path::Path,
    inflated_path: &std::path::Path,
    library: &str,
    source_url: &str,
) -> anyhow::Result<i64> {
    let file = std::fs::File::open(archive_path)?;
    let mut archive = zip::ZipArchive::new(std::io::BufReader::new(file))
        .map_err(|e| anyhow::anyhow!("not a ZIP archive: {e}"))?;
    let entry_name = (0..archive.len())
        .filter_map(|i| archive.name_for_index(i).map(String::from))
        .find(|n| n.ends_with(".db"))
        .ok_or_else(|| anyhow::anyhow!("the archive holds no .db entry"))?;
    {
        let mut entry = archive.by_name(&entry_name)?;
        let mut out = std::io::BufWriter::new(std::fs::File::create(inflated_path)?);
        std::io::copy(&mut entry, &mut out)?;
        std::io::Write::flush(&mut out)?;
    }

    let conn = rusqlite::Connection::open(inflated_path)?;
    let part_count: i64 = conn
        .query_row(
            &format!("SELECT COUNT(*) FROM {JLCPCB_PARTS_TABLE}"),
            [],
            |r| r.get(0),
        )
        .map_err(|e| {
            anyhow::anyhow!("no readable '{JLCPCB_PARTS_TABLE}' table in {entry_name}: {e}")
        })?;

    // Which library this is cannot be read back out of the file, and the
    // difference between 1 600 parts and 700 000 decides whether "not found"
    // means "not stocked" or "wrong library". Record it.
    let downloaded_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    conn.execute(
        "CREATE TABLE IF NOT EXISTS konnect_source \
         (library TEXT, source_url TEXT, downloaded_at_unix INTEGER, part_count INTEGER)",
        [],
    )?;
    conn.execute("DELETE FROM konnect_source", [])?;
    conn.execute(
        "INSERT INTO konnect_source (library, source_url, downloaded_at_unix, part_count) \
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![library, source_url, downloaded_at, part_count],
    )?;

    Ok(part_count)
}

/// Build a deterministic cache key from a tool name, the resolved DB path
/// (so pointing at a different `output_path` never serves stale results),
/// and the query parameters that affect the result set.
fn cache_key(tool: &str, db_path: &std::path::Path, parts: &[&str]) -> String {
    format!("{}|{}|{}", tool, db_path.display(), parts.join("|"))
}

/// One `UpstreamFailed` result for the download and lookup paths.
///
/// `service` is the host, not the URL: a kind a client matches on should not
/// carry a path that changes per chunk. The URL stays in the message, which is
/// the prose the site already wrote.
fn upstream_failed(url: &str, code: &'static str, message: String) -> CallToolResult {
    let service = url
        .split_once("://")
        .map_or(url, |(_, rest)| rest)
        .split('/')
        .next()
        .unwrap_or(url)
        .to_string();
    CallToolResult::error_kind(
        ToolErrorKind::UpstreamFailed {
            service,
            code,
            detail: message.clone(),
        },
        message,
    )
}

/// Whether an HTTP failure status is worth waiting on.
///
/// 429 sits with the 5xx deliberately: it is the one 4xx that says "later",
/// and grouping it with 404 would tell a client to give up on a rate limit.
fn http_status_code(status: reqwest::StatusCode) -> &'static str {
    if status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        "server_error"
    } else {
        "client_error"
    }
}

async fn handle_search_jlcpcb_parts(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    // Arguments first, database second: a caller who forgot a `required`
    // argument used to be sent to download a 2.5-million-part catalogue for a
    // call that could never run. `query` is `required` and was substituted
    // with "", which `LIKE '%%'` matches everywhere.
    let query = try_arg!(require_str(args, "query")).to_string();
    let db_path = resolve_db_path(args, ctx);
    if !db_path.exists() {
        return Ok(CallToolResult::error_kind(
            ToolErrorKind::FileNotFound {
                path: db_path.display().to_string(),
            },
            "JLCPCB database not found. Run download_jlcpcb_database first.",
        ));
    }

    let basic_only = args["basic_only"].as_bool().unwrap_or(false);
    let in_stock = args["in_stock"].as_bool().unwrap_or(true);
    let limit = args["limit"].as_u64().unwrap_or(20) as usize;
    let category = args["category"].as_str().map(String::from);

    let key = cache_key(
        "search_jlcpcb_parts",
        &db_path,
        &[
            &query,
            category.as_deref().unwrap_or(""),
            &basic_only.to_string(),
            &in_stock.to_string(),
            &limit.to_string(),
        ],
    );
    if let Some(cached) = ctx.jlcpcb_cache.get(&key) {
        let mut body = cached;
        body["cached"] = json!(true);
        return Ok(CallToolResult::text(serde_json::to_string(&body).unwrap()));
    }

    let searched_db = db_path.clone();
    let results = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<serde_json::Value>> {
        let conn = rusqlite::Connection::open(&db_path)?;

        let mut sql = format!(
            "SELECT {JLCPCB_PART_COLUMNS} FROM {JLCPCB_PARTS_TABLE} \
             WHERE (Description LIKE ?1 OR \"MFR.Part\" LIKE ?1)"
        );
        if basic_only {
            sql.push_str(" AND \"Library Type\" = 'Basic'");
        }
        if in_stock {
            sql.push_str(" AND CAST(Stock AS INTEGER) > 0");
        }
        if category.is_some() {
            sql.push_str(" AND (\"First Category\" LIKE ?2 OR \"Second Category\" LIKE ?2)");
        }
        sql.push_str(&format!(" LIMIT {}", limit));

        let like_query = format!("%{}%", query);
        let mut stmt = conn.prepare(&sql)?;

        let rows: Vec<serde_json::Value> = if category.is_some() {
            let cat_like = format!("%{}%", category.as_deref().unwrap_or(""));
            stmt.query_map(rusqlite::params![like_query, cat_like], row_to_part_json)?
                .filter_map(|r| r.ok())
                .collect()
        } else {
            stmt.query_map(rusqlite::params![like_query], row_to_part_json)?
                .filter_map(|r| r.ok())
                .collect()
        };
        Ok(rows)
    })
    .await??;

    let mut body = json!({
        "query": args["query"].as_str().unwrap_or(""),
        "count": results.len(),
        "results": results
    });
    // A search that finds nothing in the ~1 600-part Basic/Preferred subset and
    // one that finds nothing in the full catalogue mean different things, and
    // the caller cannot see which database is on disk.
    if results.is_empty() {
        if let Some(note) = provenance_note(&searched_db).await {
            body["note"] = json!(note);
        }
    }
    ctx.jlcpcb_cache.put(key, body.clone());

    body["cached"] = json!(false);
    Ok(CallToolResult::text(serde_json::to_string(&body).unwrap()))
}

/// The columns of the published `parts` table, in the order `row_to_part_json`
/// reads them. Quoted: the published names carry spaces and a dot.
const JLCPCB_PART_COLUMNS: &str = "\"LCSC Part\", \"MFR.Part\", Package, Manufacturer, \
     \"Library Type\", Description, Datasheet, Price, Stock, \"First Category\", \
     \"Second Category\"";

/// `Price` is a tier string — `1-199:0.018,200-599:0.015,...` — not a number,
/// so a `price` field and an `ORDER BY Price` both need parsing first. This is
/// the unit price at quantity 1, i.e. the first tier.
fn unit_price_usd(tiers: &str) -> Option<f64> {
    tiers
        .split(',')
        .next()?
        .rsplit(':')
        .next()?
        .trim()
        .parse::<f64>()
        .ok()
}

fn row_to_part_json(row: &rusqlite::Row) -> rusqlite::Result<serde_json::Value> {
    let price_tiers: String = row.get::<_, String>(7).unwrap_or_default();
    // `Stock` is stored as text as well.
    let stock = row
        .get::<_, String>(8)
        .unwrap_or_default()
        .trim()
        .parse::<i64>()
        .unwrap_or(0);
    let first_category: String = row.get::<_, String>(9).unwrap_or_default();
    let second_category: String = row.get::<_, String>(10).unwrap_or_default();
    Ok(json!({
        "lcsc": row.get::<_, String>(0).unwrap_or_default(),
        "mpn": row.get::<_, String>(1).unwrap_or_default(),
        "package": row.get::<_, String>(2).unwrap_or_default(),
        "manufacturer": row.get::<_, String>(3).unwrap_or_default(),
        "library_type": row.get::<_, String>(4).unwrap_or_default(),
        "description": row.get::<_, String>(5).unwrap_or_default(),
        "datasheet": row.get::<_, String>(6).unwrap_or_default(),
        "price": unit_price_usd(&price_tiers),
        "price_tiers": price_tiers,
        "stock": stock,
        "category": if second_category.is_empty() { first_category.clone() } else { format!("{first_category} / {second_category}") }
    }))
}

/// What `download_jlcpcb_database` recorded about the database on disk, phrased
/// for a caller who got no results. `None` when the file predates the
/// provenance table or is not readable.
async fn provenance_note(db_path: &std::path::Path) -> Option<String> {
    let db_path = db_path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let conn = rusqlite::Connection::open(&db_path).ok()?;
        let (library, part_count): (String, i64) = conn
            .query_row(
                "SELECT library, part_count FROM konnect_source LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok()?;
        Some(format!(
            "searched the '{library}' library ({part_count} parts); \
             re-run download_jlcpcb_database with library='current-parts' or 'all-parts' \
             for the full catalogue"
        ))
    })
    .await
    .ok()
    .flatten()
}

async fn handle_get_jlcpcb_part(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let lcsc_id = try_arg!(require_str(args, "lcsc_id")).to_string();
    let db_path = resolve_db_path(args, ctx);
    if !db_path.exists() {
        return Ok(CallToolResult::error_kind(
            ToolErrorKind::FileNotFound {
                path: db_path.display().to_string(),
            },
            "JLCPCB database not found. Run download_jlcpcb_database first.",
        ));
    }

    let key = cache_key("get_jlcpcb_part", &db_path, &[&lcsc_id]);
    if let Some(mut cached) = ctx.jlcpcb_cache.get(&key) {
        cached["cached"] = json!(true);
        return Ok(CallToolResult::text(
            serde_json::to_string(&cached).unwrap(),
        ));
    }

    // Cloned for the blocking closure: the originals are what names the
    // document and the key if the lookup finds nothing.
    let query_db_path = db_path.clone();
    let query_lcsc_id = lcsc_id.clone();
    let result =
        tokio::task::spawn_blocking(move || -> anyhow::Result<Option<serde_json::Value>> {
            let conn = rusqlite::Connection::open(&query_db_path)?;
            let mut stmt = conn.prepare(&format!(
                "SELECT {JLCPCB_PART_COLUMNS} FROM {JLCPCB_PARTS_TABLE} \
                 WHERE \"LCSC Part\" = ?1 LIMIT 1"
            ))?;
            let mut rows = stmt.query_map(rusqlite::params![query_lcsc_id], row_to_part_json)?;
            Ok(rows.next().and_then(|r| r.ok()))
        })
        .await??;

    match result {
        Some(part) => {
            ctx.jlcpcb_cache.put(key, part.clone());
            let mut part = part;
            part["cached"] = json!(false);
            Ok(CallToolResult::text(serde_json::to_string(&part).unwrap()))
        }
        None => Ok(CallToolResult::error_kind(
            ToolErrorKind::NotFound {
                document: db_path.display().to_string(),
                item_kind: "part".to_string(),
                key: lcsc_id.clone(),
                candidates: Vec::new(),
            },
            format!("Part not found in database: {lcsc_id}"),
        )),
    }
}

async fn handle_suggest_alternatives(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    // Both are `required`; substituting "" made the query `LIKE '%%'` on both
    // columns — every stocked part as an "alternative" — and cached it.
    let value = try_arg!(require_str(args, "value")).to_string();
    let footprint = try_arg!(require_str(args, "footprint")).to_string();
    let db_path = resolve_db_path(args, ctx);
    if !db_path.exists() {
        return Ok(CallToolResult::error_kind(
            ToolErrorKind::FileNotFound {
                path: db_path.display().to_string(),
            },
            "JLCPCB database not found. Run download_jlcpcb_database first.",
        ));
    }
    let max_price = args["max_price_usd"].as_f64();
    let limit = args["limit"].as_u64().unwrap_or(5) as usize;

    // Extract package from footprint (e.g. "Resistor_SMD:R_0402" → "0402")
    let package_hint = footprint
        .split(':')
        .next_back()
        .unwrap_or("")
        .split('_')
        .next_back()
        .unwrap_or("")
        .to_string();

    let key = cache_key(
        "suggest_jlcpcb_alternatives",
        &db_path,
        &[
            &value,
            &footprint,
            &max_price.map(|v| v.to_string()).unwrap_or_default(),
            &limit.to_string(),
        ],
    );
    if let Some(cached) = ctx.jlcpcb_cache.get(&key) {
        let mut body = cached;
        body["cached"] = json!(true);
        return Ok(CallToolResult::text(serde_json::to_string(&body).unwrap()));
    }

    let results = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<serde_json::Value>> {
        let conn = rusqlite::Connection::open(&db_path)?;
        let like_val = format!("%{}%", value);
        let like_pkg = format!("%{}%", package_hint);

        // `Price` is a tier string, so neither the `max_price_usd` cut nor the
        // cheapest-first ordering can be done in SQL. A bounded candidate pool
        // is read out and both are applied to the parsed unit price.
        let sql = format!(
            "SELECT {JLCPCB_PART_COLUMNS} FROM {JLCPCB_PARTS_TABLE} \
             WHERE Description LIKE ?1 AND Package LIKE ?2 AND CAST(Stock AS INTEGER) > 0 \
             LIMIT {}",
            (limit * 20).max(100)
        );

        let mut stmt = conn.prepare(&sql)?;
        let mut rows: Vec<serde_json::Value> = stmt
            .query_map(rusqlite::params![like_val, like_pkg], row_to_part_json)?
            .filter_map(|r| r.ok())
            .filter(|part| match (max_price, part["price"].as_f64()) {
                (Some(max_p), Some(price)) => price <= max_p,
                // An unpriced part cannot be shown to meet a price cap.
                (Some(_), None) => false,
                (None, _) => true,
            })
            .collect();
        rows.sort_by(|a, b| {
            let key = |p: &serde_json::Value| p["price"].as_f64().unwrap_or(f64::MAX);
            key(a).total_cmp(&key(b))
        });
        rows.truncate(limit);
        Ok(rows)
    })
    .await??;

    let body = json!({
        "value": args["value"].as_str().unwrap_or(""),
        "footprint": args["footprint"].as_str().unwrap_or(""),
        "alternatives": results
    });
    ctx.jlcpcb_cache.put(key, body.clone());

    let mut body = body;
    body["cached"] = json!(false);
    Ok(CallToolResult::text(serde_json::to_string(&body).unwrap()))
}

async fn handle_jlcpcb_stats(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let db_path = resolve_db_path(args, ctx);
    if !db_path.exists() {
        return Ok(CallToolResult::text(
            serde_json::to_string(&json!({
                "exists": false,
                "note": "Run download_jlcpcb_database to fetch the parts database"
            }))
            .unwrap(),
        ));
    }

    let meta = tokio::fs::metadata(&db_path).await?;
    let size_bytes = meta.len();

    let stats = tokio::task::spawn_blocking({
        let db_path = db_path.clone();
        move || -> anyhow::Result<DatabaseStats> {
            let conn = rusqlite::Connection::open(&db_path)?;
            let part_count: i64 = conn.query_row(
                &format!("SELECT COUNT(*) FROM {JLCPCB_PARTS_TABLE}"),
                [],
                |r| r.get(0),
            )?;
            // Written by `download_jlcpcb_database`; absent if the file came
            // from somewhere else.
            let source = conn
                .query_row(
                    "SELECT library, downloaded_at_unix FROM konnect_source LIMIT 1",
                    [],
                    |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
                )
                .ok();
            Ok(DatabaseStats {
                part_count,
                library: source.as_ref().map(|s| s.0.clone()),
                downloaded_at_unix: source.map(|s| s.1),
                // Written by the upstream build: when the catalogue was scraped.
                upstream_last_update: conn
                    .query_row("SELECT last_update FROM meta LIMIT 1", [], |r| {
                        r.get::<_, String>(0)
                    })
                    .ok(),
            })
        }
    })
    .await??;

    Ok(CallToolResult::text(
        serde_json::to_string(&json!({
            "exists": true,
            "path": db_path.to_str().unwrap_or(""),
            "size_bytes": size_bytes,
            "part_count": stats.part_count,
            "library": stats.library,
            "downloaded_at_unix": stats.downloaded_at_unix,
            "upstream_last_update": stats.upstream_last_update
        }))
        .unwrap(),
    ))
}

/// What can be read back out of a downloaded database: its size in parts, and
/// the two provenance records — ours (which library, when fetched) and
/// upstream's (when the catalogue was scraped).
struct DatabaseStats {
    part_count: i64,
    library: Option<String>,
    downloaded_at_unix: Option<i64>,
    upstream_last_update: Option<String>,
}

async fn handle_enrich_datasheets(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let overwrite = args["overwrite_existing"].as_bool().unwrap_or(false);

    let read_path = sch_path.clone();
    let content = tokio::task::spawn_blocking(move || read_consistent(&read_path)).await??;

    // Find all LCSC property values in the schematic
    let mut lcsc_ids: Vec<String> = Vec::new();
    let mut search = content.as_str();
    while let Some(pos) = search.find("(property \"LCSC\" \"") {
        let after = &search[pos + 18..];
        if let Some(end) = after.find('"') {
            lcsc_ids.push(after[..end].to_string());
        }
        search = &search[pos + 1..];
    }
    lcsc_ids.sort();
    lcsc_ids.dedup();

    if lcsc_ids.is_empty() {
        return Ok(CallToolResult::text(
            serde_json::to_string(&json!({
                "updated": 0,
                "note": "No LCSC property found in schematic components"
            }))
            .unwrap(),
        ));
    }

    // Query LCSC API for datasheet URLs
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let mut enriched = 0usize;
    let mut new_content = content.clone();

    for lcsc_id in &lcsc_ids {
        let url = format!(
            "https://wmsc.lcsc.com/ftps/wm/product/detail?productCode={}",
            lcsc_id
        );
        if let Ok(resp) = get_with_backoff(&client, &url).await {
            if resp.status().is_success() {
                if let Ok(json_resp) = resp.json::<serde_json::Value>().await {
                    if let Some(datasheet_url) = json_resp
                        .pointer("/result/dataManualUrl")
                        .and_then(|v| v.as_str())
                    {
                        // Find components with this LCSC ID and update their Datasheet property.
                        // Pattern: find (property "LCSC" "CxxxID") → walk back to symbol block →
                        // find (property "Datasheet" "...") and replace the URL.
                        let lcsc_pat = format!(r#"(property "LCSC" "{}")"#, lcsc_id);
                        let mut search_from = 0usize;
                        while let Some(lcsc_pos) = new_content[search_from..]
                            .find(&lcsc_pat)
                            .map(|i| i + search_from)
                        {
                            // Find the enclosing symbol block
                            let before = &new_content[..lcsc_pos];
                            if let Some(sym_start) = before.rfind("\n  (symbol") {
                                let sym_block = &new_content[sym_start..];
                                // Find Datasheet property within this symbol
                                let ds_pat = r#"(property "Datasheet" ""#;
                                if let Some(ds_offset) = sym_block.find(ds_pat) {
                                    let ds_abs = sym_start + ds_offset + ds_pat.len();
                                    if let Some(ds_end) = new_content[ds_abs..].find('"') {
                                        let existing = &new_content[ds_abs..ds_abs + ds_end];
                                        if overwrite || existing == "~" || existing.is_empty() {
                                            new_content = format!(
                                                "{}{}{}",
                                                &new_content[..ds_abs],
                                                datasheet_url,
                                                &new_content[ds_abs + ds_end..]
                                            );
                                            enriched += 1;
                                        }
                                    }
                                }
                            }
                            search_from = lcsc_pos + 1;
                        }
                    }
                }
            }
        }
    }

    // Write back if anything changed
    if enriched > 0 {
        write_atomic_if_unchanged(&sch_path, &content, &new_content)?;
    }

    Ok(CallToolResult::text(
        serde_json::to_string(&json!({
            "lcsc_ids_found": lcsc_ids.len(),
            "datasheets_enriched": enriched,
            "schematic": sch_path.to_str().unwrap_or("")
        }))
        .unwrap(),
    ))
}

async fn handle_get_datasheet_url(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let mpn = args["mpn"].as_str();
    let lcsc_id = args["lcsc_id"].as_str();

    if mpn.is_none() && lcsc_id.is_none() {
        return Ok(CallToolResult::error_kind(
            ToolErrorKind::InvalidArgument {
                field: "mpn".to_string(),
                reason: "one of 'mpn' or 'lcsc_id' is required".to_string(),
            },
            "Provide either 'mpn' or 'lcsc_id'",
        ));
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?;

    // Try LCSC API with lcsc_id first
    if let Some(id) = lcsc_id {
        let url = format!(
            "https://wmsc.lcsc.com/ftps/wm/product/detail?productCode={}",
            id
        );
        if let Ok(resp) = get_with_backoff(&client, &url).await {
            if resp.status().is_success() {
                if let Ok(json_resp) = resp.json::<serde_json::Value>().await {
                    if let Some(ds_url) = json_resp
                        .pointer("/result/dataManualUrl")
                        .and_then(|v| v.as_str())
                    {
                        return Ok(CallToolResult::text(
                            serde_json::to_string(&json!({
                                "lcsc_id": id,
                                "datasheet_url": ds_url
                            }))
                            .unwrap(),
                        ));
                    }
                }
            }
        }
    }

    Ok(CallToolResult::text(
        serde_json::to_string(&json!({
            "mpn": mpn,
            "lcsc_id": lcsc_id,
            "datasheet_url": null,
            "note": "Datasheet not found via LCSC API"
        }))
        .unwrap(),
    ))
}

// ─── Freerouting ──────────────────────────────────────────────────────────────

fn find_freerouting_jar(args: &serde_json::Value) -> Option<PathBuf> {
    if let Some(p) = args["jar_path"].as_str() {
        return Some(PathBuf::from(p));
    }
    // Common locations
    let candidates = [
        "freerouting.jar",
        "/usr/local/lib/freerouting/freerouting.jar",
        "/opt/freerouting/freerouting.jar",
    ];
    for c in &candidates {
        let p = PathBuf::from(c);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

async fn handle_autoroute(
    _args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    // ponytail: Freerouting workflow requires Specctra DSN export + SES import,
    // both of which were removed from kicad-cli in KiCAD 10. The tool stays in the
    // registry so callers get a clear error; remove entirely once IPC round-trip lands.
    //
    // The GUI step is not written out here — `crate::capability::manual_step_for`
    // is the only source of truth for it (D.6.3), so the message is built from
    // the same `Limitation::GuiOnlyNoApi` reason `docs/capability-matrix.md`
    // renders, and the two cannot drift apart.
    let step = crate::capability::manual_step_for("autoroute").unwrap_or(
        "kicad-cli no longer supports the Specctra DSN/SES round trip this tool needs; \
         no GUI step is on record for it",
    );
    // Catalogued rather than a `MANUAL_STEP_REQUIRED:` prefix in free text: an
    // agent loop matches on `kind`, and E9 wants a stable code beside a message
    // that may be reworded. `transient` is `none`, so a caller reading the
    // structured half already knows a retry is pointless.
    Ok(CallToolResult::error_kind(
        crate::mcp::error::ToolErrorKind::ManualStepRequired {
            tool: "autoroute".to_string(),
            step: step.to_string(),
        },
        format!("MANUAL_STEP_REQUIRED: {step}"),
    ))
}

async fn handle_check_freerouting(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let jar = find_freerouting_jar(args);

    match jar {
        None => Ok(CallToolResult::text(
            serde_json::to_string(&json!({
                "available": false,
                "note": "freerouting.jar not found. Download from https://github.com/freerouting/freerouting/releases"
            }))
            .unwrap(),
        )),
        Some(jar_path) => {
            // Try to get version from java -jar freerouting.jar --version
            let output = tokio::process::Command::new("java")
                .args(["-jar", jar_path.to_str().unwrap_or(""), "--version"])
                .output()
                .await;

            let version = match output {
                Ok(o) => {
                    let stdout = String::from_utf8_lossy(&o.stdout);
                    let stderr = String::from_utf8_lossy(&o.stderr);
                    format!("{}{}", stdout.trim(), stderr.trim())
                }
                Err(e) => format!("java not available: {e}"),
            };

            Ok(CallToolResult::text(
                serde_json::to_string(&json!({
                    "available": true,
                    "jar_path": jar_path.to_str().unwrap_or(""),
                    "version_output": version
                }))
                .unwrap(),
            ))
        }
    }
}

#[cfg(test)]
mod retry_backoff_tests {
    use super::*;

    /// End-to-end check against a real (hand-rolled) flaky HTTP server: two
    /// 503s followed by a 200 should be retried through to success, with
    /// real backoff delays elapsed in between — not just the status-code
    /// decision logic in isolation.
    #[tokio::test]
    async fn get_with_backoff_recovers_after_transient_failures() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            for resp in [
                "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\n\r\n",
                "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\n\r\n",
                "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok",
            ] {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut buf = [0u8; 1024];
                let _ = socket.read(&mut buf).await;
                socket.write_all(resp.as_bytes()).await.unwrap();
            }
        });

        let client = reqwest::Client::new();
        let url = format!("http://{}/x", addr);

        let start = std::time::Instant::now();
        let resp = get_with_backoff(&client, &url).await.unwrap();
        let elapsed = start.elapsed();

        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        // Two retries at 300ms + 600ms = 900ms minimum before the 3rd (successful) attempt.
        assert!(
            elapsed >= std::time::Duration::from_millis(900),
            "expected backoff delays to have elapsed, got {:?}",
            elapsed
        );
    }

    /// A persistent (non-transient) failure should return immediately after
    /// the first attempt — no wasted retries on a 404.
    #[tokio::test]
    async fn get_with_backoff_does_not_retry_client_errors() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = socket.read(&mut buf).await;
            socket
                .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n")
                .await
                .unwrap();
            // If get_with_backoff retried, it would try to accept() again here
            // and this task would hang until the test times out.
        });

        let client = reqwest::Client::new();
        let url = format!("http://{}/x", addr);

        let start = std::time::Instant::now();
        let resp = get_with_backoff(&client, &url).await.unwrap();
        let elapsed = start.elapsed();

        assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);
        assert!(
            elapsed < std::time::Duration::from_millis(200),
            "expected no retry delay for a 404, took {:?}",
            elapsed
        );
    }

    #[test]
    fn transient_on_rate_limit_and_server_errors() {
        assert!(is_transient_status(reqwest::StatusCode::TOO_MANY_REQUESTS));
        assert!(is_transient_status(
            reqwest::StatusCode::INTERNAL_SERVER_ERROR
        ));
        assert!(is_transient_status(reqwest::StatusCode::BAD_GATEWAY));
        assert!(is_transient_status(
            reqwest::StatusCode::SERVICE_UNAVAILABLE
        ));
        assert!(is_transient_status(reqwest::StatusCode::GATEWAY_TIMEOUT));
    }

    #[test]
    fn not_transient_on_client_errors() {
        // Retrying a 404/401/403/400 wastes time — the request itself is
        // wrong, not the server having a bad moment.
        assert!(!is_transient_status(reqwest::StatusCode::BAD_REQUEST));
        assert!(!is_transient_status(reqwest::StatusCode::UNAUTHORIZED));
        assert!(!is_transient_status(reqwest::StatusCode::FORBIDDEN));
        assert!(!is_transient_status(reqwest::StatusCode::NOT_FOUND));
    }

    #[test]
    fn not_transient_on_success() {
        assert!(!is_transient_status(reqwest::StatusCode::OK));
        assert!(!is_transient_status(reqwest::StatusCode::NO_CONTENT));
    }

    #[test]
    fn backoff_delay_doubles_each_attempt() {
        assert_eq!(backoff_delay(1), std::time::Duration::from_millis(300));
        assert_eq!(backoff_delay(2), std::time::Duration::from_millis(600));
        assert_eq!(backoff_delay(3), std::time::Duration::from_millis(1200));
    }

    #[test]
    fn backoff_delay_never_panics_on_zero_attempt() {
        // attempt is 1-based in normal use, but the saturating_sub guards
        // against an accidental 0 causing an underflow panic.
        assert_eq!(backoff_delay(0), std::time::Duration::from_millis(300));
    }
}

#[cfg(test)]
mod autoroute_tests {
    use super::*;
    use crate::router::ToolRouter;
    use crate::tools::ServerConfig;
    use std::sync::Arc;

    /// D.6.3: the `MANUAL_STEP_REQUIRED` text must be the exact reason
    /// carried by `Limitation::GuiOnlyNoApi` for `autoroute` — sourced from
    /// `crate::capability::manual_step_for`, never hand-written prose that
    /// can drift from `docs/capability-matrix.md`.
    #[tokio::test]
    async fn autoroute_names_the_manual_step_from_the_manifest() {
        let ctx = ToolContext::new(
            ServerConfig {
                kicad_cli: String::new(),
                kicad_binary: String::new(),
                ipc_address: String::new(),
                project_dir: None,
                jlcpcb_db_path: None,
                auto_load_toolsets: false,
                mode: kam_state::OperatingMode::Write,
            },
            Arc::new(ToolRouter::new()),
        );
        let result = handle_autoroute(&json!({}), &ctx).await.unwrap();
        let text = match &result.content[0] {
            crate::mcp::protocol::ToolContent::Text { text } => text.clone(),
            other => panic!("expected text content, got {other:?}"),
        };
        assert!(result.is_error);
        let step = crate::capability::manual_step_for("autoroute").unwrap();
        // Structured half: the code an agent loop matches on, and the fact
        // that no retry can help. Free text alone would make both a guess.
        let body: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(body["error"]["kind"], json!("manual_step_required"));
        assert_eq!(body["error"]["tool"], json!("autoroute"));
        assert_eq!(body["error"]["step"], json!(step));
        assert_eq!(body["error"]["transient"], json!("none"));
        assert!(body["message"].as_str().unwrap().contains(step));
    }
}

#[cfg(test)]
mod jlcpcb_cache_tests {
    use super::*;
    use crate::router::ToolRouter;
    use crate::tools::ServerConfig;
    use std::sync::Arc;

    fn test_ctx() -> ToolContext {
        ToolContext::new(
            ServerConfig {
                kicad_cli: String::new(),
                kicad_binary: String::new(),
                ipc_address: String::new(),
                project_dir: None,
                jlcpcb_db_path: None,
                auto_load_toolsets: false,
                mode: kam_state::OperatingMode::Write,
            },
            Arc::new(ToolRouter::new()),
        )
    }

    /// Builds a temp SQLite file with the schema the *published* database
    /// actually has — the FTS5 `parts` table, its quoted column names, and its
    /// text `Price` tier strings and text `Stock` — seeded with one part.
    ///
    /// It matters that this is the upstream DDL verbatim: the handlers used to
    /// query a `components` table with numeric `Price`, which no published
    /// database has ever had, and a fixture inventing that schema is what let
    /// the mismatch stand (J.2.4.4).
    pub(super) fn seed_test_db() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("jlcpcb.db");
        let conn = rusqlite::Connection::open(&db_path).expect("open db");
        create_published_schema(&conn);
        insert_part(
            &conn,
            "C14663",
            "RC0402FR-0710KL",
            "0402",
            "Basic",
            "10k resistor 0402",
            "1-199:0.01,200-:0.008",
            "5000",
        );
        (dir, db_path)
    }

    /// The published DDL, copied from a downloaded `basic-parts-fts5.db`.
    pub(super) fn create_published_schema(conn: &rusqlite::Connection) {
        conn.execute_batch(
            "CREATE VIRTUAL TABLE parts using fts5 (
                'LCSC Part', 'First Category', 'Second Category', 'MFR.Part',
                'Package', 'Solder Joint' unindexed, 'Manufacturer',
                'Library Type', 'Description', 'Datasheet' unindexed,
                'Price' unindexed, 'Stock' unindexed
            , tokenize=\"trigram\");
             CREATE TABLE meta ('filename', 'size', 'partcount', 'date', 'last_update');",
        )
        .expect("published schema");
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn insert_part(
        conn: &rusqlite::Connection,
        lcsc: &str,
        mpn: &str,
        package: &str,
        library_type: &str,
        description: &str,
        price_tiers: &str,
        stock: &str,
    ) {
        conn.execute(
            "INSERT INTO parts VALUES (?1, 'Resistors', 'Chip Resistor - Surface Mount', ?2, \
             ?3, '2', 'YAGEO', ?4, ?5, 'https://example.invalid/ds.pdf', ?6, ?7)",
            rusqlite::params![
                lcsc,
                mpn,
                package,
                library_type,
                description,
                price_tiers,
                stock
            ],
        )
        .expect("insert part");
    }

    #[tokio::test]
    async fn search_jlcpcb_parts_caches_repeated_query() {
        let (_dir, db_path) = seed_test_db();
        let ctx = test_ctx();
        let args = json!({
            "query": "10k",
            "output_path": db_path.to_str().unwrap()
        });

        let first = handle_search_jlcpcb_parts(&args, &ctx).await.unwrap();
        let second = handle_search_jlcpcb_parts(&args, &ctx).await.unwrap();

        let first_body = response_json(&first);
        let second_body = response_json(&second);
        assert_eq!(first_body["cached"], json!(false));
        assert_eq!(second_body["cached"], json!(true));
        assert_eq!(first_body["results"], second_body["results"]);
        assert_eq!(first_body["count"], json!(1));
    }

    #[tokio::test]
    async fn search_jlcpcb_parts_different_query_is_a_cache_miss() {
        let (_dir, db_path) = seed_test_db();
        let ctx = test_ctx();

        let args_a = json!({ "query": "10k", "output_path": db_path.to_str().unwrap() });
        let args_b = json!({ "query": "100nF", "output_path": db_path.to_str().unwrap() });

        handle_search_jlcpcb_parts(&args_a, &ctx).await.unwrap();
        let second = handle_search_jlcpcb_parts(&args_b, &ctx).await.unwrap();

        assert_eq!(response_json(&second)["cached"], json!(false));
    }

    #[tokio::test]
    async fn get_jlcpcb_part_caches_repeated_lookup() {
        let (_dir, db_path) = seed_test_db();
        let ctx = test_ctx();
        let args = json!({
            "lcsc_id": "C14663",
            "output_path": db_path.to_str().unwrap()
        });

        let first = handle_get_jlcpcb_part(&args, &ctx).await.unwrap();
        let second = handle_get_jlcpcb_part(&args, &ctx).await.unwrap();

        assert_eq!(response_json(&first)["cached"], json!(false));
        assert_eq!(response_json(&second)["cached"], json!(true));
        assert_eq!(response_json(&first)["lcsc"], json!("C14663"));
    }

    #[tokio::test]
    async fn suggest_alternatives_caches_repeated_query() {
        let (_dir, db_path) = seed_test_db();
        let ctx = test_ctx();
        let args = json!({
            "value": "10k",
            "footprint": "Resistor_SMD:R_0402",
            "output_path": db_path.to_str().unwrap()
        });

        let first = handle_suggest_alternatives(&args, &ctx).await.unwrap();
        let second = handle_suggest_alternatives(&args, &ctx).await.unwrap();

        assert_eq!(response_json(&first)["cached"], json!(false));
        assert_eq!(response_json(&second)["cached"], json!(true));
    }

    pub(super) fn response_json(result: &CallToolResult) -> serde_json::Value {
        match &result.content[0] {
            crate::mcp::protocol::ToolContent::Text { text } => serde_json::from_str(text).unwrap(),
            _ => panic!("expected text content"),
        }
    }
}

/// The published database, and reading it: `download_jlcpcb_database` fetching
/// the chunked archive (J.2.4.3), and the query tools speaking the schema that
/// archive contains (J.2.4.4).
///
/// The archive is served from a throwaway loopback server rather than from
/// GitHub Pages, so the whole path — manifest, 1-based chunk numbering,
/// concatenation, inflation, validation, atomic rename — is proved without a
/// third party. The gated probe in
/// `crates/konnect-core/tests/sourcing_and_manufacturing.rs` is what checks the
/// real host still publishes what this expects.
#[cfg(test)]
mod jlcpcb_published_database_tests {
    use super::jlcpcb_cache_tests::{create_published_schema, insert_part, response_json};
    use super::*;
    use crate::router::ToolRouter;
    use crate::tools::ServerConfig;
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn test_ctx() -> ToolContext {
        ToolContext::new(
            ServerConfig {
                kicad_cli: String::new(),
                kicad_binary: String::new(),
                ipc_address: String::new(),
                project_dir: None,
                jlcpcb_db_path: None,
                auto_load_toolsets: false,
                mode: kam_state::OperatingMode::Write,
            },
            Arc::new(ToolRouter::new()),
        )
    }

    /// Answers `GET /<name>` from `routes` and 404s everything else. Returns the
    /// base URL to hand to `base_url`.
    async fn serve(routes: Vec<(String, Vec<u8>)>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let routes = routes.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let read = socket.read(&mut buf).await.unwrap_or(0);
                    let request = String::from_utf8_lossy(&buf[..read]).to_string();
                    let path = request.split_whitespace().nth(1).unwrap_or("/").to_string();
                    let body = routes
                        .iter()
                        .find(|(name, _)| path == format!("/{name}"))
                        .map(|(_, body)| body.clone());
                    let response = match body {
                        Some(body) => {
                            let mut out = format!(
                                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
                                body.len()
                            )
                            .into_bytes();
                            out.extend_from_slice(&body);
                            out
                        }
                        None => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_vec(),
                    };
                    let _ = socket.write_all(&response).await;
                    let _ = socket.flush().await;
                });
            }
        });
        format!("http://{addr}")
    }

    /// A real SQLite database with the published schema and two parts, wrapped
    /// in a single-entry deflate archive the way upstream publishes it.
    fn published_archive(dir: &std::path::Path) -> Vec<u8> {
        let source = dir.join("published-source.db");
        let conn = rusqlite::Connection::open(&source).unwrap();
        create_published_schema(&conn);
        insert_part(
            &conn,
            "C14663",
            "RC0402FR-0710KL",
            "0402",
            "Basic",
            "10k resistor 0402",
            "1-199:0.01,200-:0.008",
            "5000",
        );
        insert_part(
            &conn,
            "C25744",
            "RC0402FR-071KL",
            "0402",
            "Extended",
            "1k resistor 0402",
            "1-199:0.004,200-:0.003",
            "0",
        );
        drop(conn);

        let bytes = std::fs::read(&source).unwrap();
        let mut zipped = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        zipped
            .start_file(
                "basic-parts-fts5.db",
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated),
            )
            .unwrap();
        std::io::Write::write_all(&mut zipped, &bytes).unwrap();
        zipped.finish().unwrap().into_inner()
    }

    /// The upstream artifact names for the default library, split into `chunks`
    /// pieces — the manifest first, then the chunks numbered from 1.
    fn routes(archive: &[u8], chunks: usize) -> Vec<(String, Vec<u8>)> {
        let size = archive.len().div_ceil(chunks);
        let mut routes = vec![(
            "chunk_num_basic_parts_fts5.txt".to_string(),
            chunks.to_string().into_bytes(),
        )];
        for (index, piece) in archive.chunks(size).enumerate() {
            routes.push((
                format!("basic-parts-fts5.db.zip.{:03}", index + 1),
                piece.to_vec(),
            ));
        }
        routes
    }

    fn temporaries_gone(db_path: &std::path::Path) {
        for suffix in ["download", "inflating"] {
            let leftover = sibling(db_path, suffix);
            assert!(
                !leftover.exists(),
                "a temporary was left behind at {}",
                leftover.display()
            );
        }
    }

    /// The whole download: two chunks, concatenated, inflated, validated, and
    /// then actually queried. Nothing here names a library — the default is
    /// what a caller who just wants "the database" gets.
    #[tokio::test]
    async fn a_chunked_download_lands_a_database_the_query_tools_can_read() {
        let dir = tempfile::tempdir().unwrap();
        let archive = published_archive(dir.path());
        let base_url = serve(routes(&archive, 2)).await;
        let db_path = dir.path().join("jlcpcb.db");
        let ctx = test_ctx();

        let result = handle_download_jlcpcb(
            &json!({ "output_path": db_path.to_str().unwrap(), "base_url": base_url }),
            &ctx,
        )
        .await
        .unwrap();
        let body = response_json(&result);

        assert_eq!(body["success"], json!(true), "download failed: {body}");
        assert_eq!(
            body["chunks"],
            json!(2),
            "both chunks should be used: {body}"
        );
        assert_eq!(body["library"], json!(DEFAULT_JLCPCB_LIBRARY));
        assert_eq!(body["part_count"], json!(2), "{body}");
        assert!(db_path.exists(), "no database at {}", db_path.display());
        temporaries_gone(&db_path);

        let found = response_json(
            &handle_search_jlcpcb_parts(
                &json!({ "query": "10k", "output_path": db_path.to_str().unwrap() }),
                &ctx,
            )
            .await
            .unwrap(),
        );
        assert_eq!(
            found["count"],
            json!(1),
            "the download is not queryable: {found}"
        );
        assert_eq!(found["results"][0]["lcsc"], json!("C14663"));
    }

    /// What the download recorded is readable afterwards, and a search that
    /// finds nothing says which library it searched — the difference between
    /// "not stocked" and "wrong library" is invisible otherwise.
    #[tokio::test]
    async fn stats_and_an_empty_result_name_the_library_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let archive = published_archive(dir.path());
        let base_url = serve(routes(&archive, 1)).await;
        let db_path = dir.path().join("jlcpcb.db");
        let ctx = test_ctx();

        handle_download_jlcpcb(
            &json!({ "output_path": db_path.to_str().unwrap(), "base_url": base_url }),
            &ctx,
        )
        .await
        .unwrap();

        let stats = response_json(
            &handle_jlcpcb_stats(&json!({ "output_path": db_path.to_str().unwrap() }), &ctx)
                .await
                .unwrap(),
        );
        assert_eq!(stats["exists"], json!(true), "{stats}");
        assert_eq!(stats["library"], json!(DEFAULT_JLCPCB_LIBRARY), "{stats}");
        assert_eq!(stats["part_count"], json!(2), "{stats}");

        let nothing = response_json(
            &handle_search_jlcpcb_parts(
                &json!({ "query": "STM32H747", "output_path": db_path.to_str().unwrap() }),
                &ctx,
            )
            .await
            .unwrap(),
        );
        assert_eq!(nothing["count"], json!(0));
        let note = nothing["note"].as_str().unwrap_or_default();
        assert!(
            note.contains(DEFAULT_JLCPCB_LIBRARY) && note.contains("all-parts"),
            "an empty result should say which library was searched: {nothing}"
        );
    }

    /// A chunk set the manifest promises but the host does not have is a
    /// truncated archive. The failure has to be reported *and* leave nothing
    /// behind: a half-written file at the destination is worse than no file,
    /// because every query tool would then read it as the database.
    #[tokio::test]
    async fn a_missing_chunk_leaves_no_database_behind() {
        let dir = tempfile::tempdir().unwrap();
        let archive = published_archive(dir.path());
        let mut incomplete = routes(&archive, 2);
        incomplete.pop(); // the manifest still says 2
        let base_url = serve(incomplete).await;
        let db_path = dir.path().join("jlcpcb.db");

        let result = handle_download_jlcpcb(
            &json!({ "output_path": db_path.to_str().unwrap(), "base_url": base_url }),
            &test_ctx(),
        )
        .await
        .unwrap();

        assert!(result.is_error, "the failure was not reported");
        let text = format!("{:?}", result.content);
        assert!(
            text.contains("Chunk 2/2") && text.contains("404"),
            "the error should name the chunk that failed: {text}"
        );
        assert!(
            !db_path.exists(),
            "a failed download left a database behind"
        );
        temporaries_gone(&db_path);
    }

    /// GitHub Pages answers 200 with an HTML error page for some missing paths,
    /// which is how the old URL used to be "fetched" successfully. Content that
    /// is not the database must not be renamed into place.
    #[tokio::test]
    async fn a_two_hundred_serving_something_else_is_not_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let base_url = serve(vec![
            ("chunk_num_basic_parts_fts5.txt".to_string(), b"1".to_vec()),
            (
                "basic-parts-fts5.db.zip.001".to_string(),
                b"<html><body>404</body></html>".to_vec(),
            ),
        ])
        .await;
        let db_path = dir.path().join("jlcpcb.db");

        let result = handle_download_jlcpcb(
            &json!({ "output_path": db_path.to_str().unwrap(), "base_url": base_url }),
            &test_ctx(),
        )
        .await
        .unwrap();

        assert!(result.is_error);
        assert!(
            !db_path.exists(),
            "an HTML page was accepted as the database"
        );
        temporaries_gone(&db_path);
    }

    /// A database whose `parts` table is missing is not the parts database,
    /// however well-formed the archive is.
    #[tokio::test]
    async fn an_archive_without_the_parts_table_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let wrong = dir.path().join("wrong.db");
        let conn = rusqlite::Connection::open(&wrong).unwrap();
        conn.execute("CREATE TABLE something_else (a)", []).unwrap();
        drop(conn);
        let mut zipped = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        zipped
            .start_file(
                "basic-parts-fts5.db",
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated),
            )
            .unwrap();
        std::io::Write::write_all(&mut zipped, &std::fs::read(&wrong).unwrap()).unwrap();
        let archive = zipped.finish().unwrap().into_inner();

        let base_url = serve(routes(&archive, 1)).await;
        let db_path = dir.path().join("jlcpcb.db");
        let result = handle_download_jlcpcb(
            &json!({ "output_path": db_path.to_str().unwrap(), "base_url": base_url }),
            &test_ctx(),
        )
        .await
        .unwrap();

        assert!(result.is_error);
        let text = format!("{:?}", result.content);
        assert!(
            text.contains("parts"),
            "the error should say what is missing: {text}"
        );
        assert!(!db_path.exists());
        temporaries_gone(&db_path);
    }

    /// An unknown library name is a caller mistake, and the answer is the list
    /// of names that work — without touching the network first.
    #[tokio::test]
    async fn an_unknown_library_names_the_ones_that_exist() {
        let dir = tempfile::tempdir().unwrap();
        let result = handle_download_jlcpcb(
            &json!({
                "output_path": dir.path().join("jlcpcb.db").to_str().unwrap(),
                "library": "everything"
            }),
            &test_ctx(),
        )
        .await
        .unwrap();

        assert!(result.is_error);
        let text = format!("{:?}", result.content);
        for name in JLCPCB_LIBRARIES.iter().map(|v| v.name) {
            assert!(text.contains(name), "'{name}' is missing from: {text}");
        }
    }

    // ─── The schema the published database actually has (J.2.4.4) ────────────

    fn seeded(dir: &std::path::Path) -> std::path::PathBuf {
        let db_path = dir.join("jlcpcb.db");
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        create_published_schema(&conn);
        insert_part(
            &conn,
            "C14663",
            "RC0402FR-0710KL",
            "0402",
            "Basic",
            "10k resistor 0402",
            "1-199:0.0123,200-:0.008",
            "5000",
        );
        insert_part(
            &conn,
            "C25744",
            "RC0402FR-0710KX",
            "0402",
            "Extended",
            "10k resistor 0402 thin film",
            "1-99:0.0041,100-:0.003",
            "12000",
        );
        insert_part(
            &conn,
            "C99999",
            "RC0402FR-0710KZ",
            "0402",
            "Basic",
            "10k resistor 0402 out of stock",
            "1-99:0.001",
            "0",
        );
        db_path
    }

    /// `Price` is a tier string and `Stock` is text in the published schema, so
    /// a caller reading `price` or comparing `stock` gets a parsed number, and
    /// the raw tiers stay available for a quantity decision.
    #[tokio::test]
    async fn a_result_row_carries_the_parsed_unit_price_and_stock() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = seeded(dir.path());
        let found = response_json(
            &handle_search_jlcpcb_parts(
                &json!({ "query": "RC0402FR-0710KL", "output_path": db_path.to_str().unwrap() }),
                &test_ctx(),
            )
            .await
            .unwrap(),
        );

        let part = &found["results"][0];
        assert_eq!(part["price"], json!(0.0123), "unit price at qty 1: {part}");
        assert_eq!(part["price_tiers"], json!("1-199:0.0123,200-:0.008"));
        assert_eq!(part["stock"], json!(5000), "{part}");
        assert_eq!(part["library_type"], json!("Basic"));
        assert_eq!(
            part["category"],
            json!("Resistors / Chip Resistor - Surface Mount")
        );
        assert!(part["datasheet"]
            .as_str()
            .is_some_and(|d| d.contains("http")));
    }

    /// The two filters that cut the result set have to work against the real
    /// column names and the real text `Stock`.
    #[tokio::test]
    async fn basic_only_and_in_stock_filter_on_the_published_columns() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = seeded(dir.path());
        let ctx = test_ctx();
        let path = db_path.to_str().unwrap().to_string();

        let all = response_json(
            &handle_search_jlcpcb_parts(
                &json!({ "query": "10k resistor", "output_path": path, "in_stock": false }),
                &ctx,
            )
            .await
            .unwrap(),
        );
        assert_eq!(all["count"], json!(3), "{all}");

        let stocked = response_json(
            &handle_search_jlcpcb_parts(
                &json!({ "query": "10k resistor", "output_path": path, "in_stock": true }),
                &ctx,
            )
            .await
            .unwrap(),
        );
        assert_eq!(
            stocked["count"],
            json!(2),
            "a text '0' is out of stock: {stocked}"
        );

        let basic = response_json(
            &handle_search_jlcpcb_parts(
                &json!({
                    "query": "10k resistor", "output_path": path,
                    "in_stock": false, "basic_only": true
                }),
                &ctx,
            )
            .await
            .unwrap(),
        );
        assert_eq!(
            basic["count"],
            json!(2),
            "Extended should be excluded: {basic}"
        );

        let categorised = response_json(
            &handle_search_jlcpcb_parts(
                &json!({
                    "query": "10k resistor", "output_path": path,
                    "in_stock": false, "category": "Chip Resistor"
                }),
                &ctx,
            )
            .await
            .unwrap(),
        );
        assert_eq!(
            categorised["count"],
            json!(3),
            "the category lives in two columns, not one called 'Category': {categorised}"
        );
    }

    /// Cheapest-first and `max_price_usd` are decided on the parsed unit price;
    /// `ORDER BY Price` on a tier string would sort "1-199:0.02" before
    /// "1-99:0.9" by text.
    #[tokio::test]
    async fn alternatives_are_ordered_by_unit_price_and_respect_the_cap() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = seeded(dir.path());
        let ctx = test_ctx();
        let path = db_path.to_str().unwrap().to_string();

        let cheapest = response_json(
            &handle_suggest_alternatives(
                &json!({ "value": "10k", "footprint": "Resistor_SMD:R_0402", "output_path": path }),
                &ctx,
            )
            .await
            .unwrap(),
        );
        let alternatives = cheapest["alternatives"].as_array().unwrap();
        assert_eq!(
            alternatives.len(),
            2,
            "out-of-stock parts are not alternatives: {cheapest}"
        );
        assert_eq!(
            alternatives[0]["lcsc"],
            json!("C25744"),
            "the 0.0041 part should come first: {cheapest}"
        );

        let capped = response_json(
            &handle_suggest_alternatives(
                &json!({
                    "value": "10k", "footprint": "Resistor_SMD:R_0402",
                    "output_path": path, "max_price_usd": 0.005
                }),
                &ctx,
            )
            .await
            .unwrap(),
        );
        let capped = capped["alternatives"].as_array().unwrap();
        assert_eq!(capped.len(), 1, "0.0123 is over the cap: {capped:?}");
        assert_eq!(capped[0]["lcsc"], json!("C25744"));
    }

    #[test]
    fn a_price_tier_string_yields_the_quantity_one_price() {
        assert_eq!(
            unit_price_usd("1-199:0.018,200-599:0.015,600-:0.013"),
            Some(0.018)
        );
        assert_eq!(unit_price_usd("1-:1.5"), Some(1.5));
        // No tiers at all, or a tier without a price, is not a price of zero.
        assert_eq!(unit_price_usd(""), None);
        assert_eq!(unit_price_usd("1-199"), None);
    }

    #[test]
    fn every_published_library_has_a_distinct_artifact_pair() {
        for variant in JLCPCB_LIBRARIES.iter() {
            assert!(variant.db_file_name.ends_with(".db"));
            assert!(variant.chunk_manifest.starts_with("chunk_num_"));
            assert_eq!(
                JLCPCB_LIBRARIES
                    .iter()
                    .filter(|other| other.db_file_name == variant.db_file_name)
                    .count(),
                1,
                "'{}' is not a distinct artifact",
                variant.name
            );
        }
        assert!(jlcpcb_library(DEFAULT_JLCPCB_LIBRARY).is_some());
        assert!(jlcpcb_library("nope").is_none());
    }
}

/// The three JLCPCB query tools tested the database's existence before reading
/// their own arguments, so a caller who forgot a `required` argument was sent
/// to download a 2.5-million-part catalogue for a call that could never run.
#[cfg(test)]
mod jlcpcb_required_argument_tests {
    use super::jlcpcb_cache_tests::response_json;
    use super::*;
    use crate::mcp::error::extract_error_kind;
    use crate::router::ToolRouter;
    use crate::tools::ServerConfig;
    use std::sync::Arc;

    fn test_ctx() -> ToolContext {
        ToolContext::new(
            ServerConfig {
                kicad_cli: String::new(),
                kicad_binary: String::new(),
                ipc_address: String::new(),
                project_dir: None,
                jlcpcb_db_path: None,
                auto_load_toolsets: false,
                mode: kam_state::OperatingMode::Write,
            },
            Arc::new(ToolRouter::new()),
        )
    }

    /// A path guaranteed to hold no database, so the answer proves the order:
    /// argument first, database second.
    fn absent_db(dir: &tempfile::TempDir) -> String {
        dir.path().join("no-such.db").to_string_lossy().into_owned()
    }

    #[tokio::test]
    async fn suggest_alternatives_refuses_its_missing_arguments_before_the_database_is_looked_for()
    {
        let dir = tempfile::tempdir().unwrap();
        let res =
            handle_suggest_alternatives(&json!({ "output_path": absent_db(&dir) }), &test_ctx())
                .await
                .unwrap();
        assert!(res.is_error);
        assert_eq!(
            extract_error_kind(&res).as_deref(),
            Some("invalid_argument"),
            "the caller's own mistake must be named before the database's absence"
        );
    }

    #[tokio::test]
    async fn get_jlcpcb_part_refuses_a_missing_lcsc_id_before_the_database_is_looked_for() {
        let dir = tempfile::tempdir().unwrap();
        let res = handle_get_jlcpcb_part(&json!({ "output_path": absent_db(&dir) }), &test_ctx())
            .await
            .unwrap();
        assert!(res.is_error);
        assert_eq!(
            extract_error_kind(&res).as_deref(),
            Some("invalid_argument"),
            "the caller's own mistake must be named before the database's absence"
        );
    }

    #[tokio::test]
    async fn search_jlcpcb_parts_refuses_a_missing_query_before_the_database_is_looked_for() {
        let dir = tempfile::tempdir().unwrap();
        let res =
            handle_search_jlcpcb_parts(&json!({ "output_path": absent_db(&dir) }), &test_ctx())
                .await
                .unwrap();
        assert!(res.is_error);
        assert_eq!(
            extract_error_kind(&res).as_deref(),
            Some("invalid_argument"),
            "the caller's own mistake must be named before the database's absence"
        );
    }

    /// `suggest_jlcpcb_alternatives` caches. A refused call must not leave the
    /// answer it never computed under the key its substituted arguments would
    /// have built — an explicitly empty `value` afterwards is still a miss.
    #[tokio::test]
    async fn a_refused_suggestion_puts_nothing_in_the_cache() {
        let (_dir, db_path) = super::jlcpcb_cache_tests::seed_test_db();
        let ctx = test_ctx();
        let path = db_path.to_str().unwrap().to_string();

        let refused = handle_suggest_alternatives(
            &json!({ "footprint": "Resistor_SMD:R_0402", "output_path": path.clone() }),
            &ctx,
        )
        .await
        .unwrap();
        assert!(refused.is_error);

        let explicit = handle_suggest_alternatives(
            &json!({ "value": "", "footprint": "Resistor_SMD:R_0402", "output_path": path }),
            &ctx,
        )
        .await
        .unwrap();
        assert_eq!(
            response_json(&explicit)["cached"],
            json!(false),
            "the refused call cached an answer under the substituted key"
        );
    }
}
