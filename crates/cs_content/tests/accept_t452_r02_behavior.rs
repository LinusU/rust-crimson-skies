//! Task #2160 (T452.R02): the measured original ordnance effect, fuse and
//! swarm behavior.
//!
//! Task test prefix: `accept_t452_r02_`. Finding:
//! `docs/findings/2026-10-28-t452-r02-ordnance-effects-fuses-and-swarm.md`.
//!
//! The pure tests call [`measure_component_behavior`] on hand-built rows; the
//! retail test runs the same production function on the owner's installation;
//! the image test re-reads every byte window the measured facts cite from
//! `$CS_ENGINE_IMAGE` (static analysis, never original-run behavior).

use cs_content::ordnance::{
    DeclaredOrdnanceFamily, DeclaredStatusEffectKind, MeasuredAreaSource, MeasuredEffectAction,
    MeasuredEffectDuration, MeasuredRangeEnd, OriginalAreaDeclaration, OriginalClusterShape,
    OriginalComponentLaunch, OriginalDamageDeclaration, OriginalLifetime, OriginalMountDeclaration,
    OriginalStatusEffectDeclaration, StaticProvenance, T452_04_AREA_LIFETIME, T452_04_AREA_RADIUS,
    T452_04_CLUSTER, T452_04_STATUS_DURATION, T452_04_STATUS_KIND, T452_R02_BEEPER_SEEKER,
    T452_R02_FUSE_CLAIM, T452_R02_LOST_TARGET_CLAIM, T452_R02_SMOKE_RADIUS,
    T452_R02_STATUS_INCAPACITATION, T452_R02_TANGLER_EFFECT, measure_component_behavior,
    read_original_component_table,
};

/// The image base the executable's virtual addresses are relative to.
const IMAGE_BASE: u64 = 0x40_0000;

