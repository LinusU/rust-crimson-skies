//! Acceptance scenario F49-D.04: AC04 ("empty/invalid rosters give actionable
//! validation errors instead of endless sessions") against the retail catalog.
//!
//! Spec: `specs/F49-instant-action-presets-and-custom-scenarios.md`, AC04 and
//! non-negotiable 2. Task test prefix: `accept_f49_d_04_`. Required capability:
//! **retail** (`$CS_GAME_DIR`), read-only; nothing is rendered, played or run.
//!
//! Every check goes through the production path only: `CustomScenarioDraft::
//! resolve`, `InstantActionCatalog::validate_custom` / `require_valid_custom`
//! and `cs_app::ui::instant_action::{resolve_custom, lower_custom}` and the
//! `InstantActionFlow` that launches. The retail tests assemble the catalog
//! through `retail_instant_action_catalog`; the synthetic run of the same checks
//! executes on CI, which has no original data.
//!
//! **What "retail" can and cannot show here.** The retail option table measures
//! worlds, environments, factions, airframes, pilots (the ia.zrd aces,
//! F49-D.02-FU-PILOT) and difficulty tiers; it keeps loadouts and relations
//! *unmeasured* (F49-D.02), so a custom scenario naming a loadout is refused by
//! the pure retail catalog today, which is the right answer and is asserted as
//! such
//! (`accept_f49_d_04_retail_every_measured_selection_is_accepted_and_the_unmeasured_loadout_is_refused`).
//! A request that *no* problem touches therefore cannot be built from the pure
//! retail table. The positive control uses the retail lists **plus declared
//! stand-ins** for the two unmeasured lists (`standin_catalog`): it proves
//! refusal is specific rather than unconditional, and it claims nothing about
//! what the original accepts.

use std::path::PathBuf;
use std::sync::OnceLock;

use cs_app::ui::instant_action::{
    IaAction, IaEdit, IaRefusal, IaScreen, InstantActionFlow, LowerError, ProblemCode,
    lower_custom, report_problems, resolve_custom, retail_instant_action_catalog,
};
use cs_content::ai::DifficultyTier;
use cs_content::environment::EnvironmentId;
use cs_content::instant_action::{
    CustomScenarioDraft, InstantActionCatalog, RespawnBudget, RosterSlot, ScenarioActorSpec,
    ScenarioOptionDimension, ScenarioOptions, ScenarioProblem, ScenarioProblems,
    ScenarioSchemaError, ScenarioSeed, ScenarioSide, TieOutcome, VictoryCondition, VictoryRules,
    synthetic_difficulty, synthetic_instant_action_catalog, synthetic_provenance,
};
use cs_content::pilots::DeclaredSurvivability;
use cs_content::target_rules::{DeclaredAllegiance, DeclaredRelation};
use cs_content::world::WorldId;
use cs_sim::campaign::{ProfileId, SessionGeneration};
use cs_types::content::{ContentId, ContentKind, Known, Resolved};
use cs_types::evidence::ClaimId;

// ------------------------------------------------------------------- setup ---

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("CS_GAME_DIR")
            .expect("CS_GAME_DIR must name the original installation for this retail test"),
    )
}

/// The retail catalog, assembled once.
fn retail() -> &'static InstantActionCatalog {
    static RETAIL: OnceLock<InstantActionCatalog> = OnceLock::new();
    RETAIL.get_or_init(|| {
        retail_instant_action_catalog(&game_dir())
            .expect("the retail catalog assembles from the installation")
    })
}

fn id(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("a valid fixture id")
}

fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(value, synthetic_provenance()))
}

fn profile() -> ProfileId {
    ProfileId::new("pilot.nathan").expect("a valid profile id")
}

