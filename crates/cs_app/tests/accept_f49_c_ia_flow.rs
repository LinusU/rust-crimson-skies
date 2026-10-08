//! Acceptance scenario F49-C: the Instant Action selection, customization
//! and loadout screens, and the session they launch.
//!
//! Spec: `specs/F49-instant-action-presets-and-custom-scenarios.md`, stage
//! `### F49-C`. Task test prefix: `accept_f49_c_`.
//!
//! The minimum scenario (AC03) is
//! `accept_f49_c_completing_and_retrying_ia_leaves_campaign_unchanged`: a real
//! `CampaignState` is taken to a nonzero balance and a moved progression, the
//! whole Instant Action path runs (select -> launch -> complete -> retry ->
//! complete -> finish, twice), and the campaign snapshot must be equal field
//! for field afterwards — with a control that applies a real campaign outcome
//! *afterwards*, so the equality is proved capable of failing. The rest of the
//! file covers the stage's own contracts: a refused launch propagates and
//! starts no session, a retry restores the authored scenario in a new
//! generation, a report from another generation is refused, a draft is
//! discarded only after confirmation, every visible dimension edit reaches the
//! launched scenario, the record scope refuses a foreign profile and a replay,
//! and the state table is total.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data; F49-D verifies the original catalog.

use cs_app::campaign::lower_campaign;
use cs_app::ui::instant_action::{
    ActorField, CustomDimension, IA_TABLE, IaAction, IaActionKind, IaEdit, IaEffect, IaRecordBook,
    IaRecordEntry, IaRecordError, IaRefusal, IaScreen, InstantActionFlow, LowerError,
    LoweredScenario, ProblemCode, ScenarioChange, ScenarioResult, ScenarioSelection,
    ScenarioSnapshot, diff_scenarios, evaluate_outcome, lower_preset, report_problems, rows_on,
};
use cs_content::ai::DifficultyTier;
use cs_content::campaign::declared_synthetic_campaign;
use cs_content::environment::EnvironmentId;
use cs_content::instant_action::{
    CustomScenarioDraft, RosterSlot, SYNTHETIC_IA_LOADOUT_UNSUPPORTED, ScenarioActorSpec,
    ScenarioSeed, ScenarioSide, VictoryRules, synthetic_dogfight_preset,
    synthetic_instant_action_catalog,
};
use cs_content::world::WorldId;
use cs_sim::campaign::{
    CampaignGraph, CampaignNodeKey, CampaignRunId, CampaignState, DifficultyId, EventKey,
    MissionOutcome, Outcome, OutcomeAuthority, OutcomeId, ProfileId, SessionGeneration, SymbolId,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Resolved};

/// The profile every flow and the campaign in these tests run under.
fn profile() -> ProfileId {
    ProfileId::new("pilot.nathan").expect("a valid profile id")
}

/// The `ia_scenario` identity a custom scenario in these tests runs under.
fn custom_subject() -> ContentId {
    ContentId::from_source(
        ContentKind::IaScenario,
        "synthetic.fixture_ia_scenario_custom",
    )
    .expect("a valid scenario id")
}

/// The fixture's dogfight preset: `EliminateEnemies` with no replacement
/// budget, which is what this file's outcome tests end a session on. The id
/// is checked against the production row builder, so it is one the catalog
/// really lists and not a name typed into the test.
fn dogfight_preset(flow: &InstantActionFlow) -> ContentId {
    let id = synthetic_dogfight_preset().id().clone();
    assert!(
        flow.preset_rows().iter().any(|row| row.preset_id == id),
        "the catalog lists the preset its own fixture declares"
    );
    id
}

/// The `ia_scenario` a preset row runs under.
fn preset_scenario(flow: &InstantActionFlow, preset: &ContentId) -> ContentId {
    flow.preset_rows()
        .iter()
        .find(|row| row.preset_id == *preset)
        .expect("the preset is one of the catalog's rows")
        .scenario_id
        .clone()
}

/// A flow with the first preset selected and its custom draft opened.
fn customizing() -> InstantActionFlow {
    let mut flow = InstantActionFlow::new(profile(), synthetic_instant_action_catalog());
    let preset = dogfight_preset(&flow);
    flow.select_preset(&preset)
        .expect("select accepts a preset");
    flow.apply(IaAction::StartCustom {
        subject: custom_subject(),
    })
    .expect("the custom draft opens from a selected preset");
    assert_eq!(flow.screen(), IaScreen::Customize);
    flow
}

/// Launches whatever the flow holds and returns the scenario it runs.
fn launch(flow: &mut InstantActionFlow) -> LoweredScenario {
    flow.apply(IaAction::Launch).expect("the selection lowers");
    assert_eq!(flow.screen(), IaScreen::Flight);
    flow.running_scenario()
        .expect("a launched session runs a scenario")
        .clone()
}

