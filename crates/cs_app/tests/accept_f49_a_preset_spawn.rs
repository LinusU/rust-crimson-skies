//! Acceptance scenario F49-A: the Instant Action preset and custom-scenario
//! lowering boundary.
//!
//! Spec: `specs/F49-instant-action-presets-and-custom-scenarios.md`, stage
//! `### F49-A`. Task test prefix: `accept_f49_a_`.
//!
//! These tests drive production code only:
//! [`cs_app::ui::instant_action`]'s [`lower_preset`], [`lower_custom`],
//! [`resolve_custom`], [`preset_rows`] and [`custom_dimensions`], the
//! `cs_content::instant_action` schema and fixture catalog they consume, and
//! `cs_sim::allies`' identity constructors the lowering goes through. Nothing
//! here reimplements a roster, a rule or a validation.
//!
//! The AC01 minimum scenario — launch every discovered preset and verify its
//! expected actors, world and rules — runs end to end. Removing the roster
//! lowering, dropping the world, substituting the default airframe for an
//! unmeasured one, ignoring the declared rules or the seed each makes one of
//! these fail.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data: the original 2000 PC Instant Action catalog is unmeasured and
//! F49-D verifies it.

use cs_app::ui::instant_action::{
    CustomDimension, LowerError, ProblemCode, ScenarioSelection, VictoryCondition,
    custom_dimensions, lower_custom, lower_preset, preset_rows, report_problems, resolve_custom,
    wingmate_slot,
};
use cs_content::ai::DifficultyTier;
use cs_content::instant_action::{
    CustomScenarioDraft, RespawnBudget, RosterSlot, ScenarioActorSpec, ScenarioProblemCode,
    ScenarioSide, TieOutcome, VictoryRules, synthetic_actor, synthetic_custom_request,
    synthetic_instant_action_catalog, synthetic_player_faction,
};
use cs_content::pilots::DeclaredSurvivability;
use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;

fn id(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("test id is valid")
}

fn known<T: Clone>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(
        value,
        Provenance::designed(ClaimId::new("f49a.test").expect("test claim is valid")),
    ))
}

fn unknown<T: Clone>(reason: &str) -> Resolved<T> {
    Resolved::unknown(
        ClaimId::new("f49a.test-open").expect("test claim is valid"),
        reason,
    )
    .expect("the test reason is non-empty")
}

/// What the fixture catalog declares one preset must spawn.
///
/// The expectations are written out here rather than read back from the preset,
/// so the test can fail when the lowering drifts from what the preset says
/// instead of comparing the preset to itself.
fn expected_dogfight() -> (
    &'static str,
    usize,
    usize,
    usize,
    usize,
    VictoryCondition,
    u64,
) {
    (
        "synthetic.fixture_ia_coastal",
        1,
        1,
        2,
        0,
        VictoryCondition::EliminateEnemies,
        0x00D0_0F1A_0000_0001,
    )
}

/// AC01 minimum scenario: every preset the catalog discovers launches, and the
/// lowered scenario carries exactly the actors, world and rules that preset
/// declares.
#[test]
fn accept_f49_a_every_discovered_preset_launches_with_its_expected_actors_world_and_rules() {
    let catalog = synthetic_instant_action_catalog();
    let rows = preset_rows(&catalog);
    assert_eq!(
        rows.len(),
        catalog.presets().len(),
        "every catalog preset must be selectable"
    );
    assert!(
        rows.len() >= 2,
        "the fixture catalog must exercise more than one preset"
    );

    for row in &rows {
        let lowered = lower_preset(&catalog, &row.preset_id)
            .unwrap_or_else(|error| panic!("preset {} must launch: {error}", row.preset_id));

        // The scenario identity the row advertises is the identity the plan
        // runs under: a row must never select a different authored scenario.
        assert_eq!(
            lowered.subject(),
            &row.scenario_id,
            "preset {} launched the wrong scenario",
            row.preset_id
        );
        assert_eq!(
            ScenarioSelection::Preset(row.preset_id.clone()).preset_id(),
            Some(&row.preset_id),
            "a preset selection must carry its content id, not a row number"
        );

        let preset = catalog
            .preset(&row.preset_id)
            .expect("a catalog row resolves to its preset");
        let declared = preset.parameters();

        // World and environment.
        assert_eq!(
            &lowered.world().world,
            require(declared.world()).content_id(),
            "preset {} lowered the wrong world",
            row.preset_id
        );
        assert_eq!(
            lowered.world().environment,
            require(declared.environment()).to_string(),
            "preset {} lowered the wrong environment",
            row.preset_id
        );

        // Actors: same count, same (side, slot) order, same geometry, same
        // loadout, same faction.
        let declared_actors = declared.roster().actors();
        assert_eq!(
            lowered.actors().len(),
            declared_actors.len(),
            "preset {} lost or gained an actor",
            row.preset_id
        );
        for (lowered_actor, declared_actor) in lowered.actors().iter().zip(declared_actors.iter()) {
            assert_eq!(lowered_actor.side, declared_actor.side());
            assert_eq!(lowered_actor.slot, declared_actor.slot());
            assert_eq!(
                &lowered_actor.geometry,
                &cs_sim::allies::GeometryId::try_new(require(declared_actor.airframe()).clone())
                    .expect("a declared airframe is a valid geometry id"),
                "preset {} {} lowered the wrong plane",
                row.preset_id,
                declared_actor.slot()
            );
            assert_eq!(
                lowered_actor.loadout,
                *require(declared_actor.loadout()),
                "preset {} {} lowered the wrong loadout",
                row.preset_id,
                declared_actor.slot()
            );
            assert_eq!(
                &lowered_actor.faction,
                &cs_sim::allies::FactionId::try_new(declared_actor.faction().clone())
                    .expect("a declared faction is a valid faction id"),
                "preset {} {} lowered the wrong faction",
                row.preset_id,
                declared_actor.slot()
            );
            assert!(
                lowered_actor.pilot.is_some() || declared_actor.pilot().is_none(),
                "preset {} invented a pilot for {}",
                row.preset_id,
                declared_actor.slot()
            );
        }

        // Rules.
        assert_eq!(lowered.condition(), declared.rules().condition());
        assert_eq!(lowered.respawns(), declared.rules().respawns());
        assert_eq!(lowered.deadline_ticks(), declared.rules().deadline_ticks());
        assert_eq!(lowered.tie_outcome(), declared.rules().tie_outcome());
        assert_eq!(lowered.difficulty(), declared.difficulty().tier());

        // Seed: explicit, never derived from anything else.
        assert_eq!(lowered.seed().root(), declared.seed().root());
        assert_eq!(
            lowered.seed().root(),
            lowered.seed().root(),
            "the seed is a value, not a stream"
        );
    }

    // The two fixture presets really are distinguishable: their worlds, rules
    // and seeds differ, so "launch each preset" is not the same scenario twice.
    let dogfight = lower_preset(
        &catalog,
        &id(ContentKind::IaPreset, "synthetic.fixture_ia_dogfight"),
    )
    .expect("the dogfight preset launches");
    let bomber = lower_preset(
        &catalog,
        &id(ContentKind::IaPreset, "synthetic.fixture_ia_bomber_run"),
    )
    .expect("the bomber-run preset launches");

    let (world, players, allies, enemies, neutrals, condition, seed) = expected_dogfight();
    assert_eq!(
        dogfight.world().world,
        id(ContentKind::World, world),
        "the dogfight preset loads the coastal world"
    );
    assert_eq!(dogfight.players(), players as u8);
    assert_eq!(dogfight.actors_on(ScenarioSide::Player).len(), players);
    assert_eq!(dogfight.actors_on(ScenarioSide::Ally).len(), allies);
    assert_eq!(dogfight.actors_on(ScenarioSide::Enemy).len(), enemies);
    assert_eq!(dogfight.actors_on(ScenarioSide::Neutral).len(), neutrals);
    assert_eq!(dogfight.condition(), condition);
    assert_eq!(dogfight.seed().root(), seed);

    assert_ne!(dogfight.world().world, bomber.world().world);
    assert_ne!(dogfight.condition(), bomber.condition());
    assert_ne!(dogfight.seed(), bomber.seed());
    assert_eq!(
        bomber.deadline_ticks(),
        Some(3_600),
        "the survive-to-deadline preset keeps its deadline"
    );
    assert_eq!(bomber.respawns(), RespawnBudget::PerSide { per_side: 1 });

    // Ally actors take the runtime wingmate slot; enemies and the player do
    // not, so an ally is never silently demoted or double-counted.
    assert_eq!(
        wingmate_slot(dogfight.actors_on(ScenarioSide::Ally)[0]),
        Some(cs_sim::allies::WingmateSlot(0))
    );
    assert_eq!(
        wingmate_slot(dogfight.actors_on(ScenarioSide::Enemy)[0]),
        None
    );
    assert_eq!(
        wingmate_slot(dogfight.actors_on(ScenarioSide::Player)[0]),
        None
    );
}

