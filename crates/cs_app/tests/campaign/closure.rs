//! Acceptance stage F50-A: every mission dependency closure, and the
//! identity failures a closure must report
//! (`specs/F50-per-mission-compatibility-and-full-campaign-closure.md`,
//! section `### F50-A`, minimum scenario: "Run all mission dependency
//! closures and assert none are silently omitted").
//!
//! The closures under test come from
//! `cs_content::campaign_bindings::CampaignBindings::closures`, production
//! code.

use std::collections::BTreeSet;

use cs_content::campaign_bindings::{
    BindingCategory, BindingError, CampaignBindings, CategoryState, CellState, ClosureError,
    DependencyState, Progression, REQUIRED_SUBSYSTEMS, SubsystemId,
};
use cs_content::catalog::Catalog;

use crate::common::{
    claim, declared, label, load_inventory, ready_binding, ready_campaign, repo_path,
};

/// AC01: one closure per declared mission, and together they account for
/// every declared mission and every required category of every mission they
/// reach. A mission whose closure is skipped, or a category left out of a
/// report, breaks the length and membership assertions below.
#[test]
fn accept_f50_a_every_declared_mission_closure_omits_nothing() {
    let inventory = load_inventory();
    let campaign = CampaignBindings::from_inventory(&inventory)
        .expect("the declared inventory builds placeholders");

    let reports = campaign
        .closures(None)
        .expect("the declared campaign has no progression edges to fail on");
    assert_eq!(
        reports.len(),
        inventory.len(),
        "every declared mission gets its own closure"
    );

    let mut roots = BTreeSet::new();
    let mut reached = BTreeSet::new();
    for report in &reports {
        roots.insert(report.root.clone());
        reached.extend(report.reached.iter().cloned());

        assert_eq!(
            report.cell_count(),
            report.reached.len() * BindingCategory::ALL.len(),
            "closure {} accounts for every required category of every mission it reaches",
            report.root
        );
        for category in BindingCategory::ALL {
            assert!(
                report
                    .cells
                    .iter()
                    .any(|(mission, cell_category, _)| mission == &report.root
                        && cell_category == category),
                "closure of {} includes the {} cell of its root",
                report.root,
                category.label()
            );
        }
        assert_eq!(report.reached.len(), 1, "no progression is bound yet");
        assert_eq!(report.rows, 0, "nothing is bound yet");
        assert_eq!(report.unknown_rows, 0);
        assert_eq!(
            report.subsystem_rows,
            REQUIRED_SUBSYSTEMS.len(),
            "every required subsystem row is in the closure"
        );
        assert_eq!(report.unresolved_subsystems, report.subsystem_rows);
        assert_eq!(report.unsupported_subsystems, 0);
        assert_eq!(report.unknown_progression, 1);
        assert!(
            !report.is_complete(),
            "an unresolved closure is never complete"
        );
    }

    assert_eq!(roots.len(), inventory.len(), "no root was served twice");
    assert_eq!(
        reached.len(),
        inventory.len(),
        "every declared mission is reached by some closure"
    );
    for mission in inventory.labels() {
        assert!(
            reached.contains(mission),
            "{mission} is missing from every closure"
        );
    }
}

/// A bound chain is traversed in full: the closure of the first mission
/// counts the categories, rows and subsystems of everything it reaches.
#[test]
fn accept_f50_a_a_bound_chain_is_traversed_in_full() {
    let campaign = ready_campaign();
    let report = campaign
        .closure(&label("M01"), None)
        .expect("the synthetic chain is acyclic");

    assert_eq!(report.root, label("M01"));
    assert_eq!(
        report.reached,
        vec![label("M01"), label("M02"), label("M03")],
        "the chain is walked in canonical label order"
    );
    assert_eq!(report.cell_count(), 3 * BindingCategory::ALL.len());
    assert_eq!(report.complete_cells(), report.cell_count());
    assert_eq!(report.rows, 39, "every reached mission's rows are counted");
    assert_eq!(report.unknown_rows, 0);
    assert_eq!(report.subsystem_rows, 3 * REQUIRED_SUBSYSTEMS.len());
    assert_eq!(report.unresolved_subsystems, 0);
    assert_eq!(report.unknown_progression, 0);
    assert!(report.is_complete());

    // Reaching into the middle of the chain counts only what that closure
    // reaches, so a shared visited set cannot quietly shrink a report.
    let tail = campaign
        .closure(&label("M03"), None)
        .expect("the chain is acyclic");
    assert_eq!(tail.reached, vec![label("M03")]);
    assert_eq!(tail.cell_count(), BindingCategory::ALL.len());
}

