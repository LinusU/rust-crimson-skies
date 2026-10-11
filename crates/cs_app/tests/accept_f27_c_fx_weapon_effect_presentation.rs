//! Acceptance scenarios F27-C-FX: the presentation consumer that sounds and
//! draws the weapon session's emitted effects.
//!
//! Spec: `specs/F27-guns-ammunition-hardpoints-and-ballistic-hits.md`,
//! non-negotiable 5 ("consumption, sound and muzzle effects derive from
//! accepted fire events"). Task test prefix: `accept_f27_c_fx_`. The design
//! record: `docs/findings/2026-10-11-f27-c-fx-weapon-effect-presentation.md`.
//!
//! These tests drive production code only: the real per-tick
//! [`step_weapon_session`] emits the effects, and
//! [`cs_app::weapon_effects`]'s [`present_weapon_effects`],
//! [`trigger_shot_sound`], [`spawn_muzzle_effect`] and
//! [`release_muzzle_effects`] consume them over the real
//! [`cs_sim::audio_events::AudioRouter`]. Every sound and effect id is the
//! synthetic fixture's own declared record; nothing here is original game data
//! and no `CS_GAME_DIR` access happens.
//!
//! The discriminating property is the single drain: an accepted shot presents
//! exactly one cue and one muzzle spawn, a denied input and a duplicate packet
//! present nothing on either side, and a drained batch can never be presented
//! twice.

use avian3d::prelude::LinearVelocity;
use bevy::prelude::{ChildOf, GlobalTransform, Transform, Vec3, World};
use cs_app::audio::{AudioSession, AudioSpatial};
use cs_app::scene::{NodeVisualTransform, SceneGeneration};
use cs_app::weapon_effects::{
    MuzzleEffectSpawn, SpatialPlacement, WEAPON_SHOT_PRODUCER, WeaponEffectRefusal,
    WeaponEffectReport, WeaponShotSequences, present_weapon_effects, release_muzzle_effects,
    spawn_muzzle_effect,
};
use cs_app::weapons::{
    WeaponActorBinding, WeaponEffect, WeaponOrder, WeaponSession, WeaponStep, step_weapon_session,
};
use cs_content::weapons::{
    DeclaredGunDefinition, DeclaredGunMountKind, DeclaredGunRate, SYNTHETIC_LIFETIME_TICKS,
    SYNTHETIC_TICKS_BETWEEN_SHOTS, declared_synthetic_gun, synthetic_effect_id, synthetic_sound_id,
};
use cs_sim::audio_events::{
    AudioAssetSpec, AudioBus, AudioEventId, AudioRouter, Listener, OneShotEvent, OneShotOutcome,
    PlaybackMode, SpatialMix, SpatialPolicy,
};
use cs_sim::damage::{ActorId, DamageNodeKey, DamageResolver};
use cs_sim::targeting::Allegiance;
use cs_sim::weapons::{FireIntent, FireIntentId, GunBank, SYNTHETIC_STARTING_ROUNDS};
use cs_types::Tick;
use cs_types::content::{Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::net::SessionId;
use cs_types::space::WorldPosition;

const SESSION: u64 = 41;
const ROUTER_PRODUCER: u32 = 91;
const DAMAGE_PRODUCER: u32 = 92;
/// The scene generation every live entity in these tests is stamped under.
const GENERATION: SceneGeneration = SceneGeneration(1);
/// The tick length the fixed step runs at.
const DT_S: f64 = 1.0 / 30.0;
/// The synthetic fixture's own mount, used here as the nose gun.
const NOSE_MOUNT: &str = "gun_mount_1";
const WING_MOUNT: &str = "wing_mount_1";
/// The muzzle's z, the depth these tests fire from.
const MUZZLE_Z: f32 = 10.0;
/// The fixture shot-sound spec's designed gain, asserted against the cues.
const SHOT_GAIN: f64 = 1.0;

fn session_id() -> SessionId {
    SessionId::new(SESSION).expect("the test session generation is nonzero")
}

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: session_id(),
        serial,
    }
}

fn key(name: &str) -> DamageNodeKey {
    DamageNodeKey::new(name).expect("test node keys are valid")
}

