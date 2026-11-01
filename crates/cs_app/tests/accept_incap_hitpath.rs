//! Task #2252: the `SONIC`/`FLASH` incapacitation runs from the cs_app blast
//! path and feeds `V+0x358` to the ground-blow gate.
//!
//! Task test prefix: `accept_incap_hitpath_`. A blast of a component whose
//! record carries an exposure bit measures the image's per-hit exposure
//! (`0x42e840`) from the session's own hit data — the point of contact, the
//! record's `IMPACT_PROXIMITY` radius and the victim's forward axis — and
//! applies it through `cs_sim::weapons::incapacitation::apply_exposure`. The
//! measured gate then reads that actor's `V+0x358` as `4` and zeroes its six
//! control words until the whole-tick deadline, and the player takes the
//! screen/audio record only and is never control-incapacitated.
//!
//! Every declared value here is synthetic fixture data; the measured formula
//! and its constants are `cs_sim`'s (task #2172) and the addresses are pinned
//! by the ignored image tests below. Nothing is `verified_original`.

use avian3d::prelude::LinearVelocity;
use bevy::prelude::{ChildOf, GlobalTransform, Vec3, World};
use cs_app::ordnance::{
    BlastExposureRefusal, OrdnanceEngagement, OrdnanceLaunch, OrdnanceLaunchId,
    OrdnanceLauncherBinding, OrdnanceOrder, OrdnanceSession, OrdnanceSessionTick, OrdnanceStep,
    measured_exposure_bit, step_ordnance_session,
};
use cs_app::playtest::scene::overlay_incapacitation;
use cs_app::scene::{NodeVisualTransform, SceneGeneration};
use cs_app::weapons::MountPoseBinding;
use cs_content::ordnance::{
    DECLARED_SYNTHETIC_DIRECT_KEY, DECLARED_SYNTHETIC_LAUNCHER_MOUNT, DeclaredAreaEffect,
    DeclaredFuseRule, DeclaredOrdnance, DeclaredOrdnanceDetails, DeclaredOrdnanceFamily,
    OriginalComponentLaunch, declared_synthetic_direct, declared_synthetic_provenance,
    read_original_component_table,
};
use cs_sim::damage::{
    ActorId, AttributionRule, DamageNodeKey, DamagePolicy, DamageResolver, SystemKind,
    synthetic_airframe_graph,
};
use cs_sim::flight::{
    OriginalAirframe, OriginalFlightModel, OriginalGlobals, OriginalInput, OriginalState,
    OriginalStep, SegmentProbeHit, SegmentProbeOwner,
};
use cs_sim::time::TickRate;
use cs_sim::weapons::incapacitation::{
    ExposureBit, ExposureOutcome, INCAPACITATED_MODE, incapacitated_mode,
};
use cs_sim::weapons::{BlastRecipient, ProjectileId, StatusEffectTarget, SweptHit};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::net::SessionId;
use cs_types::space::{Quaternion, WorldPosition};

const SESSION: u64 = 71;
const PRODUCER: u32 = 111;
const DAMAGE_PRODUCER: u32 = 112;
const GENERATION: SceneGeneration = SceneGeneration(1);
const DT_S: f64 = 1.0 / 30.0;
const HULL: &str = "hull";
const MOUNT_Z: f32 = 10.0;
/// The declared blast radius the exposure's ratio is measured over — the
/// stand-in for a record's `IMPACT_PROXIMITY`.
const RADIUS_M: f64 = 40.0;
/// The tick the swept contact ends the item on; later than the arming window.
const END_TICK: u64 = 6;
/// The measured full-intensity duration: `intensity * 5.0` s at 30 Hz.
const FULL_DURATION_TICKS: u64 = 150;
/// The player seat these tests name on the session.
const PLAYER_SERIAL: u64 = 2;
/// The AI the blast strikes.
const AI_SERIAL: u64 = 3;
/// The shooter.
const SHOOTER_SERIAL: u64 = 1;

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

fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(
        value,
        Provenance::designed(ClaimId::new("incap.hitpath-test").expect("a valid claim id")),
    ))
}

