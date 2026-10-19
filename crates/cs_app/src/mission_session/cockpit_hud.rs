//! The original-style in-flight HUD of the composed mission (task #1641
//! `PARITY-M01-HUD`), drawn instead of the developer overlay.
//!
//! What the owner's capture of the retail 1.02 flight shows (1024x768, 4:3) is
//! the layout this follows: a small compass strip at the top centre; a round
//! gauge cluster bottom-left (ROCKETS labelled `BOOM`, ALT, and a plane
//! silhouette that turns from green to red as the airframe is hit);
//! bottom-right a GUNS gauge (the ammunition name `SLUG` and the count) and
//! MPH; a small reticle in the screen centre; and world-space contact markers
//! in the original wording form `Name 7 o'clock`.
//!
//! **Measured vs designed.** The *presence, arrangement and wording form* are
//! taken from the capture description. Fonts, pixel positions, gauge sizes,
//! needle ranges, the altitude unit, the compass tick spacing, the plane
//! silhouette's shape and what the `/ 2000` beside the guns count means are
//! **unmeasured**: the values below are designed and recorded as unknown in
//! `docs/findings/2026-10-18-m01-cockpit-hud.md`. Every number shown is a live
//! read of the session (weapon, ordnance and damage authorities and the
//! playtest telemetry), never a constant.
//!
//! The HUD is laid out inside the largest 4:3 rectangle that fits the window
//! ([`safe_area`]), so a 16:9 window gets side bars and the gauges keep their
//! original proportions.

use bevy::prelude::{
    AlignItems, App, BackgroundColor, BorderColor, BorderRadius, Color, Commands, Component,
    Entity, FlexDirection, JustifyContent, Node, Plugin, PositionType, Query, Res, Startup, Text,
    TextColor, TextFont, UiRect, Update, Val, Window, With, default,
};
use bevy::text::FontSize;
use cs_content::config::{MessageCatalog, MessageLookup};
use cs_sim::damage::PartState;
use cs_types::net::{ActorId, SessionId};

use super::weapons::{MissionOrdnanceSession, MissionWeapons};
use crate::mission_actors::MissionGroundTarget;
use crate::playtest::PlaytestState;
use crate::ui::hud::{airframe_zones, ordnance_rows, weapon_gauge};
use crate::weapons::{SessionDamageResolver, WeaponSession};

/// The label on the rockets gauge, from the original capture.
pub const ROCKETS_LABEL: &str = "BOOM";
/// The ammunition name on the guns gauge, from the original capture.
pub const GUNS_LABEL: &str = "SLUG";
/// The original's window aspect: the HUD lives in a 4:3 safe area.
pub const SAFE_ASPECT: f32 = 4.0 / 3.0;
/// Metres per second to miles per hour (the gauge reads `MPH`).
const MPS_TO_MPH: f32 = 2.236_936;
/// Metres to feet. The original altitude gauge's unit is unmeasured; feet is
/// the designed pick that matches the `MPH` gauge's imperial family.
const M_TO_FT: f32 = 3.280_84;

/// The largest 4:3 rectangle inside a `width` x `height` window, as
/// `(width, height)`.
#[must_use]
pub fn safe_area(width: f32, height: f32) -> (f32, f32) {
    if width <= 0.0 || height <= 0.0 {
        return (0.0, 0.0);
    }
    if width / height >= SAFE_ASPECT {
        (height * SAFE_ASPECT, height)
    } else {
        (width, width / SAFE_ASPECT)
    }
}

/// The clock hour (1..=12, 12 ahead, 3 to the right) of a bearing relative to
/// the nose, in degrees, positive to the right.
#[must_use]
pub fn clock_hour(relative_deg: f32) -> u32 {
    let wrapped = relative_deg.rem_euclid(360.0);
    let hour = (wrapped / 30.0).round() as u32 % 12;
    if hour == 0 { 12 } else { hour }
}

/// The wording of a contact marker: the bare name when it is on screen,
/// `Name 7 o'clock` when it is not.
#[must_use]
pub fn marker_text(name: &str, off_screen_hour: Option<u32>) -> String {
    match off_screen_hour {
        Some(hour) => format!("{name} {hour} o'clock"),
        None => name.to_owned(),
    }
}

