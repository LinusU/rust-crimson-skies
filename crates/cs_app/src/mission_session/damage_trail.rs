//! The fire and smoke trail a damaged player airframe leaves
//! (PARITY-M01-PLAYER-DAMAGE-TRAIL, #1700).
//!
//! The original's wing fire is an anim-driven composite: the `wing_flare` quad
//! (`airframe_damage`) plus the sprite trail of the animation
//! `player_smoketrail`, which calls `dense_firetrail` `WITH_NODE prop1`
//! (`docs/findings/2026-10-21-parity2-dmg-fx-a-effect-definitions.md`). That
//! effect is two `PUFFER_STATE` emitters in `ZBD/zrdr.zbd` / `pufftrails.zrd`:
//! `smokepuffer` (`smoke101..103`, colour keys) and `firepuffer`
//! (`TEXTURE_SEQUENCE` `fire_f01..f06`). This module reads both
//! ([`measure_damage_trail`]), decodes the nine textures from the flown
//! world's texture archive, and draws the trail at the player's `prop1`
//! node while a wing region of the damage authority reports damage.
//!
//! # Applied as stored
//!
//! `DISTANCE_INTERVAL` (one puff per that much travel of the anchor, read as
//! metres), `SIZE_RANGE`, `LIFETIME_RANGE`, the constant
//! `MIN_/MAX_RANDOM_VELOCITY`, `FRICTION`, and the texture names (`TEXTURES`
//! cycle for the smoke, `TEXTURE_SEQUENCE` for the fire).
//!
//! # Designed, filed under [`DAMAGE_TRAIL_DESIGN_CLAIM`]
//!
//! The data states no unit or curve for these: the `TEXTURE_SEQUENCE` frame
//! times read as seconds of puff age; the `COLORS` key times as a fraction of
//! the puff's life (rgb 0..255, alpha 0..1, linearly interpolated); velocity
//! decays as `v * exp(-FRICTION * t)`; the quad faces the camera and blends.
//! The gate is designed too: the trail plays while any wing region reports
//! `Damaged` or `Destroyed`, because which `injure_anims` threshold calls
//! `player_smoketrail`, and the direction of that threshold, is unmeasured.
//!
//! # Not applied (unmeasured meaning)
//!
//! `GROWTH_FACTOR`, `FADE_RANGE`, `DEVIATION_DISTANCE`, the `fire_lt` light
//! flicker, the `wing_flare2` INACTIVE switch, `player_fuelleak`'s
//! `fuel_trail` at `pdp1..3`, and the per-panel `pdpanel*` fires.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use bevy::asset::{AssetApp, Assets};
use bevy::mesh::Mesh;
use bevy::prelude::{
    AlphaMode, App, Camera3d, Color, Commands, Component, Entity, FixedPostUpdate, GlobalTransform,
    Handle, Image, IntoScheduleConfigs, Mesh3d, MeshMaterial3d, Plugin, Query, Rectangle, Res,
    ResMut, Resource, StandardMaterial, Transform, Update, Vec3, Visibility, With, Without,
};
use cs_assets::install;
use cs_formats::texture::DecodedImage;
use cs_formats::zbd::{PufferState, read_animation_definitions, read_reader_nodes};
use cs_sim::damage::PartState;

use super::launch_sound::reader_member;
use crate::airframe_damage::{RegionDamagePresentation, WingFlare};
use crate::physics::PhysicsTickLedger;
use crate::playtest::propeller::PropellerBlur;
use crate::playtest_textures::{PlaytestTextureArchive, to_bevy_image};

/// The claim the trail's units, curves and gate are filed under.
pub const DAMAGE_TRAIL_DESIGN_CLAIM: &str = "parity-m01-player-damage-trail.designed-trail";

/// The reader archive holding the effect.
pub const DAMAGE_TRAIL_ARCHIVE: &str = "ZBD/zrdr.zbd";
/// The member of [`DAMAGE_TRAIL_ARCHIVE`].
pub const DAMAGE_TRAIL_MEMBER: &str = "pufftrails.zrd";
/// The animation `player_smoketrail` calls.
pub const DAMAGE_TRAIL_ANIMATION: &str = "dense_firetrail";
/// The smoke emitter inside it.
pub const DAMAGE_TRAIL_SMOKE_EMITTER: &str = "smokepuffer";
/// The fire emitter inside it.
pub const DAMAGE_TRAIL_FIRE_EMITTER: &str = "firepuffer";

