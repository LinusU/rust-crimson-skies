//! Acceptance scenarios F27-C-FX-SCHED: the session's fixed loop drives the
//! weapon-effect presentation consumer (task #1304).
//!
//! Spec: `specs/F27-guns-ammunition-hardpoints-and-ballistic-hits.md`,
//! non-negotiable 5 ("consumption, sound and muzzle effects derive from
//! accepted fire events"). Task test prefix: `accept_f27_c_fx_` (this file's
//! own selection: `accept_f27_c_fx_sched_`). The design records:
//! `docs/findings/2026-10-11-f27-c-fx-weapon-effect-presentation.md` (the
//! consumer) and
//! `docs/findings/2026-10-18-f27-c-fx-sched-scheduled-presentation.md` (this
//! stage).
//!
//! These tests drive production code only: a real
//! [`cs_app::physics::PhysicsSession`] built through `production_builder` —
//! the composition a running session uses, which installs
//! [`cs_app::weapons::WeaponSchedulePlugin`] and, beside it,
//! [`cs_app::weapon_effects::WeaponEffectSchedulePlugin`]. **No test here calls
//! `present_weapon_effects`**: the scheduled pass is its production caller, and
//! every observation is read from
//! [`cs_app::weapon_effects::WeaponEffectScheduleReport`], the spawned
//! entities and the real router ledger.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data. No `CS_GAME_DIR` access.

use avian3d::prelude::LinearVelocity;
use bevy::prelude::{ChildOf, Entity, GlobalTransform, Transform, Vec3, World};
use cs_app::audio::{AudioSession, AudioSpatial};
use cs_app::physics::{PhysicsSession, PhysicsTickLedger};
use cs_app::scene::{NodeVisualTransform, SceneGeneration, SceneGenerations};
use cs_app::targeting::{TargetingSession, lower_rules};
use cs_app::weapon_effects::{
    MuzzleEffectSpawn, SpatialPlacement, WEAPON_SHOT_PRODUCER, WeaponEffectRefusal,
    WeaponEffectReport, WeaponEffectScheduleMissing, WeaponEffectScheduleReport,
};
use cs_app::weapons::{
    MountPoseBinding, SessionDamageResolver, WeaponActorBinding, WeaponOrder, WeaponScheduleReport,
    WeaponSession, WeaponTeardownRecord, WeaponTickInput, WeaponTickInputs,
};
use cs_content::target_rules::declared_synthetic_target_rules;
use cs_content::weapons::{
    DeclaredGunDefinition, DeclaredGunMountKind, DeclaredGunRate, SYNTHETIC_LIFETIME_TICKS,
    SYNTHETIC_TICKS_BETWEEN_SHOTS, declared_synthetic_gun, synthetic_effect_id, synthetic_sound_id,
};
use cs_sim::audio_events::{
    AudioAssetSpec, AudioBus, AudioEventId, AudioRouter, Listener, OneShotEvent, OneShotOutcome,
    PlaybackMode, SpatialMix, SpatialPolicy,
};
use cs_sim::damage::{ActorId, DamageNodeKey, DamageResolver};
use cs_sim::targeting::{SelectionBinding, TargetClass, TargetRecord};
use cs_sim::weapons::{FireIntent, FireIntentId, GunBank, SYNTHETIC_STARTING_ROUNDS};
use cs_types::Tick;
use cs_types::content::{Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::net::SessionId;
use cs_types::space::WorldPosition;

/// The session generation the fixture opens.
const SESSION: u64 = 41;
/// The weapon session's router producer serial.
const ROUTER_PRODUCER: u32 = 91;
/// The damage resolver's producer serial.
const DAMAGE_PRODUCER: u32 = 92;
/// The scene generation every live entity in these tests is stamped under.
const GENERATION: SceneGeneration = SceneGeneration(1);
/// The fixed rate the schedule runs at: 64 Hz is exactly representable, so one
/// pumped tick commits exactly one fixed tick.
const FIXED_HZ: u32 = 64;
/// The synthetic fixture's own mount, used here as the nose gun.
const NOSE_MOUNT: &str = "gun_mount_1";
/// The muzzle's z, the depth these tests fire from.
const MUZZLE_Z: f32 = 10.0;
/// The fixture shot-sound spec's designed gain, asserted against the cues.
const SHOT_GAIN: f64 = 1.0;

fn session_id(session: u64) -> SessionId {
    SessionId::new(session).expect("the test session generation is nonzero")
}

/// One session's actor serial `serial`.
fn actor_in(session: u64, serial: u64) -> ActorId {
    ActorId {
        session: session_id(session),
        serial,
    }
}

/// The default session's actor serial `serial`.
fn actor(serial: u64) -> ActorId {
    actor_in(SESSION, serial)
}

fn key(name: &str) -> DamageNodeKey {
    DamageNodeKey::new(name).expect("test node keys are valid")
}

fn claim() -> ClaimId {
    ClaimId::new("f27c.fx-sched-test").expect("a valid claim id")
}

fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(value, Provenance::designed(claim())))
}