/// The known value inside a `Resolved`, for reading a selection back out of
/// the draft. The fixture catalog resolves every dimension it offers.
fn known<T: Clone>(value: &Resolved<T>) -> Option<T> {
    match value {
        Resolved::Known(known) => Some(known.value.clone()),
        Resolved::Unknown { .. } => None,
    }
}

/// The first value the flow offers that is not `current`.
fn other<T: Clone + PartialEq>(values: &[T], current: &T) -> Option<T> {
    values.iter().find(|value| *value != current).cloned()
}

/// The catalog's other world, relative to what the draft holds.
fn other_world(flow: &InstantActionFlow) -> WorldId {
    let draft = flow.draft().expect("a draft is open");
    let current = known(draft.world().expect("the seeded world")).expect("it resolves");
    flow.dimensions()
        .iter()
        .find_map(|dimension| match dimension {
            CustomDimension::World(values) => other(values, &current),
            _ => None,
        })
        .expect("the fixture offers a second world")
}

/// The catalog's other environment.
fn other_environment(flow: &InstantActionFlow) -> EnvironmentId {
    let draft = flow.draft().expect("a draft is open");
    let current = known(draft.environment().expect("the seeded environment")).expect("it resolves");
    flow.dimensions()
        .iter()
        .find_map(|dimension| match dimension {
            CustomDimension::Environment(values) => other(values, &current),
            _ => None,
        })
        .expect("the fixture offers a second environment")
}

/// The catalog's other skill tier.
fn other_difficulty(flow: &InstantActionFlow) -> DifficultyTier {
    let draft = flow.draft().expect("a draft is open");
    let current = draft.difficulty().expect("the seeded tier").tier();
    flow.dimensions()
        .iter()
        .find_map(|dimension| match dimension {
            CustomDimension::Difficulty(values) => other(values, &current),
            _ => None,
        })
        .expect("the fixture offers a second tier")
}

/// The victory rules with the catalog's other condition, keeping the
/// respawn budget and the tie the draft declares, and giving a deadline only
/// to the condition that ends on one.
fn other_rules(flow: &InstantActionFlow) -> VictoryRules {
    let draft = flow.draft().expect("a draft is open");
    let current = *draft.rules().expect("the seeded rules");
    let condition = flow
        .dimensions()
        .iter()
        .find_map(|dimension| match dimension {
            CustomDimension::Victory(values) => other(values, &current.condition()),
            _ => None,
        })
        .expect("the fixture offers a second victory condition");
    VictoryRules::try_new(
        condition,
        current.respawns(),
        condition.needs_deadline().then_some(400),
        current.tie_outcome(),
    )
    .expect("the alternative rules are valid")
}

/// The roster slot `(side, slot)` of the open draft.
fn held_actor(flow: &InstantActionFlow, side: ScenarioSide, slot: RosterSlot) -> ScenarioActorSpec {
    flow.draft()
        .expect("a draft is open")
        .roster()
        .expect("the seeded roster")
        .iter()
        .find(|actor| actor.side() == side && actor.slot() == slot)
        .expect("the preset holds that slot")
        .clone()
}

/// The catalog's other airframe, relative to what `slot` holds.
fn other_airframe(flow: &InstantActionFlow, side: ScenarioSide, slot: RosterSlot) -> ContentId {
    let current = known(held_actor(flow, side, slot).airframe()).expect("it resolves");
    flow.dimensions()
        .iter()
        .find_map(|dimension| match dimension {
            CustomDimension::Airframe(values) => other(values, &current),
            _ => None,
        })
        .expect("the fixture offers a second airframe")
}

/// The catalog's other loadout, relative to what `slot` holds.
fn other_loadout(flow: &InstantActionFlow, side: ScenarioSide, slot: RosterSlot) -> ContentId {
    let current = known(held_actor(flow, side, slot).loadout()).expect("it resolves");
    flow.dimensions()
        .iter()
        .find_map(|dimension| match dimension {
            CustomDimension::Loadout(values) => other(values, &current),
            _ => None,
        })
        .expect("the fixture offers a second loadout")
}

// --------------------------------------------------------------- campaign ---

/// One campaign outcome for the fixture campaign.
fn campaign_victory(session: u32, sequence: u32, node: &str, score: u64) -> MissionOutcome {
    MissionOutcome {
        id: OutcomeId {
            profile: profile(),
            run: CampaignRunId::new("run.one").expect("a valid run id"),
            session: SessionGeneration(session),
            terminal_event: EventKey {
                session: SessionGeneration(session),
                tick: Tick(480),
                source: SymbolId(7),
                sequence,
            },
        },
        node: CampaignNodeKey::new(node).expect("a valid node key"),
        outcome: Outcome::Succeeded,
        score,
        authority: OutcomeAuthority::Authorized,
    }
}