fn claim() -> ClaimId {
    ClaimId::new("f27c.fx-test").expect("a valid claim id")
}

fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(value, Provenance::designed(claim())))
}

/// The declared fixture gun moved onto `mount` and declared as a `kind` gun,
/// with the fixture's own cadence and lifetime.
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
fn shooter_world(world: &mut World, mounts: &[&str]) {
    let fixture = declared_synthetic_gun();
    let root = world
        .spawn((
            WeaponActorBinding {
                actor: actor(1),
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
            cs_app::weapons::MountPoseBinding {
                mount: key(mount),
                generation: GENERATION,
            },
            NodeVisualTransform(GlobalTransform::from_xyz(x, 0.0, MUZZLE_Z)),
            ChildOf(root),
        ));
    }
}

/// The session's damage authority; these tests present effects and never
/// assert damage, so no actor needs registering.
fn damage_resolver() -> DamageResolver {
    DamageResolver::new(session_id(), DAMAGE_PRODUCER)
}

/// A weapon session with the fixture's declared guns mounted on `mounts` and
/// all selected.
fn session_with_guns(mounts: &[(&str, DeclaredGunMountKind)]) -> WeaponSession {
    let guns: Vec<DeclaredGunDefinition> = mounts
        .iter()
        .map(|(mount, kind)| declared_on(mount, *kind))
        .collect();
    let bank = GunBank::try_new(mounts.iter().map(|(mount, _)| key(mount))).expect("a valid bank");
    let mut session = WeaponSession::new(SESSION, Tick(0), ROUTER_PRODUCER)
        .expect("a nonzero session generation opens");
    session
        .register(actor(1), &guns, bank, SYNTHETIC_STARTING_ROUNDS)
        .expect("the declared guns register");
    session
}

/// The relation of every shooter to every target: nothing is declared, so no
/// sweep candidate could ever be admitted (these tests never sweep a target).
fn no_relation(_shooter: ActorId, _target: ActorId) -> Option<Allegiance> {
    None
}

fn step(at: Tick) -> WeaponStep<'static> {
    WeaponStep {
        at,
        dt_s: DT_S,
        wind_velocity_m_s: [0.0; 3],
        generation: GENERATION,
        relation: &no_relation,
    }
}

/// The player shooter's intent on `tick`.
fn intent(tick: u64, sequence: u32) -> FireIntent {
    FireIntent {
        id: FireIntentId {
            session: SESSION,
            tick: Tick(tick),
            producer: 1,
            sequence,
        },
        shooter: actor(1),
    }
}

/// The installed audio session these tests present through: the fixture's
/// declared shot sound lowered as a one-shot spec on the weapons bus, unless
/// `delivered` says the load delivered nothing.
fn audio_session(delivered: bool) -> AudioSession {
    let specs = delivered.then(|| {
        AudioAssetSpec::try_new(
            synthetic_sound_id(),
            AudioBus::Weapons,
            SHOT_GAIN,
            PlaybackMode::OneShot,
        )
        .expect("the fixture shot sound is a valid one-shot spec")
    });
    AudioSession::new(
        AudioRouter::new(session_id()),
        GENERATION,
        specs.into_iter().collect::<Vec<_>>(),
    )
}

/// The listener and attenuation policy a spatialised cue is placed against:
/// the listener at the world origin, right along +x, full audible range.
fn spatial() -> AudioSpatial {
    AudioSpatial::new(
        SpatialPolicy::try_new(1.0, 10_000.0).expect("a valid policy"),
        Listener::try_new([0.0; 3], [1.0, 0.0, 0.0]).expect("a valid listener"),
    )
}

/// Probe the router's one-shot ledger with `(WEAPON_SHOT_PRODUCER, sequence)`.
///
/// The router answers `Accepted` only for an id it has never accepted, so the
/// probe is what proves whether the consumer really played that sequence.
fn probe(audio: &mut AudioSession, sequence: u32) -> OneShotOutcome {
    let event = OneShotEvent::try_new(
        AudioEventId {
            session: session_id(),
            tick: Tick(0),
            producer: WEAPON_SHOT_PRODUCER,
            sequence,
        },
        synthetic_sound_id(),
        AudioBus::Weapons,
        SHOT_GAIN,
    )
    .expect("a valid probe event");
    audio.router.play_one_shot(&event)
}

