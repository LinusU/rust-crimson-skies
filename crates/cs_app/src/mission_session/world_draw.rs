//! Drawing the mission's own world around the player
//! (M01-PLAY-WORLD-VISIBLE, Rally #1433).
//!
//! # The defect this module repairs
//!
//! [`crate::world::residency::load_world`] spawns every world object with the
//! shared whole-mesh [`Mesh3d`] handle the mesh-derived collider is built
//! from, and **no material**: the geometry exists to collide with, and the
//! engine draws none of it. The retail playtest scene
//! ([`crate::playtest_retail`]) draws its area by additionally spawning, under
//! each object's node, one child per stored material group carrying the group's
//! own [`Mesh3d`] + [`MeshMaterial3d`] (`crate::playtest_textures::TextureBinder`
//! over the group's own texture archive). The windowed mission composition
//! (`super::compose`) never did that, so `cs --mission M01` drew only the
//! player airframe against an empty background: no terrain, no sea, no
//! horizon.
//!
//! This module is that missing half, and it is deliberately the **same**
//! production path the playtest draws with — one [`TextureBinder`] over the
//! group's own [`PlaytestTextureArchive`], one part per stored material group —
//! never a second material-binding implementation:
//!
//! * [`WorldDrawSource`] carries the world group's stored material table and
//!   texture archive ([`crate::playtest_retail::WorldSources`], read through
//!   the playtest's own `read_container`/`PlaytestTextureArchive::open`) plus
//!   the stored mesh-array slot of every definition mesh reference
//!   ([`crate::mission_session::MissionContent::world_mesh_slots`]).
//! * [`build_world_draw`] binds every loaded mesh's stored material groups into
//!   [`PlaytestPart`]s, keyed by the definition's own mesh reference.
//! * [`attach_world_parts`] parents those parts under the object's node as it
//!   is spawned, so a load, a sector load and the composed `R` restart all draw
//!   the world without a second call site.
//!
//! # What this does *not* claim
//!
//! The material binding is the #666 **designed** reading
//! ([`crate::playtest_textures::NAME_READING`]) and its presentation is the
//! declared provisional one ([`crate::playtest_textures::PLAYTEST_TEXTURE_PRESENTATION_IS_PROVISIONAL`]):
//! it is evidence that the world's own textures resolve and draw on the loaded
//! geometry, never that the original renderer looked like this. Which stored
//! mesh node the original *draws* (LOD selection, variant choice) is the
//! playtest's own unmeasured question; this stage draws the definition's own
//! records, every one, so a record the mission loads is a record it shows.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use bevy::asset::{AssetApp, Assets, Handle};
use bevy::image::Image;
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::{
    Added, App, Camera3d, ChildOf, Color, Commands, Component, Entity, GlobalTransform, Local,
    Mesh, Mesh3d, MeshMaterial3d, Plugin, Query, Res, Resource, StandardMaterial, Transform,
    Update, Vec3, Visibility, With, Without,
};
use cs_content::world::{WorldCollisionRole, WorldDefinition};
use cs_formats::gamez::GameZMaterials;
use cs_types::content::{ContentId, Resolved};

use crate::playtest::scene::PlaytestSea;
use crate::playtest_retail::{
    CONTACT_SWEEP_NODE_FLAGS, PlaytestConfig, WorldScenery, WorldSources, build_world_scenery,
    read_world_sources,
};
use crate::playtest_textures::{NeutralColor, PlaytestPart, TextureBinder};
use crate::render::horizon::HorizonSpec;
use crate::render::material_cycle::MaterialCycles;
use crate::render::matrix::HORIZON_ANCHOR;
use crate::world::WorldMeshes;
use crate::world::contacts::{WorldColliderInstance, WorldObjectBinding, WorldVisual};
use crate::world::spawn::{canonical_matrix, static_collider_components};

use super::compose::{MissionCompositionError, MissionStage};

/// The neutral colour a stored material that names no texture (or one this
/// group's archive does not resolve) is drawn with: the playtest's own
/// designed world colour (`crate::playtest_retail`'s `WORLD_MATERIAL_COLOR`,
/// `[0.74, 0.70, 0.62, 1.0]`), repeated here because that constant is private
/// to its module. A neutral part is a **designed** stand-in, never a claim
/// about the original surface.
pub const WORLD_NEUTRAL_COLOR: [f32; 4] = [0.74, 0.70, 0.62, 1.0];

/// What the composition reads to draw the mission's world: the group's stored
/// material table and texture archive, plus the stored mesh-array slot of every
/// definition mesh reference.
///
/// `None` on the synthetic acceptance stages, which have no installation to
/// read one from — they spawn the world's geometry without drawing it, exactly
/// as they did before this stage.
#[derive(Clone, Debug)]
pub struct WorldDrawSource {
    sources: WorldSources,
    slots: BTreeMap<ContentId, u32>,
}

impl WorldDrawSource {
    /// Reads the world group's sources out of `install_root` through the
    /// playtest's own production readers, joined to `slots` (the definition's
    /// mesh reference to stored mesh-array slot map).
    ///
    /// # Errors
    ///
    /// [`MissionCompositionError::WorldVisual`] when discovery, the container
    /// read or the texture archive refuses.
    pub fn read(
        install_root: &Path,
        world_group: &str,
        slots: BTreeMap<ContentId, u32>,
    ) -> Result<Self, MissionCompositionError> {
        let sources = read_world_sources(install_root, world_group).map_err(|error| {
            MissionCompositionError::WorldVisual(format!(
                "the mission world's draw sources refuse: {error}"
            ))
        })?;
        Ok(Self { sources, slots })
    }