/// A campaign that has already been played: nonzero cash, a moved selection
/// and a raised revision, so "unchanged" is a real claim about it.
fn live_campaign() -> (CampaignGraph, CampaignState) {
    let graph = lower_campaign(&declared_synthetic_campaign()).expect("the fixture lowers");
    let mut state = CampaignState::begin(
        profile(),
        CampaignRunId::new("run.one").expect("a valid run id"),
        DifficultyId::new("standard").expect("a valid difficulty id"),
        &graph,
    );
    state
        .apply_outcome(&graph, &campaign_victory(1, 0, "m01", 900))
        .expect("the first mission outcome commits");
    assert_eq!(
        state.currency(),
        500,
        "the campaign has a balance to protect"
    );
    (graph, state)
}

// ----------------------------------------------------------------- tests ---

/// **AC03 — the stage's minimum scenario.** Complete an Instant Action,
/// retry it, complete it again and finish, twice over: the campaign's cash,
/// progression, unlocks, dedup ledger and profile revision must not move,
/// while the Instant Action's own records do.
#[test]
fn accept_f49_c_completing_and_retrying_ia_leaves_campaign_unchanged() {
    let (graph, mut campaign) = live_campaign();
    let before = campaign.snapshot();

    let mut flow = InstantActionFlow::new(profile(), synthetic_instant_action_catalog());
    let preset = dogfight_preset(&flow);
    let subject = preset_scenario(&flow, &preset);
    flow.select_preset(&preset).expect("the preset selects");

    // First session: complete it as a defeat.
    let first = flow.apply(IaAction::Launch).expect("the preset lowers");
    assert_eq!(
        first.effects,
        vec![IaEffect::BeginSession {
            generation: SessionGeneration(1),
            subject: subject.clone(),
        }],
        "launching begins exactly one session generation"
    );
    let generation = flow.generation().expect("a session is running");
    flow.report(
        generation,
        ScenarioSnapshot::new(9, vec![(ScenarioSide::Enemy, RosterSlot(1))]),
    )
    .expect("the player's side is destroyed");
    assert_eq!(
        flow.outcome().expect("an outcome").result(),
        ScenarioResult::Defeat
    );

    // Retry it and complete it as a victory: a new generation, the authored
    // scenario again.
    let retried = flow
        .apply(IaAction::Retry)
        .expect("a finished session retries");
    assert_eq!(
        retried.effects,
        vec![
            IaEffect::EndSession {
                generation: SessionGeneration(1)
            },
            IaEffect::BeginSession {
                generation: SessionGeneration(2),
                subject: subject.clone(),
            },
        ],
        "a retry ends the old generation before it begins the next"
    );
    let generation = flow.generation().expect("the retry runs");
    flow.report(
        generation,
        ScenarioSnapshot::new(20, vec![(ScenarioSide::Player, RosterSlot(0))]),
    )
    .expect("every enemy is destroyed");
    assert_eq!(
        flow.outcome().expect("an outcome").result(),
        ScenarioResult::Victory
    );

    // Finish: the run is recorded in the profile's own scope and the screens
    // ask to leave for the menu.
    let finished = flow.apply(IaAction::Finish).expect("an outcome can settle");
    assert_eq!(finished.to, IaScreen::Select);
    assert_eq!(
        finished.effects,
        vec![
            IaEffect::EndSession {
                generation: SessionGeneration(2)
            },
            IaEffect::LeaveToMenu,
        ]
    );
    assert_eq!(flow.records().len(), 1);
    assert!(flow.generation().is_none(), "finish tears the session down");
    assert!(flow.outcome().is_none(), "finish consumes the outcome");

    // A second full session records as the second attempt of the same
    // subject, in a generation that was never reused.
    flow.select_preset(&preset)
        .expect("the preset selects again");
    flow.apply(IaAction::Launch)
        .expect("the preset lowers again");
    let generation = flow.generation().expect("a session is running");
    flow.report(
        generation,
        ScenarioSnapshot::new(30, vec![(ScenarioSide::Player, RosterSlot(0))]),
    )
    .expect("the scenario ends");
    flow.apply(IaAction::Finish)
        .expect("the second run settles");

    let entries = flow.records().entries();
    assert_eq!(entries.len(), 2, "both finished sessions are recorded");
    assert_eq!(entries[0].attempt(), 1);
    assert_eq!(entries[1].attempt(), 2);
    assert_ne!(
        entries[0].generation(),
        entries[1].generation(),
        "each run is a different session generation"
    );
    // The session that is live when Finish is pressed is the one that is
    // settled: the defeat was superseded by its own retry and is not a run
    // that ever finished, so the record scope holds the two finished runs
    // (the retried defeat is recorded nowhere — a designed choice, noted in
    // `docs/findings/2026-10-08-f49-c-instant-action-screen-flow.md`).
    assert_eq!(entries[0].generation(), SessionGeneration(2));
    assert_eq!(entries[0].result(), ScenarioResult::Victory);
    assert_eq!(entries[1].result(), ScenarioResult::Victory);
    assert_eq!(entries[0].subject(), &subject);

    // The campaign never moved: cash, progression, unlocks, the dedup ledger
    // and the profile revision are all exactly where they were.
    assert_eq!(
        campaign.snapshot(),
        before,
        "an Instant Action run may not move campaign cash or progression"
    );

    // Control: the same campaign object still moves when a *campaign*
    // outcome is applied, so the comparison above is not vacuous.
    campaign
        .apply_outcome(&graph, &campaign_victory(2, 1, "m02", 400))
        .expect("a campaign outcome still commits");
    assert_ne!(
        campaign.snapshot().revision,
        before.revision,
        "the control proves the snapshot comparison can fail"
    );
}

