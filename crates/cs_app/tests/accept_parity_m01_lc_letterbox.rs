//! Task #1664 `PARITY-M01-LC-LETTERBOX`: the launch cinematic's black bars,
//! with the rows measured from the owner's 1024x768 Drydock frames
//! (`docs/findings/2026-10-18-m01-lc-letterbox-geometry.md`): top bar rows
//! `0..90`, bottom bar rows `675..768`. Drawn only while the flag is on.

use bevy::prelude::{App, Node, Val, Visibility, Window};
use bevy::window::WindowResolution;
use cs_app::mission_session::{
    LaunchLetterbox, MissionLetterboxPlugin, PICTURE_END, PICTURE_TOP, letterbox_bars,
};

#[test]
fn accept_parity_m01_lc_letterbox_rows_at_the_measured_frame_size() {
    let bars = letterbox_bars(1024.0, 768.0);
    assert_eq!(bars.top, 90.0, "rows 0..90 are black");
    assert_eq!(bars.bottom, 93.0, "rows 675..768 are black");
    assert_eq!((PICTURE_TOP, PICTURE_END), (90, 675));
}

#[test]
fn accept_parity_m01_lc_letterbox_scales_with_the_four_three_area() {
    // Twice the frame: the rows double.
    let twice = letterbox_bars(2048.0, 1536.0);
    assert_eq!((twice.top, twice.bottom), (180.0, 186.0));
    // A 16:9 window keeps the picture rows of its 4:3 area, full width.
    let wide = letterbox_bars(1920.0, 1080.0);
    let row = 1080.0 / 768.0;
    assert!((wide.top - 90.0 * row).abs() < 1e-3);
    assert!((wide.bottom - 93.0 * row).abs() < 1e-3);
    // A 4:3-or-narrower window: the margin above and below the area is bar.
    let tall = letterbox_bars(1024.0, 1024.0);
    assert!((tall.top - (128.0 + 90.0)).abs() < 1e-3);
    assert!((tall.bottom - (128.0 + 93.0)).abs() < 1e-3);
    // A degenerate window draws nothing.
    let none = letterbox_bars(0.0, 0.0);
    assert_eq!((none.top, none.bottom), (0.0, 0.0));
}

fn bars(app: &mut App) -> Vec<(f32, bool)> {
    let world = app.world_mut();
    let mut query = world.query::<(&Node, &Visibility)>();
    let mut found: Vec<(f32, bool)> = query
        .iter(world)
        .map(|(node, visibility)| {
            let Val::Px(height) = node.height else {
                panic!("bar height is in pixels")
            };
            (height, *visibility != Visibility::Hidden)
        })
        .collect();
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    found
}

#[test]
fn accept_parity_m01_lc_letterbox_is_drawn_only_while_the_flag_is_on() {
    let mut app = App::new();
    app.add_plugins(bevy::MinimalPlugins);
    app.world_mut().spawn(Window {
        resolution: WindowResolution::new(1024, 768),
        ..Default::default()
    });
    app.add_plugins(MissionLetterboxPlugin);

    app.update();
    assert_eq!(
        bars(&mut app),
        vec![(90.0, false), (93.0, false)],
        "off by default: both bars exist, sized, hidden"
    );

    app.world_mut()
        .resource_mut::<LaunchLetterbox>()
        .set_active(true);
    app.update();
    assert_eq!(bars(&mut app), vec![(90.0, true), (93.0, true)]);

    app.world_mut()
        .resource_mut::<LaunchLetterbox>()
        .set_active(false);
    app.update();
    assert_eq!(bars(&mut app), vec![(90.0, false), (93.0, false)]);
}
