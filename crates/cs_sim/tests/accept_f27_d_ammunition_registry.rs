//! Acceptance scenario F27-D, runtime half: every ammunition type a session can
//! fire is mapped to the damage consumer that applies it.
//!
//! Spec: `specs/F27-guns-ammunition-hardpoints-and-ballistic-hits.md`, stage
//! `### F27-D`, AC04. Task test prefix: `accept_f27_d_`.
//! Decision record:
//! `docs/findings/2026-10-03-f27-d-original-ammunition-and-loadout-audit.md`.
//!
//! The original lets a pilot choose every hardpoint's ammunition independently,
//! so one airframe really can carry several types — and nothing before this
//! stage could say, for a set of registered guns, *which types are in play* and
//! *who consumes their damage*. These tests drive the production
//! [`AmmunitionRegistry`] and pin both halves of that answer, including the two
//! cases a shortcut gets wrong: a type that delivers nothing, and two guns that
//! contradict each other about one type.
//!
//! Every value here is newly authored synthetic fixture data. No `CS_GAME_DIR`
//! access.

use cs_sim::damage::{ActorId, DamageChannel, DamageNodeKey};
use cs_sim::weapons::{
    AmmunitionId, AmmunitionRegistry, DAMAGE_CONSUMED_BY_ROUTER, Divergence, GunBank, GunCadence,
    GunDefinition, GunMountKind, GunRate, InheritanceRule, ORIGINAL_AMMUNITION_TYPES,
    ORIGINAL_GUN_GROUPS, ORIGINAL_GUN_SLOTS, ORIGINAL_SELECTABLE_GUNS, SpreadCone, WeaponDamage,
    WeaponState, covers_group, synthetic_ammunition, synthetic_effect, synthetic_gun_definition,
    synthetic_mount, synthetic_sound, uncovered_original_gun_groups,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::net::SessionId;

const PREFIX: &str = "accept_f27_d_";
const SESSION: u64 = 71;
const SESSION_ID: SessionId = match SessionId::new(SESSION) {
    Some(id) => id,
    None => panic!("the test session generation is valid"),
};

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: SESSION_ID,
        serial,
    }
}

fn node(key: &str) -> DamageNodeKey {
    DamageNodeKey::new(key).expect("a valid node key")
}

fn ammo(key: &str) -> AmmunitionId {
    AmmunitionId::try_new(
        ContentId::from_source(ContentKind::Ammo, key).expect("a valid content id"),
    )
    .expect("an ammo id")
}

/// One synthetic gun with its own declared profile.
fn gun(
    mount: &str,
    kind: GunMountKind,
    ammunition: &str,
    caliber: &str,
    armor: f64,
    internal: f64,
) -> GunDefinition {
    GunDefinition::try_new(
        node(mount),
        kind,
        caliber,
        ammo(ammunition),
        GunRate::try_new(4).expect("a positive rate"),
        640.0,
        90,
        SpreadCone::try_new(0.004).expect("a valid cone"),
        WeaponDamage::try_new(armor, internal).expect("a valid profile"),
        InheritanceRule::Full,
        synthetic_effect(),
        synthetic_sound(),
    )
    .expect("a valid synthetic gun definition")
}

/// The healthy case: every registered type maps to the production path that
/// consumes its damage, and the mounts firing it are named.
#[test]
fn accept_f27_d_every_type_in_play_maps_to_its_damage_consumer() {
    let mut registry = AmmunitionRegistry::new();
    assert!(registry.is_empty());
    assert_eq!(registry.len(), 0);

    registry
        .register(&gun(
            "mount_a",
            GunMountKind::Nose,
            "type_a",
            "caliber a",
            6.0,
            3.0,
        ))
        .expect("the first registration is accepted");
    registry
        .register(&gun(
            "mount_b",
            GunMountKind::WingLeft,
            "type_b",
            "caliber b",
            2.0,
            0.0,
        ))
        .expect("a second, different type is accepted");

    assert_eq!(registry.len(), 2, "two distinct types are in play");
    assert_eq!(
        registry.types(),
        vec![ammo("type_a"), ammo("type_b")],
        "the types are reported in ascending id order"
    );

    let consumer = registry
        .consumer(&ammo("type_a"))
        .expect("a registered type has a consumer");
    assert_eq!(consumer.ammunition(), &ammo("type_a"));
    assert_eq!(consumer.consumer(), DAMAGE_CONSUMED_BY_ROUTER);
    assert_eq!(consumer.amount(DamageChannel::Armor), Some(6.0));
    assert_eq!(consumer.amount(DamageChannel::Internal), Some(3.0));
    assert_eq!(consumer.delivering(), 2);
    assert!(consumer.is_consumed());
    assert_eq!(
        consumer.channels().collect::<Vec<_>>(),
        vec![(DamageChannel::Armor, 6.0), (DamageChannel::Internal, 3.0),],
        "the delivering channels come out in WEAPON_DAMAGE_CHANNELS order"
    );

    // The pairing is by mount, so a caller can tell which hardpoint fires what.
    assert_eq!(
        registry.mounts(&ammo("type_b")),
        [node("mount_b")],
        "the wing gun's type is fired by exactly the wing mount"
    );
    assert_eq!(registry.mounts(&ammo("type_a")), [node("mount_a")]);
    assert!(
        registry.mounts(&ammo("type_missing")).is_empty(),
        "a type nobody registered has no mounts"
    );
    assert!(
        registry.consumer(&ammo("type_missing")).is_none(),
        "and no consumer"
    );

    // And the aggregate audit rows the stage exists to produce.
    let audit = registry.audit();
    assert_eq!(audit.len(), 2);
    for (ammunition, consumer) in &audit {
        assert_eq!(consumer.ammunition(), ammunition);
        assert!(
            consumer.is_consumed(),
            "{ammunition} delivers damage on at least one channel, so it has a \
             consumer"
        );
    }
}

