//! The dark round smoke puffs an ordnance burst leaves hanging in the air
//! (PARITY2-DMG-FX-B, #1806).
//!
//! The owner's frames of the original (#1737, c10/c14) show round dark-grey
//! puffs around the player after a burst. The definition of that smoke is
//! measured (`docs/findings/2026-10-21-parity2-dmg-fx-a-effect-definitions.md`):
//! the flak effect `flak_effect` (`flak_control.zrd`) calls
//! `large_black_smokeball` (`flame_ball.zrd`), whose `PUFFER_STATE`
//! `trailpuffer2` states the count, size range, life range, growth, friction,
//! random velocity and the three `thickblksmoke01..03` textures. This module
//! reads that emitter from the retail archive ([`measure_burst_puffs`]),
//! decodes its textures from the flown world's own texture archive, and draws
//! one burst of puffs where an ordnance item of the mission's session
//! detonates ([`OrdnanceBurst`], published by `drive_mission_ordnance`).
//!
//! # What is read, what is applied, what is not
//!
//! Applied exactly as stored: `NUMBER` puffs per burst, a size drawn from
//! `SIZE_RANGE` and a life from `LIFETIME_RANGE` (both in the units the data
//! states, read as metres and seconds), a velocity per axis from
//! `MIN_`/`MAX_RANDOM_VELOCITY`, one of the `TEXTURES` per puff.
//!
//! **Designed, and filed under [`BURST_PUFF_MOTION_CLAIM`]**, because the data
//! states no unit or curve: velocity decays as `v * exp(-FRICTION * t)`; the
//! puff fades linearly from opaque to clear over its life; the quad faces the
//! camera. `GROWTH_FACTOR`, `FADE_RANGE`, `TIME_INTERVAL`, the later
//! emissions up to the stored switch-off, `COLORS` and the blend mode are
//! **not applied** (unmeasured meaning), and the flak trails, the flash light
//! and the screen flash of `flak_effect` are not drawn here.
//!
//! The source events are the mission's own ordnance detonations and
//! guidance-loss blasts. The flak shooters of the sites are #1741's: when they
//! publish an [`OrdnanceBurst`] this module draws it unchanged.

use std::fmt;
use std::path::Path;

use bevy::asset::{AssetApp, Assets};
use bevy::mesh::Mesh;
use bevy::prelude::{
    AlphaMode, App, Camera3d, Color, Commands, Component, Entity, FixedPostUpdate, Handle, Image,
    IntoScheduleConfigs, Mesh3d, MeshMaterial3d, Plugin, Query, Rectangle, Res, ResMut, Resource,
    StandardMaterial, Transform, Update, Vec3, Visibility, With, Without,
};
use cs_assets::install;
use cs_formats::texture::DecodedImage;
use cs_formats::zbd::{read_animation_definitions, read_reader_nodes};
use cs_types::Tick;

use super::launch_sound::reader_member;
use super::weapons::drive_mission_ordnance;
use crate::physics::PhysicsTickLedger;
use crate::playtest_textures::{PlaytestTextureArchive, to_bevy_image};

/// The claim the puffs' motion, fade and facing are filed under.
pub const BURST_PUFF_MOTION_CLAIM: &str = "parity2-dmg-fx-b.designed-puff-motion";

/// The reader archive and member holding the emitter.
pub const BURST_PUFF_ARCHIVE: &str = "ZBD/zrdr.zbd";
/// The member of [`BURST_PUFF_ARCHIVE`].
pub const BURST_PUFF_MEMBER: &str = "flame_ball.zrd";
/// The animation `flak_effect` calls for its smoke.
pub const BURST_PUFF_ANIMATION: &str = "large_black_smokeball";
/// The `PUFFER_STATE` inside it.
pub const BURST_PUFF_EMITTER: &str = "trailpuffer2";

/// What the stored emitter states.
#[derive(Clone, Debug, PartialEq)]
pub struct BurstPuffDefinition {
    /// `NUMBER`: puffs per emission.
    pub number: u32,
    /// `SIZE_RANGE`.
    pub size: [f32; 2],
    /// `LIFETIME_RANGE`, seconds.
    pub lifetime_s: [f32; 2],
    /// `MIN_RANDOM_VELOCITY`.
    pub min_velocity: [f32; 3],
    /// `MAX_RANDOM_VELOCITY`.
    pub max_velocity: [f32; 3],
    /// `FRICTION`.
    pub friction: f32,
    /// `TEXTURES`, in stored order.
    pub textures: Vec<String>,
}