/// `(virtual address, bytes, what the window carries)`.
const IMAGE_PINS: [(u64, &[u8], &str); 23] = [
    (
        0x004b_a7f8,
        &[0x81, 0xc9, 0x00, 0x00, 0x01, 0x00],
        "SMOKE_SCREEN sets weapon-object flag 0x10000",
    ),
    (
        0x004b_a806,
        &[0xc7, 0x46, 0x18, 0x00, 0x00, 0xa0, 0x40],
        "SMOKE_SCREEN: TIMEs default is 5.0 s at +0x18",
    ),
    (
        0x004b_a82d,
        &[0xdb, 0x40, 0x0c, 0xd9, 0x5e, 0x18],
        "SMOKE_SCREEN: the record's TIME value is stored at +0x18",
    ),
    (
        0x004b_a859,
        &[0x81, 0x0e, 0x00, 0x00, 0x04, 0x00],
        "TANGLER sets weapon-object flag 0x40000",
    ),
    (
        0x004b_a870,
        &[0xc7, 0x46, 0x1c, 0x00, 0x00, 0x20, 0x41],
        "TANGLER: RADIUS default is 10.0 m at +0x1c",
    ),
    (
        0x004b_a779,
        &[0xc7, 0x46, 0x18, 0x00, 0x00, 0xa0, 0x40],
        "BEEPER: TIME default is 5.0 s at +0x18",
    ),
    (
        0x004b_950f,
        &[
            0xd9, 0x41, 0x1c, 0x6a, 0x0c, 0xd9, 0xc0, 0xd8, 0xc9, 0xd9, 0x5e, 0x14, 0x8b, 0x90,
            0x10, 0x02, 0x00, 0x00, 0xdd, 0xd8, 0xd9, 0x42, 0x18, 0xd9, 0x5e, 0x18,
        ],
        "tangler field 0x4b94e0: radius squared to +0x14, the weapon's TIME to +0x18",
    ),
    (
        0x004b_9597,
        &[
            0xd9, 0x47, 0x18, 0xd8, 0x64, 0x24, 0x14, 0xd9, 0x57, 0x18, 0xd8, 0x1d, 0xc8, 0x32,
            0x60, 0x00,
        ],
        "tangler tick 0x4b9590: remaining -= dt and expires at <= 0",
    ),
    (
        0x004b_95e4,
        &[0xd8, 0x5f, 0x14],
        "tangler tick: each actor's squared distance is compared with the field's radius squared",
    ),
    (
        0x0042_e85b,
        &[
            0xd9, 0x45, 0x18, 0xc6, 0x45, 0x13, 0x00, 0xd8, 0x75, 0x1c, 0xd8, 0x15, 0xdc, 0x32,
            0x60, 0x00, 0xdf, 0xe0, 0xf6, 0xc4, 0x41, 0x0f, 0x85, 0xf2, 0x00, 0x00, 0x00, 0xdd,
            0xd8, 0xd9, 0x05, 0xdc, 0x32, 0x60, 0x00, 0xd8, 0x25, 0x1c, 0x41, 0x60, 0x00, 0xd8,
            0x0d, 0xa8, 0x40, 0x60, 0x00, 0xd8, 0x2d, 0xdc, 0x32, 0x60, 0x00,
        ],
        "exposure 0x42e840: ratio = arg18 / arg1c, compared with 1.0, then 1 - (ratio - 0.6) * 2.5",
    ),
    (
        0x0042_e87e,
        &[
            0xd8, 0x25, 0x1c, 0x41, 0x60, 0x00, 0xd8, 0x0d, 0xa8, 0x40, 0x60, 0x00, 0xd8, 0x2d,
            0xdc, 0x32, 0x60, 0x00,
        ],
        "exposure falloff: -0.6, *2.5, 1 - value (constants 0x60411c and 0x6040a8)",
    ),
    (
        0x0042_e9b1,
        &[0xd8, 0x0d, 0xbc, 0x36, 0x60, 0x00],
        "exposure output: intensity * 5.0 s is the incapacitation duration",
    ),
    (
        0x005b_01ec,
        &[
            0xd8, 0x5f, 0x1c, 0xdf, 0xe0, 0xf6, 0xc4, 0x01, 0x8b, 0x47, 0x74, 0x75, 0x1d, 0xf6,
            0xc4, 0x80, 0x74, 0x08, 0xa8, 0x80, 0x0f, 0x84, 0xb4, 0x00, 0x00, 0x00, 0xa9, 0x00,
            0x00, 0x00, 0x02, 0x0f, 0x84, 0x07, 0x01, 0x00, 0x00, 0xe9, 0xa4, 0x00, 0x00, 0x00,
        ],
        "range end: traveled vs RANGE (+0x1c), then LOCK_ON (0x8000) without EXPIRES (0x80), then DETONATE_AT_RANGE (0x2000000)",
    ),
    (
        0x005b_0318,
        &[0xc7, 0x46, 0x20, 0x00, 0x00, 0x00, 0x00],
        "range end: the projectile is marked dead (+0x20 = 0)",
    ),
    (
        0x005a_ddd5,
        &[0x0d, 0x00, 0x80, 0x00, 0x00],
        "the LOCK_ON key sets flag bit 15 (0x8000) on the weapon object",
    ),
    (
        0x004b_aa17,
        &[0x89, 0x56, 0x0c],
        "CLUSTER_SIZE is stored at weapon-object +0x0c",
    ),
    (
        0x004b_72be,
        &[0xff, 0x4f, 0x0c],
        "each launch decrements the weapon object's +0x0c",
    ),
    (
        0x004b_72e1,
        &[
            0x8b, 0x47, 0x0c, 0x85, 0xc0, 0x0f, 0x8f, 0xbb, 0xfb, 0xff, 0xff,
        ],
        "the launch loop stops when the +0x0c countdown reaches 0",
    ),
    (
        0x0044_1877,
        &[0xc7, 0x83, 0x88, 0x06, 0x00, 0x00, 0x80, 0x17, 0x44, 0x00],
        "BEEPER_SEEKER installs the per-step retarget callback 0x441780",
    ),
    (
        0x004b_71b1,
        &[0xa9, 0x00, 0x00, 0x01, 0x00],
        "the fire path tests SMOKE_SCREEN (0x10000) before launching",
    ),
    (
        0x004b_71da,
        &[0xd9, 0x41, 0x18],
        "the smoke emitter reads the weapon's TIME at +0x18",
    ),
    (
        0x004b_71e8,
        &[0xe8, 0x63, 0x1b, 0x00, 0x00],
        "the smoke emitter node is built by 0x4b8d50",
    ),
    (
        0x005a_f7f4,
        &[
            0xf6, 0xc4, 0x80, 0x74, 0x0f, 0x39, 0x5e, 0x70, 0x74, 0x0a, 0x56, 0x57,
        ],
        "steering guard: LOCK_ON (0x8000) and a non-null target pointer (+0x70) gate the steering call",
    ),
];

