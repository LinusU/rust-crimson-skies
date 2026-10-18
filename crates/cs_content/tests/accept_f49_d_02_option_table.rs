//! Acceptance scenario F49-D.02: the retail custom-scenario option table
//! (`ScenarioOptions`) assembled from the collections earlier evidence stages
//! measured.
//!
//! Spec: `specs/F49-instant-action-presets-and-custom-scenarios.md` (deliverable
//! "all supported custom-scenario dimensions discovered from data/UI";
//! non-negotiable 2). Task test prefix: `accept_f49_d_02_`. Required capability:
//! **retail** (`$CS_GAME_DIR`).
//!
//! The synthetic half drives the production schema directly:
//! [`cs_content::instant_action::ScenarioOptions::try_new`] must refuse every
//! malformed option table with its own error, and
//! [`cs_content::instant_action::ScenarioOptionTable`] must refuse a table that
//! does not say how a list was established and must report the dimensions no
//! production reader measured.
//!
//! The retail half (`#[ignore = "requires CS_GAME_DIR"]`) re-derives every
//! measured list from the owner's installation through the same production
//! readers the constructor uses, so a typed constant standing in for a
//! measurement fails instead of passing. A dimension with no producer must be
//! reported as unmeasured (or declared design), never silently filled from the
//! fixture.
//!
//! No original content is committed; the retail test reads the installation
//! read-only and retains nothing.

use std::path::PathBuf;

use cs_content::ai::DifficultyTier;
use cs_content::catalog::baseline::retail_baseline;
use cs_content::environment::EnvironmentId;
use cs_content::instant_action::{
    MAX_SCENARIO_PLAYERS, OptionListEvidence, ScenarioOptionDimension, ScenarioOptionTable,
    ScenarioOptions, ScenarioProblemCode, ScenarioSchemaError, VictoryCondition,
    synthetic_instant_action_catalog, synthetic_scenario_options,
};
use cs_content::target_rules::DeclaredRelation;
use cs_content::world::WorldId;
use cs_types::content::{ContentId, ContentKind};

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("the test content id is valid")
}

fn world(key: &str) -> WorldId {
    WorldId::from_key(key).expect("the test world key is valid")
}

fn environment(key: &str) -> EnvironmentId {
    EnvironmentId::new(key).expect("the test environment key is valid")
}

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("CS_GAME_DIR")
            .expect("CS_GAME_DIR must name the original installation for this retail test"),
    )
}

/// The installation's baseline, read once and shared by the retail tests.
///
/// `retail_baseline` walks the whole installation; the two retail tests need
/// the same bytes, so the expensive read happens once per test process rather
/// than once per test.
fn retail_baseline_cached() -> &'static cs_content::catalog::baseline::Baseline {
    static BASELINE: std::sync::OnceLock<cs_content::catalog::baseline::Baseline> =
        std::sync::OnceLock::new();
    BASELINE.get_or_init(|| retail_baseline(&game_dir()).expect("the original installation reads"))
}

/// The retail option table, assembled over the cached baseline.
fn retail_table() -> ScenarioOptionTable {
    cs_content::instant_action::retail_scenario_options_from_baseline(
        retail_baseline_cached(),
        Vec::new(),
    )
}

// ------------------------------------------------------------ synthetic ---