/// The definition and its decoded textures, as the stage carries them.
#[derive(Clone, Debug)]
pub struct BurstPuffSource {
    /// The emitter.
    pub definition: BurstPuffDefinition,
    /// One decoded image per [`BurstPuffDefinition::textures`] entry.
    pub images: Vec<DecodedImage>,
    /// The archive the textures came from.
    pub texture_archive: String,
}

/// Why the burst puffs could not be measured.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BurstPuffError {
    /// The archive, member or token stream could not be read.
    Read(String),
    /// The record lacks a field the puff needs.
    Unmeasured(String),
    /// A texture the emitter names did not resolve or decode.
    Texture(String),
}

impl fmt::Display for BurstPuffError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(r) => write!(f, "burst puff definition unreadable: {r}"),
            Self::Unmeasured(r) => write!(f, "burst puff definition unmeasured: {r}"),
            Self::Texture(r) => write!(f, "burst puff texture unavailable: {r}"),
        }
    }
}

impl std::error::Error for BurstPuffError {}

/// Reads the emitter from the retail reader archive.
///
/// # Errors
///
/// [`BurstPuffError`] when the archive cannot be read or the record lacks a
/// field.
pub fn read_burst_puff_definition(
    install_root: &Path,
) -> Result<BurstPuffDefinition, BurstPuffError> {
    let bytes = reader_member(install_root, BURST_PUFF_ARCHIVE, BURST_PUFF_MEMBER)
        .map_err(BurstPuffError::Read)?;
    let nodes = read_reader_nodes(&bytes).map_err(|e| BurstPuffError::Read(e.to_string()))?;
    let animations =
        read_animation_definitions(&nodes).map_err(|e| BurstPuffError::Read(e.to_string()))?;
    let animation = animations
        .iter()
        .find(|a| a.id() == BURST_PUFF_ANIMATION)
        .ok_or_else(|| {
            BurstPuffError::Unmeasured(format!("no animation {BURST_PUFF_ANIMATION}"))
        })?;
    let puffer = animation
        .puffers()
        .find(|p| p.name == BURST_PUFF_EMITTER && p.active == Some(true))
        .ok_or_else(|| BurstPuffError::Unmeasured(format!("no active {BURST_PUFF_EMITTER}")))?;
    let need =
        |what: &str| BurstPuffError::Unmeasured(format!("{BURST_PUFF_EMITTER} has no {what}"));
    let number = puffer.number.ok_or_else(|| need("NUMBER"))?;
    if !(number.is_finite() && number >= 1.0) {
        return Err(need("a positive NUMBER"));
    }
    if puffer.textures.is_empty() {
        return Err(need("TEXTURES"));
    }
    Ok(BurstPuffDefinition {
        number: number as u32,
        size: puffer.size_range.ok_or_else(|| need("SIZE_RANGE"))?,
        lifetime_s: puffer
            .lifetime_range
            .ok_or_else(|| need("LIFETIME_RANGE"))?,
        min_velocity: puffer
            .min_random_velocity
            .ok_or_else(|| need("MIN_RANDOM_VELOCITY"))?,
        max_velocity: puffer
            .max_random_velocity
            .ok_or_else(|| need("MAX_RANDOM_VELOCITY"))?,
        friction: puffer.friction.ok_or_else(|| need("FRICTION"))?,
        textures: puffer.textures.iter().map(|t| (*t).to_owned()).collect(),
    })
}

/// Reads the emitter and decodes its textures from the world group's own
/// texture archive.
///
/// # Errors
///
/// [`BurstPuffError`] when the definition or a texture is unavailable.
pub fn measure_burst_puffs(
    install_root: &Path,
    world_group: &str,
) -> Result<BurstPuffSource, BurstPuffError> {
    let definition = read_burst_puff_definition(install_root)?;
    let found =
        install::discover(install_root).map_err(|e| BurstPuffError::Texture(e.to_string()))?;
    let archive = PlaytestTextureArchive::open(install_root, &found, world_group)
        .map_err(|e| BurstPuffError::Texture(e.to_string()))?;
    let mut images = Vec::new();
    for name in &definition.textures {
        let (_, image) = archive
            .fetch(name)
            .map_err(|(why, code)| BurstPuffError::Texture(format!("{name}: {code}: {why}")))?;
        images.push(image);
    }
    Ok(BurstPuffSource {
        definition,
        images,
        texture_archive: archive.path().to_owned(),
    })
}