/// The retail catalog's own lists with declared stand-ins for the two lists
/// the installation leaves unmeasured: one loadout and two relations
/// (`factions[0]` hostile to `factions[1]`, friendly to `factions[2]`). The
/// pilots are the measured ia.zrd ace identities, not a stand-in.
///
/// Every other list, and every preset, is the retail catalog's.
fn standin_catalog(retail: &InstantActionCatalog) -> InstantActionCatalog {
    let options = retail.options();
    assert_eq!(
        options.unmeasured_dimensions(),
        [
            ScenarioOptionDimension::Loadouts,
            ScenarioOptionDimension::Relations
        ],
        "the retail table keeps exactly loadouts and relations unmeasured"
    );
    let factions = options.factions().to_vec();
    assert!(factions.len() >= 3, "the retail table measures factions");
    let relation = |from: &ContentId, to: &ContentId, allegiance| DeclaredRelation {
        from: from.clone(),
        to: to.clone(),
        allegiance: known(allegiance),
    };
    let widened = ScenarioOptions::try_new(
        options.worlds().to_vec(),
        options.environments().to_vec(),
        factions.clone(),
        options.airframes().to_vec(),
        vec![id(ContentKind::Loadout, "synthetic.ac04_standin_loadout")],
        options.pilots().to_vec(),
        options.difficulty_tiers().to_vec(),
        options.victory_conditions().to_vec(),
        vec![
            relation(&factions[0], &factions[1], DeclaredAllegiance::Hostile),
            relation(&factions[1], &factions[0], DeclaredAllegiance::Hostile),
            relation(&factions[0], &factions[2], DeclaredAllegiance::Friendly),
            relation(&factions[2], &factions[0], DeclaredAllegiance::Friendly),
        ],
        options.max_players(),
    )
    .expect("the retail lists plus stand-ins form a valid table");
    InstantActionCatalog::try_new(
        retail.presets().into_iter().cloned().collect(),
        widened,
        retail.provenance().clone(),
    )
    .expect("the retail presets fit the widened table")
}

/// The selections a valid request is built from, all taken from one catalog's
/// own option table so nothing here is typed in.
struct Base {
    world: WorldId,
    environment: EnvironmentId,
    player_faction: ContentId,
    enemy_faction: ContentId,
    /// A faction the player's is declared friendly or neutral toward.
    unfightable_faction: Option<ContentId>,
    airframes: [ContentId; 2],
    loadout: ContentId,
    tier: DifficultyTier,
    rules: VictoryRules,
}

impl Base {
    /// `loadout` is the catalog's first loadout when it has one, else `fallback`
    /// (the pure retail table has none).
    fn from(catalog: &InstantActionCatalog, fallback: Option<ContentId>) -> Self {
        let options = catalog.options();
        let relations = options.relations();
        let hostile = |relation: &DeclaredRelation| matches!(&relation.allegiance, Resolved::Known(k) if k.value == DeclaredAllegiance::Hostile);
        let fightable = relations
            .iter()
            .find(|relation| hostile(relation))
            .map(|relation| (relation.from.clone(), relation.to.clone()));
        let (player_faction, enemy_faction) = fightable.unwrap_or_else(|| {
            // No declared relation: two distinct factions, undeclared, which the
            // catalog never assumes impossible.
            (options.factions()[0].clone(), options.factions()[1].clone())
        });
        let unfightable_faction = relations.iter().find_map(|relation| {
            let blocked = matches!(&relation.allegiance, Resolved::Known(k)
                if matches!(k.value, DeclaredAllegiance::Friendly | DeclaredAllegiance::Neutral));
            (blocked && relation.from == player_faction).then(|| relation.to.clone())
        });
        let condition = if options
            .victory_conditions()
            .contains(&VictoryCondition::EliminateEnemies)
        {
            VictoryCondition::EliminateEnemies
        } else {
            options.victory_conditions()[0]
        };
        let deadline = condition.needs_deadline().then_some(3_600);
        let rules =
            VictoryRules::try_new(condition, RespawnBudget::None, deadline, TieOutcome::Draw)
                .expect("the chosen victory rules are valid");
        assert!(options.airframes().len() >= 2, "two airframes are offered");
        assert!(
            options.max_players() >= 2,
            "the seat-count checks need a ceiling of at least two"
        );
        Self {
            world: options.worlds()[0].clone(),
            environment: options.environments()[0].clone(),
            player_faction,
            enemy_faction,
            unfightable_faction,
            airframes: [
                options.airframes()[0].clone(),
                options.airframes()[1].clone(),
            ],
            loadout: options
                .loadouts()
                .first()
                .cloned()
                .or(fallback)
                .expect("a loadout to select"),
            tier: options.difficulty_tiers()[0],
            rules,
        }
    }

    fn actor(
        &self,
        side: ScenarioSide,
        slot: u32,
        faction: &ContentId,
        airframe: &ContentId,
    ) -> ScenarioActorSpec {
        ScenarioActorSpec::try_new(
            side,
            RosterSlot(slot),
            faction.clone(),
            known(airframe.clone()),
            known(self.loadout.clone()),
            None,
            known(DeclaredSurvivability::Mortal),
            synthetic_provenance(),
        )
        .expect("the fixture actor is valid")
    }

    /// One player against one enemy, on distinct factions.
    fn roster(&self) -> Vec<ScenarioActorSpec> {
        vec![
            self.actor(
                ScenarioSide::Player,
                0,
                &self.player_faction,
                &self.airframes[0],
            ),
            self.actor(
                ScenarioSide::Enemy,
                0,
                &self.enemy_faction,
                &self.airframes[1],
            ),
        ]
    }