/// A type whose declared amounts are zero on every channel is **consumed by
/// nothing**: a round of it is spawned, costs a round, sounds and lands for
/// nothing. That must be visible, not reported as a working consumer.
#[test]
fn accept_f27_d_a_type_with_no_delivering_channel_is_consumed_by_nothing() {
    let mut registry = AmmunitionRegistry::new();
    registry
        .register(&gun(
            "mount_a",
            GunMountKind::Nose,
            "type_a",
            "caliber a",
            0.0,
            0.0,
        ))
        .expect("a zero profile is a legal declaration");

    let consumer = registry
        .consumer(&ammo("type_a"))
        .expect("the type is in play");
    assert!(!consumer.is_consumed());
    assert_eq!(consumer.delivering(), 0);
    assert_eq!(
        consumer.amount(DamageChannel::Armor),
        None,
        "a zero channel has no consumer, not a consumer of zero"
    );
    assert_eq!(consumer.channels().count(), 0);
    // The type is still reported — it is the caller that must notice it is inert.
    assert_eq!(
        registry.audit().len(),
        1,
        "an inert type is still in play and must still be reported"
    );
}

/// A profile that damages only the armor channel consumes that channel alone.
#[test]
fn accept_f27_d_a_one_channel_profile_consumes_only_that_channel() {
    let mut registry = AmmunitionRegistry::new();
    registry
        .register(&gun(
            "mount_a",
            GunMountKind::Nose,
            "type_a",
            "caliber a",
            5.0,
            0.0,
        ))
        .expect("a one-channel profile is accepted");
    let consumer = registry.consumer(&ammo("type_a")).expect("a consumer");
    assert!(consumer.is_consumed());
    assert_eq!(consumer.delivering(), 1);
    assert_eq!(consumer.amount(DamageChannel::Armor), Some(5.0));
    assert_eq!(consumer.amount(DamageChannel::Internal), None);
}

/// The same type on two mounts that **agree** is one type with two mounts, not a
/// contradiction. This is the original's own shape: every hardpoint of one
/// airframe may carry the same round.
#[test]
fn accept_f27_d_two_mounts_agreeing_about_one_type_are_one_row() {
    let mut registry = AmmunitionRegistry::new();
    registry
        .register(&gun(
            "mount_a",
            GunMountKind::Nose,
            "type_a",
            "caliber a",
            6.0,
            3.0,
        ))
        .expect("the first mount is accepted");
    registry
        .register(&gun(
            "mount_b",
            GunMountKind::WingRight,
            "type_a",
            "caliber a",
            6.0,
            3.0,
        ))
        .expect("an agreeing second mount is accepted");

    assert_eq!(registry.len(), 1, "one type, however many mounts fire it");
    assert_eq!(
        registry.mounts(&ammo("type_a")),
        [node("mount_a"), node("mount_b")],
        "both mounts are named, in ascending key order"
    );
    assert_eq!(registry.audit().len(), 1);
}

