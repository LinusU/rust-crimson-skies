//! Acceptance scenario F49-D.03: the complete retail `InstantActionCatalog`
//! and AC01 ("launch each discovered preset and verify its expected actors,
//! world and rules"), as far as the installation measures them.
//!
//! Spec: `specs/F49-instant-action-presets-and-custom-scenarios.md`,
//! non-negotiable 1 and AC01. Task test prefix: `accept_f49_d_03_`. Required
//! capability: **retail** (`$CS_GAME_DIR`), read-only; nothing is rendered,
//! played or run.
//!
//! The retail tests assemble the catalog through
//! `cs_app::ui::instant_action::retail_instant_action_catalog` and check it
//! against an independent reading of the installation (the airframe table, the
//! survey, a `std::fs` walk, the weather reader). Without `CS_GAME_DIR` they
//! fail loudly. The synthetic tests drive the same production assembly over
//! hand-built descriptors and the F49-A schema, so the refusals they name are
//! exercised on CI too.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::OnceLock;

use cs_app::mission_start::AIRFRAME_TABLE;
use cs_app::ui::instant_action::{
    DIFFICULTY_UNMEASURED_REASON, DirectoryInputs, FACTION_UNMEASURED_REASON, IA_CATALOG_CLAIM,
    IA_PRESET_CLAIM, IaAction, IaEdit, IaScreen, InstantActionFlow, LOADOUT_UNMEASURED_REASON,
    LowerError, RetailCatalogError, directory_presets, directory_roster, lower_preset,
    lower_preset_roster, preset_seed, retail_instant_action_catalog,
};
use cs_content::ai::{DeclaredSkillTier, DifficultyTier};
use cs_content::instant_action::{
    Declared, IaDenominator, InstantActionCatalog, InstantActionPreset, RespawnBudget,
    ScenarioActorSpec, ScenarioOptions, ScenarioParameters, ScenarioRoster, ScenarioSchemaError,
    ScenarioSide, TieOutcome, VictoryCondition, VictoryRules, read_ia_descriptor,
    survey_retail_instant_action, synthetic_difficulty, synthetic_instant_action_catalog,
    synthetic_provenance, synthetic_scenario_options,
};
use cs_content::stunts::decode_zrd;
use cs_content::world::WorldId;
use cs_sim::allies::SurvivabilityPolicy;
use cs_types::asset_id::SourceSpan;
use cs_types::content::{ContentId, ContentKind, Origin, Resolved};
use cs_types::evidence::ClaimId;

// ------------------------------------------------------------ retail setup ---

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("CS_GAME_DIR")
            .expect("CS_GAME_DIR must name the original installation for this retail test"),
    )
}

struct Retail {
    catalog: InstantActionCatalog,
    denominator: IaDenominator,
}

/// The retail catalog and the survey it must agree with, read once.
fn retail() -> &'static Retail {
    static RETAIL: OnceLock<Retail> = OnceLock::new();
    RETAIL.get_or_init(|| {
        let dir = game_dir();
        Retail {
            catalog: retail_instant_action_catalog(&dir)
                .expect("the retail catalog assembles from the installation"),
            denominator: survey_retail_instant_action(&dir)
                .expect("the production survey reads the installation"),
        }
    })
}

/// The airframe id the executable's table gives a display name, read straight
/// from `AIRFRAME_TABLE` (not through `airframe_entry`), so the assembly's
/// mapping is checked against an independent lookup.
fn table_airframe(display_name: &str) -> ContentId {
    let row = AIRFRAME_TABLE
        .iter()
        .find(|row| row.display_name.eq_ignore_ascii_case(display_name))
        .unwrap_or_else(|| panic!("`{display_name}` is a row of the airframe table"));
    ContentId::from_source(ContentKind::Airframe, row.scene_root)
        .expect("a table scene root is a valid airframe key")
}

fn group_of(container: &str) -> &str {
    container
        .strip_prefix("zbd/")
        .and_then(|rest| rest.split('/').next())
        .expect("a container key is zbd/<group>/<leaf>/zrdr.zbd")
}

// ---------------------------------------------------------- retail: catalog ---

/// One preset per measured (directory, offered type), every one backed by the
/// installation.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f49_d_03_retail_catalog_holds_every_measured_preset_from_the_installation() {
    let retail = retail();

    // An independent count of the IA directories on disk.
    let mut on_disk = 0usize;
    let zbd = game_dir().join("ZBD");
    for group in std::fs::read_dir(&zbd)
        .expect("the ZBD directory is readable")
        .flatten()
    {
        let Ok(leaves) = std::fs::read_dir(group.path()) else {
            continue;
        };
        for leaf in leaves.flatten() {
            let name = leaf.file_name().to_string_lossy().to_ascii_lowercase();
            if name.strip_prefix("ia").is_some_and(|digits| {
                !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
            }) {
                on_disk += 1;
            }
        }
    }
    assert_eq!(on_disk, 8, "the installation holds eight IA directories");
    assert_eq!(retail.denominator.directories().len(), on_disk);

    let expected: BTreeSet<ContentId> = retail
        .denominator
        .preset_ids()
        .expect("the survey names every preset id")
        .into_iter()
        .map(|(preset, _)| preset)
        .collect();
    assert_eq!(expected.len(), 30, "F49-D.01 measured thirty preset ids");
    let held: BTreeSet<ContentId> = retail.catalog.preset_ids().into_iter().cloned().collect();
    assert_eq!(
        held, expected,
        "the catalog holds exactly the measured presets"
    );

    let scenarios: BTreeSet<&ContentId> = retail
        .catalog
        .presets()
        .iter()
        .map(|preset| preset.scenario())
        .collect();
    assert_eq!(scenarios.len(), 8, "thirty presets launch eight scenarios");

    assert!(
        retail.catalog.is_original_complete(),
        "every preset is backed by installation data"
    );
    for preset in retail.catalog.presets() {
        let Origin::Installation { source } = preset.origin() else {
            panic!("{} is not installation-backed", preset.id());
        };
        assert_eq!(source.member_key(), Some("ia.zrd"));
        assert!(
            source
                .container_path()
                .to_ascii_lowercase()
                .ends_with("zrdr.zbd")
        );
        assert_eq!(preset.provenance().claim_id.as_str(), IA_PRESET_CLAIM);
        assert!(preset.origin().is_original());
    }
    assert_eq!(
        retail.catalog.provenance().claim_id.as_str(),
        IA_CATALOG_CLAIM
    );
}

