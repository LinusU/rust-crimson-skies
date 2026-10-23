//! The fire a damaged player wing keeps burning (PARITY4-CRASH-D.03, #1946).
//!
//! The original's `destroyable_parts` of the player airframe carry an
//! `injure_anims` ladder per wing whose `pdpanelN` entries start the persistent
//! fire on the wing's `pdpN` node
//! (`docs/findings/2026-10-21-parity2-dmg-fx-a-effect-definitions.md`): port
//! `pdpanel5`/`4`/`3` at 0.50/0.30/0.15, starboard `pdpanel6`/`1`/`2` at
//! 0.40/0.30/0.15. This module reads that ladder from `ZBD/zrdr.zbd`
//! ([`read_wing_fire_definition`]), follows each wing region node's remaining
//! integrity in the session's damage authority, and from the tick a stage's
//! threshold is crossed draws the stage's measured effects
//! (`small_fireball_follow`, `short_firetrail`, `loop_short_firetrail`,
//! `large_firetrail`) at the `pdpN` node of the drawn airframe, following it
//! until the run ends. The emitters are the measured `PUFFER_STATE` records,
//! drawn with the particle runtime of the damage trail
//! ([`super::damage_trail`]): the same planner, puff model, colour keys and
//! frame lookup.
//!
//! # Applied as stored
//!
//! The ladder (thresholds, `pdpanelN` names, nodes), each effect's call node
//! and `EVENT_OFFSET` delay, and every emitter that starts active with the
//! effect: `DISTANCE_INTERVAL`, `SIZE_RANGE`, `LIFETIME_RANGE`, the random
//! velocity (zero where the record states none), `FRICTION`, the texture
//! cycle or frame sequence, the colour keys, and an `INACTIVE` event that
//! carries its own `EVENT_OFFSET` (`small_fireball_follow`: off at +1 s).
//!
//! # Designed ([`WING_FIRE_DESIGN_CLAIM`])
//!
//! * The threshold direction: a stage starts when the region's remaining
//!   integrity over its pool falls **to or below** the stored number. The
//!   record stores no comparison.
//! * The pool is [`super::DESIGNED_REGION_INTEGRITY`]; the original's zone
//!   armor and pools are unknown, so *when* a contact reaches a threshold is
//!   designed too.
//! * A distance emitter on an anchor that does not move would never emit, so
//!   the anchor is credited [`WING_FIRE_MIN_TRAVEL_M_S`] of travel per second:
//!   the fire on a static wreck keeps burning.
//! * Units and curves as in the damage trail (frame times in seconds of puff
//!   age, colour keys as a fraction of life, `v * exp(-friction * t)`).
//!
//! # Not applied (unmeasured meaning)
//!
//! `AT_NODE` offset words, `GROWTH_FACTOR`, `FADE_RANGE`, the
//! `short_firetrail` stage hand-over (`CALL_SEQUENCE ... EVENT_OFFSET`
//! switch-offs; every stage runs on), the 50 % `RANDOM_WEIGHT` of its
//! `black_smoke`, the first-person `pcdpN` branch of `pdpanel4`/`6`
//! (`large_firetrail`), the `gimmeflakes` debris and `yellow_sparks_follow`
//! sparks, and the `snd_exp_hit5` sound.

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
use cs_formats::io::ParseContext;
use cs_formats::texture::DecodedImage;
use cs_formats::zbd::{
    EffectAnimation, PufferState, ReaderNode, read_animation_definitions, read_reader_nodes,
    read_vehicle_records,
};
use cs_formats::zbd::{ZbdProbe, dispatch, read_reader_archive, read_version_one_index};
use cs_sim::damage::DamageNodeKey;

use super::damage_trail::{TrailEmitter, TrailKind, TrailPuff, colour_at, frame_at, plan_puff};
use super::{DESIGNED_REGION_INTEGRITY, PlayerVictim};
use crate::airframe_damage::AirframeDamageRegion;
use crate::physics::PhysicsTickLedger;
use crate::playtest_retail::AircraftPart;
use crate::playtest_textures::{PlaytestTextureArchive, to_bevy_image};
use crate::weapons::SessionDamageResolver;

