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
//! * **no** drawn child per live round (Rally #1742): the owner's capture of
//!   the original shows no tracer streaks for a held trigger, and the
//!   original's `FLYOUT` model for a slug (`slug.flt`) is not shown to be
//!   drawn, so a round stays an undrawn mirror until a measurement says
//!   otherwise;
//! * one drawn child per live rocket — a body oriented along the rocket's own
//!   flight path;
//! * one measured muzzle burst per accepted shot — the `mb1` quad and the
//!   rolled `mb_spinflame` quads of the shot's gun
//!   ([`super::muzzle_burst_art`], #2106), children of the shot's
//!   [`MuzzleEffectSpawn`] record, at the emitted muzzle position, oriented by
//!   the record's pose (local -Z on the fire direction) and drawn for one
//!   committed fixed tick ([`MUZZLE_FLASH_LIFETIME_TICKS`]);
//! * [`MissionHudNotice`]: a weapon refusal the publisher records (today:
//!   the rocket's "no target is selected") is published for the HUD to show,
//!   so a refused shot is reported on screen instead of silently doing
//!   nothing.
//!
//! # Everything here is designed, and says so
//!
//! The mirror carries a position and nothing else: no orientation, no mesh and
//! no asset reference is measured for a round or a rocket in the original data
//! ([`cs_app::weapons`] and [`cs_app::ordnance`] both state that boundary on
//! their own `*_transform` functions). The rocket's body and the HUD line's layout are therefore **project
//! design** under [`MISSION_WEAPON_VISUAL_CLAIM`], never a claim about the
//! original's sprites. What is *not* design is where they are drawn: every
//! pose comes from the authoritative runtime position the mirror already
//! carries.
//!
//! The presentation is a child of the mirror, so a mirror the schedule
//! despawns (a retired round, a spent rocket, a restart's teardown) takes its
//! drawing with it — the same scene-generation discipline the mirrors
//! themselves follow.

use bevy::asset::{AssetApp, Assets, RenderAssetUsages};
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};
use bevy::prelude::FixedPostUpdate;
use bevy::prelude::{
    Added, AlphaMode, App, BackgroundColor, ChildOf, Children, Color, Commands, Component, Cuboid,
    Entity, Handle, Image, IntoScheduleConfigs, Local, Mesh3d, MeshMaterial3d, Node, Plugin,
    PositionType, Quat, Query, Res, ResMut, Resource, StandardMaterial, Startup, Text, TextColor,
    TextFont, Transform, UiRect, Update, Val, Vec3, Visibility, With, Without, World, default,
};
use bevy::text::FontSize;
use cs_types::Tick;
use cs_types::content::ContentId;

use super::muzzle_burst_art::{BurstNodeArt, BurstQuad, MuzzleBurstArt};
use super::weapons::MissionWeaponReport;
use crate::ordnance::OrdnanceItemMirror;
use crate::physics::PhysicsTickLedger;
use crate::playtest_textures::to_bevy_image;
use crate::weapon_effects::{MuzzleEffectSpawn, drive_weapon_effect_presentation};

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

/// The claim the muzzle burst is filed under.
///
/// The drawn quads, their textures, scale 1, the node's pose and the one-tick
/// lifetime are read from the original's data and engine image (findings
/// `2026-10-24-f27-c-fx-orig-muzzle-effect-binding`,
/// `2026-10-25-f27-c-fx-orig-muzzle-burst-pose`), statically — checked, never
/// `verified_original`. Still unmeasured, and so not drawn or only provisional:
/// the smoke puffer, lights and gunshell companions; the blend of material
/// flags 17 (drawn alpha-blended, unlit); which weapon rows set the random-roll
/// FIRE flag (roll 0 here); and whether shooters other than the player's gun
/// bind a different burst (every shot uses the art installed for the player's
/// selected gun record).
pub const MUZZLE_FLASH_VISUAL_CLAIM: &str = "f27-c-fx-render.designed-muzzle-flash";

/// How long a muzzle burst lives, in committed fixed ticks: drawn on the
/// shot's tick, released on the next. The burst definition switches the node
/// off at `EVENT_OFFSET 0.001` seconds, which is shorter than one tick.
///
/// A burst is also never released before a rendered frame has shown it
/// ([`MuzzleFlash::presented`]): the original's flash is one frame long, and a
/// 120 Hz fixed loop can run two ticks in one frame, which would otherwise
/// release a burst born on the first of them unseen.
pub const MUZZLE_FLASH_LIFETIME_TICKS: u64 = 1;

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
    /// The body of a live rocket.
    Rocket,
    /// The flash at an accepted shot's muzzle.
    MuzzleFlash,
}

