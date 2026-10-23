//! The sparks a terrain contact throws (PARITY4-CRASH-D.02, #1945).
//!
//! The original's animation `yellow_sparks_follow` (`pufftrails.zrd`, reader
//! archive `ZBD/zrdr.zbd`) is the spark effect of a contact: its `fly_flare`
//! sequence calls `sparktrail` (switching the `PUFFER_STATE` `trailpuffer2`
//! on) and `chiptrail` (switching `chippuffer1` on) at once, then
//! `stop_chiptrail` at `EVENT_OFFSET 0.15` and `stop_p1trail` at `0.25`, which
//! set the two puffers `INACTIVE`
//! (`docs/findings/2026-10-23-crash-d-02-contact-sparks.md`). This module
//! reads both emitters and their switch-off times from the retail archive
//! ([`read_contact_spark_definition`]), decodes their textures from the flown
//! world's texture archive ([`measure_contact_sparks`]) and draws the sparks
//! at the point of every [`TerrainContactJudged`], fatal or not.
//!
//! # What is applied, what is designed, what is not
//!
//! Applied as stored: `NUMBER` particles per emission, `TIME_INTERVAL`
//! seconds between emissions from the contact until the switch-off time,
//! `SIZE_RANGE` (metres), `LIFETIME_RANGE` (seconds), a velocity per axis
//! from `MIN_`/`MAX_RANDOM_VELOCITY`, `WORLD_ACCELERATION` (metres per second
//! squared, world frame), one of the `TEXTURES` per particle.
//!
//! **Designed, filed under [`CONTACT_SPARK_MOTION_CLAIM`]:** `FRICTION` is a
//! linear drag rate (`dv/dt = a - friction * v`), "`NUMBER` per `TIME_INTERVAL`
//! emission until the switch-off" is the reading of interval versus number,
//! a particle fades linearly over its life, the quad faces the camera.
//!
//! **Not applied (unmeasured meaning):** `LOCAL_VELOCITY`, the six words of
//! `AT_NODE`, `DEVIATION_DISTANCE`, `FADE_RANGE`, `GROWTH_FACTOR` (stored 0),
//! `COLORS` and the blend mode. The `sound` sequence (`snd_ricochet1..4`) is
//! not played.

use std::fmt;
use std::path::Path;

use bevy::asset::Assets;
use bevy::mesh::Mesh;
use bevy::prelude::{
    AlphaMode, App, Camera3d, Color, Commands, Component, Entity, FixedPostUpdate, Handle, Image,
    IntoScheduleConfigs, Mesh3d, MeshMaterial3d, MessageReader, Plugin, Query, Rectangle, Res,
    ResMut, Resource, StandardMaterial, Transform, Update, Vec3, Visibility, With, Without,
};
use cs_assets::install;
use cs_formats::texture::DecodedImage;
use cs_formats::zbd::{
    EffectAnimation, EffectEvent, PufferState, read_animation_definitions, read_reader_nodes,
};

use super::burst_puffs::Splitmix;
use super::launch_sound::reader_member;
use crate::physics::PhysicsTickLedger;
use crate::playtest::fatal_contact::TerrainContactJudged;
use crate::playtest_textures::{PlaytestTextureArchive, to_bevy_image};

/// The claim the sparks' drag, emission reading, fade and facing are filed under.
pub const CONTACT_SPARK_MOTION_CLAIM: &str = "parity4-crash-d-02.designed-spark-motion";

/// The reader archive and member holding the animation.
pub const CONTACT_SPARK_ARCHIVE: &str = "ZBD/zrdr.zbd";
/// The member of [`CONTACT_SPARK_ARCHIVE`].
pub const CONTACT_SPARK_MEMBER: &str = "pufftrails.zrd";
/// The animation id.
pub const CONTACT_SPARK_ANIMATION: &str = "yellow_sparks_follow";
/// The sequence that starts the effect and schedules the switch-offs.
pub const CONTACT_SPARK_START_SEQUENCE: &str = "fly_flare";