/// Non-negotiable 1: a preset is not "a campaign launch with rewards off".
/// Each preset keeps its own identity and parameters, so two presets never
/// lower to the same scenario.
#[test]
fn accept_f49_a_presets_keep_their_own_identity_and_parameters() {
    let catalog = synthetic_instant_action_catalog();
    let mut subjects = Vec::new();
    let mut plans = Vec::new();
    for row in preset_rows(&catalog) {
        let lowered = lower_preset(&catalog, &row.preset_id).expect("a preset launches");
        subjects.push(lowered.subject().clone());
        plans.push(format!("{lowered:?}"));
    }
    let unique_subjects: std::collections::BTreeSet<&ContentId> = subjects.iter().collect();
    assert_eq!(
        unique_subjects.len(),
        subjects.len(),
        "two presets must not share one scenario identity"
    );
    let unique_plans: std::collections::BTreeSet<&String> = plans.iter().collect();
    assert_eq!(
        unique_plans.len(),
        plans.len(),
        "two presets must not lower to the same scenario"
    );
}

/// A selection carries content ids only. There is no field on it that could
/// hold campaign cash, ownership or an objective, so an Instant Action run
/// cannot write campaign progression (non-negotiable 3, AC03's precondition).
#[test]
fn accept_f49_a_a_selection_carries_content_ids_and_no_campaign_state() {
    let catalog = synthetic_instant_action_catalog();
    let selection =
        ScenarioSelection::Preset(id(ContentKind::IaPreset, "synthetic.fixture_ia_dogfight"));
    assert_eq!(
        selection.preset_id(),
        Some(&id(ContentKind::IaPreset, "synthetic.fixture_ia_dogfight"))
    );
    assert!(selection.draft().is_none());

    let custom = ScenarioSelection::Custom(Box::new(CustomScenarioDraft::new()));
    assert!(
        custom.preset_id().is_none(),
        "a custom scenario must not borrow a preset identity"
    );
    assert!(custom.draft().is_some());

    // Lowering a selection yields a plan whose only session-visible inputs are
    // the world, the actors, the rules and the seed.
    let lowered = lower_preset(
        &catalog,
        &id(ContentKind::IaPreset, "synthetic.fixture_ia_dogfight"),
    )
    .expect("the fixture preset launches");
    assert_eq!(lowered.players(), 1);
    // Two lowerings of the same selection are byte-identical: the plan is a
    // function of the selection alone.
    let again = lower_preset(
        &catalog,
        &id(ContentKind::IaPreset, "synthetic.fixture_ia_dogfight"),
    )
    .expect("the fixture preset launches");
    assert_eq!(lowered, again, "lowering must be deterministic");
}