fn row(
    record: &str,
    family: DeclaredOrdnanceFamily,
    mount: OriginalMountDeclaration,
    statuses: Vec<OriginalStatusEffectDeclaration>,
    area: OriginalAreaDeclaration,
    cluster: OriginalClusterShape,
) -> OriginalComponentLaunch {
    OriginalComponentLaunch {
        record: record.to_owned(),
        family,
        mount,
        velocity: None,
        fire_rate: None,
        proximity_trigger_m: None,
        lifetime: OriginalLifetime::Undeclared,
        area,
        damage: OriginalDamageDeclaration::default(),
        statuses,
        cluster,
        unknowns: Vec::new(),
    }
}

fn status(key: &'static str, duration_s: Option<f64>) -> OriginalStatusEffectDeclaration {
    OriginalStatusEffectDeclaration {
        key,
        duration_s,
        duration_unit: duration_s.map(|_| "TIME".to_owned()),
    }
}

fn rocket_slot() -> OriginalMountDeclaration {
    OriginalMountDeclaration::RocketSlot { slots: 8 }
}

/// The code half: the owner-supplied image carries the byte windows every
/// measured fact rests on.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_t452_r02_the_image_pins_every_measured_site() {
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
}

#[test]
fn accept_t452_r02_a_beeper_is_a_timed_marker() {
    let row = row(
        "wep_10",
        DeclaredOrdnanceFamily::GuidedRocket,
        rocket_slot(),
        vec![status("BEEPER", Some(20.0))],
        OriginalAreaDeclaration::default(),
        OriginalClusterShape::Volley { munitions: 4 },
    );
    let measured = measure_component_behavior(&row);
    let beeper = measured
        .effects
        .iter()
        .find(|effect| effect.key == "BEEPER")
        .expect("the beeper effect is measured");
    assert_eq!(beeper.action, MeasuredEffectAction::TimedMarker);
    assert_eq!(beeper.duration, MeasuredEffectDuration::RecordTime);
    assert_eq!(beeper.runtime_kind, Some(DeclaredStatusEffectKind::Marker));
    assert!(measured.resolves_claim(T452_04_STATUS_KIND));
    assert!(measured.resolves_claim(T452_04_STATUS_DURATION));
    assert!(!measured.names_claim(T452_04_STATUS_KIND));
}

#[test]
fn accept_t452_r02_sonic_and_flash_are_an_exposure_with_no_runtime_kind() {
    let row = row(
        "wep_08",
        DeclaredOrdnanceFamily::GuidedRocket,
        rocket_slot(),
        vec![status("SONIC", None), status("FLASH", None)],
        OriginalAreaDeclaration::default(),
        OriginalClusterShape::Volley { munitions: 2 },
    );
    let measured = measure_component_behavior(&row);
    assert_eq!(measured.effects.len(), 2);
    for effect in &measured.effects {
        assert_eq!(
            effect.action,
            MeasuredEffectAction::ExposureIncapacitation,
            "{}",
            effect.key
        );
        assert_eq!(effect.runtime_kind, None, "no kind is forced");
        assert_eq!(effect.claim, Some(T452_R02_STATUS_INCAPACITATION));
        match effect.duration {
            MeasuredEffectDuration::Exposure {
                max_seconds,
                full_intensity_ratio,
                zero_intensity_ratio,
                facing_scaled,
            } => {
                assert_eq!(max_seconds, 5.0);
                assert_eq!(full_intensity_ratio, 0.6);
                assert_eq!(zero_intensity_ratio, 1.0);
                assert_eq!(facing_scaled, effect.key == "FLASH");
            }
            other => panic!("{} is not an exposure: {other:?}", effect.key),
        }
    }
    assert!(measured.names_claim(T452_R02_STATUS_INCAPACITATION));
    // The duration is measured, so the missing-duration claim is resolved.
    assert!(measured.resolves_claim(T452_04_STATUS_DURATION));
    assert!(!measured.names_claim(T452_04_STATUS_DURATION));
}