/// One stored spark emitter and the time the animation switches it off.
#[derive(Clone, Debug, PartialEq)]
pub struct SparkEmitter {
    /// The `PUFFER_STATE` `NAME`.
    pub name: String,
    /// `NUMBER`: particles per emission.
    pub number: u32,
    /// `TIME_INTERVAL`, seconds.
    pub time_interval_s: f32,
    /// `SIZE_RANGE`, metres.
    pub size: [f32; 2],
    /// `LIFETIME_RANGE`, seconds.
    pub lifetime_s: [f32; 2],
    /// `MIN_RANDOM_VELOCITY`.
    pub min_velocity: [f32; 3],
    /// `MAX_RANDOM_VELOCITY`.
    pub max_velocity: [f32; 3],
    /// `WORLD_ACCELERATION`.
    pub acceleration: [f32; 3],
    /// `FRICTION`.
    pub friction: f32,
    /// `TEXTURES`, in stored order.
    pub textures: Vec<String>,
    /// The `EVENT_OFFSET` of the `CALL_SEQUENCE` whose sequence sets this
    /// puffer `INACTIVE`, seconds after the effect starts.
    pub off_after_s: f32,
}

/// The two emitters of `yellow_sparks_follow`.
#[derive(Clone, Debug, PartialEq)]
pub struct ContactSparkDefinition {
    /// Emitters, in stored order (`trailpuffer2`, `chippuffer1`).
    pub emitters: Vec<SparkEmitter>,
}

/// The definition and its decoded textures, as the stage carries them.
#[derive(Clone, Debug)]
pub struct ContactSparkSource {
    /// The emitters.
    pub definition: ContactSparkDefinition,
    /// Every distinct texture name with its decoded image.
    pub images: Vec<(String, DecodedImage)>,
    /// The archive the textures came from.
    pub texture_archive: String,
}

/// Why the contact sparks could not be measured.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContactSparkError {
    /// The archive, member or token stream could not be read.
    Read(String),
    /// The record lacks a field the sparks need.
    Unmeasured(String),
    /// A texture the emitters name did not resolve or decode.
    Texture(String),
}

impl fmt::Display for ContactSparkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(r) => write!(f, "contact spark definition unreadable: {r}"),
            Self::Unmeasured(r) => write!(f, "contact spark definition unmeasured: {r}"),
            Self::Texture(r) => write!(f, "contact spark texture unavailable: {r}"),
        }
    }
}

impl std::error::Error for ContactSparkError {}

fn event_offset(call: &cs_formats::zbd::ReaderNode<'_>) -> Option<f32> {
    let start = call.field("START_TIME")?.as_list()?;
    match start {
        [clock, seconds] if clock.as_str() == Some("EVENT_OFFSET") => seconds.as_number(),
        _ => None,
    }
}

fn emitter_of(
    puffer: &PufferState<'_>,
    off_after_s: f32,
) -> Result<SparkEmitter, ContactSparkError> {
    let need = |what: &str| ContactSparkError::Unmeasured(format!("{} has no {what}", puffer.name));
    let number = puffer.number.ok_or_else(|| need("NUMBER"))?;
    if !(number.is_finite() && number >= 1.0) {
        return Err(need("a positive NUMBER"));
    }
    let time_interval_s = puffer.time_interval.ok_or_else(|| need("TIME_INTERVAL"))?;
    if !(time_interval_s.is_finite() && time_interval_s > 0.0) {
        return Err(need("a positive TIME_INTERVAL"));
    }
    if puffer.textures.is_empty() {
        return Err(need("TEXTURES"));
    }
    Ok(SparkEmitter {
        name: puffer.name.to_owned(),
        number: number as u32,
        time_interval_s,
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
        acceleration: puffer
            .world_acceleration
            .ok_or_else(|| need("WORLD_ACCELERATION"))?,
        friction: puffer.friction.ok_or_else(|| need("FRICTION"))?,
        textures: puffer.textures.iter().map(|t| (*t).to_owned()).collect(),
        off_after_s,
    })
}