/// The declared fixture gun moved onto `mount` and declared as a `kind` gun,
/// with the fixture's own cadence, lifetime, effect and sound.
fn declared_on(mount: &str, kind: DeclaredGunMountKind) -> DeclaredGunDefinition {
    let fixture = declared_synthetic_gun();
    DeclaredGunDefinition::try_new(
        fixture.gun().clone(),
        Origin::SyntheticFixture,
        cs_content::damage::DamageNodeKey::new(mount).expect("a valid mount key"),
        kind,
        fixture.scene_binding().cloned(),
        fixture.caliber().clone(),
        fixture.ammunition().clone(),
        known(DeclaredGunRate {
            ticks_between_shots: SYNTHETIC_TICKS_BETWEEN_SHOTS,
        }),
        fixture.muzzle_velocity_mps().clone(),
        known(SYNTHETIC_LIFETIME_TICKS),
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

/// The shooter's live hierarchy: an actor root carrying the binding and the
/// airframe velocity, with one mount node per mount at the muzzle depth these
/// tests fire from.
fn shooter_world(world: &mut World, shooter: ActorId, mounts: &[&str]) -> Entity {
    let fixture = declared_synthetic_gun();
    let root = world
        .spawn((
            WeaponActorBinding {
                actor: shooter,
                guns: vec![fixture.gun().clone()],
                loadout: fixture.gun().clone(),
                generation: GENERATION,
            },
            LinearVelocity(Vec3::ZERO),
        ))
        .id();
    for (index, mount) in mounts.iter().enumerate() {
        let x = 2.0 * index as f32;
        world.spawn((
            MountPoseBinding {
                mount: key(mount),
                generation: GENERATION,
            },
            NodeVisualTransform(GlobalTransform::from_xyz(x, 0.0, MUZZLE_Z)),
            ChildOf(root),
        ));
    }
    root
}

/// A weapon session with the fixture's nose gun mounted and selected, held by
/// `shooter`.
fn session_with_one_gun(session: u64, shooter: ActorId) -> WeaponSession {
    let nose = declared_on(NOSE_MOUNT, DeclaredGunMountKind::Nose);
    let mut weapons =
        WeaponSession::new(session, Tick(0), ROUTER_PRODUCER).expect("a nonzero generation opens");
    weapons
        .register(
            shooter,
            &[nose],
            GunBank::try_new([key(NOSE_MOUNT)]).expect("a valid bank"),
            SYNTHETIC_STARTING_ROUNDS,
        )
        .expect("the declared gun registers");
    weapons
}

/// The session's damage authority; these tests never assert damage, so only the
/// resource the schedule requires is built.
fn damage_resolver(session: u64) -> DamageResolver {
    DamageResolver::new(session_id(session), DAMAGE_PRODUCER)
}

/// The session's declared allegiance authority: the synthetic target rules'
/// own table, with the shooter rostered. The schedule requires the table (and
/// its generation); these tests fire into empty space, so no relation is read.
fn targeting_session(session: u64) -> TargetingSession {
    let declared = declared_synthetic_target_rules();
    let lowered = lower_rules(&declared).expect("the fixture rules lower");
    let mut targeting = TargetingSession::new(
        session_id(session),
        lowered,
        SelectionBinding::new(),
        declared.subject().clone(),
        GENERATION,
    );
    targeting
        .store_mut()
        .register(TargetRecord {
            actor: actor_in(session, 1),
            faction: cs_sim::targeting::synthetic_player_faction(),
            class: TargetClass::Aircraft,
            objective: false,
            revealed: true,
            phase_eligible: true,
            position: WorldPosition::try_new([0.0, 0.0, 0.0])
                .expect("the fixture position is finite"),
        })
        .expect("the fixture actor registers");
    targeting
}

/// The listener and attenuation policy a spatialised cue is placed against:
/// the listener at the world origin, right along +x, full audible range.
fn spatial() -> AudioSpatial {
    AudioSpatial::new(
        SpatialPolicy::try_new(1.0, 10_000.0).expect("a valid policy"),
        Listener::try_new([0.0; 3], [1.0, 0.0, 0.0]).expect("a valid listener"),
    )
}

/// The installed audio session these tests present through: the fixture's
/// declared shot sound lowered as a one-shot spec on the weapons bus, in a
/// router of `session`.
fn audio_session(session: u64) -> AudioSession {
    let spec = AudioAssetSpec::try_new(
        synthetic_sound_id(),
        AudioBus::Weapons,
        SHOT_GAIN,
        PlaybackMode::OneShot,
    )
    .expect("the fixture shot sound is a valid one-shot spec");
    AudioSession::new(
        AudioRouter::new(session_id(session)),
        GENERATION,
        vec![spec],
    )
}

/// The production fixed-tick session — `production_builder` composes the
/// animation plugin, the F27-C weapon schedule and (through it) the
/// presentation schedule the same way a running session does.
fn schedule_session() -> PhysicsSession {
    PhysicsSession::production_builder()
        .fixed_hz(FIXED_HZ)
        .build()
}

/// The session's world, for the spawns and resources the typed API does not
/// cover.
fn world(session: &mut PhysicsSession) -> &mut World {
    session.world_mut().expect("the session is active")
}

/// Advances the session's world by `ticks` committed fixed ticks through
/// `PhysicsSession::step`, the same call a session driver pumps.
fn step(session: &mut PhysicsSession, ticks: u64) {
    session.step(ticks).expect("the session steps");
}

/// Installs one generation's session resources, the way a session's open or
/// restart does: the weapon session, its authorities, the scene generation,
/// and — when the world has one — the audio session and spatial configuration.
fn install_session(
    session: &mut PhysicsSession,
    weapons: WeaponSession,
    damage: DamageResolver,
    targeting: TargetingSession,
    audio: Option<AudioSession>,
    spatial: Option<AudioSpatial>,
) {
    let world = world(session);
    world.insert_resource(weapons);
    world.insert_resource(SessionDamageResolver::new(damage));
    world.insert_resource(targeting);
    if let Some(audio) = audio {
        world.insert_resource(audio);
    }
    if let Some(spatial) = spatial {
        world.insert_resource(spatial);
    }
    let mut generations = SceneGenerations::default();
    while generations.latest() < GENERATION {
        generations.take_next();
    }
    world.insert_resource(generations);
}

/// Publishes one tick's record the way the session's driver does.
fn publish(session: &mut PhysicsSession, input: WeaponTickInput) {
    world(session)
        .resource_mut::<WeaponTickInputs>()
        .push(input);
}

/// One tick's record for `at` in `session`, carrying `orders` and no cockpit.
fn tick_input(session: u64, at: Tick, orders: Vec<WeaponOrder>) -> WeaponTickInput {
    WeaponTickInput {
        session,
        at,
        wind_velocity_m_s: Some([0.0; 3]),
        orders,
        cockpit: None,
    }
}

/// One fire intent on `tick` of `session`, stamped the way the session's own
/// producers stamp theirs: the actor's serial as the producer.
fn intent(session: u64, tick: u64, sequence: u32) -> FireIntent {
    FireIntent {
        id: FireIntentId {
            session,
            tick: Tick(tick),
            producer: 1,
            sequence,
        },
        shooter: actor_in(session, 1),
    }
}

/// The presentation schedule's report after the last run.
fn presented(session: &PhysicsSession) -> WeaponEffectScheduleReport {
    session
        .world()
        .expect("the session is active")
        .resource::<WeaponEffectScheduleReport>()
        .clone()
}

/// The weapon schedule's report after the last run.
fn weapon_report(session: &PhysicsSession) -> WeaponScheduleReport {
    session
        .world()
        .expect("the session is active")
        .resource::<WeaponScheduleReport>()
        .clone()
}

/// The committed tick the world is sitting on.
fn committed(session: &PhysicsSession) -> Tick {
    Tick(
        session
            .world()
            .expect("the session is active")
            .resource::<PhysicsTickLedger>()
            .ticks,
    )
}

/// Every spawned muzzle effect's record, in entity order.
fn muzzle_spawns(session: &mut PhysicsSession) -> Vec<MuzzleEffectSpawn> {
    world(session)
        .iter_entities()
        .filter_map(|entity_ref| entity_ref.get::<MuzzleEffectSpawn>().cloned())
        .collect()
}

/// The weapon session's pending effect log length, and the count it has
/// recorded over its life.
fn effect_counts(session: &mut PhysicsSession) -> (usize, u64) {
    let weapons = world(session).resource::<WeaponSession>();
    (weapons.effects().len(), weapons.effects_recorded())
}

/// Probe the router's one-shot ledger with `(WEAPON_SHOT_PRODUCER, sequence)`
/// in `session`'s installed router.
///
/// The router answers `Accepted` only for an id it has never accepted, so the
/// probe is what proves whether the scheduled consumer really played that
/// sequence.
fn probe(session: &mut PhysicsSession, session_generation: u64, sequence: u32) -> OneShotOutcome {
    let event = OneShotEvent::try_new(
        AudioEventId {
            session: session_id(session_generation),
            tick: Tick(0),
            producer: WEAPON_SHOT_PRODUCER,
            sequence,
        },
        synthetic_sound_id(),
        AudioBus::Weapons,
        SHOT_GAIN,
    )
    .expect("a valid probe event");
    world(session)
        .resource_mut::<AudioSession>()
        .router
        .play_one_shot(&event)
}

/// The accepted minimum, through the production loop: one committed tick with
/// one accepted fire input presents the shot's declared sound exactly once
/// (the router ledger proves it) and spawns exactly one muzzle record carrying
/// the gun's declared effect id at the emitted muzzle position — in the same
/// tick the step accepted the shot.
#[test]
fn accept_f27_c_fx_sched_a_committed_tick_presents_its_accepted_shot_once() {
    let mut session = schedule_session();
    shooter_world(world(&mut session), actor(1), &[NOSE_MOUNT]);
    install_session(
        &mut session,
        session_with_one_gun(SESSION, actor(1)),
        damage_resolver(SESSION),
        targeting_session(SESSION),
        Some(audio_session(SESSION)),
        Some(spatial()),
    );

    publish(
        &mut session,
        tick_input(
            SESSION,
            Tick(1),
            vec![WeaponOrder::Fire(intent(SESSION, 1, 0))],
        ),
    );
    step(&mut session, 1);

    // The step ran on the committed tick and emitted one effect.
    let weapons = weapon_report(&session);
    let stepped = weapons.stepped.as_ref().expect("the schedule stepped");
    assert_eq!(
        stepped.at,
        Tick(1),
        "the committed tick stepped: {stepped:?}"
    );
    assert_eq!(
        stepped.accepted.len(),
        1,
        "the shot was accepted: {stepped:?}"
    );
    assert_eq!(stepped.effects.len(), 1, "the shot emitted one effect");
    let emitted = &stepped.effects[0];

    // The presentation pass ran for the same tick, after the step.
    let report = presented(&session);
    assert_eq!(report.ticks, 1, "the schedule ran the committed tick");
    assert_eq!(
        report.at,
        Some(Tick(1)),
        "the committed tick is the ledger's"
    );
    assert_eq!(report.session, Some(SESSION));
    assert!(report.missing.is_empty(), "nothing was missing: {report:?}");
    assert_eq!(report.released, 0, "nothing was torn down");
    let pass = report.presented.expect("the pass ran");
    assert!(pass.refusals.is_empty(), "nothing is refused: {pass:?}");
    assert_eq!(pass.played.len(), 1, "one accepted shot, one cue");
    let cue = &pass.played[0];
    assert_eq!(
        cue.event.id,
        AudioEventId {
            session: session_id(SESSION),
            tick: Tick(1),
            producer: WEAPON_SHOT_PRODUCER,
            sequence: 0,
        },
        "the cue is stamped with this consumer's producer and the accepted tick"
    );
    assert_eq!(
        cue.event.asset,
        synthetic_sound_id(),
        "the cue plays the gun's declared shot-sound id, not a hardcoded clip"
    );
    assert_eq!(
        cue.spatial,
        SpatialPlacement::Placed(SpatialMix {
            gain: 0.1,
            pan: 0.0,
        }),
        "the muzzle sits 10 m ahead of the listener: inverse-distance gain, centred pan"
    );
    assert_eq!(
        cue.origin, emitted.origin,
        "the cue carries the emitted muzzle position"
    );

    // Exactly one muzzle spawn, carrying the declared effect id and pose.
    assert_eq!(pass.spawned.len(), 1, "one accepted shot, one muzzle spawn");
    let spawns = muzzle_spawns(&mut session);
    assert_eq!(spawns.len(), 1, "exactly one muzzle entity exists");
    let spawn = &spawns[0];
    assert_eq!(
        spawn.effect,
        synthetic_effect_id(),
        "the spawn carries the gun's declared muzzle-effect id"
    );
    assert_eq!(spawn.projectile, emitted.projectile, "the shot's round");
    assert_eq!(spawn.shooter, actor(1), "the firing actor");
    assert_eq!(spawn.mount, key(NOSE_MOUNT), "the firing mount");
    assert_eq!(spawn.at, Tick(1), "the accepted tick");
    assert_eq!(spawn.generation, GENERATION, "the live scene generation");
    let pose = world(&mut session)
        .get::<Transform>(pass.spawned[0])
        .expect("the spawn is placed");
    assert_eq!(
        pose.translation,
        Vec3::new(0.0, 0.0, MUZZLE_Z),
        "the pose is the emitted muzzle position"
    );

    // The router really accepted the cue, exactly once.
    assert!(
        matches!(
            probe(&mut session, SESSION, 0),
            OneShotOutcome::SuppressedDuplicate { .. }
        ),
        "the router really accepted sequence 0, so the shot sounded exactly once"
    );
    assert!(
        matches!(
            probe(&mut session, SESSION, 1),
            OneShotOutcome::Accepted { .. }
        ),
        "sequence 1 was never played"
    );

    // The drain is the pass's only source, and it happened in this tick: the
    // pending log is empty while the recorded count still names the shot.
    assert_eq!(
        effect_counts(&mut session),
        (0, 1),
        "the log was drained once for the tick, and its recorded count survives"
    );
    assert_eq!(committed(&session), Tick(1));
}

/// A tick with no published record presents nothing — twice over — and no
/// effect is ever presented again: the first shot's cue stays played once, its
/// muzzle record stays one, and the recorded count does not double.
#[test]
fn accept_f27_c_fx_sched_an_inputless_tick_presents_nothing() {
    let mut session = schedule_session();
    shooter_world(world(&mut session), actor(1), &[NOSE_MOUNT]);
    install_session(
        &mut session,
        session_with_one_gun(SESSION, actor(1)),
        damage_resolver(SESSION),
        targeting_session(SESSION),
        Some(audio_session(SESSION)),
        Some(spatial()),
    );

    publish(
        &mut session,
        tick_input(
            SESSION,
            Tick(1),
            vec![WeaponOrder::Fire(intent(SESSION, 1, 0))],
        ),
    );
    step(&mut session, 1);
    assert_eq!(
        presented(&session)
            .presented
            .expect("the pass ran")
            .played
            .len(),
        1,
        "the accepted shot presented"
    );

    // Two more committed ticks with nothing published: the schedule runs and
    // presents nothing on either.
    step(&mut session, 1);
    let first_idle = presented(&session);
    assert_eq!(first_idle.at, Some(Tick(2)));
    assert_eq!(
        first_idle.presented,
        Some(WeaponEffectReport::default()),
        "an inputless tick presents nothing: no cue, no spawn, no refusal"
    );
    step(&mut session, 1);
    let second_idle = presented(&session);
    assert_eq!(
        second_idle.ticks, 3,
        "the schedule ran every committed tick"
    );
    assert_eq!(second_idle.at, Some(Tick(3)));
    assert_eq!(second_idle.released, 0);
    assert!(second_idle.missing.is_empty());
    assert_eq!(
        second_idle.presented,
        Some(WeaponEffectReport::default()),
        "the second inputless tick presents nothing either"
    );
    assert_eq!(
        muzzle_spawns(&mut session).len(),
        1,
        "the inputless ticks spawned no second muzzle effect"
    );
    assert_eq!(
        effect_counts(&mut session),
        (0, 1),
        "the inputless ticks drained nothing and emitted nothing"
    );

    // The cadence is served again four ticks after the first shot: the second
    // accepted shot presents once more, and the recorded count is the count of
    // accepted shots — not the pending log, which the pass drained.
    publish(
        &mut session,
        tick_input(
            SESSION,
            Tick(5),
            vec![WeaponOrder::Fire(intent(SESSION, 5, 1))],
        ),
    );
    step(&mut session, 2);
    let fired = presented(&session);
    assert_eq!(fired.at, Some(Tick(5)));
    let pass = fired.presented.expect("the pass ran");
    assert_eq!(pass.played.len(), 1, "the second shot presented its cue");
    assert_eq!(
        pass.played[0].event.id.sequence, 1,
        "the consumer's sequence advanced, so the second shot is its own event"
    );
    assert_eq!(
        effect_counts(&mut session),
        (0, 2),
        "two accepted shots were recorded and both were drained"
    );
    assert_eq!(muzzle_spawns(&mut session).len(), 2);
    assert!(
        matches!(
            probe(&mut session, SESSION, 2),
            OneShotOutcome::Accepted { .. }
        ),
        "the second shot sounded once, and no more"
    );
}

/// Each accepted shot presents on its own committed tick, once, with its own
/// record: a shot fired on tick 1 and another on tick 5 leave exactly two
/// muzzle records, stamped with their own ticks, and the ticks between them
/// present nothing — never a late batch and never a repeat.
#[test]
fn accept_f27_c_fx_sched_each_shot_presents_on_its_own_tick() {
    let mut session = schedule_session();
    shooter_world(world(&mut session), actor(1), &[NOSE_MOUNT]);
    install_session(
        &mut session,
        session_with_one_gun(SESSION, actor(1)),
        damage_resolver(SESSION),
        targeting_session(SESSION),
        Some(audio_session(SESSION)),
        Some(spatial()),
    );

    publish(
        &mut session,
        tick_input(
            SESSION,
            Tick(1),
            vec![WeaponOrder::Fire(intent(SESSION, 1, 0))],
        ),
    );
    step(&mut session, 1);
    assert_eq!(presented(&session).at, Some(Tick(1)));
    assert_eq!(muzzle_spawns(&mut session).len(), 1);

    for tick in 2..=4 {
        step(&mut session, 1);
        let idle = presented(&session);
        assert_eq!(
            idle.at,
            Some(Tick(tick)),
            "the schedule served committed tick {tick}"
        );
        assert_eq!(
            idle.presented,
            Some(WeaponEffectReport::default()),
            "tick {tick} presented nothing"
        );
        assert_eq!(
            muzzle_spawns(&mut session).len(),
            1,
            "tick {tick} added no muzzle record"
        );
    }

    publish(
        &mut session,
        tick_input(
            SESSION,
            Tick(5),
            vec![WeaponOrder::Fire(intent(SESSION, 5, 1))],
        ),
    );
    step(&mut session, 1);
    let fired = presented(&session);
    assert_eq!(fired.at, Some(Tick(5)));
    assert_eq!(fired.presented.expect("the pass ran").played.len(), 1);

    let mut second_shot_spawns = muzzle_spawns(&mut session);
    second_shot_spawns.sort_by_key(|spawn| spawn.at.0);
    assert_eq!(
        second_shot_spawns
            .iter()
            .map(|spawn| spawn.at)
            .collect::<Vec<Tick>>(),
        vec![Tick(1), Tick(5)],
        "each accepted shot's muzzle record names its own tick"
    );
    assert!(
        second_shot_spawns
            .iter()
            .all(|spawn| spawn.effect == synthetic_effect_id()),
        "every record carries the declared effect id"
    );
    assert_eq!(effect_counts(&mut session), (0, 2));
}

/// The muzzle spawns stay coherent with the weapon session's own teardown: on
/// the tick the weapon schedule reports the superseded generation torn down,
/// the dead generation's muzzle records go with it — and the new generation's
/// own accepted shot still presents in that same tick, so the release never
/// eats the live half. A removed session releases the same way.
#[test]
fn accept_f27_c_fx_sched_a_session_swap_releases_the_dead_muzzle_spawns() {
    let mut session = schedule_session();
    shooter_world(world(&mut session), actor(1), &[NOSE_MOUNT]);
    install_session(
        &mut session,
        session_with_one_gun(SESSION, actor(1)),
        damage_resolver(SESSION),
        targeting_session(SESSION),
        Some(audio_session(SESSION)),
        Some(spatial()),
    );
    publish(
        &mut session,
        tick_input(
            SESSION,
            Tick(1),
            vec![WeaponOrder::Fire(intent(SESSION, 1, 0))],
        ),
    );
    step(&mut session, 1);
    assert_eq!(
        muzzle_spawns(&mut session).len(),
        1,
        "the first shot's flash"
    );

    // The restart: a new generation's session, authorities, shooter hierarchy
    // and audio session replace the old ones.
    let restarted = SESSION + 1;
    shooter_world(world(&mut session), actor_in(restarted, 1), &[NOSE_MOUNT]);
    world(&mut session).insert_resource(session_with_one_gun(restarted, actor_in(restarted, 1)));
    world(&mut session).insert_resource(SessionDamageResolver::new(damage_resolver(restarted)));
    world(&mut session).insert_resource(targeting_session(restarted));
    world(&mut session).insert_resource(audio_session(restarted));
    publish(
        &mut session,
        tick_input(
            restarted,
            Tick(2),
            vec![WeaponOrder::Fire(intent(restarted, 2, 0))],
        ),
    );
    step(&mut session, 1);

    let weapons = weapon_report(&session);
    assert_eq!(
        weapons.tore_down,
        Some(WeaponTeardownRecord {
            session: SESSION,
            mirrors: 1,
            inputs: 0,
        }),
        "the weapon schedule names the superseded generation: {weapons:?}"
    );
    let report = presented(&session);
    assert_eq!(
        report.released, 1,
        "the dead generation's one muzzle record was released"
    );
    assert_eq!(report.session, Some(restarted), "the new generation serves");
    let pass = report
        .presented
        .expect("the pass ran for the new generation");
    assert_eq!(pass.played.len(), 1, "the new generation's shot presented");
    assert_eq!(
        pass.spawned.len(),
        1,
        "and its muzzle record spawned in the same tick"
    );
    let spawns = muzzle_spawns(&mut session);
    assert_eq!(
        spawns.len(),
        1,
        "no stale muzzle record survives the swap: {spawns:?}"
    );
    assert_eq!(
        spawns[0].at,
        Tick(2),
        "the surviving record is the new shot's"
    );
    assert_eq!(spawns[0].shooter, actor_in(restarted, 1));
    assert!(
        matches!(
            probe(&mut session, restarted, 1),
            OneShotOutcome::SuppressedDuplicate { .. }
        ),
        "the new generation's cue sounded once"
    );

    // A removed session releases the same way, and the tick presents nothing.
    world(&mut session).remove_resource::<WeaponSession>();
    step(&mut session, 1);
    let gone = presented(&session);
    assert_eq!(gone.session, None, "no session is installed");
    assert_eq!(gone.released, 1, "the removed generation's record went too");
    assert!(gone.presented.is_none(), "there is nothing to present");
    assert!(
        muzzle_spawns(&mut session).is_empty(),
        "no muzzle record outlives its session"
    );
}

/// A world with no audio session still presents the visual half: the log is
/// drained once, the muzzle record spawns, and the sound is refused by name —
/// a missing audio session is never a reason to erase the flash, or to leave
/// the effect waiting for a later tick.
#[test]
fn accept_f27_c_fx_sched_a_world_without_audio_still_presents_the_muzzle_half() {
    let mut session = schedule_session();
    shooter_world(world(&mut session), actor(1), &[NOSE_MOUNT]);
    install_session(
        &mut session,
        session_with_one_gun(SESSION, actor(1)),
        damage_resolver(SESSION),
        targeting_session(SESSION),
        None,
        Some(spatial()),
    );

    publish(
        &mut session,
        tick_input(
            SESSION,
            Tick(1),
            vec![WeaponOrder::Fire(intent(SESSION, 1, 0))],
        ),
    );
    step(&mut session, 1);

    let stepped = weapon_report(&session);
    let emitted = stepped
        .stepped
        .as_ref()
        .expect("the schedule stepped")
        .effects[0]
        .clone();
    let pass = presented(&session).presented.expect("the pass ran");
    assert!(
        pass.played.is_empty(),
        "nothing can play without an audio session: {pass:?}"
    );
    assert_eq!(
        pass.refusals,
        vec![WeaponEffectRefusal::NoAudioSession {
            projectile: emitted.projectile,
        }],
        "the audio half is refused by name"
    );
    assert_eq!(pass.spawned.len(), 1, "the muzzle half still presents");
    assert_eq!(muzzle_spawns(&mut session).len(), 1);
    assert_eq!(
        effect_counts(&mut session),
        (0, 1),
        "the one drain still happened, so nothing waits for a later tick"
    );
}

/// A schedule with the weapon session installed but no live scene generation
/// presents nothing and says so — never spawning under a guessed stamp.
#[test]
fn accept_f27_c_fx_sched_a_missing_generation_presents_nothing() {
    let mut session = schedule_session();
    shooter_world(world(&mut session), actor(1), &[NOSE_MOUNT]);
    install_session(
        &mut session,
        session_with_one_gun(SESSION, actor(1)),
        damage_resolver(SESSION),
        targeting_session(SESSION),
        Some(audio_session(SESSION)),
        Some(spatial()),
    );
    publish(
        &mut session,
        tick_input(
            SESSION,
            Tick(1),
            vec![WeaponOrder::Fire(intent(SESSION, 1, 0))],
        ),
    );
    world(&mut session).remove_resource::<SceneGenerations>();

    step(&mut session, 1);

    let report = presented(&session);
    assert_eq!(report.session, Some(SESSION));
    assert_eq!(
        report.missing,
        vec![WeaponEffectScheduleMissing::Generation],
        "the absent generation is named: {report:?}"
    );
    assert!(report.presented.is_none(), "nothing was presented");
    assert!(muzzle_spawns(&mut session).is_empty());
    // The weapon step named the same absence, so nothing was emitted either:
    // no effect waits behind the unserved tick.
    assert!(weapon_report(&session).stepped.is_none());
    assert_eq!(effect_counts(&mut session), (0, 0));
}