/// Pixel height of one contact label row at the label font size.
pub const LABEL_HEIGHT: f32 = 20.0;
/// Designed pixel width of one label character at the label font size. The
/// original's font metrics are unmeasured (`docs/findings/2026-10-18-m01-cockpit-hud.md`).
pub const LABEL_CHAR_WIDTH: f32 = 10.5;
/// Side of the square around the screen centre that labels keep clear of, so a
/// contact dead ahead never prints its name on the reticle.
pub const RETICLE_CLEARANCE: f32 = 48.0;
/// Gap kept between labels and from the window border.
const LABEL_GAP: f32 = 4.0;

/// The blue the original draws the objective marker in (the capture's text and
/// circle; the exact value is estimated from the screenshot, not measured).
pub const OBJECTIVE_BLUE: (f32, f32, f32) = (0.15, 0.15, 1.0);

/// The mission's objective site the marker points at: of the bound ground
/// targets whose `targets.zrd` row carries the `objective` marker and a
/// `description`, the one declared first.
///
/// **Designed selection.** M01 declares three such rows (`MSG_OBJ_SITE1..3`)
/// and the capture shows Site #1 first; which site the original shows when is
/// unmeasured (`ADD_OBJECTIVE_TARGET` never names a site in M01), so this
/// takes the first declared row and never retires it.
#[must_use]
pub fn objective_site<'a>(
    targets: impl IntoIterator<Item = &'a MissionGroundTarget>,
) -> Option<&'a MissionGroundTarget> {
    targets
        .into_iter()
        .filter(|target| target.objective && target.description.is_some())
        .filter_map(|target| Some((target.declared?, target)))
        .min_by_key(|(declared, _)| *declared)
        .map(|(_, target)| target)
}

/// The installation's text for a message name, or the name itself when no
/// catalog or no single string answers.
#[must_use]
pub fn message_text(name: &str, messages: Option<&MessageCatalog>) -> String {
    messages
        .and_then(|catalog| match catalog.resolve(name, None) {
            MessageLookup::Found { row, .. } => row.text.clone(),
            _ => None,
        })
        .unwrap_or_else(|| name.to_owned())
}

/// The objective marker's name: `[Examine] - Site #1`, the `help_label` text
/// in brackets then the `description` text, as the capture spells it.
#[must_use]
pub fn objective_marker_name(
    target: &MissionGroundTarget,
    messages: Option<&MessageCatalog>,
) -> Option<String> {
    let description = message_text(target.description.as_deref()?, messages);
    Some(match target.help_label.as_deref() {
        Some(help) => format!("[{}] - {description}", message_text(help, messages)),
        None => description,
    })
}

/// An axis-aligned label rectangle, `(left, top, width, height)` in pixels.
pub type LabelRect = (f32, f32, f32, f32);

/// The estimated pixel width of a label of `chars` characters.
#[must_use]
pub fn label_width(chars: usize) -> f32 {
    chars as f32 * LABEL_CHAR_WIDTH
}

/// The rectangle labels must keep clear of: the reticle's clearance square at
/// the centre of a `width` x `height` window.
#[must_use]
pub fn reticle_keep_out(width: f32, height: f32) -> LabelRect {
    (
        width / 2.0 - RETICLE_CLEARANCE / 2.0,
        height / 2.0 - RETICLE_CLEARANCE / 2.0,
        RETICLE_CLEARANCE,
        RETICLE_CLEARANCE,
    )
}

fn overlaps(a: LabelRect, b: LabelRect) -> bool {
    a.0 < b.0 + b.2 + LABEL_GAP
        && b.0 < a.0 + a.2 + LABEL_GAP
        && a.1 < b.1 + b.3 + LABEL_GAP
        && b.1 < a.1 + a.3 + LABEL_GAP
}

/// Places a `label_width` wide label as near the wanted `(x, y)` as the window
/// and the rectangles already `placed` allow, records it in `placed` and
/// returns its top-left. The label is clamped inside the window so it never
/// clips, then moved down (else up) in whole rows until it overlaps nothing.
pub fn place_label(
    wanted: (f32, f32),
    label_width: f32,
    window: (f32, f32),
    placed: &mut Vec<LabelRect>,
) -> (f32, f32) {
    let max_x = (window.0 - label_width - LABEL_GAP).max(LABEL_GAP);
    let max_y = (window.1 - LABEL_HEIGHT - LABEL_GAP).max(LABEL_GAP);
    let x = wanted.0.clamp(LABEL_GAP, max_x);
    let y0 = wanted.1.clamp(LABEL_GAP, max_y);
    let step = LABEL_HEIGHT + LABEL_GAP;
    let free = |y: f32, placed: &[LabelRect]| {
        let rect = (x, y, label_width, LABEL_HEIGHT);
        placed.iter().all(|other| !overlaps(rect, *other))
    };
    let rows = (max_y / step).ceil() as u32 + 1;
    let y = (0..rows)
        .map(|k| y0 + step * k as f32)
        .filter(|y| *y <= max_y)
        .chain(
            (1..rows)
                .map(|k| y0 - step * k as f32)
                .filter(|y| *y >= LABEL_GAP),
        )
        .find(|y| free(*y, placed))
        .unwrap_or(y0);
    placed.push((x, y, label_width, LABEL_HEIGHT));
    (x, y)
}

