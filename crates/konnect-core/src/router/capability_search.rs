//! Deterministic capability search over the tool registry.
//!
//! The point is to stop paying for a whole toolset when a task needs six tools
//! out of it. On the golden benchmark suite, `load_toolset` costs ~8 400 tokens
//! of `tools/list` refresh per task to expose ~90 tools, of which ~12 are ever
//! called. Search lets the caller name the twelve.
//!
//! No embeddings and no model call: this runs on every task, must be
//! reproducible across runs and machines, and must never be the reason a task
//! is slow. Scoring is plain lexical matching over the tool name and its
//! description, with a small synonym table for EDA vocabulary that does not
//! appear verbatim in tool names ("cap" for capacitor, "netlist" for export).
//!
//! Quality is a benchmark question, not an opinion — `bench/` measures hit rate
//! against the golden tasks, and the synonym table only grows in response to a
//! measured miss.

use crate::tools::ToolDef;
use std::collections::HashMap;

/// One search hit. `summary` is the first sentence of the tool description:
/// enough to choose between two candidates, far short of the full schema.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub name: &'static str,
    pub toolset: &'static str,
    pub summary: String,
    /// Raw lexical score of the hit in the clause that surfaced it. Not
    /// comparable between two clauses of the same query, and not exposed to
    /// callers; kept for diagnostics.
    pub score: f64,
}

/// Query words that should also match a different word in the corpus.
/// Left side is what people type, right side is what the registry says.
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
    // component <-> symbol, both ways: in KiCAD a schematic component *is* a
    // symbol instance, and the table already tied "cap", "part" and "ic" to
    // both without ever tying the two together.
    ("symbol", &["symbol", "library", "component"]),
    ("component", &["symbol"]),
    // Localization and designation vocabulary, added after
    // `get_schematic_pin_locations` was measured unreachable for "where are a
    // component's pins" (ratio 0.46 of the clause best). Only words that
    // actually occur in the registry are listed: "location" has a document
    // frequency of 0 and is deliberately absent.
    (
        "where",
        &["position", "positions", "locations", "coordinates"],
    ),
    ("reference", &["designator"]),
];

// Plural stemming was tried here and measured worse, so it is not in the code.
// Stripping a trailing "s" from both query and corpus terms looked obviously
// right — "symbols" should match `add_power_symbol` — but on the golden suite
// it moved retrieval recall at eight results per query from 100 % to 98.2 %
// (it stopped surfacing `batch_place_components` for "place multiple symbols
// on the schematic in one call") and changed nothing at lower limits. The
// prefix rule below already absorbs most plural mismatches. Do not re-add it
// without a benchmark run that shows a gain.
//
// The reverse-prefix branch in `score_tool` is a different rule, aimed at the
// same problem, that does not have this failure mode. Stemming cut the "s"
// off terms on both sides, which is symmetric and destructive: it can turn a
// non-match into a match, but it can just as well turn a match into a
// non-match, which is what happened to `batch_place_components`. Reverse
// prefix is asymmetric and purely additive — it never removes a point from
// anyone, it only adds a fallback +4/+1 when a query term is the longer,
// plural side of a singular corpus term at least three characters long
// ("templates" reaching `apply_template`, "pins" reaching
// `get_schematic_component`) and nothing stronger already matched.
//
// The floor was first set to four characters by policy, not measurement, and
// re-checked afterward: three-letter EDA terms ("pin", "net", "pad") are
// exactly the common case a floor of four excludes, and are almost always
// typed plural. Lowering the floor to three was measured, not assumed: swept
// against 3/4/5 on the full golden suite, only 3 additionally fires on
// "netlist"/"nets" -> `net` and "pins" -> `pin`, both defensible EDA
// vocabulary, no unrelated acronym collisions. Recall on the six historical
// tasks is unchanged at 100 % at every floor, and `batch_place_components` is
// still the rank-1 hit for "place multiple symbols on the schematic in one
// call" at every floor — the exact case stemming broke.
//
// The floor is not a complete fix. `get_schematic_pin_locations` still does
// not reach the top 8 for "where are a component's pins": the branch fires
// (`pin` is three letters, at the floor), but a lone +4 does not outscore the
// dozen other tools this vague, mostly-stop-word query also weakly matches.
// That is a scoring-ceiling problem, not a floor problem, and it is still
// open.
//
// F.5 closed that scoring ceiling, and the gap it names is fixed — but not by
// touching the floor. Four changes, each measured on the golden suite before
// being kept, and each visible in the constants below: IDF weighting so a term
// 130 descriptions share stops paying like a rare one; clause splitting so a
// composite intent is cut against each of its parts instead of against its
// loudest one; a relative cutoff per clause so a decided query stops padding
// its answer; and a one-per-family cap so three spellings of "place a
// component" do not spend three of the caller's eight slots. Two of the
// registry's own descriptions were rewritten in the same pass
// (`get_schematic_component`, `get_schematic_pin_locations`): they named the
// concept with only one of the two domain words, which is a documentation bug
// that no amount of scoring can work around.
//
// The result, measured by `bench/runner.py --load-mode search` on the seven
// golden tasks: retrieval precision 20.8 % -> 62 %, recall 97.1 % -> 100 %.
// Two things that looked obviously right and measured as nothing are recorded
// here so they are not retried: dropping one-character query terms (the "s" of
// "component's") changed no metric at all, and a per-clause "drop detection"
// cutoff instead of a fixed ratio was worse at every gamma tried.