    /// The world group's stored material table and texture archive.
    #[must_use]
    pub const fn sources(&self) -> &WorldSources {
        &self.sources
    }

    /// The stored mesh-array slot of each definition mesh reference.
    #[must_use]
    pub const fn slots(&self) -> &BTreeMap<ContentId, u32> {
        &self.slots
    }
}

/// The drawn parts of every world mesh the definition names, keyed by the
/// mesh reference the object records carry, plus what the binding measured.
///
/// Held as a resource for as long as the composition lives; a stage that
/// carries no [`WorldDrawSource`] inserts the empty value, so the attach system
/// is always installed and a composition never silently lacks the drawing path.
#[derive(Resource, Clone, Debug, Default)]
pub struct MissionWorldParts {
    parts: BTreeMap<ContentId, Arc<Vec<PlaytestPart>>>,
    report: WorldDrawReport,
    water: Option<WorldWaterLevel>,
    water_art: Option<WorldWaterArt>,
    horizon: Option<HorizonSubtree>,
    scenery: Option<Arc<WorldScenery>>,
}

/// The label the world's `horizon` subtree carries (PARITY-M01-ATMO-D, Rally
/// #1651). Which records hang under `horizon`, their art and their geometry are
/// the archive's own; that the original re-anchors the node to the camera each
/// render pass is measured (`docs/findings/2026-10-11-t541-horizon-commands-
/// anchor-scenery-to-camera.md`). Drawing the subtree unlit and unfogged is this
/// build's own choice: how the original lights and fogs the sky records is
/// unmeasured.
pub const HORIZON_SUBTREE_LABEL: &str = "PARITY-M01-ATMO-D horizon subtree drawn from the archive, camera anchor measured, lighting and fog of the sky records unmeasured, 2026-10-19";

/// The stored node slots of the world's `horizon` record and its descendants.
///
/// `root` is the `horizon` record itself; `nodes` holds every slot below it
/// under the stored parent slots, `root` excluded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HorizonSubtree {
    root: u32,
    nodes: BTreeSet<u32>,
}

impl HorizonSubtree {
    /// Reads the subtree out of a container's node records: the record named
    /// `horizon` and everything reached from it through the parent slots.
    /// `None` when the container stores no such record.
    #[must_use]
    pub fn read(nodes: &cs_formats::gamez::GameZNodes) -> Option<Self> {
        let root = nodes
            .nodes
            .iter()
            .find(|node| node.name.eq_ignore_ascii_case(HORIZON_ANCHOR))?
            .index;
        let mut children: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        for node in &nodes.nodes {
            if let Some(parent) = node.parent {
                children.entry(parent).or_default().push(node.index);
            }
        }
        let mut reached = BTreeSet::new();
        let mut stack = children.get(&root).cloned().unwrap_or_default();
        while let Some(slot) = stack.pop() {
            if reached.insert(slot)
                && let Some(below) = children.get(&slot)
            {
                stack.extend(below.iter().copied());
            }
        }
        Some(Self {
            root,
            nodes: reached,
        })
    }

    /// The stored slot of the `horizon` record.
    #[must_use]
    pub const fn root(&self) -> u32 {
        self.root
    }

    /// Whether `slot` is the `horizon` record or one of its descendants.
    #[must_use]
    pub fn contains(&self, slot: u32) -> bool {
        slot == self.root || self.nodes.contains(&slot)
    }
}

/// The stored node slot an imported object id (`node-<slot>`) names.
pub(super) fn node_slot_of(id: &str) -> Option<u32> {
    id.strip_prefix("node-")?.parse().ok()
}

/// The art the world's own water tiles are drawn with, measured from the parts
/// the world draw really binds (PARITY-M01-ATMO-C, Rally #1650), so the horizon
/// sea can continue the same surface instead of a flat designed colour.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldWaterArt {
    /// The material of a water tile: the original `wtr0xxxx` image bound by
    /// the world draw, with every setting the tile is drawn with.
    pub material: Handle<StandardMaterial>,
    /// The bound image's key (names the original texture).
    pub image: String,
    /// The tile's texture mapping, measured from its largest triangle:
    /// `[u, v] = uv_from_xz * [x, z]`, in repeats per metre.
    pub uv_from_xz: [[f32; 2]; 2],
    /// The mean stored corner colour over every vertex of every water part
    /// (the tiles are drawn `texture × corner colour`), `None` when no water
    /// part carries corner colours.
    pub corner_shade: Option<[f32; 3]>,
}

/// The altitude of the world's own water, measured from its drawn geometry
/// (M01-WINDOW-FARDRAW, Rally #1554).
///
/// M01's water is 144 flat quads bound to the `water1` texture; this is their
/// world-space altitude, read off the vertices the world draw really binds
/// (object placement applied), so the drawn horizon sea can sit at it instead
/// of at a designed number.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldWaterLevel {
    /// The altitude every water vertex shares, in metres.
    pub level_m: f32,
    /// Water vertices measured.
    pub vertices: usize,
}

/// The texture name the world's water quads are bound to, compared without
/// case and ignoring the extension. `ZBD/C1C/gamez.zbd`'s texture table names
/// it `water1` and M01's water material 155 spells `water1.tif`.
pub const WATER_TEXTURE_NAME: &str = "water1";

