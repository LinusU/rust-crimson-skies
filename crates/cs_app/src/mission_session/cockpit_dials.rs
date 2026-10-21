//! The cockpit panel's six live dials, the artificial horizon and the
//! compass strip (PARITY3-COCKPIT-VIEW.04, Rally #1812).
//!
//! The player airframe's `planes.zbd` scene is `player` → `player_pfighter`
//! → `cockpit1` → `gauges` (slot 56); that subtree IS the instrument panel —
//! the finding `docs/findings/2026-10-20-m01-hud-art-instruments.md` measured
//! it, and its probe dump places, in the panel's own space:
//!
//! * top row (`y` ≈ −3.05): `missilegauge` (the ROCKETS count), `horiz`
//!   (the `horizonindicator.tif` face with the `pfhorizon` attitude ball
//!   inside) and `gungauge` (the GUNS count);
//! * bottom row (`y` ≈ −4.93): `speedometer` (MPH), `damageindicator` (the
//!   airframe silhouette) and `altimeter` (ALT);
//! * above the panel (`y` ≈ −1.47): `comp`, the compass strip and its `g730`
//!   bezel;
//! * off on the angled right-hand side panel: `nitrogauge`, drawn at its
//!   authored state because its needles have no measured driver.
//!
//! Where [`super::hud_art`] re-lays the same subtree out as the chase view's
//! corner cluster through the measured `instruments.zrd` placements, this
//! module draws it **at the stored dash transforms** — the cockpit pass the
//! finding names — as ordinary children of the player body, so the dials sit
//! exactly where the panel geometry of [`super::cockpit_interior`] puts the
//! rest of `cockpit1`.
//!
//! The live drive is not copied: every spawned part is tagged by
//! [`super::hud_art::tag_part`] with the same markers the corner cluster
//! carries and [`super::hud_art::update_hud_art`] drives both clusters from
//! the same authorities — needles, the compass tape, pips, glyph cells and
//! damage zones on the panel behave exactly as the HUD pass draws them. The
//! one element only the panel has, the `pfhorizon` attitude ball, is marked
//! [`HudAttitudeBall`](super::hud_art::HudAttitudeBall) and counter-rotated
//! by pitch and roll inside the same system — a **designed** rule: the
//! original's ball mapping, axis and range are unmeasured and recorded as
//! unknown in `docs/findings/2026-10-21-parity3-cockpit-view-04-dials.md`.
//! `lowalt_on`/`stallwarning_on`, the `mg`/`gg` arrows and the nitro needles
//! keep their authored state for the same reason.
//!
//! Like the interior, the cluster hangs under a root of its own whose
//! `Visibility` is set explicitly each frame (the body itself is hidden in
//! the pilot-eye views, see [`super::cockpit_camera`]): visible while
//! [`MissionViewMode::at_pilot_eye`], hidden in the chase view and during
//! the launch cinematic.
//!
//! Nothing here is `verified_original`.

use bevy::app::PostUpdate;
use bevy::asset::{AssetApp, Assets};
use bevy::image::Image;
use bevy::prelude::{
    App, Component, Entity, IntoScheduleConfigs, Quat, Res, Resource, StandardMaterial, Transform,
    Visibility, With, World,
};
use cs_content::scene::{NodeKind, SceneGraph, SceneNode};

use super::compose::MissionCompositionError;
use super::hud_art::{tag_part, upload_gauge_part};
use super::launch_cinematic::LaunchCinematic;
use super::player_visual::{MISSION_PLAYER_AIRCRAFT_ROOT_NAME, PlayerAirframeSource};
use super::view_cycle::MissionViewMode;
use crate::playtest_retail::{
    AircraftPartAsset, PlaytestConfig, STORED_AIRCRAFT_NOSE_AXIS, UndrawnBinding, aircraft_graph,
    nose_mapping, playtest_adapter,
};
use crate::playtest_textures::TextureBinder;
use crate::world::WorldMeshes;

/// Marks the root entity every panel-dial part hangs under.
#[derive(Component, Clone, Copy, Debug)]
pub struct CockpitDialsRoot;