/// The valid custom request lowers, and the world, roster, rules and seed it
/// declares are what the plan carries.
#[test]
fn accept_f49_a_a_valid_custom_request_lowers_to_its_declared_scenario() {
    let catalog = synthetic_instant_action_catalog();
    let request = synthetic_custom_request();

    // Report first, lower second: reporting must not need a spawn.
    let resolved = resolve_custom(&catalog, CustomScenarioDraft::new()).err();
    assert!(
        resolved.is_some(),
        "an empty draft must be reported as incomplete"
    );

    let draft = CustomScenarioDraft::new()
        .with_subject(request.subject().clone())
        .with_world(request.parameters().world().clone())
        .with_environment(request.parameters().environment().clone())
        .with_roster(request.parameters().roster().actors().to_vec())
        .with_difficulty(request.parameters().difficulty().clone())
        .with_rules(*request.parameters().rules())
        .with_seed(request.parameters().seed())
        .with_players(request.players())
        .with_provenance(request.provenance().clone());

    let lowered = lower_custom(&catalog, draft).expect("the fixture request lowers");
    assert_eq!(lowered.subject(), request.subject());
    assert_eq!(
        &lowered.world().world,
        require(request.parameters().world()).content_id(),
        "the custom scenario lowered the wrong world"
    );
    assert_eq!(lowered.actors().len(), request.parameters().roster().len());
    assert_eq!(
        lowered.condition(),
        request.parameters().rules().condition()
    );
    assert_eq!(lowered.seed(), request.parameters().seed());
    assert_eq!(lowered.difficulty(), DifficultyTier::Standard);
}

/// An unmeasured plane refuses at the lowering boundary instead of becoming a
/// default aircraft, and the refusal names the actor and the open claim.
#[test]
fn accept_f49_a_an_unmeasured_plane_refuses_rather_than_defaulting() {
    let catalog = synthetic_instant_action_catalog();
    let unmeasured = ScenarioActorSpec::try_new(
        ScenarioSide::Enemy,
        RosterSlot(0),
        synthetic_player_faction(),
        unknown("the original plane for enemy slot 0 is unmeasured"),
        known(id(ContentKind::Loadout, "synthetic.fixture_ia_light_guns")),
        None,
        Resolved::Known(Known::new(
            cs_content::pilots::DeclaredSurvivability::Mortal,
            Provenance::designed(ClaimId::new("f49a.test").expect("claim is valid")),
        )),
        Provenance::designed(ClaimId::new("f49a.test").expect("claim is valid")),
    )
    .expect("an unmeasured plane is carried by the schema");

    let rules = VictoryRules::try_new(
        VictoryCondition::EliminateEnemies,
        RespawnBudget::None,
        None,
        TieOutcome::Draw,
    )
    .expect("the rules are valid");
    let draft = CustomScenarioDraft::new()
        .with_subject(id(
            ContentKind::IaScenario,
            "synthetic.fixture_ia_scenario_unmeasured",
        ))
        .with_world(known(
            cs_content::world::WorldId::from_key("synthetic.fixture_ia_coastal")
                .expect("world key is valid"),
        ))
        .with_environment(known(
            cs_content::environment::EnvironmentId::new("synthetic.fixture_ia_day_clear")
                .expect("environment key is valid"),
        ))
        .with_roster(vec![
            synthetic_actor(
                ScenarioSide::Player,
                0,
                "synthetic.fixture_ia_interceptor",
                "synthetic.fixture_ia_light_guns",
            ),
            unmeasured,
        ])
        .with_difficulty(cs_content::instant_action::synthetic_difficulty(
            DifficultyTier::Standard,
        ))
        .with_rules(rules)
        .with_seed(cs_content::instant_action::ScenarioSeed::new(3))
        .with_players(1)
        .with_provenance(Provenance::designed(
            ClaimId::new("f49a.test").expect("claim is valid"),
        ));

    let error = lower_custom(&catalog, draft).expect_err("an unmeasured plane refuses");

    // Two refusals, and both are reported: the catalog names it as an
    // unmeasured actor field, and the lowering refuses to spawn.
    match &error {
        LowerError::Invalid { problems } => {
            assert!(
                !problems
                    .with_code(ScenarioProblemCode::UnmeasuredActorField)
                    .is_empty(),
                "the catalog must report the unmeasured field: {problems}"
            );
        }
        other => panic!("expected the catalog to refuse, got {other}"),
    }
    // The catalog refuses before anything is spawned, so no identity record was
    // constructed: the request never reached the lowering.
    assert!(
        !matches!(error, LowerError::UnknownValue { .. }),
        "an unmeasured field must be refused by validation before lowering"
    );
}

/// An unmeasured *preset* whose world is unknown also refuses at the lowering
/// boundary, keeping the unknown's claim and reason verbatim.
#[test]
fn accept_f49_a_an_unknown_world_value_keeps_its_claim_and_reason() {
    let roster = cs_content::instant_action::ScenarioRoster::try_new(vec![
        synthetic_actor(
            ScenarioSide::Player,
            0,
            "synthetic.fixture_ia_interceptor",
            "synthetic.fixture_ia_light_guns",
        ),
        synthetic_actor(
            ScenarioSide::Enemy,
            0,
            "synthetic.fixture_ia_interceptor",
            "synthetic.fixture_ia_light_guns",
        ),
    ])
    .expect("the roster is valid");
    let rules = VictoryRules::try_new(
        VictoryCondition::EliminateEnemies,
        RespawnBudget::None,
        None,
        TieOutcome::Draw,
    )
    .expect("the rules are valid");
    let parameters = cs_content::instant_action::ScenarioParameters::new(
        unknown("the original preset's world is unmeasured"),
        known(
            cs_content::environment::EnvironmentId::new("synthetic.fixture_ia_day_clear")
                .expect("environment key is valid"),
        ),
        roster,
        cs_content::instant_action::synthetic_difficulty(DifficultyTier::Standard),
        rules,
        cs_content::instant_action::ScenarioSeed::new(5),
    );
    let preset = cs_content::instant_action::InstantActionPreset::try_new(
        id(
            ContentKind::IaPreset,
            "synthetic.fixture_ia_unmeasured_world",
        ),
        id(
            ContentKind::IaScenario,
            "synthetic.fixture_ia_scenario_unmeasured_world",
        ),
        unknown("the original preset's display name is unmeasured"),
        parameters,
        cs_types::content::Origin::SyntheticFixture,
        Provenance::designed(ClaimId::new("f49a.test").expect("claim is valid")),
    )
    .expect("a preset may carry an unmeasured world");

    let catalog = cs_content::instant_action::InstantActionCatalog::try_new(
        vec![preset],
        cs_content::instant_action::synthetic_scenario_options(),
        Provenance::designed(ClaimId::new("f49a.test").expect("claim is valid")),
    )
    .expect("the catalog builds");

    let error = lower_preset(
        &catalog,
        &id(
            ContentKind::IaPreset,
            "synthetic.fixture_ia_unmeasured_world",
        ),
    )
    .expect_err("an unmeasured world refuses");

    let LowerError::UnknownValue {
        field,
        claim_id,
        reason,
    } = &error
    else {
        panic!("expected an unknown-value refusal, got {error}");
    };
    assert_eq!(field, "scenario world");
    assert_eq!(claim_id.as_str(), "f49a.test-open");
    assert_eq!(reason, "the original preset's world is unmeasured");
}

