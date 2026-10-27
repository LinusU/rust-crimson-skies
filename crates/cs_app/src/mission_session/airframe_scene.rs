//! The mission session's **canonical airframe scene** (F11-F, Rally #2122):
//! the F11-C machinery — [`crate::scene::start_airframe_scene`],
//! [`crate::scene::end_airframe_scene`] and
//! [`crate::scene::AirframeScenePlugin`] — driven by the mission session's
//! own lifetime instead of a fixture producer.
//!
//! Three seams, all owned by the session:
//!
//! * **Session start** — `super::compose::add_composition` calls
//!   [`MissionAirframeScene::build`] beside the drawn player visual's own
//!   build, keeps the result as this resource and loads it through
//!   [`start_mission_airframe_scene`]. The converted graph is the production
//!   [`crate::playtest_retail::aircraft_graph`] conversion of the stage's
//!   `zbd/planes.zbd` — the GameZ node-array reader's whole-container
//!   `SceneGraph`, whose ids spell the container they came from — and the
//!   [`crate::airframe_visual::AirframeVisual`] names the airframe the
//!   mission's own start resolution committed (`airframe/player_pfighter`
//!   for M01), never a pinned default.
//! * **Session retry** — [`super::host::MissionHost::restart`] calls
//!   [`start_mission_airframe_scene`] again after the player half is
//!   rebuilt: `start_airframe_scene`'s own contract supersedes the live
//!   generation and releases its entities, and a refused load leaves the
//!   previous generation running — the failure is the restart's named
//!   error, never a silent half-scene.
//! * **Session end** — [`super::compose::teardown`] calls
//!   [`crate::scene::end_airframe_scene`], releasing every entity the live
//!   generation owns, and removes this resource and the scene's own
//!   records so a second composition in the same process starts from
//!   nothing.
//!
//! The [`crate::scene::LodDistance`] the plugin's LOD pass reads is supplied
//! by the composition from the viewer stage's own declared distance —
//! [`super::player_visual::MISSION_PLAYER_LOD_DISTANCE_M`], the same
//! designed 20 m the drawn visual's LOD pick is selected at (the original's
//! LOD rule is unmeasured; the value is labelled designed where it is
//! declared). The scene owner invents no distance of its own.
//!
//! A stage whose player airframe stayed [`Resolved::Unknown`] — every
//! mission the engine-facts table commits no row for — builds **no** scene:
//! `build` answers `None`, the composition carries no
//! [`MissionAirframeScene`] and the unknown's own recorded reason stands.
//! Nothing is loaded in its place (AGENTS.md rule 4).

use std::sync::Arc;

use bevy::prelude::{Resource, World};
use cs_content::scene::{SceneGraph, SceneNodeId};
use cs_types::content::Resolved;

use crate::airframe_visual::AirframeVisual;
use crate::playtest_retail::{aircraft_graph, playtest_adapter};
use crate::scene::{
    AirframeSessionError, SceneCollisionGeometry, SceneGeneration, start_airframe_scene,
};

use super::compose::MissionCompositionError;
use super::player_visual::PlayerAirframeSource;

/// Resource: everything a session start or retry needs to load the
/// mission's named airframe scene — the checked [`AirframeVisual`], the
/// converted container graph it draws from and the collision-geometry table
/// the load consumes.
///
/// Built once per composition by [`MissionAirframeScene::build`], inserted
/// by `add_composition` and removed by
/// [`super::compose::teardown`]. The collision table is empty today: no
/// node-level airframe geometry is measured or declared, so the load
/// reports every collider-role socket by name rather than fit a shape
/// nobody measured.
#[derive(Resource, Clone, Debug)]
pub struct MissionAirframeScene {
    visual: AirframeVisual,
    graph: Arc<SceneGraph>,
    collision: SceneCollisionGeometry,
}

impl MissionAirframeScene {
    /// The visual the load serves: the resolved airframe and its checked
    /// root in this container.
    #[must_use]
    pub const fn visual(&self) -> &AirframeVisual {
        &self.visual
    }

    /// The converted graph the subtree is imported from.
    #[must_use]
    pub const fn graph(&self) -> &Arc<SceneGraph> {
        &self.graph
    }

    /// The one-shot collision table each load consumes.
    #[must_use]
    pub const fn collision(&self) -> &SceneCollisionGeometry {
        &self.collision
    }

