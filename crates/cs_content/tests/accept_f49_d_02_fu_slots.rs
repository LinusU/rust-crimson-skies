//! Acceptance scenario F49-D.02-FU-SLOTS (#1629): the original's per-slot
//! Instant Action loadout selection is modeled, and custom scenarios
//! validate slot choices instead of a `ContentKind::Loadout` id.
//!
//! Spec: `specs/F49-instant-action-presets-and-custom-scenarios.md` (the
//! custom-scenario validation non-negotiables). Task test prefix:
//! `accept_f49_d_02_`. Required capability: **retail** plus the decrypted
//! engine image (`$CS_ENGINE_IMAGE`) for the re-measurement test.
//!
//! The synthetic half drives the production path
//! (`InstantActionCatalog::validate_custom` over a request whose actors carry
//! [`cs_content::instant_action::SlotOrdnance`]): a pick on a slot the
//! airframe lacks, a pick outside the measured domain and a named loadout
//! beside a per-slot selection must each fail by name; the measured default
//! contents must pass.
//!
//! The retail half re-reads `ORDINANCELAYOUT.SCRIPT` and the engine image's
//! template/selector tables, so a committed constant that drifts from the
//! measured surface fails rather than passing.

use cs_content::ai::DifficultyTier;
use cs_content::instant_action::{
    CustomScenarioDraft, InstantActionCatalog, RespawnBudget, RosterSlot, ScenarioActorSpec,
    ScenarioOptions, ScenarioProblemCode, ScenarioSeed, ScenarioSide, SlotOrdnance,
    SlotOrdnanceError, TieOutcome, VictoryCondition, VictoryRules, synthetic_dogfight_preset,
    synthetic_provenance, synthetic_scenario_options,
};
use cs_content::pilots::DeclaredSurvivability;
use cs_content::weapons::{
    ORIGINAL_AIRFRAME_LOADOUTS, ORIGINAL_AMMO_NONE, ORIGINAL_GUN_NONE, ORIGINAL_ROCKET_NONE,
    ORIGINAL_ROCKET_SELECTOR_ROWS, ORIGINAL_SLOT_AMMO_KINDS, original_airframe_loadout,
};
use cs_types::content::{ContentId, ContentKind, Known, Resolved};
use cs_types::evidence::ClaimId;

fn claim() -> ClaimId {
    ClaimId::new("f49d02fu.slots").expect("the test claim id is valid")
}

fn known_airframe(key: &str) -> Resolved<ContentId> {
    Resolved::Known(Known::new(
        ContentId::from_source(ContentKind::Airframe, key).expect("airframe id is valid"),
        synthetic_provenance(),
    ))
}

fn unknown_loadout() -> Resolved<ContentId> {
    Resolved::unknown(
        claim(),
        "the original offers no loadout identity list; this actor's ordnance is per-slot",
    )
    .expect("the reason is not empty")
}

/// A player-side actor whose ordnance is a per-slot selection on the given
/// airframe.
fn slot_actor(airframe_key: &str, ordnance: SlotOrdnance) -> ScenarioActorSpec {
    ScenarioActorSpec::try_new_resolved(
        ScenarioSide::Player,
        RosterSlot(0),
        Resolved::Known(Known::new(
            cs_content::instant_action::synthetic_player_faction(),
            synthetic_provenance(),
        )),
        known_airframe(airframe_key),
        unknown_loadout(),
        None,
        Resolved::Known(Known::new(
            DeclaredSurvivability::Mortal,
            synthetic_provenance(),
        )),
        synthetic_provenance(),
    )
    .expect("the slot-ordnance actor is structurally valid")
    .with_slot_ordnance(ordnance)
}

/// The fixture catalog with the Devastator airframe id added to the option
/// table, so a scenario can name it.
fn catalog_with_pfighter() -> InstantActionCatalog {
    let base = synthetic_scenario_options();
    let mut airframes = base.airframes().to_vec();
    airframes.push(
        ContentId::from_source(ContentKind::Airframe, "player_pfighter")
            .expect("the airframe id is valid"),
    );
    let options = ScenarioOptions::try_new(
        base.worlds().to_vec(),
        base.environments().to_vec(),
        base.factions().to_vec(),
        airframes,
        base.loadouts().to_vec(),
        base.pilots().to_vec(),
        base.difficulty_tiers().to_vec(),
        base.victory_conditions().to_vec(),
        base.relations().to_vec(),
        base.max_players(),
    )
    .expect("the option table with the measured airframe id is valid");
    InstantActionCatalog::try_new(
        vec![synthetic_dogfight_preset()],
        options,
        synthetic_provenance(),
    )
    .expect("the catalog is valid")
}

