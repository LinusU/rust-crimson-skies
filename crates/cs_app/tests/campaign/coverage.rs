//! Acceptance stage F50-A: campaign coverage totals over the frozen
//! denominator (`specs/F50-per-mission-compatibility-and-full-campaign-closure.md`,
//! section `### F50-A`, minimum scenario "a missing/unknown child cannot
//! disappear from aggregate totals or become ready").
//!
//! The reports under test come from
//! `cs_content::campaign_bindings::CampaignBindings::coverage`, production
//! code. Every fixture is synthetic and says so; none of this is evidence
//! about the campaign.

use cs_content::campaign_bindings::{
    BindingCategory, BindingRow, CampaignBindings, CategoryState, CellState, CoverageReport,
    DependencyState, MISSION_ROLE, MissionBinding, PROGRAM_ROLE, Progression, REQUIRED_SUBSYSTEMS,
    WORLD_ROLE,
};
use cs_types::content::ContentKind;

use crate::common::{claim, declared, label, load_inventory, ready_binding, ready_campaign};

/// Every cell is classified exactly once, so the four counts always add up
/// to the cell total a missing or unknown child could otherwise hide in.
fn assert_totals_add_up(report: &CoverageReport) {
    assert_eq!(
        report.complete_cells
            + report.unknown_cells
            + report.missing_cells
            + report.unsupported_cells,
        report.cells,
        "every (mission, category) cell is counted exactly once: {report:?}"
    );
    assert_eq!(
        report.cells,
        report.total_missions * report.required_categories
    );
    assert_eq!(
        report.subsystem_resolved + report.subsystem_unresolved + report.subsystem_unsupported,
        report.subsystem_rows,
        "every subsystem dependency row is counted exactly once: {report:?}"
    );
    assert!(
        report.unknown_rows <= report.rows,
        "unknown rows are a subset of the recorded rows: {report:?}"
    );
}

/// The declared campaign starts exactly where the work orders start:
/// every binding explicitly unresolved, every required subsystem explicitly
/// unresolved, progression unknown, nothing ready and nothing filtered out.
#[test]
fn accept_f50_a_the_declared_campaign_starts_unresolved_and_unready() {
    let inventory = load_inventory();
    let campaign = CampaignBindings::from_inventory(&inventory)
        .expect("the declared inventory builds placeholders");

    let report = campaign.coverage();
    assert_eq!(report.declared_missions, 24);
    assert_eq!(report.total_missions, 24);
    assert_eq!(report.required_categories, BindingCategory::ALL.len());
    assert_eq!(report.required_categories, 7);
    assert_eq!(report.cells, 24 * 7);
    assert_eq!(report.complete_cells, 0, "nothing is bound yet");
    assert_eq!(report.unknown_cells, report.cells);
    assert_eq!(report.missing_cells, 0);
    assert_eq!(report.unsupported_cells, 0);
    assert_eq!(report.rows, 0);
    assert_eq!(report.unknown_rows, 0);
    assert_eq!(report.subsystem_rows, 24 * REQUIRED_SUBSYSTEMS.len());
    assert_eq!(report.subsystem_resolved, 0);
    assert_eq!(report.subsystem_unresolved, report.subsystem_rows);
    assert_eq!(report.progression_known, 0);
    assert_eq!(report.progression_unknown, 24);
    assert_eq!(report.discovered_extra(), 0);
    assert_eq!(report.declared_cells(), 24 * 7);
    assert_totals_add_up(&report);
    assert!(
        !report.is_ready(),
        "a campaign of unresolved bindings is never ready"
    );

    // Every declared mission is recorded, and every one of them is a
    // discovery label rather than a retail identity.
    assert_eq!(campaign.len(), 24);
    assert_eq!(campaign.declared_count(), 24);
    for mission in campaign.missions() {
        assert!(mission.is_placeholder());
        assert!(
            mission.catalog_identity().is_none(),
            "{} carries no retail catalog identity yet",
            mission.label
        );
    }
}