/// A preset keeps the mission type its id names, and several presets of one
/// scenario are told apart by it.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f49_d_03_retail_each_preset_selects_its_measured_mission_type() {
    let retail = retail();
    for directory in retail.denominator.directories() {
        let scenario = directory.scenario_id().expect("a scenario id");
        let presets = retail.catalog.presets_for_scenario(&scenario);
        let held: Vec<&str> = presets
            .iter()
            .map(|preset| {
                preset
                    .original_mission_type()
                    .expect("a retail preset names its mission type")
            })
            .collect();
        let mut offered: Vec<&str> = directory
            .offered()
            .iter()
            .map(|offered| offered.mission_type())
            .collect();
        let mut sorted = held.clone();
        sorted.sort_unstable();
        offered.sort_unstable();
        assert_eq!(sorted, offered, "{scenario}: one preset per offered type");
        for preset in presets {
            assert!(
                preset.id().as_str().ends_with(
                    preset
                        .original_mission_type()
                        .expect("a retail preset names its mission type")
                ),
                "{} names its mission type",
                preset.id()
            );
        }
    }
}

// ------------------------------------------------------- retail: AC01 actors ---

/// The expected-actors half of AC01: every preset lowers to exactly the actors
/// its descriptor declares, with the measured planes, skills and counts.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f49_d_03_retail_every_preset_lowers_to_its_declared_actors() {
    let retail = retail();
    let mut checked = 0usize;
    for directory in retail.denominator.directories() {
        let scenario = directory.scenario_id().expect("a scenario id");
        let aircraft = directory.aircraft();
        for preset in retail.catalog.presets_for_scenario(&scenario) {
            let lowered = lower_preset_roster(&retail.catalog, preset.id())
                .expect("the measured roster lowers");
            assert_eq!(lowered.scenario, scenario);

            let on = |side| {
                lowered
                    .entries
                    .iter()
                    .filter(move |entry| entry.side == side)
                    .collect::<Vec<_>>()
            };

            // Player.
            let players = on(ScenarioSide::Player);
            assert_eq!(players.len(), 1, "{}: one player seat", preset.id());
            match aircraft.player_plane() {
                Some(plane) => {
                    let geometry = players[0]
                        .geometry
                        .as_ref()
                        .expect("a declared player plane lowers");
                    assert_eq!(
                        geometry.as_content(),
                        &table_airframe(plane),
                        "{}: the player flies the table's airframe for `{plane}`",
                        preset.id()
                    );
                }
                None => {
                    // `ZBD/c2b/IA1` declares no player_plane: unknown, never a
                    // substituted airframe.
                    assert_eq!(directory.container(), "zbd/c2b/ia1/zrdr.zbd");
                    let unmeasured = players[0]
                        .geometry
                        .as_ref()
                        .expect_err("an undeclared player plane stays unmeasured");
                    assert!(unmeasured.reason.contains("player_plane"));
                }
            }

            // Wingmen: one slot each, plane unmeasured; an undeclared count is
            // a recorded fact, not zero wingmen.
            let wingmen = on(ScenarioSide::Ally);
            match aircraft.wingmen() {
                Some(count) => {
                    assert_eq!(wingmen.len(), count as usize, "{}", preset.id());
                    assert!(lowered.unmeasured.is_empty());
                    for wingman in wingmen {
                        assert!(wingman.geometry.is_err(), "no wingman plane is measured");
                    }
                }
                None => {
                    assert!(wingmen.is_empty());
                    assert_eq!(lowered.unmeasured.len(), 1);
                    assert_eq!(lowered.unmeasured[0].field, "wingman count");
                }
            }

            // Enemy groups, in record order, then the ace.
            let enemies = on(ScenarioSide::Enemy);
            assert_eq!(enemies.len(), aircraft.enemy_groups().len() + 1);
            for (entry, group) in enemies.iter().zip(aircraft.enemy_groups()) {
                let plane = group.plane().expect("every retail group names a plane");
                assert_eq!(
                    entry
                        .geometry
                        .as_ref()
                        .expect("a group plane lowers")
                        .as_content(),
                    &table_airframe(plane)
                );
                assert_eq!(
                    entry.skill,
                    Some(
                        DeclaredSkillTier::from_label(group.skill().expect("a group skill"))
                            .expect("a measured skill label")
                    )
                );
                assert_eq!(entry.authored_count, group.count());
                assert_eq!(entry.label.as_deref(), group.name_label());
                assert_eq!(
                    entry.role.as_deref(),
                    Some(format!("group{}", group.index()).as_str())
                );
            }
            let ace = enemies.last().expect("the ace is the last enemy entry");
            assert_eq!(ace.role.as_deref(), Some("ace"));
            assert_eq!(
                ace.geometry
                    .as_ref()
                    .expect("the ace plane lowers")
                    .as_content(),
                &table_airframe(aircraft.ace().plane().expect("an ace plane"))
            );
            assert_eq!(ace.label.as_deref(), aircraft.ace().name_label());
            assert_eq!(ace.skill, Some(DeclaredSkillTier::Ace));
            checked += 1;
        }
    }
    assert_eq!(checked, 30, "all thirty presets were checked");
}

