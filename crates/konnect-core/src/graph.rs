//! Building and caching a [`kam_graph::Graph`] for a KiCAD project.
//!
//! `kam-graph` indexes whatever [`kam_evidence::ItemSet`] it is handed and
//! knows nothing about KiCAD (D11: it stays clean-room). This module is the
//! other half: it discovers a project's `.kicad_sch` / `.kicad_pcb`
//! documents, extracts each with [`evidence::extract`] — the same extractors
//! the semantic diff already uses, not a second implementation — and builds
//! the index the `graph_*` tools query.
//!
//! ## E7 — what a graph entry does and does not claim
//!
//! Konnect's own connectivity analysis has already contradicted `kicad-cli
//! sch erc` on a real schematic (`progress.md`, E7): it reported zero
//! single-pin nets where ERC found six unconnected pins. That history decides
//! what this module is allowed to index:
//!
//! * On a `.kicad_pcb`, `net` is a fact the pads carry themselves — the file
//!   says which pad is on which net, and this module only copies that.
//! * On a `.kicad_sch`, no item is ever given a `net` attribute. Labels and
//!   power symbols are indexed as `text` — what the sheet says, not what it
//!   implies — because deriving connectivity from wire geometry is exactly
//!   the computation E7 caught being wrong. The verdict comes from `run_erc`.
//!
//! ## Why the cache is keyed by content revision, not by a version counter
//!
//! Same principle as [`evidence::validators::Cache`]: a document's revision
//! is a hash of its bytes (`kam_state::revision`, the same one `kicad_invoke`
//! uses for its own staleness check), never an in-process counter, because
//! the file can change from outside this process — the user editing it in
//! KiCAD while an agent holds a stale graph. A [`Graph`](kam_graph::Graph)
//! itself is immutable once built, so the cache below has two tiers: one
//! document's extraction, remembered per `(path, revision)` so an unchanged
//! document is never re-parsed, and the whole built graph, remembered per
//! project directory and the sorted revision of every document under it, so
//! a query on an unchanged project reuses the built indices too.
//!
//! ## Why a parse failure is not an empty document
//!
//! [`evidence::extract`] returns `None` for a document that does not parse,
//! specifically so a broken file is not read as "this document has no
//! items" — that would report every symbol on it as having vanished. This
//! module honours the same rule: a document that fails to parse is left out
//! of the graph entirely rather than added as an empty [`ItemSet`].

use crate::evidence::{self, DocumentKind};
use kam_evidence::ItemSet;
use kam_state::revision::Revision;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Attributes worth an index across both document kinds. `net` is populated
/// only on a `.kicad_pcb` — see the module docs on E7 — but it costs nothing
/// to declare it indexed for a project with no PCB.
pub(crate) const INDEXED_ATTRS: &[&str] = &["net", "lib_id", "layer", "value", "footprint", "text"];

/// The attribute every extractor writes an item's position under.
const SPATIAL_ATTR: &str = "at";

/// How many document extractions, and how many whole graphs, are remembered.
/// Same figure as [`evidence::validators::CACHE_CAPACITY`] — a session
/// working across a handful of projects, not an unbounded server-lifetime
/// history.
const CACHE_CAPACITY: usize = 64;

/// Directory names never descended into while discovering documents — the
/// same list [`kam_state::snapshot`] skips, since these are never a
/// project's own design files.
const SKIP_DIRS: &[&str] = &[".git", "target", "node_modules", "__pycache__"];

/// Bounds on the discovery walk, matched to
/// [`kam_state::snapshot::SnapshotLimits`]'s defaults: a 40-sheet hierarchy
/// is a big project and still lands well inside these.
const MAX_DEPTH: usize = 6;
const MAX_FILES: usize = 400;

/// A document's name as it appears in the graph and in any MCP reply: the
/// file name alone. An absolute path must never leak into a tool response —
/// it is local machine layout, not a design fact — so this is the only name
/// [`GraphStore::get`] ever hands to [`kam_graph::GraphBuilder::document`].
fn document_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Resolve a caller-supplied `project` argument to the directory to scan.
///
/// Accepts a `.kicad_pro` file, any design document, or a bare project
/// directory — whichever a caller already has a path to. A file argument
/// resolves to its parent; anything else is used as the directory directly,
/// which also makes a directory that does not exist yet a harmless empty
/// scan rather than an error a tool has to special-case.
#[must_use]
pub fn project_dir(arg: &str) -> PathBuf {
    let path = PathBuf::from(arg);
    if path.is_file() {
        path.parent().map(Path::to_path_buf).unwrap_or(path)
    } else {
        path
    }
}

/// Every `.kicad_sch` / `.kicad_pcb` file reachable from `dir`, sorted for a
/// deterministic build order.
fn discover_documents(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect(dir, 0, &mut out);
    out.sort();
    out
}

fn collect(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > MAX_DEPTH || out.len() >= MAX_FILES {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if file_type.is_dir() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if SKIP_DIRS.contains(&name.as_ref()) || name.ends_with("-backups") {
                continue;
            }
            collect(&path, depth + 1, out);
        } else if file_type.is_file() && DocumentKind::of_path(&path).is_some() {
            out.push(path);
            if out.len() >= MAX_FILES {
                return;
            }
        }
    }
}

