//! What the composed mission draws for its weapons (M01-WINDOW-FIRE, #1536).
//!
//! The F27 gun step and the F28 ordnance step already keep an ECS mirror of
//! every live projectile — [`WeaponRoundMirror`] for a round and
//! [`OrdnanceItemMirror`] for a launched rocket — and each mirror's
//! [`Transform`] is written from the runtime's own authoritative position.
//! Those entities carry **no mesh**, and no system read them, so in the
//! owner's windowed playtest (2026-10-17) a fired gun and a launched rocket
//! produced nothing a player could see. This module is the missing
//! presentation half:
//!
//! * one drawn child per live round — a bright streak placed over the segment
//!   the round actually travelled since the last drawn frame, so a held
//!   trigger reads as a continuous stream of tracers;
//! * one drawn child per live rocket — a body oriented along the rocket's own
//!   flight path;
//! * [`MissionHudNotice`]: a weapon refusal the publisher records (today: the
//!   rocket's "no declared-hostile damage target is live") is published for
//!   the HUD to show, so a refused shot is reported on screen instead of
//!   silently doing nothing.
//!
//! # Everything here is designed, and says so
//!
//! The mirror carries a position and nothing else: no orientation, no mesh and
//! no asset reference is measured for a round or a rocket in the original data
//! ([`cs_app::weapons`] and [`cs_app::ordnance`] both state that boundary on
//! their own `*_transform` functions). The streak's colour, width and length
//! bounds, the rocket's body and the HUD line's layout are therefore **project
//! design** under [`MISSION_WEAPON_VISUAL_CLAIM`], never a claim about the
//! original's sprites. What is *not* design is where they are drawn: every
//! pose comes from the authoritative runtime position the mirror already
//! carries, and the tracer's streak is the segment the round flew, not an
//! invented trajectory.
//!
//! The presentation is a child of the mirror, so a mirror the schedule
//! despawns (a retired round, a spent rocket, a restart's teardown) takes its
//! drawing with it — the same scene-generation discipline the mirrors
//! themselves follow.

use bevy::asset::{AssetApp, Assets};
use bevy::mesh::Mesh;
use bevy::prelude::{
    Added, App, BackgroundColor, ChildOf, Children, Color, Commands, Component, Cuboid, Entity,
    Handle, IntoScheduleConfigs, Local, Mesh3d, MeshMaterial3d, Node, Plugin, PositionType, Quat,
    Query, Res, ResMut, Resource, StandardMaterial, Startup, Text, TextColor, TextFont, Transform,
    UiRect, Update, Val, Vec3, Visibility, With, Without, World, default,
};
use bevy::text::FontSize;
use cs_types::Tick;

use super::weapons::MissionWeaponReport;
use crate::ordnance::OrdnanceItemMirror;
use crate::physics::PhysicsTickLedger;
use crate::weapons::WeaponRoundMirror;

/// The claim the mission's weapon visuals and its HUD notice line are filed
/// under: they are project design, not a measurement of the original's
/// sprites, sizes or colours.
pub const MISSION_WEAPON_VISUAL_CLAIM: &str = "m01-window-fire.designed-visuals";

/// The caption the HUD notice line carries, so a reader of the screen can
/// tell this layout apart from an original one.
pub const NOTICE_HUD_CAPTION: &str = "WEAPONS — DESIGNED LAYOUT";

/// How long one notice stays on screen, in fixed ticks (two seconds at the
/// composition's 120 Hz). A refusal the publisher records again refreshes it
/// with its own tick.
pub const NOTICE_LIFETIME_TICKS: u64 = 240;

/// The drawn streak's width, in metres. Designed.
const TRACER_WIDTH_M: f32 = 0.14;
/// The shortest streak that still reads on screen, in metres. A round's first
/// drawn frame has no travel to span yet, and a streak shorter than this
/// would be a dot. Designed.
const TRACER_MIN_LENGTH_M: f32 = 0.8;
/// The longest streak, in metres: a low frame rate must not turn one round
/// into a wall. Designed.
const TRACER_MAX_LENGTH_M: f32 = 24.0;
/// The rocket body's width, in metres. Designed.
const ROCKET_WIDTH_M: f32 = 0.34;
/// The rocket body's length, in metres. Designed.
const ROCKET_LENGTH_M: f32 = 2.6;