/// The texture name `ZBD/C3/rtexture12.zbd` gives M01's water: 255 drawn parts
/// of the `ZBD/C3` world bind `wtr00000` and nothing else in the table reads as
/// water (measured by the M01-WINDOW-WORLD census, `docs/findings/
/// 2026-10-18-m01-window-world.md`). Matched like [`WATER_TEXTURE_NAME`].
pub const C3_WATER_TEXTURE_NAME: &str = "wtr00000";

/// How far apart water vertices may be in altitude and still count as one flat
/// surface, in metres.
const WATER_FLATNESS_M: f32 = 0.01;

fn is_water_image(key: &str) -> bool {
    let name = key.rsplit(':').next().unwrap_or(key).to_ascii_lowercase();
    [WATER_TEXTURE_NAME, C3_WATER_TEXTURE_NAME]
        .iter()
        .filter_map(|water| name.strip_prefix(water))
        .any(|rest| rest.is_empty() || rest.starts_with('.'))
}

impl MissionWorldParts {
    /// Builds the resource from already-bound parts, one entry per mesh.
    #[must_use]
    pub fn from_parts(parts: BTreeMap<ContentId, Vec<PlaytestPart>>) -> Self {
        let report = WorldDrawReport {
            meshes: parts.len(),
            parts: parts.values().map(Vec::len).sum(),
            textured_parts: parts
                .values()
                .flatten()
                .filter(|part| part.image.is_some())
                .count(),
        };
        Self {
            parts: parts
                .into_iter()
                .map(|(id, parts)| (id, Arc::new(parts)))
                .collect(),
            report,
            water: None,
            water_art: None,
            horizon: None,
            scenery: None,
        }
    }

    /// The authored scenery below the world record's direct children, when
    /// the world draw built any (#1735).
    #[must_use]
    pub fn scenery(&self) -> Option<&WorldScenery> {
        self.scenery.as_deref()
    }

    /// This resource with the scenery recorded.
    #[must_use]
    pub fn with_scenery(mut self, scenery: Option<WorldScenery>) -> Self {
        self.scenery = scenery.map(Arc::new);
        self
    }

    /// The world's `horizon` record and its descendants, when the container
    /// stores one.
    #[must_use]
    pub const fn horizon(&self) -> Option<&HorizonSubtree> {
        self.horizon.as_ref()
    }

    /// This resource with the `horizon` subtree recorded.
    #[must_use]
    pub fn with_horizon(mut self, horizon: Option<HorizonSubtree>) -> Self {
        self.horizon = horizon;
        self
    }

    /// The world's own water altitude, when its water geometry is one flat
    /// surface. `None` leaves the drawn sea at its designed level.
    #[must_use]
    pub const fn water_level(&self) -> Option<WorldWaterLevel> {
        self.water
    }

    /// This resource with the measured water altitude recorded.
    #[must_use]
    pub const fn with_water_level(mut self, water: Option<WorldWaterLevel>) -> Self {
        self.water = water;
        self
    }

    /// The art of the world's water tiles, when the world draw bound any.
    #[must_use]
    pub const fn water_art(&self) -> Option<&WorldWaterArt> {
        self.water_art.as_ref()
    }

    /// This resource with the measured water art recorded.
    #[must_use]
    pub fn with_water_art(mut self, art: Option<WorldWaterArt>) -> Self {
        self.water_art = art;
        self
    }

    /// The drawn parts of the mesh `id`, when this resource bound any.
    #[must_use]
    pub fn for_mesh(&self, id: &ContentId) -> Option<&[PlaytestPart]> {
        self.parts.get(id).map(|parts| parts.as_slice())
    }

    /// How many meshes this resource bound parts for.
    #[must_use]
    pub const fn mesh_count(&self) -> usize {
        self.report.meshes
    }

    /// How many drawn parts the whole world has.
    #[must_use]
    pub const fn part_count(&self) -> usize {
        self.report.parts
    }

    /// What the binding measured.
    #[must_use]
    pub const fn report(&self) -> &WorldDrawReport {
        &self.report
    }

    /// Whether no mesh in the definition resolved to a drawn part.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }
}

/// What one world-draw build measured: the meshes it bound and the parts it
/// produced. A count, never a claim about the original renderer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WorldDrawReport {
    /// Meshes the definition names that resolved to drawn parts.
    pub meshes: usize,
    /// Drawn parts across every mesh.
    pub parts: usize,
    /// Of [`Self::parts`], the ones bound to a resolved original texture.
    pub textured_parts: usize,
}