/// An empty option list is refused, naming the list.
#[test]
fn accept_f49_d_02_an_empty_option_list_is_refused_naming_it() {
    let base = synthetic_scenario_options();
    let cases: [(&'static str, Result<ScenarioOptions, ScenarioSchemaError>); 8] = [
        ("worlds", rebuild(&base, |args| args.worlds.clear())),
        (
            "environments",
            rebuild(&base, |args| args.environments.clear()),
        ),
        ("factions", rebuild(&base, |args| args.factions.clear())),
        ("airframes", rebuild(&base, |args| args.airframes.clear())),
        ("loadouts", rebuild(&base, |args| args.loadouts.clear())),
        ("pilots", rebuild(&base, |args| args.pilots.clear())),
        (
            "difficulty tiers or victory conditions",
            rebuild(&base, |args| args.difficulty_tiers.clear()),
        ),
        (
            "difficulty tiers or victory conditions",
            rebuild(&base, |args| args.victory_conditions.clear()),
        ),
    ];
    for (what, result) in cases {
        assert_eq!(
            result.expect_err("an empty list must be refused"),
            ScenarioSchemaError::EmptyOptionTable { what },
            "an empty {what} list must be refused by the table, not defaulted"
        );
    }
}

/// An id in the wrong namespace is refused, naming the list and the expected
/// namespace.
#[test]
fn accept_f49_d_02_an_id_in_the_wrong_namespace_is_refused() {
    let base = synthetic_scenario_options();
    let wrong = cid(ContentKind::Airframe, "synthetic.wrong_namespace");
    let result = rebuild(&base, |args| {
        args.factions = vec![wrong.clone()];
    });
    assert_eq!(
        result.expect_err("an airframe id is not a faction"),
        ScenarioSchemaError::OptionKindMismatch {
            what: "factions",
            id: wrong,
            expected: ContentKind::Faction,
        }
    );
}

/// A duplicated id is refused, naming the list and the entry.
#[test]
fn accept_f49_d_02_a_duplicate_id_is_refused() {
    let base = synthetic_scenario_options();
    let duplicated = world("synthetic.duplicate");
    let result = rebuild(&base, |args| {
        args.worlds = vec![duplicated.clone(), duplicated.clone()];
    });
    assert_eq!(
        result.expect_err("a repeated world is refused"),
        ScenarioSchemaError::DuplicateOption {
            what: "worlds",
            entry: "world/synthetic.duplicate".to_owned(),
        }
    );
}

/// A duplicated directed relation pair is refused, not merely deduplicated.
#[test]
fn accept_f49_d_02_a_duplicate_directed_pair_is_refused() {
    let base = synthetic_scenario_options();
    let result = rebuild(&base, |args| {
        let first = args
            .relations
            .first()
            .cloned()
            .expect("the fixture relation");
        args.relations.push(first);
    });
    assert!(
        matches!(
            result.expect_err("a repeated ordered pair is refused"),
            ScenarioSchemaError::DuplicateOption {
                what: "relations",
                ..
            }
        ),
        "a repeated ordered pair must be refused"
    );
}

/// The seat limit must be inside `1..=MAX_SCENARIO_PLAYERS`.
#[test]
fn accept_f49_d_02_a_zero_or_oversized_player_limit_is_refused() {
    let base = synthetic_scenario_options();
    for limit in [0u8, MAX_SCENARIO_PLAYERS + 1] {
        let result = rebuild(&base, |args| args.max_players = limit);
        assert_eq!(
            result.expect_err("a seat limit out of range is refused"),
            ScenarioSchemaError::InvalidPlayerLimit {
                max: limit,
                ceiling: MAX_SCENARIO_PLAYERS,
            }
        );
    }
}

/// The retail difficulty list is the measured steps: the designed extension
/// tier stays out unless the installation declares it.
///
/// The closed vocabulary is a Rust enum, so "a tier outside the vocabulary" is
/// not representable; what *is* checkable is that the measured steps are the
/// three the installation declares and that the extension is excluded.
#[test]
fn accept_f49_d_02_a_designed_extension_tier_stays_out_of_the_measured_steps() {
    let measured: Vec<DifficultyTier> = DifficultyTier::ALL
        .iter()
        .copied()
        .filter(|tier| tier.measured_step().is_some())
        .collect();
    assert_eq!(measured.len(), DifficultyTier::measured_tier_count());
    assert_eq!(
        measured.len(),
        3,
        "the original option has three measured steps"
    );
    assert!(
        DifficultyTier::Elite.is_designed_extension(),
        "the fourth declared tier is an extension with no measured counterpart"
    );
    assert!(
        !measured.contains(&DifficultyTier::Elite),
        "the extension tier must not stand in for a measured step"
    );
}

/// The option-table problem codes a screen uses name the dimensions the option
/// table validates, so a refusal can highlight the right control.
#[test]
fn accept_f49_d_02_every_refusal_names_its_dimension() {
    assert_eq!(ScenarioProblemCode::UnsupportedWorld.dimension(), "world");
    assert_eq!(
        ScenarioProblemCode::UnsupportedEnvironment.dimension(),
        "environment"
    );
    assert_eq!(
        ScenarioProblemCode::UnsupportedAirframe.dimension(),
        "roster"
    );
    assert_eq!(
        ScenarioProblemCode::UnsupportedLoadout.dimension(),
        "roster"
    );
    assert_eq!(ScenarioProblemCode::UnsupportedPilot.dimension(), "roster");
    assert_eq!(
        ScenarioProblemCode::UnsupportedVictoryCondition.dimension(),
        "rules"
    );
    assert_eq!(
        ScenarioProblemCode::InvalidPlayerCount.dimension(),
        "players"
    );
    assert_eq!(
        ScenarioProblemCode::UnsupportedDifficulty.dimension(),
        "skill"
    );
}

// ------------------------------------------------------------- assembly ---

/// A table that does not say how one of its lists was established is refused.
#[test]
fn accept_f49_d_02_a_table_missing_a_dimensions_evidence_is_refused() {
    let base = synthetic_scenario_options();
    let evidence: Vec<(ScenarioOptionDimension, OptionListEvidence)> = ScenarioOptionDimension::ALL
        .iter()
        .copied()
        .filter(|dimension| *dimension != ScenarioOptionDimension::Airframes)
        .map(|dimension| {
            (
                dimension,
                OptionListEvidence::Measured {
                    reader: "a test reader",
                },
            )
        })
        .collect();
    let error = ScenarioOptionTable::from_lists(
        base.worlds().to_vec(),
        base.environments().to_vec(),
        base.factions().to_vec(),
        base.airframes().to_vec(),
        base.loadouts().to_vec(),
        base.pilots().to_vec(),
        base.difficulty_tiers().to_vec(),
        base.victory_conditions().to_vec(),
        base.relations().to_vec(),
        base.max_players(),
        evidence,
    )
    .expect_err("a dimension with no evidence record is refused");
    assert_eq!(
        error,
        ScenarioSchemaError::MissingOptionEvidence {
            dimension: ScenarioOptionDimension::Airframes,
        }
    );
}

/// A table whose unmeasured list is empty refuses that list by name, and
/// reports the dimension as unmeasured rather than silently filling it.
#[test]
fn accept_f49_d_02_an_unmeasured_empty_list_is_refused_by_name() {
    let base = synthetic_scenario_options();
    let table = ScenarioOptionTable::from_lists(
        base.worlds().to_vec(),
        base.environments().to_vec(),
        base.factions().to_vec(),
        base.airframes().to_vec(),
        Vec::new(),
        Vec::new(),
        base.difficulty_tiers().to_vec(),
        base.victory_conditions().to_vec(),
        Vec::new(),
        base.max_players(),
        ScenarioOptionDimension::ALL
            .iter()
            .copied()
            .map(|dimension| {
                let evidence = match dimension {
                    ScenarioOptionDimension::Loadouts
                    | ScenarioOptionDimension::Pilots
                    | ScenarioOptionDimension::Relations => OptionListEvidence::Unmeasured {
                        reason: "no producer",
                        resolving_task: "a follow-up task",
                    },
                    _ => OptionListEvidence::Measured {
                        reader: "a test reader",
                    },
                };
                (dimension, evidence)
            })
            .collect(),
    )
    .expect("every dimension is named");

    assert_eq!(table.empty_required_lists(), vec!["loadouts", "pilots"]);
    assert_eq!(
        table.unmeasured_dimensions(),
        vec![
            ScenarioOptionDimension::Loadouts,
            ScenarioOptionDimension::Pilots,
            ScenarioOptionDimension::Relations,
        ]
    );
    assert_eq!(
        table
            .validate()
            .expect_err("an empty required list is refused"),
        ScenarioSchemaError::EmptyOptionTable { what: "loadouts" }
    );
}

/// A fully measured table validates and keeps its per-dimension evidence.
#[test]
fn accept_f49_d_02_a_complete_table_validates_and_keeps_its_evidence() {
    let base = synthetic_scenario_options();
    let table = ScenarioOptionTable::from_lists(
        base.worlds().to_vec(),
        base.environments().to_vec(),
        base.factions().to_vec(),
        base.airframes().to_vec(),
        base.loadouts().to_vec(),
        base.pilots().to_vec(),
        base.difficulty_tiers().to_vec(),
        base.victory_conditions().to_vec(),
        base.relations().to_vec(),
        base.max_players(),
        ScenarioOptionDimension::ALL
            .iter()
            .copied()
            .map(|dimension| {
                (
                    dimension,
                    OptionListEvidence::Measured {
                        reader: "a test reader",
                    },
                )
            })
            .collect(),
    )
    .expect("every dimension is named");
    assert!(table.empty_required_lists().is_empty());
    assert_eq!(
        table.measured_dimensions(),
        ScenarioOptionDimension::ALL.to_vec()
    );
    let validated = table.validate().expect("a complete table validates");
    assert_eq!(validated.worlds(), base.worlds());
    assert_eq!(validated.max_players(), base.max_players());
}

// ---------------------------------------------------------------- retail ---

/// The retail table: every measured list is re-derived from the installation
/// through production readers, every id is in its namespace, and each
/// dimension's evidence matches its class.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f49_d_02_retail_every_measured_list_is_rederived_from_the_installation() {
    let baseline = retail_baseline_cached();
    let table = retail_table();

    // Worlds: re-derived from the baseline's own world rows, in canonical
    // order, not from a typed constant.
    let expected_worlds: Vec<WorldId> = baseline
        .catalog
        .elements()
        .filter(|element| element.kind == ContentKind::World)
        .filter_map(|element| WorldId::new(element.id.clone()).ok())
        .collect();
    assert!(!expected_worlds.is_empty(), "the installation names worlds");
    assert_eq!(table.worlds(), expected_worlds.as_slice());

    // Factions and airframes: the same re-derivation.
    for (got, kind, label) in [
        (table.factions(), ContentKind::Faction, "factions"),
        (table.airframes(), ContentKind::Airframe, "airframes"),
    ] {
        let expected: Vec<ContentId> = baseline
            .catalog
            .elements()
            .filter(|element| element.kind == kind)
            .map(|element| element.id.clone())
            .collect();
        assert!(!expected.is_empty(), "the installation names {label}");
        assert_eq!(got, expected.as_slice(), "{label} must be re-derived");
        assert!(
            got.iter().all(|id| id.kind() == kind),
            "every {label} id is in the {kind:?} namespace"
        );
    }

    // Difficulty: the measured steps, and only those.
    let expected_tiers: Vec<DifficultyTier> = DifficultyTier::ALL
        .iter()
        .copied()
        .filter(|tier| tier.measured_step().is_some())
        .collect();
    assert_eq!(table.difficulty_tiers(), expected_tiers.as_slice());
    assert!(
        !table.difficulty_tiers().contains(&DifficultyTier::Elite),
        "the designed extension tier is not in the retail table"
    );

    // Evidence classes: measured lists are measured, the declared vocabulary is
    // designed, and the lists with no producer are unmeasured.
    for dimension in [
        ScenarioOptionDimension::Worlds,
        ScenarioOptionDimension::Factions,
        ScenarioOptionDimension::Airframes,
        ScenarioOptionDimension::DifficultyTiers,
    ] {
        assert!(
            table
                .evidence(dimension)
                .expect("a named dimension")
                .is_measured(),
            "{dimension} must be measured from the installation"
        );
    }
    for dimension in [
        ScenarioOptionDimension::VictoryConditions,
        ScenarioOptionDimension::MaxPlayers,
    ] {
        assert!(
            matches!(
                table.evidence(dimension).expect("a named dimension"),
                OptionListEvidence::Designed { .. }
            ),
            "{dimension} is declared design, never an original measurement"
        );
    }
    for dimension in [
        ScenarioOptionDimension::Environments,
        ScenarioOptionDimension::Loadouts,
        ScenarioOptionDimension::Pilots,
        ScenarioOptionDimension::Relations,
    ] {
        let evidence = table.evidence(dimension).expect("a named dimension");
        assert!(
            evidence.is_unmeasured(),
            "{dimension} has no production reader here, so it must be reported unmeasured"
        );
        match evidence {
            OptionListEvidence::Unmeasured {
                reason,
                resolving_task,
            } => {
                assert!(!reason.is_empty(), "{dimension}: a reason is recorded");
                assert!(
                    !resolving_task.is_empty(),
                    "{dimension}: a resolving task is recorded"
                );
            }
            _ => unreachable!("checked above"),
        }
    }

    // No unmeasured list is silently populated from the fixture.
    assert!(table.loadouts().is_empty());
    assert!(table.pilots().is_empty());
    assert!(table.relations().is_empty());
    assert!(table.environments().is_empty());
    assert_eq!(
        table.empty_required_lists(),
        vec!["environments", "loadouts", "pilots"],
        "the empty axes are named, not hidden"
    );
    assert_eq!(
        table
            .clone()
            .validate()
            .expect_err("an unmeasured required list refuses the table"),
        ScenarioSchemaError::EmptyOptionTable {
            what: "environments"
        }
    );

    // The declared victory vocabulary is the sheet's, and it is offered.
    assert_eq!(table.victory_conditions(), VictoryCondition::ALL);
    assert_eq!(table.max_players(), MAX_SCENARIO_PLAYERS);

    // Every measured id is a real installation row, so the fixture can never
    // stand in for one.
    let fixture = synthetic_scenario_options();
    for id in table
        .factions()
        .iter()
        .chain(table.airframes().iter())
        .chain(table.pilots().iter())
        .chain(table.loadouts().iter())
    {
        assert!(
            !fixture.factions().contains(id)
                && !fixture.airframes().contains(id)
                && !fixture.pilots().contains(id)
                && !fixture.loadouts().contains(id),
            "{id} is a fixture id, not an installation row"
        );
    }
}