/// What one stored emitter states.
#[derive(Clone, Debug, PartialEq)]
pub struct TrailEmitter {
    /// `DISTANCE_INTERVAL`.
    pub distance_interval: f32,
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
    /// `TEXTURES` (a cycle, no times); empty when the emitter uses frames.
    pub textures: Vec<String>,
    /// `TEXTURE_SEQUENCE` `(time, texture)`; empty when it uses a cycle.
    pub frames: Vec<(f32, String)>,
    /// `COLORS` keys `[t, r, g, b, a]`; empty for none (white, opaque).
    pub colors: Vec<[f32; 5]>,
}

/// The stored `dense_firetrail`.
#[derive(Clone, Debug, PartialEq)]
pub struct DamageTrailDefinition {
    /// `smokepuffer`.
    pub smoke: TrailEmitter,
    /// `firepuffer`.
    pub fire: TrailEmitter,
}

impl DamageTrailDefinition {
    /// Every texture name either emitter draws, once each.
    #[must_use]
    pub fn texture_names(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for emitter in [&self.smoke, &self.fire] {
            let all = emitter
                .textures
                .iter()
                .chain(emitter.frames.iter().map(|(_, name)| name));
            for name in all {
                if !names.contains(name) {
                    names.push(name.clone());
                }
            }
        }
        names
    }
}

/// The definition and its decoded textures, as the stage carries them.
#[derive(Clone, Debug)]
pub struct DamageTrailSource {
    /// The effect.
    pub definition: DamageTrailDefinition,
    /// `(name, image)` for each of [`DamageTrailDefinition::texture_names`].
    pub images: Vec<(String, DecodedImage)>,
    /// The archive the textures came from.
    pub texture_archive: String,
}

/// Why the trail could not be measured.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DamageTrailError {
    /// The archive, member or token stream could not be read.
    Read(String),
    /// The record lacks a field the trail needs.
    Unmeasured(String),
    /// A texture the emitters name did not resolve or decode.
    Texture(String),
}

impl fmt::Display for DamageTrailError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(r) => write!(f, "damage trail definition unreadable: {r}"),
            Self::Unmeasured(r) => write!(f, "damage trail definition unmeasured: {r}"),
            Self::Texture(r) => write!(f, "damage trail texture unavailable: {r}"),
        }
    }
}

impl std::error::Error for DamageTrailError {}

pub(super) fn emitter_of(puffer: &PufferState<'_>) -> Result<TrailEmitter, DamageTrailError> {
    let need = |what: &str| DamageTrailError::Unmeasured(format!("{} has no {what}", puffer.name));
    let distance_interval = puffer
        .distance_interval
        .filter(|d| d.is_finite() && *d > 0.0)
        .ok_or_else(|| need("a positive DISTANCE_INTERVAL"))?;
    if puffer.textures.is_empty() && puffer.texture_sequence.is_empty() {
        return Err(need("TEXTURES or TEXTURE_SEQUENCE"));
    }
    Ok(TrailEmitter {
        distance_interval,
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
        friction: puffer.friction.unwrap_or(0.0),
        textures: puffer.textures.iter().map(|t| (*t).to_owned()).collect(),
        frames: puffer
            .texture_sequence
            .iter()
            .map(|(t, name)| (*t, (*name).to_owned()))
            .collect(),
        colors: puffer.colors.clone(),
    })
}