/// Binds every world mesh the definition names to its stored material groups,
/// through the playtest's own [`TextureBinder`].
///
/// The definition's objects are walked in order and each mesh reference is
/// bound once; a mesh the definition names but this source holds no upload for
/// is skipped rather than given a substitute material (the spawn already
/// reports that object as a `MeshUnavailable` gap). The headless composition has
/// no PBR plugin, so the two asset collections the binding needs are initialised
/// here when absent — the same guard `super::player_visual::build_player_visual`
/// runs.
#[must_use]
pub fn build_world_draw(
    app: &mut App,
    source: &WorldDrawSource,
    definition: &WorldDefinition,
    meshes: &WorldMeshes,
) -> MissionWorldParts {
    if !app.world().contains_resource::<Assets<StandardMaterial>>() {
        app.init_asset::<StandardMaterial>();
    }
    if !app.world().contains_resource::<Assets<Image>>() {
        app.init_asset::<Image>();
    }
    let mut binder = TextureBinder::new(source.sources.textures(), true, true);
    let mut parts: BTreeMap<ContentId, Vec<PlaytestPart>> = BTreeMap::new();
    let horizon = HorizonSubtree::read(source.sources.world().nodes());
    for object in definition.objects() {
        let Resolved::Known(known) = object.mesh() else {
            continue;
        };
        if parts.contains_key(&known.value) {
            continue;
        }
        let Some(world_mesh) = meshes.get(&known.value) else {
            continue;
        };
        let stored_slot = source.slots.get(&known.value).copied().unwrap_or(0);
        let mut bound = binder.parts(
            app,
            source.sources.world(),
            world_mesh,
            NeutralColor(WORLD_NEUTRAL_COLOR),
            stored_slot,
        );
        stored_shading_materials(app, &mut bound);
        if node_slot_of(object.id().as_str()).is_some_and(|slot| {
            horizon
                .as_ref()
                .is_some_and(|subtree| subtree.contains(slot))
        }) {
            sky_materials(app, source.sources.world().materials(), &mut bound);
        }
        parts.insert(known.value.clone(), bound);
    }
    let water = measure_water_level(app, definition, &parts);
    let art = measure_water_art(app, definition, &parts);
    MissionWorldParts::from_parts(parts)
        .with_water_level(water)
        .with_water_art(art)
        .with_horizon(horizon)
}

/// The viewer distance the scenery's LOD bands are chosen at: the player's own
/// designed one, the same the world actors' nodes are drawn at.
const SCENERY_LOD_DISTANCE_M: f64 = super::player_visual::MISSION_PLAYER_LOD_DISTANCE_M;

/// The world record's multiplayer-named children (zeppelins, capture-the-flag
/// markers and flags): content whose role in a single-player mission nothing
/// measured states, so it is left undrawn and recorded
/// (`docs/findings/2026-10-21-m01-missing-terrain.md`).
const MULTIPLAYER_ROOT_PREFIXES: [&str; 3] = ["multiplayer", "ctf_", "cs_flag"];

/// Builds the authored scenery the definition's flat objects cannot hold
/// (#1735): the descendants of the world record's direct children, one intact
/// variant of every part at its composed transform.
///
/// `actor_nodes` are the authored names of the nodes world actors draw
/// themselves; those subtrees are left to the actors. The `horizon` child is
/// the definition's own and the `dzpath` records are destroyed at load
/// (`GameZNodes::removed_at_load`).
///
/// # Errors
///
/// [`MissionCompositionError::WorldVisual`] when a walked subtree will not
/// convert.
pub fn build_scenery(
    app: &mut App,
    source: &WorldDrawSource,
    actor_nodes: &BTreeSet<String>,
) -> Result<WorldScenery, MissionCompositionError> {
    if !app.world().contains_resource::<Assets<StandardMaterial>>() {
        app.init_asset::<StandardMaterial>();
    }
    if !app.world().contains_resource::<Assets<Image>>() {
        app.init_asset::<Image>();
    }
    let skip = |node: &cs_formats::gamez::nodes::RawNode| -> Option<String> {
        if node.name == "horizon" {
            Some("the world definition imports the horizon subtree itself".to_owned())
        } else if node
            .name
            .starts_with(cs_formats::gamez::nodes::LOAD_REMOVED_NODE_PREFIX)
        {
            Some("destroyed at world load".to_owned())
        } else if MULTIPLAYER_ROOT_PREFIXES
            .iter()
            .any(|prefix| node.name.starts_with(prefix))
        {
            Some(
                "multiplayer-mode content: its role in the single-player mission is unknown \
                 (rule 4), so it is not drawn"
                    .to_owned(),
            )
        } else if actor_nodes.contains(&node.name) {
            Some("a world actor draws this node".to_owned())
        } else {
            None
        }
    };
    let mut scenery = build_world_scenery(
        app,
        source.sources(),
        &skip,
        SCENERY_LOD_DISTANCE_M,
        &PlaytestConfig::documented(),
    )
    .map_err(|error| {
        MissionCompositionError::WorldVisual(format!("the world's scenery would not draw: {error}"))
    })?;
    for parts in scenery.meshes.values_mut() {
        stored_shading_materials(app, parts);
    }
    Ok(scenery)
}

/// Gives each part whose stored geometry carries per-corner colours its own
/// unlit copy of its material ([`STORED_SHADING_LABEL`]): the stored vertex
/// colours are the world's authored shading channel, so a vertex-coloured
/// part draws `texture × colour` without a second, dynamic lighting term.
/// A copy keeps every other user of the shared material as it was, and a
/// part with no stored colours keeps the lit material the binder gave it.
fn stored_shading_materials(app: &mut App, parts: &mut [PlaytestPart]) {
    for part in parts {
        let coloured = app
            .world()
            .get_resource::<Assets<Mesh>>()
            .and_then(|meshes| meshes.get(&part.mesh))
            .is_some_and(|mesh| mesh.attribute(Mesh::ATTRIBUTE_COLOR).is_some());
        if !coloured {
            continue;
        }
        let mut materials = app.world_mut().resource_mut::<Assets<StandardMaterial>>();
        let Some(mut shaded) = materials.get(&part.material).cloned() else {
            continue;
        };
        if shaded.unlit {
            continue;
        }
        shaded.unlit = true;
        let copy = materials.add(shaded);
        follow_material_cycle(app, &part.material, &copy);
        part.material = copy;
    }
}

