//! The player airframe's **drawn visual** in the mission composition
//! (VS-M01-RT-PLAYER-AIRFRAME-VISUAL, Rally #1216): the measured intact
//! selection of the airframe the original assigns M01's player — the
//! Devastator, scene root `player_pfighter`, model `piratefighter` —
//! spawned under the player's flight body instead of nothing.
//!
//! Everything in this module goes through the playtest's own production
//! path: [`read_aircraft_sources`] reads `zbd/planes.zbd` and the group's
//! texture archive, [`build_aircraft_visual`] runs the one pinned
//! selection/mesh-upload/textured-piece build the playtest scene runs, and
//! [`spawn_player_visual`] parents the parts to the body exactly the way
//! `playtest::scene::spawn_retail_parts` does — one child per drawn binding,
//! so an `R` restart and [`super::teardown`] take the whole airframe with the
//! body and never leave a second copy.
//!
//! The pinned values are **measured**, recorded in
//! `docs/findings/2026-10-10-vs-m01-rt-player-airframe-visual.md`: the
//! `player` root (slot 1418) whose `player_pfighter` subtree holds the player
//! Devastator, its `healthy` intact node (slot 149), the four `Lod` bands
//! (`nearest` 0–150 m chosen at the playtest's designed 20 m distance), and
//! `staticprop1` (slot 210) — never the bloodhawk's values. Which propeller
//! state and which LOD band the original shows are unmeasured; the designed
//! picks are labelled on the composition's banner and in the finding, exactly
//! as the playtest labels its own.

use std::path::Path;

use bevy::asset::{AssetApp, Assets};
use bevy::image::Image;
use bevy::prelude::{Entity, Quat, Resource, StandardMaterial, World};

use crate::playtest::propeller::PropellerSpin;
use crate::playtest_retail::{
    AircraftPartAsset, AircraftSources, PlaytestAircraftReport, PlaytestConfig, PropellerSpinSpec,
    STORED_AIRCRAFT_NOSE_AXIS, build_aircraft_visual, nose_mapping, read_aircraft_sources,
};
use crate::playtest_textures::TextureBinder;

use super::compose::MissionCompositionError;

/// The `planes.zbd` root the player Devastator's subtree hangs under:
/// `player` (stored slot **1418**), whose only child is `player_pfighter`
/// (slot 44). The generic `piratefighter` root (slot 2324) is the same model
/// without the player gear — no cockpit interior, no `pirate_hook` — and is
/// measured in the finding as the rejected candidate.
pub const MISSION_PLAYER_AIRCRAFT_ROOT_NAME: &str = "player";

/// The stored slot of the airframe's intact-state subtree: `healthy`, child
/// of `geometry` (slot 48) under `player_pfighter`.
pub const MISSION_PLAYER_INTACT_NODE_SLOT: u32 = 149;

/// The authored name the pinned intact node stores, checked on every spawn.
pub const MISSION_PLAYER_INTACT_NODE_NAME: &str = "healthy";

/// The stored slot of the one propeller mesh drawn: `staticprop1` under
/// `dontmove` (slot 207), the same name-read development choice the playtest
/// made (#648) — which propeller state the original shows is unmeasured.
pub const MISSION_PLAYER_PROP_NODE_SLOT: u32 = 210;

/// The authored name the pinned propeller node stores.
pub const MISSION_PLAYER_PROP_NODE_NAME: &str = "staticprop1";

/// The viewer distance the player airframe's LOD band is selected at: the
/// playtest's own designed value — the original's LOD rule is unmeasured. It
/// chooses `nearest` (0–150 m), the highest-detail band this airframe
/// authors.
pub const MISSION_PLAYER_LOD_DISTANCE_M: f64 =
    crate::playtest_retail::PLAYTEST_AIRCRAFT_LOD_DISTANCE_M;

/// What the stage carries so the composition can draw the player's
/// airframe: the `zbd/planes.zbd` container and the texture archive its
/// materials resolve against, read through [`read_aircraft_sources`].
///
/// `None` on the synthetic acceptance stages, which have no installation to
/// read one from — their player body spawns without a visual, as it did
/// before this stage.
#[derive(Clone, Debug)]
pub struct PlayerAirframeSource {
    sources: AircraftSources,
}

impl PlayerAirframeSource {
    /// Reads the airframe container and the texture archive for
    /// `world_group` out of `install_root` — one production discovery pass,
    /// never a second parser and never a write inside the installation.
    ///
    /// # Errors
    ///
    /// [`MissionCompositionError::AirframeVisual`] when discovery, the
    /// container read or the texture archive refuses.
    pub fn read(install_root: &Path, world_group: &str) -> Result<Self, MissionCompositionError> {
        let sources = read_aircraft_sources(install_root, world_group).map_err(|error| {
            MissionCompositionError::AirframeVisual(format!(
                "the player airframe's sources refuse: {error}"
            ))
        })?;
        Ok(Self { sources })
    }
}