/// A preset whose roster carries an unmeasured **airframe**, **loadout** or
/// **survivability** refuses at the per-actor lowering, naming the actor's side
/// and slot and keeping the unknown's own claim and reason.
///
/// A preset is not re-validated against the custom option table, so this is the
/// only path that reaches `lower_actor`'s per-actor refusals: the custom path
/// refuses the same values earlier, in catalog validation. Without this test
/// those three refusals have no coverage and an unmeasured plane could be
/// substituted for a default one unnoticed.
#[test]
fn accept_f49_a_an_unmeasured_preset_actor_field_refuses_at_the_actor_it_belongs_to() {
    let cases = [
        UnmeasuredField::Airframe,
        UnmeasuredField::Loadout,
        UnmeasuredField::Survivability,
    ];

    for case in cases {
        let roster = cs_content::instant_action::ScenarioRoster::try_new(vec![
            synthetic_actor(
                ScenarioSide::Player,
                0,
                "synthetic.fixture_ia_interceptor",
                "synthetic.fixture_ia_light_guns",
            ),
            case.actor(ScenarioSide::Enemy, 0),
        ])
        .expect("the roster is structurally valid");

        let rules = VictoryRules::try_new(
            VictoryCondition::EliminateEnemies,
            RespawnBudget::None,
            None,
            TieOutcome::Draw,
        )
        .expect("the rules are valid");
        let parameters = cs_content::instant_action::ScenarioParameters::new(
            known(
                cs_content::world::WorldId::from_key("synthetic.fixture_ia_coastal")
                    .expect("world key is valid"),
            ),
            known(
                cs_content::environment::EnvironmentId::new("synthetic.fixture_ia_day_clear")
                    .expect("environment key is valid"),
            ),
            roster,
            cs_content::instant_action::synthetic_difficulty(DifficultyTier::Standard),
            rules,
            cs_content::instant_action::ScenarioSeed::new(5),
        );
        let preset = cs_content::instant_action::InstantActionPreset::try_new(
            id(
                ContentKind::IaPreset,
                "synthetic.fixture_ia_unmeasured_actor",
            ),
            id(
                ContentKind::IaScenario,
                "synthetic.fixture_ia_scenario_unmeasured_actor",
            ),
            known("synthetic fixture unmeasured actor".to_owned()),
            parameters,
            cs_types::content::Origin::SyntheticFixture,
            Provenance::designed(ClaimId::new("f49a.test").expect("claim is valid")),
        )
        .expect("a preset may carry an unmeasured actor field");

        let catalog = cs_content::instant_action::InstantActionCatalog::try_new(
            vec![preset],
            cs_content::instant_action::synthetic_scenario_options(),
            Provenance::designed(ClaimId::new("f49a.test").expect("claim is valid")),
        )
        .expect("the catalog builds");

        let error = lower_preset(
            &catalog,
            &id(
                ContentKind::IaPreset,
                "synthetic.fixture_ia_unmeasured_actor",
            ),
        )
        .expect_err("an unmeasured actor field refuses");

        let LowerError::UnknownValue {
            field,
            claim_id,
            reason,
        } = &error
        else {
            panic!("expected an unknown-value refusal for the {case:?}, got {error}");
        };
        assert_eq!(
            field,
            case.expected_field(),
            "the refusal must name the field"
        );
        assert_eq!(
            claim_id.as_str(),
            "f49a.test-open",
            "the {case:?} refusal must keep the unknown's own claim"
        );
        assert_eq!(reason, case.reason());
    }
}

/// One actor field left deliberately unmeasured, with the refusal it must
/// produce.
#[derive(Clone, Copy, Debug)]
enum UnmeasuredField {
    Airframe,
    Loadout,
    Survivability,
}

impl UnmeasuredField {
    /// The reason the field is unknown, which the refusal must keep verbatim.
    fn reason(self) -> &'static str {
        match self {
            Self::Airframe => "the original plane is unmeasured",
            Self::Loadout => "the original ordnance is unmeasured",
            Self::Survivability => "the original survivability is unmeasured",
        }
    }

    /// The `LowerError::UnknownValue` field label the refusal must carry.
    fn expected_field(self) -> &'static str {
        match self {
            Self::Airframe => "enemy slot 0 airframe",
            Self::Loadout => "enemy slot 0 loadout",
            Self::Survivability => "enemy slot 0 survivability",
        }
    }

    /// One actor with exactly this field unknown and the other two known.
    fn actor(self, side: ScenarioSide, slot: u32) -> ScenarioActorSpec {
        let reason = self.reason();
        let open = || {
            Resolved::<ContentId>::unknown(
                ClaimId::new("f49a.test-open").expect("claim is valid"),
                reason,
            )
            .expect("the reason is non-empty")
        };
        let (airframe, loadout, survivability) = match self {
            Self::Airframe => (
                open(),
                known(id(ContentKind::Loadout, "synthetic.fixture_ia_light_guns")),
                known(DeclaredSurvivability::Mortal),
            ),
            Self::Loadout => (
                known(id(
                    ContentKind::Airframe,
                    "synthetic.fixture_ia_interceptor",
                )),
                open(),
                known(DeclaredSurvivability::Mortal),
            ),
            Self::Survivability => (
                known(id(
                    ContentKind::Airframe,
                    "synthetic.fixture_ia_interceptor",
                )),
                known(id(ContentKind::Loadout, "synthetic.fixture_ia_light_guns")),
                Resolved::unknown(
                    ClaimId::new("f49a.test-open").expect("claim is valid"),
                    reason,
                )
                .expect("the reason is non-empty"),
            ),
        };
        actor_with(side, slot, airframe, loadout, survivability)
    }
}