#[test]
fn accept_t452_r02_a_tangler_is_an_area_with_the_image_default_radius() {
    let row = row(
        "wep_12",
        DeclaredOrdnanceFamily::AreaDenialEngine,
        rocket_slot(),
        vec![status("TANGLER", Some(2.0))],
        OriginalAreaDeclaration::default(),
        OriginalClusterShape::Volley { munitions: 2 },
    );
    let measured = measure_component_behavior(&row);
    let area = measured.area.expect("a tangler has an area");
    assert_eq!(area.radius_m, Some(10.0));
    assert_eq!(
        area.radius_source,
        MeasuredAreaSource::ImageDefault { value: 10.0 }
    );
    assert_eq!(area.lifetime_s, Some(2.0));
    assert_eq!(
        area.lifetime_source,
        MeasuredAreaSource::RecordKey { key: "TIME" }
    );
    assert!(measured.resolves_claim(T452_04_AREA_RADIUS));
    assert!(measured.resolves_claim(T452_04_AREA_LIFETIME));
    assert!(!measured.names_claim(T452_04_AREA_RADIUS));
    assert!(measured.names_claim(T452_R02_TANGLER_EFFECT));
}

#[test]
fn accept_t452_r02_a_smoke_screen_has_a_lifetime_but_no_measured_radius() {
    let row = row(
        "wep_13",
        DeclaredOrdnanceFamily::AreaDenialEngine,
        rocket_slot(),
        vec![status("SMOKE_SCREEN", Some(8.0))],
        OriginalAreaDeclaration::default(),
        OriginalClusterShape::Volley { munitions: 6 },
    );
    let measured = measure_component_behavior(&row);
    let area = measured.area.expect("a smoke screen has an area");
    assert_eq!(area.radius_m, None);
    assert_eq!(
        area.radius_source,
        MeasuredAreaSource::Unmeasured {
            claim: T452_R02_SMOKE_RADIUS
        }
    );
    assert_eq!(area.lifetime_s, Some(8.0));
    assert!(measured.resolves_claim(T452_04_AREA_LIFETIME));
    assert!(measured.names_claim(T452_R02_SMOKE_RADIUS));
    // The record states no trigger at all, so its flight is now measured.
    assert_eq!(measured.fuse.range_end, MeasuredRangeEnd::ExpiresSilently);
    assert!(measured.resolves_claim(T452_R02_FUSE_CLAIM));
}

#[test]
fn accept_t452_r02_a_blast_is_instantaneous_and_has_no_area_lifetime() {
    let mut row = row(
        "wep_26",
        DeclaredOrdnanceFamily::ProximityFlak,
        rocket_slot(),
        Vec::new(),
        OriginalAreaDeclaration {
            radius_m: Some(100.0),
            lifetime_s: None,
        },
        OriginalClusterShape::Volley { munitions: 1 },
    );
    row.proximity_trigger_m = None;
    let measured = measure_component_behavior(&row);
    let area = measured.area.expect("a blast has a radius");
    assert_eq!(area.radius_m, Some(100.0));
    assert_eq!(area.lifetime_source, MeasuredAreaSource::InstantaneousBlast);
    assert_eq!(area.lifetime_s, None);
    assert!(measured.resolves_claim(T452_04_AREA_LIFETIME));
    // `wep_26` states no proximity trigger and no detonation key: its end of
    // flight is now measured and it expires instead of detonating.
    assert_eq!(measured.fuse.range_end, MeasuredRangeEnd::ExpiresSilently);
    assert!(measured.resolves_claim(T452_R02_FUSE_CLAIM));
}