/// Four measured facts that would be easy to get wrong, pinned to the data.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f49_d_03_retail_the_scenario_without_a_player_plane_keeps_it_unknown() {
    let retail = retail();
    let c2b = retail
        .denominator
        .directories()
        .iter()
        .find(|directory| directory.container() == "zbd/c2b/ia1/zrdr.zbd")
        .expect("c2b is one of the eight directories");
    assert!(c2b.aircraft().player_plane().is_none());
    assert!(c2b.aircraft().wingmen().is_none());
    for preset in retail
        .catalog
        .presets_for_scenario(&c2b.scenario_id().expect("a scenario id"))
    {
        let player = &preset.parameters().roster().actors_on(ScenarioSide::Player)[0];
        assert!(
            matches!(player.airframe(), Resolved::Unknown { .. }),
            "{}: the player airframe is unknown, not defaulted",
            preset.id()
        );
        assert_eq!(preset.unmeasured_facts().len(), 1);
        // The launch is refused on the missing wingman count before anything
        // else, naming it.
        let refusal = lower_preset(&retail.catalog, preset.id())
            .expect_err("a preset with an unmeasured fact does not launch");
        let LowerError::UnknownValue { field, reason, .. } = refusal else {
            panic!("expected an UnknownValue refusal");
        };
        assert_eq!(field, "wingman count");
        assert!(reason.contains("num_wingmen"));
    }
}

// ------------------------------------------------- retail: AC01 world, rules ---

/// The world is the directory's world group and the environment is the IA
/// archive's own weather, each verified against the production reader.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f49_d_03_retail_world_and_environment_are_the_measured_bindings() {
    let retail = retail();
    let dir = game_dir();
    let discovery = cs_assets::install::discover(&dir).expect("production discovery reads");
    for directory in retail.denominator.directories() {
        let scenario = directory.scenario_id().expect("a scenario id");
        let weather = cs_app::environment::retail::read_mission_weather(
            &discovery,
            directory
                .container()
                .strip_suffix("/zrdr.zbd")
                .expect("a reader archive key"),
        )
        .expect("every IA archive carries a bound weather.zrd");
        let expected_world =
            WorldId::from_key(group_of(directory.container())).expect("a world key");
        for preset in retail.catalog.presets_for_scenario(&scenario) {
            let parameters = preset.parameters();
            let Resolved::Known(world) = parameters.world() else {
                panic!("{}: the world group has a world row", preset.id());
            };
            assert_eq!(world.value, expected_world);
            assert!(world.provenance.source.is_some());
            let Resolved::Known(environment) = parameters.environment() else {
                panic!("{}: the IA archive's weather binds", preset.id());
            };
            assert_eq!(&environment.value, weather.binding.definition.id());
            assert_eq!(environment.provenance.source.as_ref(), Some(&weather.span));
        }
    }
}