    fn draft(&self, roster: Vec<ScenarioActorSpec>, players: u8) -> CustomScenarioDraft {
        CustomScenarioDraft::new()
            .with_subject(id(
                ContentKind::IaScenario,
                "synthetic.ac04_custom_scenario",
            ))
            .with_world(known(self.world.clone()))
            .with_environment(known(self.environment.clone()))
            .with_roster(roster)
            .with_difficulty(synthetic_difficulty(self.tier))
            .with_rules(self.rules)
            .with_seed(ScenarioSeed::new(7))
            .with_players(players)
            .with_provenance(synthetic_provenance())
    }
}

// ------------------------------------------------------------- assertions ---

/// What a screen needs from one problem: its dimension, the selection to
/// replace or the open claim, and a detail, rendered as a plain sentence and
/// never as `Debug`.
fn assert_renders(problem: &ScenarioProblem) -> String {
    let text = problem.to_string();
    assert!(
        text.contains(problem.dimension()),
        "the sentence names its dimension: {text}"
    );
    assert!(
        text.contains(problem.code().code()),
        "the sentence carries the stable code: {text}"
    );
    assert!(!problem.detail().is_empty(), "the problem says why: {text}");
    assert!(
        text.contains(problem.detail()),
        "the detail is rendered: {text}"
    );
    if let Some(offender) = problem.offender() {
        assert!(
            text.contains(offender),
            "the selection to replace is named: {text}"
        );
    }
    if let Some(claim) = problem.claim_id() {
        assert!(
            text.contains(&claim.to_string()),
            "the open claim is named: {text}"
        );
    }
    for debug_marker in ["ScenarioProblem", "{ ", "Some(", "None"] {
        assert!(
            !text.contains(debug_marker),
            "a screen sentence is not Debug output ({debug_marker}): {text}"
        );
    }
    text
}

/// The catalog's refusal of `draft`, which must be a full problem list.
fn invalid(catalog: &InstantActionCatalog, draft: CustomScenarioDraft) -> ScenarioProblems {
    match resolve_custom(catalog, draft) {
        Err(LowerError::Invalid { problems }) => {
            assert!(!problems.is_empty(), "a refusal carries its problems");
            problems
        }
        other => panic!("expected the catalog to refuse the request, got {other:?}"),
    }
}

/// The schema refusal of a draft that cannot even become a request.
fn schema_refusal(
    catalog: &InstantActionCatalog,
    draft: CustomScenarioDraft,
) -> ScenarioSchemaError {
    match resolve_custom(catalog, draft) {
        Err(LowerError::IncompleteDraft {
            missing,
            schema: Some(schema),
        }) => {
            assert!(missing.is_empty(), "every dimension was named: {missing:?}");
            schema
        }
        other => panic!("expected a schema refusal of the draft, got {other:?}"),
    }
}

// ----------------------------------------------------------------- checks ---

fn check_a_valid_request_is_accepted(catalog: &InstantActionCatalog, base: &Base) {
    let request = resolve_custom(catalog, base.draft(base.roster(), 1))
        .expect("a request built from the catalog's own table is accepted");
    assert!(catalog.validate_custom(&request).is_empty());
    catalog
        .require_valid_custom(&request)
        .expect("require_valid_custom agrees");
    let lowered =
        lower_custom(catalog, base.draft(base.roster(), 1)).expect("the accepted request lowers");
    assert_eq!(lowered.players(), 1);
    assert_eq!(lowered.actors().len(), 2, "both roster actors are lowered");
}

fn check_an_empty_roster(catalog: &InstantActionCatalog, base: &Base) {
    // No actor at all: refused where the roster is built, with a sentence.
    let schema = schema_refusal(catalog, base.draft(Vec::new(), 1));
    assert_eq!(schema, ScenarioSchemaError::EmptyRoster);
    let error = resolve_custom(catalog, base.draft(Vec::new(), 1)).unwrap_err();
    let text = error.to_string();
    assert!(
        text.contains("declares no actor"),
        "actionable sentence: {text}"
    );
    assert!(!text.contains("EmptyRoster"), "not Debug: {text}");
    assert!(
        lower_custom(catalog, base.draft(Vec::new(), 1)).is_err(),
        "an empty roster never lowers into a scenario"
    );

    // A roster with nobody to fight: every consequence is reported, not the
    // first one.
    let player_only = vec![base.actor(
        ScenarioSide::Player,
        0,
        &base.player_faction,
        &base.airframes[0],
    )];
    let problems = invalid(catalog, base.draft(player_only, 1));
    let missing = problems.with_code(ProblemCode::MissingSide);
    assert_eq!(
        missing.len(),
        1,
        "the missing enemy side is named: {problems}"
    );
    assert!(assert_renders(missing[0]).contains("no enemy"));
    if base.rules.condition().needs_enemies() {
        assert_eq!(
            problems
                .with_code(ProblemCode::ConditionUnsatisfiable)
                .len(),
            1,
            "the unwinnable victory rule is reported with it: {problems}"
        );
        assert_eq!(
            problems.len(),
            2,
            "exactly the two real problems: {problems}"
        );
    }
    for problem in problems.problems() {
        assert_renders(problem);
    }
}