#[test]
fn accept_t452_r02_a_locked_on_record_detonates_an_unguided_one_expires() {
    let glidebomb = row(
        "wep_25",
        DeclaredOrdnanceFamily::GuidedRocket,
        OriginalMountDeclaration::Unknown {
            claim: "t452_04.limit.rocket_flag_missing".to_owned(),
            reason: "no screen states a slot".to_owned(),
        },
        Vec::new(),
        OriginalAreaDeclaration {
            radius_m: Some(155.0),
            lifetime_s: None,
        },
        OriginalClusterShape::Volley { munitions: 1 },
    );
    let measured = measure_component_behavior(&glidebomb);
    assert_eq!(measured.fuse.range_end, MeasuredRangeEnd::Detonates);
    assert!(measured.fuse.consumed);
    let lost_target = measured.lost_target.expect("a seeker has lost-target rule");
    assert!(lost_target.requires_lock_on);
    assert_eq!(lost_target.behavior, "coast_then_detonate_at_range_end");
    assert!(measured.resolves_claim(T452_R02_LOST_TARGET_CLAIM));
    assert_eq!(
        measured.fuse.gate_pin,
        StaticProvenance::new(0x005b_01ec, 42)
    );
    assert_eq!(lost_target.pin, StaticProvenance::new(0x005a_f7f4, 12));

    let flak = row(
        "wep_26",
        DeclaredOrdnanceFamily::ProximityFlak,
        rocket_slot(),
        Vec::new(),
        OriginalAreaDeclaration {
            radius_m: Some(100.0),
            lifetime_s: None,
        },
        OriginalClusterShape::Volley { munitions: 1 },
    );
    let measured = measure_component_behavior(&flak);
    assert_eq!(measured.fuse.range_end, MeasuredRangeEnd::ExpiresSilently);
    assert!(measured.lost_target.is_none());
}

#[test]
fn accept_t452_r02_a_volley_launches_one_projectile_per_sub_munition() {
    let volley = row(
        "wep_24",
        DeclaredOrdnanceFamily::GuidedRocket,
        rocket_slot(),
        Vec::new(),
        OriginalAreaDeclaration::default(),
        OriginalClusterShape::Volley { munitions: 7 },
    );
    let measured = measure_component_behavior(&volley);
    let swarm = measured.swarm.expect("a volley is a swarm");
    assert_eq!(swarm.munitions, 7);
    assert_eq!(swarm.projectiles_per_launch, 1);
    assert_eq!(swarm.pin, StaticProvenance::new(0x004b_72be, 11));
    assert!(measured.resolves_claim(T452_04_CLUSTER));

    let gun = row(
        "wep_130",
        DeclaredOrdnanceFamily::DirectExplosive,
        OriginalMountDeclaration::RocketSlot { slots: 8 },
        Vec::new(),
        OriginalAreaDeclaration::default(),
        OriginalClusterShape::AmmunitionCount { rounds: 2800 },
    );
    assert!(measure_component_behavior(&gun).swarm.is_none());
}

#[test]
fn accept_t452_r02_a_beeper_seeker_is_a_named_guidance_claim() {
    let row = row(
        "wep_11",
        DeclaredOrdnanceFamily::GuidedRocket,
        rocket_slot(),
        vec![status("BEEPER_SEEKER", None)],
        OriginalAreaDeclaration::default(),
        OriginalClusterShape::Volley { munitions: 4 },
    );
    let measured = measure_component_behavior(&row);
    let effect = &measured.effects[0];
    assert_eq!(effect.action, MeasuredEffectAction::GuidanceModifier);
    assert_eq!(effect.duration, MeasuredEffectDuration::None);
    assert_eq!(effect.runtime_kind, None);
    assert!(measured.names_claim(T452_R02_BEEPER_SEEKER));
}