/// Every unmeasured dimension refuses at lowering with its own reason; each
/// refusal is reached by supplying the previous dimension from the fixture, so
/// the order and the wording of every refusal is checked.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f49_d_03_retail_every_unmeasured_dimension_refuses_at_lowering_with_its_reason() {
    let retail = retail();
    for preset in retail.catalog.presets() {
        let preset_id = preset.id();
        assert!(matches!(
            preset.parameters().difficulty(),
            Declared::Unmeasured { .. }
        ));
        assert!(matches!(
            preset.parameters().rules(),
            Declared::Unmeasured { .. }
        ));
        assert!(matches!(preset.title(), Resolved::Unknown { .. }));

        let wingmen_known = preset.unmeasured_facts().is_empty();
        let refusal = lower_preset(&retail.catalog, preset_id)
            .expect_err("no retail preset launches while its dimensions are unmeasured");
        let LowerError::UnknownValue {
            field,
            claim_id,
            reason,
        } = &refusal
        else {
            panic!("{preset_id}: expected UnknownValue, got {refusal:?}");
        };
        if wingmen_known {
            assert_eq!(field, "scenario difficulty", "{preset_id}");
            assert_eq!(reason, DIFFICULTY_UNMEASURED_REASON);
            assert!(claim_id.as_str().starts_with("f49.d.03.unmeasured."));
            // The refusal reads as a sentence naming field and reason.
            let sentence = refusal.to_string();
            assert!(sentence.contains("scenario difficulty is unknown"));
            assert!(sentence.contains("F32-D"));
        } else {
            assert_eq!(field, "wingman count");
        }

        // Supply difficulty from the fixture: the rules refuse next.
        let with_difficulty = rebuilt(
            retail,
            preset,
            Some(synthetic_difficulty(DifficultyTier::Standard)),
            None,
            false,
        );
        let LowerError::UnknownValue { field, reason, .. } =
            lower_preset(&with_difficulty, preset_id).expect_err("rules still unmeasured")
        else {
            panic!("expected UnknownValue");
        };
        if wingmen_known {
            assert_eq!(field, "scenario victory rules", "{preset_id}");
            assert!(reason.contains("VictoryCondition vocabulary"));
            assert!(
                reason.contains(preset.original_mission_type().expect("a mission type")),
                "the rules refusal names the mission type it could not map"
            );
        }

        // Supply rules too: the first actor's faction refuses.
        let rules = VictoryRules::try_new(
            VictoryCondition::EliminateEnemies,
            RespawnBudget::None,
            None,
            TieOutcome::Draw,
        )
        .expect("fixture rules are valid");
        let with_rules = rebuilt(
            retail,
            preset,
            Some(synthetic_difficulty(DifficultyTier::Standard)),
            Some(rules),
            false,
        );
        if wingmen_known {
            let LowerError::UnknownValue { field, reason, .. } =
                lower_preset(&with_rules, preset_id).expect_err("factions are unmeasured")
            else {
                panic!("expected UnknownValue");
            };
            assert_eq!(field, "player slot 0 faction", "{preset_id}");
            assert_eq!(reason, FACTION_UNMEASURED_REASON);

            // Supply every faction too: the loadout refuses next (the player
            // airframe is measured, so the airframe does not).
            let with_factions = rebuilt(
                retail,
                preset,
                Some(synthetic_difficulty(DifficultyTier::Standard)),
                Some(rules),
                true,
            );
            let LowerError::UnknownValue { field, reason, .. } =
                lower_preset(&with_factions, preset_id).expect_err("loadouts are unmeasured")
            else {
                panic!("expected UnknownValue");
            };
            let c2b = preset.id().as_str().starts_with("ia_preset/c2b-");
            if c2b {
                unreachable!("c2b is refused on its wingman count above");
            }
            assert_eq!(field, "player slot 0 loadout", "{preset_id}");
            assert_eq!(reason, LOADOUT_UNMEASURED_REASON);
        }
    }
}

/// The retail catalog is not the fixture: no preset, option or provenance is
/// shared with it.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f49_d_03_retail_nothing_in_the_catalog_comes_from_the_fixture() {
    let retail = retail();
    let fixture = synthetic_instant_action_catalog();
    let fixture_options = synthetic_scenario_options();
    for preset in retail.catalog.presets() {
        assert!(fixture.preset(preset.id()).is_none());
        assert!(!matches!(
            preset.origin(),
            Origin::SyntheticFixture | Origin::Designed
        ));
        assert!(!preset.id().as_str().contains("synthetic"));
        assert_ne!(
            preset.provenance().claim_id.as_str(),
            synthetic_provenance().claim_id.as_str()
        );
        for actor in preset.parameters().roster().actors() {
            assert!(!fixture_options.airframes().iter().any(|id| {
                matches!(actor.airframe(), Resolved::Known(known) if &known.value == id)
            }));
        }
    }
    let options = retail.catalog.options();
    for id in fixture_options.airframes() {
        assert!(!options.airframes().contains(id));
    }
    for id in fixture_options.factions() {
        assert!(!options.factions().contains(id));
    }
    for world in fixture_options.worlds() {
        assert!(!options.worlds().contains(world));
    }
    // The retail table is the measured one, with its unmeasured lists kept
    // unmeasured rather than filled.
    assert!(options.loadouts().is_empty());
    assert!(options.pilots().is_empty());
    assert!(!options.worlds().is_empty());
    assert_eq!(options.airframes().len(), 11);
    assert!(
        options
            .unmeasured_dimensions()
            .iter()
            .any(|dimension| dimension.label() == "loadouts")
    );
}

/// Every preset's seed is explicit, unique, and the documented function of its
/// id (F49 non-negotiable 4).
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f49_d_03_retail_every_preset_has_an_explicit_deterministic_seed() {
    let retail = retail();
    let mut seeds = BTreeSet::new();
    for preset in retail.catalog.presets() {
        let seed = preset.parameters().seed();
        assert_eq!(seed, preset_seed(preset.id()));
        assert!(
            seeds.insert(seed.root()),
            "{}: seeds are unique",
            preset.id()
        );
    }
    assert_eq!(seeds.len(), 30);
}

/// Customizing a retail preset does not inherit a guess: the unmeasured
/// difficulty and rules are left unset on the draft, and the player picks them.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f49_d_03_retail_customizing_a_preset_leaves_unmeasured_dimensions_unset() {
    let retail = retail();
    let preset = retail
        .catalog
        .presets()
        .into_iter()
        .find(|preset| preset.unmeasured_facts().is_empty())
        .expect("a retail preset with a declared wingman count");
    let mut flow = InstantActionFlow::new(
        cs_sim::campaign::ProfileId::new("pilot.f49d03").expect("a valid profile id"),
        retail.catalog.clone(),
    );
    flow.select_preset(preset.id())
        .expect("select accepts a preset");
    flow.apply(IaAction::StartCustom {
        subject: ContentId::from_source(ContentKind::IaScenario, "f49-d-03-custom")
            .expect("a custom subject"),
    })
    .expect("a custom draft opens from a retail preset");
    assert_eq!(flow.screen(), IaScreen::Customize);
    let draft = flow.draft().expect("a draft is open");
    assert!(draft.difficulty().is_none(), "no difficulty is inherited");
    assert!(draft.rules().is_none(), "no rules are inherited");
    let unset = draft.unset_dimensions();
    assert!(
        unset.contains(&"skill") && unset.contains(&"rules"),
        "{unset:?}"
    );
    // The player can pick a tier; the edit starts from no overrides.
    flow.edit(IaEdit::Difficulty(DifficultyTier::Standard))
        .expect("a first difficulty pick is accepted");
    assert!(flow.draft().expect("a draft").difficulty().is_some());
    // The incomplete draft does not launch.
    assert!(flow.apply(IaAction::Launch).is_err());
}

