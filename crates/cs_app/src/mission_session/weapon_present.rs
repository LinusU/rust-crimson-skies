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
//! * one drawn flash per accepted shot — a child of the shot's
//!   [`MuzzleEffectSpawn`] record, at the emitted muzzle position, for a
//!   designed number of committed fixed ticks ([`MUZZLE_FLASH_LIFETIME_TICKS`]);
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

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use bevy::asset::{AssetApp, Assets};
use bevy::mesh::Mesh;
use bevy::prelude::FixedPostUpdate;
use bevy::prelude::{
    Added, AlphaMode, App, BackgroundColor, Camera3d, ChildOf, Children, Color, Commands,
    Component, Cuboid, Entity, Handle, Image, IntoScheduleConfigs, Local, Mesh3d, MeshMaterial3d,
    Node, Plugin, PositionType, Quat, Query, Rectangle, Res, ResMut, Resource, StandardMaterial,
    Startup, Text, TextColor, TextFont, Transform, UiRect, Update, Val, Vec3, Visibility, With,
    Without, World, default,
};
use bevy::text::FontSize;
use cs_assets::install;
use cs_formats::texture::DecodedImage;
use cs_formats::zbd::{read_animation_definitions, read_reader_nodes};
use cs_types::Tick;
use cs_types::content::ContentId;

use super::damage_trail::{
    TrailEmitter, TrailKind, TrailPuff, colour_at, emitter_of, plan_emissions, plan_puff,
    trail_puff_position,
};
use super::launch_sound::reader_member;
use super::weapons::{MissionWeaponReport, MissionWeapons};
use crate::ordnance::OrdnanceItemMirror;
use crate::physics::PhysicsTickLedger;
use crate::playtest_textures::{PlaytestTextureArchive, to_bevy_image};
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

/// The claim the muzzle flash is filed under. The original's muzzle-effect id →
/// drawable mapping is not recovered (`docs/findings/2026-10-19-f27-c-fx-render-muzzle-flash.md`),
/// so the flash is a project-designed stand-in, never the 2000 original's art.
pub const MUZZLE_FLASH_VISUAL_CLAIM: &str = "f27-c-fx-render.designed-muzzle-flash";

/// How long a muzzle flash lives, in committed fixed ticks (50 ms at the
/// composition's 120 Hz). The flash born on the shot's tick is drawn through
/// the next `MUZZLE_FLASH_LIFETIME_TICKS - 1` ticks and released on the one
/// after. Designed.
pub const MUZZLE_FLASH_LIFETIME_TICKS: u64 = 6;

/// The flash cube's edge, in metres. Designed.
const MUZZLE_FLASH_SIZE_M: f32 = 0.55;

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
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MuzzleDrawable {
    /// No original asset is recovered for the declared id: this designed
    /// stand-in is drawn under `claim`. The id is kept as declared.
    DesignedStandIn {
        /// The declared muzzle-effect id the shot named.
        declared: ContentId,
        /// The claim the drawing is filed under.
        claim: &'static str,
    },
}

/// Resolves a declared muzzle-effect id to what is drawn for it.
///
/// No original-data mapping from a `hardpoint_equipment` muzzle-effect id to a
/// mesh, sprite or particle has been measured, and a name is not a binding, so
/// every id resolves to the designed stand-in, by name.
#[must_use]
pub fn resolve_muzzle_drawable(declared: &ContentId) -> MuzzleDrawable {
    MuzzleDrawable::DesignedStandIn {
        declared: declared.clone(),
        claim: MUZZLE_FLASH_VISUAL_CLAIM,
    }
}

/// Component: a [`MuzzleEffectSpawn`] record that has been drawn.
///
/// Its presence is the once-only guard: the attach pass skips a record that
/// carries it, so a record is never drawn twice.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct MuzzleFlash {
    /// What the declared id resolved to.
    pub drawable: MuzzleDrawable,
    /// The first committed tick the flash is no longer drawn on.
    pub expires_at: u64,
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
    flash: Option<VisualPair>,
}