fn check_an_airframe_nobody_offers(catalog: &InstantActionCatalog, base: &Base) {
    let phantom_a = id(ContentKind::Airframe, "synthetic.ac04_phantom_airframe_a");
    let phantom_b = id(ContentKind::Airframe, "synthetic.ac04_phantom_airframe_b");
    let phantom_world = WorldId::from_key("synthetic.ac04_phantom_world").expect("a world key");
    let roster = vec![
        base.actor(ScenarioSide::Player, 0, &base.player_faction, &phantom_a),
        base.actor(ScenarioSide::Enemy, 0, &base.enemy_faction, &phantom_b),
    ];
    let draft = base
        .draft(roster, 1)
        .with_world(known(phantom_world.clone()));
    let problems = invalid(catalog, draft);

    // All three problems come back at once.
    assert_eq!(problems.len(), 3, "world and both airframes: {problems}");
    let airframes = problems.with_code(ProblemCode::UnsupportedAirframe);
    assert_eq!(airframes.len(), 2);
    let offenders: Vec<&str> = airframes.iter().filter_map(|p| p.offender()).collect();
    assert!(offenders.contains(&phantom_a.to_string().as_str()));
    assert!(offenders.contains(&phantom_b.to_string().as_str()));
    for problem in &airframes {
        assert_eq!(problem.dimension(), "roster");
        let text = assert_renders(problem);
        assert!(
            text.contains("slot 0") && text.contains("is not a plane this catalog offers"),
            "the sentence says which slot to change: {text}"
        );
    }
    let worlds = problems.with_code(ProblemCode::UnsupportedWorld);
    assert_eq!(worlds.len(), 1);
    assert_eq!(
        worlds[0].offender(),
        Some(phantom_world.to_string().as_str())
    );
    assert_eq!(worlds[0].dimension(), "world");
    assert_renders(worlds[0]);

    // The whole list is what a screen renders: truncating it to the first
    // problem would drop a selection the player still has to fix.
    let error = LowerError::Invalid {
        problems: problems.clone(),
    };
    let reported = report_problems(&error);
    assert_eq!(reported.len(), 3);
    let joined = problems.to_string();
    for needed in [
        &phantom_a.to_string(),
        &phantom_b.to_string(),
        &phantom_world.to_string(),
    ] {
        assert!(
            joined.contains(needed),
            "{needed} is in the rendered list: {joined}"
        );
    }
}

fn check_an_impossible_faction_pair(catalog: &InstantActionCatalog, base: &Base) {
    // The same faction on both sides cannot fight itself.
    let mirrored = vec![
        base.actor(
            ScenarioSide::Player,
            0,
            &base.player_faction,
            &base.airframes[0],
        ),
        base.actor(
            ScenarioSide::Enemy,
            0,
            &base.player_faction,
            &base.airframes[1],
        ),
    ];
    let problems = invalid(catalog, base.draft(mirrored, 1));
    let factions = problems.with_code(ProblemCode::ImpossibleFaction);
    assert_eq!(factions.len(), 1, "{problems}");
    assert_eq!(problems.len(), 1, "nothing else is wrong: {problems}");
    assert_eq!(
        factions[0].offender(),
        Some(base.player_faction.to_string().as_str())
    );
    let text = assert_renders(factions[0]);
    assert!(text.contains("fights its own faction"), "{text}");

    // A faction the table does not offer cannot be spawned.
    let phantom = id(ContentKind::Faction, "synthetic.ac04_phantom_faction");
    let unoffered = vec![
        base.actor(
            ScenarioSide::Player,
            0,
            &base.player_faction,
            &base.airframes[0],
        ),
        base.actor(ScenarioSide::Enemy, 0, &phantom, &base.airframes[1]),
    ];
    let problems = invalid(catalog, base.draft(unoffered, 1));
    assert_eq!(problems.len(), 1, "{problems}");
    let text = assert_renders(problems.with_code(ProblemCode::ImpossibleFaction)[0]);
    assert!(
        text.contains(&phantom.to_string()) && text.contains("cannot spawn"),
        "{text}"
    );

    // A pair the table declares friendly or neutral has nothing to shoot at.
    if let Some(ally) = &base.unfightable_faction {
        let friendly = vec![
            base.actor(
                ScenarioSide::Player,
                0,
                &base.player_faction,
                &base.airframes[0],
            ),
            base.actor(ScenarioSide::Enemy, 0, ally, &base.airframes[1]),
        ];
        let problems = invalid(catalog, base.draft(friendly, 1));
        assert_eq!(problems.len(), 1, "{problems}");
        let text = assert_renders(problems.with_code(ProblemCode::ImpossibleFaction)[0]);
        assert!(
            text.contains("cannot be fought") || text.contains("no side has a target"),
            "{text}"
        );
    }
}