/// Rebuilds a catalog in which `preset` carries the given difficulty and rules
/// (and optionally a fixture faction on every actor), so the next refusal in
/// the lowering order can be observed.
fn rebuilt(
    retail: &Retail,
    preset: &InstantActionPreset,
    difficulty: Option<cs_content::ai::DifficultyProfile>,
    rules: Option<VictoryRules>,
    with_factions: bool,
) -> InstantActionCatalog {
    let parameters = preset.parameters();
    let roster = if with_factions {
        let actors: Vec<ScenarioActorSpec> = parameters
            .roster()
            .actors()
            .iter()
            .map(|actor| {
                ScenarioActorSpec::try_new_resolved(
                    actor.side(),
                    actor.slot(),
                    Resolved::Known(cs_types::content::Known::new(
                        cs_content::instant_action::synthetic_player_faction(),
                        synthetic_provenance(),
                    )),
                    actor.airframe().clone(),
                    actor.loadout().clone(),
                    None,
                    actor.survivability().clone(),
                    actor.provenance().clone(),
                )
                .expect("the rebuilt actor is valid")
            })
            .collect();
        ScenarioRoster::try_new(actors).expect("the rebuilt roster is valid")
    } else {
        parameters.roster().clone()
    };
    let difficulty = difficulty.expect("a rebuild supplies a difficulty");
    let rebuilt_parameters = ScenarioParameters::new(
        parameters.world().clone(),
        parameters.environment().clone(),
        roster,
        difficulty,
        rules.unwrap_or_else(|| {
            VictoryRules::try_new(
                VictoryCondition::EliminateEnemies,
                RespawnBudget::None,
                None,
                TieOutcome::Draw,
            )
            .expect("fixture rules are valid")
        }),
        parameters.seed(),
    );
    // Without supplied rules the preset keeps them unmeasured, with the same
    // mission-type wording the assembly records.
    let rebuilt_parameters = if rules.is_none() {
        rebuilt_parameters.with_unmeasured_rules(
            ClaimId::new("f49.d.03.unmeasured.rules").expect("a claim id"),
            &format!(
                "the original declares mission type `{}`; the designed VictoryCondition \
                 vocabulary does not cover it",
                preset.original_mission_type().expect("a mission type")
            ),
        )
    } else {
        rebuilt_parameters
    };
    let mut changed = InstantActionPreset::try_new(
        preset.id().clone(),
        preset.scenario().clone(),
        preset.title().clone(),
        rebuilt_parameters,
        preset.origin().clone(),
        preset.provenance().clone(),
    )
    .expect("the rebuilt preset is valid");
    if let Some(mission_type) = preset.original_mission_type() {
        changed = changed.with_original_mission_type(mission_type.to_owned());
    }
    for fact in preset.unmeasured_facts() {
        changed = changed.with_unmeasured_fact(fact.clone());
    }
    InstantActionCatalog::try_new(
        vec![changed],
        retail.catalog.options().clone(),
        retail.catalog.provenance().clone(),
    )
    .expect("the one-preset catalog is valid")
}

// ------------------------------------------------------ synthetic: failures ---

fn zrd_int(value: u32) -> Vec<u8> {
    let mut bytes = 1_u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&value.to_le_bytes());
    bytes
}

fn zrd_float(value: f32) -> Vec<u8> {
    let mut bytes = 2_u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&value.to_le_bytes());
    bytes
}

fn zrd_text(text: &str) -> Vec<u8> {
    let mut bytes = 3_u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
    bytes.extend_from_slice(text.as_bytes());
    bytes
}

/// A list node; the stored count is one more than the child count.
fn zrd_list(children: Vec<Vec<u8>>) -> Vec<u8> {
    let mut bytes = 4_u32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&((children.len() as u32) + 1).to_le_bytes());
    for child in children {
        bytes.extend_from_slice(&child);
    }
    bytes
}

fn wrapped_text(text: &str) -> Vec<u8> {
    zrd_list(vec![zrd_text(text)])
}

fn spawn_list(count: usize) -> Vec<u8> {
    zrd_list(
        (0..count)
            .map(|_| {
                zrd_list(vec![
                    zrd_float(1.0),
                    zrd_float(2.0),
                    zrd_float(3.0),
                    zrd_float(4.0),
                ])
            })
            .collect(),
    )
}

struct Descriptor<'a> {
    player_plane: Option<&'a str>,
    wingmen: Option<u32>,
    enemy_plane: &'a str,
    enemy_skill: &'a str,
    offered: &'a [&'a str],
}

impl Default for Descriptor<'_> {
    fn default() -> Self {
        Self {
            player_plane: Some("Fury"),
            wingmen: Some(2),
            enemy_plane: "Kestrel",
            enemy_skill: "veteran",
            offered: &["dogfight_ace", "dogfight_squadron"],
        }
    }
}

