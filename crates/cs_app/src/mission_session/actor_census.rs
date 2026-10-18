//! The actor census and the hostile-contact aid of the windowed mission
//! (M01-WINDOW-ACTORS, Rally #1538).
//!
//! * **The census** prints one terminal line per spawned actor — name, the
//!   model it draws, position and distance from the player — once per spawn
//!   generation, after the visual attach pass ran, so a missing model shows as
//!   `undrawn` instead of the owner discovering an empty sky.
//! * **The contact aid** is a designed HUD: a list of the nearest hostile
//!   actors with their distances, and a bracket drawn over each one on screen.
//!   The original's target brackets are unmeasured here; this is development
//!   presentation, never a claim about the original HUD.
//!
//! Hostility is the reading the mission's AI split already uses: an aircraft
//! whose measured `team` differs from the `player` record's is opposed, and a
//! world actor is hostile when its roster carries
//! [`Allegiance::Hostile`] toward the player.

use bevy::prelude::{
    App, BackgroundColor, Camera, Camera3d, Color, Commands, Component, DetectChanges, Entity,
    FontSize, GlobalTransform, IntoScheduleConfigs, Local, Node, Plugin, PositionType, Query, Res,
    Text, TextColor, TextFont, UiRect, Update, Val, Vec3, Visibility, With, default,
};
use cs_sim::targeting::Allegiance;

use super::actor_visual::{ActorVisualDrawn, MissionActorVisuals, attach_actor_visuals};
use super::compose::{MissionPlayerBody, MissionStage};
use crate::mission_actors::{MissionActorSpawnReport, MissionZeppelinActor, ScriptedAircraftActor};
use crate::mission_start::SpawnDispatch;
use crate::world_actors::WorldActorBinding;

/// How many hostile contacts the list and the brackets show.
pub const MAX_CONTACTS: usize = 8;

/// One actor as the census and the aid read it.
#[derive(Clone, Debug, PartialEq)]
pub struct CensusRow {
    /// The record name (`wingman_2`) or the world actor's node (`piratezep`).
    pub name: String,
    /// `aircraft` or `zeppelin`.
    pub kind: &'static str,
    /// What it draws: the `planes.zbd` root, `world geometry`, or `undrawn`.
    pub model: String,
    /// World position, metres.
    pub position_m: Vec3,
    /// Distance from the player, metres.
    pub distance_m: f32,
    /// Opposed to the player under the designed reading.
    pub hostile: bool,
    /// A wingman of the player's flight (the `wingman` record prefix).
    pub ally: bool,
    /// Spawned asleep (held until the mission wakes it).
    pub asleep: bool,
    /// The original's airframe display name (`Kestrel`) for an aircraft whose
    /// `kind_of` chain reaches a row of the executable's airframe table; `None`
    /// when no measured name exists (every world actor).
    pub display_name: Option<&'static str>,
}

/// The original's airframe display name a `kind_of` chain (ancestor first)
/// reaches: the row of the executable's airframe table (`0x620c70`, #715)
/// whose display name, scene root or model the nearest ancestor names.
#[must_use]
pub fn airframe_display_name(chain: &[String]) -> Option<&'static str> {
    chain.iter().rev().find_map(|name| {
        crate::mission_start::AIRFRAME_TABLE.iter().find_map(|row| {
            [row.display_name, row.scene_root, row.model]
                .iter()
                .any(|spelling| spelling.eq_ignore_ascii_case(name))
                .then_some(row.display_name)
        })
    })
}

impl CensusRow {
    /// What a marker calls the contact: the measured display name, else the
    /// record id (the original's name for it is unmeasured).
    #[must_use]
    pub fn marker_name(&self) -> &str {
        self.display_name.unwrap_or(&self.name)
    }
}

impl CensusRow {
    /// The terminal line the census prints for this row.
    #[must_use]
    pub fn line(&self) -> String {
        format!(
            "mission actor: {} {} model={} pos=({:.0}, {:.0}, {:.0}) distance={:.0} m side={}{}",
            self.kind,
            self.name,
            self.model,
            self.position_m.x,
            self.position_m.y,
            self.position_m.z,
            self.distance_m,
            if self.hostile { "hostile" } else { "other" },
            if self.asleep { " asleep" } else { "" },
        )
    }
}

/// The `n` nearest hostile rows, nearest first.
#[must_use]
pub fn nearest_hostiles(rows: &[CensusRow], n: usize) -> Vec<&CensusRow> {
    let mut hostile: Vec<&CensusRow> = rows.iter().filter(|row| row.hostile).collect();
    hostile.sort_by(|a, b| a.distance_m.total_cmp(&b.distance_m));
    hostile.truncate(n);
    hostile
}

