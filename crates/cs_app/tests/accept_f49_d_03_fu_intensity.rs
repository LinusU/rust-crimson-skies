//! F49-D.03-FU-INTENSITY (#2055): the original's per-actor `GameIntensity`.
//!
//! The measurement (`docs/findings/2026-10-25-f49-d-03-fu-intensity.md`) is
//! that an Instant Action actor's difficulty is the value the spawn routine
//! holds `GameIntensity` at while it creates the actor: the ace `ace_skill`
//! as 0/1/2, every enemy group 1, and nothing written around the wingmen, who
//! therefore run under the player's option value. The non-ignored tests drive
//! the production schema, lowering and diff over a fixture preset; the retail
//! tests read the production catalog and the owner-supplied decrypted image.

use std::fs;
use std::path::PathBuf;

use cs_app::ui::instant_action::{
    ActorField, IA_DIFFICULTY_PER_ACTOR_CLAIM, LowerError, LoweredDifficulty, ScenarioChange,
    diff_scenarios, lower_preset, retail_instant_action_catalog,
};
use cs_content::ai::DeclaredSkillTier;
use cs_content::instant_action::{
    ActorIntensity, Declared, GameIntensity, InstantActionCatalog, InstantActionPreset,
    ScenarioActorSpec, ScenarioParameters, ScenarioRoster, ScenarioSide,
    synthetic_instant_action_catalog,
};
use cs_types::evidence::ClaimId;

const CLAIM: &str = "f49.d.03.test-per-actor";

fn intensity(value: u8) -> ActorIntensity {
    ActorIntensity::Set(GameIntensity::new(value).expect("0..=2 is measured"))
}

/// The fixture's first preset with its difficulty carried per actor; actor
/// `n` gets `intensities[n]` (none leaves it unset).
fn per_actor_catalog(intensities: &[Option<ActorIntensity>]) -> (InstantActionCatalog, usize) {
    let fixture = synthetic_instant_action_catalog();
    let preset = &fixture.presets()[0];
    let parameters = preset.parameters();
    let actors: Vec<ScenarioActorSpec> = parameters
        .roster()
        .actors()
        .iter()
        .enumerate()
        .map(
            |(index, actor)| match intensities.get(index).copied().flatten() {
                Some(value) => actor.clone().with_intensity(value),
                None => actor.clone(),
            },
        )
        .collect();
    let count = actors.len();
    let changed = ScenarioParameters::new(
        parameters.world().clone(),
        parameters.environment().clone(),
        ScenarioRoster::try_new(actors).expect("the roster is valid"),
        parameters
            .difficulty()
            .declared()
            .expect("the fixture declares a difficulty")
            .clone(),
        *parameters.rules().declared().expect("declared rules"),
        parameters.seed(),
    )
    .with_per_actor_difficulty(ClaimId::new(CLAIM).expect("claim"), "carried per actor");
    let preset = InstantActionPreset::try_new(
        preset.id().clone(),
        preset.scenario().clone(),
        preset.title().clone(),
        changed,
        preset.origin().clone(),
        preset.provenance().clone(),
    )
    .expect("the preset is valid");
    let catalog = InstantActionCatalog::try_new(
        vec![preset],
        fixture.options().clone(),
        fixture.provenance().clone(),
    )
    .expect("the catalog is valid");
    (catalog, count)
}

#[test]
fn accept_f49_d_03_fu_intensity_values_are_the_measured_zero_to_two() {
    for value in 0..=2 {
        assert_eq!(
            GameIntensity::new(value).map(GameIntensity::value),
            Some(value)
        );
    }
    assert_eq!(GameIntensity::new(3), None);
}

#[test]
fn accept_f49_d_03_fu_intensity_a_per_actor_difficulty_lowers_with_each_actors_value() {
    let (probe, count) = per_actor_catalog(&[]);
    let _ = probe;
    let values: Vec<Option<ActorIntensity>> = (0..count)
        .map(|index| {
            Some(if index == 0 {
                ActorIntensity::PlayerOption
            } else {
                intensity(u8::try_from(index % 3).expect("small"))
            })
        })
        .collect();
    let (catalog, _) = per_actor_catalog(&values);
    let id = catalog.presets()[0].id().clone();
    let lowered = lower_preset(&catalog, &id).expect("every actor carries its intensity");
    assert_eq!(lowered.difficulty(), LoweredDifficulty::PerActor);
    assert_eq!(lowered.difficulty().tier(), None);
    assert_eq!(lowered.actors().len(), count);
    assert_eq!(
        lowered.actors()[0].intensity,
        Some(ActorIntensity::PlayerOption)
    );
    for (index, actor) in lowered.actors().iter().enumerate().skip(1) {
        assert_eq!(actor.intensity, values[index], "actor {index}");
    }

    // Changing one actor's value is a reported, named difference.
    let last = count - 1;
    let mut other = values.clone();
    other[last] = Some(if values[last] == Some(intensity(2)) {
        intensity(0)
    } else {
        intensity(2)
    });
    let (changed_catalog, _) = per_actor_catalog(&other);
    let changed = lower_preset(&changed_catalog, &id).expect("lowers");
    let diff = diff_scenarios(&lowered, &changed);
    assert!(
        diff.iter().any(|change| matches!(
            change,
            ScenarioChange::ActorChanged { fields, .. } if fields == &[ActorField::Intensity]
        )),
        "{diff:?}"
    );
}