fn descriptor_bytes(descriptor: &Descriptor<'_>) -> Vec<u8> {
    let mut children = vec![
        zrd_text("mission_type"),
        wrapped_text(descriptor.offered[0]),
    ];
    if let Some(plane) = descriptor.player_plane {
        children.push(zrd_text("player_plane"));
        children.push(wrapped_text(plane));
    }
    if let Some(count) = descriptor.wingmen {
        children.push(zrd_text("num_wingmen"));
        children.push(zrd_list(vec![zrd_int(count)]));
    }
    let mut spawn = Vec::new();
    for name in descriptor.offered {
        spawn.push(zrd_text(name));
        spawn.push(spawn_list(2));
    }
    children.push(zrd_text("spawn_points"));
    children.push(zrd_list(spawn));
    children.push(zrd_text("group1"));
    children.push(zrd_list(vec![
        zrd_text("num_enemies"),
        zrd_list(vec![zrd_int(4)]),
        zrd_text("enemy_name"),
        wrapped_text("MSG_TEST_ENEMY"),
        zrd_text("enemy_plane"),
        wrapped_text(descriptor.enemy_plane),
        zrd_text("enemy_skill"),
        wrapped_text(descriptor.enemy_skill),
    ]));
    children.push(zrd_text("ace_name"));
    children.push(wrapped_text("MSG_TEST_ACE"));
    children.push(zrd_text("ace_plane"));
    children.push(wrapped_text("Peacemaker"));
    children.push(zrd_text("ace_skill"));
    children.push(wrapped_text("ace"));
    zrd_list(children)
}

fn directory(descriptor: &Descriptor<'_>) -> cs_content::instant_action::IaDirectory {
    let root = decode_zrd(&descriptor_bytes(descriptor)).expect("the synthetic descriptor decodes");
    read_ia_descriptor(
        &root,
        "zbd/c1/ia1/zrdr.zbd",
        &"0".repeat(64),
        16,
        512,
        "c1",
        "ia1",
    )
    .expect("the synthetic descriptor reads")
}

/// An option table offering the real airframe rows and a world, built through
/// the F49-A constructor (the synthetic fixture's own lists otherwise).
fn options_with_real_airframes() -> ScenarioOptions {
    let fixture = synthetic_scenario_options();
    let airframes: Vec<ContentId> = AIRFRAME_TABLE
        .iter()
        .map(|row| {
            ContentId::from_source(ContentKind::Airframe, row.scene_root).expect("a valid key")
        })
        .collect();
    ScenarioOptions::try_new(
        vec![WorldId::from_key("c1").expect("a world key")],
        fixture.environments().to_vec(),
        fixture.factions().to_vec(),
        airframes,
        fixture.loadouts().to_vec(),
        fixture.pilots().to_vec(),
        fixture.difficulty_tiers().to_vec(),
        fixture.victory_conditions().to_vec(),
        fixture.relations().to_vec(),
        fixture.max_players(),
    )
    .expect("the synthetic option table is valid")
}

fn inputs<'a>(options: &'a ScenarioOptions) -> DirectoryInputs<'a> {
    DirectoryInputs {
        install_sha256: cs_assets::install::sha256(b"synthetic installation"),
        container_path: "ZBD/C1/IA1/zrdr.zbd",
        options,
        environment: Err("the synthetic archive has no weather".to_owned()),
    }
}

fn span(inputs: &DirectoryInputs<'_>) -> SourceSpan {
    SourceSpan::new(
        inputs.install_sha256,
        inputs.container_path,
        Some("ia.zrd"),
        16,
        512,
        None,
    )
    .expect("a valid span")
}

#[test]
fn accept_f49_d_03_a_plane_outside_the_airframe_table_is_refused_by_name() {
    let options = options_with_real_airframes();
    let inputs = inputs(&options);
    let bad = directory(&Descriptor {
        enemy_plane: "Zeppelin9",
        ..Descriptor::default()
    });
    let error = directory_roster(&bad, &inputs, &span(&inputs))
        .expect_err("an unknown enemy plane is not defaulted");
    let RetailCatalogError::UnknownPlane { role, plane, .. } = &error else {
        panic!("expected UnknownPlane, got {error:?}");
    };
    assert_eq!((role.as_str(), plane.as_str()), ("group1", "Zeppelin9"));
    assert!(error.to_string().contains("`Zeppelin9`"));

    let bad_player = directory(&Descriptor {
        player_plane: Some("Nonesuch"),
        ..Descriptor::default()
    });
    assert!(matches!(
        directory_roster(&bad_player, &inputs, &span(&inputs)),
        Err(RetailCatalogError::UnknownPlane { role, .. }) if role == "player_plane"
    ));
}

#[test]
fn accept_f49_d_03_a_plane_the_option_table_does_not_offer_is_refused() {
    // The fixture's own airframe list holds none of the executable's rows.
    let options = synthetic_scenario_options();
    let inputs = inputs(&options);
    let error = directory_roster(&directory(&Descriptor::default()), &inputs, &span(&inputs))
        .expect_err("a plane outside the option table is refused");
    assert!(
        matches!(error, RetailCatalogError::PlaneNotOffered { .. }),
        "{error:?}"
    );
}

#[test]
fn accept_f49_d_03_a_skill_outside_the_measured_vocabulary_is_refused() {
    let options = options_with_real_airframes();
    let inputs = inputs(&options);
    let bad = directory(&Descriptor {
        enemy_skill: "legend",
        ..Descriptor::default()
    });
    assert!(matches!(
        directory_roster(&bad, &inputs, &span(&inputs)),
        Err(RetailCatalogError::UnknownSkill { skill, .. }) if skill == "legend"
    ));
}

