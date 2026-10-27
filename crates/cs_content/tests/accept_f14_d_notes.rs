//! Acceptance scenario F14-D (notes): the baseline inventory's collection list
//! and totals have one owner, `docs/findings/2026-10-27-f14-d-baseline-totals.md`.
//!
//! Each F14-D stage used to republish the whole inventory in its own findings
//! note, so every later collection made every earlier note stale and nothing
//! failed. These tests read the findings directory as a document set:
//!
//! * the kind list is **derived** from [`ContentKind::ALL`], so a kind added to
//!   the catalog without an entry in the totals file fails without original
//!   data;
//! * the populated table must sum to the file's stated total;
//! * no other F14-D note may restate an inventory total;
//! * the retail test (`#[ignore = "requires CS_GAME_DIR"]`) compares the table
//!   with the `collections` object of the production report measured from the
//!   installation, so the numbers are never only hand-typed.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use cs_content::catalog::baseline::{baseline_report_json, retail_baseline};
use cs_types::content::ContentKind;

const TOTALS_FILE: &str = "2026-10-27-f14-d-baseline-totals.md";

/// Statements that are true only of one stage's own tree and are marked as
/// such in their note, so they are not a restated inventory total.
const STAGE_LOCAL_STATEMENTS: &[(&str, &str)] = &[(
    "2026-10-03-f14-d-4-scene-and-mesh-collections.md",
    "338 rows in six collections",
)];

fn findings_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("the workspace root")
        .join("docs/findings")
}

fn totals_text() -> String {
    let path = findings_dir().join(TOTALS_FILE);
    fs::read_to_string(&path).unwrap_or_else(|error| panic!("{} reads: {error}", path.display()))
}

/// The text between two marker comments.
fn between<'a>(text: &'a str, begin: &str, end: &str) -> &'a str {
    let start = text
        .find(begin)
        .unwrap_or_else(|| panic!("{begin} is present"))
        + begin.len();
    let stop = text[start..]
        .find(end)
        .unwrap_or_else(|| panic!("{end} follows {begin}"));
    &text[start..start + stop]
}

/// The populated table as collection label -> rows.
fn populated(text: &str) -> BTreeMap<String, usize> {
    let mut rows = BTreeMap::new();
    for line in between(text, "<!-- totals:begin -->", "<!-- totals:end -->").lines() {
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        let Some(label) = cells.get(1).and_then(|c| c.strip_prefix('`')) else {
            continue;
        };
        let label = label.trim_end_matches('`');
        let count = cells[2]
            .parse::<usize>()
            .unwrap_or_else(|_| panic!("the row count of `{label}` is a number"));
        assert!(
            rows.insert(label.to_owned(), count).is_none(),
            "`{label}` has one row in the table"
        );
    }
    rows
}

fn empty_kinds(text: &str) -> BTreeSet<String> {
    between(text, "<!-- empty:begin -->", "<!-- empty:end -->")
        .lines()
        .filter_map(|line| line.trim().strip_prefix("* `"))
        .map(|label| label.trim_end_matches('`').to_owned())
        .collect()
}

