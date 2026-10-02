//! Acceptance scenario F27-D, application half: a weapon session audits its own
//! ammunition/loadout, mapping every type it can fire to its damage consumer.
//!
//! Spec: `specs/F27-guns-ammunition-hardpoints-and-ballistic-hits.md`, stage
//! `### F27-D`, AC04. Task test prefix: `accept_f27_d_`. Decision record:
//! `docs/findings/2026-10-03-f27-d-original-ammunition-and-loadout-audit.md`.
//!
//! AC04's "maps every type to its behavior and damage consumer" has two halves:
//! the declared catalogue (F27-A's schema, audited by
//! [`cs_content::weapons::AmmunitionAudit`]) and the **runtime** — what a
//! running session can actually fire. These tests drive production
//! [`cs_app::weapons::session_ammunition_audit`] over a real
//! [`WeaponSession`] whose guns were registered through the real lowering
//! boundary, and pin the cases a shortcut gets wrong: two mounts that disagree
//! about one ammunition type, a type that delivers nothing, and a closed
//! session that has no ammunition left to audit.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data. No `CS_GAME_DIR` access.

use bevy::prelude::World;
use cs_app::weapons::{SessionRefusal, WeaponSession, session_ammunition_audit};
use cs_content::weapons::{
    DeclaredCaliber, DeclaredGunDefinition, DeclaredGunMountKind, DeclaredGunRate,
    DeclaredInheritanceRule, DeclaredSelfHitRule, DeclaredSpreadCone, DeclaredWeaponDamage,
    InteractionRules, SYNTHETIC_LIFETIME_TICKS, SYNTHETIC_MUZZLE_VELOCITY_MPS,
    SYNTHETIC_SPREAD_HALF_ANGLE_RAD, SYNTHETIC_TICKS_BETWEEN_SHOTS, synthetic_effect_id,
    synthetic_sound_id,
};
use cs_sim::damage::{ActorId, DamageChannel, DamageNodeKey};
use cs_sim::weapons::{DAMAGE_CONSUMED_BY_ROUTER, Divergence, GunBank, SYNTHETIC_STARTING_ROUNDS};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::net::SessionId;
use cs_types::space::Radians;

const PREFIX: &str = "accept_f27_d_";
const SESSION: u64 = 53;
const ROUTER_PRODUCER: u32 = 93;

const NOSE_MOUNT: &str = "gun_mount_1";
const WING_MOUNT: &str = "wing_mount_1";

fn session_id() -> SessionId {
    SessionId::new(SESSION).expect("the test session generation is nonzero")
}

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: session_id(),
        serial,
    }
}

fn claim() -> ClaimId {
    ClaimId::new("f27d.session-ammo-audit-test").expect("a valid claim id")
}

fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(value, Provenance::designed(claim())))
}

/// The runtime ammunition id: the identity the registry and the session audit
/// are keyed by.
fn ammo_id(key: &str) -> cs_sim::weapons::AmmunitionId {
    cs_sim::weapons::AmmunitionId::try_new(
        ContentId::from_source(ContentKind::Ammo, key).expect("a valid content id"),
    )
    .expect("an ammo id")
}

/// The declared-record ammunition id: the *content* half of the same identity,
/// which the lowering boundary converts.
fn declared_ammunition_id(key: &str) -> cs_content::weapons::AmmunitionId {
    cs_content::weapons::AmmunitionId::try_new(
        ContentId::from_source(ContentKind::Ammo, key).expect("a valid content id"),
    )
    .expect("an ammo id")
}

fn rules() -> InteractionRules {
    InteractionRules {
        self_hit: known(DeclaredSelfHitRule::Allowed),
        ..cs_content::weapons::synthetic_interaction_rules()
    }
}

fn profile(armor: f64, internal: f64) -> DeclaredWeaponDamage {
    DeclaredWeaponDamage {
        armor: known(armor),
        internal: known(internal),
    }
}