#[test]
fn accept_f49_d_03_a_synthetic_descriptor_assembles_one_preset_per_offered_type() {
    let options = options_with_real_airframes();
    let inputs = inputs(&options);
    let presets = directory_presets(&directory(&Descriptor::default()), &inputs)
        .expect("a well-formed descriptor assembles");
    assert_eq!(presets.len(), 2);
    assert_eq!(presets[0].scenario(), presets[1].scenario());
    assert_ne!(presets[0].id(), presets[1].id());
    let types: Vec<_> = presets
        .iter()
        .map(|preset| preset.original_mission_type().expect("a mission type"))
        .collect();
    assert_eq!(types, ["dogfight_ace", "dogfight_squadron"]);

    // The environment the synthetic archive could not bind stays unknown, and
    // the world row the option table holds is bound.
    let parameters = presets[0].parameters();
    assert!(matches!(parameters.environment(), Resolved::Unknown { .. }));
    assert!(matches!(parameters.world(), Resolved::Known(_)));

    // Two wingmen, a player, one group and the ace.
    let roster = parameters.roster();
    assert_eq!(roster.actors_on(ScenarioSide::Player).len(), 1);
    assert_eq!(roster.actors_on(ScenarioSide::Ally).len(), 2);
    assert_eq!(roster.actors_on(ScenarioSide::Enemy).len(), 2);

    // The two presets share one scenario and are accepted by the catalog.
    InstantActionCatalog::try_new(presets, options, synthetic_provenance())
        .expect("presets of one scenario that differ by mission type coexist");
}

#[test]
fn accept_f49_d_03_a_preset_with_no_player_side_actor_is_refused() {
    let options = options_with_real_airframes();
    let inputs = inputs(&options);
    let preset = directory_presets(&directory(&Descriptor::default()), &inputs)
        .expect("assembles")
        .remove(0);
    // Keep only the enemy side.
    let enemies: Vec<ScenarioActorSpec> = preset
        .parameters()
        .roster()
        .actors_on(ScenarioSide::Enemy)
        .into_iter()
        .cloned()
        .collect();
    let parameters = ScenarioParameters::new_unmeasured_rules(
        preset.parameters().world().clone(),
        preset.parameters().environment().clone(),
        ScenarioRoster::try_new(enemies).expect("an enemy-only roster is a roster"),
        preset.parameters().seed(),
        (
            ClaimId::new("f49.d.03.test").expect("a claim"),
            "unmeasured",
        ),
        (
            ClaimId::new("f49.d.03.test").expect("a claim"),
            "unmeasured",
        ),
    );
    let playerless = InstantActionPreset::try_new(
        preset.id().clone(),
        preset.scenario().clone(),
        preset.title().clone(),
        parameters,
        preset.origin().clone(),
        preset.provenance().clone(),
    )
    .expect("the preset record itself is valid");
    let error = InstantActionCatalog::try_new(vec![playerless], options, synthetic_provenance())
        .expect_err("a preset that seats nobody is refused");
    assert!(
        matches!(error, ScenarioSchemaError::PresetWithoutPlayer { .. }),
        "{error:?}"
    );
}

#[test]
fn accept_f49_d_03_duplicate_identities_are_refused_but_one_scenario_may_have_several_types() {
    let options = options_with_real_airframes();
    let inputs = inputs(&options);
    let presets =
        directory_presets(&directory(&Descriptor::default()), &inputs).expect("assembles");

    // The same preset id twice.
    let duplicate_id = vec![presets[0].clone(), presets[0].clone()];
    assert!(matches!(
        InstantActionCatalog::try_new(duplicate_id, options.clone(), synthetic_provenance()),
        Err(ScenarioSchemaError::DuplicatePreset { .. })
    ));

    // The same scenario and the same mission type under a second id.
    let other_id =
        ContentId::from_source(ContentKind::IaPreset, "c1-ia1-clone").expect("a valid preset key");
    let clone = InstantActionPreset::try_new(
        other_id,
        presets[0].scenario().clone(),
        presets[0].title().clone(),
        presets[0].parameters().clone(),
        presets[0].origin().clone(),
        presets[0].provenance().clone(),
    )
    .expect("a valid preset record")
    .with_original_mission_type(
        presets[0]
            .original_mission_type()
            .expect("a mission type")
            .to_owned(),
    );
    assert!(matches!(
        InstantActionCatalog::try_new(
            vec![presets[0].clone(), clone],
            options.clone(),
            synthetic_provenance(),
        ),
        Err(ScenarioSchemaError::DuplicateScenario(_))
    ));
}

#[test]
fn accept_f49_d_03_an_unmeasured_dimension_cannot_be_defaulted_into_a_launch() {
    let options = options_with_real_airframes();
    let inputs = inputs(&options);
    let presets =
        directory_presets(&directory(&Descriptor::default()), &inputs).expect("assembles");
    let catalog =
        InstantActionCatalog::try_new(presets, options, synthetic_provenance()).expect("valid");
    let first = catalog.presets()[0].id().clone();

    // An unmeasured value cannot be built without a reason.
    assert!(
        Declared::<u8>::unmeasured(ClaimId::new("f49.d.03.test").expect("a claim"), " ").is_err()
    );

    // The preset keeps its difficulty and rules unmeasured, and the lowering
    // refuses the first one instead of launching a defaulted scenario.
    let preset = catalog.preset(&first).expect("held");
    assert!(!preset.parameters().difficulty().is_declared());
    assert!(!preset.parameters().rules().is_declared());
    let refusal = lower_preset(&catalog, &first).expect_err("an unmeasured preset does not launch");
    // The synthetic archive has no weather, which is the first thing the
    // lowering reads; the unmeasured difficulty and rules follow it.
    assert!(
        matches!(&refusal, LowerError::UnknownValue { field, .. } if field == "scenario environment"),
        "{refusal:?}"
    );

    // A fully declared fixture preset still lowers (the refusal is the
    // unmeasured dimension, not the lowering path), and what it lowers is its
    // own roster.
    let fixture = synthetic_instant_action_catalog();
    let fixture_id = fixture.preset_ids()[0].clone();
    let lowered = lower_preset(&fixture, &fixture_id).expect("a declared preset lowers");
    assert!(!lowered.actors().is_empty());
    assert!(
        lowered
            .actors()
            .iter()
            .all(|actor| actor.survivability == SurvivabilityPolicy::Mortal)
    );
}

