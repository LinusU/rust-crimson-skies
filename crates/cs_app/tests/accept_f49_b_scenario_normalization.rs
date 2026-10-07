//! Acceptance scenario F49-B: scenario normalization and isolated outcomes.
//!
//! Spec: `specs/F49-instant-action-presets-and-custom-scenarios.md`, stage
//! `### F49-B`. Task test prefix: `accept_f49_b_`.
//!
//! The minimum scenario (AC02) changes one custom roster slot through
//! `CustomScenarioDraft::replace_roster_slot`, lowers both drafts through the
//! production `lower_custom` path and asks `diff_scenarios` what moved: only
//! the intended actor may. The outcome tests drive `evaluate_outcome` on
//! lowered scenarios.
//!
//! Every value is newly authored synthetic fixture data, never original game
//! data; F49-D verifies the original catalog.

use cs_app::ui::instant_action::{
    ActorField, LowerError, ProblemCode, ScenarioChange, ScenarioResult, ScenarioSnapshot,
    diff_scenarios, evaluate_outcome, lower_custom, lower_preset,
};
use cs_content::instant_action::{
    CustomScenarioDraft, RespawnBudget, RosterSlot, SYNTHETIC_IA_AIRFRAME_HEAVY,
    SYNTHETIC_IA_AIRFRAME_INTERCEPTOR, SYNTHETIC_IA_LOADOUT_HEAVY, SYNTHETIC_IA_LOADOUT_LIGHT,
    SYNTHETIC_IA_LOADOUT_UNSUPPORTED, ScenarioSchemaError, ScenarioSeed, ScenarioSide, TieOutcome,
    VictoryCondition, VictoryRules, synthetic_actor, synthetic_custom_request,
    synthetic_instant_action_catalog,
};

/// The valid fixture request as a draft, with a second enemy so a slot change
/// has a same-side neighbour that must not move.
fn base_draft(rules: Option<VictoryRules>) -> CustomScenarioDraft {
    let request = synthetic_custom_request();
    let mut actors = request.parameters().roster().actors().to_vec();
    actors.push(synthetic_actor(
        ScenarioSide::Enemy,
        1,
        SYNTHETIC_IA_AIRFRAME_INTERCEPTOR,
        SYNTHETIC_IA_LOADOUT_LIGHT,
    ));
    CustomScenarioDraft::new()
        .with_subject(request.subject().clone())
        .with_world(request.parameters().world().clone())
        .with_environment(request.parameters().environment().clone())
        .with_roster(actors)
        .with_difficulty(request.parameters().difficulty().clone())
        .with_rules(rules.unwrap_or(*request.parameters().rules()))
        .with_seed(request.parameters().seed())
        .with_players(request.players())
        .with_provenance(request.provenance().clone())
}

fn rules(
    condition: VictoryCondition,
    deadline: Option<u64>,
    respawns: RespawnBudget,
) -> VictoryRules {
    VictoryRules::try_new(condition, respawns, deadline, TieOutcome::Draw)
        .expect("test rules are valid")
}

/// AC02 minimum scenario: one changed roster slot changes exactly that actor.
#[test]
fn accept_f49_b_changing_one_roster_slot_changes_only_that_actor() {
    let catalog = synthetic_instant_action_catalog();
    let before = lower_custom(&catalog, base_draft(None)).expect("the base scenario lowers");

    let changed = base_draft(None)
        .replace_roster_slot(synthetic_actor(
            ScenarioSide::Enemy,
            1,
            SYNTHETIC_IA_AIRFRAME_HEAVY,
            SYNTHETIC_IA_LOADOUT_HEAVY,
        ))
        .expect("enemy slot 1 exists");
    let after = lower_custom(&catalog, changed).expect("the changed scenario lowers");

    assert_eq!(
        diff_scenarios(&before, &after),
        vec![ScenarioChange::ActorChanged {
            side: ScenarioSide::Enemy,
            slot: RosterSlot(1),
            fields: vec![ActorField::Geometry, ActorField::Loadout],
        }],
        "only enemy slot 1 may differ, and only in airframe and loadout"
    );

    // The untouched actors are equal record for record, not merely "not
    // reported": the diff is a claim about the lowered records themselves.
    for (a, b) in before.actors().iter().zip(after.actors()) {
        if (a.side, a.slot) != (ScenarioSide::Enemy, RosterSlot(1)) {
            assert_eq!(a, b, "{} {} must not move", a.side, a.slot);
        }
    }
    assert_eq!(before.actors().len(), after.actors().len());
    assert_eq!(before.seed(), after.seed());
    assert_eq!(before.world(), after.world());
}

