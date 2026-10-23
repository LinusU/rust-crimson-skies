//! Acceptance for Rally #1744 `PARITY3-HAZE-FADE-AFTER-LAUNCH`: the launch
//! cinematic runs with the fog test off and the bound zone fog comes back at
//! the hand-over, the step the original's cinematic-mode pair performs
//! (`0x450550` writes fog mode 0; `0x450e30` writes mode 1 and clears the
//! cinematic flag `0x654120`, its only other store being `0x450562` —
//! `docs/findings/2026-10-23-m01-fog-update-static.md`, section 2). The
//! original touches no fog range, colour or altitude and nothing in the
//! image interpolates them, so there is no fade to test: the five-second
//! clearing the owner's frames show is the aircraft flying through the
//! constant zone fog.
//!
//! The members are synthetic: they drive the production
//! [`install_launch_cinematic`] wiring against a `DistanceFog` the way the
//! horizon module binds it, and fail when the parking system is removed.

use bevy::app::{App, FixedLast, Update};
use bevy::prelude::{Camera3d, Color, DistanceFog, Entity, FogFalloff};
use cs_app::mission_session::{LaunchCinematic, install_launch_cinematic};

fn camera_with_fog(app: &mut App, start: f32, end: f32) -> Entity {
    app.world_mut()
        .spawn((
            Camera3d::default(),
            DistanceFog {
                color: Color::srgb(0.79, 0.79, 0.79),
                directional_light_color: Color::NONE,
                directional_light_exponent: 0.0,
                falloff: FogFalloff::Linear { start, end },
            },
        ))
        .id()
}

fn falloff(app: &App, camera: Entity) -> (f32, f32) {
    match &app.world().get::<DistanceFog>(camera).expect("fog").falloff {
        FogFalloff::Linear { start, end } => (*start, *end),
        other => panic!("the zone fog is a linear falloff, not {other:?}"),
    }
}

#[test]
fn accept_parity3_haze_fade_after_launch_fog_returns_at_hand_over() {
    let mut app = App::new();
    install_launch_cinematic(&mut app, 3).expect("installs");
    // M01's ZONE1 record: `FOG_RANGES 1000 4500`, high tier scale 1.0.
    let camera = camera_with_fog(&mut app, 1000.0, 4500.0);

    for _ in 0..3 {
        app.update();
        assert_eq!(
            falloff(&app, camera),
            (1.0e9, 2.0e9),
            "mode 0: no fog test while the cinematic plays"
        );
        app.world_mut().run_schedule(FixedLast);
    }
    // The third tick ended the phase; the next update hands the bound fog
    // back, the step the exit `0x450e30` performs with mode 1.
    assert!(!app.world().resource::<LaunchCinematic>().is_playing());
    app.world_mut().run_schedule(Update);
    assert_eq!(
        falloff(&app, camera),
        (1000.0, 4500.0),
        "mode 1: the bound zone fog is restored unchanged at hand-over"
    );
}

#[test]
fn accept_parity3_haze_fade_after_launch_the_parked_value_is_the_bound_one() {
    // The step touches no fog parameter: what comes back at hand-over is the
    // falloff the world bound, byte for byte, not a recomputed one.
    let mut app = App::new();
    install_launch_cinematic(&mut app, 2).expect("installs");
    let camera = camera_with_fog(&mut app, 234.5, 6789.25);

    app.update();
    assert_eq!(falloff(&app, camera), (1.0e9, 2.0e9));
    for _ in 0..2 {
        app.world_mut().run_schedule(FixedLast);
    }
    app.world_mut().run_schedule(Update);
    assert_eq!(falloff(&app, camera), (234.5, 6789.25));
}

#[test]
fn accept_parity3_haze_fade_after_launch_late_fog_is_parked_too() {
    // A camera that gains the fog while the cinematic is already playing is
    // parked on the next update and restored at the same hand-over.
    let mut app = App::new();
    install_launch_cinematic(&mut app, 2).expect("installs");
    app.update();
    let camera = camera_with_fog(&mut app, 600.0, 1700.0);
    app.update();
    assert_eq!(falloff(&app, camera), (1.0e9, 2.0e9));
    for _ in 0..2 {
        app.world_mut().run_schedule(FixedLast);
    }
    app.world_mut().run_schedule(Update);
    assert_eq!(falloff(&app, camera), (600.0, 1700.0));
}