fn rate() -> TickRate {
    TickRate::new(30).expect("the test rate is nonzero")
}

/// The direct-explosive fixture with a declared area of [`RADIUS_M`] and no
/// declared status, which is exactly the shape a measured `SONIC`/`FLASH`
/// record lowers to: its exposure has no fixed duration, so the declared list
/// is empty and the bit is registered separately.
///
/// `timed` puts the fuse on its own clock, which fires the blast at the item's
/// tick-end position — the position the probe flight reads — so the geometry
/// these tests assert is exact rather than the point of contact an impact
/// trigger interpolates.
fn declared_exposure(area: bool, timed: bool) -> DeclaredOrdnance {
    let fixture = declared_synthetic_direct();
    let mut projectile = match fixture.details() {
        DeclaredOrdnanceDetails::Projectile(projectile) => (**projectile).clone(),
        DeclaredOrdnanceDetails::Nitro(_) => panic!("the direct fixture is a projectile"),
    };
    if timed {
        projectile.fuse = DeclaredFuseRule::Timed {
            ticks: known(END_TICK),
        };
    }
    projectile.area_effect = area.then(|| DeclaredAreaEffect {
        radius_m: known(RADIUS_M),
        lifetime_ticks: known(5),
    });
    projectile.status = Vec::new();
    DeclaredOrdnance::try_new(
        ContentId::from_source(ContentKind::Weapon, DECLARED_SYNTHETIC_DIRECT_KEY)
            .expect("a weapon id"),
        Origin::SyntheticFixture,
        DeclaredOrdnanceFamily::DirectExplosive,
        DeclaredOrdnanceDetails::Projectile(Box::new(projectile)),
        None,
        declared_synthetic_provenance(),
    )
    .expect("the retimed direct fixture is valid")
}

fn engagement() -> OrdnanceEngagement {
    OrdnanceEngagement {
        damage_target: actor(AI_SERIAL),
        node: key(HULL),
        status_recipient: StatusEffectTarget::actor_system(
            actor(AI_SERIAL),
            SystemKind::Propulsion,
        ),
    }
}

fn launcher_world(world: &mut World) {
    let root = world
        .spawn((
            OrdnanceLauncherBinding {
                actor: actor(SHOOTER_SERIAL),
                ordnance: vec![
                    ContentId::from_source(ContentKind::Weapon, DECLARED_SYNTHETIC_DIRECT_KEY)
                        .expect("a weapon id"),
                ],
                loadout: ContentId::from_source(ContentKind::Loadout, "synthetic.fixture_loadout")
                    .expect("a loadout id"),
                generation: GENERATION,
            },
            LinearVelocity(Vec3::ZERO),
        ))
        .id();
    world.spawn((
        MountPoseBinding {
            mount: key(DECLARED_SYNTHETIC_LAUNCHER_MOUNT),
            generation: GENERATION,
        },
        NodeVisualTransform(GlobalTransform::from_xyz(0.0, 0.0, MOUNT_Z)),
        ChildOf(root),
    ));
}

fn damage_resolver() -> DamageResolver {
    let mut damage = DamageResolver::new(session_id(), DAMAGE_PRODUCER);
    for serial in [PLAYER_SERIAL, AI_SERIAL] {
        damage
            .register_actor(
                actor(serial),
                synthetic_airframe_graph(),
                DamagePolicy {
                    attribution: AttributionRule::FirstLethalHit,
                },
            )
            .expect("the fixture graph registers");
    }
    damage
}

/// What one flown shot produced, with the session still open so the ledger can
/// be read and advanced.
struct Flight {
    world: World,
    session: OrdnanceSession,
    damage: DamageResolver,
    /// The tick the item's fuse fired on.
    tick: OrdnanceSessionTick,
    /// Where the item was when its fuse fired.
    centre: [f64; 3],
}

