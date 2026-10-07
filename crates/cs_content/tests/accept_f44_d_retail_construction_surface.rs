//! F44-D against the owner's original installation: re-measuring the
//! construction screen's budget vocabulary, refusals and slot counts from the
//! shipped files.
//!
//! Spec: `specs/F44-aircraft-construction-budgets-loadouts-and-paint-editor.md`,
//! stage `### F44-D`. Task test prefix: `accept_f44_d_`. Shared contract:
//! `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! Every test here reads the owner's installation through the **production**
//! readers (`cs_assets`' ROF mount) and asserts the constants
//! `cs_content::construction` ships, so a stale constant fails rather than
//! passing. They are `#[ignore = "requires CS_GAME_DIR"]` because CI has no
//! original data; the implementing and reviewing agents run them with
//! `--include-ignored`, and they fail loudly when `CS_GAME_DIR` is absent.
//!
//! **What is measured here is file content: ids, counts and code shapes. It is
//! not evidence of how the original behaves** — that would require running the
//! original executable, which no agent can do. `retail` is read access.

#[path = "f44_d_support/mod.rs"]
mod support;

use std::collections::{BTreeMap, BTreeSet};

use cs_content::construction::{
    BudgetCategory, BudgetVocabularyGap, ORIGINAL_ARMOR_ZONE_COUNT, ORIGINAL_ARMOR_ZONES,
    ORIGINAL_BUDGET_ROWS, ORIGINAL_BUDGET_TOTALS, ORIGINAL_CONSTRUCTION_FIELDS, ORIGINAL_GUN_SLOTS,
    ORIGINAL_HARDPOINT_POINTS, ORIGINAL_PLANE_SLOTS, ORIGINAL_PURCHASE_REFUSALS,
    ORIGINAL_ROCKET_SLOTS, normalized_budget_word, original_budget_vocabulary_gaps,
    original_budget_words,
};
use support::{
    ARMOR, COUNT_BOUNDS, GUNS, HARDPOINTS, ORDINANCE_LAYOUT, PLANE_CONSTRUCTION, PURCHASE,
    RESOURCE_HEADER, game_dir, read_member,
};

/// The purchase screen's own refusal ids, with the numbers the header declares.
///
/// Kept here rather than in the crate because the *numbers* are the
/// installation's: a header that moved `IDS_PX_PUR_OVERWEIGHT` must fail this
/// test, not silently renumber a claim.
const EXPECTED_REFUSALS: [(&str, u32); 5] = [
    ("IDS_PX_PUR_PROBLEM", 1182),
    ("IDS_PX_PUR_NOENGINE", 1183),
    ("IDS_PX_PUR_NOPAINT", 1225),
    ("IDS_PX_PUR_INSUFFICIENT", 1226),
    ("IDS_PX_PUR_OVERWEIGHT", 1227),
];

/// The resource-header ids of the construction screen's weight/cost fields.
const EXPECTED_FIELDS: [(&str, u32); 4] = [
    ("IDS_PX_WEIGHTCAPACITY_TITLE", 1030),
    ("IDS_PX_CURRENTWEIGHT_TITLE", 1031),
    ("IDS_PX_PLANECOST_TITLE", 1036),
    ("IDS_PX_PUR_TOTALCOST", 1178),
];

fn text(member: &str) -> String {
    String::from_utf8_lossy(&read_member(&game_dir(), member)).into_owned()
}

/// The `#define` line a header declares for `name`, with its value.
fn declared(header: &str, name: &str) -> Option<u32> {
    let needle = format!("#define {name}");
    let line = header
        .lines()
        .find(|candidate| candidate.trim_start().starts_with(&needle))?;
    line.trim()
        .rsplit(' ')
        .next()
        .and_then(|value| value.parse::<u32>().ok())
}

/// Whether the purchase screen fills a cell through an engine callback rather
/// than through a literal: either the one-call form `callback($$E$$, <n>, …)`
/// or the per-control fill `<control>.TJ = <n>`.
fn filled_by_callback(purchase: &str, callback: u32) -> bool {
    purchase.contains(&format!("$$E$$, {callback},"))
        || purchase.contains(&format!("TJ = {callback}"))
}

/// The count a literal spells: the `[n]` of `object ES[4]`, or the `< n` of
/// `for (R = 0; R < 4; R++)`.
fn bracket_count(literal: &str) -> Option<u32> {
    if let Some(open) = literal.rfind('[')
        && let Some(close) = literal.rfind(']')
        && close > open
    {
        return literal[open + 1..close].parse::<u32>().ok();
    }
    let after = literal.split("< ").nth(1)?;
    after
        .split(|character: char| !character.is_ascii_digit())
        .find(|piece| !piece.is_empty())?
        .parse::<u32>()
        .ok()
}