/// The contact list text: one line per hostile, nearest first.
#[must_use]
pub fn contact_list_text(rows: &[CensusRow]) -> String {
    let hostile = nearest_hostiles(rows, MAX_CONTACTS);
    let mut text = if hostile.is_empty() {
        "HOSTILES: none live".to_owned()
    } else {
        let mut text = String::from("HOSTILES");
        for row in hostile {
            text.push_str(&format!("\n{:<18}{:>6.0} m", row.name, row.distance_m));
        }
        text
    };
    for row in rows.iter().filter(|row| row.ally) {
        text.push_str(&format!(
            "\nwingman {:<10}{:>6.0} m",
            row.name, row.distance_m
        ));
    }
    text
}

fn player_team(stage: Option<&MissionStage>) -> Option<u32> {
    stage.and_then(|stage| {
        stage
            .aircraft
            .iter()
            .find(|record| record.dispatch == SpawnDispatch::Player)
            .and_then(|record| record.team)
    })
}

type AircraftQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static ScriptedAircraftActor,
        &'static GlobalTransform,
        Option<&'static ActorVisualDrawn>,
    ),
>;
type ZeppelinQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static MissionZeppelinActor,
        &'static WorldActorBinding,
        &'static GlobalTransform,
        Option<&'static ActorVisualDrawn>,
    ),
>;

fn rows_of(
    player: Vec3,
    stage: Option<&MissionStage>,
    aircraft: &AircraftQuery,
    zeppelins: &ZeppelinQuery,
) -> Vec<CensusRow> {
    let team = player_team(stage);
    let mut rows = Vec::new();
    for (actor, transform, drawn) in aircraft {
        let position_m = transform.translation();
        rows.push(CensusRow {
            name: actor.record.name.clone(),
            kind: "aircraft",
            model: drawn
                .and_then(|drawn| drawn.root.clone())
                .unwrap_or_else(|| "undrawn".to_owned()),
            position_m,
            distance_m: position_m.distance(player),
            hostile: matches!((actor.record.team, team), (Some(a), Some(b)) if a != b),
            ally: actor.vehicle_record == "wingman",
            asleep: actor.asleep,
            display_name: stage
                .and_then(|stage| stage.ai_airframes.models.get(&actor.vehicle_record))
                .and_then(|airframe| airframe_display_name(&airframe.inheritance_chain)),
        });
    }
    for (actor, binding, transform, drawn) in zeppelins {
        let position_m = transform.translation();
        rows.push(CensusRow {
            name: binding
                .subject
                .key()
                .rsplit('.')
                .next()
                .unwrap_or_default()
                .to_owned(),
            kind: "zeppelin",
            model: drawn
                .and_then(|drawn| drawn.root.clone())
                .map_or_else(|| "undrawn".to_owned(), |node| format!("node {node}")),
            position_m,
            distance_m: position_m.distance(player),
            hostile: actor
                .roster
                .as_ref()
                .is_some_and(|roster| roster.player_allegiance == Some(Allegiance::Hostile)),
            ally: false,
            asleep: false,
            display_name: None,
        });
    }
    rows.sort_by(|a, b| a.distance_m.total_cmp(&b.distance_m));
    rows
}

/// Prints the census once per spawn generation, when the player exists.
fn print_census(
    report: Option<Res<MissionActorSpawnReport>>,
    stage: Option<Res<MissionStage>>,
    players: Query<&GlobalTransform, With<MissionPlayerBody>>,
    aircraft: AircraftQuery,
    zeppelins: ZeppelinQuery,
    visuals: Option<Res<MissionActorVisuals>>,
    mut pending: Local<bool>,
) {
    let Some(report) = report else { return };
    if report.is_changed() {
        *pending = true;
    }
    if !*pending {
        return;
    }
    let Ok(player) = players.single() else { return };
    *pending = false;
    let player = player.translation();
    let rows = rows_of(player, stage.as_deref(), &aircraft, &zeppelins);
    println!(
        "mission actors: {} aircraft, {} zeppelins; player at ({:.0}, {:.0}, {:.0})",
        report.aircraft.len(),
        report.zeppelins.len(),
        player.x,
        player.y,
        player.z
    );
    for row in &rows {
        println!("{}", row.line());
    }
    if let Some(stage) = stage.as_deref() {
        for record in &stage.aircraft {
            println!(
                "mission record: {} team {:?} stored pose {:?}",
                record.name, record.team, record.stored_pose
            );
        }
        for (prefix, airframe) in &stage.ai_airframes.models {
            println!(
                "mission actor model: {prefix} kind_of chain {:?} draws {}",
                airframe.inheritance_chain,
                visuals
                    .as_deref()
                    .and_then(|visuals| visuals.root_of(prefix))
                    .unwrap_or("nothing")
            );
        }
    }
    for refusal in visuals.iter().flat_map(|visuals| visuals.refusals.iter()) {
        println!("mission actor model refused: {refusal}");
    }
    for refusal in &report.refusals {
        println!("mission actor refused: {refusal}");
    }
}

