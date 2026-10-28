//! Task #2187 (T452-REG-SMOKE-STUN): what the smoke emitter's stun does to a
//! struck actor. Task test prefix: `accept_t452_smoke_stun_`.
//!
//! Static analysis of `0x4200d0` (AI actor) and `0x42e9d0` (player) in the
//! owner-supplied decrypted engine image; never original-run behavior.

use cs_content::ordnance::{
    DeclaredOrdnanceFamily, OriginalAreaDeclaration, OriginalClusterShape, OriginalComponentLaunch,
    OriginalDamageDeclaration, OriginalLifetime, OriginalMountDeclaration,
    OriginalStatusEffectDeclaration, SmokeStunRule, T452_R02_SMOKE_STUN_EFFECT,
    measure_component_behavior, read_original_component_table,
};
use cs_content::original_airframe::OriginalDocuments;

const IMAGE_BASE: u64 = 0x40_0000;

fn smoke_row() -> OriginalComponentLaunch {
    OriginalComponentLaunch {
        record: "wep_13".to_owned(),
        family: DeclaredOrdnanceFamily::AreaDenialEngine,
        mount: OriginalMountDeclaration::RocketSlot { slots: 8 },
        velocity: None,
        fire_rate: None,
        proximity_trigger_m: None,
        lifetime: OriginalLifetime::Undeclared,
        area: OriginalAreaDeclaration::default(),
        damage: OriginalDamageDeclaration::default(),
        statuses: vec![OriginalStatusEffectDeclaration {
            key: "SMOKE_SCREEN",
            duration_s: Some(8.0),
            duration_unit: Some("TIME".to_owned()),
        }],
        cluster: OriginalClusterShape::Volley { munitions: 6 },
        unknowns: Vec::new(),
    }
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn accept_t452_smoke_stun_wep_13_carries_the_rule_and_not_the_claim() {
    let measured = measure_component_behavior(&smoke_row());
    assert!(!measured.names_claim(T452_R02_SMOKE_STUN_EFFECT));
    assert!(
        measured
            .effects
            .iter()
            .all(|effect| effect.claim != Some(T452_R02_SMOKE_STUN_EFFECT))
    );
    let rule = measured.smoke_stun.expect("SMOKE_SCREEN carries the rule");
    assert_eq!(rule.range_key, "smokescreen_stun_range");
    assert!(rule.excludes_owner && rule.every_step);
    assert_eq!(rule.ai.incapacitation.mode, 4);
    assert_eq!(rule.ai.incapacitation.control_words_zeroed, 6);
    assert!(!rule.player.control_loss);
}

#[test]
fn accept_t452_smoke_stun_other_records_carry_no_rule() {
    let mut row = smoke_row();
    row.statuses[0].key = "TANGLER";
    assert!(measure_component_behavior(&row).smoke_stun.is_none());
}

#[test]
fn accept_t452_smoke_stun_reach_is_strict_range_and_cone() {
    let defaults = SmokeStunRule::measured().at_image_defaults();
    assert!(near(defaults.range_m, 200.0));
    assert!(near(defaults.cone_cosine, 0.8));
    assert!(near(defaults.interval_s, 3.0));
    assert!(defaults.reaches(199.0, 0.9));
    assert!(!defaults.reaches(200.0, 0.9), "range is strict");
    assert!(!defaults.reaches(100.0, 0.8), "the cone is strict");
    assert!(!defaults.reaches(100.0, 0.0));
}

#[test]
fn accept_t452_smoke_stun_ai_stun_lasts_the_interval() {
    let rule = SmokeStunRule::measured().at_image_defaults();
    assert!(near(rule.ai_stun_ends_at(10.0), 13.0));
}

#[test]
fn accept_t452_smoke_stun_player_tint_rearms_on_its_own_timer() {
    let rule = SmokeStunRule::measured().at_image_defaults();
    let first = rule.player_tint(0.0).expect("an unarmed emitter tints");
    assert!(near(first.seconds, 2.0) && near(first.rearm_seconds, 2.0));
    assert!(near(first.scalar, 0.97));
    let again = rule.player_tint(0.4).expect("below the 0.5 gate");
    assert!(near(again.scalar, 0.9));
    assert!(rule.player_tint(0.5).is_none(), "the gate is strict");
    assert!(rule.player_tint(2.0).is_none(), "just re-armed");
    let colour = SmokeStunRule::measured().player.colour;
    assert!(near(colour[0], 0.2) && near(colour[1], 0.29) && near(colour[2], 0.145));
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t452_smoke_stun_retail_cone_is_an_85_degree_half_angle() {
    let dir = std::env::var("CS_GAME_DIR")
        .expect("CS_GAME_DIR is not set: this measurement needs the owner's installation");
    let root = std::path::Path::new(&dir);
    let globals = OriginalDocuments::read(root)
        .expect("the retail documents read")
        .smokescreen_stun()
        .expect("the retail globals state the smokescreen keys");
    let rule = SmokeStunRule::measured().resolve(&globals);
    assert!(near(rule.range_m, 600.0) && near(rule.interval_s, 5.0));
    assert!(near(rule.cone_cosine, 85f64.to_radians().cos()));
    assert!(rule.reaches(599.0, 0.1));
    assert!(!rule.reaches(599.0, 0.05));

    let table = read_original_component_table(root).expect("the component table reads");
    let row = table
        .rows
        .iter()
        .find(|row| row.record == "wep_13")
        .expect("wep_13 is catalogued");
    assert!(measure_component_behavior(row).smoke_stun.is_some());
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_t452_smoke_stun_the_image_pins_both_stun_routines() {
    let path = std::env::var_os("CS_ENGINE_IMAGE").expect(
        "CS_ENGINE_IMAGE is not set: this measurement needs the owner-supplied decrypted \
         executable",
    );
    let image = std::fs::read(&path).expect("the engine image reads");
    let at = |va: u64, len: usize| {
        let offset = usize::try_from(va - IMAGE_BASE).expect("offset");
        image[offset..offset + len].to_vec()
    };
    // 0x4200d0: mode 4 stored at +0x358, six control words zeroed, +0xc0 set.
    assert_eq!(
        at(0x0042_0127, 10),
        [0xc7, 0x86, 0x58, 0x03, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00]
    );
    assert_eq!(at(0x0042_015a, 6), [0x89, 0x9e, 0x00, 0x01, 0x00, 0x00]);
    assert_eq!(at(0x0042_0179, 6), [0xd9, 0x9e, 0xc0, 0x00, 0x00, 0x00]);
    // 0x4b9170: the AI call passes the interval global.
    assert_eq!(
        at(0x004b_9170, 9),
        [0xa1, 0xe4, 0xc3, 0x71, 0x00, 0x50, 0x8b, 0xcf, 0xe8]
    );
    // 0x4b91ad..0x4b91d3: scalar 0.9, tint 2.0 s, re-arm 2.0 s.
    assert_eq!(
        at(0x004b_91ad, 7),
        [0xc7, 0x45, 0xfc, 0x66, 0x66, 0x66, 0x3f]
    );
    assert_eq!(at(0x004b_91cc, 5), [0x68, 0x00, 0x00, 0x00, 0x40]);
    assert_eq!(
        at(0x004b_91d3, 7),
        [0xc7, 0x46, 0x14, 0x00, 0x00, 0x00, 0x40]
    );
}