/// **Every budget row the purchase screen keeps is declared in the shipped
/// script, and every cell of it is filled by an engine callback — which is why
/// no file holds a component's weight or price.**
///
/// The second half is the budget side of what F44-D was asked to verify: the
/// *shape* of the budget is in a file and is re-measured here, while its
/// numbers are produced by the executable. [`cs_content::construction::PriceBook`]
/// therefore still carries no original quote, and this test says so with
/// evidence rather than with an assertion of ignorance.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f44_d_retail_every_budget_row_is_declared_and_filled_by_the_engine() {
    let purchase = text(PURCHASE);
    for row in ORIGINAL_BUDGET_ROWS {
        assert!(
            purchase.contains(row.label),
            "the purchase screen must declare the row label {}",
            row.label
        );
        assert!(
            purchase.contains(row.weight) && purchase.contains(row.cost),
            "the {} row must keep a weight and a cost cell",
            row.word
        );
        assert!(
            filled_by_callback(&purchase, row.weight_callback),
            "the {} weight cell must be filled by engine callback {}",
            row.word,
            row.weight_callback
        );
        assert!(
            filled_by_callback(&purchase, row.cost_callback),
            "the {} cost cell must be filled by engine callback {}",
            row.word,
            row.cost_callback
        );
    }
    assert!(
        purchase.contains(ORIGINAL_BUDGET_TOTALS.weight)
            && purchase.contains(ORIGINAL_BUDGET_TOTALS.cost),
        "the totals row keeps a weight and a cost cell"
    );
    assert!(
        filled_by_callback(&purchase, ORIGINAL_BUDGET_TOTALS.weight_callback),
        "the totals row is filled by the engine too"
    );
}

/// **The project's budget vocabulary and the purchase screen's differ, and the
/// difference is reported by name on both sides.**
///
/// The nouns are derived from the *file* here — every `pur_t_*` id that ends in
/// `weight` or `cost` — so the audit's input is the installation's own row set
/// rather than a list this test restated. The expected output is then the
/// exact three gaps the two vocabularies have: this project prices `ordnance`
/// and `equipment` with no row of the same noun, and the screen keeps a
/// `hardpoint` row this project does not price as a category.
///
/// Nothing is mapped between them: which of the two the original really
/// charges for is unmeasured (the executable holds the numbers), so the audit
/// reports the disagreement instead of quietly renaming a category.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f44_d_retail_the_budget_vocabulary_gap_is_reported_by_name() {
    let purchase = text(PURCHASE);

    let mut derived: BTreeSet<String> = BTreeSet::new();
    let mut rest = purchase.as_str();
    while let Some(at) = rest.find("pur_t_") {
        let tail = &rest[at + "pur_t_".len()..];
        let end = tail
            .find(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
            .unwrap_or(tail.len());
        let id = &tail[..end];
        if id.ends_with("weight") {
            derived.insert(normalized_budget_word(id.trim_end_matches("weight")));
        } else if id.ends_with("cost") {
            derived.insert(normalized_budget_word(id.trim_end_matches("cost")));
        }
        rest = if end == 0 { &tail[1..] } else { &tail[end..] };
    }

    let mut expected: BTreeSet<String> = original_budget_words()
        .into_iter()
        .chain(std::iter::once(normalized_budget_word(
            ORIGINAL_BUDGET_TOTALS.word,
        )))
        .collect();
    expected.retain(|word| !word.is_empty());
    assert_eq!(
        derived, expected,
        "the committed budget rows must be the rows the installation declares"
    );

    let gaps = original_budget_vocabulary_gaps();
    assert_eq!(
        gaps,
        vec![
            BudgetVocabularyGap::OursWithoutOriginal {
                ours: BudgetCategory::Ordnance,
            },
            BudgetVocabularyGap::OursWithoutOriginal {
                ours: BudgetCategory::Equipment,
            },
            BudgetVocabularyGap::OriginalWithoutOurs {
                original: "hardpoint".to_owned(),
            },
        ],
        "the two vocabularies differ in exactly these three places"
    );
    for shared in [
        BudgetCategory::Airframe,
        BudgetCategory::Engine,
        BudgetCategory::Armor,
        BudgetCategory::Guns,
    ] {
        assert!(
            !gaps.contains(&BudgetVocabularyGap::OursWithoutOriginal { ours: shared }),
            "{shared} is a noun both sides share, so it is not a gap"
        );
    }
}

/// **The purchase refusals the original names are declared by the shipped
/// header, with the crate's own ids.**
///
/// `IDS_PX_PUR_OVERWEIGHT` and `IDS_PX_PUR_INSUFFICIENT` are the two economy
/// refusals the transactional economy already enforces; the other three name a
/// rule whose original *condition* is unmeasured and is recorded as such
/// rather than implemented from a string id.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f44_d_retail_the_purchase_refusals_are_declared_by_the_header() {
    let header = text(RESOURCE_HEADER);
    for (name, id) in EXPECTED_REFUSALS {
        assert_eq!(
            declared(&header, name),
            Some(id),
            "the header must declare {name} as {id}"
        );
    }
    assert_eq!(
        ORIGINAL_PURCHASE_REFUSALS, EXPECTED_REFUSALS,
        "the crate commits the refusals the header declares"
    );
}