/// The claim the designed threshold direction, pool and static-anchor travel
/// are filed under.
pub const WING_FIRE_DESIGN_CLAIM: &str = "crash-d-03.designed-wing-fire";

/// The claim the unmeasured emitter meanings are filed under.
pub const WING_FIRE_UNKNOWN_CLAIM: &str = "crash-d-03.wing-fire-emitter-units-unmeasured";

/// The reader archive holding the records.
pub const WING_FIRE_ARCHIVE: &str = "ZBD/zrdr.zbd";
/// The member holding the vehicle records.
pub const WING_FIRE_VEHICLE_MEMBER: &str = "vehicle.zrd";
/// The member holding the `pdpanelN` animations.
pub const WING_FIRE_PANEL_MEMBER: &str = "player.zrd";
/// The member holding the effect animations.
pub const WING_FIRE_EFFECT_MEMBER: &str = "pufftrails.zrd";
/// The vehicle record of the player's Devastator.
pub const WING_FIRE_VEHICLE_KEY: &str = "pdevastator";

/// The effect animations a `pdpanelN` calls that this module draws.
pub const WING_FIRE_EFFECTS: [&str; 4] = [
    "small_fireball_follow",
    "short_firetrail",
    "loop_short_firetrail",
    "large_firetrail",
];

/// The travel per second credited to a distance emitter whose anchor is not
/// moving, metres. Designed ([`WING_FIRE_DESIGN_CLAIM`]).
pub const WING_FIRE_MIN_TRAVEL_M_S: f32 = 4.0;

/// One emitter of a drawn effect.
#[derive(Clone, Debug, PartialEq)]
pub struct WingFireEmitter {
    /// The `PUFFER_STATE` `NAME`.
    pub name: String,
    /// What the record states.
    pub emitter: TrailEmitter,
    /// Seconds after the effect starts at which its own `INACTIVE` event
    /// switches it off, when the record carries one.
    pub off_after_s: Option<f32>,
}

/// One effect a `pdpanelN` calls.
#[derive(Clone, Debug, PartialEq)]
pub struct WingFireEffect {
    /// The called animation.
    pub effect: String,
    /// The `WITH_NODE` the effect follows (`pdp3`).
    pub node: String,
    /// `EVENT_OFFSET` of the call, seconds after the stage starts.
    pub delay_s: f32,
    /// The emitters that start active with the effect.
    pub emitters: Vec<WingFireEmitter>,
}

/// One rung of a wing's `injure_anims` ladder that starts a fire.
#[derive(Clone, Debug, PartialEq)]
pub struct WingFireStage {
    /// The wing the rung belongs to.
    pub region: AirframeDamageRegion,
    /// The stored threshold.
    pub threshold: f32,
    /// The `pdpanelN` animation.
    pub panel: String,
    /// The effects it calls on `pdpN` nodes.
    pub effects: Vec<WingFireEffect>,
}

/// The measured ladders of both wings.
#[derive(Clone, Debug, PartialEq)]
pub struct WingFireDefinition {
    /// Every fire rung, port then starboard, threshold descending.
    pub stages: Vec<WingFireStage>,
}

impl WingFireDefinition {
    /// Every texture name any emitter draws, once each.
    #[must_use]
    pub fn texture_names(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for stage in &self.stages {
            for effect in &stage.effects {
                for emitter in &effect.emitters {
                    let all = emitter
                        .emitter
                        .textures
                        .iter()
                        .chain(emitter.emitter.frames.iter().map(|(_, name)| name));
                    for name in all {
                        if !names.contains(name) {
                            names.push(name.clone());
                        }
                    }
                }
            }
        }
        names
    }
}

/// The definition and its decoded textures, as the stage carries them.
#[derive(Clone, Debug)]
pub struct WingFireSource {
    /// The ladders.
    pub definition: WingFireDefinition,
    /// `(name, image)` for each of [`WingFireDefinition::texture_names`].
    pub images: Vec<(String, DecodedImage)>,
    /// The archive the textures came from.
    pub texture_archive: String,
}

