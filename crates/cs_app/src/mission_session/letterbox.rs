//! The black letterbox bars of the M01 launch cinematic (task #1664
//! `PARITY-M01-LC-LETTERBOX`, slice iii of #1642).
//!
//! **Measured.** In the owner's three Drydock frames of the retail 1.02 launch
//! cinematic (1024x768: `d1`, `d3`, `d5`, see
//! `docs/findings/2026-10-18-m01-lc-letterbox-geometry.md`) rows `0..90` and
//! `675..768` are exactly black (every channel 0) and the picture fills rows
//! `90..675` over the full width. The bars are therefore **90 rows on top and
//! 93 rows at the bottom**, not symmetric. The same rows in all three frames
//! mean the bars were fully in place from the first captured frame on; whether
//! they slide in, and when they leave, is **unmeasured** (no frame shows a
//! partial bar) and stays unknown.
//!
//! The bars are drawn only while [`LaunchLetterbox`] is active. The resource is
//! the session's flag; the cinematic phase (#1663) is what will switch it.
//! Like the cockpit HUD, the rows scale with the largest 4:3 rectangle in the
//! window ([`super::safe_area`]) and the bars extend to the window edges.

use bevy::prelude::{
    App, BackgroundColor, Color, Commands, Component, GlobalZIndex, Node, Plugin, PositionType,
    Query, Res, Resource, Startup, Update, Val, Visibility, Window, With, default,
};

use super::cockpit_hud::safe_area;

/// Width of the measured reference frames.
pub const REF_WIDTH: u32 = 1024;
/// Height of the measured reference frames.
pub const REF_HEIGHT: u32 = 768;
/// First picture row of the measured frames: rows above it are the top bar.
pub const PICTURE_TOP: u32 = 90;
/// First row of the bottom bar in the measured frames (the picture ends
/// before it).
pub const PICTURE_END: u32 = 675;

/// Whether the launch cinematic's bars are drawn.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LaunchLetterbox {
    active: bool,
}

impl LaunchLetterbox {
    /// Switches the bars on or off.
    pub fn set_active(&mut self, active: bool) {
        self.active = active;
    }

    /// Whether the bars are on.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.active
    }
}

/// The bar heights in window pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LetterboxBars {
    /// Height of the top bar, from the window's top edge.
    pub top: f32,
    /// Height of the bottom bar, up to the window's bottom edge.
    pub bottom: f32,
}

/// The bars for a `width` x `height` window: the measured rows scaled to the
/// largest 4:3 rectangle, extended to the window edges outside it.
#[must_use]
pub fn letterbox_bars(width: f32, height: f32) -> LetterboxBars {
    let (_, area_h) = safe_area(width, height);
    if area_h <= 0.0 {
        return LetterboxBars {
            top: 0.0,
            bottom: 0.0,
        };
    }
    let margin = (height - area_h) / 2.0;
    let row = area_h / REF_HEIGHT as f32;
    LetterboxBars {
        top: margin + PICTURE_TOP as f32 * row,
        bottom: margin + (REF_HEIGHT - PICTURE_END) as f32 * row,
    }
}

#[derive(Component, Clone, Copy)]
enum Bar {
    Top,
    Bottom,
}

/// Installs the letterbox bars (windowed compositions only).
#[derive(Clone, Copy, Debug, Default)]
pub struct MissionLetterboxPlugin;

impl Plugin for MissionLetterboxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LaunchLetterbox>()
            .add_systems(Startup, spawn_bars)
            .add_systems(Update, fit_bars);
    }
}

fn spawn_bars(mut commands: Commands) {
    for bar in [Bar::Top, Bar::Bottom] {
        let mut node = Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            width: Val::Percent(100.0),
            height: Val::Px(0.0),
            ..default()
        };
        match bar {
            Bar::Top => node.top = Val::Px(0.0),
            Bar::Bottom => node.bottom = Val::Px(0.0),
        }
        commands.spawn((
            node,
            BackgroundColor(Color::BLACK),
            // Above the HUD and every other overlay: the original's bars are
            // part of the picture.
            GlobalZIndex(1000),
            Visibility::Hidden,
            bar,
        ));
    }
}

fn fit_bars(
    flag: Res<LaunchLetterbox>,
    windows: Query<&Window>,
    mut bars: Query<(&Bar, &mut Node, &mut Visibility), With<Bar>>,
) {
    let Ok(window) = windows.single() else { return };
    let rows = letterbox_bars(window.width(), window.height());
    for (bar, mut node, mut visibility) in &mut bars {
        node.height = Val::Px(match bar {
            Bar::Top => rows.top,
            Bar::Bottom => rows.bottom,
        });
        *visibility = if flag.is_active() {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}