/// **Every slot count the construction screens declare is re-measured from the
/// shipped code shapes.**
///
/// Array sizes and loop bounds are counts; a control's `@globals@AR` argument
/// is not — it is a control parameter whose value differs per screen (`5`, `7`,
/// `11`, `12`, `13`, `26`, `27`, `32`) and says nothing about how many entries
/// a list holds. Only the former are used, and each is checked against the
/// constant the crate commits.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f44_d_retail_every_slot_count_is_remeasured_from_the_screens() {
    let mut measured: BTreeMap<&'static str, u32> = BTreeMap::new();
    for (member, literal, what) in COUNT_BOUNDS {
        let source = text(member);
        assert!(
            source.contains(literal),
            "{member} must still contain {literal}: {what}"
        );
        let bound =
            bracket_count(literal).unwrap_or_else(|| panic!("{literal} must spell a count"));
        measured.insert(what, bound);
    }

    assert_eq!(
        measured["the gun page's four gun-position dropdowns"], ORIGINAL_GUN_SLOTS,
        "the gun page's positions are the crate's gun slots"
    );
    assert_eq!(
        measured["the ordnance layout's four gun-ammo dropdowns"], ORIGINAL_GUN_SLOTS,
        "the ordnance layout agrees with the gun page"
    );
    assert_eq!(
        measured["the ordnance layout's eight rocket dropdowns"], ORIGINAL_ROCKET_SLOTS,
        "the rocket slots are re-measured"
    );
    assert_eq!(
        measured["the hardpoint page's two point dropdowns"], ORIGINAL_HARDPOINT_POINTS,
        "the hardpoint points are re-measured"
    );
    assert_eq!(
        measured["the armor page's four armor-point dropdowns"], ORIGINAL_ARMOR_ZONE_COUNT,
        "the armor page's zones are re-measured"
    );
    assert_eq!(
        measured["the loop that fills the four armor points"], ORIGINAL_ARMOR_ZONE_COUNT,
        "the armor loop agrees with the array"
    );
    assert_eq!(
        measured["the construction screen's four saved-plane slots"], ORIGINAL_PLANE_SLOTS,
        "the plane slots are re-measured"
    );
}

/// **The four armor zones are named by the shipped screens and by the
/// resource header, twice, and the ids match the crate's own.**
///
/// This is per-zone armor's original counterpart (non-negotiable 1's "per-zone
/// armor"): nose, tail, left, right — four zones, each a title the page builds
/// a dropdown under. Which damage-graph node each zone maps to is not in any
/// file, so the mapping stays declared rather than invented.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f44_d_retail_the_four_armor_zones_are_declared_twice() {
    let armor = text(ARMOR);
    let header = text(RESOURCE_HEADER);
    for (macro_name, id, control) in ORIGINAL_ARMOR_ZONES {
        assert!(
            armor.contains(control),
            "the armor page must build the {control} title"
        );
        assert_eq!(
            declared(&header, macro_name),
            Some(id),
            "the header must declare {macro_name} as {id}"
        );
    }
    assert_eq!(
        u32::try_from(ORIGINAL_ARMOR_ZONES.len()).expect("a count fits"),
        ORIGINAL_ARMOR_ZONE_COUNT
    );
}

/// **The construction screen keeps a weight capacity beside a current weight
/// and a plane cost — the two comparisons F44's budget is judged on.**
///
/// The *capacity's number* comes from an engine callback and is unmeasured;
/// what this test measures is that the screen asks for it at all, by the
/// header's own ids and the script's own control labels.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f44_d_retail_the_construction_screen_keeps_weight_and_cost_fields() {
    let construction = text(PLANE_CONSTRUCTION);
    let header = text(RESOURCE_HEADER);
    for (name, id) in EXPECTED_FIELDS {
        assert_eq!(
            declared(&header, name),
            Some(id),
            "the header must declare {name} as {id}"
        );
    }
    assert_eq!(ORIGINAL_CONSTRUCTION_FIELDS, EXPECTED_FIELDS);
    for control in [
        "px_t_weightcapacity",
        "px_t_currentweight",
        "px_t_planecost",
        "px_t_cashtitle",
    ] {
        assert!(
            construction.contains(control),
            "the construction screen must build the {control} field"
        );
    }
}

/// **The gun page's four positions and the ordnance layout's own four gun
/// rows agree — one number measured on two independent screens.**
///
/// Non-negotiable 1 reports "four gun positions supporting single/pair
/// selections" from the manual; here it is measured from the code of two
/// different screens instead, which is what "confirm against each discovered
/// airframe/rule profile before declaring universal limits" asks for.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f44_d_retail_the_four_gun_positions_are_measured_on_two_screens() {
    let guns = text(GUNS);
    let layout = text(ORDINANCE_LAYOUT);
    let hardpoints = text(HARDPOINTS);
    for literal in ["object ES[4]", "object DS[4]", "int AS[4]"] {
        assert!(
            guns.contains(literal),
            "the gun page must still declare {literal}"
        );
    }
    for literal in ["object PKA[4]", "object QKA[4]"] {
        assert!(
            layout.contains(literal),
            "the ordnance layout must still declare {literal}"
        );
    }
    assert!(
        hardpoints.contains("object DT[2]"),
        "the hardpoint page still builds two points"
    );
}