/// Why the wing fire could not be measured.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WingFireError {
    /// The archive, member or token stream could not be read.
    Read(String),
    /// A record lacks a field the fire needs.
    Unmeasured(String),
    /// A texture the emitters name did not resolve or decode.
    Texture(String),
}

impl fmt::Display for WingFireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(r) => write!(f, "wing fire definition unreadable: {r}"),
            Self::Unmeasured(r) => write!(f, "wing fire definition unmeasured: {r}"),
            Self::Texture(r) => write!(f, "wing fire texture unavailable: {r}"),
        }
    }
}

impl std::error::Error for WingFireError {}

fn emitter_of(puffer: &PufferState<'_>) -> Option<TrailEmitter> {
    if puffer.textures.is_empty() && puffer.texture_sequence.is_empty() {
        return None;
    }
    Some(TrailEmitter {
        distance_interval: puffer
            .distance_interval
            .filter(|d| d.is_finite() && *d > 0.0)?,
        size: puffer.size_range?,
        lifetime_s: puffer.lifetime_range?,
        min_velocity: puffer.min_random_velocity.unwrap_or([0.0; 3]),
        max_velocity: puffer.max_random_velocity.unwrap_or([0.0; 3]),
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

/// The emitters an effect starts active, with their own switch-off offsets.
fn effect_emitters(animation: &EffectAnimation<'_>) -> Vec<WingFireEmitter> {
    let mut out: Vec<WingFireEmitter> = Vec::new();
    for puffer in animation.puffers() {
        if puffer.active != Some(true)
            || puffer.start_time.is_some()
            || out.iter().any(|e| e.name == puffer.name)
        {
            continue;
        }
        let Some(emitter) = emitter_of(puffer) else {
            continue;
        };
        let off_after_s = animation
            .puffers()
            .find(|p| p.name == puffer.name && p.active == Some(false))
            .and_then(|p| p.start_time)
            .map(|(_, seconds)| seconds);
        out.push(WingFireEmitter {
            name: puffer.name.to_owned(),
            emitter,
            off_after_s,
        });
    }
    out
}

/// Every member of the reader archive named `member`: the archive holds
/// several members under one name (`player.zrd` is stored twice), of which
/// only some have the shape a caller reads.
fn reader_members(install_root: &Path, member: &str) -> Result<Vec<Vec<u8>>, WingFireError> {
    let read = |why: String| WingFireError::Read(format!("{WING_FIRE_ARCHIVE}: {why}"));
    let bytes =
        std::fs::read(install_root.join(WING_FIRE_ARCHIVE)).map_err(|e| read(e.to_string()))?;
    let spelling =
        cs_types::install::RelativePath::new(WING_FIRE_ARCHIVE).map_err(|e| read(e.to_string()))?;
    let mut context = ParseContext::with_defaults("wing_fire");
    let decision = dispatch(ZbdProbe::new(WING_FIRE_ARCHIVE, &spelling, &bytes))
        .map_err(|e| read(e.to_string()))?;
    let index =
        read_version_one_index(&mut context, decision, &bytes).map_err(|e| read(e.to_string()))?;
    let table = index.member_table();
    let archive =
        read_reader_archive(&mut context, &table, index.data()).map_err(|e| read(e.to_string()))?;
    let found: Vec<Vec<u8>> = archive
        .entries()
        .filter(|entry| String::from_utf8_lossy(entry.name()).eq_ignore_ascii_case(member))
        .map(|entry| entry.content().to_vec())
        .collect();
    if found.is_empty() {
        return Err(read(format!("no member {member}")));
    }
    Ok(found)
}

fn decode_all(members: &[Vec<u8>]) -> Result<Vec<Vec<ReaderNode<'_>>>, WingFireError> {
    members
        .iter()
        .map(|bytes| read_reader_nodes(bytes).map_err(|e| WingFireError::Read(e.to_string())))
        .collect()
}

fn animations_of<'a>(nodes: &'a [Vec<ReaderNode<'a>>]) -> Vec<EffectAnimation<'a>> {
    nodes
        .iter()
        .filter_map(|n| read_animation_definitions(n).ok())
        .flatten()
        .collect()
}

/// Reads both wings' fire ladders from the retail reader archive.
///
/// # Errors
///
/// [`WingFireError`] when a member cannot be read, the vehicle record or a
/// `pdpanelN` animation is missing, or an effect has no drawable emitter.
pub fn read_wing_fire_definition(install_root: &Path) -> Result<WingFireDefinition, WingFireError> {
    let vehicle_bytes = reader_members(install_root, WING_FIRE_VEHICLE_MEMBER)?;
    let panel_bytes = reader_members(install_root, WING_FIRE_PANEL_MEMBER)?;
    let effect_bytes = reader_members(install_root, WING_FIRE_EFFECT_MEMBER)?;
    let vehicle_nodes = decode_all(&vehicle_bytes)?;
    let panel_nodes = decode_all(&panel_bytes)?;
    let effect_nodes = decode_all(&effect_bytes)?;
    let vehicles: Vec<_> = vehicle_nodes
        .iter()
        .filter_map(|nodes| read_vehicle_records(nodes).ok())
        .flatten()
        .collect();
    let vehicle = vehicles
        .iter()
        .find(|v| v.key == WING_FIRE_VEHICLE_KEY)
        .ok_or_else(|| WingFireError::Unmeasured(format!("no vehicle {WING_FIRE_VEHICLE_KEY}")))?;
    let panels = animations_of(&panel_nodes);
    let effects = animations_of(&effect_nodes);

    let mut stages = Vec::new();
    for (part_name, region) in [
        ("leftwing", AirframeDamageRegion::LeftWing),
        ("rightwing", AirframeDamageRegion::RightWing),
    ] {
        let part = vehicle
            .destroyable_parts
            .iter()
            .find(|p| p.name == part_name)
            .ok_or_else(|| WingFireError::Unmeasured(format!("no destroyable part {part_name}")))?;
        for rung in part
            .injure_anims
            .iter()
            .filter(|r| r.animation.starts_with("pdpanel"))
        {
            let panel = panels
                .iter()
                .find(|a| a.id() == rung.animation)
                .ok_or_else(|| {
                    WingFireError::Unmeasured(format!("no animation {}", rung.animation))
                })?;
            let mut called = Vec::new();
            for call in panel.calls() {
                let (Some(node), true) = (
                    call.with_node.filter(|n| n.starts_with("pdp")),
                    WING_FIRE_EFFECTS.contains(&call.name),
                ) else {
                    continue;
                };
                let effect = effects
                    .iter()
                    .find(|a| a.id() == call.name)
                    .ok_or_else(|| {
                        WingFireError::Unmeasured(format!("no animation {}", call.name))
                    })?;
                let emitters = effect_emitters(effect);
                if emitters.is_empty() {
                    return Err(WingFireError::Unmeasured(format!(
                        "{} has no drawable active emitter",
                        call.name
                    )));
                }
                called.push(WingFireEffect {
                    effect: call.name.to_owned(),
                    node: node.to_owned(),
                    delay_s: call.start_time.map_or(0.0, |(_, seconds)| seconds),
                    emitters,
                });
            }
            if called.is_empty() {
                return Err(WingFireError::Unmeasured(format!(
                    "{} calls no drawable effect on a pdp node",
                    rung.animation
                )));
            }
            stages.push(WingFireStage {
                region,
                threshold: rung.threshold,
                panel: rung.animation.to_owned(),
                effects: called,
            });
        }
    }
    Ok(WingFireDefinition { stages })
}

/// Reads the ladders and decodes their textures from the world group's own
/// texture archive.
///
/// # Errors
///
/// [`WingFireError`] when the definition or a texture is unavailable.
pub fn measure_wing_fire(
    install_root: &Path,
    world_group: &str,
) -> Result<WingFireSource, WingFireError> {
    let definition = read_wing_fire_definition(install_root)?;
    let found =
        install::discover(install_root).map_err(|e| WingFireError::Texture(e.to_string()))?;
    let archive = PlaytestTextureArchive::open(install_root, &found, world_group)
        .map_err(|e| WingFireError::Texture(e.to_string()))?;
    let mut images = Vec::new();
    for name in definition.texture_names() {
        let (_, image) = archive
            .fetch(&name)
            .map_err(|(why, code)| WingFireError::Texture(format!("{name}: {code}: {why}")))?;
        images.push((name, image));
    }
    Ok(WingFireSource {
        definition,
        images,
        texture_archive: archive.path().to_owned(),
    })
}

/// Whether a region holding `remaining` of `pool` has reached `threshold`
/// (designed direction: remaining fraction at or below the stored number).
#[must_use]
pub fn threshold_reached(remaining: f64, pool: f64, threshold: f32) -> bool {
    pool > 0.0 && remaining / pool <= f64::from(threshold)
}

/// Where distance-emitted puffs are born for an anchor moving
/// `previous -> now` over `dt_s`: one per `interval` of travel, the travel
/// credited at least [`WING_FIRE_MIN_TRAVEL_M_S`] so a static anchor still
/// emits. Returns the points and the travel carried to the next pass.
#[must_use]
pub fn plan_wing_emissions(
    previous: Vec3,
    now: Vec3,
    dt_s: f32,
    carried: f32,
    interval: f32,
) -> (Vec<Vec3>, f32) {
    let length = previous.distance(now);
    let travel = length.max(WING_FIRE_MIN_TRAVEL_M_S * dt_s);
    if !(interval.is_finite() && interval > 0.0 && travel.is_finite()) {
        return (Vec::new(), carried);
    }
    let mut points = Vec::new();
    let mut at = interval - carried;
    while at <= travel {
        points.push(previous.lerp(now, (at / travel).clamp(0.0, 1.0)));
        at += interval;
    }
    (points, travel - (at - interval))
}

/// Component: a drawn airframe node an effect can follow (`pdp3`).
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct WingFireNode {
    /// The authored node name.
    pub name: String,
}

/// Component: one live fire puff.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct WingFirePuff {
    /// The planned puff.
    pub puff: TrailPuff,
    /// `(stage, effect, emitter)` indices into the definition.
    pub emitter: [usize; 3],
}

/// One effect that is burning now.
#[derive(Clone, Debug, PartialEq)]
pub struct BurningEffect {
    /// The wing.
    pub region: AirframeDamageRegion,
    /// The `pdpanelN` that started it.
    pub panel: String,
    /// The effect animation.
    pub effect: String,
    /// The node it follows.
    pub node: String,
    /// Tick the stage started on.
    pub started: u64,
}

#[derive(Clone, Copy, Debug, Default)]
struct EmitterCursor {
    last_anchor: Option<Vec3>,
    carried: f32,
}

/// What the module is burning and has drawn.
#[derive(Resource, Clone, Debug, Default)]
pub struct WingFireState {
    started: BTreeMap<usize, u64>,
    cursors: BTreeMap<[usize; 3], EmitterCursor>,
    emitted: u64,
    /// Puffs spawned per node name.
    pub spawned_by_node: BTreeMap<String, u64>,
}

impl WingFireState {
    /// The stage indices burning now.
    pub fn burning_stages(&self) -> impl Iterator<Item = usize> + '_ {
        self.started.keys().copied()
    }
}

/// The stage's source, waiting for the plugin to build the engine assets.
#[derive(Resource, Clone, Debug)]
pub struct WingFireSourceResource(pub WingFireSource);

/// The definition and the engine textures built from it.
#[derive(Resource, Clone, Debug)]
pub struct WingFireArt {
    definition: WingFireDefinition,
    textures: BTreeMap<String, Handle<Image>>,
    mesh: Handle<Mesh>,
}

impl WingFireArt {
    /// Builds the art from handles already made (the tests' seam).
    #[must_use]
    pub fn from_handles(
        definition: WingFireDefinition,
        textures: BTreeMap<String, Handle<Image>>,
        mesh: Handle<Mesh>,
    ) -> Self {
        Self {
            definition,
            textures,
            mesh,
        }
    }

    /// The ladders.
    #[must_use]
    pub const fn definition(&self) -> &WingFireDefinition {
        &self.definition
    }
}

/// The effects burning now, from the state and the definition.
#[must_use]
pub fn burning_effects(
    state: &WingFireState,
    definition: &WingFireDefinition,
) -> Vec<BurningEffect> {
    state
        .started
        .iter()
        .filter_map(|(index, started)| definition.stages.get(*index).map(|s| (s, *started)))
        .flat_map(|(stage, started)| {
            stage.effects.iter().map(move |effect| BurningEffect {
                region: stage.region,
                panel: stage.panel.clone(),
                effect: effect.effect.clone(),
                node: effect.node.clone(),
                started,
            })
        })
        .collect()
}

fn build_wing_fire_art(
    source: Option<Res<WingFireSourceResource>>,
    art: Option<Res<WingFireArt>>,
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
    commands.insert_resource(WingFireArt {
        definition: source.0.definition.clone(),
        textures,
        mesh: meshes.add(Rectangle::new(1.0, 1.0)),
    });
}

/// Names the drawn `pdpN` parts of the player airframe so effects can follow
/// them: the node name comes from the airframe visual's own part list.
fn mark_wing_fire_nodes(
    visual: Option<Res<super::MissionPlayerVisual>>,
    parts: Query<(Entity, &AircraftPart), Without<WingFireNode>>,
    mut commands: Commands,
) {
    let Some(visual) = visual else {
        return;
    };
    for (entity, part) in &parts {
        let Some(asset) = visual
            .parts()
            .iter()
            .find(|a| a.node_slot == part.node_slot)
        else {
            continue;
        };
        if asset.node_name.starts_with("pdp") {
            commands.entity(entity).insert(WingFireNode {
                name: asset.node_name.clone(),
            });
        }
    }
}

fn tick_hz(ledger: &PhysicsTickLedger) -> f32 {
    if ledger.timestep_s > 0.0 {
        1.0 / ledger.timestep_s
    } else {
        120.0
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_wing_fire(
    mut commands: Commands,
    art: Option<Res<WingFireArt>>,
    ledger: Option<Res<PhysicsTickLedger>>,
    damage: Option<Res<SessionDamageResolver>>,
    victim: Option<Res<PlayerVictim>>,
    mut state: ResMut<WingFireState>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    nodes: Query<(&WingFireNode, &GlobalTransform)>,
) {
    let (Some(art), Some(ledger), Some(damage), Some(victim)) = (art, ledger, damage, victim)
    else {
        return;
    };
    let hz = tick_hz(&ledger);
    let dt = 1.0 / hz;
    for (index, stage) in art.definition.stages.iter().enumerate() {
        let remaining = DamageNodeKey::new(stage.region.node_name())
            .ok()
            .and_then(|key| damage.resolver().remaining_integrity(&victim.actor, &key));
        let reached = remaining
            .is_some_and(|r| threshold_reached(r, DESIGNED_REGION_INTEGRITY, stage.threshold));
        if !reached {
            if state.started.remove(&index).is_some() {
                state.cursors.retain(|path, _| path[0] != index);
            }
            continue;
        }
        let started = *state.started.entry(index).or_insert(ledger.ticks);
        let elapsed = ledger.ticks.saturating_sub(started) as f32 * dt;
        for (e, effect) in stage.effects.iter().enumerate() {
            if elapsed < effect.delay_s {
                continue;
            }
            let Some(anchor) = nodes
                .iter()
                .find(|(node, _)| node.name == effect.node)
                .map(|(_, transform)| transform.translation())
            else {
                continue;
            };
            for (m, emitter) in effect.emitters.iter().enumerate() {
                let path = [index, e, m];
                let cursor = state.cursors.entry(path).or_default();
                let previous = cursor.last_anchor.replace(anchor).unwrap_or(anchor);
                if emitter
                    .off_after_s
                    .is_some_and(|off| elapsed >= effect.delay_s + off)
                {
                    continue;
                }
                let (points, carried) = plan_wing_emissions(
                    previous,
                    anchor,
                    dt,
                    cursor.carried,
                    emitter.emitter.distance_interval,
                );
                cursor.carried = carried;
                for point in points {
                    state.emitted += 1;
                    let kind = if emitter.emitter.frames.is_empty() {
                        TrailKind::Smoke
                    } else {
                        TrailKind::Fire
                    };
                    let puff = plan_puff(
                        &emitter.emitter,
                        kind,
                        point,
                        ledger.ticks,
                        state.emitted,
                        hz,
                    );
                    let texture = puff_texture(&emitter.emitter, &puff, 0.0);
                    let material = materials.add(StandardMaterial {
                        base_color: Color::WHITE,
                        base_color_texture: texture.and_then(|n| art.textures.get(n)).cloned(),
                        unlit: true,
                        alpha_mode: AlphaMode::Blend,
                        cull_mode: None,
                        ..Default::default()
                    });
                    commands.spawn((
                        WingFirePuff {
                            puff,
                            emitter: path,
                        },
                        Mesh3d(art.mesh.clone()),
                        MeshMaterial3d(material),
                        Transform::from_translation(point).with_scale(Vec3::splat(puff.size)),
                        Visibility::Visible,
                    ));
                    *state
                        .spawned_by_node
                        .entry(effect.node.clone())
                        .or_default() += 1;
                }
            }
        }
    }
}

fn puff_texture<'a>(emitter: &'a TrailEmitter, puff: &TrailPuff, age_s: f32) -> Option<&'a str> {
    match puff.kind {
        TrailKind::Smoke => emitter.textures.get(puff.cycle_index).map(String::as_str),
        TrailKind::Fire => frame_at(&emitter.frames, age_s),
    }
}

fn advance_wing_fire_puffs(
    mut commands: Commands,
    art: Option<Res<WingFireArt>>,
    ledger: Option<Res<PhysicsTickLedger>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut puffs: Query<(
        Entity,
        &WingFirePuff,
        &mut Transform,
        &MeshMaterial3d<StandardMaterial>,
    )>,
) {
    let (Some(art), Some(ledger)) = (art, ledger) else {
        return;
    };
    let hz = tick_hz(&ledger);
    for (entity, fire, mut transform, material) in &mut puffs {
        let puff = &fire.puff;
        let age = ledger.ticks.saturating_sub(puff.born);
        if age >= puff.life_ticks {
            commands.entity(entity).despawn();
            continue;
        }
        transform.translation = super::damage_trail::trail_puff_position(puff, age, hz);
        let [stage, effect, emitter] = fire.emitter;
        let Some(emitter) = art
            .definition
            .stages
            .get(stage)
            .and_then(|s| s.effects.get(effect))
            .and_then(|e| e.emitters.get(emitter))
        else {
            continue;
        };
        let [r, g, b, a] = colour_at(&emitter.emitter.colors, age as f32 / puff.life_ticks as f32);
        let name = puff_texture(&emitter.emitter, puff, age as f32 / hz);
        if let Some(mut material) = materials.get_mut(&material.0) {
            material.base_color = Color::srgba(r, g, b, a);
            if let Some(handle) = name.and_then(|n| art.textures.get(n)) {
                material.base_color_texture = Some(handle.clone());
            }
        }
    }
}

/// Turns every fire puff to face the camera, each rendered frame.
fn face_wing_fire_puffs(
    cameras: Query<&Transform, (With<Camera3d>, Without<WingFirePuff>)>,
    mut puffs: Query<&mut Transform, With<WingFirePuff>>,
) {
    let Some(camera) = cameras.iter().next() else {
        return;
    };
    for mut transform in &mut puffs {
        transform.rotation = camera.rotation;
    }
}

/// Installs the wing fire: node marking, emission and advance on the fixed
/// loop and the facing pass on the frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct MissionWingFirePlugin;

impl Plugin for MissionWingFirePlugin {
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
        app.init_resource::<WingFireState>()
            .add_systems(
                FixedPostUpdate,
                (
                    build_wing_fire_art,
                    mark_wing_fire_nodes,
                    emit_wing_fire,
                    advance_wing_fire_puffs,
                )
                    .chain(),
            )
            .add_systems(Update, face_wing_fire_puffs);
    }
}