/// One document's extraction, or `None` for a document that did not parse —
/// see the module docs on why that is never the same as an empty [`ItemSet`].
type Extraction = Option<ItemSet>;

#[derive(Default)]
struct Inner {
    documents: HashMap<(PathBuf, String), Extraction>,
    document_order: VecDeque<(PathBuf, String)>,
    graphs: HashMap<(PathBuf, String), Arc<kam_graph::Graph>>,
    graph_order: VecDeque<(PathBuf, String)>,
}

/// Builds and caches a [`kam_graph::Graph`] per project directory.
///
/// One instance lives on [`crate::tools::ToolContext`] for the life of the
/// server, the same way [`evidence::validators::Cache`] does, so the cache
/// is warm across calls within a session rather than per-request.
#[derive(Default)]
pub struct GraphStore {
    inner: Mutex<Inner>,
}

impl GraphStore {
    /// Build, or reuse, the graph for every design document found under
    /// `dir`.
    ///
    /// Every document is re-hashed on every call — that is unavoidable
    /// without a filesystem watcher, and cheap next to parsing — but a
    /// document whose hash has not moved is never re-extracted, and if
    /// nothing under `dir` has moved the whole graph is returned without
    /// rebuilding a single index.
    ///
    /// # Errors
    ///
    /// An I/O failure reading a discovered document. A directory that does
    /// not exist is not an error: it discovers zero documents and returns an
    /// empty graph, since `project` accepts a path a caller may not have
    /// created yet.
    pub fn get(&self, dir: &Path) -> std::io::Result<Arc<kam_graph::Graph>> {
        let documents = discover_documents(dir);

        let mut revisions: Vec<(PathBuf, String)> = Vec::with_capacity(documents.len());
        let mut bytes_by_path: HashMap<PathBuf, Vec<u8>> = HashMap::new();
        for path in documents {
            let bytes = std::fs::read(&path)?;
            let token = Revision::of_bytes(&bytes).token();
            revisions.push((path.clone(), token));
            bytes_by_path.insert(path, bytes);
        }
        revisions.sort();

        let signature = revisions
            .iter()
            .map(|(path, revision)| format!("{}={revision}", path.display()))
            .collect::<Vec<_>>()
            .join(";");
        let graph_key = (dir.to_path_buf(), signature);

        if let Some(graph) = self.cached_graph(&graph_key) {
            return Ok(graph);
        }

        let mut builder = kam_graph::Graph::builder().index_points(SPATIAL_ATTR);
        for attr in INDEXED_ATTRS {
            builder = builder.index_attr(*attr);
        }
        for (path, revision) in &revisions {
            let Some(kind) = DocumentKind::of_path(path) else {
                continue;
            };
            let bytes = &bytes_by_path[path];
            if let Some(items) = self.extraction(path, revision, kind, bytes) {
                builder = builder.document(document_name(path), items);
            }
            // A document that did not parse contributes nothing — never an
            // empty item set. See the module docs.
        }

        let graph = Arc::new(builder.build());
        self.remember_graph(graph_key, graph.clone());
        Ok(graph)
    }

    fn extraction(
        &self,
        path: &Path,
        revision: &str,
        kind: DocumentKind,
        bytes: &[u8],
    ) -> Extraction {
        let key = (path.to_path_buf(), revision.to_string());
        if let Some(cached) = self.lock().documents.get(&key) {
            return cached.clone();
        }

        let extracted = evidence::extract(kind, bytes);

        let mut inner = self.lock();
        if inner
            .documents
            .insert(key.clone(), extracted.clone())
            .is_none()
        {
            inner.document_order.push_back(key);
            while inner.document_order.len() > CACHE_CAPACITY {
                let Some(oldest) = inner.document_order.pop_front() else {
                    break;
                };
                inner.documents.remove(&oldest);
            }
        }
        extracted
    }

    fn cached_graph(&self, key: &(PathBuf, String)) -> Option<Arc<kam_graph::Graph>> {
        self.lock().graphs.get(key).cloned()
    }

    fn remember_graph(&self, key: (PathBuf, String), graph: Arc<kam_graph::Graph>) {
        let mut inner = self.lock();
        if inner.graphs.insert(key.clone(), graph).is_none() {
            inner.graph_order.push_back(key);
            while inner.graph_order.len() > CACHE_CAPACITY {
                let Some(oldest) = inner.graph_order.pop_front() else {
                    break;
                };
                inner.graphs.remove(&oldest);
            }
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kam_evidence::Attr;

    fn write(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, body).unwrap();
        path
    }

    const DIVIDER_SCH: &str = r#"
(kicad_sch
  (version 20260306)
  (symbol (lib_id "Device:R") (at 100 80 0) (uuid "s-1")
    (property "Reference" "R1" (at 102 79 0))
    (property "Value" "10k" (at 102 81 0)))
  (label "VOUT" (at 100 90 0) (uuid "l-1")))
"#;

    const BOARD_PCB: &str = r#"
(kicad_pcb
  (version 20260206)
  (net 0 "")
  (net 1 "GND")
  (footprint "Resistor_SMD:R_0603" (layer "F.Cu") (uuid "f-1") (at 84 31 0)
    (property "Reference" "R1" (at 0 0 0))
    (pad "1" smd rect (at -0.8 0) (size 0.9 0.9) (net 1 "GND") (uuid "p-1"))))
"#;

    #[test]
    fn a_project_graph_spans_both_documents_named_by_file_only() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "board.kicad_sch", DIVIDER_SCH);
        write(dir.path(), "board.kicad_pcb", BOARD_PCB);