/// The drift guard: the retail table must not equal the fixture table
/// field-for-field, so "the constructor returned the fixture" fails here.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f49_d_02_the_retail_table_is_not_the_fixture_table() {
    let table = retail_table();
    let fixture = synthetic_scenario_options();
    assert_ne!(
        table.worlds(),
        fixture.worlds(),
        "the retail world list is the installation's, not the fixture's"
    );
    assert_ne!(table.factions(), fixture.factions());
    assert_ne!(table.airframes(), fixture.airframes());
    assert!(
        table.loadouts().is_empty() && !fixture.loadouts().is_empty(),
        "the fixture offers loadouts the retail table has no producer for"
    );
    assert!(
        table.pilots().is_empty() && !fixture.pilots().is_empty(),
        "the fixture offers pilots the retail table has no producer for"
    );
    assert_ne!(
        table.max_players(),
        fixture.max_players(),
        "the retail seat bound is the module's, not the fixture's"
    );
    assert!(
        table
            .factions()
            .iter()
            .all(|id| !fixture.factions().contains(id)),
        "no fixture faction is presented as a retail one"
    );
    // And a catalog built from the fixture is not the installation's: the
    // catalog's own original-completeness flag must stay false while the fixture
    // is the only catalog.
    let catalog = synthetic_instant_action_catalog();
    assert!(
        !catalog.is_original_complete(),
        "a fixture catalog is never original-complete"
    );
    assert!(
        catalog
            .presets()
            .iter()
            .all(|preset| !preset.origin().is_original()),
        "no fixture preset claims an installation origin"
    );
}