/// Two mounts that **contradict** each other about one type are refused by name.
/// One type cannot do two different things, and nothing in the data says which
/// gun is right — so the registry keeps the first and reports the second.
#[test]
fn accept_f27_d_two_mounts_disagreeing_about_one_type_are_refused_by_name() {
    let mut registry = AmmunitionRegistry::new();
    registry
        .register(&gun(
            "mount_a",
            GunMountKind::Nose,
            "type_a",
            "caliber a",
            6.0,
            3.0,
        ))
        .expect("the first mount is accepted");

    let error = registry
        .register(&gun(
            "mount_b",
            GunMountKind::WingLeft,
            "type_a",
            "caliber a",
            9.0,
            4.0,
        ))
        .expect_err("a second, different profile for one type is refused");
    let detail = error.divergence();
    assert_eq!(detail.ammunition(), &ammo("type_a"));
    assert_eq!(detail.kind(), Divergence::Damage);
    assert_eq!(detail.registered().mount(), &node("mount_a"));
    assert_eq!(detail.offered().mount(), &node("mount_b"));
    assert_eq!(detail.registered().damage().armor, 6.0);
    assert_eq!(detail.registered().damage().internal, 3.0);
    assert_eq!(detail.offered().damage().armor, 9.0);
    assert_eq!(detail.offered().damage().internal, 4.0);
    let text = error.to_string();
    assert!(
        text.contains("type_a"),
        "the refusal names the type: {text}"
    );
    assert!(text.contains("mount_a"), "and the registered mount: {text}");
    assert!(text.contains("mount_b"), "and the offered mount: {text}");

    // The refusal changes nothing: the first declaration still stands and the
    // second mount is not silently folded in.
    let consumer = registry
        .consumer(&ammo("type_a"))
        .expect("the type survives the refusal");
    assert_eq!(
        consumer.amount(DamageChannel::Armor),
        Some(6.0),
        "a refused registration must not overwrite the profile already in play"
    );
    assert_eq!(registry.mounts(&ammo("type_a")), [node("mount_a")]);
    assert_eq!(registry.len(), 1);
}

/// The caliber belongs to the round, so two mounts of one type with two calibers
/// are the same contradiction.
#[test]
fn accept_f27_d_two_mounts_disagreeing_about_one_type_s_caliber_are_refused() {
    let mut registry = AmmunitionRegistry::new();
    registry
        .register(&gun(
            "mount_a",
            GunMountKind::Nose,
            "type_a",
            "caliber a",
            6.0,
            3.0,
        ))
        .expect("the first mount is accepted");

    let error = registry
        .register(&gun(
            "mount_b",
            GunMountKind::WingLeft,
            "type_a",
            "caliber b",
            6.0,
            3.0,
        ))
        .expect_err("a second caliber for one type is refused");
    let detail = error.divergence();
    assert_eq!(detail.ammunition(), &ammo("type_a"));
    assert_eq!(
        detail.kind(),
        Divergence::Caliber,
        "the caliber contradiction is reported as itself, not as a damage one"
    );
    assert_eq!(detail.registered().caliber(), "caliber a");
    assert_eq!(detail.offered().caliber(), "caliber b");
    let text = error.to_string();
    assert!(text.contains("caliber a"), "named: {text}");
    assert!(text.contains("caliber b"), "named: {text}");
    assert_eq!(registry.len(), 1);
}

/// The registry is built from a **resolver's** registered guns, so a session with
/// no registrations audits to nothing and a session with two shooters' guns
/// audits to all of them.
#[test]
fn accept_f27_d_the_registry_is_built_from_a_resolver_s_registered_guns() {
    let mut cadence = GunCadence::new(SESSION, Tick(0));
    assert_eq!(
        cadence.resolver().shooters(),
        Vec::new(),
        "a fresh resolver registers nobody"
    );

    let nose = gun(
        "mount_a",
        GunMountKind::Nose,
        "type_a",
        "caliber a",
        6.0,
        3.0,
    );
    let wing = gun(
        "mount_b",
        GunMountKind::WingLeft,
        "type_b",
        "caliber b",
        2.0,
        1.0,
    );
    cadence
        .register(
            actor(1),
            vec![nose.clone(), wing.clone()],
            WeaponState::try_new(
                &[nose.clone(), wing.clone()],
                GunBank::try_new([node("mount_a"), node("mount_b")])
                    .expect("the declared bank names real mounts"),
                250,
            )
            .expect("the fixture state is valid"),
        )
        .expect("the first actor's guns register");
    cadence
        .register(
            actor(2),
            vec![gun(
                "mount_c",
                GunMountKind::Tail,
                "type_a",
                "caliber a",
                6.0,
                3.0,
            )],
            WeaponState::try_new(
                &[gun(
                    "mount_c",
                    GunMountKind::Tail,
                    "type_a",
                    "caliber a",
                    6.0,
                    3.0,
                )],
                GunBank::try_new([node("mount_c")]).expect("a valid bank"),
                250,
            )
            .expect("the fixture state is valid"),
        )
        .expect("the second actor's guns register");

    let mut registry = AmmunitionRegistry::new();
    for shooter in cadence.resolver().shooters() {
        for definition in cadence.resolver().definitions(&shooter) {
            registry
                .register(definition)
                .expect("both actors agree about type_a");
        }
    }
    assert_eq!(registry.len(), 2, "two types across two shooters");
    assert_eq!(
        registry.mounts(&ammo("type_a")),
        [node("mount_a"), node("mount_c")],
        "one type fired by mounts of two different actors is one type with two mounts"
    );
    assert_eq!(registry.mounts(&ammo("type_b")), [node("mount_b")]);
}

