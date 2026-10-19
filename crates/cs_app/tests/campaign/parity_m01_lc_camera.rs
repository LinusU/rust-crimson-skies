//! PARITY-M01-LC-CAMERA (#1665): the launch cinematic plays the measured
//! camera script (finding `docs/findings/2026-10-19-m01-launch-camera.md`).
//!
//! The synthetic members run in CI. The retail members read `$CS_GAME_DIR`
//! (never write it) and fail loudly without it.

use bevy::math::Vec3;
use bevy::prelude::{GlobalTransform, Quat, Transform};
use cs_app::cinematics::launch_camera::{
    LaunchCameraScript, VariantDraw, measure_launch_camera, pick_variant,
};
use cs_app::mission_session::{
    CinematicMetrics, LaunchCameraStats, SmokeMeasurements, camera_in_world,
    evaluate_mission_smoke as evaluate,
};
use cs_types::install::RelativePath;

fn script(root: &str, draw: f32) -> LaunchCameraScript {
    let dir = std::env::var("CS_GAME_DIR").expect("a retail test needs CS_GAME_DIR to be set");
    let bytes = std::fs::read(std::path::Path::new(&dir).join("ZBD/C3/cam_anim.zbd"))
        .expect("the camera carrier reads");
    let path = RelativePath::new("zbd/c3/cam_anim.zbd").expect("a relative path");
    measure_launch_camera(&path, &bytes, root, &mut || draw).expect("the script measures")
}

fn forward(q: [f64; 4]) -> [f64; 3] {
    let [w, x, y, z] = q;
    // Column of the standard rotation matrix for local -Z, negated.
    [
        -(2.0 * (x * z + w * y)),
        -(2.0 * (y * z - w * x)),
        -(1.0 - 2.0 * (x * x + y * y)),
    ]
}

#[test]
fn accept_parity_m01_lc_camera_variant_draws_follow_the_chain_thresholds() {
    let mut low = || 0.1_f32;
    assert_eq!(
        pick_variant(&[0.2, 0.2, 0.2], &[0, 1, 2, 3], &mut low),
        Some(0)
    );
    let mut draws = [0.9_f32, 0.9, 0.1].into_iter();
    let mut draw = || draws.next().expect("a draw");
    assert_eq!(
        pick_variant(&[0.2, 0.2, 0.2], &[0, 1, 2, 3], &mut draw),
        Some(2)
    );
    let mut high = || 0.99_f32;
    assert_eq!(pick_variant(&[0.3], &[11, 12], &mut high), Some(12));
    let mut a = VariantDraw::new(3);
    let mut b = VariantDraw::new(3);
    for _ in 0..100 {
        let x = a.next_draw();
        assert!((0.0..1.0).contains(&x));
        assert_eq!(x, b.next_draw());
    }
}

#[test]
fn accept_parity_m01_lc_camera_pose_is_put_into_the_pandora_frame_without_its_scale() {
    // A frame turned 90 degrees about Y, at (10, 0, 0), scaled 3x.
    let frame = GlobalTransform::from(
        Transform::from_translation(Vec3::new(10.0, 0.0, 0.0))
            .with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2))
            .with_scale(Vec3::splat(3.0)),
    );
    let world = camera_in_world(&frame, [0.0, 0.0, -5.0], [1.0, 0.0, 0.0, 0.0]);
    // -Z of the frame is -X after the turn, then the translation.
    assert!((world.translation - Vec3::new(5.0, 0.0, 0.0)).length() < 1e-4);
    assert!((world.rotation * Vec3::NEG_Z - Vec3::NEG_X).length() < 1e-4);
    assert_eq!(world.scale, Vec3::ONE);
}

#[test]
fn accept_parity_m01_lc_camera_smoke_judges_the_cinematic_only_when_it_ran() {
    let mut m = SmokeMeasurements::default();
    assert!(
        evaluate(&m)
            .iter()
            .all(|c| c.id != "launch_cinematic_camera_played")
    );
    let ok = LaunchCameraStats {
        frames_driven: 10,
        frames_without_frame_node: 0,
        scenes_seen: 4,
        scenes_total: 4,
    };
    for (camera, shots, passes) in [
        (ok.clone(), 3, true),
        (ok.clone(), 0, false),
        (
            LaunchCameraStats {
                frames_without_frame_node: 1,
                ..ok.clone()
            },
            3,
            false,
        ),
        (
            LaunchCameraStats {
                scenes_seen: 3,
                ..ok.clone()
            },
            3,
            false,
        ),
        (
            LaunchCameraStats {
                frames_driven: 0,
                ..ok
            },
            3,
            false,
        ),
    ] {
        m.cinematic = Some(CinematicMetrics { camera, shots });
        let checks = evaluate(&m);
        let check = checks
            .iter()
            .find(|c| c.id == "launch_cinematic_camera_played")
            .expect("the check is present");
        assert_eq!(check.passed, passes, "{}", check.detail);
    }
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity_m01_lc_camera_retail_scenes_follow_the_measured_timeline() {
    let long = script("player_devastator", 0.0);
    let starts: Vec<(&str, f64)> = long
        .scenes()
        .iter()
        .map(|s| (s.animation.as_str(), (s.start * 10.0).round() / 10.0))
        .collect();
    assert_eq!(
        starts,
        [
            ("gi_scene1", 0.0),
            ("gi_scene2", 15.9),
            ("gi_1stperson", 28.3),
            ("gi_playerdrop", 38.2)
        ]
    );
    assert!((long.length - 40.15).abs() < 0.01);
    // The first draw takes each chain's first variant, the last its fallback.
    let blocks =
        |s: &LaunchCameraScript| s.scenes().iter().map(|x| x.track.block).collect::<Vec<_>>();
    assert_eq!(blocks(&long), [0, 4, 9, 11]);
    assert_eq!(blocks(&script("player_devastator", 1.0)), [3, 6, 9, 12]);
    // Balmoral skips the first-person shot.
    let short = script("player_balmoral", 0.0);
    assert_eq!(short.scenes().len(), 3);
    assert!((short.length - 30.25).abs() < 0.01);
    // The pose at time zero is the stored first position of gi_cam1.
    let first = long.pose_at(0.0).expect("a pose");
    assert_eq!(first.position, [-155.705_87, -62.025_87, -768.973_2]);
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity_m01_lc_camera_retail_the_rotation_looks_along_minus_z_at_the_pandora() {
    // Measured: in the opening scenes the camera's -Z axis, with the stored
    // quaternion read as a column-vector rotation, points at the Pandora's
    // origin (the frame's origin) to within 19 degrees (dot > 0.95; it is > 0.99 for most of each scene), every half second.
    for (draw, scene) in [(0.0, 0), (1.0, 0), (0.0, 1)] {
        let s = script("player_devastator", draw);
        let played = &s.scenes()[scene];
        let mut t = played.start + 0.1;
        while t < played.stop - 0.1 {
            let p = s.pose_at(t).expect("a pose");
            assert_eq!(p.scene, scene);
            let f = forward(p.quaternion);
            let n = p
                .position
                .iter()
                .map(|c| f64::from(*c).powi(2))
                .sum::<f64>()
                .sqrt();
            let dot: f64 = (0..3).map(|i| f[i] * -f64::from(p.position[i]) / n).sum();
            // The first variant of scene one and scene two aim at the origin
            // the whole way; the last variant of scene one sweeps past it.
            if draw == 0.0 {
                assert!(dot > 0.95, "t={t}: dot {dot}");
            }
            t += 0.5;
        }
    }
}