fn check_a_seat_count_the_roster_cannot_fill(catalog: &InstantActionCatalog, base: &Base) {
    // Two seats over a roster with one player slot.
    let problems = invalid(catalog, base.draft(base.roster(), 2));
    assert_eq!(problems.len(), 1, "{problems}");
    let seats = problems.with_code(ProblemCode::InvalidPlayerCount);
    assert_eq!(seats.len(), 1);
    assert_eq!(seats[0].dimension(), "players");
    let text = assert_renders(seats[0]);
    assert!(
        text.contains("declares 1 player slot") && text.contains("not 2"),
        "the sentence names both numbers: {text}"
    );

    // Zero seats and a count above the ceiling are refused too, in a sentence.
    for players in [0, base.max_players_plus_one(catalog)] {
        let error = resolve_custom(catalog, base.draft(base.roster(), players))
            .expect_err("an impossible seat count never resolves");
        let text = error.to_string();
        assert!(text.contains("seats"), "actionable sentence: {text}");
        assert!(!text.contains("InvalidPlayerCount"), "not Debug: {text}");
    }
}

impl Base {
    fn max_players_plus_one(&self, catalog: &InstantActionCatalog) -> u8 {
        catalog.options().max_players() + 1
    }
}

fn check_an_incomplete_draft(catalog: &InstantActionCatalog, base: &Base) {
    let error = resolve_custom(catalog, CustomScenarioDraft::new())
        .expect_err("an empty draft is not a request");
    let LowerError::IncompleteDraft { missing, schema } = &error else {
        panic!("expected an incomplete-draft refusal, got {error:?}");
    };
    assert!(schema.is_none());
    assert_eq!(
        *missing,
        CustomScenarioDraft::new().unset_dimensions(),
        "every unset dimension is named, not the first"
    );
    for dimension in [
        "subject",
        "world",
        "environment",
        "roster",
        "skill",
        "rules",
        "seed",
        "players",
    ] {
        assert!(
            missing.contains(&dimension),
            "{dimension} is named: {missing:?}"
        );
    }
    let text = error.to_string();
    for dimension in missing {
        assert!(text.contains(dimension), "{dimension} is rendered: {text}");
    }

    // A partly filled draft names only what is still unset.
    let partial = CustomScenarioDraft::new()
        .with_world(known(base.world.clone()))
        .with_environment(known(base.environment.clone()));
    let LowerError::IncompleteDraft { missing, .. } =
        resolve_custom(catalog, partial).expect_err("still incomplete")
    else {
        panic!("expected an incomplete-draft refusal");
    };
    assert!(!missing.contains(&"world") && !missing.contains(&"environment"));
    assert!(missing.contains(&"roster"));

    // Everything set but no provenance: refused, not defaulted.
    let unsourced = CustomScenarioDraft::new()
        .with_subject(id(
            ContentKind::IaScenario,
            "synthetic.ac04_custom_scenario",
        ))
        .with_world(known(base.world.clone()))
        .with_environment(known(base.environment.clone()))
        .with_roster(base.roster())
        .with_difficulty(synthetic_difficulty(base.tier))
        .with_rules(base.rules)
        .with_seed(ScenarioSeed::new(7))
        .with_players(1);
    assert!(
        unsourced.is_complete(),
        "provenance is not a named dimension"
    );
    assert_eq!(
        schema_refusal(catalog, unsourced),
        ScenarioSchemaError::MissingProvenance
    );
}