/// The measured original vocabulary, as the runtime side of the same facts
/// `cs_content::weapons` mirrors: twenty gun groups of which the designed mount
/// kinds cover nine, and four ammunition types.
///
/// These are the numbers an audit closes a declared catalogue against; the
/// retail test of this prefix re-measures them from the installation, so a
/// mismatch here cannot pass unnoticed.
#[test]
fn accept_f27_d_the_measured_original_surface_is_the_audit_closure_target() {
    assert_eq!(ORIGINAL_GUN_GROUPS.len(), 20);
    assert_eq!(ORIGINAL_GUN_GROUPS[0].id(), 3061);
    assert_eq!(ORIGINAL_GUN_GROUPS[19].id(), 3080);
    assert_eq!(ORIGINAL_AMMUNITION_TYPES, 4);
    assert_eq!(ORIGINAL_SELECTABLE_GUNS, 5);
    assert_eq!(ORIGINAL_GUN_SLOTS, 4);

    let uncovered = uncovered_original_gun_groups();
    assert_eq!(uncovered.len(), 11);
    assert!(
        uncovered.iter().all(|group| !GunMountKind::ALL
            .iter()
            .any(|kind| covers_group(*kind, group.id()))),
        "every uncovered group really is uncovered"
    );
    assert_eq!(
        ORIGINAL_GUN_GROUPS
            .iter()
            .filter(|group| GunMountKind::ALL
                .iter()
                .any(|kind| covers_group(*kind, group.id())))
            .count(),
        9,
        "nine of the twenty are covered by a designed kind"
    );
    assert!(
        ORIGINAL_GUN_GROUPS.iter().any(|group| {
            group.label() == "REARTURRET" && covers_group(GunMountKind::Tail, group.id())
        }),
        "the designed tail kind is anchored by the original's own rear-turret group"
    );
}

/// The synthetic fixture gun is *not* original data, and keeping the two
/// distinguishable is part of the contract: a stage that quietly reused the
/// fixture as evidence for the measured constants would make them
/// indistinguishable.
#[test]
fn accept_f27_d_the_fixture_gun_is_never_presented_as_original_data() {
    let fixture = synthetic_gun_definition();
    assert_eq!(fixture.ammunition(), &synthetic_ammunition());
    assert_eq!(fixture.mount(), &synthetic_mount());
    assert!(
        fixture
            .ammunition()
            .as_str()
            .contains(cs_sim::weapons::SYNTHETIC_AMMO_KEY),
        "the fixture ammunition key is the synthetic namespace, never an original \
         identifier: {}",
        fixture.ammunition()
    );
    assert!(
        !ORIGINAL_GUN_GROUPS
            .iter()
            .any(|group| group.label() == synthetic_gun_definition().caliber()),
        "a fixture caliber string is not one of the original's measured group labels"
    );
    assert_eq!(synthetic_effect(), cs_sim::weapons::synthetic_effect());
    assert_eq!(synthetic_sound(), cs_sim::weapons::synthetic_sound());
}

/// Every test in this file carries the task prefix.
#[test]
fn accept_f27_d_this_file_only_declares_the_task_test_prefix() {
    assert_eq!(PREFIX, "accept_f27_d_");
    for name in [
        "accept_f27_d_every_type_in_play_maps_to_its_damage_consumer",
        "accept_f27_d_a_type_with_no_delivering_channel_is_consumed_by_nothing",
        "accept_f27_d_a_one_channel_profile_consumes_only_that_channel",
        "accept_f27_d_two_mounts_agreeing_about_one_type_are_one_row",
        "accept_f27_d_two_mounts_disagreeing_about_one_type_are_refused_by_name",
        "accept_f27_d_two_mounts_disagreeing_about_one_type_s_caliber_are_refused",
        "accept_f27_d_the_registry_is_built_from_a_resolver_s_registered_guns",
        "accept_f27_d_the_measured_original_surface_is_the_audit_closure_target",
        "accept_f27_d_the_fixture_gun_is_never_presented_as_original_data",
        "accept_f27_d_this_file_only_declares_the_task_test_prefix",
    ] {
        assert!(name.starts_with(PREFIX), "{name} is outside the prefix");
    }
}