/// A complete request over the given player actor (plus one fixture enemy, so
/// the roster has both sides).
fn request_over(player: ScenarioActorSpec) -> cs_content::instant_action::CustomScenarioRequest {
    use cs_content::instant_action::{
        SYNTHETIC_IA_AIRFRAME_INTERCEPTOR, SYNTHETIC_IA_ENV_DAY_CLEAR, SYNTHETIC_IA_LOADOUT_LIGHT,
        SYNTHETIC_IA_WORLD_COAST, synthetic_actor, synthetic_difficulty,
    };

    let rules = VictoryRules::try_new(
        VictoryCondition::LastSideStanding,
        RespawnBudget::None,
        None,
        TieOutcome::Draw,
    )
    .expect("the fixture rules are valid");
    CustomScenarioDraft::new()
        .with_subject(
            ContentId::from_source(ContentKind::IaScenario, "synthetic.fixture_ia_slot_probe")
                .expect("the subject id is valid"),
        )
        .with_world(Resolved::Known(Known::new(
            cs_content::world::WorldId::from_key(SYNTHETIC_IA_WORLD_COAST).expect("world key"),
            synthetic_provenance(),
        )))
        .with_environment(Resolved::Known(Known::new(
            cs_content::environment::EnvironmentId::new(SYNTHETIC_IA_ENV_DAY_CLEAR)
                .expect("environment key"),
            synthetic_provenance(),
        )))
        .with_roster(vec![
            player,
            synthetic_actor(
                ScenarioSide::Enemy,
                0,
                SYNTHETIC_IA_AIRFRAME_INTERCEPTOR,
                SYNTHETIC_IA_LOADOUT_LIGHT,
            ),
        ])
        .with_difficulty(synthetic_difficulty(DifficultyTier::Standard))
        .with_rules(rules)
        .with_seed(ScenarioSeed::new(7))
        .with_players(1)
        .with_provenance(synthetic_provenance())
        .resolve()
        .expect("the probe request is structurally valid")
}

/// A per-slot selection's own domain: the five ammunition kinds and the
/// twelve rocket selector rows the original's dropdowns offer, and nothing
/// outside them — refused by name at the boundary.
#[test]
fn accept_f49_d_02_slot_domains_are_the_measured_ones() {
    let dev = original_airframe_loadout("player_pfighter").expect("a measured template");
    let ordnance = SlotOrdnance::try_new(*dev.ammo(), *dev.rockets())
        .expect("the measured default contents are inside the domains");
    assert!(ordnance.fits(dev));

    assert_eq!(
        SlotOrdnance::try_new(
            [ORIGINAL_AMMO_NONE, ORIGINAL_SLOT_AMMO_KINDS, 0, 0],
            *dev.rockets()
        ),
        Err(SlotOrdnanceError::AmmoKindOutOfRange { slot: 1, kind: 5 }),
        "kind 5 is one past the measured 0..=4 dropdown"
    );
    let mut rockets = *dev.rockets();
    rockets[1] = ORIGINAL_ROCKET_NONE + 1;
    assert_eq!(
        SlotOrdnance::try_new(*dev.ammo(), rockets),
        Err(SlotOrdnanceError::RocketOutOfRange { slot: 1, id: 12 }),
        "rocket id 12 is one past the measured selector table"
    );
}

/// A valid per-slot selection *is* the actor's ordnance: with one attached,
/// an unknown named loadout is no longer an unmeasured field, and validation
/// comes back clean.
#[test]
fn accept_f49_d_02_slot_selection_replaces_the_named_loadout() {
    let dev = original_airframe_loadout("player_pfighter").expect("a measured template");
    let ordnance = SlotOrdnance::of_airframe(dev);
    let request = request_over(slot_actor("player_pfighter", ordnance));

    let problems = catalog_with_pfighter().validate_custom(&request);
    assert!(
        problems.is_empty(),
        "the measured default per-slot loadout must validate clean, got {:?}",
        problems.problems()
    );
}

