//! #2251 (`GROUNDBLOW-GRACE-WRITER`): the `V+0xac` collision grace, written on
//! both aircraft of an aircraft-into-aircraft contact and fed to the ground
//! blow's gate.
//!
//! Task test prefix: `accept_groundblow_grace_`. The rule is static
//! (`docs/findings/2026-11-01-groundblow-gates.md`): the contact response sets
//! `V+0xac = now + 1.0` on both vehicles, and a non-player's ground blow is
//! skipped while `now < V+0xac`. The scene is synthetic (no retail data): two
//! bodies flying the original law with synthetic parameters, overlapping at
//! the start, a ground-blow probe handed to both every tick, and the law's own
//! angular momentum as the witness of whether the blow ran.
use bevy::prelude::*;
use cs_app::physics::{BodyMode, BodySpec, spawn_body};
use cs_app::playtest::collision_grace::{COLLISION_GRACE_S, CollisionGrace};
use cs_app::playtest::scene::PlaytestOriginalFlight;
use cs_app::playtest::world_sweep::GroundBlowProbes;
use cs_app::playtest::{OriginalFlightSet, fixed_ticks, fixed_timestep_s, headless_app_with};
use cs_sim::collision::{CollisionLayer, ShapeClass};
use cs_sim::flight::original::OriginalState;
use cs_sim::flight::original::{FORCE_TO_ACCEL, SegmentProbeHit, SegmentProbeOwner};
use cs_sim::flight::{FlightInput, OriginalAirframe, OriginalFlightModel, OriginalGlobals};
use cs_types::space::Quaternion;

