//! Task #2180 (T452-REG-SMOKE-RADIUS): the smoke emitter's gameplay radius.
//!
//! Task test prefix: `accept_t452_smoke_`. Finding:
//! `docs/findings/2026-10-28-t452-reg-smoke-radius.md`.
//!
//! T452.R02 recorded "no code reads a gameplay radius" for `SMOKE_SCREEN`. The
//! emitter's per-step update (`0x4b8fd0`) does: it compares the distance from
//! the owner to every other live actor with the `smokescreen_stun_range`
//! global. The radius is therefore measured, and it comes from `player.zrd`,
//! not from the weapon record. Static analysis only, never original-run
//! behavior.

use cs_content::ordnance::{
    DeclaredOrdnanceFamily, MeasuredAreaSource, OriginalAreaDeclaration, OriginalClusterShape,
    OriginalComponentLaunch, OriginalDamageDeclaration, OriginalLifetime, OriginalMountDeclaration,
    OriginalStatusEffectDeclaration, T452_04_AREA_RADIUS, T452_R02_SMOKE_RADIUS,
    T452_R02_SMOKE_STUN_EFFECT, measure_component_behavior, read_original_component_table,
};
use cs_content::original_airframe::OriginalDocuments;

const IMAGE_BASE: u64 = 0x40_0000;

/// `(virtual address, bytes, what the window carries)`.
const IMAGE_PINS: [(u64, &[u8], &str); 5] = [
    (
        0x004b_90cc,
        &[0xff, 0x10, 0x8b, 0x17, 0x8b, 0xcf],
        "the emitter update takes the owner's and another actor's positions",
    ),
    (
        0x004b_9101,
        &[
            0xe8, 0x8a, 0x95, 0xf6, 0xff, 0xd8, 0x1d, 0xdc, 0xc3, 0x71, 0x00,
        ],
        "the distance is compared with the global at 0x71c3dc",
    ),
    (
        0x004b_914d,
        &[0xd8, 0x1d, 0xe0, 0xc3, 0x71, 0x00],
        "the facing dot product is compared with the global at 0x71c3e0",
    ),
    (
        0x0047_3eb0,
        &[
            0x8b, 0x48, 0x0c, 0x89, 0x0d, 0xdc, 0xc3, 0x71, 0x00, 0xeb, 0x0a, 0xc7, 0x05, 0xdc,
            0xc3, 0x71, 0x00, 0x00, 0x00, 0x48, 0x43,
        ],
        "0x71c3dc is the looked-up key's value, defaulting to 200.0 (0x43480000)",
    ),
    (
        0x0047_3ed7,
        &[0x8b, 0x50, 0x04, 0xd9, 0x42, 0x0c],
        "the angle key is read from the same lookup",
    ),
];

/// The three key names the loader looks up (strings at `0x6276b0`,
/// `0x6276c8`, `0x6276e0`).
const IMAGE_KEYS: [(u64, &str); 3] = [
    (0x0062_76b0, "smokescreen_stun_range"),
    (0x0062_76c8, "smokescreen_stun_angle"),
    (0x0062_76e0, "smokescreen_stun_interval"),
];

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

#[test]
fn accept_t452_smoke_the_radius_claim_is_resolved_as_a_global_key() {
    let measured = measure_component_behavior(&smoke_row());
    let area = measured.area.expect("a smoke screen has an area");
    assert_eq!(
        area.radius_source,
        MeasuredAreaSource::GlobalKey {
            key: "smokescreen_stun_range",
            image_default: 200.0,
        }
    );
    assert!(measured.resolves_claim(T452_04_AREA_RADIUS));
    assert!(
        !measured.names_claim(T452_R02_SMOKE_RADIUS),
        "the radius claim is resolved, no record keeps it"
    );
    assert!(
        measured.names_claim(T452_R02_SMOKE_STUN_EFFECT),
        "what the stun does is a separate, still-open claim"
    );
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t452_smoke_retail_states_the_range_cone_and_interval() {
    let dir = std::env::var("CS_GAME_DIR")
        .expect("CS_GAME_DIR is not set: this measurement needs the owner's installation");
    let root = std::path::Path::new(&dir);
    let stun = OriginalDocuments::read(root)
        .expect("the retail documents read")
        .smokescreen_stun()
        .expect("the retail globals state the smokescreen keys");
    assert_eq!(stun.range_m, 600.0);
    assert_eq!(stun.cone_angle_deg, 170.0);
    assert_eq!(stun.interval_s, 5.0);

    // The same production reader the registration uses lowers wep_13 to the
    // global-key answer.
    let table = read_original_component_table(root).expect("the component table reads");
    let row = table
        .rows
        .iter()
        .find(|row| row.record == "wep_13")
        .expect("wep_13 is catalogued");
    let measured = measure_component_behavior(row);
    let area = measured.area.expect("wep_13 is an area");
    assert!(matches!(
        area.radius_source,
        MeasuredAreaSource::GlobalKey { .. }
    ));
    assert_eq!(area.lifetime_s, Some(8.0));
    assert!(!measured.names_claim(T452_R02_SMOKE_RADIUS));
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_t452_smoke_the_image_pins_the_radius_consumer() {
    let path = std::env::var_os("CS_ENGINE_IMAGE").expect(
        "CS_ENGINE_IMAGE is not set: this measurement needs the owner-supplied decrypted \
         executable",
    );
    let image = std::fs::read(&path).expect("the engine image reads");
    for (address, bytes, meaning) in IMAGE_PINS {
        let offset = usize::try_from(address - IMAGE_BASE).expect("offset");
        assert_eq!(
            &image[offset..offset + bytes.len()],
            bytes,
            "0x{address:x} no longer carries: {meaning}"
        );
    }
    for (address, key) in IMAGE_KEYS {
        let offset = usize::try_from(address - IMAGE_BASE).expect("offset");
        let bytes = key.as_bytes();
        assert_eq!(&image[offset..offset + bytes.len()], bytes, "key {key}");
        assert_eq!(image[offset + bytes.len()], 0, "key {key} is terminated");
    }
}