/// The eight compass letters at the multiples of 45 degrees.
fn compass_letter(degrees: i32) -> Option<&'static str> {
    match degrees.rem_euclid(360) {
        0 => Some("N"),
        45 => Some("NE"),
        90 => Some("E"),
        135 => Some("SE"),
        180 => Some("S"),
        225 => Some("SW"),
        270 => Some("W"),
        315 => Some("NW"),
        _ => None,
    }
}

/// The compass strip around `heading_deg`: ticks every 15 degrees over a
/// 90 degree window, lettered at the eight points, the cell nearest the
/// heading marked by the pointer line returned second.
#[must_use]
pub fn compass_strip(heading_deg: f32) -> (String, String) {
    let centre = ((heading_deg.rem_euclid(360.0) / 15.0).round() as i32) * 15;
    let mut strip = String::new();
    let mut pointer = String::new();
    for step in -3..=3 {
        let at = centre + step * 15;
        let cell = compass_letter(at).unwrap_or(".");
        strip.push_str(&format!("{cell:^4}"));
        pointer.push_str(if step == 0 { "  v " } else { "    " });
    }
    (strip, pointer)
}

/// The airframe silhouette tint for a damage fraction in `[0, 1]`: green
/// intact, red destroyed.
#[must_use]
pub fn damage_tint(fraction: f32) -> (f32, f32, f32) {
    let f = fraction.clamp(0.0, 1.0);
    (0.15 + 0.85 * f, 0.8 * (1.0 - f) + 0.1, 0.1)
}

/// The fraction of the airframe that has been hit: a damaged node counts half,
/// a destroyed node whole; unknown nodes are not counted.
#[must_use]
pub fn damage_fraction(states: &[PartState]) -> f32 {
    let known: Vec<&PartState> = states
        .iter()
        .filter(|state| **state != PartState::Unknown)
        .collect();
    if known.is_empty() {
        return 0.0;
    }
    let hit: f32 = known
        .iter()
        .map(|state| match state {
            PartState::Damaged => 0.5,
            PartState::Destroyed => 1.0,
            _ => 0.0,
        })
        .sum();
    hit / known.len() as f32
}

/// The live values the gauges show, read from the session each frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CockpitValues {
    /// Rockets left across the player's launchers, when an ordnance session
    /// serves the player.
    pub rockets: Option<u64>,
    /// Rounds left across the selected gun bank, when a weapon session serves
    /// the player.
    pub rounds: Option<u64>,
    /// Altitude in feet.
    pub altitude_ft: i64,
    /// Speed in miles per hour.
    pub speed_mph: i64,
    /// Heading in degrees.
    pub heading_deg: f32,
    /// Hit fraction of the airframe, `[0, 1]`.
    pub damage: f32,
}

/// Reads the values from the session authorities and the telemetry.
fn read_values(
    state: &PlaytestState,
    mission: Option<&MissionWeapons>,
    weapons: Option<&WeaponSession>,
    ordnance: Option<&MissionOrdnanceSession>,
    damage: Option<&SessionDamageResolver>,
) -> CockpitValues {
    let t = state.telemetry;
    let mut values = CockpitValues {
        altitude_ft: (t.altitude_m * M_TO_FT).round() as i64,
        speed_mph: (t.speed_m_s * MPS_TO_MPH).round() as i64,
        heading_deg: t.heading_deg,
        ..CockpitValues::default()
    };
    let Some(mission) = mission else {
        return values;
    };
    let session: SessionId = mission.session();
    let actor: ActorId = mission.player();
    // A foreign-generation authority is refused by the gather; the gauge then
    // shows no count rather than another session's.
    if let Ok(Some(gauge)) = weapon_gauge(weapons, session, actor) {
        values.rounds = Some(gauge.selected_rounds);
    }
    if let Ok(Some(rows)) = ordnance_rows(ordnance.map(|o| &o.0), session, actor) {
        values.rockets = Some(rows.iter().map(|row| row.remaining_units).sum());
    }
    if let Ok(Some(zones)) =
        airframe_zones(damage.map(SessionDamageResolver::resolver), session, actor)
    {
        let states: Vec<PartState> = zones.iter().map(|zone| zone.state).collect();
        values.damage = damage_fraction(&states);
    }
    values
}

