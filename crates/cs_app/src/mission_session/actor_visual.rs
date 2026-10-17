//! The drawn visual of the mission's scripted aircraft (M01-WINDOW-ACTORS,
//! Rally #1538).
//!
//! #1435 spawned M01's `aiv.zrd` aircraft as physics bodies with a damage
//! binding and nothing to draw, so the windowed run flew past them unseen.
//! This module gives each one the airframe model its `vehicle.zrd` record
//! points at, through the same production path the player's airframe is built
//! with ([`build_aircraft_visual`] over `zbd/planes.zbd`, one textured piece per
//! stored material group).
//!
//! **Which model** is the one designed step. The original resolves a record's
//! name prefix against `vehicle.zrd` (#1453); what that record's fields map to a
//! drawn model is unmeasured. The reading used here: the `kind_of` chain the
//! flight import already walked ([`MissionAiAirframe::inheritance_chain`],
//! measured) is searched from the record outward for the first ancestor whose
//! name is also a root of `planes.zbd`. A record whose chain names no such
//! root, or whose root draws nothing, is refused by name and stays undrawn;
//! no model is ever substituted. The label is [`ACTOR_MODEL_CLAIM`].
//!
//! Nothing here is `verified_original`.

use std::collections::BTreeMap;

use bevy::prelude::{App, Component, Entity, Quat, Resource, With, Without, World};
use cs_script::ir::ActorId;
use cs_types::content::ContentId;

use super::player_visual::PlayerAirframeSource;
use super::world_draw::WorldDrawSource;
use crate::mission_actors::{MissionZeppelinActor, ScriptedAircraftActor};
use crate::mission_ai::MissionAiAirframes;
use crate::playtest_retail::{
    AircraftPartAsset, PlaytestConfig, STORED_AIRCRAFT_NOSE_AXIS, aircraft_graph,
    build_aircraft_visual, build_world_node_visual, nose_mapping, playtest_adapter,
};
use crate::playtest_textures::TextureBinder;
use crate::world_actors::WorldActorBinding;

/// The claim the chain-to-model reading is filed under.
pub const ACTOR_MODEL_CLAIM: &str = "m01-window-actors.designed-model-from-kind-of-chain";

/// The intact subtree's authored name and the propeller node's authored name,
/// the two the airframe selection pins by slot for the player (#1216); here
/// each is found by name inside the chosen root's own subtree.
const INTACT_NODE_NAME: &str = "healthy";
const PROP_NODE_NAME: &str = "staticprop1";

/// The designed viewer distance the LOD band is chosen at, the player's own.
const LOD_DISTANCE_M: f64 = super::player_visual::MISSION_PLAYER_LOD_DISTANCE_M;

struct ModelTemplate {
    root: String,
    parts: Vec<AircraftPartAsset>,
    rotation: Quat,
}

/// Resource: the drawn model of every scripted-aircraft name prefix the stage
/// imported, and every prefix that could not be drawn with its reason.
#[derive(Resource, Default)]
pub struct MissionActorVisuals {
    by_prefix: BTreeMap<String, ModelTemplate>,
    /// The drawn parts of each zeppelin world actor's node, by the program's
    /// subject, placed relative to the node's own pose.
    zeppelins: BTreeMap<ContentId, (String, Vec<AircraftPartAsset>)>,
    /// One line per prefix that has no drawn model, verbatim reason.
    pub refusals: Vec<String>,
}

impl MissionActorVisuals {
    /// The `planes.zbd` root a prefix draws, when it draws one.
    #[must_use]
    pub fn root_of(&self, prefix: &str) -> Option<&str> {
        self.by_prefix
            .get(prefix)
            .map(|template| template.root.as_str())
    }

    /// The number of prefixes and zeppelin subjects with a drawn model.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_prefix.len() + self.zeppelins.len()
    }

    /// Whether nothing has a drawn model.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Component: this scripted aircraft's visual was attempted. `root` is the
