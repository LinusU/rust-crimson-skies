//! Does an accepted shot draw the measured muzzle burst, not the designed
//! stand-in cube? (#2106) Task test prefix: `accept_f27_c_fx_orig_draw_c_`.
//!
//! The burst's quads, binding, pose and one-tick lifetime are measured in
//! `docs/findings/2026-10-24-f27-c-fx-orig-muzzle-effect-binding.md` and
//! `docs/findings/2026-10-25-f27-c-fx-orig-muzzle-burst-pose.md`. The synthetic
//! tests use a newly authored burst; the retail test reads `$CS_GAME_DIR`
//! (never writes it) and fails loudly without it.

use std::path::PathBuf;

use bevy::asset::Assets;
use bevy::input::ButtonInput;
use bevy::input::keyboard::KeyCode;
use bevy::mesh::{Mesh, VertexAttributeValues};
use bevy::prelude::{App, Children, Entity, Mesh3d, Quat, Transform, Vec3, World};

use cs_app::mission_session::muzzle_burst_art::load_muzzle_burst_art;
use cs_app::mission_session::{
    MUZZLE_FLASH_LIFETIME_TICKS, MuzzleBurstArtResource, MuzzleDrawable, MuzzleFlash, WeaponVisual,
    WeaponVisualKind, build_headless, teardown,
};
use cs_app::physics::PhysicsTickLedger;
use cs_app::scene::SceneGeneration;
use cs_app::weapon_effects::{MuzzleEffectSpawn, spawn_muzzle_effect};
use cs_app::weapons::WeaponEffect;
use cs_sim::weapons::ProjectileId;
use cs_types::Tick;
use cs_types::space::WorldPosition;

use crate::campaign::f27_c_fx_render::synthetic_art;
use crate::campaign::vs_m01_rt_window::{Scratch, synthetic_stage};

fn composed(
    label: &str,
    art: Option<cs_app::mission_session::muzzle_burst_art::MuzzleBurstArt>,
) -> (Scratch, App) {
    let scratch = Scratch::new(label);
    let stage = synthetic_stage(scratch.path());
    let mut app = build_headless(&stage).expect("the synthetic stage composes");
    if let Some(art) = art {
        app.insert_resource(MuzzleBurstArtResource(art));
    }
    app.update();
    (scratch, app)
}

fn records(app: &App) -> Vec<Entity> {
    app.world()
        .iter_entities()
        .filter(|entity| entity.contains::<MuzzleEffectSpawn>())
        .map(|entity| entity.id())
        .collect()
}

fn quads_of(app: &App, record: Entity) -> Vec<Entity> {
    app.world()
        .get::<Children>(record)
        .map(|children| {
            children
                .iter()
                .copied()
                .filter(|child| {
                    app.world()
                        .get::<WeaponVisual>(*child)
                        .is_some_and(|v| v.kind == WeaponVisualKind::MuzzleFlash)
                })
                .collect()
        })
        .unwrap_or_default()
}

fn fire_once(app: &mut App) {
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Space);
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::Space);
    assert!(!records(app).is_empty(), "the press was accepted");
}

fn positions(app: &App, quad: Entity) -> Vec<[f32; 3]> {
    let handle = app.world().get::<Mesh3d>(quad).expect("a mesh").0.clone();
    let meshes = app.world().resource::<Assets<Mesh>>();
    let mesh = meshes.get(&handle).expect("the mesh is stored");
    match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
        Some(VertexAttributeValues::Float32x3(values)) => values.clone(),
        other => panic!("positions expected, got {other:?}"),
    }
}

/// **A record for the gun's shot draws the art's own quads at scale 1 — and no
/// cube.**
#[test]
fn accept_f27_c_fx_orig_draw_c_a_shot_draws_the_measured_quads_and_no_cube() {
    let art = synthetic_art();
    let (_scratch, mut app) = composed("f27-c-fx-orig-draw-c-quads", Some(art.clone()));
    fire_once(&mut app);
    for record in records(&app) {
        let quads = quads_of(&app, record);
        assert_eq!(quads.len(), art.flash.quads.len() + art.spin.quads.len());
        let expected: Vec<[[f32; 3]; 4]> = art
            .flash
            .quads
            .iter()
            .chain(&art.spin.quads)
            .map(|quad| quad.corners)
            .collect();
        for (quad, corners) in quads.iter().zip(&expected) {
            assert_eq!(&positions(&app, *quad), corners, "stored corners, scale 1");
            let transform = app.world().get::<Transform>(*quad).expect("pose");
            assert_eq!(transform.scale, Vec3::ONE);
            assert_eq!(transform.translation, Vec3::ZERO);
            assert_eq!(positions(&app, *quad).len(), 4, "a quad, not a cuboid");
        }
        // The first quad is the flash and is unrolled; spin quads carry the
        // shot's one measured roll about z.
        let roll = match &app.world().get::<MuzzleFlash>(record).unwrap().drawable {
            MuzzleDrawable::MeasuredBurst { spin_roll_deg, .. } => *spin_roll_deg,
            other => panic!("measured burst expected, got {other:?}"),
        };
        assert!(art.spin_rolls_deg.contains(&roll));
        let flash = app.world().get::<Transform>(quads[0]).unwrap();
        assert_eq!(flash.rotation, Quat::IDENTITY);
        let spin = app.world().get::<Transform>(quads[1]).unwrap();
        assert!(
            spin.rotation
                .angle_between(Quat::from_rotation_z(roll.to_radians()))
                < 1e-5
        );
    }
    teardown(&mut app);
}