/// The installation half: the same production function over the measured rows.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t452_r02_retail_the_measured_facts_read_the_original_rows() {
    let dir = std::env::var("CS_GAME_DIR")
        .expect("CS_GAME_DIR is not set: this measurement needs the owner's installation");
    let table = read_original_component_table(std::path::Path::new(&dir))
        .expect("the component table reads");
    let measured: Vec<_> = table
        .rows
        .iter()
        .map(|row| (row.record.as_str(), measure_component_behavior(row)))
        .collect();
    let get = |record: &str| {
        measured
            .iter()
            .find(|(name, _)| *name == record)
            .map(|(_, measured)| measured)
            .unwrap_or_else(|| panic!("{record} is not in the measured table"))
    };

    let tangler = get("wep_12");
    let area = tangler.area.expect("wep_12 is an area");
    assert_eq!(area.radius_m, Some(10.0));
    assert_eq!(area.lifetime_s, Some(2.0));
    assert!(tangler.resolves_claim(T452_04_AREA_RADIUS));
    assert!(tangler.resolves_claim(T452_04_AREA_LIFETIME));
    assert_eq!(tangler.fuse.range_end, MeasuredRangeEnd::ExpiresSilently);

    let smoke = get("wep_13");
    let area = smoke.area.expect("wep_13 is an area");
    assert_eq!(area.radius_m, None);
    assert_eq!(area.lifetime_s, Some(8.0));
    assert!(smoke.names_claim(T452_R02_SMOKE_RADIUS));
    assert!(smoke.resolves_claim(T452_R02_FUSE_CLAIM));
    assert_eq!(smoke.fuse.range_end, MeasuredRangeEnd::ExpiresSilently);

    let flak = get("wep_26");
    assert_eq!(flak.fuse.range_end, MeasuredRangeEnd::ExpiresSilently);
    assert!(flak.resolves_claim(T452_R02_FUSE_CLAIM));
    assert!(flak.lost_target.is_none());

    let glidebomb = get("wep_25");
    assert_eq!(glidebomb.fuse.range_end, MeasuredRangeEnd::Detonates);
    assert!(glidebomb.lost_target.expect("a seeker").requires_lock_on);
    assert!(glidebomb.resolves_claim(T452_R02_LOST_TARGET_CLAIM));

    let flare = get("wep_10");
    let beeper = flare
        .effects
        .iter()
        .find(|effect| effect.key == "BEEPER")
        .expect("wep_10 declares BEEPER");
    assert_eq!(beeper.runtime_kind, Some(DeclaredStatusEffectKind::Marker));
    assert_eq!(beeper.duration, MeasuredEffectDuration::RecordTime);

    let sonic = get("wep_08");
    assert_eq!(
        sonic.effects[0].action,
        MeasuredEffectAction::ExposureIncapacitation
    );
    let flash = get("wep_09");
    assert_eq!(
        flash.effects[0].action,
        MeasuredEffectAction::ExposureIncapacitation
    );

    let seeker = get("wep_11");
    assert!(seeker.names_claim(T452_R02_BEEPER_SEEKER));

    for (name, measured) in &measured {
        if let OriginalClusterShape::Volley { munitions } = table
            .rows
            .iter()
            .find(|row| row.record == *name)
            .expect("the row exists")
            .cluster
        {
            let swarm = measured
                .swarm
                .unwrap_or_else(|| panic!("{name} is a volley and must be measured"));
            assert_eq!(swarm.munitions, munitions, "{name}");
            assert_eq!(swarm.projectiles_per_launch, 1, "{name}");
            assert!(measured.resolves_claim(T452_04_CLUSTER), "{name}");
        }
        // A resolved claim is never still named as unmeasured on the same row.
        for resolved in &measured.resolved {
            assert!(
                !measured.names_claim(resolved),
                "{name} resolves {resolved} and still names it"
            );
        }
    }
}
