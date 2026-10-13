//! Acceptance tests T-IDENTITY-ORDNANCE (#601): ordnance session and
//! instance identity **are** the shared `cs_types::net` `SessionId` /
//! `ActorId` (the F28 gap, closed exactly as #397 closed animation, #442
//! damage and #496 audio).
//!
//! Before this migration `cs_sim::weapons::ordnance` carried its own session
//! identity as a bare `u64` and `StatusEffectInstanceId`/`AreaInstanceId`
//! were two more `{ session: u64, serial: u64 }` structs — field for field
//! the contract's `ActorId { session, serial }`. Both names are now aliases
//! of the shared contract type, and every per-session ledger and runtime —
//! guidance set, status ledger, area ledger, nitro ledger, ordnance runtime —
//! is bound to the shared nonzero `SessionId` at its constructor, so a
//! "session zero" sentinel is unrepresentable while a foreign generation is
//! still refused by name.
//!
//! These tests are discriminating: if the ordnance-scoped structs are
//! reinstated, the `shared_actor(...)` / `let through_shared: ActorId = ...`
//! bindings and the `fn(SessionId)` boundary calls do not compile; if the
//! confinement rule or the field mapping changes, the equality assertions
//! fail. Every value is newly authored synthetic fixture data.

use std::collections::BTreeSet;

use cs_sim::damage::ActorId;
use cs_sim::time::TickRate;
use cs_sim::weapons::ordnance::{
    AreaInstanceId, ArmingRule, EquipmentRules, GuidanceRule, GuidanceSet, GuidanceTracker,
    GuidanceUpdate, LostTargetBehavior, LostTargetReason, NitroError, NitroLedger, OrdnanceFamily,
    OrdnanceRuntime, OrdnanceRuntimeError, ProjectileOrdnance, SYNTHETIC_AREA_LIFETIME_TICKS,
    SYNTHETIC_DIRECT_KEY, StatusEffectError, StatusEffectInstanceId, StatusEffectLedger,
    StatusEffectTarget, synthetic_area_effect, synthetic_channels, synthetic_choke,
    synthetic_guided_rocket, synthetic_launch_geometry, synthetic_media,
    synthetic_nitro_parameters, synthetic_ordnance, synthetic_stack_load,
};
use cs_sim::weapons::{MountTransform, ProjectileId};
use cs_types::Tick;
use cs_types::net::{ActorId as SharedActorId, SessionId};
use cs_types::space::{UnitVec3, WorldPosition};

const SESSION: u64 = 11;
const TICKS_PER_SECOND: u32 = 30;

fn session(value: u64) -> SessionId {
    SessionId::new(value).expect("a nonzero session generation")
}

/// Names the shared type: passing a [`StatusEffectInstanceId`] to this
/// function is what proves the alias is the shared id and not a look-alike
/// struct.
fn shared_actor(id: SharedActorId) -> SharedActorId {
    id
}

/// Names the shared type: passing the runtime's session to this function is
/// what proves the boundary carries the shared nonzero `SessionId`.
fn shared_session(id: SessionId) -> SessionId {
    id
}

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: session(SESSION),
        serial,
    }
}

fn projectile(serial: u64) -> ProjectileId {
    ProjectileId {
        session: SESSION,
        serial,
    }
}

fn tick(value: u64) -> Tick {
    Tick(value)
}

fn rate() -> TickRate {
    TickRate::new(TICKS_PER_SECOND).expect("the test rate is non-zero")
}

fn position(value: [f64; 3]) -> WorldPosition {
    WorldPosition::try_new(value).expect("test positions are finite")
}

fn mount() -> MountTransform {
    MountTransform::try_new(
        position([0.0; 3]),
        UnitVec3::try_new([0.0, 0.0, -1.0]).expect("unit vector"),
        [0.0; 3],
    )
    .expect("the fixture mount is valid")
}

fn runtime() -> OrdnanceRuntime {
    OrdnanceRuntime::new(session(SESSION), tick(0), rate(), 7)
}