/// A loadout id in the roster's `faction` field is refused by the schema, so
/// the boundary's `FactionId` construction can never be handed a non-faction:
/// the refusal happens before a roster exists, not during lowering.
#[test]
fn accept_f49_a_a_non_faction_actor_faction_is_refused_by_the_schema() {
    assert!(
        ScenarioActorSpec::try_new(
            ScenarioSide::Enemy,
            RosterSlot(0),
            id(ContentKind::Loadout, "synthetic.fixture_ia_light_guns"),
            known(id(
                ContentKind::Airframe,
                "synthetic.fixture_ia_interceptor"
            )),
            known(id(ContentKind::Loadout, "synthetic.fixture_ia_light_guns")),
            None,
            known(DeclaredSurvivability::Mortal),
            Provenance::designed(ClaimId::new("f49a.test").expect("claim is valid")),
        )
        .is_err(),
        "a loadout id must not pass as a faction"
    );
}

/// Builds one actor from fully explicit fields, so a test can leave exactly one
/// of them unmeasured.
fn actor_with(
    side: ScenarioSide,
    slot: u32,
    airframe: Resolved<ContentId>,
    loadout: Resolved<ContentId>,
    survivability: Resolved<DeclaredSurvivability>,
) -> ScenarioActorSpec {
    ScenarioActorSpec::try_new(
        side,
        RosterSlot(slot),
        if side == ScenarioSide::Enemy {
            cs_content::instant_action::synthetic_opposition_faction()
        } else {
            synthetic_player_faction()
        },
        airframe,
        loadout,
        Some(id(ContentKind::Pilot, "synthetic.fixture_pilot")),
        survivability,
        Provenance::designed(ClaimId::new("f49a.test").expect("claim is valid")),
    )
    .expect("the actor is structurally valid")
}

/// A preset the catalog does not hold is refused by id, not by falling back to
/// some other preset.
#[test]
fn accept_f49_a_an_unknown_preset_id_refuses_without_a_fallback() {
    let catalog = synthetic_instant_action_catalog();
    let error = lower_preset(
        &catalog,
        &id(ContentKind::IaPreset, "synthetic.fixture_ia_absent"),
    )
    .expect_err("an absent preset must refuse");
    assert_eq!(
        error,
        LowerError::UnknownPreset {
            preset: id(ContentKind::IaPreset, "synthetic.fixture_ia_absent")
        }
    );
}

/// Every dimension a custom-scenario screen offers comes from the catalog, so
/// an option the catalog does not declare cannot be shown (non-negotiable 5).
#[test]
fn accept_f49_a_the_offered_custom_dimensions_come_from_the_catalog() {
    let catalog = synthetic_instant_action_catalog();
    let dimensions = custom_dimensions(&catalog);
    let options = catalog.options();

    assert_eq!(dimensions.len(), 6, "the sheet names six custom dimensions");
    for dimension in &dimensions {
        assert!(
            !dimension.is_empty(),
            "dimension {} offers no value, so it must not be rendered",
            dimension.name()
        );
        assert!(!dimension.name().is_empty());
    }

    let find = |name: &str| {
        dimensions
            .iter()
            .find(|dimension| dimension.name() == name)
            .unwrap_or_else(|| panic!("the boundary offers a {name} dimension"))
    };

    // Each group's length is exactly the catalog's option count, so a screen
    // cannot display an option the catalog would then refuse.
    match find("world") {
        CustomDimension::World(values) => assert_eq!(values, options.worlds()),
        other => panic!("expected a world dimension, got {other:?}"),
    }
    match find("environment") {
        CustomDimension::Environment(values) => assert_eq!(values, options.environments()),
        other => panic!("expected an environment dimension, got {other:?}"),
    }
    match find("roster") {
        CustomDimension::Airframe(values) => assert_eq!(values, options.airframes()),
        other => panic!("expected an airframe dimension, got {other:?}"),
    }
    match find("skill") {
        CustomDimension::Difficulty(values) => assert_eq!(values, options.difficulty_tiers()),
        other => panic!("expected a difficulty dimension, got {other:?}"),
    }
    match find("rules") {
        CustomDimension::Victory(values) => assert_eq!(values, options.victory_conditions()),
        other => panic!("expected a victory dimension, got {other:?}"),
    }

    // Every offered airframe and loadout is one a plan will accept, so no
    // visible option is inert.
    for airframe in options.airframes() {
        let draft = draft_with(&catalog, |actors| {
            replace_enemy(actors, 0, airframe.key(), "synthetic.fixture_ia_light_guns");
        });
        assert!(
            lower_custom(&catalog, draft).is_ok(),
            "the offered airframe {airframe} must be selectable"
        );
    }
    for loadout in options.loadouts() {
        let draft = draft_with(&catalog, |actors| {
            replace_enemy(actors, 0, "synthetic.fixture_ia_interceptor", loadout.key());
        });
        assert!(
            lower_custom(&catalog, draft).is_ok(),
            "the offered loadout {loadout} must be selectable"
        );
    }

    // And an option the catalog does *not* offer is refused by the same path.
    let refused = draft_with(&catalog, |actors| {
        replace_enemy(
            actors,
            0,
            "synthetic.fixture_ia_interceptor",
            "synthetic.fixture_ia_unlisted",
        );
    });
    let error = lower_custom(&catalog, refused).expect_err("an unlisted loadout refuses");
    let LowerError::Invalid { problems } = &error else {
        panic!("expected a validation refusal, got {error}");
    };
    assert_eq!(
        problems
            .with_code(ScenarioProblemCode::UnsupportedLoadout)
            .len(),
        1,
        "the unlisted ordnance must be named: {problems}"
    );
}