/// Prints the wingmen's distance each second for the first five seconds and
/// every five seconds after, so the terminal shows whether the flight forms up.
fn print_wingmen(
    stage: Option<Res<MissionStage>>,
    players: Query<&GlobalTransform, With<MissionPlayerBody>>,
    aircraft: AircraftQuery,
    zeppelins: ZeppelinQuery,
    mut frames: Local<u64>,
) {
    let Ok(player) = players.single() else { return };
    *frames += 1;
    let second = *frames / 60;
    if !(*frames).is_multiple_of(60) || (second > 5 && !second.is_multiple_of(5)) {
        return;
    }
    let rows = rows_of(
        player.translation(),
        stage.as_deref(),
        &aircraft,
        &zeppelins,
    );
    let wingmen: Vec<String> = rows
        .iter()
        .filter(|row| row.ally)
        .map(|row| format!("{} {:.0} m", row.name, row.distance_m))
        .collect();
    println!("mission wingmen t+{second}s: {}", wingmen.join(", "));
}

/// The contact list node.
#[derive(Component)]
struct ContactList;

/// One bracket drawn over a hostile.
#[derive(Component)]
struct ContactBracket;

fn spawn_contact_list(mut commands: Commands) {
    commands.spawn((
        Text::new("HOSTILES"),
        TextFont {
            font_size: FontSize::Px(16.0),
            ..default()
        },
        TextColor(Color::srgb(1.0, 0.4, 0.3)),
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(200.0),
            left: Val::Px(8.0),
            padding: UiRect::all(Val::Px(6.0)),
            ..default()
        },
        Visibility::Visible,
        ContactList,
        crate::playtest::visuals::DeveloperOverlayNode,
    ));
}

#[allow(clippy::too_many_arguments)]
fn update_contacts(
    mut commands: Commands,
    stage: Option<Res<MissionStage>>,
    players: Query<&GlobalTransform, With<MissionPlayerBody>>,
    aircraft: AircraftQuery,
    zeppelins: ZeppelinQuery,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    mut lists: Query<&mut Text, With<ContactList>>,
    brackets: Query<Entity, With<ContactBracket>>,
    overlay: Option<Res<crate::playtest::visuals::DeveloperOverlay>>,
    windows: Query<&bevy::prelude::Window>,
) {
    let overlay_on = crate::playtest::visuals::overlay_visible(overlay.as_deref());
    for entity in &brackets {
        commands.entity(entity).despawn();
    }
    let Ok(player) = players.single() else { return };
    let rows = rows_of(
        player.translation(),
        stage.as_deref(),
        &aircraft,
        &zeppelins,
    );
    let text = contact_list_text(&rows);
    for mut list in &mut lists {
        if list.0 != text {
            list.0.clone_from(&text);
        }
    }
    let Some((camera, camera_transform)) = cameras.iter().next() else {
        return;
    };
    let marked = nearest_hostiles(&rows, MAX_CONTACTS)
        .into_iter()
        .chain(rows.iter().filter(|row| row.ally));
    if !overlay_on {
        // The original-style markers: the bare name on screen, `Name 7
        // o'clock` clamped to the screen edge when off screen (#1641).
        let Ok(window) = windows.single() else { return };
        let window_size = (window.width(), window.height());
        let mut placed = vec![super::cockpit_hud::reticle_keep_out(
            window_size.0,
            window_size.1,
        )];
        for row in marked.filter(|row| !row.ally) {
            spawn_original_marker(
                &mut commands,
                row,
                camera,
                camera_transform,
                window_size,
                &mut placed,
            );
        }
        return;
    }
    for row in marked {
        // Behind the camera has no screen position.
        let toward = row.position_m - camera_transform.translation();
        if toward.dot(*camera_transform.forward()) <= 0.0 {
            continue;
        }
        let Ok(at) = camera.world_to_viewport(camera_transform, row.position_m) else {
            continue;
        };
        commands.spawn((
            Text::new(format!("[ {} {:.0} m ]", row.name, row.distance_m)),
            TextFont {
                font_size: FontSize::Px(14.0),
                ..default()
            },
            TextColor(if row.ally {
                Color::srgb(0.4, 1.0, 0.4)
            } else {
                Color::srgb(1.0, 0.3, 0.2)
            }),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(at.x - 50.0),
                top: Val::Px(at.y - 8.0),
                ..default()
            },
            ContactBracket,
        ));
    }
}