/// Flies one exposure shot from tick 0 and ends it on [`END_TICK`] by a swept
/// contact on the AI actor.
///
/// `build` receives the item's position at the end tick and returns the
/// recipients the step supplies; a probe flight (identical and deterministic)
/// supplies that position when recipients are wanted.
fn fly(
    declared: &DeclaredOrdnance,
    bit: ExposureBit,
    player: Option<ActorId>,
    build: Option<&dyn Fn([f64; 3]) -> Vec<BlastRecipient>>,
) -> Flight {
    let probe = build.map(|_| {
        let flown = fly(declared, bit, player, None);
        flown.centre
    });
    let supplied = build.map(|build| build(probe.expect("a probe position")));
    let mut world = World::new();
    launcher_world(&mut world);
    let mut catalogue = cs_sim::weapons::OrdnanceRegistry::new();
    let component = cs_app::ordnance::lower_ordnance(declared).expect("the fixture lowers");
    catalogue
        .register(component)
        .expect("the fixture registers");
    let mut session = OrdnanceSession::new(SESSION, Tick(0), rate(), PRODUCER, catalogue)
        .expect("a nonzero session opens");
    let ordnance = session
        .register(actor(SHOOTER_SERIAL), std::slice::from_ref(declared))
        .expect("the fixture registers for the shooter")[0]
        .ordnance
        .clone();
    session
        .register_exposure(&ordnance, bit)
        .expect("the session is open");
    let mut session = match player {
        Some(player) => session.with_player(player),
        None => session,
    };
    let mut damage = damage_resolver();
    let mut projectile: Option<ProjectileId> = None;
    let mut fired: Option<(OrdnanceSessionTick, [f64; 3])> = None;
    for at in 0..=END_TICK {
        let orders = if at == 0 {
            vec![OrdnanceOrder::Launch(OrdnanceLaunch {
                id: OrdnanceLaunchId {
                    session: SESSION,
                    tick: Tick(0),
                    producer: PRODUCER,
                    sequence: 0,
                },
                shooter: actor(SHOOTER_SERIAL),
                ordnance: ordnance.clone(),
                target: None,
                engagement: engagement(),
            })]
        } else {
            Vec::new()
        };
        let impacts = match projectile {
            Some(projectile) if at == END_TICK => vec![SweptHit {
                projectile,
                target: actor(AI_SERIAL),
                time_of_impact: 0.5,
            }],
            _ => Vec::new(),
        };
        let tick = step_ordnance_session(
            &mut world,
            &mut session,
            &mut damage,
            &orders,
            &OrdnanceStep {
                at: Tick(at),
                dt_s: DT_S,
                wind_velocity_m_s: [0.0; 3],
                generation: GENERATION,
                guidance: None,
                targets: &[],
                impacts: &impacts,
                blast_recipients: supplied.as_deref(),
                victim_nodes: None,
            },
        );
        if at == 0 {
            projectile = Some(tick.launched[0]);
        }
        if fired.is_none() && !tick.detonations.is_empty() {
            let centre = session
                .runtime()
                .get(&projectile.expect("launched"))
                .map(|item| item.current())
                .expect("a triggered item stays live until the next advance");
            fired = Some((tick, [centre.x(), centre.y(), centre.z()]));
        }
    }
    let (tick, centre) = fired.expect("the item ended");
    Flight {
        world,
        session,
        damage,
        tick,
        centre,
    }
}

/// One step of the still-open flight at `at`, with no orders and no contacts.
fn advance(flight: &mut Flight, at: Tick) -> OrdnanceSessionTick {
    step_ordnance_session(
        &mut flight.world,
        &mut flight.session,
        &mut flight.damage,
        &[],
        &OrdnanceStep {
            at,
            dt_s: DT_S,
            wind_velocity_m_s: [0.0; 3],
            generation: GENERATION,
            guidance: None,
            targets: &[],
            impacts: &[],
            blast_recipients: None,
            victim_nodes: None,
        },
    )
}

/// A recipient at `distance_m` along +X from the detonation, with an optional
/// forward axis.
fn recipient(
    serial: u64,
    distance_m: f64,
    centre: [f64; 3],
    facing: Option<[f64; 3]>,
) -> BlastRecipient {
    let position = WorldPosition::try_new([centre[0] + distance_m, centre[1], centre[2]])
        .expect("a finite position");
    let recipient = BlastRecipient::new(actor(serial), key(HULL), position);
    match facing {
        Some(facing) => recipient.with_facing(facing),
        None => recipient,
    }
}