/// A refusal propagates verbatim, the screen keeps the player where the
/// problem can be fixed, and no session generation is spent.
#[test]
fn accept_f49_c_a_refused_launch_propagates_and_starts_no_session() {
    let mut flow = InstantActionFlow::new(profile(), synthetic_instant_action_catalog());

    // An id the catalog does not hold.
    let unknown =
        ContentId::from_source(ContentKind::IaPreset, "synthetic.fixture_ia_missing_preset")
            .expect("a valid preset id");
    flow.select_preset(&unknown)
        .expect("the selection is not checked early");
    let refusal = flow
        .apply(IaAction::Launch)
        .expect_err("the catalog holds no such preset");
    assert_eq!(refusal.code(), "not_lowerable");
    assert!(matches!(
        &refusal,
        IaRefusal::Lower(LowerError::UnknownPreset { preset }) if *preset == unknown
    ));
    assert_eq!(flow.screen(), IaScreen::Select, "the screen did not move");
    assert!(flow.generation().is_none(), "no session was started");
    assert!(
        matches!(flow.last_error(), Some(LowerError::UnknownPreset { .. })),
        "the screen can render the refusal"
    );

    // A customization the option table refuses: every problem reported, not
    // one per retry, and still no session.
    let mut flow = customizing();
    flow.edit(IaEdit::SlotLoadout {
        side: ScenarioSide::Player,
        slot: RosterSlot(0),
        loadout: ContentId::from_source(ContentKind::Loadout, SYNTHETIC_IA_LOADOUT_UNSUPPORTED)
            .expect("a valid loadout id"),
    })
    .expect("the edit itself is well formed");
    let refusal = flow
        .apply(IaAction::Launch)
        .expect_err("the option table refuses it");
    let IaRefusal::Lower(LowerError::Invalid { problems }) = &refusal else {
        panic!("an unsupported loadout must be refused as invalid, got {refusal:?}");
    };
    assert!(
        !problems
            .with_code(ProblemCode::UnsupportedLoadout)
            .is_empty(),
        "the problem names the loadout"
    );
    let reported = LowerError::Invalid {
        problems: problems.clone(),
    };
    assert!(
        !report_problems(&reported).is_empty(),
        "the screen can list every problem at once"
    );
    assert!(
        refusal.to_string().contains("loadout"),
        "the message is actionable, got {refusal}"
    );
    assert_eq!(
        flow.screen(),
        IaScreen::Customize,
        "the player stays where the problem can be fixed"
    );
    assert!(flow.generation().is_none(), "no session was started");
    assert!(flow.outcome().is_none());
}