/// Replaces one enemy actor, keeping its slot index.
///
/// Reached by side and slot rather than by list position, because the declared
/// roster is sorted by `(side, slot)` and a positional edit would change a
/// different actor whenever the sort changed.
fn replace_enemy(actors: &mut [ScenarioActorSpec], slot: u32, airframe: &str, loadout: &str) {
    let enemy = actors
        .iter_mut()
        .find(|actor| actor.side() == ScenarioSide::Enemy && actor.slot().index() == slot)
        .unwrap_or_else(|| panic!("the fixture roster has an enemy at slot {slot}"));
    *enemy = synthetic_actor(ScenarioSide::Enemy, slot, airframe, loadout);
}

/// The draft `draft_with` produces: the valid fixture request with one roster
/// slot replaced by `mutate`.
fn draft_with(
    catalog: &cs_content::instant_action::InstantActionCatalog,
    mutate: impl FnOnce(&mut Vec<ScenarioActorSpec>),
) -> CustomScenarioDraft {
    let request = synthetic_custom_request();
    let mut actors = request.parameters().roster().actors().to_vec();
    mutate(&mut actors);
    let _ = catalog;
    CustomScenarioDraft::new()
        .with_subject(request.subject().clone())
        .with_world(request.parameters().world().clone())
        .with_environment(request.parameters().environment().clone())
        .with_roster(actors)
        .with_difficulty(request.parameters().difficulty().clone())
        .with_rules(*request.parameters().rules())
        .with_seed(request.parameters().seed())
        .with_players(request.players())
        .with_provenance(request.provenance().clone())
}

/// AC04 at the boundary: an invalid custom scenario is refused with the whole
/// problem list, before anything is spawned, so a screen shows every problem
/// at once instead of the player retrying into an endless session.
#[test]
fn accept_f49_a_an_invalid_custom_scenario_is_refused_with_every_problem() {
    let catalog = synthetic_instant_action_catalog();
    let draft = CustomScenarioDraft::new()
        .with_subject(id(
            ContentKind::IaScenario,
            "synthetic.fixture_ia_scenario_broken",
        ))
        .with_world(known(
            cs_content::world::WorldId::from_key("synthetic.fixture_ia_absent_world")
                .expect("world key is valid"),
        ))
        .with_environment(known(
            cs_content::environment::EnvironmentId::new("synthetic.fixture_ia_absent_env")
                .expect("environment key is valid"),
        ))
        .with_roster(vec![synthetic_actor(
            ScenarioSide::Player,
            0,
            "synthetic.fixture_ia_interceptor",
            "synthetic.fixture_ia_light_guns",
        )])
        .with_difficulty(cs_content::instant_action::synthetic_difficulty(
            DifficultyTier::Elite,
        ))
        .with_rules(
            VictoryRules::try_new(
                VictoryCondition::EliminateEnemies,
                RespawnBudget::None,
                None,
                TieOutcome::Draw,
            )
            .expect("the rules are valid"),
        )
        .with_seed(cs_content::instant_action::ScenarioSeed::new(9))
        .with_players(
            cs_content::instant_action::MAX_SCENARIO_PLAYERS
                .min(catalog.options().max_players() + 1),
        )
        .with_provenance(Provenance::designed(
            ClaimId::new("f49a.test").expect("claim is valid"),
        ));

    let error = lower_custom(&catalog, draft).expect_err("the scenario must be refused");
    let LowerError::Invalid { problems } = &error else {
        panic!("expected a validation refusal, got {error}");
    };

    // Every problem at once, not just the first.
    let codes: Vec<&str> = problems
        .problems()
        .iter()
        .map(|problem| problem.code().code())
        .collect();
    for expected in [
        "unsupported_world",
        "unsupported_environment",
        "unsupported_difficulty",
        "invalid_player_count",
        "missing_side",
        "condition_unsatisfiable",
    ] {
        assert!(
            codes.contains(&expected),
            "expected problem {expected}, got {problems}"
        );
    }
    assert!(
        problems.len() >= 6,
        "problems must be reported together, got {}: {problems}",
        problems.len()
    );
    for problem in problems.problems() {
        assert!(!problem.dimension().is_empty());
        assert!(!problem.detail().is_empty());
    }
}

/// An incomplete draft names every unset dimension, so no dimension can
/// silently default into "the first one in the list".
#[test]
fn accept_f49_a_an_incomplete_custom_draft_names_every_unset_dimension() {
    let catalog = synthetic_instant_action_catalog();
    let error =
        lower_custom(&catalog, CustomScenarioDraft::new()).expect_err("an empty draft must refuse");
    let LowerError::IncompleteDraft { missing, .. } = &error else {
        panic!("expected an incomplete-draft refusal, got {error}");
    };
    assert_eq!(
        missing,
        &vec![
            "subject",
            "world",
            "environment",
            "roster",
            "skill",
            "rules",
            "seed",
            "players",
        ]
    );
}

/// The lowered plan's actors are in declared `(side, slot)` order, so a diff of
/// two plans shows exactly which actor moved. This is the baseline AC02's
/// one-slot change is measured against (F49-B performs the change).
#[test]
fn accept_f49_a_the_lowered_actor_order_is_the_declared_order() {
    let catalog = synthetic_instant_action_catalog();
    let request = synthetic_custom_request();
    let draft = CustomScenarioDraft::new()
        .with_subject(request.subject().clone())
        .with_world(request.parameters().world().clone())
        .with_environment(request.parameters().environment().clone())
        .with_roster(request.parameters().roster().actors().to_vec())
        .with_difficulty(request.parameters().difficulty().clone())
        .with_rules(*request.parameters().rules())
        .with_seed(request.parameters().seed())
        .with_players(request.players())
        .with_provenance(request.provenance().clone());

    let lowered = lower_custom(&catalog, draft).expect("the fixture request lowers");
    let order: Vec<(ScenarioSide, u32)> = lowered
        .actors()
        .iter()
        .map(|actor| (actor.side, actor.slot.index()))
        .collect();
    assert_eq!(
        order,
        vec![
            (ScenarioSide::Player, 0),
            (ScenarioSide::Ally, 0),
            (ScenarioSide::Enemy, 0),
        ],
        "the plan's actor order must match the declared roster order"
    );
    // Reversing the declared roster input lowers to the same order, because the
    // schema sorts once and the plan preserves it.
    let mut reversed = request.parameters().roster().actors().to_vec();
    reversed.reverse();
    let reversed_draft = CustomScenarioDraft::new()
        .with_subject(request.subject().clone())
        .with_world(request.parameters().world().clone())
        .with_environment(request.parameters().environment().clone())
        .with_roster(reversed)
        .with_difficulty(request.parameters().difficulty().clone())
        .with_rules(*request.parameters().rules())
        .with_seed(request.parameters().seed())
        .with_players(request.players())
        .with_provenance(request.provenance().clone());
    assert_eq!(
        lower_custom(&catalog, reversed_draft).expect("reversed order also lowers"),
        lowered,
        "actor input order must not change the lowered scenario"
    );
}