/// Every spawned muzzle effect's record, in entity order.
fn muzzle_spawns(world: &World) -> Vec<&MuzzleEffectSpawn> {
    world
        .iter_entities()
        .filter_map(|entity_ref| entity_ref.get::<MuzzleEffectSpawn>())
        .collect()
}

/// The accepted minimum: one accepted shot presents exactly one placed cue of
/// the load's declared shot sound and exactly one muzzle-effect spawn carrying
/// the gun's declared effect id at the emitted muzzle position — and the
/// router ledger proves the cue was really accepted, once.
#[test]
fn accept_f27_c_fx_an_accepted_shot_sounds_once_and_spawns_its_declared_muzzle_effect() {
    let mut world = World::new();
    shooter_world(&mut world, &[NOSE_MOUNT]);
    let mut session = session_with_guns(&[(NOSE_MOUNT, DeclaredGunMountKind::Nose)]);
    let mut damage = damage_resolver();
    let mut audio = audio_session(true);
    let mut sequences = WeaponShotSequences::new();
    let listener = spatial();

    let fired = step_weapon_session(
        &mut world,
        &mut session,
        &mut damage,
        &[WeaponOrder::Fire(intent(0, 0))],
        &step(Tick(0)),
    );
    assert_eq!(fired.accepted.len(), 1, "the nose gun fires one shot");
    assert_eq!(
        fired.effects.len(),
        1,
        "the accepted shot emitted one effect"
    );

    let report = present_weapon_effects(
        &mut world,
        &mut session,
        &mut audio,
        &mut sequences,
        Some(&listener),
        GENERATION,
    );
    assert!(report.refusals.is_empty(), "nothing is refused: {report:?}");
    assert_eq!(report.played.len(), 1, "one accepted shot, one cue");
    let cue = &report.played[0];
    assert_eq!(
        cue.event.id,
        AudioEventId {
            session: session_id(),
            tick: Tick(0),
            producer: WEAPON_SHOT_PRODUCER,
            sequence: 0,
        },
        "the cue is stamped with this consumer's producer and the accepted tick"
    );
    assert_eq!(
        cue.event.asset,
        synthetic_sound_id(),
        "the cue plays the gun's own declared shot-sound id, not a hardcoded clip"
    );
    assert_eq!(
        cue.event.bus,
        AudioBus::Weapons,
        "the bus comes from the delivered spec"
    );
    assert_eq!(
        cue.event.gain, SHOT_GAIN,
        "the gain comes from the delivered spec"
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
        cue.origin, fired.effects[0].origin,
        "the cue carries the emitted muzzle position"
    );

    assert_eq!(
        report.spawned.len(),
        1,
        "one accepted shot, one muzzle spawn"
    );
    let entity = report.spawned[0];
    let spawn = world
        .get::<MuzzleEffectSpawn>(entity)
        .expect("the spawned entity carries its record");
    assert_eq!(
        spawn.effect,
        synthetic_effect_id(),
        "the spawn carries the gun's declared muzzle-effect id, not a hardcoded mesh"
    );
    assert_eq!(
        spawn.projectile, fired.effects[0].projectile,
        "the shot's round"
    );
    assert_eq!(spawn.shooter, actor(1), "the firing actor");
    assert_eq!(spawn.mount, key(NOSE_MOUNT), "the firing mount");
    assert_eq!(spawn.at, Tick(0), "the accepted tick");
    assert_eq!(
        spawn.generation, GENERATION,
        "the caller's scene generation"
    );
    let pose = world.get::<Transform>(entity).expect("the spawn is placed");
    assert_eq!(
        pose.translation,
        Vec3::new(0.0, 0.0, MUZZLE_Z),
        "the pose is the emitted muzzle position, f32-cast from the record"
    );

    assert!(
        matches!(
            probe(&mut audio, 0),
            OneShotOutcome::SuppressedDuplicate { .. }
        ),
        "the router really accepted sequence 0, so the shot sounded exactly once"
    );
    assert!(
        matches!(probe(&mut audio, 1), OneShotOutcome::Accepted { .. }),
        "sequence 1 was never played"
    );
}

