//! Diagnostic-only probe for `router::capability_search::search` and for
//! scoring-rule variants that are simulated *only* here, never wired into
//! production.
//!
//! Reads the golden tasks' plain-language intents (dumped by
//! `bench/retrieval_intents.py`) and:
//!   1. runs each intent through the real `search()` used by
//!      `find_capabilities`, printing ranked hits and a per-task needed-tool
//!      summary;
//!   2. reproduces the runner's `--load-mode search` methodology (union of
//!      top-`limit` hits per task, precision = hits/|union|, recall =
//!      hits/|needed|) on two perimeters: the 6 tasks that existed when
//!      `docs/benchmark.md`'s 22.4 %/100 % line was measured (`01`..`06`,
//!      before `07_sch_inspection` was added in 16b9119) and all 7;
//!   3. sweeps 3 scoring variants x 6 relative-cutoff thresholds x 2 limits
//!      (36 combinations), each scored the same way, to see whether a cheap
//!      change to ranking (never to `capability_search` itself) buys back
//!      precision without losing recall.
//!
//! The scoring variants (`idf`, `idf_norm`) are reimplemented locally against
//! the corpus using the same field weights and synonym table as
//! `capability_search::score_tool` (necessarily duplicated here since that
//! function is private) — never patched into the module under test. A sanity
//! check at startup verifies the local `current` reimplementation reproduces
//! the real `search()` exactly before any variant is trusted.
//!
//! Usage:
//!     cargo run -p konnect-core --release --example retrieval_probe \
//!         [path/to/_retrieval_intents.json]
//!
//! Default input: `bench/results/_retrieval_intents.json`.

use konnect_core::router::capability_search::search as prod_search;
use konnect_core::router::ToolRouter;
use konnect_core::tools::ToolDef;
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Deserialize)]
struct TaskIntents {
    task: String,
    intents: Vec<String>,
    needed: Vec<String>,
}

const PROBE_LIMIT: usize = 30;

// ---------------------------------------------------------------------------
// Duplicated from `capability_search.rs` (private there). Kept byte-for-byte
// in spirit so the `current` variant is a faithful reimplementation, verified
// against the real `search()` at startup.
// ---------------------------------------------------------------------------

const SYNONYMS: &[(&str, &[&str])] = &[
    ("cap", &["capacitor", "component", "symbol"]),
    ("capacitor", &["component", "symbol"]),
    ("resistor", &["component", "symbol"]),
    ("chip", &["component", "symbol", "footprint"]),
    ("ic", &["component", "symbol"]),
    ("part", &["component", "symbol", "footprint"]),
    ("wire", &["connect", "wire"]),
    ("connect", &["wire", "connect"]),
    ("rail", &["power", "net"]),
    ("supply", &["power"]),
    ("gnd", &["power", "ground"]),
    ("ground", &["power"]),
    ("decoupling", &["decoupling", "capacitor", "component"]),
    ("erc", &["erc", "electrical", "check"]),
    ("drc", &["drc", "design", "rules", "check"]),
    ("bom", &["bom", "bill", "materials"]),
    ("netlist", &["netlist", "export"]),
    ("gerber", &["gerber", "export", "fabrication"]),
    ("fab", &["manufacturing", "fabrication", "gerber"]),
    ("place", &["place", "add", "position"]),
    ("route", &["route", "trace", "track"]),
    ("track", &["trace", "route"]),
    ("sheet", &["sheet", "hierarchical", "hierarchy"]),
    ("label", &["label", "net"]),
    ("footprint", &["footprint", "library"]),
    ("symbol", &["symbol", "library"]),
];

/// Axis G3: location / designation vocabulary. Only words that actually
/// occur in the corpus can ever pay; the run prints each one's document
/// frequency so an inert entry is visible.
const G3_SYNONYMS: &[(&str, &[&str])] = &[
    (
        "where",
        &[
            "location",
            "locations",
            "position",
            "positions",
            "coordinates",
        ],
    ),
    ("reference", &["designator"]),
];

/// Axis G4: description rewrites, applied to the probe's corpus only. Same
/// meaning, but the concept is named with both domain words.
const G4_DESCRIPTIONS: &[(&str, &str)] = &[
    (
        "get_schematic_component",
        "Get all properties, position, and pin locations for a single schematic component, that is one symbol instance, looked up by its reference designator.",
    ),
    (
        "get_schematic_pin_locations",
        "Get the exact schematic-space (X,Y) coordinates showing where every pin of a component symbol is located, accounting for rotation and mirroring. Uses the canonical pin transform.",
    ),
];

const STOP_WORDS: &[&str] = &[
    "a", "an", "and", "at", "by", "for", "from", "in", "into", "is", "it", "of", "on", "or", "the",
    "to", "with", "my", "this", "that", "please", "kicad",
];

fn terms(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_ascii_lowercase())
        .collect()
}

fn is_stop_word(term: &str) -> bool {
    STOP_WORDS.contains(&term)
}