/// Duplicate, cycle and dangling identities are reported as failures. None
/// of them is repaired by guessing which record was meant.
#[test]
fn accept_f50_a_cycle_duplicate_and_dangling_identities_are_reported() {
    // Duplicate identity: two real records with one label never merge.
    let mut campaign = ready_campaign();
    let error = campaign
        .insert(ready_binding(
            "M01",
            "A contradictory second row",
            Progression::known(Vec::new()),
        ))
        .expect_err("a duplicate identity is refused");
    assert!(
        matches!(&error, BindingError::DuplicateId { label } if label.as_str() == "M01"),
        "{error}"
    );

    // Unknown root.
    let error = campaign
        .closure(&label("M77"), None)
        .expect_err("a mission that is not recorded cannot be a root");
    assert!(
        matches!(error, ClosureError::UnknownMission { .. }),
        "{error}"
    );

    // Cycle: M01 -> M02 -> M01.
    let cyclic = declared(vec![
        ready_binding("M01", "One", Progression::known(vec![label("M02")])),
        ready_binding("M02", "Two", Progression::known(vec![label("M01")])),
    ]);
    let error = cyclic
        .closure(&label("M01"), None)
        .expect_err("a progression cycle is reported, not walked");
    match error {
        ClosureError::Cycle { chain } => assert_eq!(
            chain,
            vec![label("M01"), label("M02"), label("M01")],
            "the reported chain is the cycle itself"
        ),
        other => panic!("expected a cycle, got {other}"),
    }

    // Dangling progression identity: a successor that is not recorded.
    let dangling = declared(vec![ready_binding(
        "M01",
        "One",
        Progression::known(vec![label("M09")]),
    )]);
    let error = dangling
        .closure(&label("M01"), None)
        .expect_err("a successor nothing records is reported");
    match error {
        ClosureError::DanglingProgression { from, target } => {
            assert_eq!(from, label("M01"));
            assert_eq!(target, label("M09"));
        }
        other => panic!("expected a dangling progression, got {other}"),
    }

    // Dangling content identity: a row naming content the catalog does not
    // hold. With no catalog handed in, the mission graph alone is checked.
    let catalog = Catalog::new();
    let error = campaign
        .closure(&label("M01"), Some(&catalog))
        .expect_err("content identities are checked against the catalog");
    match error {
        ClosureError::DanglingContent {
            mission,
            role,
            target,
        } => {
            assert_eq!(mission, label("M01"));
            assert!(!role.is_empty());
            assert_eq!(target.kind(), cs_types::content::ContentKind::Mission);
        }
        other => panic!("expected a dangling content identity, got {other}"),
    }
    assert!(
        campaign.closure(&label("M01"), None).is_ok(),
        "the same closure passes when no catalog is supplied"
    );

    // A row nobody has resolved is still a row: it is reported as unknown,
    // never as absent.
    let mut open_row = ready_binding("M01", "One", Progression::known(Vec::new()));
    open_row.categories.remove(&BindingCategory::Actors);
    let open = declared(vec![open_row]);
    let report = open.closure(&label("M01"), None).expect("no edges fail");
    assert_eq!(
        report.cell_count(),
        BindingCategory::ALL.len(),
        "the unrecorded category is still in the report"
    );
    assert_eq!(
        report
            .cells
            .iter()
            .filter(|(_, _, state)| *state == CellState::Missing)
            .count(),
        1,
        "the unrecorded category is counted as missing"
    );
    assert!(!report.is_complete());

    // A blank reason cannot stand in for an explicit unknown.
    assert!(
        matches!(
            CategoryState::unresolved(claim("f50.a.blank_reason"), "   "),
            Err(BindingError::EmptyReason { .. })
        ),
        "an unresolved category must carry a reason"
    );
}