/// Non-negotiable 5, end to end: a denied input and a duplicate packet emit no
/// effect, so the presentation pass plays and spawns nothing for them — and
/// the cue ledger proves no sequence was consumed.
#[test]
fn accept_f27_c_fx_a_denied_shot_and_a_duplicate_packet_present_nothing() {
    let mut world = World::new();
    shooter_world(&mut world, &[NOSE_MOUNT]);
    let mut session = session_with_guns(&[(NOSE_MOUNT, DeclaredGunMountKind::Nose)]);
    let mut damage = damage_resolver();
    let mut audio = audio_session(true);
    let mut sequences = WeaponShotSequences::new();
    let listener = spatial();

    // Tick 0: the accepted shot presents its cue and its spawn.
    let _ = step_weapon_session(
        &mut world,
        &mut session,
        &mut damage,
        &[WeaponOrder::Fire(intent(0, 0))],
        &step(Tick(0)),
    );
    let first = present_weapon_effects(
        &mut world,
        &mut session,
        &mut audio,
        &mut sequences,
        Some(&listener),
        GENERATION,
    );
    assert_eq!(first.played.len(), 1, "the accepted shot presented its cue");
    assert_eq!(
        first.spawned.len(),
        1,
        "the accepted shot presented its spawn"
    );

    // Tick 1: the mount is still cooling down (the fixture's four-tick
    // cadence), so the input is denied whole — no effect, hence nothing to
    // present on either side.
    let denied = step_weapon_session(
        &mut world,
        &mut session,
        &mut damage,
        &[WeaponOrder::Fire(intent(1, 1))],
        &step(Tick(1)),
    );
    assert!(
        denied.accepted.is_empty(),
        "the still-cooling mount denies the shot: {denied:?}"
    );
    assert!(denied.effects.is_empty(), "a denied input emits no effect");
    let second = present_weapon_effects(
        &mut world,
        &mut session,
        &mut audio,
        &mut sequences,
        Some(&listener),
        GENERATION,
    );
    assert_eq!(
        second,
        WeaponEffectReport::default(),
        "a denied input presents nothing: no cue, no spawn, no refusal"
    );

    // The tick-0 intent replayed later is a duplicate packet: refused by the
    // once-only tick, emitting nothing, so it presents nothing either.
    let replay = step_weapon_session(
        &mut world,
        &mut session,
        &mut damage,
        &[WeaponOrder::Fire(intent(0, 0))],
        &step(Tick(2)),
    );
    assert!(
        replay.effects.is_empty(),
        "a duplicate packet emits no effect"
    );
    let third = present_weapon_effects(
        &mut world,
        &mut session,
        &mut audio,
        &mut sequences,
        Some(&listener),
        GENERATION,
    );
    assert_eq!(
        third,
        WeaponEffectReport::default(),
        "a duplicate packet presents nothing"
    );

    assert_eq!(
        muzzle_spawns(&world).len(),
        1,
        "only the accepted shot ever spawned a muzzle effect"
    );
    assert!(
        matches!(probe(&mut audio, 1), OneShotOutcome::Accepted { .. }),
        "the denied and replayed inputs consumed no cue sequence"
    );
}

/// An accepted shot whose declared sound the load never delivered is refused
/// by name — never a guessed clip — and the muzzle half still spawns, because
/// the two declarations are independent.
#[test]
fn accept_f27_c_fx_an_undelivered_shot_sound_is_refused_by_name() {
    let mut world = World::new();
    shooter_world(&mut world, &[NOSE_MOUNT]);
    let mut session = session_with_guns(&[(NOSE_MOUNT, DeclaredGunMountKind::Nose)]);
    let mut damage = damage_resolver();
    let mut audio = audio_session(false);
    let mut sequences = WeaponShotSequences::new();
    let listener = spatial();

    let fired = step_weapon_session(
        &mut world,
        &mut session,
        &mut damage,
        &[WeaponOrder::Fire(intent(0, 0))],
        &step(Tick(0)),
    );
    let report = present_weapon_effects(
        &mut world,
        &mut session,
        &mut audio,
        &mut sequences,
        Some(&listener),
        GENERATION,
    );
    assert!(
        report.played.is_empty(),
        "nothing playable names the asset: {report:?}"
    );
    assert_eq!(
        report.refusals,
        vec![WeaponEffectRefusal::UnknownAsset {
            projectile: fired.effects[0].projectile,
            sound: synthetic_sound_id(),
        }],
        "the missing declared sound is refused by name"
    );
    assert_eq!(
        report.spawned.len(),
        1,
        "the muzzle half still presents the accepted shot"
    );
    assert!(
        matches!(probe(&mut audio, 0), OneShotOutcome::Accepted { .. }),
        "the refused audio half played no sequence at all"
    );
}