/// The measured environment list the caller supplies is carried through and
/// tagged measured, so the F19/F20 weather reader's ids reach the table.
///
/// This is the synthetic half of the environment dimension: the retail test in
/// `cs_app` supplies the ids the production weather reader measured, while
/// here a caller-supplied list is shown to be preserved and classified
/// measured, never dropped or re-derived.
#[test]
fn accept_f49_d_02_a_measured_environment_list_is_carried_and_tagged() {
    let base = synthetic_scenario_options();
    let supplied = vec![environment("synthetic.caller.supplied")];
    let table = ScenarioOptionTable::from_lists(
        base.worlds().to_vec(),
        supplied.clone(),
        base.factions().to_vec(),
        base.airframes().to_vec(),
        base.loadouts().to_vec(),
        base.pilots().to_vec(),
        base.difficulty_tiers().to_vec(),
        base.victory_conditions().to_vec(),
        base.relations().to_vec(),
        base.max_players(),
        ScenarioOptionDimension::ALL
            .iter()
            .copied()
            .map(|dimension| {
                (
                    dimension,
                    OptionListEvidence::Measured {
                        reader: "a test reader",
                    },
                )
            })
            .collect(),
    )
    .expect("every dimension is named");
    assert_eq!(table.environments(), supplied.as_slice());
    assert!(
        table
            .evidence(ScenarioOptionDimension::Environments)
            .expect("a named dimension")
            .is_measured()
    );
}

