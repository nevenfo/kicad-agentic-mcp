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

fn expansions(term: &str) -> &'static [&'static str] {
    SYNONYMS
        .iter()
        .find(|(k, _)| *k == term)
        .map(|(_, v)| *v)
        .unwrap_or(&[])
}

fn query_terms(query: &str) -> Vec<String> {
    terms(query)
        .into_iter()
        .filter(|t| !is_stop_word(t))
        .collect()
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
        let tools = ToolRouter::new().all_tools_with_toolset();
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

fn sanity_check(corpus: &Corpus) {
    // Production `score_tool` now bakes in the reverse-prefix rule (axis D)
    // unconditionally, so the local reference has to run with D on to match
    // it — `rank_all(Variant::Current, false, ...)` would silently diverge.
    for query in [
        "add a resistor to the schematic",
        "run erc and list the components and nets",
        "export bom and netlist",
        "search reference circuit templates and instantiate one",
        "where are a component's pins",
    ] {
        let prod: Vec<(&str, u32)> = prod_search(&corpus.tools, query, corpus.tools.len())
            .into_iter()
            .map(|h| (h.name, h.score))
            .collect();
        let local: Vec<(&str, f64)> = rank_all(Variant::Current, true, corpus, query);
        assert_eq!(prod.len(), local.len(), "count mismatch for {query:?}");
        for ((pn, ps), (ln, ls)) in prod.iter().zip(local.iter()) {
            assert_eq!(pn, ln, "order mismatch for {query:?}");
            assert!(
                (*ps as f64 - *ls).abs() < 1e-9,
                "score mismatch for {query:?} on {pn}: prod={ps} local={ls}"
            );
        }
    }
    println!(
        "sanity check: local `current D=on` reimplementation matches production search() exactly"
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
        let mut best: BTreeMap<&str, (usize, u32, &str)> = BTreeMap::new();
        for intent in &task.intents {
            println!("-- intent: {intent:?}");
            let hits = prod_search(&corpus.tools, intent, PROBE_LIMIT);
            for (i, hit) in hits.iter().enumerate() {
                let rank = i + 1;
                let needed = task.needed.iter().any(|n| n == hit.name);
                println!(
                    "  {rank:>2} {:>3} {}{}",
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
}