/// Spatialisation uses the emitted muzzle position: each cue's placement is
/// the spatial law's answer for that shot's own muzzle, and a world without a
/// listener reports the cue unplaced rather than defaulting it onto the
/// listener.
#[test]
fn accept_f27_c_fx_spatialisation_uses_the_emitted_muzzle_position() {
    let mut world = World::new();
    shooter_world(&mut world, &[NOSE_MOUNT, WING_MOUNT]);
    let mut session = session_with_guns(&[
        (NOSE_MOUNT, DeclaredGunMountKind::Nose),
        (WING_MOUNT, DeclaredGunMountKind::WingLeft),
    ]);
    let mut damage = damage_resolver();
    let mut audio = audio_session(true);
    let mut sequences = WeaponShotSequences::new();

    // The listener sits one metre right of the nose, so the wing muzzle — two
    // metres right, ten metres ahead — places off-axis: distance sqrt(101),
    // gain 1/sqrt(101), pan +1/sqrt(101). A cue defaulted onto the listener
    // would instead read gain 1.0 and pan 0.0.
    let listener = AudioSpatial::new(
        SpatialPolicy::try_new(1.0, 10_000.0).expect("a valid policy"),
        Listener::try_new([1.0, 0.0, 0.0], [1.0, 0.0, 0.0]).expect("a valid listener"),
    );
    let fired = step_weapon_session(
        &mut world,
        &mut session,
        &mut damage,
        &[WeaponOrder::Fire(intent(0, 0))],
        &step(Tick(0)),
    );
    let placed = present_weapon_effects(
        &mut world,
        &mut session,
        &mut audio,
        &mut sequences,
        Some(&listener),
        GENERATION,
    );
    assert_eq!(placed.played.len(), 2, "the whole bank fired");
    let projectile_of = |mount: &str| {
        fired
            .effects
            .iter()
            .find(|effect| effect.mount == key(mount))
            .expect("the mount fired")
            .projectile
    };
    let wing = placed
        .played
        .iter()
        .find(|cue| cue.projectile == projectile_of(WING_MOUNT))
        .expect("the wing cue exists");
    let distance = 101.0_f64.sqrt();
    assert_eq!(
        wing.spatial,
        SpatialPlacement::Placed(SpatialMix {
            gain: 1.0 / distance,
            pan: 1.0 / distance,
        }),
        "the wing cue is placed at the emitted muzzle, off the listener's axis"
    );
    let nose = placed
        .played
        .iter()
        .find(|cue| cue.projectile == projectile_of(NOSE_MOUNT))
        .expect("the nose cue exists");
    assert_eq!(
        nose.spatial,
        SpatialPlacement::Placed(SpatialMix {
            gain: 1.0 / distance,
            pan: -1.0 / distance,
        }),
        "the nose muzzle is one metre left of the listener: same distance, mirrored pan"
    );

    // A second bank fires once the four-tick cadence is served, and this time
    // the world carries no listener: the cues still play — the declared sound
    // is delivered — but their placement is reported missing, never guessed.
    let _ = step_weapon_session(
        &mut world,
        &mut session,
        &mut damage,
        &[WeaponOrder::Fire(intent(4, 1))],
        &step(Tick(4)),
    );
    let unplaced = present_weapon_effects(
        &mut world,
        &mut session,
        &mut audio,
        &mut sequences,
        None,
        GENERATION,
    );
    assert_eq!(unplaced.played.len(), 2, "the reloaded bank fired again");
    assert!(
        unplaced
            .played
            .iter()
            .all(|cue| cue.spatial == SpatialPlacement::NoListener),
        "without a listener every cue reports itself unplaced: {unplaced:?}"
    );
}

