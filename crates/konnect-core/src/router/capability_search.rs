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
            score += 4;
        }

        if desc_terms.iter().any(|t| t == qt) {
            score += 2;
        } else if desc_terms
            .iter()
            .any(|t| t.len() >= REVERSE_PREFIX_MIN_LEN && qt.starts_with(t.as_str()))
        {
            score += 1;
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