/// The required subsystem rows are the F50 sheet's own prerequisite list,
/// read from the sheet rather than restated here. The other subsystem
/// assertions in this file compare counts against `REQUIRED_SUBSYSTEMS.len()`
/// on both sides, so without this one the set could shrink — a dropped
/// prerequisite feature would make every mission report one row fewer and
/// still be ready.
#[test]
fn accept_f50_a_required_subsystems_match_the_sheet() {
    let sheet = std::fs::read_to_string(repo_path(
        "specs/F50-per-mission-compatibility-and-full-campaign-closure.md",
    ))
    .expect("the F50 sheet reads");

    let line = sheet
        .lines()
        .find(|line| line.starts_with("**Prerequisite features:**"))
        .expect("the sheet still names its prerequisite features");
    let listed: Vec<String> = line
        .trim_start_matches("**Prerequisite features:**")
        .trim()
        .trim_end_matches('.')
        .split(',')
        .map(|feature| feature.trim().to_owned())
        .collect();

    let expected: Vec<String> = listed
        .iter()
        .map(|feature| {
            SubsystemId::new(feature)
                .unwrap_or_else(|error| panic!("{feature} is a subsystem identity: {error}"))
                .as_str()
                .to_owned()
        })
        .collect();
    let actual: Vec<String> = REQUIRED_SUBSYSTEMS
        .iter()
        .map(|id| (*id).to_owned())
        .collect();

    assert_eq!(
        actual.len(),
        expected.len(),
        "one dependency row per prerequisite feature, no more and no fewer"
    );
    let mut sorted_expected = expected.clone();
    let mut sorted_actual = actual.clone();
    sorted_expected.sort();
    sorted_actual.sort();
    assert_eq!(
        sorted_actual, sorted_expected,
        "the required subsystem rows are exactly the sheet's prerequisite features"
    );
    assert_eq!(
        sorted_actual.len(),
        sorted_actual.iter().collect::<BTreeSet<_>>().len(),
        "the sheet's prerequisite list names no feature twice"
    );
    assert_eq!(
        sorted_actual.len(),
        23,
        "the F50 sheet names 23 prerequisite features"
    );
}

/// Every required subsystem carries an explicit row for every mission, and a
/// campaign built from the declared inventory starts with every one of those
/// rows unresolved. A mission whose dependency list were ever shortened below
/// the sheet's list is refused on admission, and one that carried a
/// default/empty state instead of an explicit unknown would show up here.
#[test]
fn accept_f50_a_every_mission_carries_a_row_for_every_required_subsystem() {
    let campaign = CampaignBindings::from_inventory(&load_inventory())
        .expect("the declared inventory builds placeholders");
    let required: BTreeSet<&str> = REQUIRED_SUBSYSTEMS.iter().copied().collect();

    for mission in campaign.missions() {
        let recorded: BTreeSet<&str> = mission
            .dependencies
            .iter()
            .map(|row| row.subsystem.as_str())
            .collect();
        assert_eq!(
            recorded, required,
            "{} carries a row for exactly the required subsystems",
            mission.label
        );
        for row in &mission.dependencies {
            assert!(
                matches!(row.state, DependencyState::Unresolved { .. }),
                "{}/{} is an explicit unresolved row, not a default",
                mission.label,
                row.subsystem
            );
        }
    }
    let report = campaign.coverage();
    assert_eq!(report.subsystem_rows, 24 * REQUIRED_SUBSYSTEMS.len());
    assert_eq!(report.subsystem_resolved, 0);
    assert_eq!(report.subsystem_unresolved, report.subsystem_rows);
}