/// A drained batch can never be presented twice: the second pass over an
/// unchanged session sees an empty log and the ledger proves no second cue.
#[test]
fn accept_f27_c_fx_a_repeated_present_over_an_unchanged_session_plays_nothing() {
    let mut world = World::new();
    shooter_world(&mut world, &[NOSE_MOUNT]);
    let mut session = session_with_guns(&[(NOSE_MOUNT, DeclaredGunMountKind::Nose)]);
    let mut damage = damage_resolver();
    let mut audio = audio_session(true);
    let mut sequences = WeaponShotSequences::new();
    let listener = spatial();

    let _ = step_weapon_session(
        &mut world,
        &mut session,
        &mut damage,
        &[WeaponOrder::Fire(intent(0, 0))],
        &step(Tick(0)),
    );
    let first = present_weapon_effects(
        &mut world,
        &mut session,
        &mut audio,
        &mut sequences,
        Some(&listener),
        GENERATION,
    );
    assert_eq!(first.played.len(), 1, "the accepted shot presented once");

    let again = present_weapon_effects(
        &mut world,
        &mut session,
        &mut audio,
        &mut sequences,
        Some(&listener),
        GENERATION,
    );
    assert_eq!(
        again,
        WeaponEffectReport::default(),
        "the drained log presents nothing a second time, although the round is still live"
    );
    assert_eq!(
        muzzle_spawns(&world).len(),
        1,
        "the round being live does not spawn a second muzzle effect"
    );
    assert!(
        matches!(probe(&mut audio, 1), OneShotOutcome::Accepted { .. }),
        "no second cue was accepted"
    );
}

/// Teardown: release despawns every muzzle-effect spawn in one call, a second
/// call is a no-op, and a spawn left by a previous scene generation goes too.
#[test]
fn accept_f27_c_fx_release_spawns_a_reload_leaves_no_stale_muzzle_effect() {
    let mut world = World::new();
    shooter_world(&mut world, &[NOSE_MOUNT]);
    let mut session = session_with_guns(&[(NOSE_MOUNT, DeclaredGunMountKind::Nose)]);
    let mut damage = damage_resolver();
    let mut audio = audio_session(true);
    let mut sequences = WeaponShotSequences::new();
    let listener = spatial();

    let _ = step_weapon_session(
        &mut world,
        &mut session,
        &mut damage,
        &[WeaponOrder::Fire(intent(0, 0))],
        &step(Tick(0)),
    );
    let first = present_weapon_effects(
        &mut world,
        &mut session,
        &mut audio,
        &mut sequences,
        Some(&listener),
        GENERATION,
    );
    assert_eq!(first.spawned.len(), 1, "one accepted shot, one spawn");

    // A spawn stamped by the previous scene generation — what a reloaded
    // scene leaves behind — is released by the same total pass.
    let effect = WeaponEffect {
        projectile: first.played[0].projectile,
        shooter: actor(1),
        mount: key(NOSE_MOUNT),
        effect: synthetic_effect_id(),
        sound: synthetic_sound_id(),
        origin: WorldPosition::try_new([0.0, 0.0, f64::from(MUZZLE_Z)])
            .expect("a finite muzzle position"),
        at: Tick(0),
    };
    let stale = spawn_muzzle_effect(&mut world, &effect, SceneGeneration(0));
    assert!(
        world.get::<MuzzleEffectSpawn>(stale).is_some(),
        "the stale spawn exists"
    );

    assert_eq!(
        release_muzzle_effects(&mut world),
        2,
        "both the live spawn and the stale generation's spawn go"
    );
    assert!(
        muzzle_spawns(&world).is_empty(),
        "no muzzle-effect entity outlives teardown"
    );
    assert_eq!(
        release_muzzle_effects(&mut world),
        0,
        "a second release is a no-op"
    );
}