/// Finds the switch-off time of puffer `name`: the `EVENT_OFFSET` of the
/// `fly_flare` `CALL_SEQUENCE` that names a sequence setting it `INACTIVE`.
fn switch_off(animation: &EffectAnimation<'_>, name: &str) -> Option<f32> {
    let start = animation
        .sequences
        .iter()
        .find(|s| s.name == Some(CONTACT_SPARK_START_SEQUENCE))?;
    start.events.iter().find_map(|event| {
        let EffectEvent::Other { keyword, value } = event else {
            return None;
        };
        if *keyword != "CALL_SEQUENCE" {
            return None;
        }
        let target = value.field("NAME")?.as_list()?.first()?.as_str()?;
        let offset = event_offset(value)?;
        let sequence = animation
            .sequences
            .iter()
            .find(|s| s.name == Some(target))?;
        sequence
            .events
            .iter()
            .any(|e| matches!(e, EffectEvent::Puffer(p) if p.name == name && p.active == Some(false)))
            .then_some(offset)
    })
}

/// Reads both emitters and their switch-off times from the retail archive.
///
/// # Errors
///
/// [`ContactSparkError`] when the archive cannot be read, the animation is
/// absent, or an emitter lacks a field or a switch-off.
pub fn read_contact_spark_definition(
    install_root: &Path,
) -> Result<ContactSparkDefinition, ContactSparkError> {
    let bytes = reader_member(install_root, CONTACT_SPARK_ARCHIVE, CONTACT_SPARK_MEMBER)
        .map_err(ContactSparkError::Read)?;
    let nodes = read_reader_nodes(&bytes).map_err(|e| ContactSparkError::Read(e.to_string()))?;
    let animations =
        read_animation_definitions(&nodes).map_err(|e| ContactSparkError::Read(e.to_string()))?;
    let animation = animations
        .iter()
        .find(|a| a.id() == CONTACT_SPARK_ANIMATION)
        .ok_or_else(|| {
            ContactSparkError::Unmeasured(format!("no animation {CONTACT_SPARK_ANIMATION}"))
        })?;
    let mut emitters = Vec::new();
    for puffer in animation.puffers().filter(|p| p.active == Some(true)) {
        let off = switch_off(animation, puffer.name).ok_or_else(|| {
            ContactSparkError::Unmeasured(format!("no switch-off of {}", puffer.name))
        })?;
        emitters.push(emitter_of(puffer, off)?);
    }
    if emitters.is_empty() {
        return Err(ContactSparkError::Unmeasured(
            "no active emitter".to_owned(),
        ));
    }
    Ok(ContactSparkDefinition { emitters })
}

/// Reads the emitters and decodes their textures from the world group's own
/// texture archive.
///
/// # Errors
///
/// [`ContactSparkError`] when the definition or a texture is unavailable.
pub fn measure_contact_sparks(
    install_root: &Path,
    world_group: &str,
) -> Result<ContactSparkSource, ContactSparkError> {
    let definition = read_contact_spark_definition(install_root)?;
    let found =
        install::discover(install_root).map_err(|e| ContactSparkError::Texture(e.to_string()))?;
    let archive = PlaytestTextureArchive::open(install_root, &found, world_group)
        .map_err(|e| ContactSparkError::Texture(e.to_string()))?;
    let mut images: Vec<(String, DecodedImage)> = Vec::new();
    for name in definition.emitters.iter().flat_map(|e| &e.textures) {
        if images.iter().any(|(known, _)| known == name) {
            continue;
        }
        let (_, image) = archive
            .fetch(name)
            .map_err(|(why, code)| ContactSparkError::Texture(format!("{name}: {code}: {why}")))?;
        images.push((name.clone(), image));
    }
    Ok(ContactSparkSource {
        definition,
        images,
        texture_archive: archive.path().to_owned(),
    })
}

