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
use cs_content::ai::DifficultyTier;
use cs_content::instant_action::{
    CustomScenarioDraft, RespawnBudget, RosterSlot, SYNTHETIC_IA_AIRFRAME_HEAVY,
    SYNTHETIC_IA_AIRFRAME_INTERCEPTOR, SYNTHETIC_IA_LOADOUT_HEAVY, SYNTHETIC_IA_LOADOUT_LIGHT,
    SYNTHETIC_IA_LOADOUT_UNSUPPORTED, ScenarioSchemaError, ScenarioSeed, ScenarioSide, TieOutcome,
    VictoryCondition, VictoryRules, synthetic_actor, synthetic_custom_request,
    synthetic_difficulty, synthetic_instant_action_catalog,
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

    let more_seats = lower_custom(&catalog, base_draft(None).with_players(2)).unwrap();
    assert_eq!(
        diff_scenarios(&before, &more_seats),
        vec![ScenarioChange::Players]
    );

    let harder = lower_custom(
        &catalog,
        base_draft(None).with_difficulty(synthetic_difficulty(DifficultyTier::Hard)),
    )
    .unwrap();
    assert_eq!(
        diff_scenarios(&before, &harder),
        vec![ScenarioChange::Difficulty]
    );

    // The two authored presets are the fixture's other authored dimensions:
    // they differ in every scenario-level dimension the diff reports except
    // the player count, which `lower_preset` reads as single-player for both.
    let presets = catalog.presets();
    let a = lower_preset(&catalog, presets[0].id()).unwrap();
    let b = lower_preset(&catalog, presets[1].id()).unwrap();
    let changes = diff_scenarios(&a, &b);
    for expected in [
        ScenarioChange::Subject,
        ScenarioChange::World,
        ScenarioChange::Environment,
        ScenarioChange::Rules,
        ScenarioChange::Difficulty,
        ScenarioChange::Seed,
    ] {
        assert!(changes.contains(&expected), "{expected:?} not reported");
    }
    assert!(!changes.contains(&ScenarioChange::Players));
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

/// The replacement budget is declared **per side**, so each coalition's
/// spending counts against its own replacements and nobody else's.
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

    // The player's side is wiped while the opposition is still flying, but
    // neither the player's nor the allies' own budget is spent: the scenario
    // runs on. The two replacements the *enemy* side spent do not spend
    // theirs.
    let running = ScenarioSnapshot::new(9, alive(&[(ScenarioSide::Enemy, 1)]))
        .with_respawns_used(ScenarioSide::Enemy, 2);
    assert_eq!(
        evaluate_outcome(&scenario, &running),
        None,
        "the player's side still has replacements left"
    );

    // The same picture with both friendly sides' budgets spent is terminal:
    // spending is counted per side, and neither side of the coalition can
    // field an actor any more.
    let lost = ScenarioSnapshot::new(9, alive(&[(ScenarioSide::Enemy, 1)]))
        .with_respawns_used(ScenarioSide::Player, 2)
        .with_respawns_used(ScenarioSide::Ally, 2);
    assert_eq!(
        evaluate_outcome(&scenario, &lost).map(|o| o.result()),
        Some(ScenarioResult::Defeat)
    );

    // The opposition wiped out with its budget spent is a victory even while
    // the player's side still has replacements to call on.
    let won = ScenarioSnapshot::new(9, alive(&[(ScenarioSide::Player, 0)]))
        .with_respawns_used(ScenarioSide::Enemy, 2);
    assert_eq!(
        evaluate_outcome(&scenario, &won).map(|o| o.result()),
        Some(ScenarioResult::Victory)
    );
}

/// A side the roster does not fly has no actor to replace, so its untouched
/// budget is not a reserve for the coalition.
#[test]
fn accept_f49_b_a_side_the_roster_does_not_fly_is_not_a_reserve() {
    let catalog = synthetic_instant_action_catalog();
    let request = synthetic_custom_request();
    let without_allies: Vec<_> = request
        .parameters()
        .roster()
        .actors()
        .iter()
        .filter(|actor| actor.side() != ScenarioSide::Ally)
        .cloned()
        .collect();
    let scenario = lower_custom(
        &catalog,
        base_draft(Some(rules(
            VictoryCondition::EliminateEnemies,
            None,
            RespawnBudget::PerSide { per_side: 2 },
        )))
        .with_roster(without_allies),
    )
    .unwrap();

    // The roster flies no ally, so with the player's own budget spent there
    // is nobody left who could replace anything.
    let lost = ScenarioSnapshot::new(6, alive(&[(ScenarioSide::Enemy, 0)]))
        .with_respawns_used(ScenarioSide::Player, 2);
    assert_eq!(
        evaluate_outcome(&scenario, &lost).map(|o| o.result()),
        Some(ScenarioResult::Defeat)
    );

    // The same snapshot with the player's budget untouched still runs.
    assert_eq!(
        evaluate_outcome(
            &scenario,
            &ScenarioSnapshot::new(6, alive(&[(ScenarioSide::Enemy, 0)]))
        ),
        None
    );
}

/// Mutual destruction on one tick satisfies both conditions at once, which is
/// the tie the declared [`TieOutcome`] resolves — for this condition as much
/// as for `LastSideStanding`.
#[test]
fn accept_f49_b_eliminate_enemies_resolves_a_mutual_wipe_with_the_declared_tie() {
    let catalog = synthetic_instant_action_catalog();
    for (tie, expected) in [
        (TieOutcome::Draw, ScenarioResult::Draw),
        (TieOutcome::PlayerFavour, ScenarioResult::Victory),
        (TieOutcome::OppositionFavour, ScenarioResult::Defeat),
    ] {
        let rules = VictoryRules::try_new(
            VictoryCondition::EliminateEnemies,
            RespawnBudget::None,
            None,
            tie,
        )
        .unwrap();
        let scenario = lower_custom(&catalog, base_draft(Some(rules))).unwrap();
        let outcome = evaluate_outcome(&scenario, &ScenarioSnapshot::new(7, alive(&[])))
            .expect("both coalitions destroyed ends the scenario");
        assert_eq!(outcome.result(), expected, "tie outcome {tie}");
    }
}

/// Unaligned traffic is neither side: it never keeps a wiped scenario running
/// and never hands anybody a win.
#[test]
fn accept_f49_b_neutral_traffic_never_decides_an_outcome() {
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

    // Only neutral traffic still flying: both coalitions are gone, so the
    // declared tie decides — neutral actors are not an opposition to wipe.
    let leftovers = evaluate_outcome(
        &scenario,
        &ScenarioSnapshot::new(3, alive(&[(ScenarioSide::Neutral, 0)])),
    )
    .expect("neither coalition is still flying");
    assert_eq!(leftovers.result(), ScenarioResult::Draw);

    // Neutral actors flying beside the player do not make the enemy side
    // "not yet eliminated": the scenario is won as soon as every enemy is out.
    let won = evaluate_outcome(
        &scenario,
        &ScenarioSnapshot::new(
            4,
            alive(&[(ScenarioSide::Player, 0), (ScenarioSide::Neutral, 0)]),
        ),
    )
    .expect("no enemy is left");
    assert_eq!(won.result(), ScenarioResult::Victory);
}
