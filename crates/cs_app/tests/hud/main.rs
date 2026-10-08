//! Acceptance stage F46: instrument values and display-unit policy (F46-A),
//! the HUD frame over the session authorities (F46-B) and the in-flight page
//! session (F46-C).
//!
//! `specs/F46-hud-instruments-mission-map-and-pause.md`, sections
//! `### F46-A`, `### F46-B` and `### F46-C`. F46-A's minimum scenario is
//! AC01's shape: a known attitude quaternion produces the expected horizon
//! and heading. F46-B's is AC02's: a bank that fires its last round reads
//! `empty` on the very next frame while the selection — the authority's own
//! state — is untouched. F46-C's is the aircraft swap: a `HudSession` rebinds
//! and every instrument, gauge and page describes the new aircraft only. The
//! rest covers the unit policy, the low-altitude hysteresis, the map's
//! authored-geography and revealed-only contacts, the mode-aware pause and
//! teardown/retry rebinding. Everything is authored synthetic data through
//! the real `WeaponSession`, `OrdnanceSession`, `DamageResolver`,
//! `TargetConsumers`, `WorldDefinition`/`WorldInstance`, `ObjectiveDisplay`
//! and `TargetStore`; no original unit, datum, gauge layout, map composition
//! or target semantic is read, so this proves the projection only, never
//! original display behavior (F46-D).

use cs_app::ordnance::OrdnanceSession;
use cs_app::ui::hud::AircraftSample;
use cs_app::weapons::WeaponSession;
use cs_content::ordnance::DeclaredOrdnance;
use cs_content::weapons::{DeclaredGunDefinition, DeclaredGunMountKind, declared_synthetic_gun};
use cs_sim::damage::{AttributionRule, DamagePolicy, DamageResolver, synthetic_airframe_graph};
use cs_sim::time::TickRate;
use cs_sim::weapons::GunBank;
use cs_types::Tick;
use cs_types::content::{DamageNodeKey, Origin, Provenance};
use cs_types::evidence::ClaimId;
use cs_types::net::{ActorId, SessionId};
use cs_types::space::{Quaternion, Radians, UnitVec3};

mod attitude;
mod binding;
mod gauges;
mod session;
mod targets;
mod units;

pub const EPS: f64 = 1e-9;
/// The fixture's nose and wing mount keys.
pub const NOSE_MOUNT: &str = "nose_mount";
pub const WING_MOUNT: &str = "wing_mount";
/// The producer serials the fixture sessions stamp their records with.
pub const ROUTER_PRODUCER: u32 = 3;
pub const DAMAGE_PRODUCER: u32 = 4;
pub const ORDNANCE_PRODUCER: u32 = 5;

pub fn session(n: u64) -> SessionId {
    SessionId::new(n).expect("session")
}

pub fn actor(s: u64, serial: u64) -> ActorId {
    ActorId {
        session: session(s),
        serial,
    }
}

pub fn key(name: &str) -> DamageNodeKey {
    DamageNodeKey::new(name).expect("test node keys are valid")
}

/// `angle` radians about `axis` (canonical axes, right-hand rule).
pub fn rot(axis: [f64; 3], angle: f64) -> Quaternion {
    Quaternion::from_axis_angle(UnitVec3::try_new(axis).expect("axis"), Radians(angle))
        .expect("rotation")
}

/// A level, still-air sample for `actor(s, serial)`.
pub fn level_sample(s: u64, serial: u64) -> AircraftSample {
    AircraftSample {
        session: session(s),
        actor: actor(s, serial),
        attitude: Quaternion::IDENTITY,
        velocity_mps: [0.0, 0.0, -50.0],
        wind_mps: [0.0; 3],
        height_m: 500.0,
        ground_height_m: Some(0.0),
    }
}

fn claim() -> ClaimId {
    ClaimId::new("f46b.hud-frame-test").expect("a valid claim id")
}

/// The declared fixture gun moved onto `mount` and declared as `kind`.
pub fn declared_on(mount: &str, kind: DeclaredGunMountKind) -> DeclaredGunDefinition {
    let fixture = declared_synthetic_gun();
    DeclaredGunDefinition::try_new(
        fixture.gun().clone(),
        Origin::SyntheticFixture,
        DamageNodeKey::new(mount).expect("a valid mount key"),
        kind,
        fixture.scene_binding().cloned(),
        fixture.caliber().clone(),
        fixture.ammunition().clone(),
        fixture.rate().clone(),
        fixture.muzzle_velocity_mps().clone(),
        fixture.lifetime_ticks().clone(),
        fixture.spread().clone(),
        fixture.damage().clone(),
        fixture.inheritance().clone(),
        fixture.effect().clone(),
        fixture.sound().clone(),
        fixture.rules().clone(),
        Provenance::designed(claim()),
    )
    .expect("a valid declared gun")
}

/// A real `WeaponSession` for session `s` with the actor's declared guns on
/// `mounts` and the `bank` mounts selected, `rounds` on each mount.
pub fn weapon_session(
    s: u64,
    serial: u64,
    mounts: &[(&str, DeclaredGunMountKind)],
    bank: &[&str],
    rounds: u64,
) -> WeaponSession {
    let declared: Vec<DeclaredGunDefinition> = mounts
        .iter()
        .map(|(mount, kind)| declared_on(mount, *kind))
        .collect();
    let mut session =
        WeaponSession::new(s, Tick(0), ROUTER_PRODUCER).expect("a nonzero generation opens");
    session
        .register(
            actor(s, serial),
            &declared,
            GunBank::try_new(bank.iter().map(|mount| key(mount))).expect("a valid bank"),
            rounds,
        )
        .expect("the declared guns register");
    session
}

/// A real `DamageResolver` for session `s` with the actor's synthetic
/// airframe graph registered.
pub fn damage_for(s: u64, serial: u64) -> DamageResolver {
    let mut damage = DamageResolver::new(session(s), DAMAGE_PRODUCER);
    damage
        .register_actor(
            actor(s, serial),
            synthetic_airframe_graph(),
            DamagePolicy {
                attribution: AttributionRule::FirstLethalHit,
            },
        )
        .expect("the fixture graph registers");
    damage
}

/// A real `OrdnanceSession` for session `s` with `declared` registered for
/// the actor.
pub fn ordnance_for(s: u64, serial: u64, declared: &[DeclaredOrdnance]) -> OrdnanceSession {
    let mut session = OrdnanceSession::new(
        s,
        Tick(0),
        TickRate::new(60).expect("a nonzero tick rate"),
        ORDNANCE_PRODUCER,
    )
    .expect("a nonzero generation opens");
    session
        .register(actor(s, serial), declared)
        .expect("the declared ordnance registers");
    session
}