/// A pick on a slot the actor's airframe does not have is refused by name:
/// the Devastator has no gun slot 3 and no rocket slots 2, 3, 6 or 7.
#[test]
fn accept_f49_d_02_slots_the_airframe_lacks_are_refused() {
    let dev = original_airframe_loadout("player_pfighter").expect("a measured template");
    assert!(!dev.gun_slot_present(3) && !dev.rocket_slot_present(2));

    let mut ammo = *dev.ammo();
    ammo[3] = 0;
    let ordnance = SlotOrdnance::try_new(ammo, *dev.rockets()).expect("in-domain pick");
    let request = request_over(slot_actor("player_pfighter", ordnance));
    let problems = catalog_with_pfighter().validate_custom(&request);
    let hits = problems.with_code(ScenarioProblemCode::ImpossibleSlotChoice);
    assert_eq!(
        hits.len(),
        1,
        "one impossible gun-slot pick expected, got {:?}",
        problems.problems()
    );
    assert!(
        hits[0].detail().contains("gun slot 3"),
        "{}",
        hits[0].detail()
    );

    let mut rockets = *dev.rockets();
    rockets[6] = 0;
    let ordnance = SlotOrdnance::try_new(*dev.ammo(), rockets).expect("in-domain pick");
    let request = request_over(slot_actor("player_pfighter", ordnance));
    let problems = catalog_with_pfighter().validate_custom(&request);
    let hits = problems.with_code(ScenarioProblemCode::ImpossibleSlotChoice);
    assert_eq!(
        hits.len(),
        1,
        "one impossible rocket-slot pick expected, got {:?}",
        problems.problems()
    );
    assert!(
        hits[0].detail().contains("rocket slot 6"),
        "{}",
        hits[0].detail()
    );
}

/// A named loadout beside a per-slot selection is a second, contradictory
/// ordnance declaration and is refused by name.
#[test]
fn accept_f49_d_02_slot_selection_conflicts_with_a_named_loadout() {
    use cs_content::instant_action::SYNTHETIC_IA_LOADOUT_LIGHT;

    let dev = original_airframe_loadout("player_pfighter").expect("a measured template");
    let actor = ScenarioActorSpec::try_new(
        ScenarioSide::Player,
        RosterSlot(0),
        cs_content::instant_action::synthetic_player_faction(),
        known_airframe("player_pfighter"),
        Resolved::Known(Known::new(
            ContentId::from_source(ContentKind::Loadout, SYNTHETIC_IA_LOADOUT_LIGHT)
                .expect("loadout id is valid"),
            synthetic_provenance(),
        )),
        None,
        Resolved::Known(Known::new(
            DeclaredSurvivability::Mortal,
            synthetic_provenance(),
        )),
        synthetic_provenance(),
    )
    .expect("the actor is structurally valid")
    .with_slot_ordnance(SlotOrdnance::of_airframe(dev));

    let problems = catalog_with_pfighter().validate_custom(&request_over(actor));
    let hits = problems.with_code(ScenarioProblemCode::ConflictingOrdnance);
    assert_eq!(
        hits.len(),
        1,
        "one conflicting ordnance expected, got {:?}",
        problems.problems()
    );
}

/// A per-slot selection on an airframe with no measured slot template cannot
/// be checked, so it is refused rather than assumed legal.
#[test]
fn accept_f49_d_02_slots_on_an_unmeasured_airframe_are_refused() {
    use cs_content::instant_action::SYNTHETIC_IA_AIRFRAME_INTERCEPTOR;

    let dev = original_airframe_loadout("player_pfighter").expect("a measured template");
    let actor = slot_actor(
        SYNTHETIC_IA_AIRFRAME_INTERCEPTOR,
        SlotOrdnance::of_airframe(dev),
    );
    let problems = catalog_with_pfighter().validate_custom(&request_over(actor));
    let hits = problems.with_code(ScenarioProblemCode::ImpossibleSlotChoice);
    assert_eq!(
        hits.len(),
        1,
        "the unmeasured airframe's slot surface must be refused, got {:?}",
        problems.problems()
    );
    assert!(
        hits[0].detail().contains("no measured slot template"),
        "{}",
        hits[0].detail()
    );
}