fn synthetic_law() -> OriginalFlightModel {
    let airframe = OriginalAirframe::from_values(&[
        ("roll_torque", 7.5),
        ("pitch_torque", 3.3),
        ("rudder_torque", 2.0),
        ("level_off_rate", 1.0),
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

/// A hit with a non-zero torque term, as the law's own tests use.
fn probe() -> SegmentProbeHit {
    SegmentProbeHit {
        distance_m: 100.0,
        normal: [0.0, 300.0, 400.0],
        owner: SegmentProbeOwner::World,
    }
}

/// Whether the probes are handed out yet: the contact is reported before any
/// blow runs, so the momentum is a clean witness of the gate.
#[derive(Resource)]
struct ProbesArmed {
    ai: bool,
    player: bool,
}

/// Hands every flying record a probe each tick, after the world sweep's fill
/// and before the law steps.
fn hand_out_probes(
    armed: Res<ProbesArmed>,
    mut probes: ResMut<GroundBlowProbes>,
    records: Query<(Entity, &PlaytestOriginalFlight)>,
) {
    probes.hits = records
        .iter()
        .filter(|(_, flight)| {
            if flight.is_player {
                armed.player
            } else {
                armed.ai
            }
        })
        .map(|(entity, _)| (entity, probe()))
        .collect();
}

struct Scene {
    app: App,
    ai: Entity,
    player: Entity,
}

fn spawn_aircraft(app: &mut App, is_player: bool, position_m: [f32; 3]) -> Entity {
    let model = synthetic_law();
    let spec = BodySpec {
        layer: CollisionLayer::Aircraft,
        shape: ShapeClass::Solid,
        mode: BodyMode::Dynamic,
        mass_kg: (model.airframe.veh_weight / FORCE_TO_ACCEL) as f32,
        half_extents_m: [6.0, 1.0, 5.0],
        position_m,
        linear_velocity_m_s: [0.0; 3],
    };
    let entity = spawn_body(app.world_mut(), &spec).expect("the body spawns");
    let mut state = OriginalState::at(f64::from(position_m[1]), Quaternion::IDENTITY, 0.0, 1.0e9);
    state.position_m = position_m.map(f64::from);
    app.world_mut()
        .entity_mut(entity)
        .insert(PlaytestOriginalFlight {
            model,
            state,
            command: FlightInput::NEUTRAL,
            hold_attitude: false,
            is_player,
            provenance: "synthetic fixture: the original law for the grace writer",
        });
    entity
}

/// Two aircraft far above the scene's ground, `separation_m` apart along X:
/// 5 m overlaps their boxes, 100 m keeps them clear.
fn scene(separation_m: f32) -> Scene {
    let mut app = headless_app_with(|app| {
        app.insert_resource(ProbesArmed {
            ai: false,
            player: false,
        });
        app.add_systems(
            FixedUpdate,
            hand_out_probes
                .after(OriginalFlightSet::GroundBlowFill)
                .before(OriginalFlightSet::OriginalFlight),
        );
    });
    app.update();
    let ai = spawn_aircraft(&mut app, false, [4000.0, 900.0, 0.0]);
    let player = spawn_aircraft(&mut app, true, [4000.0 + separation_m, 900.0, 0.0]);
    Scene { app, ai, player }
}

fn momentum(app: &App, entity: Entity) -> [f64; 3] {
    app.world()
        .get::<PlaytestOriginalFlight>(entity)
        .expect("a flying record")
        .state
        .angular_momentum_world
}

fn grace(app: &App, entity: Entity) -> Option<f64> {
    app.world()
        .get::<CollisionGrace>(entity)
        .map(|grace| grace.until_s)
}

/// A scene that is never handed a probe, flown in lockstep with `probed`: the
/// witness of a blow is the difference between the two, so no threshold on the
/// law's other terms is needed.
fn lockstep(separation_m: f32) -> (Scene, Scene) {
    let probed = scene(separation_m);
    let bare = scene(separation_m);
    (probed, bare)
}

/// Hands the probe to the seats named from now on. The seats are armed one at
/// a time where the other seat's blow would move the body the contact solver
/// shares with it.
fn arm(scene: &mut Scene, ai: bool, player: bool) {
    *scene.app.world_mut().resource_mut::<ProbesArmed>() = ProbesArmed { ai, player };
}

fn differs(probed: &Scene, bare: &Scene, pick: fn(&Scene) -> Entity) -> bool {
    momentum(&probed.app, pick(probed)) != momentum(&bare.app, pick(bare))
}

fn ai_of(scene: &Scene) -> Entity {
    scene.ai
}

fn player_of(scene: &Scene) -> Entity {
    scene.player
}

#[test]
fn accept_groundblow_grace_control_without_contact_the_ai_blows_at_once() {
    let (mut probed, mut bare) = lockstep(100.0);
    probed.app.update();
    bare.app.update();
    arm(&mut probed, true, true);
    probed.app.update();
    bare.app.update();
    assert!(grace(&probed.app, probed.ai).is_none());
    assert!(grace(&probed.app, probed.player).is_none());
    assert!(
        differs(&probed, &bare, ai_of),
        "the probe makes the AI blow"
    );
    assert!(differs(&probed, &bare, player_of));
}

#[test]
fn accept_groundblow_grace_contact_gates_the_ai_for_one_second_and_not_the_player() {
    let (mut probed, mut bare) = lockstep(5.0);
    let dt_s = fixed_timestep_s(&probed.app);
    probed.app.update();
    bare.app.update();

    // (a) both aircraft carry the same deadline: contact time + 1.0 s.
    let ai_until = grace(&probed.app, probed.ai).expect("the AI got a grace");
    let player_until = grace(&probed.app, probed.player).expect("the player got a grace");
    assert_eq!(ai_until, player_until);
    let contact_tick = (ai_until - COLLISION_GRACE_S) / dt_s;
    assert!((contact_tick - contact_tick.round()).abs() < 1e-6);
    assert!(contact_tick >= 1.0 && contact_tick <= fixed_ticks(&probed.app) as f64);

    // (b) with a probe handed to the AI of one run, its blow stays off through
    // every tick before the deadline.
    arm(&mut probed, true, false);
    let mut frames_inside = 0;
    while (fixed_ticks(&probed.app) as f64 + 2.0) * dt_s < ai_until {
        probed.app.update();
        bare.app.update();
        assert!(
            !differs(&probed, &bare, ai_of),
            "the AI blew inside the grace"
        );
        frames_inside += 1;
        assert!(frames_inside < 200);
    }
    assert!(frames_inside > 30, "the grace spans many frames");

    // The deadline passes and the AI's blow runs again.
    while (fixed_ticks(&probed.app) as f64) * dt_s < ai_until + 0.05 {
        probed.app.update();
        bare.app.update();
    }
    assert!(
        differs(&probed, &bare, ai_of),
        "the AI blows after the grace"
    );
}

#[test]
fn accept_groundblow_grace_player_blows_inside_its_own_grace() {
    let (mut probed, mut bare) = lockstep(5.0);
    probed.app.update();
    bare.app.update();
    let until = grace(&probed.app, probed.player).expect("the player got a grace");
    let dt_s = fixed_timestep_s(&probed.app);

    // (c) the same second, the probe on the player's seat: the player's blow
    // runs, because the law never gates a player by the grace.
    arm(&mut probed, false, true);
    probed.app.update();
    bare.app.update();
    assert!((fixed_ticks(&probed.app) as f64 + 2.0) * dt_s < until);
    assert!(differs(&probed, &bare, player_of));
}

/// A scene whose bodies carry `ai_until` / `player_until` before the first
/// frame, so the first contact meets vehicles already inside (or outside) a
/// grace.
fn scene_with_graces(ai_until: Option<f64>, player_until: Option<f64>) -> Scene {
    let mut scene = scene(5.0);
    for (entity, until_s) in [(scene.ai, ai_until), (scene.player, player_until)] {
        if let Some(until_s) = until_s {
            scene
                .app
                .world_mut()
                .entity_mut(entity)
                .insert(CollisionGrace { until_s });
        }
    }
    scene.app.update();
    scene
}

#[test]
fn accept_sweep_grace_skip_boundary_is_strictly_before_the_deadline() {
    use cs_app::playtest::collision_grace::sweep_skipped;
    let grace = CollisionGrace { until_s: 2.0 };
    assert!(sweep_skipped(Some(&grace), 1.999));
    assert!(!sweep_skipped(Some(&grace), 2.0));
    assert!(!sweep_skipped(None, 0.0));
}

#[test]
fn accept_sweep_grace_skip_contact_between_two_vehicles_in_grace_is_not_seen() {
    let far = 1.0e6;
    let scene = scene_with_graces(Some(far), Some(far));
    assert_eq!(grace(&scene.app, scene.ai), Some(far));
    assert_eq!(grace(&scene.app, scene.player), Some(far));
}

#[test]
fn accept_sweep_grace_skip_one_vehicle_outside_its_grace_still_sees_the_contact() {
    let far = 1.0e6;
    let scene = scene_with_graces(Some(far), None);
    let dt_s = fixed_timestep_s(&scene.app);
    let ai_until = grace(&scene.app, scene.ai).expect("the AI has a grace");
    let player_until = grace(&scene.app, scene.player).expect("the player got one");
    assert_eq!(ai_until, player_until);
    assert!(ai_until < far);
    assert!(ai_until - COLLISION_GRACE_S >= dt_s);
}
