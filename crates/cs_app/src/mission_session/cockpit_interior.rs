//! The player's cockpit interior (PARITY3-COCKPIT-VIEW.03, Rally #1811).
//!
//! The M01 player's `planes.zbd` scene is `player` → `player_pfighter` →
//! `cockpit1`; the interior the owner's Drydock frame shows (riveted canopy
//! struts, the panel body, the gunsight glass on its stalk) is the
//! mesh-bearing part of that `cockpit1` subtree. This module draws it through
//! the one production airframe path — [`aircraft_graph`], the stored render
//! mesh, the F17-B upload and [`TextureBinder`] — and parents it to the
//! player body, **visible only while the mission view is a pilot-eye view**
//! ([`MissionViewMode::at_pilot_eye`]).
//!
//! The body is hidden in the pilot-eye views (so the exterior airframe does
//! not surround the eye, see [`super::cockpit_camera`]); the interior hangs
//! under a root of its own whose `Visibility` is set explicitly each frame, so
//! it shows while the body is hidden and hides again in the chase view.
//!
//! **Left out, with its reason, in the report:** the `gauges` subtree is the
//! live instrument dials (PARITY3-COCKPIT-VIEW.04, #1812); every other
//! mesh-bearing node of `cockpit1` is drawn. The gunsight glass is drawn
//! alpha-blended from its texture's own coverage plane — no opacity constant
//! is invented. Anything the stored records do not decide (see the finding
//! `docs/findings/2026-10-21-parity3-cockpit-view-03-interior.md`) is recorded
//! there as unknown.
//!
//! Nothing here is `verified_original`.

use bevy::app::PostUpdate;
use bevy::asset::{AssetApp, Assets};
use bevy::image::Image;
use bevy::prelude::{
    AlphaMode, App, Component, Entity, Handle, IntoScheduleConfigs, Mesh, Quat, Res, Resource,
    StandardMaterial, Transform, Visibility, With, World,
};
use cs_content::scene::{NodeKind, SceneNode};
use cs_types::content::Resolved;

use super::compose::MissionCompositionError;
use super::launch_cinematic::LaunchCinematic;
use super::player_visual::{MISSION_PLAYER_AIRCRAFT_ROOT_NAME, PlayerAirframeSource};
use super::view_cycle::MissionViewMode;
use crate::playtest_retail::{
    AircraftPartAsset, PlaytestConfig, STORED_AIRCRAFT_NOSE_AXIS, UndrawnBinding, aircraft_graph,
    nose_mapping, playtest_adapter,
};
use crate::playtest_textures::{NeutralColor, TextureBinder};
use crate::world::WorldMeshes;
use crate::world::retail::{stored_presentation_unknowns, stored_render_mesh};

/// The authored name of the cockpit subtree under the player airframe.
pub const COCKPIT_INTERIOR_NODE_NAME: &str = "cockpit1";

/// The authored name of the instrument-dial subtree, drawn by
/// PARITY3-COCKPIT-VIEW.04 and not here.
pub const COCKPIT_GAUGES_NODE_NAME: &str = "gauges";

/// The grey a part draws in when its stored material names no texture. The
/// stored cockpit materials all name one; this is the playtest's neutral
/// development colour, never claimed as original.
const NEUTRAL_INTERIOR_COLOR: [f32; 4] = [0.5, 0.5, 0.5, 1.0];

/// The node whose mesh stores the gunsight glass pane: `inside` (slot 54).
pub const COCKPIT_GLASS_NODE_NAME: &str = "inside";

/// The texture the gunsight glass pane binds: `cockpit6`, a 128x128 image
/// whose coverage plane is almost entirely partial (15 570 of 16 384 texels
/// strictly between 0 and 255), on a two-triangle quad. Picked by name from
/// the measured material list; the stored records do not flag a glass class.
pub const COCKPIT_GLASS_TEXTURE: &str = "cockpit6";