/// The declared original airframe the ground blow is exercised on: the
/// fixture the `cs_sim` ground-blow suite declares, imported through the same
/// `from_values` boundaries production uses.
fn original_model() -> OriginalFlightModel {
    let airframe = OriginalAirframe::from_values(&[
        ("roll_torque", 7.5),
        ("pitch_torque", 3.3),
        ("rudder_torque", 2.0),
        ("level_off_rate", 4.0),
        ("return_rate", 3.0),
        ("ang_momentum_damp", 5.0),
        ("rec_moments_inertia_x", 1.18),
        ("rec_moments_inertia_y", 1.0),
        ("rec_moments_inertia_z", 1.1),
        ("fd_speed", 135.0),
        ("engine_factor", 0.62),
        ("drag_factor", 0.37),
        ("veh_weight", 1900.0),
        ("ref_area", 330.0),
        ("gravity", 20.0),
    ])
    .expect("the declared airframe fields are complete");
    let globals = OriginalGlobals::from_values(&[
        ("nom_gravity", 20.0),
        ("lift_aoa_0_deg", 5.0),
        ("lift_aoa_1_deg", 9.0),
        ("max_aoa_deg", 46.0),
        ("high_g_0", 9.0),
        ("high_g_1", 15.0),
        ("low_g_0", -6.0),
        ("low_g_1", -9.0),
        ("lift_accel_rate", 0.75),
        ("stall_mag", 1.25),
        ("turn_fade_in_mph", 10.0),
        ("turn_fade_out_mph", 50.0),
        ("yaw_low_speed", 0.0625),
        ("yaw_high_speed", 0.17),
        ("yaw_fade_in_mph", 10.0),
        ("yaw_max_mph", 50.0),
        ("yaw_fade_out_mph", 400.0),
        ("groundblow_elev_m", 400.0),
        ("groundblow_mag", 10.0),
        ("ai_groundblow", 0.5),
    ])
    .expect("the declared globals are complete");
    OriginalFlightModel::full(airframe, globals)
}

/// A level, hands-off aircraft at 300 m moving along +X.
fn probing_state() -> OriginalState {
    let mut state = OriginalState::at(300.0, Quaternion::IDENTITY, 0.0, 1e9);
    state.velocity_mps = [100.0, 0.0, 0.0];
    state
}

const DT: f64 = 0.01;

fn stepped(
    model: &OriginalFlightModel,
    state: &OriginalState,
    input: OriginalInput,
) -> (OriginalState, OriginalStep) {
    let mut state = state.clone();
    let step = model
        .step(&mut state, input, DT)
        .expect("the declared step is valid");
    (state, step)
}

/// The ground blow's probe hit: a surface normal with a chord, which the law
/// turns into a non-zero torque and a velocity steering term.
fn surf_hit() -> SegmentProbeHit {
    SegmentProbeHit {
        distance_m: 100.0,
        normal: [0.0, 300.0, 400.0],
        owner: SegmentProbeOwner::World,
    }
}

/// Whether the ground blow changes the step: the probed step and state against
/// the same input unprobed.
fn blow_runs(model: &OriginalFlightModel, input: OriginalInput) -> bool {
    let state = probing_state();
    let probed = OriginalInput {
        segment_probe: Some(surf_hit()),
        ..input
    };
    let (bare_state, bare) = stepped(model, &state, input);
    let (blown_state, step) = stepped(model, &state, probed);
    step.world_torque != bare.world_torque || blown_state.velocity_mps != bare_state.velocity_mps
}

/// The AI's own command: pitch and throttle held, so a zeroed control word is
/// observable.
fn ai_input() -> OriginalInput {
    OriginalInput {
        pitch: 1.0,
        throttle: 1.0,
        is_player: false,
        ..OriginalInput::default()
    }
}