/// The player body's drawn airframe: one [`AircraftPartAsset`] per mesh
/// binding of the measured intact selection, the nose mapping that lands the
/// stored −Z nose on the body's forward, and the selection's own report.
///
/// Built once per composition in [`super::compose::add_composition`] and held
/// as a resource so the `AircraftSpawner` re-spawns the same parts on every
/// `R` restart; [`super::compose::teardown`] removes it.
#[derive(Resource)]
pub struct MissionPlayerVisual {
    parts: Vec<AircraftPartAsset>,
    rotation: Quat,
    propeller: Option<PropellerSpinSpec>,
    report: PlaytestAircraftReport,
}

impl MissionPlayerVisual {
    /// The drawn parts, in the graph's preorder.
    #[must_use]
    pub fn parts(&self) -> &[AircraftPartAsset] {
        &self.parts
    }

    /// The nose mapping applied once to each part's composed transform.
    #[must_use]
    pub const fn rotation(&self) -> Quat {
        self.rotation
    }

    /// The drawn propeller's measured spin spec, when the selection drew it.
    #[must_use]
    pub const fn propeller(&self) -> Option<&PropellerSpinSpec> {
        self.propeller.as_ref()
    }

    /// What the selection read: the pinned nodes, the chosen LOD band, the
    /// drawn parts and every binding it did not draw, with its reason.
    #[must_use]
    pub const fn report(&self) -> &PlaytestAircraftReport {
        &self.report
    }
}

/// Builds the player airframe's drawn visual through the production path:
/// [`build_aircraft_visual`] over the stage's `planes.zbd` container with
/// this module's measured pins, and [`nose_mapping`] over the measured
/// container-wide nose axis (#709).
///
/// # Errors
///
/// [`MissionCompositionError::AirframeVisual`] when the production build
/// refuses — a pinned slot holding the wrong name, a selection the LOD rule
/// refuses, or a drawn mesh that would not upload.
pub fn build_player_visual(
    app: &mut bevy::prelude::App,
    source: &PlayerAirframeSource,
) -> Result<MissionPlayerVisual, MissionCompositionError> {
    // The headless composition has no PBR plugin; the asset collections are
    // all the material upload needs — the same guard `playtest::retail::install`
    // runs before building the playtest's own airframe.
    if !app.world().contains_resource::<Assets<StandardMaterial>>() {
        app.init_asset::<StandardMaterial>();
    }
    if !app.world().contains_resource::<Assets<Image>>() {
        app.init_asset::<Image>();
    }
    // Only the `aircraft_*` fields are read by the build; the rest of the
    // documented playtest configuration passes through unchanged.
    let config = PlaytestConfig {
        aircraft_root_name: MISSION_PLAYER_AIRCRAFT_ROOT_NAME.to_owned(),
        aircraft_intact_node_slot: MISSION_PLAYER_INTACT_NODE_SLOT,
        aircraft_intact_node_name: MISSION_PLAYER_INTACT_NODE_NAME.to_owned(),
        aircraft_prop_node_slot: MISSION_PLAYER_PROP_NODE_SLOT,
        aircraft_prop_node_name: MISSION_PLAYER_PROP_NODE_NAME.to_owned(),
        aircraft_lod_distance_m: MISSION_PLAYER_LOD_DISTANCE_M,
        ..PlaytestConfig::documented()
    };
    let mut binder = TextureBinder::new(
        source.sources.textures(),
        config.textured,
        config.decal_offset,
    );
    let visual = build_aircraft_visual(app, source.sources.aircraft(), &mut binder, &config)
        .map_err(|error| {
            MissionCompositionError::AirframeVisual(format!(
                "the player airframe's visual refuses: {error}"
            ))
        })?;
    let _textures = binder.finish();
    let rotation = nose_mapping(STORED_AIRCRAFT_NOSE_AXIS).map_err(|error| {
        MissionCompositionError::AirframeVisual(format!(
            "the measured nose axis would not map: {error}"
        ))
    })?;
    Ok(MissionPlayerVisual {
        parts: visual.parts,
        rotation,
        propeller: visual.propeller,
        report: visual.report,
    })
}

/// Puts every drawn mesh of the player airframe under `body`: one child per
/// drawn binding at its composed transform (the measured nose mapping applied
/// once), one drawn grandchild per stored material group, and
/// [`PropellerSpin`] on the one drawn propeller — the same spawn
/// `playtest::scene::spawn_retail_parts` performs for the playtest's
/// airframe. The body flies `PlaytestOriginalFlight`, so the propeller spin
/// system reads its throttle and turns the disc with no mission-side wiring.
///
/// A stage that carries no [`MissionPlayerVisual`] — the synthetic
/// acceptance stages — spawns the body alone and returns.
pub fn spawn_player_visual(world: &mut World, body: Entity) {
    let Some(visual) = world.remove_resource::<MissionPlayerVisual>() else {
        return;
    };
    for part in visual.parts() {
        let base = part.oriented(visual.rotation());
        let entity = part.spawn(world, body, base);
        if let Some(spec) = visual.propeller()
            && spec.node_slot == part.node_slot
        {
            world
                .entity_mut(entity)
                .insert(PropellerSpin::from_hub(&spec.hub, base));
        }
    }
    world.insert_resource(visual);
}