/// Marks the root entity every interior part hangs under.
#[derive(Component, Clone, Copy, Debug)]
pub struct CockpitInteriorRoot;

/// One node of the `cockpit1` subtree as the measurement saw it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CockpitInteriorNode {
    /// The stored slot.
    pub slot: u32,
    /// The authored name.
    pub name: String,
    /// The mesh-array slot the node binds, when it binds one.
    pub mesh_index: Option<u32>,
    /// The node kind's name (`Object3d`, `Lod`, …).
    pub kind: String,
    /// Depth below `cockpit1`.
    pub depth: usize,
}

/// What the interior build decided: the whole subtree, the drawn parts and
/// every mesh binding it did not draw, with its reason.
#[derive(Clone, Debug, Default)]
pub struct CockpitInteriorReport {
    /// Every node of the subtree, in the graph's preorder.
    pub nodes: Vec<CockpitInteriorNode>,
    /// The drawn bindings' `(slot, name)`.
    pub drawn: Vec<(u32, String)>,
    /// The pieces drawn alpha-blended from their texture's coverage plane, as
    /// `(node name, image key)`.
    pub glass: Vec<(String, String)>,
    /// The bindings not drawn.
    pub undrawn: Vec<UndrawnBinding>,
}

/// The interior parts, held as a resource so an `R` restart re-spawns them.
#[derive(Resource)]
pub struct MissionCockpitInterior {
    parts: Vec<AircraftPartAsset>,
    rotation: Quat,
    report: CockpitInteriorReport,
}

impl MissionCockpitInterior {
    /// An interior from already-built parts and the nose mapping, with an
    /// empty report (authored stages and tests).
    #[must_use]
    pub fn from_parts(parts: Vec<AircraftPartAsset>, rotation: Quat) -> Self {
        Self {
            parts,
            rotation,
            report: CockpitInteriorReport::default(),
        }
    }

    /// The drawn parts, in preorder.
    #[must_use]
    pub fn parts(&self) -> &[AircraftPartAsset] {
        &self.parts
    }

    /// What the build decided.
    #[must_use]
    pub const fn report(&self) -> &CockpitInteriorReport {
        &self.report
    }
}

fn refuse(reason: String) -> MissionCompositionError {
    MissionCompositionError::AirframeVisual(reason)
}

fn kind_name(node: &SceneNode) -> String {
    match node.kind() {
        NodeKind::Lod(_) => "Lod".to_owned(),
        other => format!("{other:?}")
            .split(['(', ' ', '{'])
            .next()
            .unwrap_or_default()
            .to_owned(),
    }
}

/// Why a node is a damage decal rather than intact furniture, or `None`.
///
/// Designed from the stored names and textures, not measured: the five
/// `bulletN` groups (`bulNa`..`bulNd`) bind `cpit_bullethole1/2` and
/// `pcdp4`/`pcdp6` bind `damage1`, all tiny quads that only make sense on a
/// damaged cockpit. When and how the original shows them is unmeasured, so the
/// intact interior leaves them undrawn.
fn damage_decal_reason(node: &SceneNode, planes: &cs_content::scene::SceneGraph) -> Option<String> {
    let under_bullets = node
        .parent()
        .and_then(|id| planes.node(id))
        .is_some_and(|parent| {
            parent.name().starts_with("bullet") && node.name().starts_with("bul")
        });
    if under_bullets || node.name().starts_with("pcdp") {
        return Some(
            "cockpit damage decal (bullet hole / damage1 quad): shown by an unmeasured \
             damage state, so the intact interior leaves it undrawn"
                .to_owned(),
        );
    }
    None
}