fn expansions(term: &str) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = SYNONYMS
        .iter()
        .find(|(k, _)| *k == term)
        .map(|(_, v)| v.to_vec())
        .unwrap_or_default();
    if g_flag(&G2_COMPONENT_SYMBOL) {
        match term {
            "component" => out.push("symbol"),
            "symbol" => out.push("component"),
            _ => {}
        }
    }
    if g_flag(&G3_LOCATION_VOCAB) {
        if let Some((_, extra)) = G3_SYNONYMS.iter().find(|(k, _)| *k == term) {
            out.extend_from_slice(extra);
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

fn query_terms(query: &str) -> Vec<String> {
    terms(query)
        .into_iter()
        .filter(|t| !is_stop_word(t))
        .filter(|t| !(g_flag(&G1_DROP_ONE_CHAR) && t.chars().count() == 1))
        .collect()
}

// ---------------------------------------------------------------------------
// Axis G (F.5.5): vocabulary levers, simulated in the probe only. Each lever
// is a process-global switch consulted by `query_terms` / `expansions`, so it
// reaches the scoring code without threading a config through every call.
// They are all OFF by default, which is what `sanity_check` runs under.
//   G1 drops one-character query terms (the `s` of "component's").
//   G2 links component <-> symbol both ways.
//   G3 adds location/designation vocabulary.
//   G4 is *not* here: it rewrites two descriptions, so it is a separate
//      corpus built by `Corpus::with_g4_descriptions`.
// ---------------------------------------------------------------------------

static G1_DROP_ONE_CHAR: AtomicBool = AtomicBool::new(false);
static G2_COMPONENT_SYMBOL: AtomicBool = AtomicBool::new(false);
static G3_LOCATION_VOCAB: AtomicBool = AtomicBool::new(false);

fn g_flag(flag: &AtomicBool) -> bool {
    flag.load(Ordering::Relaxed)
}

fn set_g(g1: bool, g2: bool, g3: bool) {
    G1_DROP_ONE_CHAR.store(g1, Ordering::Relaxed);
    G2_COMPONENT_SYMBOL.store(g2, Ordering::Relaxed);
    G3_LOCATION_VOCAB.store(g3, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Scoring variants. `Current` reproduces `score_tool` (weight 1 per term).
// `Idf` multiplies each query term's whole contribution (name + description +
// synonyms) by that term's BM25-style IDF over the corpus. `IdfNorm` further
// divides the description-derived part of the score by a BM25 length-norm
// factor, so a long description stops out-accumulating a short, precise one.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Variant {
    Current,
    Idf,
    IdfNorm,
}

impl Variant {
    const ALL: [Variant; 3] = [Variant::Current, Variant::Idf, Variant::IdfNorm];

    fn label(self) -> &'static str {
        match self {
            Variant::Current => "current",
            Variant::Idf => "idf",
            Variant::IdfNorm => "idf_norm",
        }
    }
}

struct Corpus {
    tools: Vec<(&'static str, ToolDef)>,
    idf: HashMap<String, f64>,
    avg_desc_len: f64,
}

impl Corpus {
    fn build() -> Self {
        Self::from_tools(ToolRouter::new().all_tools_with_toolset())
    }

    /// Axis G4: the same corpus, with the two self-describing-badly tools'
    /// descriptions rewritten *locally* (never in `sch_components.rs`). The
    /// IDF table is recomputed from the rewritten text, since the rewrite
    /// changes document frequencies.
    fn with_g4_descriptions() -> Self {
        let mut tools = ToolRouter::new().all_tools_with_toolset();
        for (_, def) in &mut tools {
            if let Some((_, text)) = G4_DESCRIPTIONS.iter().find(|(n, _)| *n == def.name) {
                def.description = text;
            }
        }
        Self::from_tools(tools)
    }

    fn from_tools(tools: Vec<(&'static str, ToolDef)>) -> Self {
        let n = tools.len() as f64;

        let mut df: HashMap<String, usize> = HashMap::new();
        let mut desc_lens = Vec::with_capacity(tools.len());
        for (_, def) in &tools {
            let mut vocab: Vec<String> = terms(def.name);
            vocab.extend(terms(def.description));
            vocab.sort();
            vocab.dedup();
            for t in vocab {
                *df.entry(t).or_insert(0) += 1;
            }
            desc_lens.push(terms(def.description).len());
        }
        let idf = df
            .into_iter()
            .map(|(t, d)| {
                let d = d as f64;
                let v = ((n - d + 0.5) / (d + 0.5) + 1.0).ln().max(0.0);
                (t, v)
            })
            .collect();
        let avg_desc_len = desc_lens.iter().sum::<usize>() as f64 / desc_lens.len() as f64;

        Corpus {
            tools,
            idf,
            avg_desc_len,
        }
    }

    fn idf_of(&self, term: &str) -> f64 {
        *self.idf.get(term).unwrap_or(&0.0)
    }
}

const BM25_B: f64 = 0.75;

fn desc_len_norm(dl: usize, avgdl: f64) -> f64 {
    1.0 - BM25_B + BM25_B * (dl as f64 / avgdl)
}

// Minimum corpus-term length for axis D's reverse-prefix rule, so it does not
// fire on noise like "a" or "add" starting a long query word.
// Kept in sync with `capability_search::REVERSE_PREFIX_MIN_LEN` (production).
const REVERSE_PREFIX_MIN_LEN: usize = 3;

fn score_tool_variant(
    variant: Variant,
    reverse_prefix: bool,
    min_len: usize,
    corpus: &Corpus,
    def: &ToolDef,
    qterms: &[String],
) -> f64 {
    let name_terms = terms(def.name);
    let desc_terms = terms(def.description);
    let desc_factor = match variant {
        Variant::IdfNorm => 1.0 / desc_len_norm(desc_terms.len(), corpus.avg_desc_len),
        _ => 1.0,
    };

    let mut score = 0.0f64;
    for qt in qterms {
        let w = match variant {
            Variant::Current => 1.0,
            Variant::Idf | Variant::IdfNorm => corpus.idf_of(qt),
        };

        // Axis D: reverse prefix. The existing 10/5/3 cascade only matches a
        // corpus term that is a prefix of (or equal to) the query term, which
        // can never fire when the query uses a plural ("pins") of a corpus
        // singular ("pin") — the query term is the longer one. This branch is
        // last resort: it only fires if none of 10/5/3 already matched.
        let name_contrib = if name_terms.iter().any(|t| t == qt) {
            10.0
        } else if name_terms.iter().any(|t| t.starts_with(qt.as_str())) {
            5.0
        } else if def.name.contains(qt.as_str()) {
            3.0
        } else if reverse_prefix
            && name_terms
                .iter()
                .any(|t| t.len() >= min_len && qt.starts_with(t.as_str()))
        {
            4.0
        } else {
            0.0
        };
        score += name_contrib * w;

        let desc_contrib = if desc_terms.iter().any(|t| t == qt) {
            2.0
        } else if reverse_prefix
            && desc_terms
                .iter()
                .any(|t| t.len() >= min_len && qt.starts_with(t.as_str()))
        {
            1.0
        } else {
            0.0
        };
        score += desc_contrib * w * desc_factor;

        for syn in expansions(qt) {
            // Mirrors `score_tool`'s `if name match { 4 } else if desc match { 1 }`:
            // a synonym scores once, on the stronger field it hits, not both.
            if name_terms.iter().any(|t| t == syn) {
                score += 4.0 * w;
            } else if desc_terms.iter().any(|t| t == syn) {
                score += 1.0 * w * desc_factor;
            }
        }
    }
    score
}

/// Same per-term math as `score_tool_variant`'s loop body, isolated to one
/// query term, for diagnostics only: "which term paid for this hit, and by
/// how much".
fn term_contribution(
    variant: Variant,
    reverse_prefix: bool,
    min_len: usize,
    corpus: &Corpus,
    def: &ToolDef,
    qt: &str,
) -> f64 {
    let name_terms = terms(def.name);
    let desc_terms = terms(def.description);
    let desc_factor = match variant {
        Variant::IdfNorm => 1.0 / desc_len_norm(desc_terms.len(), corpus.avg_desc_len),
        _ => 1.0,
    };
    let w = match variant {
        Variant::Current => 1.0,
        Variant::Idf | Variant::IdfNorm => corpus.idf_of(qt),
    };

    let name_contrib = if name_terms.iter().any(|t| t == qt) {
        10.0
    } else if name_terms.iter().any(|t| t.starts_with(qt)) {
        5.0
    } else if def.name.contains(qt) {
        3.0
    } else if reverse_prefix
        && name_terms
            .iter()
            .any(|t| t.len() >= min_len && qt.starts_with(t.as_str()))
    {
        4.0
    } else {
        0.0
    };
    let mut score = name_contrib * w;

    let desc_contrib = if desc_terms.iter().any(|t| t == qt) {
        2.0
    } else if reverse_prefix
        && desc_terms
            .iter()
            .any(|t| t.len() >= min_len && qt.starts_with(t.as_str()))
    {
        1.0
    } else {
        0.0
    };
    score += desc_contrib * w * desc_factor;

    for syn in expansions(qt) {
        if name_terms.iter().any(|t| t == syn) {
            score += 4.0 * w;
        } else if desc_terms.iter().any(|t| t == syn) {
            score += 1.0 * w * desc_factor;
        }
    }
    score
}

/// Would axis D's reverse-prefix rule fire for this (term, tool) pair, if it
/// were on? Diagnostic only — explains a zero-contribution term.
fn reverse_prefix_would_fire(def: &ToolDef, qt: &str, min_len: usize) -> bool {
    let name_terms = terms(def.name);
    let desc_terms = terms(def.description);
    name_terms
        .iter()
        .chain(desc_terms.iter())
        .any(|t| t.len() >= min_len && qt.starts_with(t.as_str()))
}

/// Rank the whole corpus against `query` under `variant`, unbounded, best
/// first, ties broken on name (mirrors `capability_search::search`).
fn rank_all(
    variant: Variant,
    reverse_prefix: bool,
    corpus: &Corpus,
    query: &str,
) -> Vec<(&'static str, f64)> {
    rank_all_ml(
        variant,
        reverse_prefix,
        REVERSE_PREFIX_MIN_LEN,
        corpus,
        query,
    )
}

/// `rank_all` with an explicit reverse-prefix floor, for the floor sweep.
fn rank_all_ml(
    variant: Variant,
    reverse_prefix: bool,
    min_len: usize,
    corpus: &Corpus,
    query: &str,
) -> Vec<(&'static str, f64)> {
    let qterms = query_terms(query);
    if qterms.is_empty() {
        return Vec::new();
    }
    let mut hits: Vec<(&'static str, f64)> = corpus
        .tools
        .iter()
        .filter_map(|(_, def)| {
            let s = score_tool_variant(variant, reverse_prefix, min_len, corpus, def, &qterms);
            (s > 0.0).then_some((def.name, s))
        })
        .collect();
    hits.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap().then_with(|| a.0.cmp(b.0)));
    hits
}

/// Axis E: cut at the first sharp drop between consecutive ranked scores
/// (`score[i+1] < gamma * score[i]`), instead of a fixed ratio to the query's
/// best score. A floor keeps at least 3 results (or all of them, if fewer)
/// even when the very first drop is sharp, so a flat, undifferentiated query
/// is not strangled to one hit.
fn decrochage_keep_count(ranked: &[(&'static str, f64)], gamma: f64) -> usize {
    let n = ranked.len();
    if n == 0 {
        return 0;
    }
    let mut cut = n;
    for i in 0..n.saturating_sub(1) {
        if ranked[i + 1].1 < gamma * ranked[i].1 {
            cut = i + 1;
            break;
        }
    }
    cut.max(3.min(n))
}

#[derive(Clone, Copy, Debug)]
enum CutE {
    None,
    Decrochage(f64),
}

impl CutE {
    fn label(self) -> String {
        match self {
            CutE::None => "none".to_string(),
            CutE::Decrochage(g) => format!("decrochage({g:.1})"),
        }
    }
}

fn apply_cut_e(ranked: &[(&'static str, f64)], cut: CutE, limit: usize) -> Vec<&'static str> {
    let kept = match cut {
        CutE::None => ranked.len(),
        CutE::Decrochage(gamma) => decrochage_keep_count(ranked, gamma),
    };
    ranked
        .iter()
        .take(kept.min(limit))
        .map(|(n, _)| *n)
        .collect()
}

/// Cutoff-then-limit, exactly the order the coordinator specified: keep hits
/// with `score >= tau * best_score_of_the_query`, *then* truncate to `limit`.
fn cutoff_then_limit(ranked: &[(&'static str, f64)], tau: f64, limit: usize) -> Vec<&'static str> {
    let Some(&(_, max)) = ranked.first() else {
        return Vec::new();
    };
    ranked
        .iter()
        .filter(|(_, s)| *s >= tau * max)
        .take(limit)
        .map(|(n, _)| *n)
        .collect()
}

/// Production check at startup.
///
/// Until F.5 this asserted that the local `current D=on` reimplementation
/// reproduced `search()` hit for hit and score for score. That equality is
/// gone on purpose: production now runs the F.5 pipeline (IDF weighting,
/// clause splitting, per-clause relative cutoff, one-per-family cap), which
/// this probe simulates as axes F/G/I on top of a *different* baseline. What
/// is still worth failing loudly on is the behaviour that pipeline is
/// supposed to have, so that is what is asserted here.
fn sanity_check(corpus: &Corpus) {
    let all = corpus.tools.len();

    // D6 negative control, the one that has to hold in every run.
    let d6 = prod_search(
        &corpus.tools,
        "place multiple symbols on the schematic in one call",
        8,
    );
    assert_eq!(
        d6.first().map(|h| h.name),
        Some("batch_place_components"),
        "production lost the D6 rank-1 control"
    );

    // Clause splitting: both halves of a composite intent come back.
    let composite: Vec<&str> = prod_search(
        &corpus.tools,
        "search reference circuit templates and instantiate one",
        8,
    )
    .into_iter()
    .map(|h| h.name)
    .collect();
    for expected in ["search_templates", "apply_template"] {
        assert!(
            composite.contains(&expected),
            "production dropped {expected} from a composite intent: {composite:?}"
        );
    }

    // A decided query stops well short of the limit.
    let decided = prod_search(&corpus.tools, "run erc", 8);
    assert!(
        decided.len() < 8,
        "production padded a decided query to the limit: {} hits",
        decided.len()
    );

    // The scores are still positive and sorted the way the caller sees them.
    let ranked = prod_search(&corpus.tools, "export bom and netlist", 8);
    assert!(ranked.iter().all(|h| h.score > 0.0));
    assert!(!ranked.is_empty() && ranked.len() <= 8);

    println!(
        "sanity check: production pipeline holds D6, clause splitting, and the cutoff ({all} tools in corpus)"
    );
}

// ---------------------------------------------------------------------------
// Task perimeters and the runner's own precision/recall math.
// ---------------------------------------------------------------------------

fn is_historical(task: &str) -> bool {
    // 07_sch_inspection was added in 16b9119, after the 22.4 %/100 % line in
    // docs/benchmark.md was measured on the other 6.
    !task.starts_with("07_")
}

struct TaskResult {
    task: String,
    found: usize,
    precision: f64,
    recall: f64,
}

fn score_task(
    variant: Variant,
    corpus: &Corpus,
    task: &TaskIntents,
    tau: f64,
    limit: usize,
) -> TaskResult {
    score_task_tau(variant, false, corpus, task, tau, limit).0
}

/// Like `score_task`, generalized with axis D, and also returning the union
/// itself — needed by the missing-tool analysis and the fine tau sweep.
fn score_task_tau(
    variant: Variant,
    reverse_prefix: bool,
    corpus: &Corpus,
    task: &TaskIntents,
    tau: f64,
    limit: usize,
) -> (TaskResult, Vec<&'static str>) {
    score_task_tau_ml(
        variant,
        reverse_prefix,
        REVERSE_PREFIX_MIN_LEN,
        corpus,
        task,
        tau,
        limit,
    )
}

/// `score_task_tau` with an explicit reverse-prefix floor, for the floor sweep.
fn score_task_tau_ml(
    variant: Variant,
    reverse_prefix: bool,
    min_len: usize,
    corpus: &Corpus,
    task: &TaskIntents,
    tau: f64,
    limit: usize,
) -> (TaskResult, Vec<&'static str>) {
    let mut found: Vec<&str> = Vec::new();
    for intent in &task.intents {
        let ranked = rank_all_ml(variant, reverse_prefix, min_len, corpus, intent);
        for name in cutoff_then_limit(&ranked, tau, limit) {
            if !found.contains(&name) {
                found.push(name);
            }
        }
    }
    let hits = task
        .needed
        .iter()
        .filter(|n| found.contains(&n.as_str()))
        .count();
    let precision = if found.is_empty() {
        0.0
    } else {
        hits as f64 / found.len() as f64
    };
    let recall = if task.needed.is_empty() {
        1.0
    } else {
        hits as f64 / task.needed.len() as f64
    };
    let result = TaskResult {
        task: task.task.clone(),
        found: found.len(),
        precision,
        recall,
    };
    (result, found)
}

struct PerimeterAgg {
    precision: f64,
    recall: f64,
    avg_union: f64,
    failing: Vec<String>,
}

fn aggregate(results: &[TaskResult], filter: impl Fn(&str) -> bool) -> PerimeterAgg {
    let subset: Vec<&TaskResult> = results.iter().filter(|r| filter(&r.task)).collect();
    let n = subset.len() as f64;
    let precision = subset.iter().map(|r| r.precision).sum::<f64>() / n;
    let recall = subset.iter().map(|r| r.recall).sum::<f64>() / n;
    let avg_union = subset.iter().map(|r| r.found as f64).sum::<f64>() / n;
    let failing = subset
        .iter()
        .filter(|r| r.recall < 1.0)
        .map(|r| r.task.clone())
        .collect();
    PerimeterAgg {
        precision,
        recall,
        avg_union,
        failing,
    }
}

fn main() {
    let arg = std::env::args().nth(1);
    let path = arg
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("bench/results/_retrieval_intents.json"));

    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let tasks: Vec<TaskIntents> = serde_json::from_str(&raw)
        .unwrap_or_else(|e| panic!("cannot parse {}: {e}", path.display()));

    let corpus = Corpus::build();
    sanity_check(&corpus);

    // --- Section 1: current search, ranked hits + needed-tool summary. -----
    for task in &tasks {
        println!("\n==== task {} ====", task.task);
        let mut best: BTreeMap<&str, (usize, f64, &str)> = BTreeMap::new();
        for intent in &task.intents {
            println!("-- intent: {intent:?}");
            let hits = prod_search(&corpus.tools, intent, PROBE_LIMIT);
            for (i, hit) in hits.iter().enumerate() {
                let rank = i + 1;
                let needed = task.needed.iter().any(|n| n == hit.name);
                println!(
                    "  {rank:>2} {:>6.1} {}{}",
                    hit.score,
                    hit.name,
                    if needed { " [NEEDED]" } else { "" }
                );
                if needed {
                    let e = best.entry(hit.name).or_insert((rank, hit.score, intent));
                    if hit.score > e.1 {
                        *e = (rank, hit.score, intent);
                    }
                }
            }
        }
        println!("-- needed summary for {} --", task.task);
        for tool in &task.needed {
            match best.get(tool.as_str()) {
                Some((rank, score, intent)) => {
                    println!("  {tool}: best rank {rank}, score {score}, via {intent:?}");
                }
                None => println!("  {tool}: MISSING (not in any top-{PROBE_LIMIT})"),
            }
        }
    }

    // --- Section 2: reference sweep, current/tau=0, both perimeters. -------
    println!("\n==== reference: current scoring, tau=0.0 (matches production search()) ====");
    for &limit in &[5usize, 8] {
        let results: Vec<TaskResult> = tasks
            .iter()
            .map(|t| score_task(Variant::Current, &corpus, t, 0.0, limit))
            .collect();
        let hist = aggregate(&results, is_historical);
        let all = aggregate(&results, |_| true);
        println!(
            "  limit={limit:>2}  6-task historical: precision={:.1}% recall={:.1}% avg_union={:.1}  |  7-task: precision={:.1}% recall={:.1}% avg_union={:.1}",
            hist.precision * 100.0, hist.recall * 100.0, hist.avg_union,
            all.precision * 100.0, all.recall * 100.0, all.avg_union,
        );
    }

    // --- Section 3: 3 x 6 x 2 = 36-combination grid. ------------------------
    const TAUS: &[f64] = &[0.0, 0.3, 0.4, 0.5, 0.6, 0.7];
    const LIMITS: &[usize] = &[5, 8];

    struct Row {
        variant: Variant,
        tau: f64,
        limit: usize,
        hist: PerimeterAgg,
        all: PerimeterAgg,
    }

    let mut rows = Vec::new();
    for &variant in &Variant::ALL {
        for &tau in TAUS {
            for &limit in LIMITS {
                let results: Vec<TaskResult> = tasks
                    .iter()
                    .map(|t| score_task(variant, &corpus, t, tau, limit))
                    .collect();
                rows.push(Row {
                    variant,
                    tau,
                    limit,
                    hist: aggregate(&results, is_historical),
                    all: aggregate(&results, |_| true),
                });
            }
        }
    }

    // recall==100% on the 6-task historical perimeter first, sorted by
    // precision desc within that group; everything else follows.
    rows.sort_by(|a, b| {
        let a_ok = a.hist.recall >= 0.9999;
        let b_ok = b.hist.recall >= 0.9999;
        b_ok.cmp(&a_ok)
            .then(b.hist.precision.partial_cmp(&a.hist.precision).unwrap())
    });

    println!("\n==== 36-combination grid (scoring x tau x limit) ====");
    println!("variant   tau  lim | hist6: prec  recall union | all7: prec  recall union | fail");
    for r in &rows {
        let fail = if r.all.failing.is_empty() {
            String::new()
        } else {
            format!("fail:{}", r.all.failing.join(","))
        };
        println!(
            "{:<9} {:.1}  {:>2}  | {:>5.1}% {:>6.1}% {:>5.1}      | {:>5.1}% {:>6.1}% {:>5.1}     | {}",
            r.variant.label(),
            r.tau,
            r.limit,
            r.hist.precision * 100.0,
            r.hist.recall * 100.0,
            r.hist.avg_union,
            r.all.precision * 100.0,
            r.all.recall * 100.0,
            r.all.avg_union,
            fail,
        );
    }

    let best = &rows[0];
    println!(
        "\nbest (recall=100% on 6-task historical, highest precision): {} tau={} limit={}",
        best.variant.label(),
        best.tau,
        best.limit
    );

    // --- Corrected ratio (mise au point 2): per needed tool, the intent that
    // gives it its BEST SCORE (not any intent), ratio = that score / the best
    // score anywhere in that same intent. Minimum over all (task, tool) pairs
    // in a perimeter is the highest cutoff tau that loses no recall.
    fn min_needed_ratio(
        variant: Variant,
        corpus: &Corpus,
        tasks: &[TaskIntents],
        filter: impl Fn(&str) -> bool,
    ) -> f64 {
        let mut min_ratio = f64::INFINITY;
        for task in tasks.iter().filter(|t| filter(&t.task)) {
            for tool in &task.needed {
                let mut best_score = f64::NEG_INFINITY;
                let mut best_ratio = f64::NEG_INFINITY;
                for intent in &task.intents {
                    let ranked = rank_all(variant, false, corpus, intent);
                    let Some(&(_, max)) = ranked.first() else {
                        continue;
                    };
                    let score = ranked
                        .iter()
                        .find(|(n, _)| *n == tool.as_str())
                        .map(|(_, s)| *s)
                        .unwrap_or(0.0);
                    if score > best_score {
                        best_score = score;
                        best_ratio = score / max;
                    }
                }
                if best_ratio.is_finite() {
                    min_ratio = min_ratio.min(best_ratio);
                }
            }
        }
        min_ratio
    }

    println!(
        "\n==== corrected minimal needed-tool ratio (best intent per tool, mise au point 2) ===="
    );
    for &(label, v) in &[
        ("current", Variant::Current),
        (best.variant.label(), best.variant),
    ] {
        let r6 = min_needed_ratio(v, &corpus, &tasks, is_historical);
        let r7 = min_needed_ratio(v, &corpus, &tasks, |_| true);
        println!("  {label:<9} 6-task historical min ratio={r6:.3}   7-task min ratio={r7:.3}");
    }

    // --- Needed tools ranked past 8 under the best combination. -------------
    println!("\n==== needed tools ranked past 8, under best combination's scoring ====");
    let mut any = false;
    for task in &tasks {
        for tool in &task.needed {
            let mut best_rank: Option<(usize, &str)> = None;
            for intent in &task.intents {
                let ranked = rank_all(best.variant, false, &corpus, intent);
                if let Some(pos) = ranked.iter().position(|(n, _)| *n == tool.as_str()) {
                    let rank = pos + 1;
                    if best_rank.is_none_or(|(r, _)| rank < r) {
                        best_rank = Some((rank, intent));
                    }
                }
            }
            match best_rank {
                Some((rank, intent)) if rank > 8 => {
                    any = true;
                    println!(
                        "  {}/{}: best rank {} via {:?}",
                        task.task, tool, rank, intent
                    );
                }
                None => {
                    any = true;
                    println!(
                        "  {}/{}: not found in any intent's ranking",
                        task.task, tool
                    );
                }
                _ => {}
            }
        }
    }
    if !any {
        println!("  none");
    }

    // --- 07_sch_inspection: best rank ever achieved by get_schematic_pin_locations. ---
    println!("\n==== 07_sch_inspection: get_schematic_pin_locations across all variants ====");
    if let Some(task) = tasks.iter().find(|t| t.task == "07_sch_inspection") {
        let mut overall_best: Option<(usize, Variant, &str)> = None;
        for &variant in &Variant::ALL {
            for intent in &task.intents {
                let ranked = rank_all(variant, false, &corpus, intent);
                if let Some(pos) = ranked
                    .iter()
                    .position(|(n, _)| *n == "get_schematic_pin_locations")
                {
                    let rank = pos + 1;
                    if overall_best.is_none_or(|(r, _, _)| rank < r) {
                        overall_best = Some((rank, variant, intent));
                    }
                }
            }
        }
        match overall_best {
            Some((rank, variant, intent)) => println!(
                "  best rank ever observed: {rank} under {} via {:?}",
                variant.label(),
                intent
            ),
            None => println!("  never found by any intent under any variant"),
        }
        let always_fails_at_8 = rows
            .iter()
            .filter(|r| r.limit == 8)
            .all(|r| r.all.failing.iter().any(|f| f == "07_sch_inspection"));
        println!(
            "  07_sch_inspection recall=100% ever reached at limit=8 across the grid: {}",
            if always_fails_at_8 {
                "NO"
            } else {
                "yes, at least once"
            }
        );
    }

    // =========================================================================
    // Section 4: axis D (reverse prefix, on/off) x axis E (decrochage cutoff,
    // none + 3 gammas) x scoring {current, idf} x limit {5, 8} = 32
    // combinations, plus the untouched reference (current, D off, E off,
    // limit=8) for comparison.
    // =========================================================================

    fn score_task_de(
        variant: Variant,
        reverse_prefix: bool,
        corpus: &Corpus,
        task: &TaskIntents,
        cut: CutE,
        limit: usize,
    ) -> TaskResult {
        let mut found: Vec<&str> = Vec::new();
        for intent in &task.intents {
            let ranked = rank_all(variant, reverse_prefix, corpus, intent);
            for name in apply_cut_e(&ranked, cut, limit) {
                if !found.contains(&name) {
                    found.push(name);
                }
            }
        }
        let hits = task
            .needed
            .iter()
            .filter(|n| found.contains(&n.as_str()))
            .count();
        let precision = if found.is_empty() {
            0.0
        } else {
            hits as f64 / found.len() as f64
        };
        let recall = if task.needed.is_empty() {
            1.0
        } else {
            hits as f64 / task.needed.len() as f64
        };
        TaskResult {
            task: task.task.clone(),
            found: found.len(),
            precision,
            recall,
        }
    }

    struct RowDE {
        variant: Variant,
        reverse_prefix: bool,
        cut: CutE,
        limit: usize,
        results: Vec<TaskResult>,
        hist: PerimeterAgg,
        all: PerimeterAgg,
    }

    const DE_SCORINGS: [Variant; 2] = [Variant::Current, Variant::Idf];
    const DE_LIMITS: [usize; 2] = [5, 8];
    let de_cuts = [
        CutE::None,
        CutE::Decrochage(0.5),
        CutE::Decrochage(0.6),
        CutE::Decrochage(0.7),
    ];

    let mut de_rows: Vec<RowDE> = Vec::new();
    for &variant in &DE_SCORINGS {
        for &reverse_prefix in &[false, true] {
            for &cut in &de_cuts {
                for &limit in &DE_LIMITS {
                    let results: Vec<TaskResult> = tasks
                        .iter()
                        .map(|t| score_task_de(variant, reverse_prefix, &corpus, t, cut, limit))
                        .collect();
                    de_rows.push(RowDE {
                        variant,
                        reverse_prefix,
                        cut,
                        limit,
                        hist: aggregate(&results, is_historical),
                        all: aggregate(&results, |_| true),
                        results,
                    });
                }
            }
        }
    }
    println!("\n(D x E grid: {} combinations computed)", de_rows.len());

    // The untouched reference: current scoring, D off, E off, limit=8. Same
    // number as section 2/3's baseline, recomputed here on the D/E code path
    // so it is directly comparable to the grid below.
    let reference = {
        let results: Vec<TaskResult> = tasks
            .iter()
            .map(|t| score_task_de(Variant::Current, false, &corpus, t, CutE::None, 8))
            .collect();
        RowDE {
            variant: Variant::Current,
            reverse_prefix: false,
            cut: CutE::None,
            limit: 8,
            hist: aggregate(&results, is_historical),
            all: aggregate(&results, |_| true),
            results,
        }
    };

    let mut de_sorted: Vec<&RowDE> = de_rows.iter().filter(|r| r.hist.recall >= 0.9999).collect();
    de_sorted.sort_by(|a, b| b.hist.precision.partial_cmp(&a.hist.precision).unwrap());

    println!(
        "\n==== reference + D x E combinations keeping recall=100% on the 6-task historical perimeter, sorted by precision desc ===="
    );
    println!(
        "  REFERENCE  current  D=off  E=none          lim=8 | hist6: prec={:.1}% recall={:.1}% union={:.1} | all7: prec={:.1}% recall={:.1}% union={:.1}",
        reference.hist.precision * 100.0,
        reference.hist.recall * 100.0,
        reference.hist.avg_union,
        reference.all.precision * 100.0,
        reference.all.recall * 100.0,
        reference.all.avg_union,
    );
    for r in &de_sorted {
        println!(
            "  {:<8} D={:<3} E={:<15} lim={} | hist6: prec={:.1}% recall={:.1}% union={:.1} | all7: prec={:.1}% recall={:.1}% union={:.1}",
            r.variant.label(),
            if r.reverse_prefix { "on" } else { "off" },
            r.cut.label(),
            r.limit,
            r.hist.precision * 100.0,
            r.hist.recall * 100.0,
            r.hist.avg_union,
            r.all.precision * 100.0,
            r.all.recall * 100.0,
            r.all.avg_union,
        );
    }

    match de_sorted.first() {
        Some(best_de) if best_de.hist.precision > reference.hist.precision + 1e-9 => {
            println!(
                "\nbest D x E combination beats the reference on 6-task historical precision: {} D={} E={} limit={} ({:.1}% vs {:.1}%)",
                best_de.variant.label(),
                if best_de.reverse_prefix { "on" } else { "off" },
                best_de.cut.label(),
                best_de.limit,
                best_de.hist.precision * 100.0,
                reference.hist.precision * 100.0,
            );

            // Ranks of the two 07_sch_inspection tools and of
            // batch_place_components on task 06 (D6's negative control),
            // under the best combination's scoring + D setting.
            if let Some(task07) = tasks.iter().find(|t| t.task == "07_sch_inspection") {
                for tool in ["get_schematic_pin_locations", "get_schematic_component"] {
                    let mut best_rank: Option<(usize, &str)> = None;
                    for intent in &task07.intents {
                        let ranked =
                            rank_all(best_de.variant, best_de.reverse_prefix, &corpus, intent);
                        if let Some(pos) = ranked.iter().position(|(n, _)| *n == tool) {
                            let rank = pos + 1;
                            if best_rank.is_none_or(|(r, _)| rank < r) {
                                best_rank = Some((rank, intent));
                            }
                        }
                    }
                    match best_rank {
                        Some((rank, intent)) => println!(
                            "  07_sch_inspection/{tool}: best rank {rank} via {intent:?} under best combination"
                        ),
                        None => println!(
                            "  07_sch_inspection/{tool}: not found under best combination"
                        ),
                    }
                }
            }
        }
        _ => {
            println!(
                "\nno D x E combination beats the reference on 6-task historical precision ({:.1}%); reference stands.",
                reference.hist.precision * 100.0
            );
        }
    }

    // D6 negative control: batch_place_components on task 06's intent
    // "place multiple symbols on the schematic in one call", rank before
    // (D off) and after (D on), for both scoring variants.
    println!("\n==== D6 negative control: batch_place_components rank, D off vs on ====");
    if let Some(task06) = tasks.iter().find(|t| t.task == "06_recovery") {
        let intent = "place multiple symbols on the schematic in one call";
        assert!(
            task06.intents.iter().any(|i| i == intent),
            "expected intent {intent:?} on task 06"
        );
        for &variant in &DE_SCORINGS {
            for &reverse_prefix in &[false, true] {
                let ranked = rank_all(variant, reverse_prefix, &corpus, intent);
                let rank = ranked
                    .iter()
                    .position(|(n, _)| *n == "batch_place_components")
                    .map(|p| p + 1);
                println!(
                    "  {} D={:<3}: batch_place_components rank = {}",
                    variant.label(),
                    if reverse_prefix { "on" } else { "off" },
                    rank.map(|r| r.to_string())
                        .unwrap_or_else(|| "not found".to_string()),
                );
            }
        }
    }

    // Does D alone (no E) already bring task 07 to recall=100% at limit=8?
    println!("\n==== D alone (E=none), limit=8: does 07_sch_inspection reach recall=100%? ====");
    for &variant in &DE_SCORINGS {
        if let Some(row) = de_rows.iter().find(|r| {
            r.variant == variant && r.reverse_prefix && matches!(r.cut, CutE::None) && r.limit == 8
        }) {
            let task07 = row.results.iter().find(|r| r.task == "07_sch_inspection");
            let recall_100 = task07.map(|r| r.recall >= 0.9999).unwrap_or(false);
            println!(
                "  {}: 07_sch_inspection recall={:.1}% (100%={})  | hist6 precision={:.1}%  all7 precision={:.1}%",
                variant.label(),
                task07.map(|r| r.recall * 100.0).unwrap_or(0.0),
                recall_100,
                row.hist.precision * 100.0,
                row.all.precision * 100.0,
            );
        }
    }

    // =========================================================================
    // Section 5: what the F.5 validation gate (precision@8 >= 60%, recall@8
    // >= 98%) actually costs under `idf tau=0.7 limit=8` — the row the
    // coordinator flagged as crossing docs/benchmark.md's frontier (61.5%
    // precision / 81.3% recall) at 65.5% / 94.0% on the 6-task historical
    // perimeter. Per-tool accounting of every needed tool that row still
    // misses, then a fine tau sweep around it.
    // =========================================================================

    fn find_def<'a>(corpus: &'a Corpus, name: &str) -> &'a ToolDef {
        &corpus
            .tools
            .iter()
            .find(|(_, d)| d.name == name)
            .unwrap_or_else(|| panic!("tool {name} not in corpus"))
            .1
    }

    println!("\n==== missing-tool accounting under idf tau=0.7 limit=8 (D off) ====");
    for task_name in [
        "03_sch_template_stm32",
        "05_manufacturing_exports",
        "07_sch_inspection",
    ] {
        let Some(task) = tasks.iter().find(|t| t.task == task_name) else {
            continue;
        };
        let (_, found) = score_task_tau(Variant::Idf, false, &corpus, task, 0.7, 8);
        let missing: Vec<&str> = task
            .needed
            .iter()
            .filter(|n| !found.contains(&n.as_str()))
            .map(|s| s.as_str())
            .collect();
        println!("-- {task_name} -- missing: {missing:?}");
        for tool in &missing {
            let def = find_def(&corpus, tool);
            // 2/3: best-scoring intent for this tool, its rank, its score,
            // and the query's own best score (for the ratio to the tau=0.7 cutoff).
            let mut best: Option<(&str, usize, f64, f64)> = None; // (intent, rank, score, query_max)
            for intent in &task.intents {
                let ranked = rank_all(Variant::Idf, false, &corpus, intent);
                let Some(&(_, qmax)) = ranked.first() else {
                    continue;
                };
                if let Some(pos) = ranked.iter().position(|(n, _)| *n == *tool) {
                    let score = ranked[pos].1;
                    if best.is_none_or(|(_, _, s, _)| score > s) {
                        best = Some((intent, pos + 1, score, qmax));
                    }
                }
            }
            let Some((intent, rank, score, qmax)) = best else {
                println!(
                    "  {tool}: not found by ANY intent (score 0 everywhere) — no ratio to report"
                );
                continue;
            };
            let ratio = score / qmax;
            println!(
                "  {tool}: best intent {intent:?}, rank {rank}, score {score:.2} / query-best {qmax:.2} = ratio {ratio:.3} (needs >= 0.700)"
            );

            // 4: which query terms pay for this tool's score, which don't.
            let qterms = query_terms(intent);
            let mut paying = Vec::new();
            let mut not_paying = Vec::new();
            for qt in &qterms {
                let c = term_contribution(
                    Variant::Idf,
                    false,
                    REVERSE_PREFIX_MIN_LEN,
                    &corpus,
                    def,
                    qt,
                );
                if c > 0.0 {
                    paying.push(format!("{qt}(+{c:.2})"));
                } else if reverse_prefix_would_fire(def, qt, REVERSE_PREFIX_MIN_LEN) {
                    not_paying.push(format!("{qt}(0, would score under axis D)"));
                } else {
                    not_paying.push(format!("{qt}(0, absent from name+description)"));
                }
            }
            println!("    paying terms: {}", paying.join(", "));
            println!("    non-paying terms: {}", not_paying.join(", "));

            // 5: who sits at rank 1 for that intent, and why.
            let ranked = rank_all(Variant::Idf, false, &corpus, intent);
            let (top_name, top_score) = ranked[0];
            let top_def = find_def(&corpus, top_name);
            let top_paying: Vec<String> = qterms
                .iter()
                .filter_map(|qt| {
                    let c = term_contribution(
                        Variant::Idf,
                        false,
                        REVERSE_PREFIX_MIN_LEN,
                        &corpus,
                        top_def,
                        qt,
                    );
                    (c > 0.0).then(|| format!("{qt}(+{c:.2})"))
                })
                .collect();
            println!(
                "    rank 1 is {top_name} (score {top_score:.2}), paid by: {}",
                top_paying.join(", ")
            );
        }
    }

    // Fine tau sweep around 0.7, idf only, D on/off, limit=8.
    const FINE_TAUS: [f64; 7] = [0.60, 0.62, 0.65, 0.68, 0.70, 0.72, 0.75];
    println!("\n==== fine tau sweep, idf, limit=8, D on/off ====");
    println!("tau   D   | hist6: prec  recall union failing                              | all7: prec  recall union failing");
    for &tau in &FINE_TAUS {
        for &reverse_prefix in &[false, true] {
            let mut results = Vec::new();
            for t in &tasks {
                let (r, _) = score_task_tau(Variant::Idf, reverse_prefix, &corpus, t, tau, 8);
                results.push(r);
            }
            let hist = aggregate(&results, is_historical);
            let all = aggregate(&results, |_| true);
            println!(
                "{:.2}  {:<3} | {:>5.1}% {:>6.1}% {:>5.1}  {:<38} | {:>5.1}% {:>6.1}% {:>5.1}  {}",
                tau,
                if reverse_prefix { "on" } else { "off" },
                hist.precision * 100.0,
                hist.recall * 100.0,
                hist.avg_union,
                hist.failing.join(","),
                all.precision * 100.0,
                all.recall * 100.0,
                all.avg_union,
                all.failing.join(","),
            );
        }
    }

    // =========================================================================
    // Section 6: measure `REVERSE_PREFIX_MIN_LEN` instead of postulating it.
    // Production configuration: `current` scoring, tau=0, limit=8, D on.
    // =========================================================================

    println!(
        "\n==== reverse-prefix floor sweep: current, tau=0, limit=8, D on (production config) ===="
    );
    println!("min_len | hist6: prec  recall union failing                              | all7: prec  recall union failing");
    let mut floor_ok: Vec<usize> = Vec::new();
    for &min_len in &[3usize, 4, 5] {
        let mut results = Vec::new();
        for t in &tasks {
            let (r, _) = score_task_tau_ml(Variant::Current, true, min_len, &corpus, t, 0.0, 8);
            results.push(r);
        }
        let hist = aggregate(&results, is_historical);
        let all = aggregate(&results, |_| true);
        println!(
            "{min_len:>7} | {:>5.1}% {:>6.1}% {:>5.1}  {:<38} | {:>5.1}% {:>6.1}% {:>5.1}  {}",
            hist.precision * 100.0,
            hist.recall * 100.0,
            hist.avg_union,
            hist.failing.join(","),
            all.precision * 100.0,
            all.recall * 100.0,
            all.avg_union,
            all.failing.join(","),
        );
        if hist.recall >= 0.9999 {
            floor_ok.push(min_len);
        }
    }

    // Ranks of interest at floor=3.
    println!("\n==== floor=3: ranks of interest ====");
    for (tool, intent) in [
        (
            "get_schematic_pin_locations",
            "where are a component's pins",
        ),
        (
            "get_schematic_pin_locations",
            "find nets with a single pin on them",
        ),
        ("get_schematic_component", "where are a component's pins"),
    ] {
        let ranked = rank_all_ml(Variant::Current, true, 3, &corpus, intent);
        match ranked.iter().position(|(n, _)| *n == tool) {
            Some(pos) => println!("  {tool} on {intent:?}: rank {}", pos + 1),
            None => println!("  {tool} on {intent:?}: not found (top-{})", ranked.len()),
        }
    }

    // D6 negative control at all three floors.
    println!("\n==== D6 negative control, all three floors ====");
    for &min_len in &[3usize, 4, 5] {
        let ranked = rank_all_ml(
            Variant::Current,
            true,
            min_len,
            &corpus,
            "place multiple symbols on the schematic in one call",
        );
        let rank = ranked
            .iter()
            .position(|(n, _)| *n == "batch_place_components")
            .map(|p| p + 1);
        println!(
            "  min_len={min_len}: batch_place_components rank = {}",
            rank.map(|r| r.to_string())
                .unwrap_or_else(|| "not found".to_string())
        );
    }

    // False positives at floor=3: every (query term, 3-letter name term, tool)
    // triple that fires the reverse-prefix branch across the golden intents,
    // deduplicated. floor=4 already covers name terms of length >= 4, so only
    // length-exactly-3 matches are new risk introduced by lowering the floor.
    println!("\n==== floor=3 false-positive scan across all golden intents ====");
    let mut triples: Vec<(String, String, &'static str)> = Vec::new();
    for task in &tasks {
        for intent in &task.intents {
            let qterms = query_terms(intent);
            for (_, def) in &corpus.tools {
                let name_terms = terms(def.name);
                for qt in &qterms {
                    // Only count it if the branch actually fires at floor=3,
                    // i.e. nothing in the 10/5/3 cascade already matched.
                    let exact = name_terms.iter().any(|t| t == qt);
                    let prefix = name_terms.iter().any(|t| t.starts_with(qt.as_str()));
                    let substr = def.name.contains(qt.as_str());
                    if exact || prefix || substr {
                        continue;
                    }
                    for nt in &name_terms {
                        if nt.len() == 3 && qt.starts_with(nt.as_str()) {
                            let triple = (qt.clone(), nt.clone(), def.name);
                            if !triples.contains(&triple) {
                                triples.push(triple);
                            }
                        }
                    }
                }
            }
        }
    }
    triples.sort();
    for (qt, nt, tool) in &triples {
        println!("  query={qt:<12} name_term={nt:<5} tool={tool}");
    }
    println!(
        "  ({} distinct (query term, 3-letter name term, tool) triples)",
        triples.len()
    );

    // Decision: adopt floor 3 in production only if it keeps hist6 recall
    // at 100% (measured above) AND the D6 control still holds at rank 1
    // (measured above, printed for min_len=3).
    println!(
        "\nfloors keeping hist6 recall=100%: {:?} — decision left to the caller of this run's output",
        floor_ok
    );

    // =======================================================================
    // Axis F (F.5.1): clause splitting, measurement only.
    // =======================================================================
    let f_taus: [f64; 7] = [0.0, 0.50, 0.60, 0.65, 0.70, 0.75, 0.80];
    assert_single_clause_identity(&corpus, &tasks, &f_taus);

    println!("\n==== axis F grid (D on, limit=8) ====");
    let mut grid: Vec<(FCfg, PerimeterAgg, PerimeterAgg)> = Vec::new();
    for &variant in &[Variant::Current, Variant::Idf] {
        for f_on in [false, true] {
            for &tau in &f_taus {
                for &pcl in if f_on {
                    &[3usize, 4, 8][..]
                } else {
                    &[8usize][..]
                } {
                    let cfg = FCfg {
                        variant,
                        reverse_prefix: true,
                        min_len: REVERSE_PREFIX_MIN_LEN,
                        f_on,
                        tau,
                        per_clause_limit: pcl,
                        limit: 8,
                    };
                    let results: Vec<TaskResult> = tasks
                        .iter()
                        .map(|t| score_task_f(cfg, &corpus, t).0)
                        .collect();
                    let h = aggregate(&results, is_historical);
                    let a = aggregate(&results, |_| true);
                    println!(
                        "{} | hist6: prec {:5.1}% recall {:5.1}% union {:4.1} failing {:?} | all7: prec {:5.1}% recall {:5.1}% union {:4.1} failing {:?}",
                        cfg.label(),
                        h.precision * 100.0,
                        h.recall * 100.0,
                        h.avg_union,
                        h.failing,
                        a.precision * 100.0,
                        a.recall * 100.0,
                        a.avg_union,
                        a.failing
                    );
                    grid.push((cfg, h, a));
                }
            }
        }
    }

    // --- A. Combinations meeting recall >= 98 % on both perimeters. ---------
    println!("\n==== A. recall >= 98% on hist6 AND all7, sorted by hist6 precision ====");
    let mut passing: Vec<&(FCfg, PerimeterAgg, PerimeterAgg)> = grid
        .iter()
        .filter(|(_, h, a)| h.recall >= 0.98 && a.recall >= 0.98)
        .collect();
    passing.sort_by(|x, y| y.1.precision.partial_cmp(&x.1.precision).unwrap());
    for (cfg, h, a) in &passing {
        println!(
            "{} | hist6: prec {:5.1}% recall {:5.1}% union {:4.1} | all7: prec {:5.1}% recall {:5.1}% union {:4.1}",
            cfg.label(),
            h.precision * 100.0,
            h.recall * 100.0,
            h.avg_union,
            a.precision * 100.0,
            a.recall * 100.0,
            a.avg_union
        );
    }
    if passing.is_empty() {
        println!("  (none)");
    }
    match passing.first() {
        Some((cfg, h, _)) if h.precision >= 0.60 => println!(
            "  VERDICT: target MET - {} reaches prec {:.1}% at recall >= 98% on both perimeters",
            cfg.label(),
            h.precision * 100.0
        ),
        Some((_, h, _)) => println!(
            "  VERDICT: target MISSED - best precision at recall >= 98% is {:.1}% (< 60%)",
            h.precision * 100.0
        ),
        None => println!("  VERDICT: target MISSED - no combination holds recall >= 98% on both"),
    }

    // --- A-bis. The hist6-only gate, when all7 is structurally out of reach. -
    println!("\n==== A-bis. recall >= 98% on hist6 only, sorted by hist6 precision ====");
    let mut passing_h6: Vec<&(FCfg, PerimeterAgg, PerimeterAgg)> =
        grid.iter().filter(|(_, h, _)| h.recall >= 0.98).collect();
    passing_h6.sort_by(|x, y| y.1.precision.partial_cmp(&x.1.precision).unwrap());
    for (cfg, h, a) in passing_h6.iter().take(12) {
        println!(
            "{} | hist6: prec {:5.1}% recall {:5.1}% union {:4.1} | all7: prec {:5.1}% recall {:5.1}% union {:4.1}",
            cfg.label(),
            h.precision * 100.0,
            h.recall * 100.0,
            h.avg_union,
            a.precision * 100.0,
            a.recall * 100.0,
            a.avg_union
        );
    }

    // Best combination: highest hist6 precision among A; if A is empty, the
    // same among A-bis; failing that, the best recall/precision compromise.
    let best = passing
        .first()
        .copied()
        .or_else(|| passing_h6.first().copied())
        .unwrap_or_else(|| {
            grid.iter()
                .max_by(|x, y| {
                    let sx = x.1.recall.min(x.2.recall) * 10.0 + x.1.precision;
                    let sy = y.1.recall.min(y.2.recall) * 10.0 + y.1.precision;
                    sx.partial_cmp(&sy).unwrap()
                })
                .expect("grid is never empty")
        });
    let best_cfg = best.0;
    println!(
        "\nbest combination for sections B and D: {} ({})",
        best_cfg.label(),
        if passing.is_empty() {
            "top of A-bis: hist6 gate, section A empty"
        } else {
            "top of section A"
        }
    );

    // --- B. Missing-tool accounting under the best combination. ------------
    println!(
        "\n==== B. missing-tool accounting under {} ====",
        best_cfg.label()
    );
    let mut any_missing = false;
    for task in &tasks {
        let (_, found) = score_task_f(best_cfg, &corpus, task);
        let missing: Vec<&str> = task
            .needed
            .iter()
            .filter(|n| !found.contains(&n.as_str()))
            .map(|s| s.as_str())
            .collect();
        if missing.is_empty() {
            continue;
        }
        any_missing = true;
        println!("-- {} -- missing: {missing:?}", task.task);
        for tool in &missing {
            let def = corpus
                .tools
                .iter()
                .find(|(_, d)| d.name == *tool)
                .map(|(_, d)| d)
                .expect("needed tool must exist in the corpus");
            // best (intent, clause, rank, ratio-to-clause-best) over all clauses
            let mut best_hit: Option<(&str, String, usize, f64)> = None;
            for intent in &task.intents {
                let clauses = if best_cfg.f_on {
                    split_clauses(intent)
                } else {
                    vec![intent.clone()]
                };
                for clause in clauses {
                    let ranked = rank_all_ml(
                        best_cfg.variant,
                        best_cfg.reverse_prefix,
                        best_cfg.min_len,
                        &corpus,
                        &clause,
                    );
                    let Some(&(_, cmax)) = ranked.first() else {
                        continue;
                    };
                    if let Some(pos) = ranked.iter().position(|(n, _)| *n == *tool) {
                        let ratio = ranked[pos].1 / cmax;
                        if best_hit.as_ref().is_none_or(|(_, _, _, r)| ratio > *r) {
                            best_hit = Some((intent, clause.clone(), pos + 1, ratio));
                        }
                    }
                }
            }
            let Some((intent, clause, rank, ratio)) = best_hit else {
                println!("  {tool}: not found by ANY clause (score 0 everywhere)");
                continue;
            };
            println!(
                "  {tool}: best intent {intent:?}, clause {clause:?}, rank {rank}, ratio {ratio:.3} (needs >= {:.3})",
                best_cfg.tau
            );
            let mut paying = Vec::new();
            let mut not_paying = Vec::new();
            for qt in &query_terms(&clause) {
                let c = term_contribution(
                    best_cfg.variant,
                    best_cfg.reverse_prefix,
                    best_cfg.min_len,
                    &corpus,
                    def,
                    qt,
                );
                if c > 0.0 {
                    paying.push(format!("{qt}(+{c:.2})"));
                } else {
                    not_paying.push(format!("{qt}(0)"));
                }
            }
            println!("    paying terms: {}", paying.join(", "));
            println!("    non-paying terms: {}", not_paying.join(", "));
            let ranked = rank_all_ml(
                best_cfg.variant,
                best_cfg.reverse_prefix,
                best_cfg.min_len,
                &corpus,
                &clause,
            );
            if let Some((n, s)) = ranked.first() {
                println!("    clause rank 1: {n} (score {s:.2})");
            }
        }
    }
    if !any_missing {
        println!("  (no missing tool on any task)");
    }

    // --- C. D6 negative control under F on and F off. ----------------------
    println!("\n==== C. D6 negative control (batch_place_components must stay rank 1) ====");
    let d6_query = "place multiple symbols on the schematic in one call";
    println!("  clauses: {:?}", split_clauses(d6_query));
    for &variant in &[Variant::Current, Variant::Idf] {
        for f_on in [false, true] {
            let cfg = FCfg {
                variant,
                reverse_prefix: true,
                min_len: REVERSE_PREFIX_MIN_LEN,
                f_on,
                tau: best_cfg.tau,
                per_clause_limit: best_cfg.per_clause_limit,
                limit: 8,
            };
            let hits = intent_hits_f(cfg, &corpus, d6_query);
            let rank = hits
                .iter()
                .position(|n| *n == "batch_place_components")
                .map(|p| p + 1);
            println!(
                "  {} -> batch_place_components rank {} {}",
                cfg.label(),
                rank.map(|r| r.to_string())
                    .unwrap_or_else(|| "absent".to_string()),
                if rank == Some(1) { "OK" } else { "LOST" }
            );
        }
    }

    // --- D. Per-task unions under the best combination. ---------------------
    println!("\n==== D. per-task union under {} ====", best_cfg.label());
    for task in &tasks {
        let (res, found) = score_task_f(best_cfg, &corpus, task);
        println!(
            "-- {} : union {} tools, prec {:.1}%, recall {:.1}%",
            task.task,
            found.len(),
            res.precision * 100.0,
            res.recall * 100.0
        );
        println!("   union: {found:?}");
    }

    // =======================================================================
    // Axis G (F.5.5): vocabulary. Measurement only, F=on throughout.
    // `sanity_check` already ran at the top of main, with every G lever off
    // and on the un-overridden corpus, so production is still compared to
    // itself.
    // =======================================================================
    set_g(false, false, false);
    let corpus_g4 = Corpus::with_g4_descriptions();

    println!("\n==== axis G: corpus document frequency of the G3 vocabulary ====");
    for (key, syns) in G3_SYNONYMS {
        for w in *syns {
            println!(
                "  {key} -> {w:<12} df={} (df={} under G4 rewrites)",
                corpus_df(&corpus, w),
                corpus_df(&corpus_g4, w)
            );
        }
    }
    for w in ["component", "symbol"] {
        println!("  (G2 term) {w:<12} df={}", corpus_df(&corpus, w));
    }

    let lever_sets = GLevers::SETS;
    let g_taus: [f64; 4] = [0.60, 0.65, 0.70, 0.75];

    // The clause-unique invariant must still hold under every lever set.
    for lv in lever_sets {
        lv.apply();
        assert_single_clause_identity(lv.corpus(&corpus, &corpus_g4), &tasks, &g_taus);
    }
    set_g(false, false, false);

    println!("\n==== axis G grid (D on, F on, limit=8) ====");
    let mut ggrid: Vec<(GLevers, FCfg, PerimeterAgg, PerimeterAgg)> = Vec::new();
    for lv in lever_sets {
        lv.apply();
        let c = lv.corpus(&corpus, &corpus_g4);
        for &variant in &[Variant::Current, Variant::Idf] {
            for &pcl in &[3usize, 4] {
                for &tau in &g_taus {
                    let cfg = FCfg {
                        variant,
                        reverse_prefix: true,
                        min_len: REVERSE_PREFIX_MIN_LEN,
                        f_on: true,
                        tau,
                        per_clause_limit: pcl,
                        limit: 8,
                    };
                    let results: Vec<TaskResult> =
                        tasks.iter().map(|t| score_task_f(cfg, c, t).0).collect();
                    let h = aggregate(&results, is_historical);
                    let a = aggregate(&results, |_| true);
                    println!(
                        "G={:<12} {} | hist6: prec {:5.1}% recall {:5.1}% union {:4.1} failing {:?} | all7: prec {:5.1}% recall {:5.1}% union {:4.1} failing {:?}",
                        lv.label,
                        cfg.label(),
                        h.precision * 100.0,
                        h.recall * 100.0,
                        h.avg_union,
                        h.failing,
                        a.precision * 100.0,
                        a.recall * 100.0,
                        a.avg_union,
                        a.failing
                    );
                    ggrid.push((lv, cfg, h, a));
                }
            }
        }
    }
    set_g(false, false, false);

    // --- A. recall >= 98 % on all7, sorted by all7 precision. --------------
    println!("\n==== G-A. recall >= 98% on all7, sorted by all7 precision ====");
    let mut gpass: Vec<&(GLevers, FCfg, PerimeterAgg, PerimeterAgg)> = ggrid
        .iter()
        .filter(|(_, _, _, a)| a.recall >= 0.98)
        .collect();
    gpass.sort_by(|x, y| y.3.precision.partial_cmp(&x.3.precision).unwrap());
    for (lv, cfg, h, a) in &gpass {
        println!(
            "G={:<12} {} | all7: prec {:5.1}% recall {:5.1}% union {:4.1} | hist6: prec {:5.1}% recall {:5.1}% union {:4.1}",
            lv.label,
            cfg.label(),
            a.precision * 100.0,
            a.recall * 100.0,
            a.avg_union,
            h.precision * 100.0,
            h.recall * 100.0,
            h.avg_union
        );
    }
    if gpass.is_empty() {
        println!("  (none)");
    }
    match gpass.first() {
        Some((lv, cfg, _, a)) if a.precision >= 0.60 => println!(
            "  VERDICT: target MET - G={} {} gives all7 prec {:.1}% at recall {:.1}%",
            lv.label,
            cfg.label(),
            a.precision * 100.0,
            a.recall * 100.0
        ),
        Some((lv, cfg, _, a)) => println!(
            "  VERDICT: target MISSED - best all7 precision at recall >= 98% is {:.1}% (G={} {})",
            a.precision * 100.0,
            lv.label,
            cfg.label()
        ),
        None => println!("  VERDICT: target MISSED - no combination holds all7 recall >= 98%"),
    }

    let gbest = gpass.first().copied().unwrap_or_else(|| {
        ggrid
            .iter()
            .max_by(|x, y| {
                let sx = x.3.recall * 10.0 + x.3.precision;
                let sy = y.3.recall * 10.0 + y.3.precision;
                sx.partial_cmp(&sy).unwrap()
            })
            .expect("grid is never empty")
    });
    let (gbest_lv, gbest_cfg) = (gbest.0, gbest.1);
    println!(
        "\nbest axis-G combination: G={} {} ({})",
        gbest_lv.label,
        gbest_cfg.label(),
        if gpass.is_empty() {
            "no combination clears all7 recall 98%, best recall/precision compromise"
        } else {
            "top of G-A"
        }
    );

    // --- B. Missing tools and per-task unions under that combination. ------
    gbest_lv.apply();
    let cbest = gbest_lv.corpus(&corpus, &corpus_g4);
    println!("\n==== G-B. missing tools under G={} ====", gbest_lv.label);
    for task in &tasks {
        let (_, found) = score_task_f(gbest_cfg, cbest, task);
        let missing: Vec<&str> = task
            .needed
            .iter()
            .filter(|n| !found.contains(&n.as_str()))
            .map(|s| s.as_str())
            .collect();
        if missing.is_empty() {
            continue;
        }
        println!("-- {} -- missing: {missing:?}", task.task);
        for tool in &missing {
            let mut best_hit: Option<(&str, String, usize, f64)> = None;
            for intent in &task.intents {
                for clause in split_clauses(intent) {
                    let ranked = rank_all_ml(
                        gbest_cfg.variant,
                        gbest_cfg.reverse_prefix,
                        gbest_cfg.min_len,
                        cbest,
                        &clause,
                    );
                    let Some(&(_, cmax)) = ranked.first() else {
                        continue;
                    };
                    if let Some(pos) = ranked.iter().position(|(n, _)| *n == *tool) {
                        let ratio = ranked[pos].1 / cmax;
                        if best_hit.as_ref().is_none_or(|(_, _, _, r)| ratio > *r) {
                            best_hit = Some((intent, clause.clone(), pos + 1, ratio));
                        }
                    }
                }
            }
            match best_hit {
                Some((intent, clause, rank, ratio)) => {
                    let ranked = rank_all_ml(
                        gbest_cfg.variant,
                        gbest_cfg.reverse_prefix,
                        gbest_cfg.min_len,
                        cbest,
                        &clause,
                    );
                    let leader = ranked
                        .first()
                        .map(|(n, s)| format!("{n} ({s:.2})"))
                        .unwrap_or_default();
                    println!(
                        "  {tool}: intent {intent:?}, clause {clause:?}, rank {rank}, ratio {ratio:.3} (needs >= {:.3}), clause rank 1: {leader}",
                        gbest_cfg.tau
                    );
                }
                None => println!("  {tool}: not found by ANY clause"),
            }
        }
    }
    println!("\n==== G-B. per-task union under G={} ====", gbest_lv.label);
    for task in &tasks {
        let (res, found) = score_task_f(gbest_cfg, cbest, task);
        println!(
            "-- {} : union {} tools, prec {:.1}%, recall {:.1}%",
            task.task,
            found.len(),
            res.precision * 100.0,
            res.recall * 100.0
        );
        println!("   union: {found:?}");
    }
    set_g(false, false, false);

    // --- C. D6 negative control under every lever, isolated and cumulated. -
    println!("\n==== G-C. D6 negative control ====");
    for lv in lever_sets {
        lv.apply();
        let c = lv.corpus(&corpus, &corpus_g4);
        for &variant in &[Variant::Current, Variant::Idf] {
            let cfg = FCfg {
                variant,
                reverse_prefix: true,
                min_len: REVERSE_PREFIX_MIN_LEN,
                f_on: true,
                tau: gbest_cfg.tau,
                per_clause_limit: gbest_cfg.per_clause_limit,
                limit: 8,
            };
            let hits = intent_hits_f(cfg, c, d6_query);
            let rank = hits
                .iter()
                .position(|n| *n == "batch_place_components")
                .map(|p| p + 1);
            println!(
                "  G={:<12} {} -> rank {} {}",
                lv.label,
                cfg.label(),
                rank.map(|r| r.to_string())
                    .unwrap_or_else(|| "absent".to_string()),
                if rank == Some(1) { "OK" } else { "LOST" }
            );
        }
    }
    set_g(false, false, false);

    // --- D. Priced cost of G2's false positives. ---------------------------
    println!("\n==== G-D. what component<->symbol costs, in union size ====");
    let d_cfg = FCfg {
        variant: gbest_cfg.variant,
        reverse_prefix: true,
        min_len: REVERSE_PREFIX_MIN_LEN,
        f_on: true,
        tau: gbest_cfg.tau,
        per_clause_limit: gbest_cfg.per_clause_limit,
        limit: 8,
    };
    let mut worst: Option<(String, Vec<&'static str>, Vec<&'static str>)> = None;
    let mut sum_before = 0.0f64;
    let mut sum_after = 0.0f64;
    for task in &tasks {
        set_g(false, false, false);
        let (_, before) = score_task_f(d_cfg, &corpus, task);
        set_g(false, true, false);
        let (_, after) = score_task_f(d_cfg, &corpus, task);
        set_g(false, false, false);
        let added: Vec<&'static str> = after
            .iter()
            .filter(|n| !before.contains(n))
            .copied()
            .collect();
        let dropped: Vec<&'static str> = before
            .iter()
            .filter(|n| !after.contains(n))
            .copied()
            .collect();
        sum_before += before.len() as f64;
        sum_after += after.len() as f64;
        println!(
            "-- {} : union {} -> {} (+{} -{})",
            task.task,
            before.len(),
            after.len(),
            added.len(),
            dropped.len()
        );
        if worst.as_ref().is_none_or(|(_, a, _)| added.len() > a.len()) {
            worst = Some((task.task.clone(), added, dropped));
        }
    }
    let n = tasks.len() as f64;
    println!(
        "  average union per task: {:.1} without G2 -> {:.1} with G2 ({:+.1} tools)",
        sum_before / n,
        sum_after / n,
        (sum_after - sum_before) / n
    );
    if let Some((task, added, dropped)) = worst {
        println!("  most affected task: {task}");
        println!("    entering: {added:?}");
        println!("    leaving:  {dropped:?}");
    }

    // --- E. The frontier, when the target is not reached. -------------------
    println!("\n==== G-E. frontier: best all7 recall in the grid ====");
    let max_recall = ggrid
        .iter()
        .map(|(_, _, _, a)| a.recall)
        .fold(0.0f64, f64::max);
    let mut at_max: Vec<&(GLevers, FCfg, PerimeterAgg, PerimeterAgg)> = ggrid
        .iter()
        .filter(|(_, _, _, a)| a.recall >= max_recall - 1e-9)
        .collect();
    at_max.sort_by(|x, y| y.3.precision.partial_cmp(&x.3.precision).unwrap());
    println!("  best all7 recall reachable: {:.1}%", max_recall * 100.0);
    for (lv, cfg, _, a) in at_max.iter().take(6) {
        println!(
            "  G={:<12} {} | all7: prec {:5.1}% recall {:5.1}% union {:4.1} failing {:?}",
            lv.label,
            cfg.label(),
            a.precision * 100.0,
            a.recall * 100.0,
            a.avg_union,
            a.failing
        );
    }
    set_g(false, false, false);

    // =======================================================================
    // Axis H (F.5.6): clause budget. Fixed base - variant=Idf, D=on, F=on,
    // G = G2+G3+G4 (G1 is inert and left out), limit=8.
    // `sanity_check` ran at the top of main with every lever off.
    // =======================================================================
    let h_levers = GLevers {
        label: "G2+G3+G4",
        g1: false,
        g2: true,
        g3: true,
        g4: true,
    };
    h_levers.apply();
    let ch: &Corpus = h_levers.corpus(&corpus, &corpus_g4);
    assert_single_clause_identity(ch, &tasks, &[0.60, 0.65, 0.70]);

    // The axis H plumbing must reproduce axis F exactly under the base policy,
    // on every intent, or the grid below is not comparable to the F.5.5 one.
    for task in &tasks {
        for intent in &task.intents {
            for &tau in &[0.60f64, 0.65, 0.70] {
                for &pcl in &[3usize, 4] {
                    let f = intent_hits_f(
                        FCfg {
                            variant: Variant::Idf,
                            reverse_prefix: true,
                            min_len: REVERSE_PREFIX_MIN_LEN,
                            f_on: true,
                            tau,
                            per_clause_limit: pcl,
                            limit: 8,
                        },
                        ch,
                        intent,
                    );
                    let h = intent_hits_h(
                        HCfg {
                            variant: Variant::Idf,
                            reverse_prefix: true,
                            min_len: REVERSE_PREFIX_MIN_LEN,
                            limit: 8,
                            cut: HCut::TauFixed { tau, pcl },
                        },
                        ch,
                        intent,
                    );
                    assert_eq!(
                        f, h,
                        "axis H base policy diverges from axis F on {intent:?}"
                    );
                }
            }
        }
    }
    println!("axis H invariant: base policy reproduces axis F on every intent");

    let mut cuts: Vec<HCut> = vec![
        HCut::TauFixed { tau: 0.65, pcl: 4 },
        HCut::TauFixed { tau: 0.65, pcl: 3 },
    ];
    for &gamma in &[0.5f64, 0.6, 0.7, 0.8] {
        for &floor in &[1usize, 2, 3] {
            cuts.push(HCut::Decrochage { gamma, floor });
        }
    }
    for &tau in &[0.60f64, 0.65, 0.70] {
        for &floor in &[1usize, 2] {
            cuts.push(HCut::Budget { tau, floor });
        }
    }
    for &tau in &[0.60f64, 0.65, 0.70] {
        for &gamma in &[0.6f64, 0.7, 0.8] {
            for &floor in &[1usize, 2] {
                cuts.push(HCut::TauAndDecrochage { tau, gamma, floor });
            }
        }
    }
    for &tau in &[0.60f64, 0.65, 0.70] {
        for &tau_short in &[0.70f64, 0.80, 0.90] {
            for &pcl in &[3usize, 4] {
                cuts.push(HCut::ShortTau {
                    tau,
                    tau_short,
                    pcl,
                });
            }
        }
    }

    println!("\n==== axis H grid (idf, D on, F on, G=G2+G3+G4, limit=8) ====");
    let mut hgrid: Vec<(HCfg, PerimeterAgg, PerimeterAgg)> = Vec::new();
    for cut in cuts {
        let cfg = HCfg {
            variant: Variant::Idf,
            reverse_prefix: true,
            min_len: REVERSE_PREFIX_MIN_LEN,
            limit: 8,
            cut,
        };
        let results: Vec<TaskResult> = tasks.iter().map(|t| score_task_h(cfg, ch, t).0).collect();
        let h = aggregate(&results, is_historical);
        let a = aggregate(&results, |_| true);
        println!(
            "{} | all7: prec {:5.1}% recall {:5.1}% union {:4.1} failing {:?} | hist6: prec {:5.1}% recall {:5.1}% union {:4.1}",
            cfg.label(),
            a.precision * 100.0,
            a.recall * 100.0,
            a.avg_union,
            a.failing,
            h.precision * 100.0,
            h.recall * 100.0,
            h.avg_union
        );
        hgrid.push((cfg, h, a));
    }

    // --- H-A. recall all7 >= 98 %, sorted by all7 precision. ---------------
    println!("\n==== H-A. recall >= 98% on all7, sorted by all7 precision ====");
    let mut hpass: Vec<&(HCfg, PerimeterAgg, PerimeterAgg)> =
        hgrid.iter().filter(|(_, _, a)| a.recall >= 0.98).collect();
    hpass.sort_by(|x, y| y.2.precision.partial_cmp(&x.2.precision).unwrap());
    for (cfg, h, a) in &hpass {
        println!(
            "{} | all7: prec {:5.1}% recall {:5.1}% union {:4.1} | hist6: prec {:5.1}% recall {:5.1}% union {:4.1}",
            cfg.label(),
            a.precision * 100.0,
            a.recall * 100.0,
            a.avg_union,
            h.precision * 100.0,
            h.recall * 100.0,
            h.avg_union
        );
    }
    if hpass.is_empty() {
        println!("  (none)");
    }
    match hpass.first() {
        Some((cfg, _, a)) if a.precision >= 0.60 => println!(
            "  VERDICT: target MET - {} gives all7 prec {:.1}% at recall {:.1}%",
            cfg.label(),
            a.precision * 100.0,
            a.recall * 100.0
        ),
        Some((cfg, _, a)) => println!(
            "  VERDICT: target MISSED - best all7 precision at recall >= 98% is {:.1}% ({})",
            a.precision * 100.0,
            cfg.label()
        ),
        None => println!("  VERDICT: target MISSED - no H policy holds all7 recall >= 98%"),
    }

    let hbest = hpass.first().copied().unwrap_or_else(|| {
        hgrid
            .iter()
            .max_by(|x, y| {
                let sx = x.2.recall * 10.0 + x.2.precision;
                let sy = y.2.recall * 10.0 + y.2.precision;
                sx.partial_cmp(&sy).unwrap()
            })
            .expect("grid is never empty")
    });
    let hbest_cfg = hbest.0;
    println!("\nbest axis-H policy: {}", hbest_cfg.label());

    // --- H-B. Per-task union and leftovers under the best policy. ----------
    println!(
        "\n==== H-B. per-task union under {} ====",
        hbest_cfg.label()
    );
    for task in &tasks {
        let (res, found) = score_task_h(hbest_cfg, ch, task);
        let missing: Vec<&str> = task
            .needed
            .iter()
            .filter(|n| !found.contains(&n.as_str()))
            .map(|s| s.as_str())
            .collect();
        println!(
            "-- {} : union {} tools, prec {:.1}%, recall {:.1}%, missing {missing:?}",
            task.task,
            found.len(),
            res.precision * 100.0,
            res.recall * 100.0
        );
        println!("   union: {found:?}");
    }

    // --- H-C. D6 control under the best policy of each family. -------------
    println!("\n==== H-C. D6 negative control, best policy of each H family ====");
    for family in ["base", "H1", "H2", "H3", "H4"] {
        let best_of = hgrid
            .iter()
            .filter(|(cfg, _, a)| cfg.cut.family() == family && a.recall >= 0.98)
            .max_by(|x, y| x.2.precision.partial_cmp(&y.2.precision).unwrap())
            .or_else(|| {
                hgrid
                    .iter()
                    .filter(|(cfg, _, _)| cfg.cut.family() == family)
                    .max_by(|x, y| {
                        let sx = x.2.recall * 10.0 + x.2.precision;
                        let sy = y.2.recall * 10.0 + y.2.precision;
                        sx.partial_cmp(&sy).unwrap()
                    })
            });
        let Some((cfg, _, a)) = best_of else { continue };
        let hits = intent_hits_h(*cfg, ch, d6_query);
        let rank = hits
            .iter()
            .position(|n| *n == "batch_place_components")
            .map(|p| p + 1);
        println!(
            "  {} (all7 prec {:5.1}% recall {:5.1}%) -> batch_place_components rank {} {}",
            cfg.label(),
            a.precision * 100.0,
            a.recall * 100.0,
            rank.map(|r| r.to_string())
                .unwrap_or_else(|| "absent".to_string()),
            if rank == Some(1) { "OK" } else { "LOST" }
        );
    }

    // --- H-D. What the best policy removed, and whether it was noise. ------
    println!(
        "\n==== H-D. tipping point: {} vs base tau=0.65 pcl=4 ====",
        hbest_cfg.label()
    );
    let base_cfg = HCfg {
        variant: Variant::Idf,
        reverse_prefix: true,
        min_len: REVERSE_PREFIX_MIN_LEN,
        limit: 8,
        cut: HCut::TauFixed { tau: 0.65, pcl: 4 },
    };
    for task in &tasks {
        let (_, base_found) = score_task_h(base_cfg, ch, task);
        let (_, best_found) = score_task_h(hbest_cfg, ch, task);
        let needed: Vec<&str> = task.needed.iter().map(|s| s.as_str()).collect();
        let out_noise: Vec<&str> = base_found
            .iter()
            .filter(|n| !best_found.contains(n) && !needed.contains(&(**n)))
            .copied()
            .collect();
        let out_needed: Vec<&str> = base_found
            .iter()
            .filter(|n| !best_found.contains(n) && needed.contains(&(**n)))
            .copied()
            .collect();
        let entered: Vec<&str> = best_found
            .iter()
            .filter(|n| !base_found.contains(n))
            .copied()
            .collect();
        println!(
            "-- {} : union {} -> {} | dropped {} false positives, {} true positives, {} entered",
            task.task,
            base_found.len(),
            best_found.len(),
            out_noise.len(),
            out_needed.len(),
            entered.len()
        );
        if task.task.starts_with("01_") || task.task.starts_with("05_") {
            println!("   false positives removed: {out_noise:?}");
            println!("   true positives lost:     {out_needed:?}");
            println!("   entered:                 {entered:?}");
        }
    }

    set_g(false, false, false);

    // =======================================================================
    // Axis I (F.5.7): tool families. Base - idf, D on, F on, G=G2+G3+G4,
    // tau=0.65, limit=8. sanity_check ran at the top of main, levers off.
    // =======================================================================
    h_levers.apply();
    assert_single_clause_identity(ch, &tasks, &[0.65]);
    // Axis I with the cap off must reproduce axis F exactly, on every intent.
    for task in &tasks {
        for intent in &task.intents {
            for &pcl in &[3usize, 4] {
                let f = intent_hits_f(
                    FCfg {
                        variant: Variant::Idf,
                        reverse_prefix: true,
                        min_len: REVERSE_PREFIX_MIN_LEN,
                        f_on: true,
                        tau: 0.65,
                        per_clause_limit: pcl,
                        limit: 8,
                    },
                    ch,
                    intent,
                );
                let i = intent_hits_i(
                    ICfg {
                        variant: Variant::Idf,
                        reverse_prefix: true,
                        min_len: REVERSE_PREFIX_MIN_LEN,
                        tau: 0.65,
                        per_clause_limit: pcl,
                        limit: 8,
                        per_family: None,
                    },
                    ch,
                    intent,
                );
                assert_eq!(
                    f, i,
                    "axis I with cap off diverges from axis F on {intent:?}"
                );
            }
        }
    }
    println!("axis I invariant: cap off reproduces axis F on every intent");

    // C. What the normalization groups, checked before it is trusted.
    println!("\n==== I-C. family normalization ====");
    for probe_name in [
        "place_component",
        "place_component_array",
        "batch_place_components",
        "export_netlist",
        "export_netlist_summary",
        "generate_netlist",
        "add_schematic_component",
    ] {
        println!("  {probe_name:<26} -> family {}", family_of(probe_name));
    }
    let mut fams: BTreeMap<String, Vec<&'static str>> = BTreeMap::new();
    for (_, def) in &ch.tools {
        fams.entry(family_of(def.name)).or_default().push(def.name);
    }
    let big: Vec<(&String, &Vec<&'static str>)> =
        fams.iter().filter(|(_, v)| v.len() >= 3).collect();
    println!(
        "  families with 3+ members ({} of {}):",
        big.len(),
        fams.len()
    );
    for (fam, members) in &big {
        println!("    {fam:<28} {members:?}");
    }
    let pair = fams.iter().find(|(_, v)| {
        v.contains(&"add_schematic_component") && v.contains(&"batch_place_components")
    });
    println!(
        "  add_schematic_component and batch_place_components in the same family: {}",
        if pair.is_some() {
            "YES - K=1 would lose one on 06_recovery"
        } else {
            "no"
        }
    );

    // A. Grid.
    println!("\n==== axis I grid (idf, D on, F on, G=G2+G3+G4, tau=0.65) ====");
    let mut igrid: Vec<(ICfg, PerimeterAgg, PerimeterAgg)> = Vec::new();
    for &pcl in &[3usize, 4] {
        for per_family in [None, Some(2usize), Some(1usize)] {
            let cfg = ICfg {
                variant: Variant::Idf,
                reverse_prefix: true,
                min_len: REVERSE_PREFIX_MIN_LEN,
                tau: 0.65,
                per_clause_limit: pcl,
                limit: 8,
                per_family,
            };
            let results: Vec<TaskResult> =
                tasks.iter().map(|t| score_task_i(cfg, ch, t).0).collect();
            let h = aggregate(&results, is_historical);
            let a = aggregate(&results, |_| true);
            println!(
                "{} | all7: prec {:5.1}% recall {:5.1}% union {:4.1} failing {:?} | hist6: prec {:5.1}% recall {:5.1}% union {:4.1}",
                cfg.label(),
                a.precision * 100.0,
                a.recall * 100.0,
                a.avg_union,
                a.failing,
                h.precision * 100.0,
                h.recall * 100.0,
                h.avg_union
            );
            igrid.push((cfg, h, a));
        }
    }

    println!("\n==== I-A. recall >= 98% on all7, sorted by all7 precision ====");
    let mut ipass: Vec<&(ICfg, PerimeterAgg, PerimeterAgg)> =
        igrid.iter().filter(|(_, _, a)| a.recall >= 0.98).collect();
    ipass.sort_by(|x, y| y.2.precision.partial_cmp(&x.2.precision).unwrap());
    for (cfg, h, a) in &ipass {
        println!(
            "{} | all7: prec {:5.1}% recall {:5.1}% union {:4.1} | hist6: prec {:5.1}% recall {:5.1}% union {:4.1}",
            cfg.label(),
            a.precision * 100.0,
            a.recall * 100.0,
            a.avg_union,
            h.precision * 100.0,
            h.recall * 100.0,
            h.avg_union
        );
    }
    if ipass.is_empty() {
        println!("  (none)");
    }
    match ipass.first() {
        Some((cfg, _, a)) if a.precision >= 0.60 => println!(
            "  VERDICT: target MET - {} gives all7 prec {:.1}% at recall {:.1}%",
            cfg.label(),
            a.precision * 100.0,
            a.recall * 100.0
        ),
        Some((cfg, _, a)) => println!(
            "  VERDICT: target MISSED - best all7 precision at recall >= 98% is {:.1}% ({}), gap to 60% = {:.1} pts",
            a.precision * 100.0,
            cfg.label(),
            60.0 - a.precision * 100.0
        ),
        None => println!("  VERDICT: target MISSED - no axis I setting holds all7 recall >= 98%"),
    }

    let ibest = ipass.first().copied().unwrap_or_else(|| {
        igrid
            .iter()
            .max_by(|x, y| {
                let sx = x.2.recall * 10.0 + x.2.precision;
                let sy = y.2.recall * 10.0 + y.2.precision;
                sx.partial_cmp(&sy).unwrap()
            })
            .expect("grid is never empty")
    });
    let ibest_cfg = ibest.0;
    println!("\nbest axis-I setting: {}", ibest_cfg.label());

    // B. Unions, and the detail on 05.
    println!(
        "\n==== I-B. per-task union under {} ====",
        ibest_cfg.label()
    );
    let ref_cfg = ICfg {
        per_family: None,
        ..ibest_cfg
    };
    for task in &tasks {
        let (res, found) = score_task_i(ibest_cfg, ch, task);
        let missing: Vec<&str> = task
            .needed
            .iter()
            .filter(|n| !found.contains(&n.as_str()))
            .map(|s| s.as_str())
            .collect();
        println!(
            "-- {} : union {} tools, prec {:.1}%, recall {:.1}%, missing {missing:?}",
            task.task,
            found.len(),
            res.precision * 100.0,
            res.recall * 100.0
        );
        println!("   union: {found:?}");
    }
    println!("\n==== I-B. what the family cap removes, per task ====");
    for task in &tasks {
        let (_, before) = score_task_i(ref_cfg, ch, task);
        let (_, after) = score_task_i(ibest_cfg, ch, task);
        let needed: Vec<&str> = task.needed.iter().map(|s| s.as_str()).collect();
        let removed: Vec<String> = before
            .iter()
            .filter(|n| !after.contains(n))
            .map(|n| {
                format!(
                    "{n} [{}] (family {})",
                    if needed.contains(n) {
                        "NEEDED"
                    } else {
                        "noise"
                    },
                    family_of(n)
                )
            })
            .collect();
        let entered: Vec<&str> = after
            .iter()
            .filter(|n| !before.contains(n))
            .copied()
            .collect();
        println!(
            "-- {} : union {} -> {}, entered {entered:?}",
            task.task,
            before.len(),
            after.len()
        );
        for r in &removed {
            println!("     removed {r}");
        }
    }

    // D. D6 control under every axis I setting.
    println!("\n==== I-D. D6 negative control ====");
    for (cfg, _, _) in &igrid {
        let hits = intent_hits_i(*cfg, ch, d6_query);
        let rank = hits
            .iter()
            .position(|n| *n == "batch_place_components")
            .map(|p| p + 1);
        println!(
            "  {} -> batch_place_components rank {} {}",
            cfg.label(),
            rank.map(|r| r.to_string())
                .unwrap_or_else(|| "absent".to_string()),
            if rank == Some(1) { "OK" } else { "LOST" }
        );
    }

    set_g(false, false, false);
}

// ---------------------------------------------------------------------------
// Axis F: clause splitting. A composite intent ("export bom and netlist and
// schematic svg") is dominated lexically by a single tool, so a relative
// cutoff strangles the other clauses' tools. Splitting the query on
// connectors, cutting each clause on its *own* best score, then merging by
// ratio-to-own-clause-best keeps every clause's leader.
// ---------------------------------------------------------------------------

const CLAUSE_SEPARATORS: [&str; 5] = [" then ", " and ", " & ", ";", ","];

/// Split an intent into clauses. Clauses whose `query_terms` are empty (pure
/// stop words) are dropped; if nothing survives, the whole query is the single
/// clause, so the caller always gets at least one usable clause.
fn split_clauses(query: &str) -> Vec<String> {
    let mut work = query.to_ascii_lowercase();
    for sep in CLAUSE_SEPARATORS {
        work = work.replace(sep, "\u{1}");
    }
    let clauses: Vec<String> = work
        .split('\u{1}')
        .map(|c| c.trim().to_string())
        .filter(|c| !query_terms(c).is_empty())
        .collect();
    if clauses.is_empty() {
        vec![query.trim().to_string()]
    } else {
        clauses
    }
}

/// One point of the axis-F grid.
#[derive(Clone, Copy)]
struct FCfg {
    variant: Variant,
    reverse_prefix: bool,
    min_len: usize,
    f_on: bool,
    tau: f64,
    per_clause_limit: usize,
    limit: usize,
}

impl FCfg {
    fn label(&self) -> String {
        format!(
            "{:<7} F={:<3} tau={:.2} pcl={:<2} limit={}",
            self.variant.label(),
            if self.f_on { "on" } else { "off" },
            self.tau,
            if self.f_on {
                self.per_clause_limit.to_string()
            } else {
                "-".to_string()
            },
            self.limit
        )
    }
}

/// Hits returned for a single intent under `cfg`. With F off this is exactly
/// `cutoff_then_limit` on the whole query; with F on, the per-clause cutoffs
/// are merged by ratio-to-own-clause-best (descending), name ascending as the
/// tie-break, and truncated to the global limit.
fn intent_hits_f(cfg: FCfg, corpus: &Corpus, query: &str) -> Vec<&'static str> {
    if !cfg.f_on {
        let ranked = rank_all_ml(cfg.variant, cfg.reverse_prefix, cfg.min_len, corpus, query);
        return cutoff_then_limit(&ranked, cfg.tau, cfg.limit);
    }
    let mut merged: Vec<(&'static str, f64)> = Vec::new();
    for clause in split_clauses(query) {
        let ranked = rank_all_ml(
            cfg.variant,
            cfg.reverse_prefix,
            cfg.min_len,
            corpus,
            &clause,
        );
        let Some(&(_, cmax)) = ranked.first() else {
            continue;
        };
        for name in cutoff_then_limit(&ranked, cfg.tau, cfg.per_clause_limit) {
            let score = ranked
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, s)| *s)
                .unwrap_or(0.0);
            let ratio = score / cmax;
            match merged.iter_mut().find(|(n, _)| *n == name) {
                Some(entry) => {
                    if ratio > entry.1 {
                        entry.1 = ratio;
                    }
                }
                None => merged.push((name, ratio)),
            }
        }
    }
    merged.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap().then_with(|| a.0.cmp(b.0)));
    merged.into_iter().take(cfg.limit).map(|(n, _)| n).collect()
}

fn score_task_f(cfg: FCfg, corpus: &Corpus, task: &TaskIntents) -> (TaskResult, Vec<&'static str>) {
    let mut found: Vec<&'static str> = Vec::new();
    for intent in &task.intents {
        for name in intent_hits_f(cfg, corpus, intent) {
            if !found.contains(&name) {
                found.push(name);
            }
        }
    }
    let hits = task
        .needed
        .iter()
        .filter(|n| found.contains(&n.as_str()))
        .count();
    let precision = if found.is_empty() {
        0.0
    } else {
        hits as f64 / found.len() as f64
    };
    let recall = if task.needed.is_empty() {
        1.0
    } else {
        hits as f64 / task.needed.len() as f64
    };
    (
        TaskResult {
            task: task.task.clone(),
            found: found.len(),
            precision,
            recall,
        },
        found,
    )
}

/// Runtime invariant: on an intent that splits into a single clause, F=on with
/// `per_clause_limit == limit` must reproduce F=off byte for byte. A violation
/// means the merge order or the clause filter changed non-composite behaviour,
/// which would make the whole grid uninterpretable — so it panics.
fn assert_single_clause_identity(corpus: &Corpus, tasks: &[TaskIntents], taus: &[f64]) {
    let mut checked = 0usize;
    for task in tasks {
        for intent in &task.intents {
            if split_clauses(intent).len() != 1 {
                continue;
            }
            for &variant in &[Variant::Current, Variant::Idf] {
                for &tau in taus {
                    let base = FCfg {
                        variant,
                        reverse_prefix: true,
                        min_len: REVERSE_PREFIX_MIN_LEN,
                        f_on: false,
                        tau,
                        per_clause_limit: 8,
                        limit: 8,
                    };
                    let off = intent_hits_f(base, corpus, intent);
                    let on = intent_hits_f(FCfg { f_on: true, ..base }, corpus, intent);
                    assert_eq!(
                        off,
                        on,
                        "axis F broke the single-clause identity on {intent:?} \
                         (variant={}, tau={tau:.2})",
                        variant.label()
                    );
                    checked += 1;
                }
            }
        }
    }
    println!(
        "axis F invariant: F=on == F=off on every single-clause intent ({checked} comparisons)"
    );
}

// ---------------------------------------------------------------------------
// Axis G plumbing: one lever set = the three global switches plus the choice
// of corpus (G4 rewrites descriptions, so it is a different corpus, not a
// switch inside the scorer).
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct GLevers {
    label: &'static str,
    g1: bool,
    g2: bool,
    g3: bool,
    g4: bool,
}

impl GLevers {
    const SETS: [GLevers; 8] = [
        GLevers {
            label: "none",
            g1: false,
            g2: false,
            g3: false,
            g4: false,
        },
        GLevers {
            label: "G1",
            g1: true,
            g2: false,
            g3: false,
            g4: false,
        },
        GLevers {
            label: "G2",
            g1: false,
            g2: true,
            g3: false,
            g4: false,
        },
        GLevers {
            label: "G3",
            g1: false,
            g2: false,
            g3: true,
            g4: false,
        },
        GLevers {
            label: "G4",
            g1: false,
            g2: false,
            g3: false,
            g4: true,
        },
        GLevers {
            label: "G1+G2",
            g1: true,
            g2: true,
            g3: false,
            g4: false,
        },
        GLevers {
            label: "G1+G2+G3",
            g1: true,
            g2: true,
            g3: true,
            g4: false,
        },
        GLevers {
            label: "G1+G2+G3+G4",
            g1: true,
            g2: true,
            g3: true,
            g4: true,
        },
    ];

    fn apply(&self) {
        set_g(self.g1, self.g2, self.g3);
    }

    fn corpus<'a>(&self, plain: &'a Corpus, g4: &'a Corpus) -> &'a Corpus {
        if self.g4 {
            g4
        } else {
            plain
        }
    }
}

/// Document frequency of a term over the corpus (name + description), so an
/// inert synonym is visible as df=0.
fn corpus_df(corpus: &Corpus, term: &str) -> usize {
    corpus
        .tools
        .iter()
        .filter(|(_, def)| {
            terms(def.name).iter().any(|t| t == term)
                || terms(def.description).iter().any(|t| t == term)
        })
        .count()
}

// ---------------------------------------------------------------------------
// Axis H (F.5.6): how many results a clause deserves. Axis F gave every
// clause the same `per_clause_limit`, so a one-word clause with a clear
// winner still spent 3-4 slots on near-ties. These policies decide the size
// of a clause's contribution from the clause's own score profile.
// ---------------------------------------------------------------------------

/// `decrochage_keep_count` with a parametrable floor (the hard-coded 3 is
/// exactly what makes a decided clause expensive).
fn decrochage_keep_floor(ranked: &[(&'static str, f64)], gamma: f64, floor: usize) -> usize {
    let n = ranked.len();
    if n == 0 {
        return 0;
    }
    let mut cut = n;
    for i in 0..n.saturating_sub(1) {
        if ranked[i + 1].1 < gamma * ranked[i].1 {
            cut = i + 1;
            break;
        }
    }
    cut.max(floor.min(n))
}

#[derive(Clone, Copy)]
enum HCut {
    /// Axis F baseline: fixed relative cutoff, fixed per-clause limit.
    TauFixed { tau: f64, pcl: usize },
    /// H1: per-clause drop detection with a parametrable floor.
    Decrochage { gamma: f64, floor: usize },
    /// H2: global budget split across the clauses of the query.
    Budget { tau: f64, floor: usize },
    /// H3: a hit must clear the ratio *and* sit before the drop.
    TauAndDecrochage { tau: f64, gamma: f64, floor: usize },
    /// H4: a stricter ratio for one-term clauses, which are ambiguous by
    /// construction.
    ShortTau {
        tau: f64,
        tau_short: f64,
        pcl: usize,
    },
}

impl HCut {
    fn family(self) -> &'static str {
        match self {
            HCut::TauFixed { .. } => "base",
            HCut::Decrochage { .. } => "H1",
            HCut::Budget { .. } => "H2",
            HCut::TauAndDecrochage { .. } => "H3",
            HCut::ShortTau { .. } => "H4",
        }
    }

    fn label(self) -> String {
        match self {
            HCut::TauFixed { tau, pcl } => format!("base   tau={tau:.2} pcl={pcl}"),
            HCut::Decrochage { gamma, floor } => format!("H1     gamma={gamma:.1} floor={floor}"),
            HCut::Budget { tau, floor } => format!("H2     tau={tau:.2} floor={floor}"),
            HCut::TauAndDecrochage { tau, gamma, floor } => {
                format!("H3     tau={tau:.2} gamma={gamma:.1} floor={floor}")
            }
            HCut::ShortTau {
                tau,
                tau_short,
                pcl,
            } => format!("H4     tau={tau:.2} tau1={tau_short:.2} pcl={pcl}"),
        }
    }
}

#[derive(Clone, Copy)]
struct HCfg {
    variant: Variant,
    reverse_prefix: bool,
    min_len: usize,
    limit: usize,
    cut: HCut,
}

impl HCfg {
    fn label(&self) -> String {
        format!("{:<34} limit={}", self.cut.label(), self.limit)
    }
}

/// What one clause keeps, under `cut`. `n_clauses` is the number of clauses
/// the whole query split into (H2 divides the budget by it), `n_terms` the
/// number of scoring terms of this clause (H4 hardens on 1).
fn clause_keep(
    cut: HCut,
    ranked: &[(&'static str, f64)],
    n_clauses: usize,
    n_terms: usize,
    limit: usize,
) -> Vec<&'static str> {
    match cut {
        HCut::TauFixed { tau, pcl } => cutoff_then_limit(ranked, tau, pcl),
        HCut::Decrochage { gamma, floor } => {
            let keep = decrochage_keep_floor(ranked, gamma, floor).min(limit);
            ranked.iter().take(keep).map(|(n, _)| *n).collect()
        }
        HCut::Budget { tau, floor } => {
            let pcl = floor.max(limit / n_clauses.max(1));
            cutoff_then_limit(ranked, tau, pcl)
        }
        HCut::TauAndDecrochage { tau, gamma, floor } => {
            let keep = decrochage_keep_floor(ranked, gamma, floor).min(ranked.len());
            cutoff_then_limit(&ranked[..keep], tau, limit)
        }
        HCut::ShortTau {
            tau,
            tau_short,
            pcl,
        } => {
            let t = if n_terms <= 1 { tau_short } else { tau };
            cutoff_then_limit(ranked, t, pcl)
        }
    }
}

/// Axis-H counterpart of `intent_hits_f`: clause split, per-clause policy,
/// then the same ratio-to-own-clause-best merge truncated to the global limit.
fn intent_hits_h(cfg: HCfg, corpus: &Corpus, query: &str) -> Vec<&'static str> {
    let clauses = split_clauses(query);
    let n_clauses = clauses.len();
    let mut merged: Vec<(&'static str, f64)> = Vec::new();
    for clause in &clauses {
        let ranked = rank_all_ml(cfg.variant, cfg.reverse_prefix, cfg.min_len, corpus, clause);
        let Some(&(_, cmax)) = ranked.first() else {
            continue;
        };
        let n_terms = query_terms(clause).len();
        for name in clause_keep(cfg.cut, &ranked, n_clauses, n_terms, cfg.limit) {
            let score = ranked
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, s)| *s)
                .unwrap_or(0.0);
            let ratio = score / cmax;
            match merged.iter_mut().find(|(n, _)| *n == name) {
                Some(entry) => {
                    if ratio > entry.1 {
                        entry.1 = ratio;
                    }
                }
                None => merged.push((name, ratio)),
            }
        }
    }
    merged.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap().then_with(|| a.0.cmp(b.0)));
    merged.into_iter().take(cfg.limit).map(|(n, _)| n).collect()
}

fn score_task_h(cfg: HCfg, corpus: &Corpus, task: &TaskIntents) -> (TaskResult, Vec<&'static str>) {
    let mut found: Vec<&'static str> = Vec::new();
    for intent in &task.intents {
        for name in intent_hits_h(cfg, corpus, intent) {
            if !found.contains(&name) {
                found.push(name);
            }
        }
    }
    let hits = task
        .needed
        .iter()
        .filter(|n| found.contains(&n.as_str()))
        .count();
    let precision = if found.is_empty() {
        0.0
    } else {
        hits as f64 / found.len() as f64
    };
    let recall = if task.needed.is_empty() {
        1.0
    } else {
        hits as f64 / task.needed.len() as f64
    };
    (
        TaskResult {
            task: task.task.clone(),
            found: found.len(),
            precision,
            recall,
        },
        found,
    )
}

// ---------------------------------------------------------------------------
// Axis I (F.5.7): tool families. Serving place_component,
// place_component_array and batch_place_components in the same answer is
// noise for an LLM even when all three score well. A family is the tool name
// normalized: name terms, singularized, minus the modifier words, sorted.
// ---------------------------------------------------------------------------

const FAMILY_MODIFIERS: [&str; 6] = ["batch", "array", "summary", "all", "single", "multi"];

fn singularize(term: &str) -> String {
    match term.strip_suffix('s') {
        Some(stem) if stem.len() >= 3 && !term.ends_with("ss") => stem.to_string(),
        _ => term.to_string(),
    }
}

fn family_of(name: &str) -> String {
    let mut parts: Vec<String> = terms(name)
        .iter()
        .map(|t| singularize(t))
        .filter(|t| !FAMILY_MODIFIERS.contains(&t.as_str()))
        .collect();
    parts.sort();
    parts.dedup();
    if parts.is_empty() {
        name.to_string()
    } else {
        parts.join("_")
    }
}

#[derive(Clone, Copy)]
struct ICfg {
    variant: Variant,
    reverse_prefix: bool,
    min_len: usize,
    tau: f64,
    per_clause_limit: usize,
    limit: usize,
    /// `None` = axis I off (reference line), `Some(k)` = at most k tools per
    /// family per query.
    per_family: Option<usize>,
}

impl ICfg {
    fn label(&self) -> String {
        format!(
            "tau={:.2} pcl={} K={:<4} limit={}",
            self.tau,
            self.per_clause_limit,
            self.per_family
                .map(|k| k.to_string())
                .unwrap_or_else(|| "off".to_string()),
            self.limit
        )
    }
}

/// Clause merge identical to `intent_hits_f`, then the per-family cap, then
/// the global truncation — in that order, as specified.
fn intent_hits_i(cfg: ICfg, corpus: &Corpus, query: &str) -> Vec<&'static str> {
    let mut merged: Vec<(&'static str, f64)> = Vec::new();
    for clause in split_clauses(query) {
        let ranked = rank_all_ml(
            cfg.variant,
            cfg.reverse_prefix,
            cfg.min_len,
            corpus,
            &clause,
        );
        let Some(&(_, cmax)) = ranked.first() else {
            continue;
        };
        for name in cutoff_then_limit(&ranked, cfg.tau, cfg.per_clause_limit) {
            let score = ranked
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, s)| *s)
                .unwrap_or(0.0);
            let ratio = score / cmax;
            match merged.iter_mut().find(|(n, _)| *n == name) {
                Some(entry) => {
                    if ratio > entry.1 {
                        entry.1 = ratio;
                    }
                }
                None => merged.push((name, ratio)),
            }
        }
    }
    merged.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap().then_with(|| a.0.cmp(b.0)));

    let mut kept: Vec<&'static str> = Vec::new();
    let mut per_family: Vec<(String, usize)> = Vec::new();
    for (name, _) in merged {
        if let Some(k) = cfg.per_family {
            let fam = family_of(name);
            let slot = match per_family.iter_mut().find(|(f, _)| *f == fam) {
                Some(slot) => slot,
                None => {
                    per_family.push((fam, 0));
                    per_family.last_mut().expect("just pushed")
                }
            };
            if slot.1 >= k {
                continue;
            }
            slot.1 += 1;
        }
        kept.push(name);
        if kept.len() == cfg.limit {
            break;
        }
    }
    kept
}

fn score_task_i(cfg: ICfg, corpus: &Corpus, task: &TaskIntents) -> (TaskResult, Vec<&'static str>) {
    let mut found: Vec<&'static str> = Vec::new();
    for intent in &task.intents {
        for name in intent_hits_i(cfg, corpus, intent) {
            if !found.contains(&name) {
                found.push(name);
            }
        }
    }
    let hits = task
        .needed
        .iter()
        .filter(|n| found.contains(&n.as_str()))
        .count();
    let precision = if found.is_empty() {
        0.0
    } else {
        hits as f64 / found.len() as f64
    };
    let recall = if task.needed.is_empty() {
        1.0
    } else {
        hits as f64 / task.needed.len() as f64
    };
    (
        TaskResult {
            task: task.task.clone(),
            found: found.len(),
            precision,
            recall,
        },
        found,
    )
}