/// The guided-with-area component the area-instance test drives: a targeted
/// item with the declared `Detonate` lost-target behavior and the synthetic
/// bounded area, so one guidance loss produces a real
/// [`cs_sim::weapons::ordnance::GuidanceDetonation`] the production
/// [`OrdnanceRuntime::open_detonation_area`] consumes.
fn guided_with_area() -> ProjectileOrdnance {
    ProjectileOrdnance::try_new(
        synthetic_ordnance("synthetic.t601_guided_area"),
        OrdnanceFamily::GuidedRocket,
        synthetic_launch_geometry(),
        synthetic_stack_load(),
        ArmingRule::AfterTicks(1),
        cs_sim::weapons::ordnance::FuseRule::Timed { ticks: 30 },
        GuidanceRule::Targeted {
            lost_target: LostTargetBehavior::Detonate,
        },
        // The item must outlive the declared area it leaves behind.
        SYNTHETIC_AREA_LIFETIME_TICKS,
        Some(synthetic_area_effect()),
        synthetic_channels(),
        Vec::new(),
        synthetic_media(),
        EquipmentRules::new(None, BTreeSet::new()),
    )
    .expect("the fixture component is valid")
}

/// A status effect's instance id **is** the shared `ActorId`: a real ledger
/// application returns the shared id, it binds to `ActorId` field for field,
/// and it passes through a `fn(ActorId)` boundary.
#[test]
fn accept_t601_status_effect_instance_id_is_the_shared_actor_id() {
    let mut ledger = StatusEffectLedger::new(session(SESSION), tick(0));
    let source = synthetic_ordnance(SYNTHETIC_DIRECT_KEY);
    let instance: StatusEffectInstanceId = ledger
        .apply(
            session(SESSION),
            tick(0),
            StatusEffectTarget::whole_actor(actor(3)),
            &source,
            &synthetic_choke(),
        )
        .expect("the ledger is this session's");

    let through_shared: SharedActorId = instance;
    assert_eq!(
        through_shared,
        SharedActorId {
            session: session(SESSION),
            serial: 0,
        },
        "an ordnance effect id is the shared ActorId {{ session, serial }} shape"
    );
    assert_eq!(
        shared_actor(instance).session,
        session(SESSION),
        "an ordnance effect id passes through a fn(ActorId) boundary"
    );
    let effect = ledger.get(&instance).expect("the effect is live");
    assert_eq!(
        effect.instance, instance,
        "the ledger stores and reports the same shared id"
    );

    // The shared type is what an ordnance-scoped literal denotes, so a
    // reinstated ordnance struct would not compile here.
    let literal: StatusEffectInstanceId = SharedActorId {
        session: session(SESSION),
        serial: 9,
    };
    assert_eq!(shared_actor(literal).serial, 9);
}

/// A triggered area's instance id **is** the shared `ActorId`: a real
/// production area opening returns the shared id, which keys the area
/// ledger.
#[test]
fn accept_t601_area_instance_id_is_the_shared_actor_id() {
    let mut runtime = runtime();
    let target = actor(9);
    runtime
        .launch(
            actor(1),
            projectile(1),
            &guided_with_area(),
            &mount(),
            Some(target),
            [0.0; 3],
        )
        .expect("the fixture launches");
    let lost = runtime.guidance_tick(
        session(SESSION),
        cs_sim::weapons::ordnance::TargetObservation::destroyed(target, tick(1)),
    );
    let detonation = lost.detonated.first().expect("the item detonates");

    let instance: AreaInstanceId = runtime
        .open_detonation_area(session(SESSION), tick(1), detonation, &[])
        .expect("the runtime opens the declared area");

    let through_shared: SharedActorId = instance;
    assert_eq!(through_shared.session, session(SESSION));
    assert_eq!(
        shared_actor(instance).serial,
        0,
        "the first area of a session is serial 0, as a shared ActorId"
    );
    assert!(
        runtime.areas().get(&instance).is_some(),
        "the shared id keys the area ledger"
    );
}