/// One node of the `gauges` subtree as the measurement saw it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CockpitDialNode {
    /// The stored slot.
    pub slot: u32,
    /// The authored name.
    pub name: String,
    /// The mesh-array slot the node binds, when it binds one.
    pub mesh_index: Option<u32>,
    /// The node kind's name (`Object3d`, `Lod`, …).
    pub kind: String,
    /// Depth below `gauges`.
    pub depth: usize,
}

/// What the dials build decided: the whole subtree, the drawn parts and
/// every mesh binding it did not draw, with its reason.
#[derive(Clone, Debug, Default)]
pub struct CockpitDialsReport {
    /// Every node of the subtree, in the graph's preorder.
    pub nodes: Vec<CockpitDialNode>,
    /// The drawn bindings' `(slot, name)`.
    pub drawn: Vec<(u32, String)>,
    /// The bindings not drawn.
    pub undrawn: Vec<UndrawnBinding>,
    /// Elements drawn at their authored state because no measured driver
    /// exists, and other recorded unknowns.
    pub unknowns: Vec<String>,
}

/// The dial parts, held as a resource so an `R` restart re-spawns them.
#[derive(Resource)]
pub struct MissionCockpitDials {
    parts: Vec<AircraftPartAsset>,
    rotation: Quat,
    report: CockpitDialsReport,
}

impl MissionCockpitDials {
    /// Dials from already-built parts and the nose mapping, with an empty
    /// report (authored stages and tests).
    #[must_use]
    pub fn from_parts(parts: Vec<AircraftPartAsset>, rotation: Quat) -> Self {
        Self {
            parts,
            rotation,
            report: CockpitDialsReport::default(),
        }
    }

    /// The drawn parts, in preorder.
    #[must_use]
    pub fn parts(&self) -> &[AircraftPartAsset] {
        &self.parts
    }