/// The committed constants are re-measured against the retail editor script
/// and the decrypted engine image: the slot counts, the dropdown types, the
/// callback ids, the twelve selector rows and the eleven template records.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_f49_d_02_slot_surface_is_remeasured_against_retail() {
    use cs_content::instant_action::IA_LOADOUT_EDITOR_MEMBER;

    let root = crate::f27_d_support::game_dir();
    let find = crate::f27_d_support::find;
    let editor = crate::f27_d_support::read_member(&root, IA_LOADOUT_EDITOR_MEMBER);

    // The editor's slot surface: four gun names, four ammunition dropdowns,
    // eight rocket dropdowns, the two dropdown types and the read/write
    // callbacks.
    for needle in [
        &b"object PKA[4]"[..],
        b"object QKA[4]",
        b"object RKA[8]",
        b"callback($$E$$, 2030, (ZQ), (BA)",
        b"callback($$E$$, 2031, (ZQ), (BA)",
        b"callback($$E$$, 2036, (ZQ), (BA)",
        b"callback($$E$$, 2037, (ZQ), (BA)",
    ] {
        assert!(
            find(&editor, needle).is_some(),
            "{IA_LOADOUT_EDITOR_MEMBER} no longer carries {:?}",
            String::from_utf8_lossy(needle)
        );
    }

    let image = crate::f64_b_support::engine_image();
    let base = 0x400000usize;
    let dword = |va: usize| {
        u32::from_le_bytes(image[va - base..va - base + 4].try_into().expect("4 bytes"))
    };

    // The rocket selector table at 0x619efc: twelve rows of eight bytes, in
    // the order ORIGINAL_ROCKET_SELECTOR_ROWS commits them.
    for (id, row) in ORIGINAL_ROCKET_SELECTOR_ROWS.iter().enumerate() {
        let va = 0x619efc + id * 8;
        assert_eq!(
            (row.gate, row.aux),
            (dword(va), dword(va + 4)),
            "rocket selector row {id} drifted"
        );
    }

    // The roster template table at 0x619f58: records 0..=10 keyed by
    // scene_root, each record's +0x2c plane id equal to its index.
    for (index, (root, loadout)) in ORIGINAL_AIRFRAME_LOADOUTS.iter().enumerate() {
        let rec = 0x619f58 + index * 0xcc;
        assert_eq!(dword(rec + 0x2c) as usize, index, "{root}: plane id");
        assert_eq!(
            image[rec + 0x84 - base],
            loadout.paired_gun_mask(),
            "{root}: pair mask"
        );
        assert_eq!(
            (dword(rec + 0x34), dword(rec + 0x38)),
            loadout.rocket_banks(),
            "{root}: rocket banks"
        );
        for slot in 0..4 {
            assert_eq!(
                dword(rec + 0x88 + 4 * slot),
                loadout.guns()[slot],
                "{root} gun {slot}"
            );
            assert_eq!(
                dword(rec + 0x98 + 4 * slot),
                loadout.ammo()[slot],
                "{root} ammo {slot}"
            );
        }
        for slot in 0..8 {
            assert_eq!(
                dword(rec + 0xa8 + 4 * slot),
                loadout.rockets()[slot],
                "{root} rocket {slot}"
            );
        }
        assert!(loadout.guns()[0] != ORIGINAL_GUN_NONE);
    }

    // The selector's gate rule is real: at progress 0 only the gate-1 rows —
    // ids 0, 1 and the "none" row — are offered, and an unlock override opens
    // every row.
    assert_eq!(
        cs_content::weapons::original_offered_rockets(0, false),
        vec![0, 1, ORIGINAL_ROCKET_NONE]
    );
    assert_eq!(
        cs_content::weapons::original_offered_rockets(24, false).len(),
        ORIGINAL_ROCKET_SELECTOR_ROWS.len()
    );
}