/// Makes `copy` play the texture cycle of the material it was copied from, when
/// that material is cycled (the `wtr0xxxx` water, task #1981): a copy made for
/// shading would otherwise stay on the first frame.
fn follow_material_cycle(
    app: &mut App,
    original: &Handle<StandardMaterial>,
    copy: &Handle<StandardMaterial>,
) {
    if let Some(mut cycles) = app.world_mut().get_resource_mut::<MaterialCycles>() {
        cycles.follow(original, copy.clone());
    }
}

/// Gives each part of a `horizon` record its own unlit, unfogged copy of its
/// material ([`HORIZON_SUBTREE_LABEL`]): the stored vertex colours carry the
/// sky's shading, a distance fog would wash the 4-9 km sky records flat, and a
/// copy keeps every other user of the shared material as it was. A part whose
/// stored material names no texture is drawn in its vertex colours alone. A part
/// whose stored material names a texture the group's archives do not hold is
/// **not drawn**: M01's `cloud1.tif` and `cloud2.tif` (the two 22 m quads of
/// `g1155`) are in no `ZBD/C3` texture archive, and a white stand-in square
/// would be invented art.
fn sky_materials(app: &mut App, stored: &GameZMaterials, parts: &mut Vec<PlaytestPart>) {
    parts.retain(|part| {
        part.image.is_some()
            || !stored
                .material(part.stored_material)
                .is_some_and(|material| stored.names_a_texture(material))
    });
    for part in parts.iter_mut() {
        display_value_corner_colours(app, part);
    }
    for part in parts {
        let mut materials = app.world_mut().resource_mut::<Assets<StandardMaterial>>();
        let Some(mut sky) = materials.get(&part.material).cloned() else {
            continue;
        };
        sky.unlit = true;
        sky.fog_enabled = false;
        if part.image.is_none() {
            sky.base_color = Color::WHITE;
        }
        let copy = materials.add(sky);
        follow_material_cycle(app, &part.material, &copy);
        part.material = copy;
    }
}

/// Re-reads a sky part's stored corner colours as display values.
///
/// The stored per-corner colours ride the byte scale and, drawn as linear
/// light (what a vertex-colour attribute is to the renderer), give M01's sky
/// the pale, desaturated cast of the frames before this change. The original's
/// launch frame instead shows the zenith cap's stored `(176, 209, 242)` as
/// itself: Drydock's top row averages `(179, 210, 245)`
/// (`docs/findings/2026-10-20-parity2-m01-sky-colour.md`). The fixed-function
/// era multiplied texture and corner colour in display space, so the sky
/// parts convert each stored channel from sRGB to linear and the renderer's
/// own output encoding gives the stored byte back. Only `horizon` parts take
/// this: the other records' colour space stays as `normalize_corner_colours`
/// left it.
fn display_value_corner_colours(app: &mut App, part: &mut PlaytestPart) {
    let mut meshes = app.world_mut().resource_mut::<Assets<Mesh>>();
    let Some(mut mesh) = meshes.get(&part.mesh).cloned() else {
        return;
    };
    let Some(VertexAttributeValues::Float32x4(values)) = mesh.attribute(Mesh::ATTRIBUTE_COLOR)
    else {
        return;
    };
    let linear: Vec<[f32; 4]> = values
        .iter()
        .map(|&[r, g, b, a]| {
            let to_linear =
                |c: f32| bevy::color::LinearRgba::from(bevy::color::Srgba::new(c, c, c, 1.0)).red;
            [to_linear(r), to_linear(g), to_linear(b), a]
        })
        .collect();
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, linear);
    part.mesh = meshes.add(mesh);
}

/// A `horizon` record's offset from the `horizon` record, kept so the subtree
/// can follow the camera as one rigid piece.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct HorizonRest(pub Vec3);

/// Drags the `horizon` subtree to the camera's world position every frame, all
/// three axes and no rotation, as the original's `CameraSetHorizon horizon`
/// does (`docs/findings/2026-10-11-t541-horizon-commands-anchor-scenery-to-
/// camera.md`). Every record keeps its stored offset from the `horizon`
/// record.
fn anchor_horizon_to_camera(
    mut commands: Commands,
    mut home: Local<Option<Vec3>>,
    parts: Res<MissionWorldParts>,
    cameras: Query<&GlobalTransform, (With<Camera3d>, Without<crate::render::OverlayCamera>)>,
    mut records: Query<(
        Entity,
        &WorldObjectBinding,
        &mut Transform,
        Option<&HorizonRest>,
    )>,
) {
    let (Some(horizon), Ok(camera)) = (parts.horizon(), cameras.single()) else {
        return;
    };
    let camera = camera.translation();
    if home.is_none() {
        // The `horizon` record's own stored translation, read before the
        // first move: every other record's rest is its offset from it.
        *home = records
            .iter()
            .find(|(_, binding, _, _)| {
                node_slot_of(binding.object().as_str()) == Some(horizon.root())
            })
            .map(|(_, _, transform, _)| transform.translation);
    }
    let Some(root) = *home else {
        return;
    };
    for (entity, binding, mut transform, rest) in &mut records {
        if !node_slot_of(binding.object().as_str()).is_some_and(|slot| horizon.contains(slot)) {
            continue;
        }
        match rest {
            Some(rest) => transform.translation = camera + rest.0,
            None => {
                let rest = transform.translation - root;
                commands.entity(entity).insert(HorizonRest(rest));
                transform.translation = camera + rest;
            }
        }
    }
}