/// The plan exposes its seed for a developer tool and a replay to record, and
/// the stream derivation is domain separated (non-negotiable 4).
#[test]
fn accept_f49_a_the_lowered_seed_is_displayable_and_domain_separated() {
    let catalog = synthetic_instant_action_catalog();
    let lowered = lower_preset(
        &catalog,
        &id(ContentKind::IaPreset, "synthetic.fixture_ia_dogfight"),
    )
    .expect("the fixture preset launches");

    let seed = lowered.seed();
    assert_eq!(seed.root(), 0x00D0_0F1A_0000_0001);
    // The value a developer tool shows and a replay captures.
    assert!(format!("{}", seed.root()).parse::<u64>().is_ok());
    assert_ne!(
        seed.stream(cs_content::instant_action::SCENARIO_SEED_DOMAIN),
        seed.stream(cs_content::instant_action::SCENARIO_SEED_DOMAIN ^ 0xFF),
        "two domains must produce independent streams from one scenario seed"
    );
    // The seed is carried, not re-derived per actor: one scenario, one seed.
    assert_eq!(seed, lowered.seed());
}

/// The fixture catalog is synthetic and does not claim the original catalog is
/// complete, so nothing here can be read as original-preset evidence.
#[test]
fn accept_f49_a_the_fixture_catalog_does_not_claim_original_coverage() {
    let catalog = synthetic_instant_action_catalog();
    assert!(
        !catalog.is_original_complete(),
        "the fixture catalog must never claim to be the original catalog"
    );
    for preset in catalog.presets() {
        assert!(
            !preset.origin().is_original(),
            "fixture preset {} claims original origin",
            preset.id()
        );
        assert!(
            preset.provenance().class == cs_types::evidence::ClaimStatus::Designed,
            "fixture preset {} must be designed provenance",
            preset.id()
        );
    }
}

/// A screen that only reports gets the whole problem list from one error, so
/// AC04's "every problem at once" holds without the screen re-validating.
#[test]
fn accept_f49_a_a_reporting_screen_reads_every_problem_from_one_error() {
    let catalog = synthetic_instant_action_catalog();
    let draft = CustomScenarioDraft::new()
        .with_subject(id(
            ContentKind::IaScenario,
            "synthetic.fixture_ia_scenario_reporting",
        ))
        .with_world(known(
            cs_content::world::WorldId::from_key("synthetic.fixture_ia_absent_world")
                .expect("world key is valid"),
        ))
        .with_environment(known(
            cs_content::environment::EnvironmentId::new("synthetic.fixture_ia_absent_env")
                .expect("environment key is valid"),
        ))
        .with_roster(vec![synthetic_actor(
            ScenarioSide::Player,
            0,
            "synthetic.fixture_ia_interceptor",
            "synthetic.fixture_ia_light_guns",
        )])
        .with_difficulty(cs_content::instant_action::synthetic_difficulty(
            DifficultyTier::Elite,
        ))
        .with_rules(
            VictoryRules::try_new(
                VictoryCondition::EliminateEnemies,
                RespawnBudget::None,
                None,
                TieOutcome::Draw,
            )
            .expect("the rules are valid"),
        )
        .with_seed(cs_content::instant_action::ScenarioSeed::new(11))
        .with_players(1)
        .with_provenance(Provenance::designed(
            ClaimId::new("f49a.test").expect("claim is valid"),
        ));

    let error = lower_custom(&catalog, draft).expect_err("the scenario must be refused");
    let reported = report_problems(&error);
    assert!(
        reported.len() >= 5,
        "one error must carry every problem, got {}",
        reported.len()
    );
    // The reported entries are the very problems validation produced, not a
    // re-derived or truncated copy.
    let LowerError::Invalid { problems } = &error else {
        panic!("expected a validation refusal, got {error}");
    };
    assert_eq!(
        reported.len(),
        problems.problems().len(),
        "the report must not drop or invent problems"
    );
    for problem in reported {
        assert!(!problem.dimension().is_empty());
        assert!(!problem.detail().is_empty());
    }

    // A refusal that is not a validation problem reports nothing rather than
    // pretending it has none — the caller still sees the error itself.
    assert!(
        report_problems(&LowerError::UnknownPreset {
            preset: id(ContentKind::IaPreset, "synthetic.fixture_ia_absent"),
        })
        .is_empty()
    );
}