fn check_an_unknown_is_refused_not_defaulted(catalog: &InstantActionCatalog, base: &Base) {
    let claim = ClaimId::new("f49.d.04.test.open-claim").expect("a claim id");
    let reason = "the original does not say which plane this is";
    let unknown_plane: Resolved<ContentId> =
        Resolved::unknown(claim.clone(), reason).expect("an unknown with a reason");
    let unknown_loadout: Resolved<ContentId> =
        Resolved::unknown(claim.clone(), "the loadout is unmeasured").expect("an unknown");
    let player = ScenarioActorSpec::try_new(
        ScenarioSide::Player,
        RosterSlot(0),
        base.player_faction.clone(),
        unknown_plane,
        unknown_loadout,
        None,
        known(DeclaredSurvivability::Mortal),
        synthetic_provenance(),
    )
    .expect("an unknown value passes assembly and refuses in validation");
    let roster = vec![
        player,
        base.actor(
            ScenarioSide::Enemy,
            0,
            &base.enemy_faction,
            &base.airframes[1],
        ),
    ];
    let problems = invalid(catalog, base.draft(roster.clone(), 1));
    let open = problems.with_code(ProblemCode::UnmeasuredActorField);
    assert_eq!(open.len(), 2, "the plane and the loadout: {problems}");
    for problem in &open {
        assert_eq!(problem.claim_id(), Some(&claim), "the open claim is kept");
        assert_eq!(problem.offender(), None, "there is no selection to replace");
        assert_renders(problem);
    }
    assert!(
        open.iter().any(|problem| problem.detail() == reason),
        "the recorded reason reaches the player verbatim"
    );
    assert!(
        lower_custom(catalog, base.draft(roster, 1)).is_err(),
        "an unknown plane never lowers into some other plane"
    );
}

fn check_every_problem_comes_back_at_once(catalog: &InstantActionCatalog, base: &Base) {
    let phantom = id(ContentKind::Airframe, "synthetic.ac04_phantom_airframe_a");
    let phantom_world = WorldId::from_key("synthetic.ac04_phantom_world").expect("a world key");
    // A phantom world, a phantom airframe, the same faction on both sides and
    // two seats over one slot, in one request.
    let roster = vec![
        base.actor(ScenarioSide::Player, 0, &base.player_faction, &phantom),
        base.actor(
            ScenarioSide::Enemy,
            0,
            &base.player_faction,
            &base.airframes[1],
        ),
    ];
    let problems = invalid(
        catalog,
        base.draft(roster, 2).with_world(known(phantom_world)),
    );
    let mut codes: Vec<ProblemCode> = problems.problems().iter().map(|p| p.code()).collect();
    codes.dedup();
    assert_eq!(
        codes,
        [
            ProblemCode::UnsupportedWorld,
            ProblemCode::UnsupportedAirframe,
            ProblemCode::ImpossibleFaction,
            ProblemCode::InvalidPlayerCount
        ],
        "one entry per class, in the stable order: {problems}"
    );
    assert_eq!(problems.len(), 4);
    let sentences: Vec<String> = problems.problems().iter().map(assert_renders).collect();
    assert_eq!(
        problems.to_string(),
        sentences.join("; "),
        "the screen's list is the sentences, in order"
    );
}

/// A refused selection starts nothing: no session generation, no record, no
/// half-spawn, and the player stays where the problem can be fixed.
fn check_a_refused_selection_starts_no_session(catalog: InstantActionCatalog) {
    let mut flow = InstantActionFlow::new(profile(), catalog);
    let preset = flow.preset_rows()[0].preset_id.clone();
    flow.select_preset(&preset)
        .expect("a catalog preset selects");
    flow.apply(IaAction::StartCustom {
        subject: id(ContentKind::IaScenario, "synthetic.ac04_custom_scenario"),
    })
    .expect("the custom draft opens from a preset");
    let original = flow
        .draft()
        .and_then(|draft| draft.roster())
        .and_then(|roster| {
            roster
                .iter()
                .find(|actor| actor.side() == ScenarioSide::Player && actor.slot() == RosterSlot(0))
        })
        .and_then(|actor| actor.airframe().clone().known())
        .expect("the preset's player airframe is known");
    let phantom = id(ContentKind::Airframe, "synthetic.ac04_phantom_airframe_a");
    flow.edit(IaEdit::SlotAirframe {
        side: ScenarioSide::Player,
        slot: RosterSlot(0),
        airframe: phantom,
    })
    .expect("the edit itself is well formed");

    let refusal = flow
        .apply(IaAction::Launch)
        .expect_err("the draft is refused");
    let IaRefusal::Lower(LowerError::Invalid { problems }) = &refusal else {
        panic!("expected the catalog's full problem list, got {refusal:?}");
    };
    assert_eq!(
        problems.with_code(ProblemCode::UnsupportedAirframe).len(),
        1
    );
    assert!(!refusal.to_string().is_empty());
    assert_eq!(flow.screen(), IaScreen::Customize, "the player stays put");
    assert!(
        flow.generation().is_none(),
        "no session generation was spent"
    );
    assert!(flow.running_scenario().is_none(), "nothing was spawned");
    assert!(flow.outcome().is_none());
    assert!(flow.records().is_empty(), "no record entry was written");
    assert!(
        flow.last_error().is_some(),
        "the screen can render the refusal"
    );

    // The positive control: the same flow launches once the selection is one
    // the table offers, and only then does a generation begin.
    flow.edit(IaEdit::SlotAirframe {
        side: ScenarioSide::Player,
        slot: RosterSlot(0),
        airframe: original,
    })
    .expect("the original airframe is well formed");
    flow.apply(IaAction::Launch)
        .expect("the fixed selection launches");
    assert_eq!(flow.generation(), Some(SessionGeneration(1)));
}