impl WeaponVisualAssets {
    /// The muzzle flash pair, built on first use.
    fn flash(
        &mut self,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
    ) -> VisualPair {
        self.flash
            .get_or_insert_with(|| VisualPair {
                mesh: meshes.add(Cuboid::new(
                    MUZZLE_FLASH_SIZE_M,
                    MUZZLE_FLASH_SIZE_M,
                    MUZZLE_FLASH_SIZE_M,
                )),
                material: materials.add(StandardMaterial {
                    base_color: Color::srgb(1.0, 0.95, 0.7),
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

/// Draws one flash for every muzzle-effect record that has none yet.
///
/// Runs on the fixed loop right after the presentation pass that spawns the
/// records, so a shot's flash exists in the tick that accepted it. The flash is
/// a child of the record: whatever releases the record releases the drawing.
fn attach_muzzle_flashes(
    mut commands: Commands,
    mut assets: ResMut<WeaponVisualAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    spawns: Query<(Entity, &MuzzleEffectSpawn), Without<MuzzleFlash>>,
) {
    if spawns.is_empty() {
        return;
    }
    let drawn = assets.flash(&mut meshes, &mut materials);
    for (entity, spawn) in &spawns {
        commands.entity(entity).insert((
            Visibility::Inherited,
            MuzzleFlash {
                drawable: resolve_muzzle_drawable(&spawn.effect),
                expires_at: spawn.at.0.saturating_add(MUZZLE_FLASH_LIFETIME_TICKS),
            },
        ));
        commands.spawn((
            WeaponVisual {
                kind: WeaponVisualKind::MuzzleFlash,
            },
            Mesh3d(drawn.mesh.clone()),
            MeshMaterial3d(drawn.material.clone()),
            Transform::IDENTITY,
            ChildOf(entity),
        ));
    }
}

/// Releases every flash whose designed lifetime has run out, record and
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
        if ledger.ticks >= flash.expires_at {
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

// ---------------------------------------------------------------------------
// The BOOM rocket's smoke trail (M01 rocket launch puffs and smoke trail, #1827)
// ---------------------------------------------------------------------------

/// The claim the trail's units, curves and gate are filed under.
pub const ROCKET_TRAIL_DESIGN_CLAIM: &str = "m01-rocket-trail.designed-trail";
/// The reader archive holding the effect.
pub const ROCKET_TRAIL_ARCHIVE: &str = "ZBD/zrdr.zbd";
/// The member of [`ROCKET_TRAIL_ARCHIVE`]. `weapons.zrd` binds it: `wep_06`
/// (`BOOM`) `FLYOUT` `MODEL_ANIMATION he_rocket`.
pub const ROCKET_TRAIL_MEMBER: &str = "missile_puffers.zrd";
/// The animation the BOOM flyout names.
pub const ROCKET_TRAIL_ANIMATION: &str = "he_rocket";
/// The `PUFFER_STATE` inside it that is switched on (`ACTIVE`).
pub const ROCKET_TRAIL_EMITTER: &str = "he_rocket_trail";

/// What the stored `he_rocket` animation states for its trail.
#[derive(Clone, Debug, PartialEq)]
pub struct RocketTrailDefinition {
    /// `he_rocket_trail`, anchored `AT_NODE he_rocket`.
    pub emitter: TrailEmitter,
    /// The `ANIMATION_OFFSET` the second, `ACTIVE FALSE` state of the same
    /// emitter carries (seconds, 10.0 in the retail data). Its meaning
    /// (switch-off after that offset) is not measured and is not applied.
    pub switch_off_offset_s: Option<f32>,
}

/// The definition and its decoded texture, as the stage carries them.
#[derive(Clone, Debug)]
pub struct RocketTrailSource {
    /// The effect.
    pub definition: RocketTrailDefinition,
    /// `(name, image)` for each of the emitter's textures.
    pub images: Vec<(String, DecodedImage)>,
    /// The archive the textures came from.
    pub texture_archive: String,
}

/// Why the rocket trail could not be measured.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RocketTrailError {
    /// The archive, member or token stream could not be read.
    Read(String),
    /// The record lacks a field the trail needs.
    Unmeasured(String),
    /// A texture the emitter names did not resolve or decode.
    Texture(String),
}

impl fmt::Display for RocketTrailError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(r) => write!(f, "rocket trail definition unreadable: {r}"),
            Self::Unmeasured(r) => write!(f, "rocket trail definition unmeasured: {r}"),
            Self::Texture(r) => write!(f, "rocket trail texture unavailable: {r}"),
        }
    }
}

impl std::error::Error for RocketTrailError {}

/// Reads `he_rocket` from the retail reader archive.
///
/// # Errors
///
/// [`RocketTrailError`] when the archive cannot be read or a field is absent.
pub fn read_rocket_trail_definition(
    install_root: &Path,
) -> Result<RocketTrailDefinition, RocketTrailError> {
    let bytes = reader_member(install_root, ROCKET_TRAIL_ARCHIVE, ROCKET_TRAIL_MEMBER)
        .map_err(RocketTrailError::Read)?;
    let nodes = read_reader_nodes(&bytes).map_err(|e| RocketTrailError::Read(e.to_string()))?;
    let animations =
        read_animation_definitions(&nodes).map_err(|e| RocketTrailError::Read(e.to_string()))?;
    let animation = animations
        .iter()
        .find(|a| a.id() == ROCKET_TRAIL_ANIMATION)
        .ok_or_else(|| {
            RocketTrailError::Unmeasured(format!("no animation {ROCKET_TRAIL_ANIMATION}"))
        })?;
    let active = animation
        .puffers()
        .find(|p| p.name == ROCKET_TRAIL_EMITTER && p.active == Some(true))
        .ok_or_else(|| RocketTrailError::Unmeasured(format!("no active {ROCKET_TRAIL_EMITTER}")))?;
    let emitter = emitter_of(active).map_err(|e| RocketTrailError::Unmeasured(e.to_string()))?;
    let switch_off_offset_s = animation
        .puffers()
        .filter(|p| p.name == ROCKET_TRAIL_EMITTER && p.active == Some(false))
        .find_map(|p| p.start_time.filter(|(k, _)| *k == "ANIMATION_OFFSET"))
        .map(|(_, seconds)| seconds);
    Ok(RocketTrailDefinition {
        emitter,
        switch_off_offset_s,
    })
}

/// Reads the effect and decodes its texture from the world group's own
/// texture archive.
///
/// # Errors
///
/// [`RocketTrailError`] when the definition or a texture is unavailable.
pub fn measure_rocket_trail(
    install_root: &Path,
    world_group: &str,
) -> Result<RocketTrailSource, RocketTrailError> {
    let definition = read_rocket_trail_definition(install_root)?;
    let found =
        install::discover(install_root).map_err(|e| RocketTrailError::Texture(e.to_string()))?;
    let archive = PlaytestTextureArchive::open(install_root, &found, world_group)
        .map_err(|e| RocketTrailError::Texture(e.to_string()))?;
    let mut images = Vec::new();
    for name in &definition.emitter.textures {
        let (_, image) = archive
            .fetch(name)
            .map_err(|(why, code)| RocketTrailError::Texture(format!("{name}: {code}: {why}")))?;
        images.push((name.clone(), image));
    }
    Ok(RocketTrailSource {
        definition,
        images,
        texture_archive: archive.path().to_owned(),
    })
}

/// Component: one live puff of a rocket's trail.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct RocketTrailPuff(pub TrailPuff);

/// What the trail has drawn, for the smoke check and the tests.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RocketTrailReport {
    /// Trail puffs spawned.
    pub puffs_spawned: u64,
}

/// Component on a rocket mirror: the emitter's progress along its flight.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct RocketTrailCursor {
    last: Option<Vec3>,
    carried: f32,
    emitted: u64,
}

/// The stage's source, waiting for the plugin to build the engine assets.
#[derive(Resource, Clone, Debug)]
pub struct RocketTrailSourceResource(pub RocketTrailSource);

/// The definition and the engine texture built from it.
#[derive(Resource, Clone, Debug)]
pub struct RocketTrailArt {
    definition: RocketTrailDefinition,
    textures: BTreeMap<String, Handle<Image>>,
    mesh: Handle<Mesh>,
}

impl RocketTrailArt {
    /// Builds the art from handles already made (the tests' seam).
    #[must_use]
    pub fn from_handles(
        definition: RocketTrailDefinition,
        textures: BTreeMap<String, Handle<Image>>,
        mesh: Handle<Mesh>,
    ) -> Self {
        Self {
            definition,
            textures,
            mesh,
        }
    }
}

fn tick_hz_of(ledger: &PhysicsTickLedger) -> f32 {
    if ledger.timestep_s > 0.0 {
        1.0 / ledger.timestep_s
    } else {
        120.0
    }
}

fn build_rocket_trail_art(
    source: Option<Res<RocketTrailSourceResource>>,
    art: Option<Res<RocketTrailArt>>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut commands: Commands,
) {
    if art.is_some() {
        return;
    }
    let Some(source) = source else {
        return;
    };
    let textures = source
        .0
        .images
        .iter()
        .map(|(name, decoded)| (name.clone(), images.add(to_bevy_image(decoded).0)))
        .collect();
    commands.insert_resource(RocketTrailArt {
        definition: source.0.definition.clone(),
        textures,
        mesh: meshes.add(Rectangle::new(1.0, 1.0)),
    });
}

/// Drops one puff per `DISTANCE_INTERVAL` of the player's rocket's travel.
#[allow(clippy::too_many_arguments)]
fn emit_rocket_trail(
    mut commands: Commands,
    art: Option<Res<RocketTrailArt>>,
    ledger: Option<Res<PhysicsTickLedger>>,
    weapons: Option<Res<MissionWeapons>>,
    mut report: ResMut<RocketTrailReport>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut rockets: Query<(
        &OrdnanceItemMirror,
        &Transform,
        Option<&mut RocketTrailCursor>,
        Entity,
    )>,
) {
    let (Some(art), Some(ledger), Some(weapons)) = (art, ledger, weapons) else {
        return;
    };
    let tick_hz = tick_hz_of(&ledger);
    let emitter = &art.definition.emitter;
    for (mirror, transform, cursor, entity) in &mut rockets {
        if mirror.shooter != weapons.player() {
            continue;
        }
        let now = transform.translation;
        let Some(mut cursor) = cursor else {
            commands.entity(entity).insert(RocketTrailCursor {
                last: Some(now),
                ..RocketTrailCursor::default()
            });
            continue;
        };
        let Some(previous) = cursor.last.replace(now) else {
            continue;
        };
        let (points, carried) =
            plan_emissions(previous, now, cursor.carried, emitter.distance_interval);
        cursor.carried = carried;
        for point in points {
            cursor.emitted += 1;
            let puff = plan_puff(
                emitter,
                TrailKind::Smoke,
                point,
                ledger.ticks,
                cursor.emitted ^ (mirror.projectile.serial << 32),
                tick_hz,
            );
            let texture = emitter.textures.get(puff.cycle_index);
            let material = materials.add(StandardMaterial {
                base_color: Color::WHITE,
                base_color_texture: texture.and_then(|n| art.textures.get(n)).cloned(),
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                cull_mode: None,
                ..Default::default()
            });
            commands.spawn((
                RocketTrailPuff(puff),
                Mesh3d(art.mesh.clone()),
                MeshMaterial3d(material),
                Transform::from_translation(point).with_scale(Vec3::splat(puff.size)),
                Visibility::Visible,
            ));
            report.puffs_spawned += 1;
        }
    }
}

fn advance_rocket_trail_puffs(
    mut commands: Commands,
    art: Option<Res<RocketTrailArt>>,
    ledger: Option<Res<PhysicsTickLedger>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut puffs: Query<(
        Entity,
        &RocketTrailPuff,
        &mut Transform,
        &MeshMaterial3d<StandardMaterial>,
    )>,
) {
    let (Some(art), Some(ledger)) = (art, ledger) else {
        return;
    };
    let tick_hz = tick_hz_of(&ledger);
    for (entity, RocketTrailPuff(puff), mut transform, material) in &mut puffs {
        let age = ledger.ticks.saturating_sub(puff.born);
        if age >= puff.life_ticks {
            commands.entity(entity).despawn();
            continue;
        }
        transform.translation = trail_puff_position(puff, age, tick_hz);
        let fraction = age as f32 / puff.life_ticks as f32;
        let [r, g, b, a] = colour_at(&art.definition.emitter.colors, fraction);
        if let Some(mut material) = materials.get_mut(&material.0) {
            material.base_color = Color::srgba(r, g, b, a);
        }
    }
}

/// Turns every rocket trail puff to face the camera, each rendered frame.
fn face_rocket_trail_puffs(
    cameras: Query<&Transform, (With<Camera3d>, Without<RocketTrailPuff>)>,
    mut puffs: Query<&mut Transform, With<RocketTrailPuff>>,
) {
    let Some(camera) = cameras.iter().next() else {
        return;
    };
    for mut transform in &mut puffs {
        transform.rotation = camera.rotation;
    }
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
            .init_resource::<RocketTrailReport>()
            .add_systems(
                FixedPostUpdate,
                (
                    build_rocket_trail_art,
                    emit_rocket_trail,
                    advance_rocket_trail_puffs,
                )
                    .chain()
                    .after(drive_weapon_effect_presentation),
            )
            .add_systems(Update, face_rocket_trail_puffs)
            .add_systems(
                Update,
                (attach_rocket_visuals, orient_weapon_visuals).chain(),
            )
            .add_systems(
                FixedPostUpdate,
                (attach_muzzle_flashes, expire_muzzle_flashes)
                    .chain()
                    .after(drive_weapon_effect_presentation),
            )
            .add_systems(Update, publish_rocket_notices.after(orient_weapon_visuals));
    }
}