/// Removing one required category keeps the cell total intact and counts
/// the gap as `missing` instead of dropping the cell, and readiness goes
/// with it.
#[test]
fn accept_f50_a_a_missing_child_stays_in_the_totals_and_blocks_ready() {
    let baseline = ready_campaign().coverage();

    let mut without_objectives = ready_binding(
        "M02",
        "Synthetic Two",
        Progression::known(vec![label("M03")]),
    );
    assert!(
        without_objectives
            .categories
            .remove(&BindingCategory::Objectives)
            .is_some(),
        "the fixture starts with every required category"
    );
    let campaign = declared(vec![
        ready_binding(
            "M01",
            "Synthetic One",
            Progression::known(vec![label("M02")]),
        ),
        without_objectives,
        ready_binding("M03", "Synthetic Three", Progression::known(Vec::new())),
    ]);

    let report = campaign.coverage();
    assert_eq!(
        report.cells, baseline.cells,
        "a missing category changes no total"
    );
    assert_eq!(
        report.rows,
        baseline.rows - 3,
        "the three objective rows left with their category; the cell is what must stay counted"
    );
    assert_eq!(report.complete_cells, baseline.complete_cells - 1);
    assert_eq!(report.missing_cells, 1);
    assert_eq!(report.unknown_cells, 0);
    assert_eq!(report.unsupported_cells, 0);
    assert_totals_add_up(&report);
    assert!(
        !report.is_ready(),
        "a mission with an unrecorded required category is not ready"
    );
}

/// An unknown row keeps its cell, its row and the totals it belongs to, and
/// it is never counted as complete.
#[test]
fn accept_f50_a_an_unknown_child_stays_in_the_totals_and_blocks_ready() {
    let baseline = ready_campaign().coverage();

    let mut with_unknown_objective = ready_binding(
        "M02",
        "Synthetic Two",
        Progression::known(vec![label("M03")]),
    );
    {
        let Some(CategoryState::Rows(rows)) = with_unknown_objective
            .categories
            .get_mut(&BindingCategory::Objectives)
        else {
            panic!("the fixture records its objectives as rows");
        };
        rows.retain(|row| row.role() != "objective");
        rows.push(
            BindingRow::unknown(
                "objective",
                claim("f50.a.unbound_objective"),
                "objective id not yet bound to original data",
            )
            .expect("the unknown row is well formed"),
        );
    }
    let campaign = declared(vec![
        ready_binding(
            "M01",
            "Synthetic One",
            Progression::known(vec![label("M02")]),
        ),
        with_unknown_objective,
        ready_binding("M03", "Synthetic Three", Progression::known(Vec::new())),
    ]);

    let report = campaign.coverage();
    assert_eq!(
        report.cells, baseline.cells,
        "the unknown row changed no total"
    );
    assert_eq!(report.rows, baseline.rows, "the unknown row is still a row");
    assert_eq!(report.unknown_rows, 1);
    assert_eq!(report.complete_cells, baseline.complete_cells - 1);
    assert_eq!(report.unknown_cells, 1);
    assert_eq!(report.missing_cells, 0);
    assert_totals_add_up(&report);
    assert!(
        !report.is_ready(),
        "a mission with an unknown child row is not ready"
    );
}

/// An unresolved subsystem dependency blocks readiness even while every
/// content category is complete: the explicit dependency row is part of
/// what the mission waits for.
#[test]
fn accept_f50_a_an_unresolved_subsystem_row_blocks_ready() {
    let baseline = ready_campaign().coverage();

    let mut with_open_dependency = ready_binding(
        "M02",
        "Synthetic Two",
        Progression::known(vec![label("M03")]),
    );
    with_open_dependency.dependencies[0].state =
        DependencyState::unresolved(claim("f50.a.open_subsystem"), "subsystem work not started")
            .expect("the unresolved state is well formed");
    let campaign = declared(vec![
        ready_binding(
            "M01",
            "Synthetic One",
            Progression::known(vec![label("M02")]),
        ),
        with_open_dependency,
        ready_binding("M03", "Synthetic Three", Progression::known(Vec::new())),
    ]);

    let report = campaign.coverage();
    assert_eq!(report.cells, baseline.cells);
    assert_eq!(
        report.complete_cells, baseline.complete_cells,
        "every content category is still complete"
    );
    assert_eq!(report.subsystem_rows, baseline.subsystem_rows);
    assert_eq!(report.subsystem_resolved, baseline.subsystem_resolved - 1);
    assert_eq!(report.subsystem_unresolved, 1);
    assert_totals_add_up(&report);
    assert!(
        !report.is_ready(),
        "one unresolved subsystem dependency keeps the campaign unready"
    );
}