/// Component: one live spark.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct ContactSpark {
    /// The emitter it came from.
    pub emitter: usize,
    /// Tick it is born on (emissions after the contact are born later).
    pub born: u64,
    /// Ticks it lives.
    pub life_ticks: u64,
    /// Where it started.
    pub origin: Vec3,
    /// Its initial velocity, metres per second.
    pub velocity: Vec3,
    /// Its edge length, metres.
    pub size: f32,
    /// Index into the emitter's `textures`.
    pub texture: usize,
    /// `WORLD_ACCELERATION` of the emitter.
    pub acceleration: Vec3,
    /// `FRICTION` of the emitter.
    pub friction: f32,
}

/// How many contacts have been drawn and sparks spawned, for the smoke check
/// and the tests.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ContactSparkReport {
    /// Contacts drawn.
    pub contacts_drawn: u64,
    /// Contacts dropped because no definition is installed.
    pub contacts_undrawn: u64,
    /// Sparks spawned.
    pub sparks_spawned: u64,
}

/// The installed definition and the engine textures built from it.
#[derive(Resource, Clone, Debug)]
pub(super) struct ContactSparkArt {
    definition: ContactSparkDefinition,
    textures: Vec<(String, Handle<Image>)>,
    mesh: Handle<Mesh>,
}

/// The stage's source, waiting for the plugin to build the engine assets.
#[derive(Resource, Clone, Debug)]
pub struct ContactSparkSourceResource(pub ContactSparkSource);

/// The sparks of one contact at `tick`, as plain data: for every emitter,
/// `NUMBER` particles at every `TIME_INTERVAL` from the contact up to (not
/// including) the switch-off time.
#[must_use]
pub fn plan_contact_sparks(
    definition: &ContactSparkDefinition,
    tick: u64,
    point: Vec3,
    tick_hz: f32,
) -> Vec<ContactSpark> {
    let seed = tick
        ^ u64::from(point.x.to_bits())
        ^ (u64::from(point.y.to_bits()) << 21)
        ^ (u64::from(point.z.to_bits()) << 42);
    let mut rng = Splitmix(seed);
    let mut sparks = Vec::new();
    for (index, emitter) in definition.emitters.iter().enumerate() {
        let mut emission = 0u32;
        loop {
            let at_s = emission as f32 * emitter.time_interval_s;
            if at_s >= emitter.off_after_s - 1e-4 {
                break;
            }
            let born = tick + (at_s * tick_hz).round() as u64;
            for _ in 0..emitter.number {
                let life_s = rng.between(emitter.lifetime_s).max(0.0);
                let axis = |rng: &mut Splitmix, i: usize| {
                    rng.between([emitter.min_velocity[i], emitter.max_velocity[i]])
                };
                let velocity = Vec3::new(axis(&mut rng, 0), axis(&mut rng, 1), axis(&mut rng, 2));
                sparks.push(ContactSpark {
                    emitter: index,
                    born,
                    life_ticks: (life_s * tick_hz).ceil().max(1.0) as u64,
                    origin: point,
                    velocity,
                    size: rng.between(emitter.size),
                    texture: (rng.next_unit() * emitter.textures.len() as f32) as usize
                        % emitter.textures.len().max(1),
                    acceleration: Vec3::from(emitter.acceleration),
                    friction: emitter.friction,
                });
            }
            emission += 1;
        }
    }
    sparks
}

/// A spark's position and opacity at `age_ticks`.
#[must_use]
pub fn spark_pose(spark: &ContactSpark, age_ticks: u64, tick_hz: f32) -> (Vec3, f32) {
    let t = age_ticks as f32 / tick_hz;
    let fade = 1.0 - (age_ticks as f32 / spark.life_ticks as f32).clamp(0.0, 1.0);
    if spark.friction <= f32::EPSILON {
        let position = spark.origin + spark.velocity * t + spark.acceleration * (0.5 * t * t);
        return (position, fade);
    }
    // dv/dt = a - f v: v = a/f + (v0 - a/f) e^{-ft}
    let f = spark.friction;
    let terminal = spark.acceleration / f;
    let decay = (1.0 - (-f * t).exp()) / f;
    (
        spark.origin + terminal * t + (spark.velocity - terminal) * decay,
        fade,
    )
}