/// Reads `dense_firetrail` from the retail reader archive.
///
/// # Errors
///
/// [`DamageTrailError`] when the archive cannot be read or a field is absent.
pub fn read_damage_trail_definition(
    install_root: &Path,
) -> Result<DamageTrailDefinition, DamageTrailError> {
    let bytes = reader_member(install_root, DAMAGE_TRAIL_ARCHIVE, DAMAGE_TRAIL_MEMBER)
        .map_err(DamageTrailError::Read)?;
    let nodes = read_reader_nodes(&bytes).map_err(|e| DamageTrailError::Read(e.to_string()))?;
    let animations =
        read_animation_definitions(&nodes).map_err(|e| DamageTrailError::Read(e.to_string()))?;
    let animation = animations
        .iter()
        .find(|a| a.id() == DAMAGE_TRAIL_ANIMATION)
        .ok_or_else(|| {
            DamageTrailError::Unmeasured(format!("no animation {DAMAGE_TRAIL_ANIMATION}"))
        })?;
    let active = |name: &str| {
        animation
            .puffers()
            .find(|p| p.name == name && p.active == Some(true))
            .ok_or_else(|| DamageTrailError::Unmeasured(format!("no active {name}")))
    };
    Ok(DamageTrailDefinition {
        smoke: emitter_of(active(DAMAGE_TRAIL_SMOKE_EMITTER)?)?,
        fire: emitter_of(active(DAMAGE_TRAIL_FIRE_EMITTER)?)?,
    })
}

/// Reads the effect and decodes its textures from the world group's own
/// texture archive.
///
/// # Errors
///
/// [`DamageTrailError`] when the definition or a texture is unavailable.
pub fn measure_damage_trail(
    install_root: &Path,
    world_group: &str,
) -> Result<DamageTrailSource, DamageTrailError> {
    let definition = read_damage_trail_definition(install_root)?;
    let found =
        install::discover(install_root).map_err(|e| DamageTrailError::Texture(e.to_string()))?;
    let archive = PlaytestTextureArchive::open(install_root, &found, world_group)
        .map_err(|e| DamageTrailError::Texture(e.to_string()))?;
    let mut images = Vec::new();
    for name in definition.texture_names() {
        let (_, image) = archive
            .fetch(&name)
            .map_err(|(why, code)| DamageTrailError::Texture(format!("{name}: {code}: {why}")))?;
        images.push((name, image));
    }
    Ok(DamageTrailSource {
        definition,
        images,
        texture_archive: archive.path().to_owned(),
    })
}

/// Which emitter a puff came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrailKind {
    /// `smokepuffer`.
    Smoke,
    /// `firepuffer`.
    Fire,
}

/// Component: one live trail puff.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct TrailPuff {
    /// The emitter it came from.
    pub kind: TrailKind,
    /// Tick it was born on.
    pub born: u64,
    /// Ticks it lives.
    pub life_ticks: u64,
    /// Where it started.
    pub origin: Vec3,
    /// Its constant initial velocity, metres per second.
    pub velocity: Vec3,
    /// Its edge length, metres.
    pub size: f32,
    /// `FRICTION` of the emitter.
    pub friction: f32,
    /// Index into the emitter's `TEXTURES` cycle (smoke only).
    pub cycle_index: usize,
}

/// What the module has drawn, for the smoke check and the tests.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DamageTrailReport {
    /// Smoke puffs spawned.
    pub smoke_spawned: u64,
    /// Fire puffs spawned.
    pub fire_spawned: u64,
    /// Passes that found the gate open.
    pub passes_burning: u64,
}

/// Emitter progress: where the anchor was last pass and the travel carried
/// toward the next puff of each emitter.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct DamageTrailCursor {
    last_anchor: Option<Vec3>,
    carried: [f32; 2],
    emitted: u64,
}

/// The stage's source, waiting for the plugin to build the engine assets.
#[derive(Resource, Clone, Debug)]
pub struct DamageTrailSourceResource(pub DamageTrailSource);

/// The definition and the engine textures built from it.
#[derive(Resource, Clone, Debug)]
pub struct DamageTrailArt {
    definition: DamageTrailDefinition,
    textures: BTreeMap<String, Handle<Image>>,
    mesh: Handle<Mesh>,
}