fn terms(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_ascii_lowercase())
        .collect()
}

/// Words that carry no selection signal. Dropping them keeps a natural-language
/// query ("add a capacitor to the schematic") from scoring every tool that
/// mentions "the schematic" — which is nearly all of them.
const STOP_WORDS: &[&str] = &[
    "a", "an", "and", "at", "by", "for", "from", "in", "into", "is", "it", "of", "on", "or", "the",
    "to", "with", "my", "this", "that", "please", "kicad",
];

fn is_stop_word(term: &str) -> bool {
    STOP_WORDS.contains(&term)
}

/// Expand a query term with its synonyms. The term itself always scores full
/// weight; synonyms score at a discount.
fn expansions(term: &str) -> &'static [&'static str] {
    SYNONYMS
        .iter()
        .find(|(k, _)| *k == term)
        .map(|(_, v)| *v)
        .unwrap_or(&[])
}

// Minimum length of a corpus term for the reverse-prefix rule below, so it
// does not fire on noise like "a" or "add" starting a long query word. Swept
// against {3, 4, 5} on the golden suite (see the module comment above); 3 is
// the smallest floor that stays clean, and it is the one in use.
const REVERSE_PREFIX_MIN_LEN: usize = 3;

/// Inverse document frequency over the corpus, BM25-style.
///
/// Flat lexical weighting made every query term worth the same, so "schematic"
/// — which 130-odd descriptions mention — paid as much as "netlist". On the
/// golden suite, weighting each term by its IDF is what let a relative cutoff
/// be tight enough to matter: precision at eight results went from 20.8 % to
/// the 62 % measured below, at 100 % recall.
///
/// Built per call, on purpose. Measured on the 202-tool registry: 0.64 ms per
/// build in release (3.4 ms in a debug build), against 1.7 ms for a whole
/// `search()` call and milliseconds more of JSON in the `find_capabilities`
/// round trip that wraps it. A `OnceLock` would shave a fraction of a
/// millisecond off a call that is already not on any hot path, and would have
/// to grow an invalidation story the day the registry stops being static.
/// Re-measure before caching: if the registry ever passes a few thousand
/// tools, this stops being free.
struct Idf {
    table: HashMap<String, f64>,
}