/// Builds the interior parts of the player airframe's `cockpit1` subtree.
///
/// # Errors
///
/// [`MissionCompositionError::AirframeVisual`] when the player root or its
/// `cockpit1` is absent or ambiguous, or no mesh of the subtree builds.
pub fn build_cockpit_interior(
    app: &mut App,
    source: &PlayerAirframeSource,
) -> Result<MissionCockpitInterior, MissionCompositionError> {
    if !app.world().contains_resource::<Assets<StandardMaterial>>() {
        app.init_asset::<StandardMaterial>();
    }
    if !app.world().contains_resource::<Assets<Image>>() {
        app.init_asset::<Image>();
    }
    let container = source.sources().aircraft();
    let adapter = playtest_adapter().map_err(|error| refuse(error.to_string()))?;
    let planes = aircraft_graph(container, &adapter).map_err(|error| refuse(error.to_string()))?;
    let root = planes
        .root(MISSION_PLAYER_AIRCRAFT_ROOT_NAME)
        .map_err(|_| {
            refuse(format!(
                "no airframe root {MISSION_PLAYER_AIRCRAFT_ROOT_NAME}"
            ))
        })?;
    let members = planes.subtree(root.id());
    let mut cockpits = members
        .iter()
        .copied()
        .filter(|node| node.name() == COCKPIT_INTERIOR_NODE_NAME);
    let (Some(cockpit), None) = (cockpits.next(), cockpits.next()) else {
        return Err(refuse(format!(
            "{MISSION_PLAYER_AIRCRAFT_ROOT_NAME} does not hold exactly one \
             {COCKPIT_INTERIOR_NODE_NAME}"
        )));
    };
    let subtree = planes.subtree(cockpit.id());
    let gauges: Vec<u32> = subtree
        .iter()
        .filter(|node| node.name() == COCKPIT_GAUGES_NODE_NAME)
        .flat_map(|gauges| planes.subtree(gauges.id()))
        .map(|node| node.index())
        .collect();

    let depth_of = |node: &SceneNode| {
        let mut depth = 0;
        let mut cursor = Some(node);
        while let Some(current) = cursor {
            if current.index() == cockpit.index() {
                break;
            }
            depth += 1;
            cursor = current.parent().and_then(|id| planes.node(id));
        }
        depth
    };
    let config = PlaytestConfig::documented();
    let mut report = CockpitInteriorReport::default();
    let mut meshes = WorldMeshes::new();
    let mut binder = TextureBinder::new(
        source.sources().textures(),
        config.textured,
        config.decal_offset,
    );
    let mut parts = Vec::new();
    for node in &subtree {
        report.nodes.push(CockpitInteriorNode {
            slot: node.index(),
            name: node.name().to_owned(),
            mesh_index: node.mesh().map(|binding| binding.index),
            kind: kind_name(node),
            depth: depth_of(node),
        });
        let Some(binding) = node.mesh() else { continue };
        let undrawn = |reason: String| UndrawnBinding {
            node_slot: node.index(),
            node_name: node.name().to_owned(),
            mesh_index: binding.index,
            reason,
        };
        if gauges.contains(&node.index()) {
            report.undrawn.push(undrawn(
                "under `gauges`: the live instrument dials are PARITY3-COCKPIT-VIEW.04 (#1812)"
                    .to_owned(),
            ));
            continue;
        }
        if let Some(reason) = damage_decal_reason(node, &planes) {
            report.undrawn.push(undrawn(reason));
            continue;
        }
        let id = match &binding.mesh {
            Resolved::Known(known) => known.value.clone(),
            Resolved::Unknown { reason, .. } => {
                report
                    .undrawn
                    .push(undrawn(format!("the mesh binding is unknown: {reason}")));
                continue;
            }
        };
        let Some(slot) = container.meshes().get(binding.index) else {
            report
                .undrawn
                .push(undrawn("the container stores no such mesh".to_owned()));
            continue;
        };
        let render = match stored_render_mesh(slot) {
            Ok(render) => render,
            Err(error) => {
                report
                    .undrawn
                    .push(undrawn(format!("the stored mesh would not build: {error}")));
                continue;
            }
        };
        let unknowns = stored_presentation_unknowns(&render);
        if let Err(reason) = meshes.insert_render_mesh(id.clone(), &render, &unknowns) {
            report
                .undrawn
                .push(undrawn(format!("the mesh would not upload: {reason}")));
            continue;
        }
        let Some(uploaded) = meshes.get(&id) else {
            report
                .undrawn
                .push(undrawn("the uploaded mesh is not registered".to_owned()));
            continue;
        };
        let handle: Handle<Mesh> = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(uploaded.mesh().clone());
        let pieces = binder.parts(
            app,
            container,
            uploaded,
            NeutralColor(NEUTRAL_INTERIOR_COLOR),
            binding.index,
        );
        if node.name() == COCKPIT_GLASS_NODE_NAME {
            let mut materials = app.world_mut().resource_mut::<Assets<StandardMaterial>>();
            for piece in &pieces {
                let Some(image) = piece.image.as_deref() else {
                    continue;
                };
                if image.ends_with(&format!(":{COCKPIT_GLASS_TEXTURE}"))
                    && let Some(mut material) = materials.get_mut(&piece.material)
                {
                    material.alpha_mode = AlphaMode::Blend;
                    report
                        .glass
                        .push((node.name().to_owned(), image.to_owned()));
                }
            }
        }
        let local = crate::scene::NodeVisualTransform::from_canonical(node.visual_transform())
            .map_err(|error| {
                refuse(format!(
                    "cockpit node {} would not convert: {error}",
                    node.index()
                ))
            })?
            .global()
            .compute_transform();
        report.drawn.push((node.index(), node.name().to_owned()));
        parts.push(AircraftPartAsset {
            node_slot: node.index(),
            node_name: node.name().to_owned(),
            mesh: handle,
            pieces,
            local,
        });
    }
    let _textures = binder.finish();
    if parts.is_empty() {
        return Err(refuse(
            "the cockpit1 subtree draws no mesh outside the gauges".to_owned(),
        ));
    }
    let rotation = nose_mapping(STORED_AIRCRAFT_NOSE_AXIS)
        .map_err(|error| refuse(format!("the measured nose axis would not map: {error}")))?;
    Ok(MissionCockpitInterior {
        parts,
        rotation,
        report,
    })
}