/// One declared gun, mounted on `mount` as `kind`, firing `ammunition_key` with
/// its own declared damage profile.
///
/// The lowering boundary turns the profile into the runtime's `WeaponDamage`
/// this session's router emits, so the audit's damage consumer is *the declared
/// amount* and not a restatement of it.
fn declared_gun(
    key: &str,
    mount: &str,
    kind: DeclaredGunMountKind,
    ammunition_key: &str,
    caliber_text: &str,
    damage: DeclaredWeaponDamage,
) -> DeclaredGunDefinition {
    DeclaredGunDefinition::try_new(
        ContentId::from_source(ContentKind::Weapon, key).expect("a valid content id"),
        Origin::SyntheticFixture,
        cs_content::damage::DamageNodeKey::new(mount).expect("a valid mount key"),
        kind,
        None,
        known(DeclaredCaliber::try_new(caliber_text).expect("a valid caliber")),
        known(declared_ammunition_id(ammunition_key)),
        known(DeclaredGunRate {
            ticks_between_shots: SYNTHETIC_TICKS_BETWEEN_SHOTS,
        }),
        known(SYNTHETIC_MUZZLE_VELOCITY_MPS),
        known(SYNTHETIC_LIFETIME_TICKS),
        DeclaredSpreadCone {
            half_angle: known(Radians(SYNTHETIC_SPREAD_HALF_ANGLE_RAD)),
        },
        damage,
        known(DeclaredInheritanceRule::Full),
        known(synthetic_effect_id()),
        known(synthetic_sound_id()),
        rules(),
        Provenance::designed(claim()),
    )
    .expect("a valid declared gun")
}

fn session() -> WeaponSession {
    WeaponSession::new(SESSION, Tick(0), ROUTER_PRODUCER).expect("a nonzero session generation")
}

fn bank(mounts: &[&str]) -> GunBank {
    GunBank::try_new(
        mounts
            .iter()
            .map(|mount| DamageNodeKey::new(mount).expect("a valid mount key")),
    )
    .expect("the bank names real mounts")
}

/// The healthy case: a session holding two mounts with two ammunition types
/// audits to one row per type, each naming the production path that consumes
/// its damage and the mount that fires it.
#[test]
fn accept_f27_d_a_session_audits_every_type_it_can_fire() {
    let mut session = session();
    session
        .register(
            actor(1),
            &[
                declared_gun(
                    "gun_nose",
                    NOSE_MOUNT,
                    DeclaredGunMountKind::Nose,
                    "type_a",
                    "caliber a",
                    profile(6.0, 3.0),
                ),
                declared_gun(
                    "gun_wing",
                    WING_MOUNT,
                    DeclaredGunMountKind::WingLeft,
                    "type_b",
                    "caliber b",
                    profile(2.0, 0.0),
                ),
            ],
            bank(&[NOSE_MOUNT, WING_MOUNT]),
            SYNTHETIC_STARTING_ROUNDS,
        )
        .expect("both declared guns lower and register");

    let audit = session_ammunition_audit(&session);
    assert_eq!(
        audit.rows().len(),
        2,
        "one row per ammunition type the session can fire"
    );
    assert!(audit.refused().is_empty());
    assert!(audit.is_complete());
    assert!(audit.unconsumed().is_empty());

    let nose = audit
        .row(&ammo_id("type_a"))
        .expect("the nose gun's type has a row");
    assert_eq!(
        nose.consumer().consumer(),
        DAMAGE_CONSUMED_BY_ROUTER,
        "the row names the production path that applies the declared amounts"
    );
    assert_eq!(nose.consumer().amount(DamageChannel::Armor), Some(6.0));
    assert_eq!(nose.consumer().amount(DamageChannel::Internal), Some(3.0));
    assert_eq!(
        nose.mounts(),
        [DamageNodeKey::new(NOSE_MOUNT).expect("a valid mount key")],
        "and the mount that fires it"
    );

    let wing = audit
        .row(&ammo_id("type_b"))
        .expect("the wing gun's type has a row");
    assert_eq!(wing.consumer().amount(DamageChannel::Armor), Some(2.0));
    assert_eq!(
        wing.consumer().amount(DamageChannel::Internal),
        None,
        "a declared zero damages no internal structure and therefore has no \
         consumer on that channel"
    );
    assert!(wing.consumer().is_consumed());
}