/// Component: what a drawn child presents (see the module docs).
///
/// One component rather than two marker types, so the orientation query stays
/// one query over the children the mirrors own.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct WeaponVisual {
    /// What this child draws.
    pub kind: WeaponVisualKind,
}

/// What one [`WeaponVisual`] child draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeaponVisualKind {
    /// The tracer streak of a live round.
    RoundTracer,
    /// The body of a live rocket.
    Rocket,
}

/// One mirror's motion sample, kept so the drawn child can be oriented along
/// the way the projectile actually flew.
///
/// The mirror itself carries a position and no orientation (measured
/// orientation does not exist), so the direction comes from the only
/// first-party source there is: two consecutive authoritative positions. The
/// first sample after a spawn establishes the baseline and draws nothing new.
#[derive(Component, Clone, Copy, Debug)]
pub struct WeaponVisualSample {
    /// The last world position this mirror was drawn at, `None` before the
    /// first sample.
    last: Option<Vec3>,
}

impl WeaponVisualSample {
    /// Samples `now`, returning the unit direction travelled since the
    /// previous sample and the distance travelled, or `None` when there is no
    /// previous sample or the travel is degenerate.
    fn sample(&mut self, now: Vec3) -> Option<(Vec3, f32)> {
        let previous = self.last.replace(now)?;
        let delta = now - previous;
        let length = delta.length();
        if !length.is_finite() || length <= f32::EPSILON {
            return None;
        }
        Some((delta / length, length))
    }
}

/// The tracer streak's [`Transform`] for one frame's travelled segment.
///
/// The parent mirror is already at the round's *current* position, so the
/// streak's origin is pulled back by half its length: the drawn box spans
/// `[previous, current]` exactly — what the round flew this frame — and is
/// scaled along its own long axis. A travel longer than the designed maximum
/// is clamped, never extrapolated.
fn streak_transform(direction: Vec3, travelled: f32) -> Transform {
    let length = travelled.clamp(TRACER_MIN_LENGTH_M, TRACER_MAX_LENGTH_M);
    Transform::from_translation(-direction * (length * 0.5))
        .with_rotation(Quat::from_rotation_arc(Vec3::Z, direction))
        .with_scale(Vec3::new(1.0, 1.0, length))
}

/// The drawn mesh + material pair one weapon visual is built from.
#[derive(Clone, Debug)]
struct VisualPair {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}

/// The mission's weapon visuals, built once and shared by every spawn.
///
/// [`StandardMaterial`] is deliberately the plain unlit engine material: no
/// custom pipeline is resolved for a designed visual, and an unlit streak is
/// readable against night, terrain and sea alike.
#[derive(Resource, Default)]
struct WeaponVisualAssets {
    tracer: Option<VisualPair>,
    rocket: Option<VisualPair>,
}

impl WeaponVisualAssets {
    /// The tracer pair, built on first use.
    fn tracer(
        &mut self,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
    ) -> VisualPair {
        self.tracer
            .get_or_insert_with(|| VisualPair {
                mesh: meshes.add(Cuboid::new(TRACER_WIDTH_M, TRACER_WIDTH_M, 1.0)),
                material: materials.add(StandardMaterial {
                    base_color: Color::srgb(1.0, 0.88, 0.25),
                    unlit: true,
                    ..default()
                }),
            })
            .clone()
    }

    /// The rocket body pair, built on first use.
    fn rocket(
        &mut self,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
    ) -> VisualPair {
        self.rocket
            .get_or_insert_with(|| VisualPair {
                mesh: meshes.add(Cuboid::new(ROCKET_WIDTH_M, ROCKET_WIDTH_M, ROCKET_LENGTH_M)),
                material: materials.add(StandardMaterial {
                    base_color: Color::srgb(1.0, 0.32, 0.08),
                    unlit: true,
                    ..default()
                }),
            })
            .clone()
    }
}