/// Discovered content beyond the declared denominator is visible — it grows
/// the cell total and blocks readiness — and only an explicit `declare`
/// call adds it to the denominator.
#[test]
fn accept_f50_a_discovered_content_stays_visible() {
    let inventory = load_inventory();
    let mut campaign = CampaignBindings::from_inventory(&inventory)
        .expect("the declared inventory builds placeholders");

    let discovered = label("X99");
    campaign
        .insert(
            MissionBinding::unresolved(
                discovered.clone(),
                Some("Discovered scenario beyond the work orders".to_owned()),
            )
            .expect("the placeholder is well formed"),
        )
        .expect("the discovered mission records");

    let report = campaign.coverage();
    assert_eq!(report.declared_missions, 24);
    assert_eq!(report.total_missions, 25);
    assert_eq!(report.discovered_extra(), 1);
    assert_eq!(report.cells, 25 * 7);
    assert_eq!(
        report.declared_cells(),
        24 * 7,
        "the denominator and the recorded set are reported separately"
    );
    assert!(report.cells > report.declared_cells());
    assert_totals_add_up(&report);
    assert!(!report.is_ready());

    campaign
        .declare(&discovered)
        .expect("a recorded mission can join the denominator");
    let report = campaign.coverage();
    assert_eq!(report.declared_missions, 25);
    assert_eq!(report.discovered_extra(), 0);
    assert_eq!(report.cells, 25 * 7);
    assert_totals_add_up(&report);
}

/// The synthetic fixture proves the ready path exists at all: with every
/// category complete, every subsystem row resolved and every progression
/// recorded, the report says ready.
#[test]
fn accept_f50_a_a_fully_bound_campaign_is_ready() {
    let report = ready_campaign().coverage();
    assert_eq!(report.declared_missions, 3);
    assert_eq!(report.total_missions, 3);
    assert_eq!(report.cells, 21);
    assert_eq!(report.complete_cells, 21);
    assert_eq!(report.missing_cells, 0);
    assert_eq!(report.unknown_cells, 0);
    assert_eq!(report.unsupported_cells, 0);
    assert_eq!(report.rows, 39, "three missions, thirteen rows each");
    assert_eq!(report.unknown_rows, 0);
    assert_eq!(report.subsystem_resolved, report.subsystem_rows);
    assert_eq!(report.subsystem_unresolved, 0);
    assert_eq!(report.progression_known, 3);
    assert_eq!(report.progression_unknown, 0);
    assert_eq!(report.discovered_extra(), 0);
    assert_totals_add_up(&report);
    assert!(report.is_ready());
}

/// An explicitly unsupported category is counted as unsupported — not as
/// unused, and not as ready.
#[test]
fn accept_f50_a_an_unsupported_category_stays_in_the_totals_and_blocks_ready() {
    let mut unsupported = ready_binding(
        "M02",
        "Synthetic Two",
        Progression::known(vec![label("M03")]),
    );
    unsupported.categories.insert(
        BindingCategory::RewardsProgression,
        CategoryState::unsupported("no reward subsystem exists yet").expect("the reason is given"),
    );
    let campaign = declared(vec![
        ready_binding(
            "M01",
            "Synthetic One",
            Progression::known(vec![label("M02")]),
        ),
        unsupported,
        ready_binding("M03", "Synthetic Three", Progression::known(Vec::new())),
    ]);

    let report = campaign.coverage();
    assert_eq!(report.cells, 21);
    assert_eq!(report.unsupported_cells, 1);
    assert_eq!(report.complete_cells, 20);
    assert_totals_add_up(&report);
    assert!(!report.is_ready());
}

/// The identity rows of a complete binding carry the kind their role means,
/// so a synthetic `world` row cannot quietly point at a mission.
#[test]
fn accept_f50_a_identity_rows_carry_the_kind_their_role_means() {
    let binding = ready_binding("M01", "Synthetic One", Progression::known(Vec::new()));
    let Some(CategoryState::Rows(rows)) = binding.category(BindingCategory::MissionIdentity) else {
        panic!("the fixture records its identity as rows");
    };
    assert_eq!(rows.len(), 3);
    for (role, kind) in [
        (MISSION_ROLE, ContentKind::Mission),
        (WORLD_ROLE, ContentKind::World),
        (PROGRAM_ROLE, ContentKind::Script),
    ] {
        let row = rows
            .iter()
            .find(|row| row.role() == role)
            .unwrap_or_else(|| panic!("the {role} row is present"));
        assert_eq!(
            row.content_target()
                .unwrap_or_else(|| panic!("the {role} row is bound"))
                .kind(),
            kind,
            "the {role} row carries the kind its role means"
        );
    }
    assert!(
        binding
            .cells()
            .all(|(_, state)| state == CellState::Complete),
        "every required category of a ready binding is complete"
    );
}