fn build_art(
    source: Option<Res<ContactSparkSourceResource>>,
    art: Option<Res<ContactSparkArt>>,
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
    commands.insert_resource(ContactSparkArt {
        definition: source.0.definition.clone(),
        textures,
        mesh: meshes.add(Rectangle::new(1.0, 1.0)),
    });
}

fn spawn_contact_sparks(
    mut commands: Commands,
    mut judged: MessageReader<TerrainContactJudged>,
    mut report: ResMut<ContactSparkReport>,
    art: Option<Res<ContactSparkArt>>,
    ledger: Option<Res<PhysicsTickLedger>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let contacts: Vec<_> = judged.read().copied().collect();
    if contacts.is_empty() {
        return;
    }
    let Some(art) = art else {
        report.contacts_undrawn += contacts.len() as u64;
        return;
    };
    let tick_hz = ledger
        .as_ref()
        .filter(|l| l.timestep_s > 0.0)
        .map_or(120.0, |l| 1.0 / l.timestep_s);
    for contact in contacts {
        report.contacts_drawn += 1;
        for spark in plan_contact_sparks(&art.definition, contact.tick, contact.point_m, tick_hz) {
            let name = &art.definition.emitters[spark.emitter].textures[spark.texture];
            let material = materials.add(StandardMaterial {
                base_color: Color::WHITE,
                base_color_texture: art
                    .textures
                    .iter()
                    .find(|(known, _)| known == name)
                    .map(|(_, handle)| handle.clone()),
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                cull_mode: None,
                ..Default::default()
            });
            commands.spawn((
                Mesh3d(art.mesh.clone()),
                MeshMaterial3d(material),
                Transform::from_translation(spark.origin).with_scale(Vec3::splat(spark.size)),
                Visibility::Hidden,
                spark,
            ));
            report.sparks_spawned += 1;
        }
    }
}

fn advance_contact_sparks(
    mut commands: Commands,
    ledger: Option<Res<PhysicsTickLedger>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut sparks: Query<(
        Entity,
        &ContactSpark,
        &mut Transform,
        &mut Visibility,
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
    for (entity, spark, mut transform, mut visibility, material) in &mut sparks {
        if ledger.ticks < spark.born {
            continue;
        }
        let age = ledger.ticks - spark.born;
        if age >= spark.life_ticks {
            commands.entity(entity).despawn();
            continue;
        }
        let (position, opacity) = spark_pose(spark, age, tick_hz);
        transform.translation = position;
        *visibility = Visibility::Visible;
        if let Some(mut material) = materials.get_mut(&material.0) {
            material.base_color = Color::srgba(1.0, 1.0, 1.0, opacity);
        }
    }
}

fn face_contact_sparks(
    cameras: Query<&Transform, (With<Camera3d>, Without<ContactSpark>)>,
    mut sparks: Query<&mut Transform, With<ContactSpark>>,
) {
    let Some(camera) = cameras.iter().next() else {
        return;
    };
    for mut transform in &mut sparks {
        transform.rotation = camera.rotation;
    }
}

/// Installs the contact sparks: the spawn and advance passes on the fixed
/// loop, the facing pass on the frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct MissionContactSparkPlugin;

impl Plugin for MissionContactSparkPlugin {
    fn build(&self, app: &mut App) {
        use bevy::asset::AssetApp;
        if !app.world().contains_resource::<Assets<StandardMaterial>>() {
            app.init_asset::<StandardMaterial>();
        }
        if !app.world().contains_resource::<Assets<Mesh>>() {
            app.init_asset::<Mesh>();
        }
        if !app.world().contains_resource::<Assets<Image>>() {
            app.init_asset::<Image>();
        }
        app.add_message::<TerrainContactJudged>()
            .init_resource::<ContactSparkReport>()
            .add_systems(
                FixedPostUpdate,
                (build_art, spawn_contact_sparks, advance_contact_sparks).chain(),
            )
            .add_systems(Update, face_contact_sparks);
    }
}