/// Parents one drawn streak to every round mirror that has none yet.
///
/// The mirror becomes a visibility node the same way the mission's world
/// objects do: a parent that draws only through its children states
/// [`Visibility::Inherited`] rather than inheriting Bevy's B0004 default.
fn attach_round_tracers(
    mut commands: Commands,
    mut assets: ResMut<WeaponVisualAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    rounds: Query<(Entity, &Transform), Added<WeaponRoundMirror>>,
) {
    if rounds.is_empty() {
        return;
    }
    let drawn = assets.tracer(&mut meshes, &mut materials);
    for (entity, transform) in &rounds {
        commands.entity(entity).insert((
            Visibility::Inherited,
            WeaponVisualSample {
                last: Some(transform.translation),
            },
        ));
        commands.spawn((
            WeaponVisual {
                kind: WeaponVisualKind::RoundTracer,
            },
            Mesh3d(drawn.mesh.clone()),
            MeshMaterial3d(drawn.material.clone()),
            Transform::IDENTITY,
            ChildOf(entity),
        ));
    }
}

/// Parents one drawn body to every rocket mirror that has none yet.
fn attach_rocket_visuals(
    mut commands: Commands,
    mut assets: ResMut<WeaponVisualAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    rockets: Query<(Entity, &Transform), Added<OrdnanceItemMirror>>,
) {
    if rockets.is_empty() {
        return;
    }
    let drawn = assets.rocket(&mut meshes, &mut materials);
    for (entity, transform) in &rockets {
        commands.entity(entity).insert((
            Visibility::Inherited,
            WeaponVisualSample {
                last: Some(transform.translation),
            },
        ));
        commands.spawn((
            WeaponVisual {
                kind: WeaponVisualKind::Rocket,
            },
            Mesh3d(drawn.mesh.clone()),
            MeshMaterial3d(drawn.material.clone()),
            Transform::IDENTITY,
            ChildOf(entity),
        ));
    }
}

/// Orients every drawn weapon visual along the way its projectile flew.
///
/// A tracer spans the frame's travelled segment; a rocket body points along
/// the same direction and stays centred on its mirror, because a rocket is an
/// object and a tracer is a path. Neither ever invents a direction: a frame
/// that produced no measurable travel keeps the previous pose.
fn orient_weapon_visuals(
    mut mirrors: Query<(&Transform, &mut WeaponVisualSample, &Children)>,
    mut drawn: Query<(&mut Transform, &WeaponVisual), Without<WeaponVisualSample>>,
) {
    for (parent, mut sample, children) in &mut mirrors {
        let Some((direction, travelled)) = sample.sample(parent.translation) else {
            continue;
        };
        for child in children.iter() {
            let Ok((mut transform, visual)) = drawn.get_mut(*child) else {
                continue;
            };
            match visual.kind {
                WeaponVisualKind::RoundTracer => {
                    *transform = streak_transform(direction, travelled);
                }
                WeaponVisualKind::Rocket => {
                    transform.rotation = Quat::from_rotation_arc(Vec3::Z, direction);
                }
            }
        }
    }
}

/// One line the mission's HUD shows about its weapons.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissionNoticeLine {
    /// The line, verbatim from the refusal the publisher recorded — a
    /// diagnostic, never an invented caption.
    pub text: String,
    /// The tick the refusal was recorded on, which is what the HUD's lifetime
    /// counts from.
    pub at: Tick,
}

/// The weapon notice the mission's HUD shows (see the module docs).
///
/// [`super::weapons::MissionWeaponReport::rocket_refusals`] keeps each distinct
/// refusal, but the player pressing the key again is a new event, so a notice
/// is published whenever `rocket_refusal_events` **grows**, and the newest
/// refusal is the line shown. A repeat refreshes the line with its own tick.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct MissionHudNotice {
    /// The line on screen, `None` while nothing has been refused.
    pub current: Option<MissionNoticeLine>,
    /// How many of the report's refusals have been published already.
    published: usize,
}

impl MissionHudNotice {
    /// How many refusals this notice has published. A refusal repeats every
    /// tick it holds, but is published once.
    #[must_use]
    pub const fn published(&self) -> usize {
        self.published
    }
}

/// Publishes the newest rocket refusal as the HUD's notice.
///
/// Runs once per rendered frame, after the fixed loop that wrote this frame's
/// report. A world that refuses nothing publishes nothing, and a publisher
/// that has not run a committed tick yet (`report.at` is `None`) publishes
/// nothing either — the notice is never invented from a tick nobody served.
pub fn publish_rocket_notices(world: &mut World) {
    let published = world.resource::<MissionHudNotice>().published;
    let newest = world
        .get_resource::<MissionWeaponReport>()
        .and_then(|report| {
            let at = report.at?;
            let text = report.rocket_refusals.last().cloned()?;
            let events = usize::try_from(report.rocket_refusal_events).unwrap_or(usize::MAX);
            (events > published).then_some((at, text, events))
        });
    let Some((at, text, count)) = newest else {
        return;
    };
    let mut notice = world
        .remove_resource::<MissionHudNotice>()
        .unwrap_or_default();
    notice.published = notice.published.max(count);
    notice.current = Some(MissionNoticeLine { text, at });
    world.insert_resource(notice);
}