    /// What the build decided.
    #[must_use]
    pub const fn report(&self) -> &CockpitDialsReport {
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

/// Depth below `gauges`, for the report.
fn depth_of(node: &SceneNode, gauges: &SceneNode, planes: &SceneGraph) -> usize {
    let mut depth = 0;
    let mut cursor = Some(node);
    while let Some(current) = cursor {
        if current.index() == gauges.index() {
            break;
        }
        depth += 1;
        cursor = current.parent().and_then(|id| planes.node(id));
    }
    depth
}

/// Builds the panel-dial parts of the player airframe's `gauges` subtree at
/// their stored transforms.
///
/// Unlike [`super::hud_art::build_hud_art`] — which re-places the subtree at
/// the measured `instruments.zrd` HUD positions — every part keeps its
/// composed stored transform: `instruments.zrd` places nothing here, `horiz`
/// included. Every mesh-bearing node is drawn (the furniture `markers` nodes
/// bind no mesh and only appear in the report).
///
/// # Errors
///
/// [`MissionCompositionError::AirframeVisual`] when the player tree, its
/// `gauges` node or the mesh pipeline refuses, or no mesh of the subtree
/// builds.
pub fn build_cockpit_dials(
    app: &mut App,
    source: &PlayerAirframeSource,
) -> Result<MissionCockpitDials, MissionCompositionError> {
    if !app.world().contains_resource::<Assets<StandardMaterial>>() {
        app.init_asset::<StandardMaterial>();
    }
    if !app.world().contains_resource::<Assets<Image>>() {
        app.init_asset::<Image>();
    }
    let aircraft = source.sources().aircraft();
    let adapter = playtest_adapter().map_err(|error| refuse(error.to_string()))?;
    let planes = aircraft_graph(aircraft, &adapter).map_err(|error| refuse(error.to_string()))?;
    let gauges = super::hud_art::find_path(
        &planes,
        &[
            MISSION_PLAYER_AIRCRAFT_ROOT_NAME,
            "player_pfighter",
            "cockpit1",
            "gauges",
        ],
    )
    .ok_or_else(|| refuse("the player tree has no cockpit1 > gauges path".to_owned()))?;

    let mut report = CockpitDialsReport::default();
    report.unknowns.push(
        "lowalt_on/stallwarning_on/mgarrow/ggarrow/nitro needles keep their authored \
         state: no measured driver (the finding records thresholds and roles as \
         unmeasured)"
            .to_owned(),
    );
    report.unknowns.push(
        "the pfhorizon ball's attitude mapping, axis and range are unmeasured: \
         the counter-rotation drive is designed"
            .to_owned(),
    );

    let config = PlaytestConfig::documented();
    let mut meshes = WorldMeshes::new();
    let mut binder = TextureBinder::new(
        source.sources().textures(),
        config.textured,
        config.decal_offset,
    );
    let mut parts = Vec::new();
    for node in planes.subtree(gauges.id()) {
        report.nodes.push(CockpitDialNode {
            slot: node.index(),
            name: node.name().trim_end_matches('\0').to_owned(),
            mesh_index: node.mesh().map(|binding| binding.index),
            kind: kind_name(node),
            depth: depth_of(node, gauges, &planes),
        });
        let mut reasons = Vec::new();
        let Some((handle, pieces)) =
            upload_gauge_part(app, aircraft, &mut binder, &mut meshes, node, &mut reasons)?
        else {
            if let Some(binding) = node.mesh() {
                report.undrawn.push(UndrawnBinding {
                    node_slot: node.index(),
                    node_name: node.name().trim_end_matches('\0').to_owned(),
                    mesh_index: binding.index,
                    reason: reasons.join("; "),
                });
            }
            continue;
        };
        let local = crate::scene::NodeVisualTransform::from_canonical(node.visual_transform())
            .map_err(|error| {
                refuse(format!(
                    "gauge node {} would not convert: {error}",
                    node.index()
                ))
            })?
            .global()
            .compute_transform();
        report
            .drawn
            .push((node.index(), node.name().trim_end_matches('\0').to_owned()));
        parts.push(AircraftPartAsset {
            node_slot: node.index(),
            node_name: node.name().trim_end_matches('\0').to_owned(),
            mesh: handle,
            pieces,
            local,
        });
    }
    let _textures = binder.finish();
    if parts.is_empty() {
        return Err(refuse(
            "the gauges subtree draws no mesh: the panel has no dials".to_owned(),
        ));
    }
    let rotation = nose_mapping(STORED_AIRCRAFT_NOSE_AXIS)
        .map_err(|error| refuse(format!("the measured nose axis would not map: {error}")))?;
    Ok(MissionCockpitDials {
        parts,
        rotation,
        report,
    })
}

/// Spawns the panel dials under `body`: one root (hidden until a pilot-eye
/// view shows it) and one child per drawn binding, tagged with the markers
/// [`super::hud_art::update_hud_art`] drives — the same tagging the corner
/// cluster gets, so no drive logic is copied. A stage with no
/// [`MissionCockpitDials`] spawns nothing.
pub fn spawn_cockpit_dials(world: &mut World, body: Entity) {
    let Some(dials) = world.remove_resource::<MissionCockpitDials>() else {
        return;
    };
    let root = world
        .spawn((
            CockpitDialsRoot,
            Transform::IDENTITY,
            Visibility::Hidden,
            bevy::prelude::ChildOf(body),
        ))
        .id();
    for part in dials.parts() {
        let base = part.oriented(dials.rotation);
        let entity = part.spawn(world, root, base);
        tag_part(world, entity, part, &base);
    }
    world.insert_resource(dials);
}

/// Installs [`drive_cockpit_dials`] before transform propagation.
pub fn install_cockpit_dials(app: &mut App) {
    app.init_resource::<MissionViewMode>();
    app.add_systems(
        PostUpdate,
        drive_cockpit_dials.before(bevy::transform::TransformSystems::Propagate),
    );
}

/// Shows the panel dials in the pilot-eye views and hides them otherwise —
/// the same rule [`super::cockpit_interior::drive_cockpit_interior`] gives
/// the rest of the cockpit. The explicit `Visible` overrides the hidden body
/// the cluster hangs under.
pub fn drive_cockpit_dials(
    view: Res<MissionViewMode>,
    cinematic: Option<Res<LaunchCinematic>>,
    mut roots: bevy::prelude::Query<&mut Visibility, With<CockpitDialsRoot>>,
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