/// Two mounts of one airframe carrying the **same** type — the original's own
/// shape, since every hardpoint chooses its ammunition independently — is one
/// type with two mounts, not two types and not a contradiction.
#[test]
fn accept_f27_d_two_mounts_of_one_type_audit_to_one_row() {
    let mut session = session();
    session
        .register(
            actor(1),
            &[
                declared_gun(
                    "gun_nose",
                    NOSE_MOUNT,
                    DeclaredGunMountKind::Nose,
                    "type_a",
                    "caliber a",
                    profile(6.0, 3.0),
                ),
                declared_gun(
                    "gun_wing",
                    WING_MOUNT,
                    DeclaredGunMountKind::WingLeft,
                    "type_a",
                    "caliber a",
                    profile(6.0, 3.0),
                ),
            ],
            bank(&[NOSE_MOUNT, WING_MOUNT]),
            SYNTHETIC_STARTING_ROUNDS,
        )
        .expect("both guns lower and register");

    let audit = session_ammunition_audit(&session);
    assert_eq!(audit.rows().len(), 1, "one type, two mounts");
    assert!(audit.refused().is_empty());
    assert!(audit.is_complete());
    assert_eq!(
        audit.row(&ammo_id("type_a")).expect("a row").mounts().len(),
        2,
        "both mounts are named"
    );
}

/// Two mounts that **contradict** each other about one type are reported by the
/// session audit. Registration itself succeeds — nothing about a loadout is
/// refused, because the session has no authority over which declaration is
/// right — but the audit must not let the contradiction pass silently.
#[test]
fn accept_f27_d_two_mounts_disagreeing_about_one_type_are_reported() {
    let mut session = session();
    session
        .register(
            actor(1),
            &[
                declared_gun(
                    "gun_nose",
                    NOSE_MOUNT,
                    DeclaredGunMountKind::Nose,
                    "type_a",
                    "caliber a",
                    profile(6.0, 3.0),
                ),
                declared_gun(
                    "gun_wing",
                    WING_MOUNT,
                    DeclaredGunMountKind::WingLeft,
                    "type_a",
                    "caliber a",
                    profile(9.0, 4.0),
                ),
            ],
            bank(&[NOSE_MOUNT, WING_MOUNT]),
            SYNTHETIC_STARTING_ROUNDS,
        )
        .expect("both guns lower and register: a contradiction is a data fact");

    let audit = session_ammunition_audit(&session);
    assert_eq!(
        audit.refused().len(),
        1,
        "the contradiction between the two mounts is reported"
    );
    let detail = audit.refused()[0].divergence();
    assert_eq!(detail.ammunition(), &ammo_id("type_a"));
    assert_eq!(detail.kind(), Divergence::Damage);
    assert_eq!(detail.offered().damage().armor, 9.0);
    assert!(
        !audit.is_complete(),
        "a session carrying a contradiction is not a complete loadout"
    );
    // And the first registration still stands: reporting is not repairing.
    let row = audit
        .row(&ammo_id("type_a"))
        .expect("the type still has a row");
    assert_eq!(
        row.consumer().amount(DamageChannel::Armor),
        Some(6.0),
        "the audit reports the contradiction without picking a winner"
    );
    assert_eq!(row.mounts().len(), 1);
}

/// A type whose declared amounts are zero everywhere is reported as consumed by
/// nothing: a round of it costs a round, sounds and lands for nothing.
#[test]
fn accept_f27_d_a_session_type_that_delivers_nothing_is_named() {
    let mut session = session();
    session
        .register(
            actor(1),
            &[declared_gun(
                "gun_nose",
                NOSE_MOUNT,
                DeclaredGunMountKind::Nose,
                "type_inert",
                "caliber a",
                profile(0.0, 0.0),
            )],
            bank(&[NOSE_MOUNT]),
            SYNTHETIC_STARTING_ROUNDS,
        )
        .expect("an inert gun still lowers and registers");

    let audit = session_ammunition_audit(&session);
    assert!(audit.refused().is_empty());
    assert_eq!(audit.rows().len(), 1, "the type is still in play");
    assert_eq!(audit.unconsumed(), vec![&ammo_id("type_inert")]);
    assert!(
        !audit.is_complete(),
        "a session that can fire a round which damages nothing is not complete"
    );
}