impl Idf {
    fn build(corpus: &[(&'static str, ToolDef)]) -> Self {
        let n = corpus.len() as f64;
        let mut df: HashMap<String, usize> = HashMap::new();
        for (_, def) in corpus {
            let mut vocab: Vec<String> = terms(def.name);
            vocab.extend(terms(def.description));
            vocab.sort();
            vocab.dedup();
            for t in vocab {
                *df.entry(t).or_insert(0) += 1;
            }
        }
        let table = df
            .into_iter()
            .map(|(t, d)| {
                let d = d as f64;
                (t, ((n - d + 0.5) / (d + 0.5) + 1.0).ln().max(0.0))
            })
            .collect();
        Self { table }
    }

    fn of(&self, term: &str) -> f64 {
        self.table.get(term).copied().unwrap_or(0.0)
    }
}

fn score_tool(idf: &Idf, query_terms: &[String], def: &ToolDef) -> f64 {
    let name_terms = terms(def.name);
    let desc_terms = terms(def.description);
    let mut score = 0.0f64;

    for qt in query_terms {
        // Every contribution of this term — name, description, synonyms — is
        // weighted by how rare the term is in the registry.
        let w = idf.of(qt);

        // Exact hit on a word of the tool name is the strongest signal there is:
        // `run_erc` for "erc" must outrank every tool whose description merely
        // mentions ERC.
        let name_points = if name_terms.iter().any(|t| t == qt) {
            10.0
        } else if name_terms.iter().any(|t| t.starts_with(qt.as_str())) {
            5.0
        } else if def.name.contains(qt.as_str()) {
            3.0
        } else if name_terms
            .iter()
            .any(|t| t.len() >= REVERSE_PREFIX_MIN_LEN && qt.starts_with(t.as_str()))
        {
            // Reverse prefix, last resort: the branches above only match a
            // corpus term that is a prefix of (or equal to) the query term,
            // which can never fire when the query uses a plural of a corpus
            // singular — "pins" is longer than `pin`, so `t.starts_with(qt)`
            // never holds. This is the mirror check, and it only applies once
            // nothing stronger already scored.
            4.0
        } else {
            0.0
        };
        score += name_points * w;

        let desc_points = if desc_terms.iter().any(|t| t == qt) {
            2.0
        } else if desc_terms
            .iter()
            .any(|t| t.len() >= REVERSE_PREFIX_MIN_LEN && qt.starts_with(t.as_str()))
        {
            1.0
        } else {
            0.0
        };
        score += desc_points * w;

        for syn in expansions(qt) {
            if name_terms.iter().any(|t| t == syn) {
                score += 4.0 * w;
            } else if desc_terms.iter().any(|t| t == syn) {
                score += 1.0 * w;
            }
        }
    }

    score
}

/// Connectors that separate two things a caller is asking for at once. A
/// composite intent ("export bom and netlist and schematic svg") is dominated
/// lexically by whichever tool matches one of its parts best, so a cutoff
/// computed over the whole query silently drops the tools of every other part:
/// on the golden suite, `apply_template` scored 0.21 of the best hit for
/// "search reference circuit templates and instantiate one", where
/// `search_templates` took rank 1. Cutting per clause is what recovered them.
const CLAUSE_SEPARATORS: [&str; 5] = [" then ", " and ", " & ", ";", ","];

/// A hit is kept only if it scores at least this fraction of the best score of
/// its own clause. Swept over {0, 0.5, 0.6, 0.65, 0.7, 0.75, 0.8} on the golden
/// suite: 0.65 is the largest value that keeps recall at 100 %, and it is worth
/// ~14 points of precision over no cutoff at all.
const CLAUSE_SCORE_RATIO: f64 = 0.65;

/// Query terms of a clause, minus the words that carry no selection signal.
fn query_terms(query: &str) -> Vec<String> {
    terms(query)
        .into_iter()
        .filter(|t| !is_stop_word(t))
        .collect()
}

/// Split a query into clauses. A clause made only of stop words is dropped; if
/// that leaves nothing, the whole query is the single clause, so the caller
/// always gets the same behaviour it had before splitting existed.
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

/// How many hits one clause may contribute before the merge.
///
/// Measured at `limit = 8`, where 4 is the value that holds recall at 100 %
/// (3 loses `list_schematic_nets` on the divider task, 8 dilutes precision by
/// six points). The `limit / 2` generalization to other limits is an
/// extrapolation, not a measurement: `find_capabilities` accepts a limit from
/// 1 to 50 and only 8 has ever been on the benchmark.
fn per_clause_limit(limit: usize) -> usize {
    (limit / 2).max(2)
}

/// Modifier words that distinguish two spellings of the same capability rather
/// than two capabilities. Derived from the registry, not from taste:
/// `place_component` / `place_component_array` / `batch_place_components` is
/// the only three-member family in the whole corpus, and
/// `export_netlist` / `export_netlist_summary` the only other pair the cap
/// touches on the golden suite.
const FAMILY_MODIFIERS: [&str; 6] = ["batch", "array", "summary", "all", "single", "multi"];

/// At most this many tools per family in one answer. Measured: K=1 is worth
/// +7.1 points of precision over no cap and +5.4 over K=2, at unchanged recall
/// — every tool it removed on the golden suite was a non-needed one.
const MAX_PER_FAMILY: usize = 1;

fn singularize(term: &str) -> String {
    match term.strip_suffix('s') {
        Some(stem) if stem.len() >= 3 && !term.ends_with("ss") => stem.to_string(),
        _ => term.to_string(),
    }
}

/// Family key of a tool name: its terms, singularized, minus the modifiers,
/// **in order**. Order matters and sorting them would be a bug:
/// `get_component_nets` ("which nets is this component on") and
/// `get_net_components` ("which components are on this net") are two different
/// tools that an order-insensitive key would merge, and the cap would then
/// silently drop one of them. The golden suite never asks for both, which is
/// exactly why this is pinned by a test instead of by the benchmark.
fn family_of(name: &str) -> String {
    let parts: Vec<String> = terms(name)
        .into_iter()
        .map(|t| singularize(&t))
        .filter(|t| !FAMILY_MODIFIERS.contains(&t.as_str()))
        .collect();
    if parts.is_empty() {
        name.to_string()
    } else {
        parts.join("_")
    }
}

/// First sentence of a description, capped. Descriptions run to several
/// sentences of usage guidance; the first one says what the tool does, which is
/// all a chooser needs.
fn summarize(description: &str) -> String {
    let first = description
        .split_once(". ")
        .map(|(head, _)| head)
        .unwrap_or(description)
        .trim_end_matches('.');
    let mut s = first.to_string();
    if s.chars().count() > 160 {
        s = s.chars().take(157).collect::<String>() + "...";
    }
    s
}

/// Rank the registered tools against `query`, best first.
///
/// The pipeline, in order: split the query into clauses, rank the corpus
/// against each clause on its own, keep what clears `CLAUSE_SCORE_RATIO` of
/// that clause's best score up to `per_clause_limit`, merge the clauses on the
/// ratio each hit reached in its own clause (raw scores from two clauses are
/// not comparable), cap each tool family at `MAX_PER_FAMILY`, then truncate to
/// `limit`.
///
/// Ties break on tool name so the ordering is stable across runs — MCP clients
/// cache list responses and a shuffling result set defeats that.
///
/// A decided query returns fewer than `limit` hits, and that is the point: the
/// benchmark measures the union of what a task's queries return, and padding it
/// to `limit` with near-misses is what cost precision.
pub fn search(corpus: &[(&'static str, ToolDef)], query: &str, limit: usize) -> Vec<Hit> {
    if limit == 0 {
        return Vec::new();
    }
    let idf = Idf::build(corpus);
    let per_clause = per_clause_limit(limit);

    // (corpus index, best ratio-to-its-own-clause-best, raw score behind it)
    let mut merged: Vec<(usize, f64, f64)> = Vec::new();

    for clause in split_clauses(query) {
        let qterms = query_terms(&clause);
        if qterms.is_empty() {
            continue;
        }
        let mut ranked: Vec<(usize, f64)> = corpus
            .iter()
            .enumerate()
            .filter_map(|(i, (_, def))| {
                let s = score_tool(&idf, &qterms, def);
                (s > 0.0).then_some((i, s))
            })
            .collect();
        ranked.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| corpus[a.0].1.name.cmp(corpus[b.0].1.name))
        });
        let Some(&(_, best)) = ranked.first() else {
            continue;
        };

        for (i, score) in ranked
            .into_iter()
            .filter(|(_, s)| *s >= CLAUSE_SCORE_RATIO * best)
            .take(per_clause)
        {
            let ratio = score / best;
            match merged.iter_mut().find(|(j, _, _)| *j == i) {
                Some(entry) => {
                    if ratio > entry.1 {
                        entry.1 = ratio;
                        entry.2 = score;
                    }
                }
                None => merged.push((i, ratio, score)),
            }
        }
    }

    merged.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| corpus[a.0].1.name.cmp(corpus[b.0].1.name))
    });

    let mut families: Vec<(String, usize)> = Vec::new();
    let mut hits: Vec<Hit> = Vec::new();
    for (i, _, score) in merged {
        let (toolset, def) = &corpus[i];
        let family = family_of(def.name);
        match families.iter_mut().find(|(f, _)| *f == family) {
            Some(slot) if slot.1 >= MAX_PER_FAMILY => continue,
            Some(slot) => slot.1 += 1,
            None => families.push((family, 1)),
        }
        hits.push(Hit {
            name: def.name,
            toolset,
            summary: summarize(def.description),
            score,
        });
        if hits.len() == limit {
            break;
        }
    }
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::router::ToolRouter;

    fn corpus() -> Vec<(&'static str, ToolDef)> {
        ToolRouter::new().all_tools_with_toolset()
    }

    fn names(query: &str, limit: usize) -> Vec<&'static str> {
        search(&corpus(), query, limit)
            .into_iter()
            .map(|h| h.name)
            .collect()
    }

    #[test]
    fn empty_and_stop_word_only_queries_return_nothing() {
        assert!(names("", 5).is_empty());
        assert!(names("the a of to", 5).is_empty());
    }

    #[test]
    fn exact_tool_name_ranks_first() {
        assert_eq!(names("run erc", 1), vec!["run_erc"]);
        assert_eq!(names("run drc", 1), vec!["run_drc"]);
    }

    #[test]
    fn natural_language_finds_the_right_authoring_tools() {
        let hits = names("add a resistor to the schematic", 8);
        assert!(
            hits.contains(&"add_schematic_component") || hits.contains(&"batch_place_components"),
            "expected a schematic placement tool, got {hits:?}"
        );
    }

    #[test]
    fn wiring_query_finds_pin_connection() {
        let hits = names("connect two component pins with a wire", 8);
        assert!(
            hits.contains(&"connect_pins"),
            "expected connect_pins, got {hits:?}"
        );
    }

    #[test]
    fn export_query_finds_bom_and_netlist() {
        let hits = names("export bom and netlist", 10);
        assert!(hits.contains(&"export_bom"), "{hits:?}");
        assert!(
            hits.contains(&"generate_netlist") || hits.contains(&"export_netlist"),
            "{hits:?}"
        );
    }

    #[test]
    fn results_are_stable_across_repeated_searches() {
        let corpus = corpus();
        let a = search(&corpus, "place a decoupling capacitor", 10);
        let b = search(&corpus, "place a decoupling capacitor", 10);
        assert_eq!(a, b);
    }

    #[test]
    // Amended in F.5: this used to assert *exactly* `limit` results, which
    // silently encoded the old behaviour of padding every answer to the limit
    // with near-misses. That padding is what the relative cutoff and the
    // per-clause budget exist to remove, so the assertion is now the one the
    // name always claimed — never more than the limit. `see
    // decided_query_returns_fewer_than_limit` for the other half.
    fn limit_is_respected() {
        // Both halves matter: `<= limit` alone would also pass if the search
        // returned nothing at all, which is the one way of respecting a limit
        // that would make the tool useless.
        for limit in [3, 8] {
            let hits = names("schematic", limit);
            assert!(hits.len() <= limit, "{limit}: got {hits:?}");
            assert!(!hits.is_empty(), "{limit}: returned nothing");
        }
    }

    #[test]
    // The whole point of F.5: a query with a clear winner must not drag seven
    // also-rans in behind it. Before the cutoff, every query returned exactly
    // `limit` hits and the union of a task's queries reached 34 tools for the
    // ~7 it needed.
    fn decided_query_returns_fewer_than_limit() {
        let hits = names("run erc", 8);
        assert!(
            hits.len() < 8,
            "a decided query should not fill the limit, got {hits:?}"
        );
        assert_eq!(hits.first(), Some(&"run_erc"), "{hits:?}");
    }

    #[test]
    // The family key keeps term order, and this is why. `get_component_nets`
    // ("which nets is this component on") and `get_net_components` ("which
    // components are on this net") are different questions; an
    // order-insensitive key merges them and the family cap then drops one at
    // random. No golden task asks for both, so the benchmark cannot catch
    // this — only this test can.
    fn opposite_direction_tools_are_not_the_same_family() {
        assert_ne!(
            family_of("get_component_nets"),
            family_of("get_net_components"),
            "order-insensitive family key would silently drop one of the two"
        );
    }

    #[test]
    // The family the cap was built for: three spellings of "put components on
    // the schematic". They must share a key, and only the best-ranked one may
    // survive into an answer.
    fn placement_spellings_share_a_family_and_only_one_survives() {
        let members = [
            "place_component",
            "place_component_array",
            "batch_place_components",
        ];
        let key = family_of(members[0]);
        for m in members {
            assert_eq!(family_of(m), key, "{m} should be in family {key}");
        }
        let hits = names("place multiple symbols on the schematic in one call", 8);
        let survivors: Vec<&&str> = hits.iter().filter(|h| members.contains(h)).collect();
        assert_eq!(
            survivors,
            vec![&"batch_place_components"],
            "the cap must keep exactly the best-ranked member, got {hits:?}"
        );
    }

    #[test]
    // The counter-case, from `06_recovery`: adding a symbol and batch-placing
    // components are *not* the same capability, and a task needs both. If the
    // family key ever collapses them, this fails.
    fn adding_and_batch_placing_can_coexist_in_one_answer() {
        assert_ne!(
            family_of("add_schematic_component"),
            family_of("batch_place_components")
        );
        let hits = names("add a symbol to the schematic and place components", 8);
        assert!(hits.contains(&"add_schematic_component"), "{hits:?}");
        assert!(hits.contains(&"batch_place_components"), "{hits:?}");
    }

    #[test]
    // Clause splitting, end to end: a composite intent must return the tools
    // of *each* of its halves. Before splitting, `search_templates` took the
    // whole query and `apply_template` scored 0.21 of the best hit, far under
    // any usable cutoff.
    fn composite_intent_returns_a_tool_per_clause() {
        let hits = names("search reference circuit templates and instantiate one", 8);
        assert!(hits.contains(&"search_templates"), "{hits:?}");
        assert!(hits.contains(&"apply_template"), "{hits:?}");
    }

    #[test]
    fn every_hit_names_a_real_tool_in_a_real_toolset() {
        let router = ToolRouter::new();
        for hit in search(&corpus(), "route a trace on the pcb", 10) {
            assert!(
                router.find_tool_def(hit.name).is_some(),
                "search returned a tool the registry cannot resolve: {}",
                hit.name
            );
            assert!(
                super::super::registry::tools_for(hit.toolset).is_some(),
                "search returned an unknown toolset: {}",
                hit.toolset
            );
        }
    }

    #[test]
    // D6's negative control, pinned: reverse prefix must never cost
    // `batch_place_components` its rank-1 spot on the intent that plural
    // stemming broke. This is the guard that would have caught D6.
    fn d6_negative_control_holds() {
        assert_eq!(
            names("place multiple symbols on the schematic in one call", 1),
            vec!["batch_place_components"]
        );
    }

    #[test]
    // Reverse prefix at work, well above the floor: "templates" (query) only
    // reaches `apply_template` (corpus term "template", 8 letters) through
    // the new branch — nothing else in the existing cascade lets a longer
    // query term match a shorter corpus term.
    fn reverse_prefix_surfaces_plural_query_over_singular_name() {
        let hits = names("search reference circuit templates and instantiate one", 30);
        assert!(
            hits.contains(&"apply_template"),
            "expected apply_template via the reverse-prefix rule, got {hits:?}"
        );
    }

    #[test]
    // Reverse prefix at the floor: "pins" only reaches `get_schematic_component`
    // (via the query's "component" and "pins" -> "pin", three letters, exactly
    // the floor) inside the top 8 with the floor at 3. This is what moving the
    // floor from 4 to 3 measurably bought back; `get_schematic_pin_locations`
    // is a separate, still-open gap (see the module comment) and is not
    // asserted here.
    fn reverse_prefix_at_the_floor_surfaces_get_schematic_component() {
        let hits = names("where are a component's pins", 8);
        assert!(
            hits.contains(&"get_schematic_component"),
            "expected get_schematic_component in the top 8 via the floor=3 reverse-prefix rule, got {hits:?}"
        );
    }

    #[test]
    fn summary_is_much_smaller_than_the_full_description() {
        let corpus = corpus();
        let (_, heavy) = corpus
            .iter()
            .max_by_key(|(_, d)| d.description.len())
            .unwrap();
        let summary = summarize(heavy.description);
        assert!(
            summary.chars().count() <= 160,
            "summary not capped: {} chars",
            summary.chars().count()
        );
    }
}