/// drawn `planes.zbd` root, or `None` when the prefix has no drawn model.
#[derive(Component, Clone, Debug)]
pub struct ActorVisualDrawn {
    /// The model root drawn under the body, if any.
    pub root: Option<String>,
}

/// Builds the drawn model of every prefix `airframes` imported.
///
/// Refusals are recorded on the result, never raised: an actor whose model
/// cannot be drawn is still spawned (it is a body and a damage target), and
/// the census names it as undrawn.
pub fn build_actor_visuals(
    app: &mut App,
    source: &PlayerAirframeSource,
    airframes: &MissionAiAirframes,
    world: Option<&WorldDrawSource>,
    subjects: &BTreeMap<ActorId, ContentId>,
) -> MissionActorVisuals {
    let mut visuals = MissionActorVisuals::default();
    build_zeppelin_visuals(app, &mut visuals, world, subjects);
    let planes = match playtest_adapter()
        .and_then(|adapter| aircraft_graph(source.sources().aircraft(), &adapter))
    {
        Ok(planes) => planes,
        Err(error) => {
            visuals
                .refusals
                .push(format!("planes.zbd's graph would not build: {error}"));
            return visuals;
        }
    };
    let rotation = match nose_mapping(STORED_AIRCRAFT_NOSE_AXIS) {
        Ok(rotation) => rotation,
        Err(error) => {
            visuals
                .refusals
                .push(format!("the measured nose axis would not map: {error}"));
            return visuals;
        }
    };
    if !app
        .world()
        .contains_resource::<bevy::asset::Assets<bevy::prelude::StandardMaterial>>()
    {
        bevy::asset::AssetApp::init_asset::<bevy::prelude::StandardMaterial>(app);
    }
    if !app
        .world()
        .contains_resource::<bevy::asset::Assets<bevy::image::Image>>()
    {
        bevy::asset::AssetApp::init_asset::<bevy::image::Image>(app);
    }
    // Roots already built, so two prefixes that resolve to one model share it.
    let mut built: BTreeMap<String, Vec<AircraftPartAsset>> = BTreeMap::new();
    for (prefix, airframe) in &airframes.models {
        // The chain is ancestor first; the record itself is last. Search from
        // the record outward.
        let Some(root) = airframe
            .inheritance_chain
            .iter()
            .rev()
            .find_map(|name| model_root(&planes, name))
        else {
            visuals.refusals.push(format!(
                "{prefix}: no ancestor in its kind_of chain {:?} names a planes.zbd root, \
                 directly or through the original's airframe table",
                airframe.inheritance_chain
            ));
            continue;
        };
        let parts = if let Some(parts) = built.get(&root) {
            parts.clone()
        } else {
            let slots = planes.root(&root).ok().map(|node| {
                let members = planes.subtree(node.id());
                let find = |name: &str| {
                    members
                        .iter()
                        .find(|member| member.name() == name)
                        .map(|member| member.index())
                };
                (find(INTACT_NODE_NAME), find(PROP_NODE_NAME))
            });
            let Some((Some(intact), Some(prop))) = slots else {
                visuals.refusals.push(format!(
                    "{prefix}: root {root:?} holds no {INTACT_NODE_NAME:?} / {PROP_NODE_NAME:?} node"
                ));
                continue;
            };
            let config = PlaytestConfig {
                aircraft_root_name: root.clone(),
                aircraft_intact_node_slot: intact,
                aircraft_intact_node_name: INTACT_NODE_NAME.to_owned(),
                aircraft_prop_node_slot: prop,
                aircraft_prop_node_name: PROP_NODE_NAME.to_owned(),
                aircraft_lod_distance_m: LOD_DISTANCE_M,
                ..PlaytestConfig::documented()
            };
            let mut binder = TextureBinder::new(
                source.sources().textures(),
                config.textured,
                config.decal_offset,
            );
            let visual =
                match build_aircraft_visual(app, source.sources().aircraft(), &mut binder, &config)
                {
                    Ok(visual) => visual,
                    Err(error) => {
                        visuals
                            .refusals
                            .push(format!("{prefix}: root {root:?} would not draw: {error}"));
                        continue;
                    }
                };
            let _textures = binder.finish();
            built.insert(root.clone(), visual.parts.clone());
            visual.parts
        };
        visuals.by_prefix.insert(
            prefix.clone(),
            ModelTemplate {
                root,
                parts,
                rotation,
            },
        );
    }
    visuals
}