/// Puts the drawn horizon sea at the world's own water altitude, when `parts`
/// measured one and the app carries a [`HorizonSpec`] (the windowed face only).
/// Returns whether the sea was moved.
///
/// The spec is read when the horizon spawns at `Startup`, so this runs during
/// composition, before the first update.
pub fn bind_sea_to_world_water(app: &mut App, parts: &MissionWorldParts) -> bool {
    let Some(water) = parts.water_level() else {
        return false;
    };
    let Some(mut spec) = app.world_mut().get_resource_mut::<HorizonSpec>() else {
        return false;
    };
    spec.bind_world_water_level(water.level_m);
    if let Some(art) = parts.water_art() {
        spec.bind_world_water_art(art.material.clone(), art.uv_from_xz, &art.image);
        if let Some(shade) = art.corner_shade {
            spec.bind_world_water_shade(shade);
        }
    }
    true
}

/// Measures the art of the world's water: the material of the water part with
/// the most triangles, and its texture mapping from that part's largest
/// non-degenerate triangle in world space (object placement applied). No water
/// part, or none with a measurable mapping, gives `None`.
fn measure_water_art(
    app: &App,
    definition: &WorldDefinition,
    parts: &BTreeMap<ContentId, Vec<PlaytestPart>>,
) -> Option<WorldWaterArt> {
    let assets = app.world().get_resource::<Assets<Mesh>>()?;
    let mut best: Option<(usize, WorldWaterArt)> = None;
    let mut shade_sum = [0.0_f64; 3];
    let mut shade_count = 0_u64;
    for object in definition.objects() {
        let Resolved::Known(known) = object.mesh() else {
            continue;
        };
        let Some(bound) = parts.get(&known.value) else {
            continue;
        };
        let placement = canonical_matrix(object.transform());
        for part in bound {
            let Some(image) = part.image.as_deref().filter(|key| is_water_image(key)) else {
                continue;
            };
            let Some(mesh) = assets.get(&part.mesh) else {
                continue;
            };
            if let Some(VertexAttributeValues::Float32x4(colours)) =
                mesh.attribute(Mesh::ATTRIBUTE_COLOR)
            {
                for colour in colours {
                    for (sum, channel) in shade_sum.iter_mut().zip(colour) {
                        *sum += f64::from(*channel);
                    }
                    shade_count += 1;
                }
            }
            if best.as_ref().is_some_and(|(n, _)| *n >= part.triangles) {
                continue;
            }
            let Some(uv_from_xz) = texture_mapping(mesh, &placement) else {
                continue;
            };
            best = Some((
                part.triangles,
                WorldWaterArt {
                    material: part.material.clone(),
                    image: image.to_owned(),
                    uv_from_xz,
                    corner_shade: None,
                },
            ));
        }
    }
    let shade = (shade_count > 0).then(|| shade_sum.map(|sum| (sum / shade_count as f64) as f32));
    best.map(|(_, art)| WorldWaterArt {
        corner_shade: shade,
        ..art
    })
}

/// The linear map from world `[x, z]` to `[u, v]` of the largest triangle of
/// `mesh` under `placement`, or `None` when the mesh has no positions, no UVs
/// or only degenerate triangles.
fn texture_mapping(mesh: &Mesh, placement: &bevy::prelude::Mat4) -> Option<[[f32; 2]; 2]> {
    let Some(VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        return None;
    };
    let Some(VertexAttributeValues::Float32x2(uvs)) = mesh.attribute(Mesh::ATTRIBUTE_UV_0) else {
        return None;
    };
    let indices: Vec<usize> = match mesh.indices()? {
        bevy::mesh::Indices::U16(i) => i.iter().map(|v| usize::from(*v)).collect(),
        bevy::mesh::Indices::U32(i) => i.iter().map(|v| *v as usize).collect(),
    };
    let xz = |i: usize| {
        let p = placement.transform_point3(Vec3::from(*positions.get(i)?));
        Some([p.x, p.z])
    };
    let mut best: Option<(f32, [[f32; 2]; 2])> = None;
    for tri in indices.as_chunks::<3>().0 {
        let (Some(a), Some(b), Some(c)) = (xz(tri[0]), xz(tri[1]), xz(tri[2])) else {
            continue;
        };
        let (Some(ua), Some(ub), Some(uc)) = (uvs.get(tri[0]), uvs.get(tri[1]), uvs.get(tri[2]))
        else {
            continue;
        };
        let (e1, e2) = ([b[0] - a[0], b[1] - a[1]], [c[0] - a[0], c[1] - a[1]]);
        let det = e1[0] * e2[1] - e1[1] * e2[0];
        if det.abs() < 1.0 || best.as_ref().is_some_and(|(d, _)| *d >= det.abs()) {
            continue;
        }
        // Solve J * e = du for both edges: J = dU * E^-1.
        let (d1, d2) = (
            [ub[0] - ua[0], ub[1] - ua[1]],
            [uc[0] - ua[0], uc[1] - ua[1]],
        );
        let inv = [[e2[1] / det, -e2[0] / det], [-e1[1] / det, e1[0] / det]];
        let row = |k: usize| {
            [
                d1[k] * inv[0][0] + d2[k] * inv[1][0],
                d1[k] * inv[0][1] + d2[k] * inv[1][1],
            ]
        };
        best = Some((det.abs(), [row(0), row(1)]));
    }
    best.map(|(_, j)| j)
}