#[test]
fn accept_f49_d_03_an_unknown_loadout_in_a_roster_actor_refuses_the_launch() {
    // A fully declared preset whose first actor's loadout is unmeasured: the
    // lowering names the actor's loadout rather than substituting one.
    let fixture = synthetic_instant_action_catalog();
    let preset = fixture.presets()[0].clone();
    let mut actors: Vec<ScenarioActorSpec> = preset.parameters().roster().actors().to_vec();
    let target = actors
        .iter()
        .position(|actor| actor.side() == ScenarioSide::Player)
        .expect("the fixture has a player actor");
    let original = actors[target].clone();
    actors[target] = ScenarioActorSpec::try_new_resolved(
        original.side(),
        original.slot(),
        original.faction().clone(),
        original.airframe().clone(),
        Resolved::unknown(
            ClaimId::new("f49.d.03.test.loadout").expect("a claim"),
            "no loadout was measured",
        )
        .expect("a reason"),
        None,
        original.survivability().clone(),
        original.provenance().clone(),
    )
    .expect("valid actor");
    let parameters = ScenarioParameters::new(
        preset.parameters().world().clone(),
        preset.parameters().environment().clone(),
        ScenarioRoster::try_new(actors).expect("valid roster"),
        synthetic_difficulty(DifficultyTier::Standard),
        *preset.parameters().rules().declared().expect("declared"),
        preset.parameters().seed(),
    );
    let changed = InstantActionPreset::try_new(
        preset.id().clone(),
        preset.scenario().clone(),
        preset.title().clone(),
        parameters,
        preset.origin().clone(),
        preset.provenance().clone(),
    )
    .expect("valid preset");
    let catalog = InstantActionCatalog::try_new(
        vec![changed],
        fixture.options().clone(),
        fixture.provenance().clone(),
    )
    .expect("valid catalog");
    let LowerError::UnknownValue { field, reason, .. } =
        lower_preset(&catalog, preset.id()).expect_err("an unknown loadout refuses")
    else {
        panic!("expected UnknownValue");
    };
    assert!(field.ends_with("loadout"), "{field}");
    assert_eq!(reason, "no loadout was measured");
}

#[test]
fn accept_f49_d_03_a_partial_option_table_keeps_only_unmeasured_lists_empty() {
    use cs_content::instant_action::{
        OptionListEvidence, ScenarioOptionDimension, ScenarioOptionTable,
    };
    let fixture = synthetic_scenario_options();
    let evidence = |loadouts: OptionListEvidence| {
        ScenarioOptionDimension::ALL
            .iter()
            .map(|dimension| {
                (
                    *dimension,
                    if *dimension == ScenarioOptionDimension::Loadouts {
                        loadouts.clone()
                    } else {
                        OptionListEvidence::Measured { reader: "fixture" }
                    },
                )
            })
            .collect::<Vec<_>>()
    };
    let table = |loadouts: Vec<ContentId>, evidence| {
        ScenarioOptionTable::from_lists(
            fixture.worlds().to_vec(),
            fixture.environments().to_vec(),
            fixture.factions().to_vec(),
            fixture.airframes().to_vec(),
            loadouts,
            fixture.pilots().to_vec(),
            fixture.difficulty_tiers().to_vec(),
            fixture.victory_conditions().to_vec(),
            fixture.relations().to_vec(),
            fixture.max_players(),
            evidence,
        )
        .expect("every dimension has evidence")
    };
    // Empty and recorded unmeasured: accepted, and reported as unmeasured.
    let partial = table(
        Vec::new(),
        evidence(OptionListEvidence::Unmeasured {
            reason: "no producer",
            resolving_task: "a follow-up task",
        }),
    )
    .validate_partial()
    .expect("an unmeasured empty list is tolerated");
    assert_eq!(
        partial.unmeasured_dimensions(),
        [ScenarioOptionDimension::Loadouts]
    );
    // Empty but claimed measured: still refused, naming the list.
    assert!(matches!(
        table(
            Vec::new(),
            evidence(OptionListEvidence::Measured { reader: "x" })
        )
        .validate_partial(),
        Err(ScenarioSchemaError::EmptyOptionTable { what: "loadouts" })
    ));
    // The strict validation still refuses the unmeasured empty list.
    assert!(matches!(
        table(
            Vec::new(),
            evidence(OptionListEvidence::Unmeasured {
                reason: "no producer",
                resolving_task: "a follow-up task",
            }),
        )
        .validate(),
        Err(ScenarioSchemaError::EmptyOptionTable { what: "loadouts" })
    ));
}