/// The diff is not blind to the other dimensions: each reports as itself.
#[test]
fn accept_f49_b_scenario_level_changes_report_as_themselves() {
    let catalog = synthetic_instant_action_catalog();
    let before = lower_custom(&catalog, base_draft(None)).unwrap();
    assert!(diff_scenarios(&before, &before.clone()).is_empty());

    let reseeded =
        lower_custom(&catalog, base_draft(None).with_seed(ScenarioSeed::new(7))).unwrap();
    assert_eq!(
        diff_scenarios(&before, &reseeded),
        vec![ScenarioChange::Seed]
    );

    let new_rules = lower_custom(
        &catalog,
        base_draft(Some(rules(
            VictoryCondition::EliminateEnemies,
            None,
            RespawnBudget::None,
        ))),
    )
    .unwrap();
    assert_eq!(
        diff_scenarios(&before, &new_rules),
        vec![ScenarioChange::Rules]
    );

    // Whole different authored scenarios differ in more than one dimension.
    let presets = catalog.presets();
    let a = lower_preset(&catalog, presets[0].id()).unwrap();
    let b = lower_preset(&catalog, presets[1].id()).unwrap();
    let changes = diff_scenarios(&a, &b);
    assert!(changes.contains(&ScenarioChange::Subject));
}

/// Actors match by identity, so a removed actor does not shift its neighbours.
#[test]
fn accept_f49_b_added_and_removed_actors_are_matched_by_slot_not_position() {
    let catalog = synthetic_instant_action_catalog();
    let full = lower_custom(&catalog, base_draft(None)).unwrap();
    let request = synthetic_custom_request();
    let without = lower_custom(
        &catalog,
        base_draft(None).with_roster(request.parameters().roster().actors().to_vec()),
    )
    .unwrap();
    assert_eq!(
        diff_scenarios(&full, &without),
        vec![ScenarioChange::ActorRemoved {
            side: ScenarioSide::Enemy,
            slot: RosterSlot(1),
        }]
    );
    assert_eq!(
        diff_scenarios(&without, &full),
        vec![ScenarioChange::ActorAdded {
            side: ScenarioSide::Enemy,
            slot: RosterSlot(1),
        }]
    );
}

#[test]
fn accept_f49_b_replacing_a_slot_the_roster_does_not_hold_is_refused() {
    let error = base_draft(None)
        .replace_roster_slot(synthetic_actor(
            ScenarioSide::Enemy,
            9,
            SYNTHETIC_IA_AIRFRAME_INTERCEPTOR,
            SYNTHETIC_IA_LOADOUT_LIGHT,
        ))
        .expect_err("there is no enemy slot 9");
    assert!(matches!(
        error,
        ScenarioSchemaError::NoSuchRosterSlot {
            side: ScenarioSide::Enemy,
            slot: RosterSlot(9)
        }
    ));
    assert!(error.to_string().contains("enemy"), "{error}");

    let empty = CustomScenarioDraft::new()
        .replace_roster_slot(synthetic_actor(
            ScenarioSide::Player,
            0,
            SYNTHETIC_IA_AIRFRAME_INTERCEPTOR,
            SYNTHETIC_IA_LOADOUT_LIGHT,
        ))
        .expect_err("a draft without a roster has no slot to replace");
    assert!(matches!(
        empty,
        ScenarioSchemaError::NoSuchRosterSlot { .. }
    ));
}

/// A changed slot goes through the same validators as any other selection.
#[test]
fn accept_f49_b_a_changed_slot_is_still_validated() {
    let catalog = synthetic_instant_action_catalog();
    let bad = base_draft(None)
        .replace_roster_slot(synthetic_actor(
            ScenarioSide::Enemy,
            1,
            SYNTHETIC_IA_AIRFRAME_INTERCEPTOR,
            SYNTHETIC_IA_LOADOUT_UNSUPPORTED,
        ))
        .unwrap();
    match lower_custom(&catalog, bad) {
        Err(LowerError::Invalid { problems }) => {
            assert!(
                !problems
                    .with_code(ProblemCode::UnsupportedLoadout)
                    .is_empty()
            );
        }
        other => panic!("an unsupported loadout must be refused, got {other:?}"),
    }
}