// ------------------------------------------------------------- helpers ---

/// A mutable view of the fixture table's lists, so a test can empty or corrupt
/// exactly one of them and rebuild the table.
struct Args {
    worlds: Vec<WorldId>,
    environments: Vec<EnvironmentId>,
    factions: Vec<ContentId>,
    airframes: Vec<ContentId>,
    loadouts: Vec<ContentId>,
    pilots: Vec<ContentId>,
    difficulty_tiers: Vec<DifficultyTier>,
    victory_conditions: Vec<VictoryCondition>,
    relations: Vec<DeclaredRelation>,
    max_players: u8,
}

/// Rebuilds the fixture table with one list mutated, through the production
/// `ScenarioOptions::try_new` that validates each list.
fn rebuild(
    base: &ScenarioOptions,
    mutate: impl FnOnce(&mut Args),
) -> Result<ScenarioOptions, ScenarioSchemaError> {
    let mut args = Args {
        worlds: base.worlds().to_vec(),
        environments: base.environments().to_vec(),
        factions: base.factions().to_vec(),
        airframes: base.airframes().to_vec(),
        loadouts: base.loadouts().to_vec(),
        pilots: base.pilots().to_vec(),
        difficulty_tiers: base.difficulty_tiers().to_vec(),
        victory_conditions: base.victory_conditions().to_vec(),
        relations: base.relations().to_vec(),
        max_players: base.max_players(),
    };
    mutate(&mut args);
    ScenarioOptions::try_new(
        args.worlds,
        args.environments,
        args.factions,
        args.airframes,
        args.loadouts,
        args.pilots,
        args.difficulty_tiers,
        args.victory_conditions,
        args.relations,
        args.max_players,
    )
}