/// Spawns the interior under `body`: one root (hidden until a pilot-eye view
/// shows it) and one child per drawn binding. A stage with no
/// [`MissionCockpitInterior`] spawns nothing.
pub fn spawn_cockpit_interior(world: &mut World, body: Entity) {
    let Some(interior) = world.remove_resource::<MissionCockpitInterior>() else {
        return;
    };
    let root = world
        .spawn((
            CockpitInteriorRoot,
            Transform::IDENTITY,
            Visibility::Hidden,
            bevy::prelude::ChildOf(body),
        ))
        .id();
    for part in interior.parts() {
        let base = part.oriented(interior.rotation);
        part.spawn(world, root, base);
    }
    world.insert_resource(interior);
}

/// Installs [`drive_cockpit_interior`] before transform propagation.
pub fn install_cockpit_interior(app: &mut App) {
    app.init_resource::<MissionViewMode>();
    app.add_systems(
        PostUpdate,
        drive_cockpit_interior.before(bevy::transform::TransformSystems::Propagate),
    );
}

/// Shows the interior in the pilot-eye views and hides it otherwise. The
/// explicit `Visible` overrides the hidden body it hangs under.
pub fn drive_cockpit_interior(
    view: Res<MissionViewMode>,
    cinematic: Option<Res<LaunchCinematic>>,
    mut roots: bevy::prelude::Query<&mut Visibility, With<CockpitInteriorRoot>>,
) {
    let cinematic_plays = cinematic.is_some_and(|phase| phase.is_playing());
    let wanted = if view.at_pilot_eye() && !cinematic_plays {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    for mut visibility in &mut roots {
        if *visibility != wanted {
            *visibility = wanted;
        }
    }
}