/// Retry restores the authored initial state — the same lowering, the same
/// seed and every actor back — in a generation that is never reused, and a
/// callback from a replaced generation cannot settle it.
#[test]
fn accept_f49_c_retry_restores_the_authored_scenario_in_a_new_generation() {
    let mut flow = InstantActionFlow::new(profile(), synthetic_instant_action_catalog());
    let preset = dogfight_preset(&flow);
    flow.select_preset(&preset).expect("the preset selects");
    flow.apply(IaAction::Launch).expect("the preset lowers");
    let staged = flow
        .authored_snapshot()
        .expect("a session stages its roster");
    let authored = flow.running_scenario().expect("a session runs").clone();
    assert_eq!(
        flow.seed(),
        Some(authored.seed()),
        "the running scenario's seed is what the screen shows"
    );

    // The run ends with most of the roster destroyed.
    flow.report(
        SessionGeneration(1),
        ScenarioSnapshot::new(11, vec![(ScenarioSide::Enemy, RosterSlot(0))]),
    )
    .expect("the player's side is wiped");
    assert_eq!(flow.outcome().expect("an outcome").ended_tick(), 11);

    // The results screen accepts no report at all.
    assert_eq!(
        flow.report(SessionGeneration(1), ScenarioSnapshot::new(12, Vec::new())),
        Err(IaRefusal::WrongScreen {
            expected: IaScreen::Flight,
            actual: IaScreen::Results,
        })
    );

    let transition = flow
        .apply(IaAction::Retry)
        .expect("a finished session retries");
    assert_eq!(transition.to, IaScreen::Flight);
    assert_eq!(
        transition.effects,
        vec![
            IaEffect::EndSession {
                generation: SessionGeneration(1)
            },
            IaEffect::BeginSession {
                generation: SessionGeneration(2),
                subject: authored.subject().clone(),
            },
        ]
    );
    assert_eq!(flow.generation(), Some(SessionGeneration(2)));
    assert_eq!(
        flow.running_scenario(),
        Some(&authored),
        "retry runs the authored lowering again, not the depleted one"
    );
    assert_eq!(
        flow.authored_snapshot(),
        Some(staged.clone()),
        "every actor is back at tick 0"
    );
    assert!(
        flow.outcome().is_none(),
        "the previous run's outcome does not belong to the new session"
    );

    // A delayed callback from the replaced generation is refused by name
    // instead of settling the run that is actually flying.
    let refusal = flow
        .report(
            SessionGeneration(1),
            ScenarioSnapshot::new(12, vec![(ScenarioSide::Enemy, RosterSlot(0))]),
        )
        .expect_err("generation 1 is gone");
    assert_eq!(
        refusal,
        IaRefusal::StaleGeneration {
            expected: SessionGeneration(2),
            got: SessionGeneration(1),
        }
    );
    assert_eq!(
        flow.screen(),
        IaScreen::Flight,
        "a stale report changes nothing"
    );

    // While it runs, a report that ends nothing is refused rather than
    // treated as an outcome.
    assert_eq!(
        flow.report(
            SessionGeneration(2),
            ScenarioSnapshot::new(
                13,
                vec![
                    (ScenarioSide::Player, RosterSlot(0)),
                    (ScenarioSide::Enemy, RosterSlot(1)),
                ],
            ),
        ),
        Err(IaRefusal::StillRunning)
    );

    // Abandoning ends the live generation without recording anything.
    let abandoned = flow
        .apply(IaAction::Back)
        .expect("a live session can be left");
    assert_eq!(
        abandoned.effects,
        vec![IaEffect::EndSession {
            generation: SessionGeneration(2)
        }]
    );
    assert_eq!(abandoned.to, IaScreen::Select);
    assert!(flow.generation().is_none());
    assert!(
        flow.records().is_empty(),
        "an abandoned run is not a settled run"
    );
}

/// Back from a draft discards it, but only after the prompt the contract
/// asks for has been answered; a clean draft leaves at once.
#[test]
fn accept_f49_c_back_discards_a_draft_only_after_confirmation() {
    let mut flow = customizing();
    let preset = dogfight_preset(&flow);
    assert!(!flow.draft_is_dirty());

    // A clean draft leaves immediately: nothing was changed to lose.
    let clean = flow.apply(IaAction::Back).expect("a clean draft leaves");
    assert_eq!(clean.to, IaScreen::Select);
    assert_eq!(clean.effects, vec![IaEffect::DiscardDraft]);
    assert!(flow.draft().is_none());

    // An edited draft asks first, and the prompt gates everything else.
    flow.apply(IaAction::StartCustom {
        subject: custom_subject(),
    })
    .expect("the draft reopens");
    let airframe = other_airframe(&flow, ScenarioSide::Enemy, RosterSlot(1));
    flow.edit(IaEdit::SlotAirframe {
        side: ScenarioSide::Enemy,
        slot: RosterSlot(1),
        airframe,
    })
    .expect("the edit applies");
    assert!(flow.draft_is_dirty());

    let asked = flow.apply(IaAction::Back).expect("a dirty draft asks");
    assert_eq!(asked.from, IaScreen::Customize);
    assert_eq!(asked.to, IaScreen::Customize, "the draft is still open");
    assert_eq!(asked.effects, vec![IaEffect::AskDiscard]);
    assert!(flow.pending_discard());

    assert_eq!(
        flow.apply(IaAction::Launch),
        Err(IaRefusal::ConfirmationPending),
        "the prompt gates every other action"
    );
    assert_eq!(
        flow.apply(IaAction::Back),
        Err(IaRefusal::ConfirmationPending),
        "including another Back"
    );
    assert_eq!(
        flow.select_preset(&preset),
        Err(IaRefusal::ConfirmationPending),
        "including a new selection"
    );

    // Keep editing returns the screen to normal editing, edits intact.
    let kept = flow
        .apply(IaAction::KeepEditing)
        .expect("the prompt answers");
    assert_eq!(kept.to, IaScreen::Customize);
    assert!(!flow.pending_discard());
    assert!(flow.draft().is_some(), "keeping the draft keeps the edits");
    assert_eq!(
        flow.apply(IaAction::KeepEditing),
        Err(IaRefusal::NoPendingDiscard),
        "there is no prompt to answer any more"
    );

    // Confirming the discard drops the draft and puts the preset selection
    // back exactly as it was.
    flow.apply(IaAction::Back).expect("the prompt reopens");
    let discarded = flow
        .apply(IaAction::ConfirmDiscard)
        .expect("the prompt answers");
    assert_eq!(discarded.to, IaScreen::Select);
    assert_eq!(discarded.effects, vec![IaEffect::DiscardDraft]);
    assert!(flow.draft().is_none());
    assert!(!flow.draft_is_dirty());
    assert_eq!(flow.preset_id(), Some(&preset));
    assert_eq!(
        flow.apply(IaAction::ConfirmDiscard),
        Err(IaRefusal::NoPendingDiscard),
        "there is no prompt to answer any more"
    );
}