/// A `SONIC` hit at the centre of the blast incapacitates the AI it struck:
/// `V+0x358 == 4`, every control word zero, the ground blow suppressed, all
/// until the measured deadline tick and nothing after it.
#[test]
fn accept_incap_hitpath_a_sonic_hit_incapacitates_an_ai_until_the_measured_deadline() {
    let centre_of = |c: [f64; 3]| vec![recipient(AI_SERIAL, 0.0, c, None)];
    let mut flight = fly(
        &declared_exposure(true, false),
        ExposureBit::Sonic,
        Some(actor(PLAYER_SERIAL)),
        Some(&centre_of),
    );

    assert!(
        flight.tick.exposures_refused.is_empty(),
        "{:?}",
        flight.tick.exposures_refused
    );
    assert_eq!(flight.tick.exposures.len(), 1);
    let applied = flight.tick.exposures[0].clone();
    assert_eq!(applied.target, actor(AI_SERIAL));
    assert_eq!(applied.bit, ExposureBit::Sonic);
    let ExposureOutcome::Incapacitated {
        exposure,
        ticks,
        expires_at,
        ..
    } = applied.outcome
    else {
        panic!(
            "a non-player inside the radius is incapacitated, got {:?}",
            applied.outcome
        );
    };
    assert!(
        (exposure.intensity - 1.0).abs() < 1e-12,
        "full at the centre"
    );
    assert!((exposure.duration_s - 5.0).abs() < 1e-12);
    assert_eq!(ticks, FULL_DURATION_TICKS);
    assert_eq!(
        expires_at,
        Tick(flight.tick.at.0 + FULL_DURATION_TICKS),
        "the deadline is the burst tick plus the measured length"
    );

    // The ledger the flight consumer reads holds the mode.
    assert_eq!(
        incapacitated_mode(flight.session.runtime().status(), actor(AI_SERIAL)),
        Some(INCAPACITATED_MODE)
    );

    let model = original_model();
    let ledger = flight.session.runtime().status();
    let held = overlay_incapacitation(ai_input(), false, ledger, actor(AI_SERIAL));
    assert_eq!(held.status_word, 4, "`V+0x358` reads the measured mode");
    assert_eq!(
        (held.roll, held.pitch, held.yaw, held.throttle),
        (0.0, 0.0, 0.0, 0.0),
        "the six control words read zero"
    );
    assert!(!held.nitro && !held.engine_out);
    assert!(
        !blow_runs(&model, held),
        "an incapacitated AI runs no ground blow"
    );

    // The tick before the deadline still holds it.
    advance(&mut flight, Tick(expires_at.0 - 1));
    assert_eq!(
        incapacitated_mode(flight.session.runtime().status(), actor(AI_SERIAL)),
        Some(INCAPACITATED_MODE)
    );

    // The deadline tick clears it, and the controls and the blow come back.
    let cleared = advance(&mut flight, expires_at);
    assert!(
        cleared
            .status_expired
            .iter()
            .any(|effect| effect.target == StatusEffectTarget::whole_actor(actor(AI_SERIAL))),
        "the ledger reports the deadline once"
    );
    assert_eq!(
        incapacitated_mode(flight.session.runtime().status(), actor(AI_SERIAL)),
        None
    );
    let ledger = flight.session.runtime().status();
    let restored = overlay_incapacitation(ai_input(), false, ledger, actor(AI_SERIAL));
    assert_eq!(restored.status_word, 0);
    assert_eq!(restored.pitch, 1.0);
    assert_eq!(restored.throttle, 1.0);
    assert!(blow_runs(&model, restored), "a recovered AI blows again");
}