/// Draws each zeppelin world actor's own world-container node: the actor's
/// body is a stored node (#772), and no other path draws its subtree.
fn build_zeppelin_visuals(
    app: &mut App,
    visuals: &mut MissionActorVisuals,
    world: Option<&WorldDrawSource>,
    subjects: &BTreeMap<ActorId, ContentId>,
) {
    let Some(world) = world else { return };
    for subject in subjects.values() {
        // The subject spells `container.<..>.world1.<node name>`: the node's
        // authored name is the last segment.
        let name = subject.key().rsplit('.').next().unwrap_or_default();
        match build_world_node_visual(
            app,
            world.sources(),
            name,
            LOD_DISTANCE_M,
            &PlaytestConfig::documented(),
        ) {
            Ok(parts) => {
                visuals
                    .zeppelins
                    .insert(subject.clone(), (name.to_owned(), parts));
            }
            Err(error) => visuals
                .refusals
                .push(format!("zeppelin {name}: its node would not draw: {error}")),
        }
    }
}

/// The `planes.zbd` root a `kind_of` chain element names: the element itself
/// when it is a root, else the `model` of the original's airframe-table row
/// (`0x620c70`, #715) whose display name it is — row 5 pairs `Devastator` with
/// `piratefighter`, which is how a `devastator` record reaches its model.
fn model_root(planes: &cs_content::scene::SceneGraph, name: &str) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    if planes.root(&lower).is_ok() {
        return Some(lower);
    }
    let model = crate::mission_start::AIRFRAME_TABLE
        .get(crate::mission_start::airframe_index(&lower)?)?
        .model;
    planes.root(model).is_ok().then(|| model.to_owned())
}

/// Update system: parents the drawn model under every scripted aircraft that
/// has not had its visual attempted, including the fresh generation after a
/// restart. Exclusive because part spawning takes the world.
pub fn attach_actor_visuals(world: &mut World) {
    let pending: Vec<(Entity, String)> = {
        let mut query = world.query_filtered::<
            (Entity, &ScriptedAircraftActor),
            bevy::prelude::Without<ActorVisualDrawn>,
        >();
        query
            .iter(world)
            .map(|(entity, actor)| (entity, actor.vehicle_record.clone()))
            .collect()
    };
    let Some(visuals) = world.remove_resource::<MissionActorVisuals>() else {
        return;
    };
    attach_zeppelin_visuals(world, &visuals);
    for (entity, prefix) in pending {
        let root = visuals.by_prefix.get(&prefix).map(|template| {
            for part in &template.parts {
                let base = part.oriented(template.rotation);
                part.spawn(world, entity, base);
            }
            template.root.clone()
        });
        world.entity_mut(entity).insert(ActorVisualDrawn { root });
    }
    world.insert_resource(visuals);
}

fn attach_zeppelin_visuals(world: &mut World, visuals: &MissionActorVisuals) {
    let pending: Vec<(Entity, ContentId)> = {
        let mut query = world.query_filtered::<
            (Entity, &WorldActorBinding),
            (With<MissionZeppelinActor>, Without<ActorVisualDrawn>),
        >();
        query
            .iter(world)
            .map(|(entity, binding)| (entity, binding.subject.clone()))
            .collect()
    };
    for (entity, subject) in pending {
        let root = visuals.zeppelins.get(&subject).map(|(name, parts)| {
            for part in parts {
                part.spawn(world, entity, part.local);
            }
            name.clone()
        });
        world.entity_mut(entity).insert(ActorVisualDrawn { root });
    }
}