/// The safe-area root every gauge hangs from.
#[derive(Component)]
struct SafeArea;
#[derive(Component)]
struct CompassStrip;
#[derive(Component)]
struct CompassPointer;
#[derive(Component)]
struct RocketsValue;
#[derive(Component)]
struct AltValue;
#[derive(Component)]
struct GunsValue;
#[derive(Component)]
struct MphValue;
#[derive(Component)]
struct DamagePart;

/// Installs the cockpit HUD (windowed compositions only).
#[derive(Clone, Copy, Debug, Default)]
pub struct MissionCockpitHudPlugin;

impl Plugin for MissionCockpitHudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_cockpit)
            .add_systems(Update, (fit_safe_area, update_cockpit));
    }
}

const HUD_GREEN: Color = Color::srgb(0.55, 1.0, 0.55);
const GAUGE_FILL: Color = Color::srgba(0.0, 0.05, 0.0, 0.55);

fn text(size: f32, value: &str, marker: impl Component) -> impl bevy::prelude::Bundle {
    (
        Text::new(value),
        TextFont {
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(HUD_GREEN),
        marker,
    )
}

/// One round gauge: a ring with a caption row and a value row inside.
fn gauge(parent: &mut bevy::prelude::ChildSpawnerCommands, caption: &str) -> Entity {
    parent
        .spawn((
            Node {
                width: Val::Percent(30.0),
                aspect_ratio: Some(1.0),
                border: UiRect::all(Val::Px(3.0)),
                border_radius: BorderRadius::MAX,
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BorderColor::all(HUD_GREEN),
            BackgroundColor(GAUGE_FILL),
        ))
        .with_children(|ring| {
            ring.spawn((
                Text::new(caption),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(HUD_GREEN),
            ));
        })
        .id()
}

fn spawn_cockpit(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Px(0.0),
                height: Val::Px(0.0),
                ..default()
            },
            SafeArea,
        ))
        .with_children(|area| {
            // Compass strip, top centre.
            area.spawn(Node {
                position_type: PositionType::Absolute,
                top: Val::Percent(1.5),
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|strip| {
                strip.spawn(text(16.0, "", CompassStrip));
                strip.spawn(text(14.0, "", CompassPointer));
            });
            // Reticle, screen centre.
            for (w, h) in [(18.0, 2.0), (2.0, 18.0)] {
                area.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Percent(50.0),
                        top: Val::Percent(50.0),
                        width: Val::Px(w),
                        height: Val::Px(h),
                        margin: UiRect {
                            left: Val::Px(-w / 2.0),
                            top: Val::Px(-h / 2.0),
                            ..default()
                        },
                        ..default()
                    },
                    BackgroundColor(HUD_GREEN),
                ));
            }
            // Bottom-left cluster: ROCKETS, ALT, damage silhouette.
            area.spawn(Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(2.0),
                bottom: Val::Percent(3.0),
                width: Val::Percent(36.0),
                column_gap: Val::Percent(2.0),
                ..default()
            })
            .with_children(|row| {
                let rockets = gauge(row, ROCKETS_LABEL);
                let alt = gauge(row, "ALT");
                let hull = gauge(row, "");
                row_value(row, rockets, text(20.0, "", RocketsValue));
                row_value(row, alt, text(18.0, "", AltValue));
                // Plane silhouette: fuselage, wing and tail bars.
                for (w, h, dy) in [(8.0, 40.0, 0.0), (44.0, 8.0, -4.0), (20.0, 6.0, 14.0)] {
                    let part = row
                        .spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                width: Val::Px(w),
                                height: Val::Px(h),
                                margin: UiRect::top(Val::Px(dy)),
                                ..default()
                            },
                            BackgroundColor(Color::srgb(0.15, 0.9, 0.1)),
                            DamagePart,
                        ))
                        .id();
                    row.commands().entity(hull).add_child(part);
                }
            });
            // Bottom-right cluster: GUNS, MPH.
            area.spawn(Node {
                position_type: PositionType::Absolute,
                right: Val::Percent(2.0),
                bottom: Val::Percent(3.0),
                width: Val::Percent(24.0),
                column_gap: Val::Percent(2.0),
                ..default()
            })
            .with_children(|row| {
                let guns = gauge(row, GUNS_LABEL);
                let mph = gauge(row, "MPH");
                row_value(row, guns, text(20.0, "", GunsValue));
                row_value(row, mph, text(18.0, "", MphValue));
            });
        });
}