    /// Loads this scene into `world` through the production session entry:
    /// a fresh [`SceneGeneration`] on success, the previous scene released
    /// only after the new import committed.
    ///
    /// # Errors
    ///
    /// [`AirframeSessionError`] when the load is refused (the running scene
    /// is then untouched) or was not served.
    pub fn load(&self, world: &mut World) -> Result<SceneGeneration, AirframeSessionError> {
        start_airframe_scene(
            world,
            self.visual.clone(),
            self.graph.clone(),
            self.collision.clone(),
        )
    }

    /// Builds the session's scene source from the stage's player-airframe
    /// source: the `airframe/<key>` the mission's start resolution named,
    /// resolved to its enclosing root inside the source's converted
    /// `planes.zbd` graph.
    ///
    /// `Ok(None)` when the stage's airframe is [`Resolved::Unknown`]: the
    /// recorded reason stays on the resolution itself and the composition
    /// installs no scene — an unknown airframe is never replaced by a
    /// nearest or default model.
    ///
    /// # Errors
    ///
    /// [`MissionCompositionError::AirframeVisual`] when the declared
    /// adapter, the container's conversion, the root resolution or the
    /// visual reference refuses — every refusal names the step and the
    /// key it refused at.
    pub fn build(source: &PlayerAirframeSource) -> Result<Option<Self>, MissionCompositionError> {
        let Resolved::Known(airframe) = source.airframe() else {
            return Ok(None);
        };
        let adapter = playtest_adapter().map_err(|error| {
            MissionCompositionError::AirframeVisual(format!(
                "the player airframe's scene adapter refuses: {error}"
            ))
        })?;
        let graph = aircraft_graph(source.sources().aircraft(), &adapter).map_err(|error| {
            MissionCompositionError::AirframeVisual(format!(
                "the player airframe's scene graph refuses: {error}"
            ))
        })?;
        let root = airframe_root(&graph, airframe.value.key())
            .map_err(MissionCompositionError::AirframeVisual)?;
        let visual = AirframeVisual::new(airframe.value.clone(), graph.container().clone(), root)
            .map_err(|error| {
            MissionCompositionError::AirframeVisual(format!(
                "the player airframe's scene visual refuses: {error}"
            ))
        })?;
        Ok(Some(Self {
            visual,
            graph: Arc::new(graph),
            collision: SceneCollisionGeometry::new(),
        }))
    }
}

/// The scene root a named airframe subtree hangs from inside `graph`.
///
/// The name is the airframe's own key (`player_pfighter`), resolved through
/// the container's parent records — never an index or a prefix guess. M01's
/// `player_pfighter` is not itself a root: it hangs under the `player`
/// wrapper root, which is the checked [`cs_content::scene::SceneRootRef`]
/// the import starts from. A name the container does not store exactly
/// once is refused rather than resolved by guess.
fn airframe_root(graph: &SceneGraph, name: &str) -> Result<SceneNodeId, String> {
    let mut found = graph.nodes().iter().filter(|node| node.name() == name);
    let Some(named) = found.next() else {
        return Err(format!(
            "the player airframe's scene graph {} names no node {name:?}",
            graph.container().key()
        ));
    };
    if found.next().is_some() {
        return Err(format!(
            "the player airframe's scene graph {} names {name:?} more than once, so the \
             airframe's root is ambiguous",
            graph.container().key()
        ));
    }
    let mut root = named;
    while let Some(parent) = root.parent().and_then(|id| graph.node(id)) {
        root = parent;
    }
    Ok(root.id().clone())
}

/// The session's airframe-scene load, called by the composition's session
/// start and by [`super::host::MissionHost::restart`]'s retry.
///
/// `Ok(None)` when the world carries no [`MissionAirframeScene`] — a
/// synthetic stage or a mission whose airframe resolved unknown — so the
/// same call is correct on every composition path and the absence is the
/// stage's own record, not a fault this seam invents.
///
/// # Errors
///
/// [`AirframeSessionError`] from the load itself.
pub fn start_mission_airframe_scene(
    world: &mut World,
) -> Result<Option<SceneGeneration>, AirframeSessionError> {
    let Some(scene) = world.get_resource::<MissionAirframeScene>().cloned() else {
        return Ok(None);
    };
    scene.load(world).map(Some)
}