/// **The burst is drawn on the shot's tick and released on the next.**
#[test]
fn accept_f27_c_fx_orig_draw_c_the_burst_lives_one_tick() {
    assert_eq!(MUZZLE_FLASH_LIFETIME_TICKS, 1);
    let (_scratch, mut app) = composed("f27-c-fx-orig-draw-c-life", Some(synthetic_art()));
    fire_once(&mut app);
    let record = records(&app)[0];
    let born = app.world().get::<MuzzleEffectSpawn>(record).unwrap().at.0;
    let flash = app.world().get::<MuzzleFlash>(record).unwrap();
    assert_eq!(flash.expires_at, born + 1);
    assert!(!quads_of(&app, record).is_empty());
    let mut guard = 0;
    while app.world().get_entity(record).is_ok() {
        app.update();
        guard += 1;
        assert!(guard < 10, "the burst must be released");
    }
    assert!(
        app.world().resource::<PhysicsTickLedger>().ticks > born,
        "released no earlier than the next tick"
    );
    teardown(&mut app);
}

/// **With no measured art installed nothing is drawn for the shot: there is no
/// designed substitute.**
#[test]
fn accept_f27_c_fx_orig_draw_c_no_art_draws_nothing() {
    let (_scratch, mut app) = composed("f27-c-fx-orig-draw-c-none", None);
    fire_once(&mut app);
    for record in records(&app) {
        assert!(matches!(
            app.world().get::<MuzzleFlash>(record).unwrap().drawable,
            MuzzleDrawable::NotDrawn { .. }
        ));
        assert!(quads_of(&app, record).is_empty());
    }
    teardown(&mut app);
}

/// **The record's pose puts the node's local -Z on the fire direction.**
#[test]
fn accept_f27_c_fx_orig_draw_c_the_record_pose_points_minus_z_along_the_shot() {
    // An id the mission itself declared, taken from a real shot's record.
    let (_scratch, mut app) = composed("f27-c-fx-orig-draw-c-pose", None);
    fire_once(&mut app);
    let (id, shooter, mount) = {
        let spawn = app
            .world()
            .get::<MuzzleEffectSpawn>(records(&app)[0])
            .unwrap();
        (spawn.effect.clone(), spawn.shooter, spawn.mount.clone())
    };
    teardown(&mut app);
    for direction in [
        [0.0, 0.0, -1.0],
        [1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0],
        [-0.6, 0.0, 0.8],
        [0.0, 0.5, -0.8660254],
        [0.3, -0.4, -0.8660254],
    ] {
        let mut world = World::new();
        let effect = WeaponEffect {
            projectile: ProjectileId {
                session: 1,
                serial: 1,
            },
            shooter,
            mount: mount.clone(),
            effect: id.clone(),
            sound: id.clone(),
            origin: WorldPosition::try_new([1.0, 2.0, 3.0]).unwrap(),
            at: Tick(0),
            forward: Some(direction),
        };
        let record = spawn_muzzle_effect(&mut world, &effect, SceneGeneration(0));
        let pose = world.get::<Transform>(record).unwrap();
        assert_eq!(pose.translation, Vec3::new(1.0, 2.0, 3.0));
        let want = Vec3::new(
            direction[0] as f32,
            direction[1] as f32,
            direction[2] as f32,
        );
        let got = pose.rotation * Vec3::NEG_Z;
        assert!((got - want).length() < 1e-5, "{direction:?}: {got:?}");
        // Level fire keeps local +Y up.
        if direction[1] == 0.0 {
            assert!((pose.rotation * Vec3::Y - Vec3::Y).length() < 1e-5);
        }
    }
}

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("CS_GAME_DIR").unwrap_or_else(|_| {
            panic!("CS_GAME_DIR is not set: #2106 needs the retail capability")
        }),
    )
}

/// **The retail wep_50 burst, loaded from the installation, is what a shot
/// draws.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f27_c_fx_orig_draw_c_wep_50_draws_the_retail_slug_burst() {
    let art =
        load_muzzle_burst_art(&game_dir(), "c3", "wep_50").expect("the M01 group's art loads");
    assert_eq!(art.burst, "muzzle_burst_slug");
    let (_scratch, mut app) = composed("f27-c-fx-orig-draw-c-retail", Some(art.clone()));
    fire_once(&mut app);
    for record in records(&app) {
        let quads = quads_of(&app, record);
        assert_eq!(quads.len(), 4, "mb1 (1 quad) + mb_spinflame (3 quads)");
        assert_eq!(
            positions(&app, quads[0]),
            art.flash.quads[0].corners.to_vec()
        );
        let MuzzleDrawable::MeasuredBurst { weapon, burst, .. } =
            &app.world().get::<MuzzleFlash>(record).unwrap().drawable
        else {
            panic!("measured burst expected");
        };
        assert_eq!(
            (weapon.as_str(), burst.as_str()),
            ("wep_50", "muzzle_burst_slug")
        );
    }
    teardown(&mut app);
}