/// Every `ContentKind` is accounted for in exactly one of the two lists, and
/// the table adds up to the stated total.
#[test]
fn accept_f14_d_notes_totals_file_accounts_for_every_content_kind() {
    let text = totals_text();
    let populated = populated(&text);
    let empty = empty_kinds(&text);
    let all: BTreeSet<String> = ContentKind::ALL
        .iter()
        .map(|kind| kind.label().to_owned())
        .collect();
    let listed: BTreeSet<String> = populated
        .keys()
        .cloned()
        .chain(empty.iter().cloned())
        .collect();
    assert_eq!(
        populated.len() + empty.len(),
        listed.len(),
        "a kind is listed as both populated and empty"
    );
    let missing: Vec<_> = all.difference(&listed).collect();
    assert!(
        missing.is_empty(),
        "{TOTALS_FILE} does not account for {missing:?}: add each to the populated table (with its \
         measured rows) or to the empty list"
    );
    let unknown: Vec<_> = listed.difference(&all).collect();
    assert!(
        unknown.is_empty(),
        "{TOTALS_FILE} lists {unknown:?}, which are not ContentKind labels"
    );

    let sum: usize = populated.values().sum();
    let stated = text
        .lines()
        .find_map(|line| line.strip_prefix("**Total: "))
        .expect("the file states a total");
    let mut words = stated.split_whitespace();
    let rows: usize = words
        .next()
        .and_then(|w| w.parse().ok())
        .expect("a row total");
    let collections: usize = words
        .nth(2)
        .and_then(|w| w.parse().ok())
        .expect("a collection count");
    assert_eq!(rows, sum, "the stated row total equals the table's sum");
    assert_eq!(
        collections,
        populated.len(),
        "the stated collection count equals the table's rows"
    );
}

/// No F14-D stage note restates the inventory's totals: it would go stale with
/// the next collection.
#[test]
fn accept_f14_d_notes_no_stage_note_restates_the_inventory_total() {
    let totals = totals_text();
    let populated = populated(&totals);
    let grand: usize = populated.values().sum();
    let spellings = [grand.to_string(), {
        let digits = grand.to_string();
        let split = digits.len() - 3;
        format!("{} {}", &digits[..split], &digits[split..])
    }];
    let mut offenders = Vec::new();
    for entry in fs::read_dir(findings_dir()).expect("docs/findings reads") {
        let path = entry.expect("a directory entry").path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if !name.contains("f14-d") || name == TOTALS_FILE || !name.ends_with(".md") {
            continue;
        }
        let text = fs::read_to_string(&path).expect("a note reads");
        let lowered = text.to_lowercase();
        let mut hits: Vec<String> = spellings
            .iter()
            .filter(|spelling| text.contains(spelling.as_str()))
            .map(|spelling| format!("the current total {spelling}"))
            .collect();
        for (index, _) in lowered.match_indices(" collections") {
            let head = &lowered[..index];
            let phrase: Vec<&str> = head.split_whitespace().rev().take(4).collect();
            // "<n> rows in|across|over <word> collections"
            if phrase.len() == 4
                && ["in", "across", "over"].contains(&phrase[1])
                && phrase[2] == "rows"
                && STAGE_LOCAL_STATEMENTS.iter().all(|(file, _)| *file != name)
            {
                hits.push(format!("\"rows {} {} collections\"", phrase[1], phrase[0]));
            }
        }
        for needle in [
            "populated collection,",
            "of the required collections are populated",
        ] {
            if lowered.contains(needle) {
                hits.push(format!("\"{needle}\""));
            }
        }
        if !hits.is_empty() {
            offenders.push(format!("{name}: {}", hits.join(", ")));
        }
    }
    assert!(
        offenders.is_empty(),
        "these notes restate inventory totals; point at {TOTALS_FILE} instead: {offenders:#?}"
    );
}

/// The retail half: the table equals what the production report measures.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f14_d_notes_retail_totals_equal_the_production_report() {
    let game_dir =
        PathBuf::from(std::env::var("CS_GAME_DIR").expect(
            "CS_GAME_DIR must name the original installation for the retail acceptance test",
        ));
    let baseline = retail_baseline(&game_dir).expect("the original installation reads");
    let mut measured: BTreeMap<String, usize> = BTreeMap::new();
    for element in baseline.catalog.elements() {
        *measured.entry(element.kind.label().to_owned()).or_default() += 1;
    }
    // The report renders exactly this map as its `collections` object.
    let report = baseline_report_json(&baseline);
    for (label, rows) in &measured {
        assert!(
            report.contains(&format!("\"{label}\":{rows}")),
            "the report's collections object carries {label}: {rows}"
        );
    }
    assert_eq!(
        populated(&totals_text()),
        measured,
        "{TOTALS_FILE} is out of date: re-measure with `cs-inspect catalog`"
    );
}