#[test]
fn accept_f49_d_03_fu_intensity_an_actor_without_a_value_refuses_by_name() {
    let (probe, count) = per_actor_catalog(&[]);
    let id = probe.presets()[0].id().clone();
    let mut values: Vec<Option<ActorIntensity>> = vec![Some(intensity(1)); count];
    values[count - 1] = None;
    let (catalog, _) = per_actor_catalog(&values);
    let refusal = lower_preset(&catalog, &id).expect_err("an unset intensity does not lower");
    let LowerError::UnknownValue {
        field, claim_id, ..
    } = &refusal
    else {
        panic!("expected UnknownValue, got {refusal:?}");
    };
    assert!(field.ends_with("intensity"), "{field}");
    assert_eq!(claim_id.as_str(), CLAIM);
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f49_d_03_fu_intensity_retail_every_actor_carries_the_spawn_routines_value() {
    let game_dir = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"));
    let catalog = retail_instant_action_catalog(&game_dir).expect("the retail catalog assembles");
    assert_eq!(catalog.presets().len(), 30);
    let mut aces = 0;
    for preset in catalog.presets() {
        let Declared::PerActor { claim_id, .. } = preset.parameters().difficulty() else {
            panic!("{}: difficulty is carried per actor", preset.id());
        };
        assert_eq!(claim_id.as_str(), IA_DIFFICULTY_PER_ACTOR_CLAIM);
        for actor in preset.parameters().roster().actors() {
            let role = actor.role().expect("retail actors carry a role");
            let expected = match (actor.side(), role) {
                (ScenarioSide::Ally, _) => ActorIntensity::PlayerOption,
                (_, "ace") => {
                    aces += 1;
                    intensity(match actor.skill() {
                        Some(DeclaredSkillTier::Novice) => 0,
                        Some(DeclaredSkillTier::Veteran) => 1,
                        Some(DeclaredSkillTier::Ace) => 2,
                        None => panic!("{}: an ace without ace_skill", preset.id()),
                    })
                }
                _ => intensity(1),
            };
            if actor.side() == ScenarioSide::Player {
                assert_eq!(actor.intensity(), None, "{}: the player", preset.id());
            } else {
                assert_eq!(actor.intensity(), Some(expected), "{} {role}", preset.id());
            }
        }
    }
    // The type that runs is the descriptor's default and none is
    // `dogfight_ace`, so no retail preset seats the named ace
    // (F49-D.03-FU-RULES-TYPE-ORDER); its intensity is measured on the
    // synthetic descriptor.
    assert_eq!(aces, 0, "no retail preset seats the named ace");
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_f49_d_03_fu_intensity_retail_the_wingman_spawn_runs_under_the_players_option() {
    let path = std::env::var("CS_ENGINE_IMAGE").expect("CS_ENGINE_IMAGE must be set");
    let image = fs::read(path).expect("the engine image reads");
    let (text_va, text_len) = (0x0040_1000_u32, 0x0020_14bd_usize);
    let text = section_bytes(&image, text_va, text_len);
    let callers = |target: u32| -> Vec<u32> {
        (0..text_len - 5)
            .filter(|&at| {
                text[at] == 0xe8 && {
                    let rel = i32::from_le_bytes(text[at + 1..at + 5].try_into().expect("4 bytes"));
                    (text_va + at as u32)
                        .wrapping_add(5)
                        .wrapping_add_signed(rel)
                        == target
                }
            })
            .map(|at| text_va + at as u32)
            .collect()
    };
    let spawns = callers(0x0047_c210);
    // The wingman loop's spawn call is in the scenario spawn routine, before
    // the ace block, and no GameIntensity getter/setter is called anywhere
    // from the routine's entry to it.
    let wingman_spawn = 0x0045_a9a1_u32;
    assert!(spawns.contains(&wingman_spawn));
    let touches: Vec<u32> = callers(0x0044_0700)
        .into_iter()
        .chain(callers(0x0044_0710))
        .collect();
    assert!(
        touches
            .iter()
            .all(|&site| !(0x0045_a390..=wingman_spawn).contains(&site)),
        "{touches:x?}"
    );
    // The ace and the groups are bracketed (read, set, spawn, restore).
    for (read, set, spawn, restore) in [
        (
            0x0045_acbe_u32,
            0x0045_ace3_u32,
            0x0045_acf6_u32,
            0x0045_acfc_u32,
        ),
        (0x0045_b100, 0x0045_b120, 0x0045_b133, 0x0045_b139),
    ] {
        assert!(callers(0x0044_0710).contains(&read));
        assert!(callers(0x0044_0700).contains(&set));
        assert!(spawns.contains(&spawn));
        assert!(callers(0x0044_0700).contains(&restore));
    }
    // The wingman loop is driven by the count and plane the descriptor sets
    // (`mov eax, [0x718ce0]` at the loop test).
    assert_eq!(
        section_bytes(&image, 0x0045_a494, 5),
        [0xa1, 0xe0, 0x8c, 0x71, 0x00]
    );
}

fn section_bytes(image: &[u8], va: u32, len: usize) -> &[u8] {
    let u16_at = |at: usize| u16::from_le_bytes(image[at..at + 2].try_into().expect("2 bytes"));
    let u32_at = |at: usize| u32::from_le_bytes(image[at..at + 4].try_into().expect("4 bytes"));
    let pe = u32_at(0x3c) as usize;
    let sections = usize::from(u16_at(pe + 6));
    let optional = usize::from(u16_at(pe + 20));
    let image_base = u32_at(pe + 24 + 28);
    let table = pe + 24 + optional;
    for index in 0..sections {
        let entry = table + index * 40;
        let virtual_size = u32_at(entry + 8);
        let section_va = image_base + u32_at(entry + 12);
        let raw_size = u32_at(entry + 16);
        let raw = u32_at(entry + 20) as usize;
        if va >= section_va && va - section_va + len as u32 <= virtual_size.min(raw_size) {
            let start = raw + (va - section_va) as usize;
            return &image[start..start + len];
        }
    }
    panic!("virtual address {va:#x} is in no section");
}