/// The retail half: a custom draft opened from a retail preset, edited to an
/// airframe nobody offers, is refused with the screen's own sentence and starts
/// nothing. The draft also leaves the preset's unmeasured difficulty and rules
/// unset, and the refusal names them.
fn check_retail_custom_refusal(catalog: InstantActionCatalog) {
    let mut flow = InstantActionFlow::new(profile(), catalog);
    let preset = flow.preset_rows()[0].preset_id.clone();
    flow.select_preset(&preset)
        .expect("a catalog preset selects");
    flow.apply(IaAction::StartCustom {
        subject: id(ContentKind::IaScenario, "synthetic.ac04_custom_scenario"),
    })
    .expect("the custom draft opens from a preset");
    flow.edit(IaEdit::SlotAirframe {
        side: ScenarioSide::Player,
        slot: RosterSlot(0),
        airframe: id(ContentKind::Airframe, "synthetic.ac04_phantom_airframe_a"),
    })
    .expect("the edit itself is well formed");
    let refusal = flow
        .apply(IaAction::Launch)
        .expect_err("the retail draft cannot launch");
    let IaRefusal::Lower(LowerError::IncompleteDraft { missing, .. }) = &refusal else {
        panic!("expected the unset dimensions to be named, got {refusal:?}");
    };
    assert!(
        missing.contains(&"skill") && missing.contains(&"rules"),
        "the unmeasured difficulty and rules are named, not defaulted: {missing:?}"
    );
    let text = refusal.to_string();
    assert!(text.contains("skill") && text.contains("rules"), "{text}");
    assert_eq!(flow.screen(), IaScreen::Customize);
    assert!(flow.generation().is_none());
    assert!(flow.running_scenario().is_none());
    assert!(flow.records().is_empty());
    assert!(flow.last_error().is_some());
}

// ----------------------------------------------------------------- tests ---

/// Every AC04 class on the synthetic catalog: runs on CI, where the retail
/// tests below cannot.
#[test]
fn accept_f49_d_04_synthetic_every_invalid_class_is_reported_in_full() {
    let catalog = synthetic_instant_action_catalog();
    let base = Base::from(&catalog, None);
    check_a_valid_request_is_accepted(&catalog, &base);
    check_an_empty_roster(&catalog, &base);
    check_an_airframe_nobody_offers(&catalog, &base);
    check_an_impossible_faction_pair(&catalog, &base);
    check_a_seat_count_the_roster_cannot_fill(&catalog, &base);
    check_an_incomplete_draft(&catalog, &base);
    check_an_unknown_is_refused_not_defaulted(&catalog, &base);
    check_every_problem_comes_back_at_once(&catalog, &base);
}

#[test]
fn accept_f49_d_04_synthetic_a_refused_selection_starts_no_session() {
    check_a_refused_selection_starts_no_session(synthetic_instant_action_catalog());
}