/// The deadline is the measured `intensity * 5.0` s: a hit at 0.8 of the radius
/// (ratio 0.64) is `1 - 2.5 * 0.04 = 0.9` strong, three hundredths of a second
/// short of the full five.
#[test]
fn accept_incap_hitpath_the_deadline_is_the_measured_intensity_times_five_seconds() {
    let at_eighty_percent = |c: [f64; 3]| vec![recipient(AI_SERIAL, RADIUS_M * 0.8, c, None)];
    let flight = fly(
        &declared_exposure(true, true),
        ExposureBit::Sonic,
        None,
        Some(&at_eighty_percent),
    );
    let applied = &flight.tick.exposures[0];
    let ExposureOutcome::Incapacitated {
        exposure,
        ticks,
        expires_at,
        ..
    } = applied.outcome
    else {
        panic!(
            "0.8 of the radius is inside the falloff, got {:?}",
            applied.outcome
        );
    };
    assert!((exposure.intensity - 0.9).abs() < 1e-9, "{:?}", exposure);
    assert!((exposure.duration_s - 4.5).abs() < 1e-9);
    assert_eq!(ticks, 135, "ceil(4.5 * 30)");
    assert_eq!(expires_at, Tick(flight.tick.at.0 + 135));

    // At and beyond the radius the intensity is zero: nothing is applied.
    let outside = |c: [f64; 3]| vec![recipient(AI_SERIAL, RADIUS_M, c, None)];
    let flight = fly(
        &declared_exposure(true, true),
        ExposureBit::Sonic,
        None,
        Some(&outside),
    );
    assert_eq!(flight.tick.exposures[0].outcome, ExposureOutcome::NoEffect);
    assert_eq!(
        incapacitated_mode(flight.session.runtime().status(), actor(AI_SERIAL)),
        None
    );
}

/// `FLASH` scales the intensity by `clamp(2 * facing_dot, 0, 1)` over the
/// **normalised** victim-minus-source offset: 10 m away with a forward axis
/// whose dot with that offset is 0.25 halves a full-intensity hit, where the
/// unnormalised offset (dot 2.5, clamped) would have left it at full.
#[test]
fn accept_incap_hitpath_flash_scales_by_the_normalised_facing() {
    let forward = [0.25, 0.968_245_836_551_854_3, 0.0];
    let facing = |c: [f64; 3]| vec![recipient(AI_SERIAL, 10.0, c, Some(forward))];
    let flight = fly(
        &declared_exposure(true, true),
        ExposureBit::Flash,
        None,
        Some(&facing),
    );
    let ExposureOutcome::Incapacitated { exposure, .. } = flight.tick.exposures[0].outcome else {
        panic!(
            "a faced hit is an incapacitation, got {:?}",
            flight.tick.exposures[0].outcome
        );
    };
    assert!(
        (exposure.intensity - 0.5).abs() < 1e-12,
        "dot 0.25 halves it, got {exposure:?}"
    );
    assert!((exposure.duration_s - 2.5).abs() < 1e-12);

    // Facing away: the measured routine returns no effect at all.
    let away = |c: [f64; 3]| vec![recipient(AI_SERIAL, 10.0, c, Some([-1.0, 0.0, 0.0]))];
    let flight = fly(
        &declared_exposure(true, true),
        ExposureBit::Flash,
        None,
        Some(&away),
    );
    assert_eq!(flight.tick.exposures[0].outcome, ExposureOutcome::NoEffect);
    assert_eq!(
        incapacitated_mode(flight.session.runtime().status(), actor(AI_SERIAL)),
        None
    );
}