/// One ordnance burst: the tick it happened on and where.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrdnanceBurst {
    /// The committed tick the burst was resolved on.
    pub at: Tick,
    /// The world position of the burst.
    pub position: Vec3,
}

/// Bursts published by the ordnance step and not yet drawn.
#[derive(Resource, Clone, Debug, Default)]
pub struct PendingOrdnanceBursts(pub Vec<OrdnanceBurst>);

/// How many puffs the module has spawned and how many bursts it could not
/// draw (no definition installed), for the smoke check and the tests.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BurstPuffReport {
    /// Bursts drawn.
    pub bursts_drawn: u64,
    /// Bursts dropped because no definition is installed.
    pub bursts_undrawn: u64,
    /// Puffs spawned.
    pub puffs_spawned: u64,
}

/// Component: one live puff.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct BurstPuff {
    /// Tick it was born on.
    pub born: u64,
    /// Ticks it lives.
    pub life_ticks: u64,
    /// Where it started.
    pub origin: Vec3,
    /// Its initial velocity, metres per second.
    pub velocity: Vec3,
    /// Its edge length, metres.
    pub size: f32,
    /// `FRICTION` of the stored emitter.
    pub friction: f32,
}

/// The installed definition and the engine textures built from it.
#[derive(Resource, Clone, Debug)]
struct BurstPuffArt {
    definition: BurstPuffDefinition,
    textures: Vec<Handle<Image>>,
    mesh: Handle<Mesh>,
}

/// The stage's source, waiting for the plugin to build the engine assets.
#[derive(Resource, Clone, Debug)]
pub struct BurstPuffSourceResource(pub BurstPuffSource);

/// A small deterministic generator (splitmix64), so a burst is reproducible
/// from its tick and position.
struct Splitmix(u64);

impl Splitmix {
    fn next_unit(&mut self) -> f32 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        ((z >> 40) as f32) / ((1u64 << 24) as f32)
    }

    fn between(&mut self, range: [f32; 2]) -> f32 {
        range[0] + (range[1] - range[0]) * self.next_unit()
    }
}

/// The puffs of one burst, as plain data.
#[must_use]
pub fn plan_burst(
    definition: &BurstPuffDefinition,
    burst: OrdnanceBurst,
    tick_hz: f32,
) -> Vec<(BurstPuff, usize)> {
    let seed = burst.at.0
        ^ u64::from(burst.position.x.to_bits())
        ^ (u64::from(burst.position.y.to_bits()) << 21)
        ^ (u64::from(burst.position.z.to_bits()) << 42);
    let mut rng = Splitmix(seed);
    (0..definition.number as usize)
        .map(|index| {
            let life_s = rng.between(definition.lifetime_s).max(0.0);
            let velocity = Vec3::new(
                rng.between([definition.min_velocity[0], definition.max_velocity[0]]),
                rng.between([definition.min_velocity[1], definition.max_velocity[1]]),
                rng.between([definition.min_velocity[2], definition.max_velocity[2]]),
            );
            let puff = BurstPuff {
                born: burst.at.0,
                life_ticks: (life_s * tick_hz).ceil().max(1.0) as u64,
                origin: burst.position,
                velocity,
                size: rng.between(definition.size),
                friction: definition.friction,
            };
            (puff, index % definition.textures.len().max(1))
        })
        .collect()
}

/// A puff's position and opacity at `age_ticks`.
#[must_use]
pub fn puff_pose(puff: &BurstPuff, age_ticks: u64, tick_hz: f32) -> (Vec3, f32) {
    let t = age_ticks as f32 / tick_hz;
    let travelled = if puff.friction > f32::EPSILON {
        (1.0 - (-puff.friction * t).exp()) / puff.friction
    } else {
        t
    };
    let fade = 1.0 - (age_ticks as f32 / puff.life_ticks as f32).clamp(0.0, 1.0);
    (puff.origin + puff.velocity * travelled, fade)
}