/// The `actors` category is recorded with a forced-airframe row — the owner
/// ruling names forced and captured airframes explicitly — and the open row
/// vocabulary never widens the closed category set.
#[test]
fn accept_f50_a_forced_airframes_are_a_first_class_row() {
    let binding = ready_binding("M01", "Synthetic One", Progression::known(Vec::new()));
    let Some(CategoryState::Rows(rows)) = binding.category(BindingCategory::Actors) else {
        panic!("the fixture records its actors as rows");
    };
    let forced = rows
        .iter()
        .find(|row| row.role() == "forced_airframe")
        .expect("a forced airframe row is recorded");
    assert_eq!(
        forced
            .content_target()
            .expect("the forced airframe row is bound")
            .kind(),
        ContentKind::Airframe
    );
    assert_eq!(
        binding
            .cells()
            .map(|(category, _)| category)
            .collect::<Vec<_>>(),
        BindingCategory::ALL.to_vec(),
        "a binding is measured against every required category, in report order"
    );
}

/// Every required category has one stable label that maps back to it, so a
/// report written as text can be read again without a second vocabulary.
#[test]
fn accept_f50_a_category_labels_round_trip() {
    for category in BindingCategory::ALL {
        assert_eq!(
            BindingCategory::from_label(category.label()),
            Some(*category),
            "{} maps back to itself",
            category.label()
        );
    }
    assert_eq!(BindingCategory::from_label("not_a_category"), None);
}

/// A reference/evidence link resolves to an evidence claim rather than to a
/// content id: the rows of the `reference_evidence` category have their own
/// target type.
#[test]
fn accept_f50_a_reference_evidence_rows_carry_evidence_claims() {
    let binding = ready_binding("M01", "Synthetic One", Progression::known(Vec::new()));
    let Some(CategoryState::Rows(rows)) = binding.category(BindingCategory::ReferenceEvidence)
    else {
        panic!("the fixture records its evidence links as rows");
    };
    assert_eq!(rows.len(), 1);
    assert!(
        rows[0].content_target().is_none(),
        "an evidence link is not a content id"
    );
    assert_eq!(
        rows[0]
            .evidence_target()
            .expect("the evidence link is bound")
            .as_str(),
        "f50.a.synthetic.evidence"
    );
}

/// The frozen denominator is part of readiness, not a precondition of it. A
/// campaign with nothing declared, and a campaign whose recorded missions are
/// not all in the declared denominator, are both unready even when every
/// recorded cell is complete — that is the "filtering to the working subset"
/// failure spec F50 non-negotiable behavior 5 forbids.
#[test]
fn accept_f50_a_readiness_requires_a_frozen_denominator() {
    // Nothing declared at all: there is no denominator to be complete
    // against, so the aggregate is not ready.
    let empty = CampaignBindings::new().coverage();
    assert_eq!(empty.total_missions, 0);
    assert_eq!(empty.declared_missions, 0);
    assert!(
        !empty.is_ready(),
        "a campaign with no declared denominator is never ready"
    );

    // Every cell complete, but the recorded set is not the declared one.
    let mut undeclared = CampaignBindings::new();
    for binding in [
        ready_binding("M01", "One", Progression::known(Vec::new())),
        ready_binding("M02", "Two", Progression::known(Vec::new())),
        ready_binding("M03", "Three", Progression::known(Vec::new())),
    ] {
        undeclared
            .insert(binding)
            .expect("the synthetic binding records");
    }
    let report = undeclared.coverage();
    assert_eq!(report.total_missions, 3);
    assert_eq!(report.declared_missions, 0);
    assert_eq!(report.complete_cells, report.cells);
    assert_eq!(report.subsystem_unresolved, 0);
    assert_eq!(report.progression_unknown, 0);
    assert!(
        !report.is_ready(),
        "every cell complete is not readiness while the denominator is unfrozen"
    );

    // Declaring all but one still leaves the aggregate short of its baseline.
    undeclared
        .declare(&label("M01"))
        .expect("a recorded mission joins the denominator");
    undeclared
        .declare(&label("M02"))
        .expect("a recorded mission joins the denominator");
    let partial = undeclared.coverage();
    assert_eq!(partial.declared_missions, 2);
    assert_eq!(partial.total_missions, 3);
    assert!(!partial.is_ready(), "a partial denominator is not ready");
    assert_eq!(partial.discovered_extra(), 1);

    // Declaring the last one freezes the denominator and the campaign is ready.
    undeclared
        .declare(&label("M03"))
        .expect("a recorded mission joins the denominator");
    let frozen = undeclared.coverage();
    assert_eq!(frozen.declared_missions, 3);
    assert_eq!(frozen.discovered_extra(), 0);
    assert_eq!(frozen.declared_cells(), frozen.cells);
    assert!(
        frozen.is_ready(),
        "a fully bound campaign over a frozen denominator is ready"
    );
}