fn alive(slots: &[(ScenarioSide, u32)]) -> Vec<(ScenarioSide, RosterSlot)> {
    slots.iter().map(|&(s, i)| (s, RosterSlot(i))).collect()
}

#[test]
fn accept_f49_b_eliminate_enemies_ends_only_when_one_side_is_out() {
    let catalog = synthetic_instant_action_catalog();
    let scenario = lower_custom(
        &catalog,
        base_draft(Some(rules(
            VictoryCondition::EliminateEnemies,
            None,
            RespawnBudget::None,
        ))),
    )
    .unwrap();
    use ScenarioSide::{Ally, Enemy, Player};

    let running = ScenarioSnapshot::new(10, alive(&[(Player, 0), (Enemy, 0)]));
    assert_eq!(evaluate_outcome(&scenario, &running), None);

    let won = evaluate_outcome(&scenario, &ScenarioSnapshot::new(20, alive(&[(Ally, 0)])))
        .expect("every enemy destroyed ends the scenario");
    assert_eq!(won.result(), ScenarioResult::Victory);
    assert_eq!(won.ended_tick(), 20);
    assert_eq!(won.subject(), scenario.subject());
    assert_eq!(won.seed(), scenario.seed());

    let lost = evaluate_outcome(&scenario, &ScenarioSnapshot::new(30, alive(&[(Enemy, 1)])))
        .expect("the player's side destroyed ends the scenario");
    assert_eq!(lost.result(), ScenarioResult::Defeat);
}

#[test]
fn accept_f49_b_last_side_standing_applies_the_declared_tie_outcome() {
    let catalog = synthetic_instant_action_catalog();
    for (tie, expected) in [
        (TieOutcome::Draw, ScenarioResult::Draw),
        (TieOutcome::PlayerFavour, ScenarioResult::Victory),
        (TieOutcome::OppositionFavour, ScenarioResult::Defeat),
    ] {
        let rules = VictoryRules::try_new(
            VictoryCondition::LastSideStanding,
            RespawnBudget::None,
            None,
            tie,
        )
        .unwrap();
        let scenario = lower_custom(&catalog, base_draft(Some(rules))).unwrap();
        let outcome = evaluate_outcome(&scenario, &ScenarioSnapshot::new(5, alive(&[])))
            .expect("both sides emptied ends the scenario");
        assert_eq!(outcome.result(), expected, "tie outcome {tie}");
    }
}

#[test]
fn accept_f49_b_survive_to_deadline_wins_at_the_deadline_and_not_before() {
    let catalog = synthetic_instant_action_catalog();
    let scenario = lower_custom(
        &catalog,
        base_draft(Some(rules(
            VictoryCondition::SurviveToDeadline,
            Some(100),
            RespawnBudget::None,
        ))),
    )
    .unwrap();
    let flying = alive(&[(ScenarioSide::Player, 0), (ScenarioSide::Enemy, 0)]);
    assert_eq!(
        evaluate_outcome(&scenario, &ScenarioSnapshot::new(99, flying.clone())),
        None
    );
    let outcome = evaluate_outcome(&scenario, &ScenarioSnapshot::new(100, flying)).unwrap();
    assert_eq!(outcome.result(), ScenarioResult::Victory);

    let dead = evaluate_outcome(
        &scenario,
        &ScenarioSnapshot::new(50, alive(&[(ScenarioSide::Enemy, 0)])),
    )
    .unwrap();
    assert_eq!(dead.result(), ScenarioResult::Defeat);
}

#[test]
fn accept_f49_b_a_side_with_replacements_left_is_not_yet_out() {
    let catalog = synthetic_instant_action_catalog();
    let scenario = lower_custom(
        &catalog,
        base_draft(Some(rules(
            VictoryCondition::EliminateEnemies,
            None,
            RespawnBudget::PerSide { per_side: 2 },
        ))),
    )
    .unwrap();
    let wiped = alive(&[(ScenarioSide::Player, 0)]);
    assert_eq!(
        evaluate_outcome(&scenario, &ScenarioSnapshot::new(9, wiped.clone())),
        None,
        "a replacement is still available"
    );
    let spent = ScenarioSnapshot::new(9, wiped).with_respawns_used(2);
    assert_eq!(
        evaluate_outcome(&scenario, &spent).map(|o| o.result()),
        Some(ScenarioResult::Victory)
    );
}