/// The session boundary is the shared nonzero `SessionId` everywhere: the
/// runtime, its guidance set, its ledgers and every input boundary report
/// and compare that type, and a foreign generation is still refused **by
/// name** with shared `SessionId` payloads — a session-zero sentinel simply
/// cannot be built.
#[test]
fn accept_t601_session_boundary_is_the_shared_nonzero_session_id() {
    assert_eq!(
        SessionId::new(0),
        None,
        "the zero sentinel cannot be a session"
    );

    let mut runtime = runtime();
    assert_eq!(
        shared_session(runtime.session()),
        session(SESSION),
        "the runtime's session is the shared SessionId"
    );
    assert_eq!(runtime.guidance().session(), session(SESSION));
    assert_eq!(runtime.status().session(), session(SESSION));
    assert_eq!(runtime.areas().session(), session(SESSION));

    let nitro_shooter = actor(1);
    runtime
        .register_nitro(nitro_shooter, synthetic_nitro_parameters())
        .expect("the booster registers");
    assert_eq!(
        runtime.nitro(&nitro_shooter).map(NitroLedger::session),
        Some(session(SESSION)),
        "the nitro ledger is confined to the shared SessionId"
    );

    // A foreign generation is still refused by name, and the refusal carries
    // the shared `SessionId`s — never a bare `u64` alias.
    let foreign = session(SESSION + 1);
    let mut ledger = StatusEffectLedger::new(session(SESSION), tick(0));
    let source = synthetic_ordnance(SYNTHETIC_DIRECT_KEY);
    assert_eq!(
        ledger.apply(
            foreign,
            tick(0),
            StatusEffectTarget::whole_actor(actor(3)),
            &source,
            &synthetic_choke(),
        ),
        Err(StatusEffectError::ForeignSession {
            expected: session(SESSION),
            found: foreign,
        }),
        "the status ledger refuses a foreign generation with SessionIds"
    );
}

/// The guidance confinement rule survived the type change: an observation
/// stamped with another shared `SessionId` loses every target as
/// `ForeignSession` carrying the two shared ids.
#[test]
fn accept_t601_guidance_confinement_survives_the_session_id_migration() {
    let definition = synthetic_guided_rocket();
    let target = actor(9);
    let mut set = GuidanceSet::new(session(SESSION));
    set.insert(GuidanceTracker::new(
        session(SESSION),
        projectile(4),
        definition.guidance(),
        Some(target),
    ));

    let foreign = session(SESSION + 1);
    let updates = set.session_tick(
        foreign,
        cs_sim::weapons::ordnance::TargetObservation::alive(target, tick(6)),
    );
    assert!(
        matches!(
            updates.get(&projectile(4)).copied(),
            Some(GuidanceUpdate::Lost {
                reason: LostTargetReason::ForeignSession { expected, found },
                ..
            }) if expected == session(SESSION) && found == foreign
        ),
        "a foreign generation is a lost target reported with the shared SessionIds"
    );

    // And the runtime refuses the same way, by name.
    let mut runtime = runtime();
    assert_eq!(
        runtime.advance_status(foreign, tick(1)),
        Err(OrdnanceRuntimeError::Status(
            StatusEffectError::ForeignSession {
                expected: session(SESSION),
                found: foreign,
            }
        )),
    );
    assert_eq!(
        runtime.route_trigger(
            foreign,
            tick(1),
            &projectile(1),
            actor(2),
            cs_sim::damage::DamageNodeKey::new("node").expect("a valid node"),
        ),
        Err(OrdnanceRuntimeError::ForeignSession {
            expected: SESSION,
            found: SESSION + 1,
        }),
        "the runtime-level refusal still names foreign_session and reports the generation"
    );
}

/// Nitro confinement is the shared `SessionId` too, refused by name.
#[test]
fn accept_t601_nitro_confinement_is_the_shared_session_id() {
    let mut ledger = NitroLedger::new(
        session(SESSION),
        tick(0),
        rate(),
        synthetic_nitro_parameters(),
    );
    assert_eq!(ledger.session(), session(SESSION));
    let foreign = session(SESSION + 1);
    assert_eq!(
        ledger.request(foreign, tick(1), true),
        Err(NitroError::ForeignSession {
            expected: session(SESSION),
            found: foreign,
        }),
        "a foreign generation is refused with the shared SessionIds"
    );
}