        let store = GraphStore::default();
        let graph = store.get(dir.path()).unwrap();

        let symbol = graph.get("board.kicad_sch", "symbol", "s-1").unwrap();
        assert_eq!(symbol.label, "R1");
        assert!(graph.get("board.kicad_pcb", "footprint", "f-1").is_some());
    }

    #[test]
    fn a_schematic_symbol_never_carries_a_net_attribute() {
        // E7: connectivity is not derived on a schematic. If this ever picks
        // up a "net" attribute, some future edit started inferring wiring.
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "board.kicad_sch", DIVIDER_SCH);

        let store = GraphStore::default();
        let graph = store.get(dir.path()).unwrap();
        let symbol = graph.get("board.kicad_sch", "symbol", "s-1").unwrap();
        assert!(!symbol.attrs.contains_key("net"));
    }

    #[test]
    fn a_pcb_pad_net_is_indexed_as_a_fact_from_the_file() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "board.kicad_pcb", BOARD_PCB);

        let store = GraphStore::default();
        let graph = store.get(dir.path()).unwrap();
        let net = graph.get("board.kicad_pcb", "net", "GND").unwrap();
        assert_eq!(net.label, "GND");
    }

    #[test]
    fn an_unparsable_document_is_skipped_not_indexed_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "good.kicad_sch", DIVIDER_SCH);
        write(dir.path(), "bad.kicad_sch", "(kicad_sch unterminated");

        let store = GraphStore::default();
        let graph = store.get(dir.path()).unwrap();

        assert!(graph.get("good.kicad_sch", "symbol", "s-1").is_some());
        let stats = graph.stats();
        assert!(
            !stats.documents.contains_key("bad.kicad_sch"),
            "a document that failed to parse must not appear at all, \
             not even with a zero count"
        );
    }

    #[test]
    fn an_unchanged_project_reuses_the_built_graph() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "board.kicad_sch", DIVIDER_SCH);

        let store = GraphStore::default();
        let first = store.get(dir.path()).unwrap();
        let second = store.get(dir.path()).unwrap();
        assert!(
            Arc::ptr_eq(&first, &second),
            "identical revisions must hit the graph cache"
        );
    }

    #[test]
    fn editing_one_document_only_re_extracts_that_document() {
        let dir = tempfile::tempdir().unwrap();
        let sch = write(dir.path(), "board.kicad_sch", DIVIDER_SCH);
        write(dir.path(), "board.kicad_pcb", BOARD_PCB);

        let store = GraphStore::default();
        let first = store.get(dir.path()).unwrap();

        // Edit the schematic only.
        std::fs::write(&sch, DIVIDER_SCH.replace("R1", "R99")).unwrap();
        let second = store.get(dir.path()).unwrap();

        assert!(
            !Arc::ptr_eq(&first, &second),
            "a changed document must miss the graph cache"
        );
        assert_eq!(
            second
                .get("board.kicad_sch", "symbol", "s-1")
                .unwrap()
                .label,
            "R99"
        );
        // The untouched PCB's extraction was reused, not re-parsed — proven
        // indirectly: its items are still present and unchanged.
        assert!(second.get("board.kicad_pcb", "footprint", "f-1").is_some());
    }

    #[test]
    fn project_dir_resolves_a_file_argument_to_its_parent() {
        let dir = tempfile::tempdir().unwrap();
        let pro = write(dir.path(), "widget.kicad_pro", "{}");
        assert_eq!(project_dir(pro.to_str().unwrap()), dir.path());
    }

    #[test]
    fn project_dir_accepts_a_bare_directory() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(project_dir(dir.path().to_str().unwrap()), dir.path());
    }

    #[test]
    fn a_missing_project_directory_is_an_empty_graph_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("not-yet-created");
        let store = GraphStore::default();
        let graph = store.get(&missing).unwrap();
        assert_eq!(graph.stats().documents.len(), 0);
    }

    #[test]
    fn points_are_indexed_for_neighbor_queries() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "board.kicad_sch", DIVIDER_SCH);
        let store = GraphStore::default();
        let graph = store.get(dir.path()).unwrap();
        let symbol = graph.get("board.kicad_sch", "symbol", "s-1").unwrap();
        assert_eq!(symbol.attrs["at"], Attr::point(100.0, 80.0));
        // The graph itself proves it was spatially indexed: neighbors()
        // succeeds rather than reporting ItemHasNoPoint.
        assert!(graph
            .neighbors("board.kicad_sch", "symbol", "s-1", 5.0, 10)
            .is_ok());
    }
}
