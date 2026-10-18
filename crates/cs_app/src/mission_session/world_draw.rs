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

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use bevy::asset::{AssetApp, Assets};
use bevy::image::Image;
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::{
    Added, App, ChildOf, Commands, Entity, Mesh, Mesh3d, MeshMaterial3d, Plugin, Query, Res,
    Resource, StandardMaterial, Transform, Update, Vec3, Visibility,
};
use cs_content::world::WorldDefinition;
use cs_types::content::{ContentId, Resolved};

use crate::playtest_retail::{WorldSources, read_world_sources};
use crate::playtest_textures::{NeutralColor, PlaytestPart, TextureBinder};
use crate::render::horizon::HorizonSpec;
use crate::world::WorldMeshes;
use crate::world::contacts::{WorldObjectBinding, WorldVisual};
use crate::world::spawn::canonical_matrix;

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
        }
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
        let bound = binder.parts(
            app,
            source.sources.world(),
            world_mesh,
            NeutralColor(WORLD_NEUTRAL_COLOR),
            stored_slot,
        );
        parts.insert(known.value.clone(), bound);
    }
    let water = measure_water_level(app, definition, &parts);
    MissionWorldParts::from_parts(parts).with_water_level(water)
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
    true
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
            commands.spawn((
                Mesh3d(part.mesh.clone()),
                MeshMaterial3d(part.material.clone()),
                Transform::IDENTITY,
                ChildOf(entity),
            ));
        }
    }
}

/// Installs [`attach_world_parts`]. Added once by
/// [`super::compose::add_composition`], so every composition (windowed and
/// headless, first load and the composed restart) draws its world.
#[derive(Clone, Copy, Debug, Default)]
pub struct MissionWorldDrawPlugin;

impl Plugin for MissionWorldDrawPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, attach_world_parts);
    }
}