/// The player takes the screen/audio record and keeps its controls: the
/// session names it as the seat `0x71c298`, the ledger never holds the mode,
/// and the player seat refuses the word even when the ledger holds one for its
/// actor.
#[test]
fn accept_incap_hitpath_the_player_is_never_control_incapacitated() {
    let centre_of = |c: [f64; 3]| vec![recipient(PLAYER_SERIAL, 0.0, c, None)];
    let flight = fly(
        &declared_exposure(true, true),
        ExposureBit::Sonic,
        Some(actor(PLAYER_SERIAL)),
        Some(&centre_of),
    );
    let applied = &flight.tick.exposures[0];
    assert_eq!(applied.target, actor(PLAYER_SERIAL));
    let ExposureOutcome::PlayerScreenAudio(exposure) = applied.outcome else {
        panic!(
            "the player takes the screen/audio record, got {:?}",
            applied.outcome
        );
    };
    assert!((exposure.intensity - 1.0).abs() < 1e-12);
    assert_eq!(
        incapacitated_mode(flight.session.runtime().status(), actor(PLAYER_SERIAL)),
        None,
        "the player's controls are not zeroed"
    );

    let mut input = ai_input();
    input.is_player = true;
    let ledger = flight.session.runtime().status();
    let under = overlay_incapacitation(input, true, ledger, actor(PLAYER_SERIAL));
    assert_eq!(under.status_word, 0);
    assert_eq!(under.pitch, 1.0);
    assert_eq!(under.throttle, 1.0);
    assert!(
        blow_runs(&original_model(), under),
        "the player's blow runs"
    );

    // The seat guard: the same actor struck in a session that does not name it
    // as the player does get the ledger entry — the measured gate's own
    // `0x71c298` test — and the player seat still reads no control loss.
    let other = fly(
        &declared_exposure(true, true),
        ExposureBit::Sonic,
        None,
        Some(&centre_of),
    );
    assert_eq!(
        incapacitated_mode(other.session.runtime().status(), actor(PLAYER_SERIAL)),
        Some(INCAPACITATED_MODE)
    );
    let ledger = other.session.runtime().status();
    let guarded = overlay_incapacitation(input, true, ledger, actor(PLAYER_SERIAL));
    assert_eq!(guarded.status_word, 0);
    assert_eq!(guarded.pitch, 1.0);
    assert_eq!(guarded.throttle, 1.0);
    assert!(!guarded.nitro && !guarded.engine_out);
}

/// A `FLASH` victim with no forward axis and a blast with no supplied
/// recipients are refusals, not an exposure measured over an invented value.
#[test]
fn accept_incap_hitpath_an_unmeasurable_exposure_is_refused_by_name() {
    let no_facing = |c: [f64; 3]| vec![recipient(AI_SERIAL, 0.0, c, None)];
    let flight = fly(
        &declared_exposure(true, true),
        ExposureBit::Flash,
        None,
        Some(&no_facing),
    );
    assert!(flight.tick.exposures.is_empty());
    assert!(matches!(
        flight.tick.exposures_refused.as_slice(),
        [BlastExposureRefusal::MissingFacing { target, .. }] if *target == actor(AI_SERIAL)
    ));
    assert_eq!(
        incapacitated_mode(flight.session.runtime().status(), actor(AI_SERIAL)),
        None
    );

    let flight = fly(
        &declared_exposure(true, true),
        ExposureBit::Sonic,
        None,
        None,
    );
    assert!(flight.tick.exposures.is_empty());
    assert!(matches!(
        flight.tick.exposures_refused.as_slice(),
        [BlastExposureRefusal::MissingRecipients { .. }]
    ));

    // A record with no measured radius has no ratio to measure over.
    let centre_of = |c: [f64; 3]| vec![recipient(AI_SERIAL, 0.0, c, None)];
    let flight = fly(
        &declared_exposure(false, true),
        ExposureBit::Sonic,
        None,
        Some(&centre_of),
    );
    assert!(flight.tick.exposures.is_empty());
    assert!(matches!(
        flight.tick.exposures_refused.as_slice(),
        [BlastExposureRefusal::MissingRadius { .. }]
    ));
}

/// The production producer reads the bit off a measured row: the record's
/// effect keys are the ones T452.R.05 measured, and a record with neither bit
/// carries no exposure. The facing flag the image passes to `0x42e840` is the
/// `FLASH` bit (`shr 0xb`, `and 1` at `0x4ba576`), so a record declaring both
/// is faced and `FLASH` wins.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_incap_hitpath_retail_the_measured_records_state_their_bit() {
    let dir = std::env::var_os("CS_GAME_DIR").expect(
        "CS_GAME_DIR is not set: this measurement needs the original installation (capability \
         `retail`)",
    );
    let table = read_original_component_table(std::path::Path::new(&dir))
        .expect("the component table reads");
    let mut with_bit: Vec<(String, ExposureBit)> = table
        .rows
        .iter()
        .filter_map(|row: &OriginalComponentLaunch| {
            measured_exposure_bit(row).map(|bit| (row.record.clone(), bit))
        })
        .collect();
    with_bit.sort_by(|left, right| left.0.cmp(&right.0));
    // The three records #2172 measured as declaring an effect bit. `wep_08` is
    // the `SONIC`-only record, and the two `FLASH` records are faced — the
    // image's facing flag is the `FLASH` bit, so a record declaring both would
    // be faced too.
    assert_eq!(
        with_bit,
        vec![
            ("wep_08".to_owned(), ExposureBit::Sonic),
            ("wep_09".to_owned(), ExposureBit::Flash),
            ("wep_15".to_owned(), ExposureBit::Flash),
        ]
    );
    // A rocket with no effect bit carries none.
    let plain = table.row("wep_05").expect("wep_05 is classified");
    assert_eq!(measured_exposure_bit(plain), None);
}