/// The one line the HUD draws for [`MissionHudNotice`], windowed compositions
/// only — the headless face has no renderer and the resource above is what
/// the acceptance tests read.
#[derive(Clone, Copy, Debug, Default)]
pub struct MissionNoticeHudPlugin;

impl Plugin for MissionNoticeHudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_notice_line)
            .add_systems(Update, update_notice_line);
    }
}

/// The line node the update system finds.
#[derive(Component)]
struct NoticeLine;

fn spawn_notice_line(mut commands: Commands) {
    commands.spawn((
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(16.0),
            ..default()
        },
        TextColor(Color::srgb(1.0, 0.45, 0.35)),
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(190.0),
            left: Val::Px(8.0),
            padding: UiRect::all(Val::Px(6.0)),
            ..default()
        },
        Visibility::Hidden,
        NoticeLine,
        crate::playtest::visuals::DeveloperOverlayNode,
    ));
}

/// Shows the newest notice for its lifetime, then hides it again.
///
/// The clock is the composition's own committed tick, so a paused or unfocused
/// run freezes the line exactly as it freezes the simulation.
fn update_notice_line(
    notice: Option<Res<MissionHudNotice>>,
    ledger: Option<Res<PhysicsTickLedger>>,
    overlay: Option<Res<crate::playtest::visuals::DeveloperOverlay>>,
    mut lines: Query<(&mut Text, &mut Visibility), With<NoticeLine>>,
    mut last: Local<Option<(String, bool)>>,
) {
    // A developer diagnostic: shown only with the overlay (#1641).
    let overlay_on = crate::playtest::visuals::overlay_visible(overlay.as_deref());
    let current = notice.as_deref().and_then(|notice| notice.current.as_ref());
    let now = ledger.map_or(0, |ledger| ledger.ticks);
    let shown = current
        .filter(|line| now.saturating_sub(line.at.0) <= NOTICE_LIFETIME_TICKS)
        .filter(|_| overlay_on);
    let text = shown.map_or_else(String::new, |line| {
        format!("{NOTICE_HUD_CAPTION}\n{}", line.text)
    });
    if last
        .as_ref()
        .is_some_and(|(t, o)| *t == text && *o == overlay_on)
    {
        return;
    }
    for (mut node, mut visibility) in &mut lines {
        node.0.clone_from(&text);
        *visibility = if shown.is_some() {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    *last = Some((text, overlay_on));
}

/// Wires the mission's weapon presentation: the two drawn mirrors, their
/// orientation and the refusal notice.
///
/// Installed by [`super::compose::add_composition`] beside
/// [`super::world_draw::MissionWorldDrawPlugin`], so every composition —
/// windowed and headless — presents its weapons the same way. The headless
/// face draws nothing on a screen, but it still spawns the drawn children,
/// which is what the acceptance tests assert against.
#[derive(Clone, Copy, Debug, Default)]
pub struct MissionWeaponPresentationPlugin;

impl Plugin for MissionWeaponPresentationPlugin {
    fn build(&self, app: &mut App) {
        // The window's renderer registers these; the headless composition has
        // no PBR plugin, so both collections are initialised here when
        // absent — the same guard the mission's draw builders run.
        if !app.world().contains_resource::<Assets<StandardMaterial>>() {
            app.init_asset::<StandardMaterial>();
        }
        if !app.world().contains_resource::<Assets<Mesh>>() {
            app.init_asset::<Mesh>();
        }
        app.init_resource::<MissionHudNotice>()
            .init_resource::<WeaponVisualAssets>()
            .add_systems(
                Update,
                (
                    attach_round_tracers,
                    attach_rocket_visuals,
                    orient_weapon_visuals,
                )
                    .chain(),
            )
            .add_systems(Update, publish_rocket_notices.after(orient_weapon_visuals));
    }
}