/// Measures the altitude of every vertex of every part bound to the water
/// texture, at its object's placement. Flat within [`WATER_FLATNESS_M`] gives
/// the level; no water, or water that is not one flat surface, gives `None`
/// rather than a guessed number.
fn measure_water_level(
    app: &App,
    definition: &WorldDefinition,
    parts: &BTreeMap<ContentId, Vec<PlaytestPart>>,
) -> Option<WorldWaterLevel> {
    let assets = app.world().get_resource::<Assets<Mesh>>()?;
    let mut altitudes: Vec<f32> = Vec::new();
    for object in definition.objects() {
        let Resolved::Known(known) = object.mesh() else {
            continue;
        };
        let Some(bound) = parts.get(&known.value) else {
            continue;
        };
        let placement = canonical_matrix(object.transform());
        for part in bound
            .iter()
            .filter(|part| part.image.as_deref().is_some_and(is_water_image))
        {
            let Some(VertexAttributeValues::Float32x3(positions)) = assets
                .get(&part.mesh)
                .and_then(|mesh| mesh.attribute(Mesh::ATTRIBUTE_POSITION))
            else {
                continue;
            };
            altitudes.extend(
                positions
                    .iter()
                    .map(|position| placement.transform_point3(Vec3::from(*position)).y),
            );
        }
    }
    water_level_of(&altitudes)
}

/// The sea altitude of a set of water-vertex altitudes: the lowest one, when
/// more than half of the vertices lie within [`WATER_FLATNESS_M`] of it. `ZBD/C1C`'s
/// water is all one flat surface; `ZBD/C3`'s `wtr00000` also binds raised
/// pools and falls (up to ~13 m above the surface in M01), so a rule that
/// demanded every vertex flat would find no sea there. A set with no majority
/// at one altitude gives `None` rather than a guessed number.
#[must_use]
pub fn water_level_of(altitudes: &[f32]) -> Option<WorldWaterLevel> {
    let low = altitudes.iter().copied().reduce(f32::min)?;
    let at_low = altitudes
        .iter()
        .filter(|y| **y - low <= WATER_FLATNESS_M)
        .count();
    (at_low * 2 > altitudes.len()).then_some(WorldWaterLevel {
        level_m: low,
        vertices: altitudes.len(),
    })
}

/// Parents the drawn parts of every world object as it is spawned: one child per
/// stored material group carrying the group's own [`Mesh3d`] +
/// [`MeshMaterial3d`], at the object's own placement (the child's local
/// transform is the identity and it inherits the object's global transform), so
/// a mission overlay that moves the object moves what it draws.
///
/// The whole-mesh entity keeps its material-less [`Mesh3d`] — the collider is
/// derived from it — exactly as the playtest scene keeps its own, and the
/// material-bound children are the only drawn geometry.
fn attach_world_parts(
    mut commands: Commands,
    parts: Res<MissionWorldParts>,
    stage: Res<MissionStage>,
    spawned: Query<(Entity, &WorldObjectBinding), Added<WorldVisual>>,
) {
    for (entity, binding) in &spawned {
        let Some(object) = stage.world.object(binding.object()) else {
            continue;
        };
        let Resolved::Known(known) = object.mesh() else {
            continue;
        };
        let Some(drawn) = parts.for_mesh(&known.value) else {
            continue;
        };
        if drawn.is_empty() {
            continue;
        }
        // The node becomes a visibility node: a world record that presents no
        // mesh of its own (a role-`None` or cuboid record whose mesh reference
        // still resolved) has no `Visibility`, and a material-bound child
        // under a parent without one is Bevy's B0004. Stating `Inherited` here
        // is exactly what a node with drawn children is.
        commands.entity(entity).insert(Visibility::Inherited);
        for part in drawn {
            let child = commands
                .spawn((
                    Mesh3d(part.mesh.clone()),
                    MeshMaterial3d(part.material.clone()),
                    Transform::IDENTITY,
                    ChildOf(entity),
                ))
                .id();
            if part.image.as_deref().is_some_and(is_water_image) {
                commands.entity(child).insert(WorldWaterPart);
            }
        }
    }
}

/// The root entity of the drawn scenery ([`MissionWorldParts::scenery`]).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct WorldSceneryRoot;