/// Every visible customization option reaches the scenario that is actually
/// launched (F49 non-negotiable 5), and each reaches it as itself.
#[test]
fn accept_f49_c_every_visible_dimension_edit_reaches_the_launched_scenario() {
    let mut base = customizing();
    let baseline = launch(&mut base);

    // World.
    let mut flow = customizing();
    let world = other_world(&flow);
    flow.edit(IaEdit::World(world))
        .expect("the world is offered");
    assert_eq!(
        diff_scenarios(&baseline, &launch(&mut flow)),
        vec![ScenarioChange::World]
    );

    // Environment.
    let mut flow = customizing();
    let environment = other_environment(&flow);
    flow.edit(IaEdit::Environment(environment))
        .expect("the environment is offered");
    assert_eq!(
        diff_scenarios(&baseline, &launch(&mut flow)),
        vec![ScenarioChange::Environment]
    );

    // Skill.
    let mut flow = customizing();
    let tier = other_difficulty(&flow);
    flow.edit(IaEdit::Difficulty(tier))
        .expect("the tier is offered");
    assert_eq!(
        diff_scenarios(&baseline, &launch(&mut flow)),
        vec![ScenarioChange::Difficulty]
    );

    // Rules.
    let mut flow = customizing();
    let rules = other_rules(&flow);
    flow.edit(IaEdit::Rules(rules))
        .expect("the rules are offered");
    assert_eq!(
        diff_scenarios(&baseline, &launch(&mut flow)),
        vec![ScenarioChange::Rules]
    );

    // One slot's airframe: only that actor moves, and only in its geometry.
    let mut flow = customizing();
    let airframe = other_airframe(&flow, ScenarioSide::Enemy, RosterSlot(1));
    flow.edit(IaEdit::SlotAirframe {
        side: ScenarioSide::Enemy,
        slot: RosterSlot(1),
        airframe,
    })
    .expect("the airframe is offered");
    assert_eq!(
        diff_scenarios(&baseline, &launch(&mut flow)),
        vec![ScenarioChange::ActorChanged {
            side: ScenarioSide::Enemy,
            slot: RosterSlot(1),
            fields: vec![ActorField::Geometry],
        }]
    );

    // The same slot's loadout: only that actor moves, and only in its
    // loadout.
    let mut flow = customizing();
    let loadout = other_loadout(&flow, ScenarioSide::Enemy, RosterSlot(1));
    flow.edit(IaEdit::SlotLoadout {
        side: ScenarioSide::Enemy,
        slot: RosterSlot(1),
        loadout: loadout.clone(),
    })
    .expect("the loadout is offered");
    assert_eq!(
        diff_scenarios(&baseline, &launch(&mut flow)),
        vec![ScenarioChange::ActorChanged {
            side: ScenarioSide::Enemy,
            slot: RosterSlot(1),
            fields: vec![ActorField::Loadout],
        }]
    );

    // Editing a slot the roster does not hold is refused before the draft
    // moves, so a loadout control cannot invent a seat.
    let mut flow = customizing();
    let refusal = flow
        .edit(IaEdit::SlotLoadout {
            side: ScenarioSide::Ally,
            slot: RosterSlot(9),
            loadout,
        })
        .expect_err("the roster has no ally slot 9");
    assert_eq!(refusal.code(), "edit_refused");
    assert!(
        refusal.to_string().contains("ally"),
        "the refusal names the side, got {refusal}"
    );
    assert_eq!(
        diff_scenarios(&baseline, &launch(&mut flow)),
        Vec::<ScenarioChange>::new(),
        "a refused edit leaves the launched scenario untouched"
    );
}