/// Draws one hostile marker in the original form: red, the contact's name, and
/// when the contact is off screen an arrow at the screen edge and the clock
/// bearing from the camera's heading.
fn spawn_original_marker(
    commands: &mut Commands,
    row: &CensusRow,
    camera: &Camera,
    camera_transform: &GlobalTransform,
    window: (f32, f32),
    placed: &mut Vec<super::cockpit_hud::LabelRect>,
) {
    let toward = row.position_m - camera_transform.translation();
    let forward = *camera_transform.forward();
    let right = *camera_transform.right();
    let relative_deg = toward.dot(right).atan2(toward.dot(forward)).to_degrees();
    let on_screen = if toward.dot(forward) > 0.0 {
        camera
            .world_to_viewport(camera_transform, row.position_m)
            .ok()
            .filter(|at| at.x >= 0.0 && at.y >= 0.0 && at.x <= window.0 && at.y <= window.1)
    } else {
        None
    };
    let name = row.marker_name();
    // The wanted spot: beside the contact on screen, else on the edge toward
    // its bearing. `place_label` then keeps it clear of the reticle, the
    // window border and every label placed before it.
    let (wanted, label) = match on_screen {
        Some(at) => (
            (at.x + 14.0, at.y - 8.0),
            super::cockpit_hud::marker_text(name, None),
        ),
        None => {
            let hour = super::cockpit_hud::clock_hour(relative_deg);
            let (arrow, right) = if relative_deg.rem_euclid(360.0) < 180.0 {
                ("->", true)
            } else {
                ("<-", false)
            };
            let label = format!(
                "{arrow} {}",
                super::cockpit_hud::marker_text(name, Some(hour))
            );
            let width = super::cockpit_hud::label_width(label.chars().count());
            let x = if right { window.0 - width - 10.0 } else { 10.0 };
            ((x, window.1 * 0.4), label)
        }
    };
    let width = super::cockpit_hud::label_width(label.chars().count());
    let (left, top) = super::cockpit_hud::place_label(wanted, width, window, placed);
    commands.spawn((
        Text::new(label),
        TextFont {
            font_size: FontSize::Px(16.0),
            ..default()
        },
        TextColor(Color::srgb(1.0, 0.2, 0.2)),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(left),
            top: Val::Px(top),
            ..default()
        },
        ContactBracket,
    ));
}

/// Installs the visual attach pass, the census and (windowed compositions
/// only) the contact aid.
#[derive(Clone, Copy, Debug, Default)]
pub struct MissionActorAidPlugin {
    /// Whether the composition has a window to draw the contact aid in.
    pub windowed: bool,
}

impl Plugin for MissionActorAidPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (attach_actor_visuals, print_census, print_wingmen).chain(),
        );
        if self.windowed {
            app.add_systems(bevy::prelude::Startup, spawn_contact_list)
                .add_systems(Update, update_contacts);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, distance_m: f32, hostile: bool) -> CensusRow {
        CensusRow {
            name: name.to_owned(),
            kind: "aircraft",
            model: "piratefighter".to_owned(),
            position_m: Vec3::ZERO,
            distance_m,
            hostile,
            ally: false,
            asleep: false,
            display_name: None,
        }
    }

    #[test]
    fn accept_m01_window_actors_contact_list_is_nearest_hostile_first() {
        let rows = [
            row("far", 900.0, true),
            row("friend", 10.0, false),
            row("near", 120.0, true),
        ];
        let text = contact_list_text(&rows);
        let near = text.find("near").expect("near listed");
        let far = text.find("far").expect("far listed");
        assert!(near < far);
        assert!(!text.contains("friend"));
        assert_eq!(
            contact_list_text(&[row("friend", 1.0, false)]),
            "HOSTILES: none live"
        );
    }

    #[test]
    fn accept_parity_m01_hud_marker_names_come_from_the_airframe_table() {
        let chain = |names: &[&str]| names.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>();
        // The nearest ancestor that names a table row wins.
        assert_eq!(
            airframe_display_name(&chain(&["aircraft", "kestrel", "medkestrel"])),
            Some("Kestrel")
        );
        // A model spelling resolves to its row's display name.
        assert_eq!(
            airframe_display_name(&chain(&["piratefighter", "rusdevastator"])),
            Some("Devastator")
        );
        assert_eq!(airframe_display_name(&chain(&["cargozep"])), None);
        let mut named = row("medkestrel_2", 10.0, true);
        assert_eq!(named.marker_name(), "medkestrel_2");
        named.display_name = Some("Kestrel");
        assert_eq!(named.marker_name(), "Kestrel");
    }
}