/// Hangs a value text under a gauge ring.
fn row_value(
    row: &mut bevy::prelude::ChildSpawnerCommands,
    ring: Entity,
    value: impl bevy::prelude::Bundle,
) {
    let value = row.spawn(value).id();
    row.commands().entity(ring).add_child(value);
}

/// Sizes the safe-area root to the largest 4:3 rectangle in the window,
/// centred.
fn fit_safe_area(
    windows: Query<&Window>,
    phase: Option<bevy::prelude::Res<super::launch_cinematic::LaunchCinematic>>,
    mut areas: Query<(&mut Node, &mut bevy::prelude::Visibility), With<SafeArea>>,
) {
    let Ok(window) = windows.single() else { return };
    let (w, h) = safe_area(window.width(), window.height());
    // The launch cinematic holds the HUD back until it hands over (#1663).
    let held = phase.is_some_and(|phase| phase.is_playing());
    for (mut node, mut visibility) in &mut areas {
        *visibility = if held {
            bevy::prelude::Visibility::Hidden
        } else {
            bevy::prelude::Visibility::Inherited
        };
        node.left = Val::Px((window.width() - w) / 2.0);
        node.top = Val::Px((window.height() - h) / 2.0);
        node.width = Val::Px(w);
        node.height = Val::Px(h);
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn update_cockpit(
    state: Res<PlaytestState>,
    mission: Option<Res<MissionWeapons>>,
    weapons: Option<Res<WeaponSession>>,
    ordnance: Option<Res<MissionOrdnanceSession>>,
    damage: Option<Res<SessionDamageResolver>>,
    mut texts: Query<(
        &mut Text,
        Option<&CompassStrip>,
        Option<&CompassPointer>,
        Option<&RocketsValue>,
        Option<&AltValue>,
        Option<&GunsValue>,
        Option<&MphValue>,
    )>,
    mut hull: Query<&mut BackgroundColor, With<DamagePart>>,
) {
    let values = read_values(
        &state,
        mission.as_deref(),
        weapons.as_deref(),
        ordnance.as_deref(),
        damage.as_deref(),
    );
    let count = |n: Option<u64>| n.map_or_else(|| "--".to_owned(), |n| n.to_string());
    let (strip, pointer) = compass_strip(values.heading_deg);
    for (mut text, strip_tag, pointer_tag, rockets, alt, guns, mph) in &mut texts {
        let new = if strip_tag.is_some() {
            strip.clone()
        } else if pointer_tag.is_some() {
            pointer.clone()
        } else if rockets.is_some() {
            count(values.rockets)
        } else if alt.is_some() {
            values.altitude_ft.to_string()
        } else if guns.is_some() {
            count(values.rounds)
        } else if mph.is_some() {
            values.speed_mph.to_string()
        } else {
            continue;
        };
        if text.0 != new {
            text.0 = new;
        }
    }
    let (r, g, b) = damage_tint(values.damage);
    for mut fill in &mut hull {
        fill.0 = Color::srgb(r, g, b);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accept_parity_m01_hud_clock_hours_follow_the_original_form() {
        assert_eq!(clock_hour(0.0), 12);
        assert_eq!(clock_hour(90.0), 3);
        assert_eq!(clock_hour(180.0), 6);
        assert_eq!(clock_hour(-150.0), 7);
        assert_eq!(clock_hour(359.0), 12);
        assert_eq!(marker_text("Kestrel", Some(7)), "Kestrel 7 o'clock");
        assert_eq!(marker_text("Kestrel", None), "Kestrel");
    }

    #[test]
    fn accept_parity_m01_hud_safe_area_is_four_by_three_at_any_window() {
        let (w, h) = safe_area(1280.0, 720.0);
        assert!((w / h - SAFE_ASPECT).abs() < 1e-4 && (h - 720.0).abs() < 1e-3);
        let (w, h) = safe_area(800.0, 800.0);
        assert!((w - 800.0).abs() < 1e-3 && (w / h - SAFE_ASPECT).abs() < 1e-4);
        assert_eq!(safe_area(0.0, 10.0), (0.0, 0.0));
    }

    #[test]
    fn accept_parity_m01_hud_compass_letters_the_heading() {
        let (strip, pointer) = compass_strip(2.0);
        assert!(strip.contains('N') && pointer.contains('v'));
        let (strip, _) = compass_strip(315.0);
        assert!(strip.contains("NW"));
    }

    #[test]
    fn accept_parity_m01_hud_damage_gauge_turns_red_when_hit() {
        let intact = damage_fraction(&[PartState::Intact, PartState::Intact]);
        let hit = damage_fraction(&[PartState::Destroyed, PartState::Intact]);
        assert_eq!(intact, 0.0);
        assert_eq!(hit, 0.5);
        let (r0, g0, _) = damage_tint(intact);
        let (r1, g1, _) = damage_tint(1.0);
        assert!(g0 > r0 && r1 > g1);
        assert_eq!(damage_fraction(&[PartState::Unknown]), 0.0);
    }

    #[test]
    fn accept_parity_m01_hud_dev_overlay_flag_is_explicit_and_mission_only() {
        use crate::cli::{CliRequest, parse};
        let words = |w: &[&str]| w.iter().map(|x| (*x).to_owned()).collect::<Vec<_>>();
        let CliRequest::Mission(on) = parse(words(&[
            "--cs-path",
            "/i",
            "--mission",
            "M01",
            "--dev-overlay",
        ])) else {
            panic!("a mission request");
        };
        assert!(on.dev_overlay);
        let CliRequest::Mission(off) = parse(words(&["--cs-path", "/i", "--mission", "M01"]))
        else {
            panic!("a mission request");
        };
        assert!(!off.dev_overlay);
        assert!(matches!(
            parse(words(&["--playtest", "--dev-overlay"])),
            CliRequest::Invalid { .. }
        ));
    }

    fn site(declared: Option<usize>, objective: bool, text: Option<&str>) -> MissionGroundTarget {
        MissionGroundTarget {
            node: "n".to_owned(),
            declared,
            damage_actor: ActorId {
                session: SessionId::new(1).expect("nonzero"),
                serial: 1,
            },
            generation: crate::scene::SceneGeneration::default(),
            position_m: [0.0; 3],
            healthy: None,
            destroyed: None,
            coupling: Vec::new(),
            objective,
            other_target: false,
            description: text.map(str::to_owned),
            help_label: Some("MSG_OBJ_EXAMINE".to_owned()),
            roster: None,
            allegiance_open: None,
        }
    }

    #[test]
    fn accept_parity_m01_hud_objective_marker_picks_first_declared_site() {
        let rows = [
            site(Some(9), true, Some("MSG_OBJ_SITE3")),
            site(Some(7), true, Some("MSG_OBJ_SITE1")),
            site(Some(1), false, Some("MSG_OBJ_DESTROY")),
            site(None, true, Some("MSG_OBJ_SITE2")),
        ];
        let picked = objective_site(&rows).expect("a site");
        assert_eq!(picked.description.as_deref(), Some("MSG_OBJ_SITE1"));
        assert!(objective_site(&rows[2..3]).is_none());
        // Without a catalog the marker names the messages instead of text.
        assert_eq!(
            objective_marker_name(picked, None).as_deref(),
            Some("[MSG_OBJ_EXAMINE] - MSG_OBJ_SITE1")
        );
    }

    #[test]
    fn accept_parity_m01_hud_labels_never_overlap_or_clip() {
        let window = (1024.0, 768.0);
        let mut placed = vec![reticle_keep_out(window.0, window.1)];
        // Three contacts wanting the same right-edge spot, one dead ahead.
        let mut spots = Vec::new();
        for (wanted, chars) in [
            ((1020.0, 300.0), 12),
            ((1020.0, 300.0), 6),
            ((1010.0, 305.0), 20),
            ((512.0 + 14.0, 384.0 - 8.0), 7),
        ] {
            let width = label_width(chars);
            let (x, y) = place_label(wanted, width, window, &mut placed);
            assert!(x >= 0.0 && x + width <= window.0, "clipped horizontally");
            assert!(
                y >= 0.0 && y + LABEL_HEIGHT <= window.1,
                "clipped vertically"
            );
            spots.push((x, y, width, LABEL_HEIGHT));
        }
        for (i, a) in spots.iter().enumerate() {
            for b in &spots[i + 1..] {
                assert!(!overlaps(*a, *b), "{a:?} overlaps {b:?}");
            }
            assert!(!overlaps(*a, reticle_keep_out(window.0, window.1)));
        }
    }
}