/// The pure retail catalog: every measured selection is accepted, so the only
/// thing standing between a retail-measured request and a launch is the
/// loadout the installation does not measure.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f49_d_04_retail_every_measured_selection_is_accepted_and_the_unmeasured_loadout_is_refused()
 {
    let catalog = retail();
    assert!(catalog.is_original_complete());
    assert!(catalog.options().loadouts().is_empty());
    let base = Base::from(
        catalog,
        Some(id(ContentKind::Loadout, "synthetic.ac04_standin_loadout")),
    );

    // World, environment, both factions, both airframes, tier, rules, seed and
    // seat count are all retail selections; the loadout is the only refusal and
    // it is reported once per actor, naming the selection to replace.
    let problems = invalid(catalog, base.draft(base.roster(), 1));
    assert_eq!(problems.len(), 2, "{problems}");
    for problem in problems.problems() {
        assert_eq!(problem.code(), ProblemCode::UnsupportedLoadout);
        assert_eq!(problem.dimension(), "roster");
        assert_eq!(problem.offender(), Some(base.loadout.to_string().as_str()));
        assert_renders(problem);
    }

    // With the loadout left unmeasured, as the retail presets leave it, the
    // refusal names the open claim instead of a selection.
    let claim = ClaimId::new("f49.d.04.test.loadout-unmeasured").expect("a claim id");
    let actor = |side, slot, faction: &ContentId, airframe: &ContentId| {
        ScenarioActorSpec::try_new(
            side,
            RosterSlot(slot),
            faction.clone(),
            known(airframe.clone()),
            Resolved::unknown(claim.clone(), "the original's loadouts are not measured")
                .expect("an unknown with a reason"),
            None,
            known(DeclaredSurvivability::Mortal),
            synthetic_provenance(),
        )
        .expect("an actor with an unmeasured loadout assembles")
    };
    let roster = vec![
        actor(
            ScenarioSide::Player,
            0,
            &base.player_faction,
            &base.airframes[0],
        ),
        actor(
            ScenarioSide::Enemy,
            0,
            &base.enemy_faction,
            &base.airframes[1],
        ),
    ];
    let problems = invalid(catalog, base.draft(roster, 1));
    assert_eq!(problems.len(), 2, "{problems}");
    for problem in problems.problems() {
        assert_eq!(problem.code(), ProblemCode::UnmeasuredActorField);
        assert_eq!(problem.claim_id(), Some(&claim));
        assert_renders(problem);
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f49_d_04_retail_a_valid_request_from_the_retail_lists_is_accepted() {
    let catalog = standin_catalog(retail());
    let base = Base::from(&catalog, None);
    check_a_valid_request_is_accepted(&catalog, &base);
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f49_d_04_retail_an_empty_roster_is_refused_with_a_sentence() {
    let catalog = standin_catalog(retail());
    check_an_empty_roster(&catalog, &Base::from(&catalog, None));
    // And on the pure retail catalog, where the refusal happens before any
    // option table is consulted.
    let pure = retail();
    let base = Base::from(
        pure,
        Some(id(ContentKind::Loadout, "synthetic.ac04_standin_loadout")),
    );
    assert_eq!(
        schema_refusal(pure, base.draft(Vec::new(), 1)),
        ScenarioSchemaError::EmptyRoster
    );
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f49_d_04_retail_an_airframe_no_option_table_offers_is_refused_with_every_other_problem() {
    let catalog = standin_catalog(retail());
    check_an_airframe_nobody_offers(&catalog, &Base::from(&catalog, None));
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f49_d_04_retail_an_impossible_faction_pair_is_refused() {
    let catalog = standin_catalog(retail());
    let base = Base::from(&catalog, None);
    assert!(
        base.unfightable_faction.is_some(),
        "the stand-in table declares a friendly pair to refuse"
    );
    check_an_impossible_faction_pair(&catalog, &base);
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f49_d_04_retail_a_seat_count_the_roster_cannot_fill_is_refused() {
    let catalog = standin_catalog(retail());
    check_a_seat_count_the_roster_cannot_fill(&catalog, &Base::from(&catalog, None));
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f49_d_04_retail_an_incomplete_draft_names_every_unset_dimension() {
    let catalog = standin_catalog(retail());
    check_an_incomplete_draft(&catalog, &Base::from(&catalog, None));
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f49_d_04_retail_an_unknown_is_refused_with_its_claim_never_defaulted() {
    let catalog = standin_catalog(retail());
    check_an_unknown_is_refused_not_defaulted(&catalog, &Base::from(&catalog, None));
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f49_d_04_retail_every_problem_of_a_request_comes_back_at_once() {
    let catalog = standin_catalog(retail());
    check_every_problem_comes_back_at_once(&catalog, &Base::from(&catalog, None));
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f49_d_04_retail_a_refused_selection_starts_no_session() {
    // The preset's own actors carry unmeasured factions and loadouts, so the
    // pure retail catalog cannot launch even the unedited draft; the refusal
    // below is the retail half and the positive control is the synthetic run.
    check_retail_custom_refusal(retail().clone());
}

/// No retail preset launches today (F49-D.03): launching one is refused before
/// any runtime identity exists, never as an endless or half-started session.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f49_d_04_retail_launching_a_preset_whose_facts_are_unmeasured_starts_no_session() {
    let mut flow = InstantActionFlow::new(profile(), retail().clone());
    let rows = flow.preset_rows();
    assert_eq!(rows.len(), 30);
    for row in rows {
        flow.select_preset(&row.preset_id).expect("selects");
        let refusal = flow
            .apply(IaAction::Launch)
            .expect_err("an unmeasured fact refuses the launch");
        assert!(matches!(refusal, IaRefusal::Lower(_)), "{refusal:?}");
        assert!(!refusal.to_string().is_empty());
        assert_eq!(flow.screen(), IaScreen::Select);
        assert!(
            flow.generation().is_none(),
            "{}: no generation",
            row.preset_id
        );
        assert!(flow.running_scenario().is_none());
        assert!(flow.records().is_empty());
    }
}