/// A session with no registered guns audits to nothing, and a **closed** one has
/// no ammunition left at all: teardown released the cadence and with it every
/// registered gun.
#[test]
fn accept_f27_d_an_empty_or_closed_session_audits_to_nothing() {
    let mut empty = session();
    let audit = session_ammunition_audit(&empty);
    assert!(audit.rows().is_empty());
    assert!(audit.refused().is_empty());
    assert!(
        audit.is_complete(),
        "vacuously: no type can be missing a consumer"
    );

    empty
        .register(
            actor(1),
            &[declared_gun(
                "gun_nose",
                NOSE_MOUNT,
                DeclaredGunMountKind::Nose,
                "type_a",
                "caliber a",
                profile(6.0, 3.0),
            )],
            bank(&[NOSE_MOUNT]),
            SYNTHETIC_STARTING_ROUNDS,
        )
        .expect("the gun registers");
    assert_eq!(
        session_ammunition_audit(&empty).rows().len(),
        1,
        "a registered gun is in play"
    );

    empty.close(&mut World::new());
    let closed = session_ammunition_audit(&empty);
    assert!(
        closed.rows().is_empty(),
        "a closed session holds no ammunition to audit"
    );
    assert!(closed.is_complete(), "and nothing can be missing from it");
}

/// Two shooters in one session share the audit: the original lets every
/// aircraft choose its own loadout, so the session's ammunition set is the union
/// over its registered actors, not one airframe's.
#[test]
fn accept_f27_d_the_session_audit_spans_every_registered_actor() {
    let mut session = session();
    session
        .register(
            actor(1),
            &[declared_gun(
                "gun_nose",
                NOSE_MOUNT,
                DeclaredGunMountKind::Nose,
                "type_a",
                "caliber a",
                profile(6.0, 3.0),
            )],
            bank(&[NOSE_MOUNT]),
            SYNTHETIC_STARTING_ROUNDS,
        )
        .expect("the first actor registers");
    session
        .register(
            actor(2),
            &[declared_gun(
                "gun_wing",
                WING_MOUNT,
                DeclaredGunMountKind::WingLeft,
                "type_a",
                "caliber a",
                profile(6.0, 3.0),
            )],
            bank(&[WING_MOUNT]),
            SYNTHETIC_STARTING_ROUNDS,
        )
        .expect("the second actor registers");

    let audit = session_ammunition_audit(&session);
    assert_eq!(
        audit.rows().len(),
        1,
        "two actors agreeing about one type is one type"
    );
    assert!(audit.refused().is_empty(), "and it is not a contradiction");
    assert_eq!(
        audit.row(&ammo_id("type_a")).expect("a row").mounts().len(),
        2,
        "both actors' mounts are named"
    );
}

/// The audit reads the session it is given and holds no state of its own, so a
/// second call answers the same thing unless the session changed.
#[test]
fn accept_f27_d_the_session_audit_is_a_pure_read() {
    let mut session = session();
    session
        .register(
            actor(1),
            &[declared_gun(
                "gun_nose",
                NOSE_MOUNT,
                DeclaredGunMountKind::Nose,
                "type_a",
                "caliber a",
                profile(6.0, 3.0),
            )],
            bank(&[NOSE_MOUNT]),
            SYNTHETIC_STARTING_ROUNDS,
        )
        .expect("the gun registers");

    let first = session_ammunition_audit(&session);
    let second = session_ammunition_audit(&session);
    assert_eq!(first, second, "reading twice answers the same thing");

    // It also leaves the session alone: firing still works afterwards.
    assert!(!session.is_closed());
    assert!(
        session.state(&actor(1)).is_some(),
        "the audit is not a mutator"
    );
}

/// Every test in this file carries the task prefix.
#[test]
fn accept_f27_d_this_file_only_declares_the_task_test_prefix() {
    assert_eq!(PREFIX, "accept_f27_d_");
    for name in [
        "accept_f27_d_a_session_audits_every_type_it_can_fire",
        "accept_f27_d_two_mounts_of_one_type_audit_to_one_row",
        "accept_f27_d_two_mounts_disagreeing_about_one_type_are_reported",
        "accept_f27_d_a_session_type_that_delivers_nothing_is_named",
        "accept_f27_d_an_empty_or_closed_session_audits_to_nothing",
        "accept_f27_d_the_session_audit_spans_every_registered_actor",
        "accept_f27_d_the_session_audit_is_a_pure_read",
        "accept_f27_d_this_file_only_declares_the_task_test_prefix",
    ] {
        assert!(name.starts_with(PREFIX), "{name} is outside the prefix");
    }
    assert!(matches!(
        WeaponSession::new(0, Tick(0), ROUTER_PRODUCER)
            .expect_err("session generation zero is refused"),
        SessionRefusal::NoSession
    ));
}