/// The measured negative behind the unmeasured `loadouts` list (#1575): the
/// original's Instant Action screen edits a loadout per slot through the
/// ordinance-layout editor and carries no loadout identity list. The reason the
/// retail table reports must therefore name those two members.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f49_d_02_retail_loadouts_are_a_measured_negative_not_an_identity_list() {
    use cs_content::instant_action::{
        IA_LOADOUT_EDITOR_MEMBER, IA_LOADOUT_SCREEN_MEMBER, LOADOUT_UNMEASURED_REASON,
    };

    let root = crate::f27_d_support::game_dir();
    let find = crate::f27_d_support::find;
    let screen = crate::f27_d_support::read_member(&root, IA_LOADOUT_SCREEN_MEMBER);
    let editor = crate::f27_d_support::read_member(&root, IA_LOADOUT_EDITOR_MEMBER);

    // The screen's change-weapons button runs the editor for the player (-1)
    // or a wingman (-2).
    for needle in [
        &b"ia_b_changeweapons"[..],
        b"@globals@ZQ = -1",
        b"@globals@ZQ = -2",
        b"ordinancelayout.script",
    ] {
        assert!(
            find(&screen, needle).is_some(),
            "{IA_LOADOUT_SCREEN_MEMBER} no longer carries {:?}",
            String::from_utf8_lossy(needle)
        );
    }
    // The editor selects per slot by index through engine callbacks.
    for needle in [
        &b"callback($$E$$, 2030, (ZQ), (BA)"[..],
        b"callback($$E$$, 2036, (ZQ), (BA)",
    ] {
        assert!(
            find(&editor, needle).is_some(),
            "{IA_LOADOUT_EDITOR_MEMBER} no longer carries {:?}",
            String::from_utf8_lossy(needle)
        );
    }
    // Neither member names or enumerates a loadout.
    for (member, bytes) in [(&screen, "screen"), (&editor, "editor")].map(|(b, m)| (m, b)) {
        assert!(
            !bytes
                .to_ascii_lowercase()
                .windows(7)
                .any(|w| w == b"loadout"),
            "the {member} names a loadout, so an identity list may exist"
        );
    }

    // And the production table says exactly that, naming both members.
    let table = retail_table();
    match table
        .evidence(ScenarioOptionDimension::Loadouts)
        .expect("a named dimension")
    {
        OptionListEvidence::Unmeasured { reason, .. } => {
            assert_eq!(*reason, LOADOUT_UNMEASURED_REASON);
            assert!(reason.contains("ORDINANCELAYOUT.SCRIPT"));
            assert!(reason.contains("no loadout identity list"));
        }
        other => panic!("loadouts must stay unmeasured, got {}", other.label()),
    }
    assert!(table.loadouts().is_empty());
}