/// What a declared muzzle-effect id resolves to for drawing.
#[derive(Clone, Debug, PartialEq)]
pub enum MuzzleDrawable {
    /// The measured burst of the weapon row's `FIRE > ANIMATION` binding.
    MeasuredBurst {
        /// The declared muzzle-effect id the shot named.
        declared: ContentId,
        /// The weapon record whose binding chose the art, e.g. `wep_50`.
        weapon: String,
        /// The bound burst, e.g. `muzzle_burst_slug`.
        burst: String,
        /// The roll about z the shot's `mb_spinflame` was turned to, one of
        /// the definition's measured rolls.
        spin_roll_deg: f32,
    },
    /// No measured art is installed, so nothing is drawn for the shot (there
    /// is no designed substitute). The record still exists for the audit.
    NotDrawn {
        /// The declared muzzle-effect id the shot named.
        declared: ContentId,
        /// The claim the omission is filed under.
        claim: &'static str,
    },
}

/// Resolves a declared muzzle-effect id to what is drawn for it.
///
/// `art` is the burst the weapon row's binding chose, when the stage loaded
/// one; `projectile_serial` picks the spin roll of this shot (the original
/// picks one of the measured rolls at random with equal weight, so any
/// deterministic choice among them is as measured).
#[must_use]
pub fn resolve_muzzle_drawable(
    declared: &ContentId,
    art: Option<&MuzzleBurstArt>,
    projectile_serial: u64,
) -> MuzzleDrawable {
    let Some(art) = art else {
        return MuzzleDrawable::NotDrawn {
            declared: declared.clone(),
            claim: MUZZLE_FLASH_VISUAL_CLAIM,
        };
    };
    let rolls = art.spin_rolls_deg.len().max(1) as u64;
    let pick = (projectile_serial.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 33) % rolls;
    MuzzleDrawable::MeasuredBurst {
        declared: declared.clone(),
        weapon: art.weapon.clone(),
        burst: art.burst.clone(),
        spin_roll_deg: art
            .spin_rolls_deg
            .get(pick as usize)
            .copied()
            .unwrap_or(0.0),
    }
}

/// Resource: the burst art the stage loaded from the installation, waiting
/// for [`build_muzzle_burst_drawing`] to turn it into engine assets.
#[derive(Resource, Clone, Debug)]
pub struct MuzzleBurstArtResource(pub MuzzleBurstArt);

/// One quad's engine assets.
#[derive(Clone, Debug)]
struct BurstQuadDraw {
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
}

/// The engine assets of the installed burst art.
#[derive(Resource, Clone, Debug)]
struct MuzzleBurstDrawing {
    flash: Vec<BurstQuadDraw>,
    spin: Vec<BurstQuadDraw>,
}

/// Component: a [`MuzzleEffectSpawn`] record that has been drawn.
///
/// Its presence is the once-only guard: the attach pass skips a record that
/// carries it, so a record is never drawn twice.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct MuzzleFlash {
    /// What the declared id resolved to.
    pub drawable: MuzzleDrawable,
    /// The first committed tick the flash is no longer drawn on.
    pub expires_at: u64,
    /// Whether a rendered frame has shown the burst; it is not released before.
    pub presented: bool,
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
    rocket: Option<VisualPair>,
}

impl WeaponVisualAssets {
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

/// One quad as an engine mesh: the stored corners and uvs in stored order,
/// split into the triangles (0,1,2) and (0,2,3).
fn quad_mesh(quad: &BurstQuad) -> Mesh {
    let positions: Vec<[f32; 3]> = quad.corners.to_vec();
    let uvs: Vec<[f32; 2]> = quad.uvs.to_vec();
    let a = Vec3::from(quad.corners[0]);
    let normal = (Vec3::from(quad.corners[1]) - a)
        .cross(Vec3::from(quad.corners[2]) - a)
        .normalize_or_zero();
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![normal.to_array(); 4])
    .with_inserted_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]))
}

/// Builds the engine meshes, textures and materials of the installed burst
/// art, once.
fn build_muzzle_burst_drawing(
    source: Option<Res<MuzzleBurstArtResource>>,
    built: Option<Res<MuzzleBurstDrawing>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let (Some(source), None) = (source, built) else {
        return;
    };
    let art = &source.0;
    let textures: Vec<(&str, Handle<Image>)> = art
        .textures
        .iter()
        .map(|texture| {
            (
                texture.name.as_str(),
                images.add(to_bevy_image(&texture.image).0),
            )
        })
        .collect();
    let mut draw = |node: &BurstNodeArt| -> Vec<BurstQuadDraw> {
        node.quads
            .iter()
            .map(|quad| BurstQuadDraw {
                mesh: meshes.add(quad_mesh(quad)),
                material: materials.add(StandardMaterial {
                    base_color: Color::WHITE,
                    base_color_texture: textures
                        .iter()
                        .find(|(name, _)| *name == quad.texture)
                        .map(|(_, handle)| handle.clone()),
                    unlit: true,
                    alpha_mode: AlphaMode::Blend,
                    cull_mode: None,
                    ..default()
                }),
            })
            .collect()
    };
    let flash = draw(&art.flash);
    let spin = draw(&art.spin);
    commands.insert_resource(MuzzleBurstDrawing { flash, spin });
}