fn build_art(
    source: Option<Res<BurstPuffSourceResource>>,
    art: Option<Res<BurstPuffArt>>,
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
        .map(|decoded| images.add(to_bevy_image(decoded).0))
        .collect();
    commands.insert_resource(BurstPuffArt {
        definition: source.0.definition.clone(),
        textures,
        mesh: meshes.add(Rectangle::new(1.0, 1.0)),
    });
}

fn spawn_burst_puffs(
    mut commands: Commands,
    mut pending: ResMut<PendingOrdnanceBursts>,
    mut report: ResMut<BurstPuffReport>,
    art: Option<Res<BurstPuffArt>>,
    ledger: Option<Res<PhysicsTickLedger>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let bursts = std::mem::take(&mut pending.0);
    if bursts.is_empty() {
        return;
    }
    let Some(art) = art else {
        report.bursts_undrawn += bursts.len() as u64;
        return;
    };
    let tick_hz = ledger
        .as_ref()
        .filter(|l| l.timestep_s > 0.0)
        .map_or(120.0, |l| 1.0 / l.timestep_s);
    for burst in bursts {
        report.bursts_drawn += 1;
        for (puff, texture) in plan_burst(&art.definition, burst, tick_hz) {
            let material = materials.add(StandardMaterial {
                base_color: Color::WHITE,
                base_color_texture: art.textures.get(texture).cloned(),
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                cull_mode: None,
                ..Default::default()
            });
            commands.spawn((
                puff,
                Mesh3d(art.mesh.clone()),
                MeshMaterial3d(material),
                Transform::from_translation(puff.origin).with_scale(Vec3::splat(puff.size)),
                Visibility::Visible,
            ));
            report.puffs_spawned += 1;
        }
    }
}

fn advance_burst_puffs(
    mut commands: Commands,
    ledger: Option<Res<PhysicsTickLedger>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut puffs: Query<(
        Entity,
        &BurstPuff,
        &mut Transform,
        &MeshMaterial3d<StandardMaterial>,
    )>,
) {
    let Some(ledger) = ledger else {
        return;
    };
    let tick_hz = if ledger.timestep_s > 0.0 {
        1.0 / ledger.timestep_s
    } else {
        120.0
    };
    for (entity, puff, mut transform, material) in &mut puffs {
        let age = ledger.ticks.saturating_sub(puff.born);
        if age >= puff.life_ticks {
            commands.entity(entity).despawn();
            continue;
        }
        let (position, opacity) = puff_pose(puff, age, tick_hz);
        transform.translation = position;
        if let Some(mut material) = materials.get_mut(&material.0) {
            material.base_color = Color::srgba(1.0, 1.0, 1.0, opacity);
        }
    }
}

/// Turns every puff to face the camera, each rendered frame.
fn face_burst_puffs(
    cameras: Query<&Transform, (With<Camera3d>, Without<BurstPuff>)>,
    mut puffs: Query<(&BurstPuff, &mut Transform)>,
) {
    let Some(camera) = cameras.iter().next() else {
        return;
    };
    for (_, mut transform) in &mut puffs {
        transform.rotation = camera.rotation;
    }
}

/// Installs the burst puffs: the pending list, the spawn and advance passes on
/// the fixed loop, and the facing pass on the frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct MissionBurstPuffPlugin;

impl Plugin for MissionBurstPuffPlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<Assets<StandardMaterial>>() {
            app.init_asset::<StandardMaterial>();
        }
        if !app.world().contains_resource::<Assets<Mesh>>() {
            app.init_asset::<Mesh>();
        }
        if !app.world().contains_resource::<Assets<Image>>() {
            app.init_asset::<Image>();
        }
        app.init_resource::<PendingOrdnanceBursts>()
            .init_resource::<BurstPuffReport>()
            .add_systems(
                FixedPostUpdate,
                (build_art, spawn_burst_puffs, advance_burst_puffs)
                    .chain()
                    .after(drive_mission_ordnance),
            )
            .add_systems(Update, face_burst_puffs);
    }
}