/// The bytes #2172 left open: the `FLASH` facing term dot-multiplies the
/// victim's forward axis with a **normalised** victim-minus-source offset.
/// `0x42e8ed` hands the offset to `0x422690`, which divides it by its own
/// length in place and returns the length the caller discards at `0x42e8f6`;
/// `0x42e94e` compares the dot with the `0.5` at `0x6032e0`, which is the
/// `clamp(2 * dot, 0, 1)` the finding states.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_incap_hitpath_the_image_pins_the_normalised_facing_term() {
    let path = std::env::var_os("CS_ENGINE_IMAGE").expect(
        "CS_ENGINE_IMAGE is not set: this measurement needs the owner-supplied decrypted \
         executable",
    );
    let image = std::fs::read(&path).expect("the engine image reads");
    const IMAGE_BASE: u64 = 0x40_0000;
    let at = |va: u64, len: usize| {
        let offset = usize::try_from(va - IMAGE_BASE).expect("an offset");
        &image[offset..offset + len]
    };
    assert_eq!(
        at(0x0042_e8ed, 10),
        &[0x8d, 0x45, 0xe8, 0x50, 0xe8, 0x9a, 0x3d, 0xff, 0xff, 0xdd],
        "the exposure hands the offset vector to 0x422690"
    );
    assert_eq!(
        at(0x0042_26be, 27),
        &[
            0xd9, 0xe8, 0xde, 0xf1, 0xd9, 0xc0, 0xd8, 0x09, 0xd9, 0xc1, 0xd8, 0x49, 0x04, 0xd9,
            0xca, 0xd8, 0x49, 0x08, 0xd9, 0xc9, 0xd9, 0x19, 0xd9, 0xc9, 0xd9, 0x59, 0x04,
        ],
        "0x422690 divides the vector by its length in place (fdivrp st(1), then three fmul/stp)"
    );
    assert_eq!(
        at(0x0042_26b2, 12),
        &[
            0xd9, 0x55, 0xfc, 0xf7, 0x45, 0xfc, 0xff, 0xff, 0xff, 0x7f, 0x74, 0x23
        ],
        "a zero-length vector skips the division and stays zero"
    );
    assert_eq!(
        at(0x0042_e94e, 6),
        &[0xd9, 0x45, 0x08, 0xd8, 0x1d, 0xe0],
        "the dot is compared with the 0.5 at 0x6032e0"
    );
    assert_eq!(
        at(0x0060_32e0, 4),
        &[0x00, 0x00, 0x00, 0x3f],
        "0.5: past half the facing dot the scale is one"
    );
    assert_eq!(
        at(0x0060_411c, 4),
        &[0x9a, 0x99, 0x19, 0x3f],
        "0.6: the full-intensity ratio"
    );
    assert_eq!(
        at(0x0060_40a8, 4),
        &[0x00, 0x00, 0x20, 0x40],
        "2.5: the falloff slope"
    );
    assert_eq!(
        at(0x0060_36bc, 4),
        &[0x00, 0x00, 0xa0, 0x40],
        "5.0: seconds at full intensity"
    );
    assert_eq!(
        at(0x004b_a576, 10),
        &[0xc1, 0xe9, 0x0b, 0x80, 0xe1, 0x01, 0x33, 0xdb, 0x88, 0x4d],
        "the per-hit caller's facing flag is the FLASH bit"
    );
}