/// Draws the measured burst for every muzzle-effect record that has none yet.
///
/// Runs on the fixed loop right after the presentation pass that spawns the
/// records, so a shot's burst exists in the tick that accepted it. The quads
/// are children of the record: the record's `Transform` is the burst node's
/// pose (translation at the muzzle, local -Z on the fire direction), quads sit
/// at scale 1 in the node's local space, and whatever releases the record
/// releases the drawing.
fn attach_muzzle_flashes(
    mut commands: Commands,
    art: Option<Res<MuzzleBurstArtResource>>,
    drawing: Option<Res<MuzzleBurstDrawing>>,
    spawns: Query<(Entity, &MuzzleEffectSpawn), Without<MuzzleFlash>>,
) {
    for (entity, spawn) in &spawns {
        let drawable = resolve_muzzle_drawable(
            &spawn.effect,
            art.as_deref().map(|art| &art.0),
            spawn.projectile.serial,
        );
        let roll = match &drawable {
            MuzzleDrawable::MeasuredBurst { spin_roll_deg, .. } => Some(*spin_roll_deg),
            MuzzleDrawable::NotDrawn { .. } => None,
        };
        commands.entity(entity).insert((
            Visibility::Inherited,
            MuzzleFlash {
                drawable,
                expires_at: spawn.at.0.saturating_add(MUZZLE_FLASH_LIFETIME_TICKS),
                presented: false,
            },
        ));
        let (Some(roll), Some(drawing)) = (roll, drawing.as_deref()) else {
            continue;
        };
        let quads = drawing
            .flash
            .iter()
            .map(|quad| (quad, Quat::IDENTITY))
            .chain(
                drawing
                    .spin
                    .iter()
                    .map(|quad| (quad, Quat::from_rotation_z(roll.to_radians()))),
            );
        for (quad, rotation) in quads {
            commands.spawn((
                WeaponVisual {
                    kind: WeaponVisualKind::MuzzleFlash,
                },
                Mesh3d(quad.mesh.clone()),
                MeshMaterial3d(quad.material.clone()),
                Transform::from_rotation(rotation),
                ChildOf(entity),
            ));
        }
    }
}

/// Marks every burst a rendered frame has now shown, so the next fixed tick
/// past its lifetime may release it.
fn present_muzzle_flashes(mut flashes: Query<&mut MuzzleFlash>) {
    for mut flash in &mut flashes {
        if !flash.presented {
            flash.presented = true;
        }
    }
}

/// Releases every burst whose lifetime has run out, record and
/// drawing together.
///
/// The clock is the committed tick ledger, so a paused or unfocused run
/// freezes a flash exactly as it freezes the simulation; without a ledger
/// nothing expires.
fn expire_muzzle_flashes(
    mut commands: Commands,
    ledger: Option<Res<PhysicsTickLedger>>,
    flashes: Query<(Entity, &MuzzleFlash)>,
) {
    let Some(ledger) = ledger else {
        return;
    };
    for (entity, flash) in &flashes {
        if flash.presented && ledger.ticks >= flash.expires_at {
            commands.entity(entity).despawn();
        }
    }
}

/// Orients every drawn weapon visual along the way its projectile flew.
///
/// A rocket body points along the direction it flew and stays centred on its
/// mirror. It never invents a direction: a frame
/// that produced no measurable travel keeps the previous pose.
fn orient_weapon_visuals(
    mut mirrors: Query<(&Transform, &mut WeaponVisualSample, &Children)>,
    mut drawn: Query<(&mut Transform, &WeaponVisual), Without<WeaponVisualSample>>,
) {
    for (parent, mut sample, children) in &mut mirrors {
        let Some((direction, _travelled)) = sample.sample(parent.translation) else {
            continue;
        };
        for child in children.iter() {
            let Ok((mut transform, visual)) = drawn.get_mut(*child) else {
                continue;
            };
            match visual.kind {
                WeaponVisualKind::Rocket => {
                    transform.rotation = Quat::from_rotation_arc(Vec3::Z, direction);
                }
                // A flash has no measured orientation and is not a child of a
                // mirror, so it never reaches this loop.
                WeaponVisualKind::MuzzleFlash => {}
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
        if !app.world().contains_resource::<Assets<Image>>() {
            app.init_asset::<Image>();
        }
        app.init_resource::<MissionHudNotice>()
            .init_resource::<WeaponVisualAssets>()
            .add_systems(
                Update,
                (attach_rocket_visuals, orient_weapon_visuals).chain(),
            )
            .add_systems(
                FixedPostUpdate,
                (
                    build_muzzle_burst_drawing,
                    attach_muzzle_flashes,
                    expire_muzzle_flashes,
                )
                    .chain()
                    .after(drive_weapon_effect_presentation),
            )
            .add_systems(Update, present_muzzle_flashes)
            .add_systems(Update, publish_rocket_notices.after(orient_weapon_visuals));
    }
}