/// The record scope is explicit: it refuses another profile's entry and a
/// second settlement of the same session, changing nothing either time.
#[test]
fn accept_f49_c_the_record_scope_refuses_a_foreign_profile_and_a_replay() {
    let catalog = synthetic_instant_action_catalog();
    let preset = synthetic_dogfight_preset().id().clone();
    let scenario = lower_preset(&catalog, &preset).expect("the preset lowers");
    // Both coalitions destroyed on one tick: the declared tie decides.
    let outcome = evaluate_outcome(&scenario, &ScenarioSnapshot::new(7, Vec::new()))
        .expect("an emptied scenario has ended");
    assert_eq!(outcome.result(), ScenarioResult::Draw);

    let mut book = IaRecordBook::new(profile());
    assert!(book.is_empty());
    assert_eq!(book.profile(), &profile());

    // Another pilot's record does not belong to this scope.
    let foreign = ProfileId::new("pilot.mallory").expect("a valid profile id");
    let refusal = book
        .record(IaRecordEntry::new(
            foreign.clone(),
            SessionGeneration(1),
            &outcome,
            1,
        ))
        .expect_err("a record scope is per profile");
    assert_eq!(
        refusal,
        IaRecordError::ForeignProfile {
            expected: profile(),
            offered: foreign,
        }
    );
    assert_eq!(refusal.code(), "foreign_profile");
    assert!(book.is_empty(), "a refused entry writes nothing");

    // The scope's own entry records, carrying the subject, the seed, the
    // result, the generation and the attempt.
    let entry = IaRecordEntry::new(profile(), SessionGeneration(1), &outcome, 1);
    assert_eq!(entry.subject(), scenario.subject());
    assert_eq!(entry.seed(), scenario.seed());
    assert_eq!(entry.ended_tick(), 7);
    assert_eq!(entry.result(), ScenarioResult::Draw);
    assert_eq!(entry.attempt(), 1);
    assert_eq!(entry.profile(), &profile());
    book.record(entry.clone())
        .expect("the scope's own entry records");
    assert_eq!(book.len(), 1);

    // The same session settled twice is refused, so a replay cannot double
    // the record.
    let replay = book
        .record(IaRecordEntry::new(
            profile(),
            SessionGeneration(1),
            &outcome,
            1,
        ))
        .expect_err("one generation settles once");
    assert_eq!(
        replay,
        IaRecordError::DuplicateGeneration {
            generation: SessionGeneration(1),
        }
    );
    assert_eq!(
        book.entries(),
        std::slice::from_ref(&entry),
        "the book is unchanged by the refusal"
    );
}

/// The state table is total: no duplicate row, a row on every screen, every
/// screen reachable from Select and a path back to Select from each.
#[test]
fn accept_f49_c_the_state_table_is_total() {
    let screens = [
        IaScreen::Select,
        IaScreen::Customize,
        IaScreen::Flight,
        IaScreen::Results,
    ];

    for screen in screens {
        // Every screen has at least one row, and every screen — including
        // Select's own — can get back to Select, so no screen is a dead end.
        assert!(
            rows_on(screen).next().is_some(),
            "{screen:?} has no row at all"
        );
        assert!(
            path_to(screen, IaScreen::Select),
            "{screen:?} has no path back to Select"
        );
    }

    // Every (screen, action) pair appears at most once.
    for (index, row) in IA_TABLE.iter().enumerate() {
        for other in &IA_TABLE[index + 1..] {
            assert!(
                !(row.from == other.from && row.action == other.action),
                "duplicate row for {:?}/{:?}",
                row.from,
                row.action
            );
        }
    }

    // Walk the table from Select: every screen must be reached, and every
    // screen must be able to get back to Select.
    let mut reached = vec![IaScreen::Select];
    let mut frontier = vec![IaScreen::Select];
    while let Some(screen) = frontier.pop() {
        for row in rows_on(screen) {
            if !reached.contains(&row.to) {
                reached.push(row.to);
                frontier.push(row.to);
            }
        }
    }
    for screen in screens {
        assert!(
            reached.contains(&screen),
            "{screen:?} is unreachable from Select"
        );
    }
}

/// Whether the table can walk from `from` to `to`.
fn path_to(from: IaScreen, to: IaScreen) -> bool {
    if from == to {
        return true;
    }
    let mut seen = vec![from];
    let mut frontier = vec![from];
    while let Some(screen) = frontier.pop() {
        for row in rows_on(screen) {
            if row.to == to {
                return true;
            }
            if !seen.contains(&row.to) {
                seen.push(row.to);
                frontier.push(row.to);
            }
        }
    }
    false
}