/// Spawns the scenery once the world's first object is present, under one root
/// entity at the world origin; each node's composed transform is the child's
/// own.
fn attach_world_scenery(
    mut commands: Commands,
    parts: Res<MissionWorldParts>,
    loaded: Query<(), Added<WorldVisual>>,
    existing: Query<(), With<WorldSceneryRoot>>,
) {
    if loaded.is_empty() || !existing.is_empty() {
        return;
    }
    let Some(scenery) = parts.scenery() else {
        return;
    };
    let root = commands
        .spawn((WorldSceneryRoot, Transform::IDENTITY, Visibility::Inherited))
        .id();
    for placement in &scenery.placements {
        let Some(pieces) = scenery.meshes.get(&placement.mesh) else {
            continue;
        };
        let node = commands
            .spawn((placement.global, Visibility::Inherited, ChildOf(root)))
            .id();
        for part in pieces {
            let child = commands
                .spawn((
                    Mesh3d(part.mesh.clone()),
                    MeshMaterial3d(part.material.clone()),
                    Transform::IDENTITY,
                    ChildOf(node),
                ))
                .id();
            if part.image.as_deref().is_some_and(is_water_image) {
                commands.entity(child).insert(WorldWaterPart);
            }
        }
        // The sea is a contact source for the player. The original's sweep
        // tests a node that stores `CONTACT_SWEEP_NODE_FLAGS` and does so
        // through the node's own mesh; M01's water tiles store both bits
        // (`docs/findings/2026-10-22-m01-sea-contact.md`). Only the water
        // tiles are `PlaytestSea`; every other node that stores the bits (the
        // mountains, cliffs and land below the grouping nodes) is a solid
        // `WorldSceneryCollider` on the mesh's own collision triangles, so a
        // rock arch or cave keeps its opening (#1814). The fatal-contact judge
        // names `WorldColliderInstance`, so a hit on one is judged like the
        // world mesh; the response is the same contact law as the sea's.
        if placement.node_flags & CONTACT_SWEEP_NODE_FLAGS == CONTACT_SWEEP_NODE_FLAGS
            && let Some(collision) = scenery.colliders.get(&placement.mesh)
            && let Some(collider) = static_collider_components(placement.global, collision)
        {
            let is_water = pieces
                .iter()
                .any(|part| part.image.as_deref().is_some_and(is_water_image));
            if is_water {
                commands.spawn((collider, PlaytestSea, ChildOf(root)));
            } else {
                commands.spawn((
                    collider,
                    WorldSceneryCollider {
                        node_slot: placement.node_slot,
                    },
                    WorldColliderInstance::new(WorldCollisionRole::Solid),
                    ChildOf(root),
                ));
            }
        }
    }
}

/// A solid collider of one scenery node below a world grouping node: the
/// mesh's collision triangles at the node's composed transform (#1814).
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorldSceneryCollider {
    /// The stored slot of the node it belongs to.
    pub node_slot: u32,
}

/// Marks a drawn child whose material binds the world's water texture
/// ([`WATER_TEXTURE_NAME`] or [`C3_WATER_TEXTURE_NAME`]): the world's own sea
/// tiles. The owner-command smoke hides exactly these to measure how much of
/// the drawn sea they are.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WorldWaterPart;

/// Installs [`attach_world_parts`]. Added once by
/// [`super::compose::add_composition`], so every composition (windowed and
/// headless, first load and the composed restart) draws its world.
#[derive(Clone, Copy, Debug, Default)]
pub struct MissionWorldDrawPlugin;

impl Plugin for MissionWorldDrawPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                attach_world_parts,
                attach_world_scenery,
                anchor_horizon_to_camera,
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playtest_textures::neutral_material;
    use bevy::asset::{Assets, RenderAssetUsages};
    use bevy::mesh::PrimitiveTopology;

    /// A triangle-mesh part bound to `material`, storing `colors` when given.
    fn part_with(
        app: &mut App,
        material: Handle<StandardMaterial>,
        colors: Option<Vec<[f32; 4]>>,
    ) -> PlaytestPart {
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0_f32; 3]; 3]);
        if let Some(colors) = colors {
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
        }
        let mesh = app.world_mut().resource_mut::<Assets<Mesh>>().add(mesh);
        PlaytestPart {
            mesh,
            material,
            stored_material: 0,
            triangles: 1,
            image: None,
        }
    }

    /// **PARITY-M01-ISLAND-SHADING (#1706): a vertex-coloured world part draws
    /// unlit; an uncoloured one keeps the lit material it bound.**
    ///
    /// `ZBD/C3`'s world geometry stores a complete per-corner shading channel
    /// (the terrain tiles colour every corner in) while stored normals are
    /// partial — the authored colours are the surface's shading input, so the
    /// drawn material multiplies its texture by the stored colour only. The
    /// choice is this build's own presentation, not a measured original rule.
    #[test]
    fn accept_parity_m01_island_shading_vertex_coloured_world_parts_draw_unlit() {
        let mut app = App::new();
        app.insert_resource(Assets::<Mesh>::default());
        app.insert_resource(Assets::<StandardMaterial>::default());
        let shared = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(neutral_material(WORLD_NEUTRAL_COLOR));

        let coloured = part_with(
            &mut app,
            shared.clone(),
            Some(vec![[0.6, 0.7, 0.5, 1.0]; 3]),
        );
        let plain = part_with(&mut app, shared.clone(), None);
        let plain_material = plain.material.clone();
        let mut parts = vec![coloured, plain];

        stored_shading_materials(&mut app, &mut parts);

        let materials = app.world().resource::<Assets<StandardMaterial>>();
        let unlit = materials
            .get(&parts[0].material)
            .expect("the shaded copy exists");
        assert!(unlit.unlit, "the vertex-coloured part draws unlit");
        assert!(
            unlit.fog_enabled,
            "unlit changes the lighting term, not the weather binding"
        );
        assert_ne!(
            parts[0].material, shared,
            "the part drew its own copy so other users keep the shared one"
        );
        assert_eq!(parts[1].material, plain_material);
        let lit = materials
            .get(&parts[1].material)
            .expect("the uncoloured part's material is unchanged");
        assert!(!lit.unlit, "a part with no stored colours stays lit");

        // An already-unlit part (a `horizon` subtree part `sky_materials`
        // prepared) is left exactly as it was.
        let sky = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial {
                unlit: true,
                ..neutral_material(WORLD_NEUTRAL_COLOR)
            });
        let mut parts = vec![part_with(&mut app, sky.clone(), Some(vec![[1.0; 4]; 3]))];
        stored_shading_materials(&mut app, &mut parts);
        assert_eq!(parts[0].material, sky);
    }
}
