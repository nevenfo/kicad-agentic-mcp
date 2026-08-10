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

/// One search hit. `summary` is the first sentence of the tool description:
/// enough to choose between two candidates, far short of the full schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub name: &'static str,
    pub toolset: &'static str,
    pub summary: String,
    pub score: u32,
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
    ("symbol", &["symbol", "library"]),
];

// Plural stemming was tried here and measured worse, so it is not in the code.
// Stripping a trailing "s" from both query and corpus terms looked obviously
// right — "symbols" should match `add_power_symbol` — but on the golden suite
// it moved retrieval recall at eight results per query from 100 % to 98.2 %
// (it stopped surfacing `batch_place_components` for "place multiple symbols
// on the schematic in one call") and changed nothing at lower limits. The
// prefix rule below already absorbs most plural mismatches. Do not re-add it
// without a benchmark run that shows a gain.

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

fn score_tool(query_terms: &[String], def: &ToolDef) -> u32 {
    let name_terms = terms(def.name);
    let desc_terms = terms(def.description);
    let mut score = 0u32;

    for qt in query_terms {
        // Exact hit on a word of the tool name is the strongest signal there is:
        // `run_erc` for "erc" must outrank every tool whose description merely
        // mentions ERC.
        if name_terms.iter().any(|t| t == qt) {
            score += 10;
        } else if name_terms.iter().any(|t| t.starts_with(qt.as_str())) {
            score += 5;
        } else if def.name.contains(qt.as_str()) {
            score += 3;
        }

        if desc_terms.iter().any(|t| t == qt) {
            score += 2;
        }

        for syn in expansions(qt) {
            if name_terms.iter().any(|t| t == syn) {
                score += 4;
            } else if desc_terms.iter().any(|t| t == syn) {
                score += 1;
            }
        }
    }

    score
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

/// Rank every registered tool against `query`, best first.
///
/// Ties break on tool name so the ordering is stable across runs — MCP clients
/// cache list responses and a shuffling result set defeats that.
pub fn search(corpus: &[(&'static str, ToolDef)], query: &str, limit: usize) -> Vec<Hit> {
    let query_terms: Vec<String> = terms(query)
        .into_iter()
        .filter(|t| !is_stop_word(t))
        .collect();
    if query_terms.is_empty() {
        return Vec::new();
    }

    let mut hits: Vec<Hit> = corpus
        .iter()
        .filter_map(|(toolset, def)| {
            let score = score_tool(&query_terms, def);
            (score > 0).then(|| Hit {
                name: def.name,
                toolset,
                summary: summarize(def.description),
                score,
            })
        })
        .collect();

    hits.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.name.cmp(b.name)));
    hits.truncate(limit);
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
    fn limit_is_respected() {
        assert_eq!(names("schematic", 3).len(), 3);
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