/// A discarded edit never leaks: reopening reseeds the draft from the
/// preset's own authored dimensions.
#[test]
fn accept_f49_c_reopening_after_a_discard_reseeds_from_the_preset() {
    let mut flow = customizing();
    let seeded: CustomScenarioDraft = flow.draft().expect("a draft is open").clone();
    assert!(seeded.is_complete(), "the seeded draft launches as it is");

    let airframe = other_airframe(&flow, ScenarioSide::Enemy, RosterSlot(0));
    flow.edit(IaEdit::SlotAirframe {
        side: ScenarioSide::Enemy,
        slot: RosterSlot(0),
        airframe,
    })
    .expect("the edit applies");
    assert_ne!(
        flow.draft().expect("the edited draft"),
        &seeded,
        "the edit changed the draft"
    );

    flow.apply(IaAction::Back).expect("a dirty draft asks");
    flow.apply(IaAction::ConfirmDiscard)
        .expect("the prompt answers");
    flow.apply(IaAction::StartCustom {
        subject: custom_subject(),
    })
    .expect("the draft reopens");
    assert_eq!(
        flow.draft().expect("the reopened draft"),
        &seeded,
        "the reopened draft is the preset's, not the discarded one"
    );
}

/// Calls that belong to another screen are refused by name, and a flow with
/// nothing selected refuses to launch rather than picking something.
#[test]
fn accept_f49_c_calls_outside_their_screen_are_refused() {
    let mut flow = InstantActionFlow::new(profile(), synthetic_instant_action_catalog());

    assert_eq!(
        flow.edit(IaEdit::Difficulty(DifficultyTier::Hard)),
        Err(IaRefusal::WrongScreen {
            expected: IaScreen::Customize,
            actual: IaScreen::Select,
        })
    );
    assert_eq!(
        flow.apply(IaAction::Finish),
        Err(IaRefusal::NoTransition {
            screen: IaScreen::Select,
            action: IaActionKind::Finish,
        })
    );
    assert_eq!(
        flow.apply(IaAction::Launch),
        Err(IaRefusal::NoSelection),
        "launching without a selection is refused, not defaulted"
    );
    assert_eq!(
        flow.apply(IaAction::StartCustom {
            subject: custom_subject(),
        }),
        Err(IaRefusal::NoPresetSelected),
        "a custom scenario is seeded from a preset the player chose"
    );
    assert!(
        flow.last_error().is_none(),
        "a refusal that never lowered left no lowering error behind"
    );

    assert_eq!(flow.screen(), IaScreen::Select);
    assert_eq!(flow.records().profile(), &profile());
    assert!(
        flow.records().is_empty(),
        "nothing was launched, so nothing was settled"
    );
}

/// The screens hold the F49-A selection type, so they cannot invent a
/// fourth kind of thing to launch, and an untouched scenario has not ended.
#[test]
fn accept_f49_c_the_flow_launches_only_the_f49_a_selection() {
    let mut flow = customizing();
    assert!(
        matches!(flow.selection(), Some(ScenarioSelection::Custom(_))),
        "the customize screen holds a custom draft"
    );

    flow.apply(IaAction::Back).expect("a clean draft leaves");
    assert!(
        matches!(flow.selection(), Some(ScenarioSelection::Preset(_))),
        "the select screen holds a preset"
    );

    let empty = InstantActionFlow::new(profile(), synthetic_instant_action_catalog());
    assert!(empty.selection().is_none());
    assert!(empty.draft().is_none());
    let catalog = synthetic_instant_action_catalog();
    let scenario = lower_preset(&catalog, &dogfight_preset(&empty)).expect("the preset lowers");
    assert_eq!(
        evaluate_outcome(
            &scenario,
            &ScenarioSnapshot::new(
                1,
                vec![
                    (ScenarioSide::Player, RosterSlot(0)),
                    (ScenarioSide::Enemy, RosterSlot(0)),
                ],
            ),
        ),
        None,
        "both coalitions still flying has not ended"
    );
}

/// The displayed seed is the running scenario's own root seed, not one
/// minted for the screen (F49 non-negotiable 4).
#[test]
fn accept_f49_c_the_displayed_seed_is_the_running_scenarios_seed() {
    let mut flow = customizing();
    assert_eq!(flow.seed(), None, "no session runs before a launch");
    flow.apply(IaAction::Launch).expect("the draft lowers");
    let running = flow.running_scenario().expect("a session runs").clone();
    assert_eq!(flow.seed(), Some(running.seed()));

    // The same preset lowered directly carries the same root seed, so the
    // screen shows the scenario's own seed rather than a copy.
    let catalog = synthetic_instant_action_catalog();
    let authored = lower_preset(&catalog, &dogfight_preset(&flow)).expect("the preset lowers");
    assert_eq!(
        flow.seed().map(ScenarioSeed::root),
        Some(authored.seed().root())
    );
}