/// A seat count the roster cannot fill is refused **through the production
/// lowering path**, not only by the catalog's validator, and the refusal names
/// the roster rather than asking for a dimension that is already set.
///
/// This is F49 non-negotiable 2's "invalid player count/roster configuration".
/// A draft can fill every control on the form and still ask for four human
/// seats over a roster with one player-side actor; the catalog's ceiling of four
/// is a property of the option table, not of this roster, so a range check
/// alone would accept it and the session would then seat three players in
/// nothing.
#[test]
fn accept_f49_a_a_seat_count_the_roster_cannot_fill_is_refused_before_anything_lowers() {
    let catalog = synthetic_instant_action_catalog();
    let with_seats = |players: u8| {
        CustomScenarioDraft::new()
            .with_subject(id(
                ContentKind::IaScenario,
                "synthetic.fixture_ia_scenario_seats",
            ))
            .with_world(known(
                cs_content::world::WorldId::from_key("synthetic.fixture_ia_coastal")
                    .expect("world key is valid"),
            ))
            .with_environment(known(
                cs_content::environment::EnvironmentId::new("synthetic.fixture_ia_day_clear")
                    .expect("environment key is valid"),
            ))
            .with_roster(vec![
                synthetic_actor(
                    ScenarioSide::Player,
                    0,
                    "synthetic.fixture_ia_interceptor",
                    "synthetic.fixture_ia_light_guns",
                ),
                synthetic_actor(
                    ScenarioSide::Enemy,
                    0,
                    "synthetic.fixture_ia_interceptor",
                    "synthetic.fixture_ia_light_guns",
                ),
            ])
            .with_difficulty(cs_content::instant_action::synthetic_difficulty(
                DifficultyTier::Standard,
            ))
            .with_rules(
                VictoryRules::try_new(
                    VictoryCondition::EliminateEnemies,
                    RespawnBudget::None,
                    None,
                    TieOutcome::Draw,
                )
                .expect("the rules are valid"),
            )
            .with_seed(cs_content::instant_action::ScenarioSeed::new(41))
            .with_players(players)
            .with_provenance(Provenance::designed(
                ClaimId::new("f49a.test").expect("claim is valid"),
            ))
    };

    // One seat for the one player slot: the baseline a later stage may widen.
    let valid = lower_custom(&catalog, with_seats(1)).expect("one seat lowers");
    assert_eq!(valid.players(), 1);

    for seats in 2..=catalog.options().max_players() {
        let error = lower_custom(&catalog, with_seats(seats))
            .expect_err("a seat count the roster cannot fill is refused");
        let reported = report_problems(&error);
        assert_eq!(
            reported.len(),
            1,
            "{seats} seats over a one-player roster is one problem: {error}"
        );
        assert_eq!(reported[0].code(), ProblemCode::InvalidPlayerCount);
        assert_eq!(reported[0].dimension(), "players");
        assert!(
            reported[0].detail().contains("player slot"),
            "the message names the roster the seats do not fit, so a screen can act on it: {error}"
        );
        // The draft named every dimension, so nothing is "unset" and the
        // refusal is not misfiled as an incomplete form.
        assert!(
            !matches!(error, LowerError::IncompleteDraft { .. }),
            "a complete draft with a wrong seat count is a validation problem: {error}"
        );
    }
}

/// Every user-facing refusal a screen shows is a sentence, not a debug dump.
///
/// `LowerError` renders the schema refusal inside `IncompleteDraft`; rendering
/// it with `Debug` would put a variant name and field list in front of a player
/// at exactly the moment AC04 asks for an actionable message. Each variant is
/// rendered here and checked for the `Debug` shape of the type it carries.
#[test]
fn accept_f49_a_every_refusal_a_screen_renders_is_a_sentence_not_a_debug_dump() {
    let catalog = synthetic_instant_action_catalog();
    let complete_but_unbuildable = CustomScenarioDraft::new()
        .with_subject(id(
            ContentKind::IaScenario,
            "synthetic.fixture_ia_scenario_empty_roster",
        ))
        .with_world(known(
            cs_content::world::WorldId::from_key("synthetic.fixture_ia_coastal")
                .expect("world key is valid"),
        ))
        .with_environment(known(
            cs_content::environment::EnvironmentId::new("synthetic.fixture_ia_day_clear")
                .expect("environment key is valid"),
        ))
        .with_roster(Vec::new())
        .with_difficulty(cs_content::instant_action::synthetic_difficulty(
            DifficultyTier::Standard,
        ))
        .with_rules(
            VictoryRules::try_new(
                VictoryCondition::EliminateEnemies,
                RespawnBudget::None,
                None,
                TieOutcome::Draw,
            )
            .expect("the rules are valid"),
        )
        .with_seed(cs_content::instant_action::ScenarioSeed::new(43))
        .with_players(1)
        .with_provenance(Provenance::designed(
            ClaimId::new("f49a.test").expect("claim is valid"),
        ));

    let errors = vec![
        lower_custom(&catalog, complete_but_unbuildable).expect_err("an empty roster is refused"),
        lower_preset(
            &catalog,
            &id(ContentKind::IaPreset, "synthetic.fixture_ia_absent"),
        )
        .expect_err("an unknown preset is refused"),
        lower_custom(
            &catalog,
            CustomScenarioDraft::new()
                .with_world(known(
                    cs_content::world::WorldId::from_key("synthetic.fixture_ia_coastal")
                        .expect("world key is valid"),
                ))
                .with_roster(vec![synthetic_actor(
                    ScenarioSide::Player,
                    0,
                    "synthetic.fixture_ia_interceptor",
                    "synthetic.fixture_ia_light_guns",
                )]),
        )
        .expect_err("an incomplete draft is refused"),
    ];

    for error in &errors {
        let rendered = error.to_string();
        assert!(
            !rendered.is_empty(),
            "every refusal must render something a screen can show: {error:?}"
        );
        // `Debug` on these types always names the variant in PascalCase and, for
        // the schema-carrying one, wraps the payload in `Some(...)`.
        for leak in ["IncompleteDraft", "UnknownPreset", "Some(", "None"] {
            assert!(
                !rendered.contains(leak),
                "the rendered refusal leaks a debug shape ({leak:?}): {rendered}"
            );
        }
        // A `Display` refusal is prose and carries no Rust type syntax. A
        // `Debug` dump always does: braces around the variant's fields, and a
        // `PascalCase` variant name. Both are rejected above and here, so this
        // pins the shape without over-specifying punctuation or phrasing.
        assert!(
            !rendered.contains('{') && !rendered.contains('}'),
            "a rendered refusal carries no struct braces: {rendered}"
        );
        assert!(
            rendered.chars().any(char::is_whitespace),
            "a refusal is prose, not a single value dump: {rendered}"
        );
        assert!(
            !rendered.contains("= "),
            "a rendered refusal carries no field assignment: {rendered}"
        );
    }
}

/// The known value a test needs, panicking with the field name when the value
/// is an unknown.
fn require<T: Clone>(value: &Resolved<T>) -> &T {
    match value {
        Resolved::Known(known) => &known.value,
        Resolved::Unknown { reason, .. } => panic!("the fixture value is known: {reason}"),
    }
}