// ------------------------------------------------------------ relations ---

/// The installation states allegiance only as a per-record team spelling in the
/// loader's own vocabulary, and no record names a faction row, so no directed
/// faction relation exists to list; the table's relations stay empty with the
/// measured negative as the reason.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f49_d_02_retail_relations_are_a_measured_negative() {
    let table = retail_table();
    let measurement =
        cs_content::target_rules::measure_retail_relation_surface(&game_dir(), table.factions())
            .expect("the installation's relation surface reads");

    assert!(
        !measurement.team_spellings().is_empty(),
        "the installation states some team spellings, so the negative is measured over data"
    );
    assert!(
        measurement.outside_vocabulary().is_empty(),
        "every team spelling is in the loader's vocabulary: {:?}",
        measurement.outside_vocabulary()
    );
    assert!(!measurement.states_cross_faction_relation());
    assert!(table.relations().is_empty());
    match table
        .evidence(ScenarioOptionDimension::Relations)
        .expect("a named dimension")
    {
        OptionListEvidence::Unmeasured { reason, .. } => {
            assert!(reason.starts_with("measured negative"), "{reason}");
        }
        other => panic!("relations must record the measured negative, got {other:?}"),
    }
}

/// A faction key used as a team spelling is a stated relation surface; the
/// negative is only as good as this check, so it is exercised on its own.
#[test]
fn accept_f49_d_02_the_team_vocabulary_is_the_loaders_three_words() {
    let words: Vec<_> = cs_content::target_rules::ORIGINAL_TEAM_VOCABULARY
        .iter()
        .map(|(word, number)| (*word, *number))
        .collect();
    assert_eq!(words, vec![("neutral", 0), ("ally", 1), ("enemy", 2)]);
}