impl DamageTrailArt {
    /// Builds the art from handles already made (the tests' seam; the plugin
    /// builds it from [`DamageTrailSourceResource`]).
    #[must_use]
    pub fn from_handles(
        definition: DamageTrailDefinition,
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

/// Travel along `previous -> now` that lands whole intervals, as the points
/// where puffs are born, and the remainder carried to the next pass.
#[must_use]
pub fn plan_emissions(previous: Vec3, now: Vec3, carried: f32, interval: f32) -> (Vec<Vec3>, f32) {
    let length = previous.distance(now);
    if !(interval.is_finite() && interval > 0.0 && length.is_finite()) {
        return (Vec::new(), carried);
    }
    let mut points = Vec::new();
    let mut at = interval - carried;
    while at <= length {
        points.push(previous.lerp(now, at / length.max(f32::EPSILON)));
        at += interval;
    }
    (points, length - (at - interval))
}

/// The tint of a puff at `fraction` of its life from the stored colour keys:
/// linear between keys, rgb `/255`; no keys is opaque white.
#[must_use]
pub fn colour_at(colors: &[[f32; 5]], fraction: f32) -> [f32; 4] {
    let Some(first) = colors.first() else {
        return [1.0, 1.0, 1.0, 1.0];
    };
    let key = |k: &[f32; 5]| [k[1] / 255.0, k[2] / 255.0, k[3] / 255.0, k[4]];
    if fraction <= first[0] {
        return key(first);
    }
    for pair in colors.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        if fraction <= b[0] {
            let span = (b[0] - a[0]).max(f32::EPSILON);
            let t = ((fraction - a[0]) / span).clamp(0.0, 1.0);
            let (ka, kb) = (key(a), key(b));
            return std::array::from_fn(|i| ka[i] + (kb[i] - ka[i]) * t);
        }
    }
    colors.last().map_or([1.0; 4], key)
}

/// The `TEXTURE_SEQUENCE` frame shown at `age_s`: the last frame whose time
/// has passed.
#[must_use]
pub fn frame_at(frames: &[(f32, String)], age_s: f32) -> Option<&str> {
    frames
        .iter()
        .take_while(|(time, _)| *time <= age_s)
        .last()
        .or_else(|| frames.first())
        .map(|(_, name)| name.as_str())
}

/// A small deterministic generator (splitmix64).
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

/// One puff of `emitter` born at `origin` on tick `born`.
#[must_use]
pub fn plan_puff(
    emitter: &TrailEmitter,
    kind: TrailKind,
    origin: Vec3,
    born: u64,
    index: u64,
    tick_hz: f32,
) -> TrailPuff {
    let mut rng = Splitmix(born ^ index.rotate_left(17) ^ (kind as u64) << 60);
    let life_s = rng.between(emitter.lifetime_s).max(0.0);
    let velocity = Vec3::new(
        rng.between([emitter.min_velocity[0], emitter.max_velocity[0]]),
        rng.between([emitter.min_velocity[1], emitter.max_velocity[1]]),
        rng.between([emitter.min_velocity[2], emitter.max_velocity[2]]),
    );
    TrailPuff {
        kind,
        born,
        life_ticks: (life_s * tick_hz).ceil().max(1.0) as u64,
        origin,
        velocity,
        size: rng.between(emitter.size),
        friction: emitter.friction,
        cycle_index: if emitter.textures.is_empty() {
            0
        } else {
            index as usize % emitter.textures.len()
        },
    }
}

/// A puff's position at `age_ticks`.
#[must_use]
pub fn trail_puff_position(puff: &TrailPuff, age_ticks: u64, tick_hz: f32) -> Vec3 {
    let t = age_ticks as f32 / tick_hz;
    let travelled = if puff.friction > f32::EPSILON {
        (1.0 - (-puff.friction * t).exp()) / puff.friction
    } else {
        t
    };
    puff.origin + puff.velocity * travelled
}

fn build_trail_art(
    source: Option<Res<DamageTrailSourceResource>>,
    art: Option<Res<DamageTrailArt>>,
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
    commands.insert_resource(DamageTrailArt {
        definition: source.0.definition.clone(),
        textures,
        mesh: meshes.add(Rectangle::new(1.0, 1.0)),
    });
}

/// The gate: any wing flare's published region state is damaged or destroyed.
fn wings_burning(flares: &Query<(&WingFlare, Option<&RegionDamagePresentation>)>) -> bool {
    flares.iter().any(|(_, record)| {
        record.is_some_and(|r| matches!(r.state, PartState::Damaged | PartState::Destroyed))
    })
}

#[allow(clippy::too_many_arguments)]
fn emit_damage_trail(
    mut commands: Commands,
    art: Option<Res<DamageTrailArt>>,
    ledger: Option<Res<PhysicsTickLedger>>,
    mut cursor: ResMut<DamageTrailCursor>,
    mut report: ResMut<DamageTrailReport>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    flares: Query<(&WingFlare, Option<&RegionDamagePresentation>)>,
    anchors: Query<&GlobalTransform, With<PropellerBlur>>,
) {
    let (Some(art), Some(ledger)) = (art, ledger) else {
        return;
    };
    let Some(anchor) = anchors.iter().next().map(GlobalTransform::translation) else {
        return;
    };
    let previous = cursor.last_anchor.replace(anchor);
    if !wings_burning(&flares) {
        cursor.carried = [0.0; 2];
        return;
    }
    report.passes_burning += 1;
    let Some(previous) = previous else {
        return;
    };
    let tick_hz = if ledger.timestep_s > 0.0 {
        1.0 / ledger.timestep_s
    } else {
        120.0
    };
    let emitters = [
        (TrailKind::Smoke, &art.definition.smoke),
        (TrailKind::Fire, &art.definition.fire),
    ];
    for (slot, (kind, emitter)) in emitters.into_iter().enumerate() {
        let (points, carried) = plan_emissions(
            previous,
            anchor,
            cursor.carried[slot],
            emitter.distance_interval,
        );
        cursor.carried[slot] = carried;
        for point in points {
            cursor.emitted += 1;
            let puff = plan_puff(emitter, kind, point, ledger.ticks, cursor.emitted, tick_hz);
            let first = match kind {
                TrailKind::Smoke => emitter.textures.get(puff.cycle_index),
                TrailKind::Fire => emitter.frames.first().map(|(_, name)| name),
            };
            let material = materials.add(StandardMaterial {
                base_color: Color::WHITE,
                base_color_texture: first.and_then(|name| art.textures.get(name)).cloned(),
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                cull_mode: None,
                ..Default::default()
            });
            commands.spawn((
                puff,
                Mesh3d(art.mesh.clone()),
                MeshMaterial3d(material),
                Transform::from_translation(point).with_scale(Vec3::splat(puff.size)),
                Visibility::Visible,
            ));
            match kind {
                TrailKind::Smoke => report.smoke_spawned += 1,
                TrailKind::Fire => report.fire_spawned += 1,
            }
        }
    }
}

fn advance_trail_puffs(
    mut commands: Commands,
    art: Option<Res<DamageTrailArt>>,
    ledger: Option<Res<PhysicsTickLedger>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut puffs: Query<(
        Entity,
        &TrailPuff,
        &mut Transform,
        &MeshMaterial3d<StandardMaterial>,
    )>,
) {
    let (Some(art), Some(ledger)) = (art, ledger) else {
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
        transform.translation = trail_puff_position(puff, age, tick_hz);
        let emitter = match puff.kind {
            TrailKind::Smoke => &art.definition.smoke,
            TrailKind::Fire => &art.definition.fire,
        };
        let fraction = age as f32 / puff.life_ticks as f32;
        let [r, g, b, a] = colour_at(&emitter.colors, fraction);
        let name = match puff.kind {
            TrailKind::Smoke => emitter.textures.get(puff.cycle_index).map(String::as_str),
            TrailKind::Fire => frame_at(&emitter.frames, age as f32 / tick_hz),
        };
        if let Some(mut material) = materials.get_mut(&material.0) {
            material.base_color = Color::srgba(r, g, b, a);
            if let Some(handle) = name.and_then(|n| art.textures.get(n)) {
                material.base_color_texture = Some(handle.clone());
            }
        }
    }
}

/// Turns every trail puff to face the camera, each rendered frame.
fn face_trail_puffs(
    cameras: Query<&Transform, (With<Camera3d>, Without<TrailPuff>)>,
    mut puffs: Query<&mut Transform, With<TrailPuff>>,
) {
    let Some(camera) = cameras.iter().next() else {
        return;
    };
    for mut transform in &mut puffs {
        transform.rotation = camera.rotation;
    }
}

/// Installs the damage trail: the emit and advance passes on the fixed loop
/// and the facing pass on the frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct MissionDamageTrailPlugin;

impl Plugin for MissionDamageTrailPlugin {
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
        app.init_resource::<DamageTrailReport>()
            .init_resource::<DamageTrailCursor>()
            .add_systems(
                FixedPostUpdate,
                (build_trail_art, emit_damage_trail, advance_trail_puffs).chain(),
            )
            .add_systems(Update, face_trail_puffs);
    }
}
