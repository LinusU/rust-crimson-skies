//! The original gauge-cluster HUD art (task #1726 `PARITY2-M01-HUD-ART`).
//!
//! The owner captures of the retail game show the HUD's dials as authored
//! meshes, not flat sprites: the player airframe's scene in `zbd/planes.zbd`
//! carries a `cockpit1 > gauges` subtree whose `Object3d` nodes bind the dial
//! faces, needles, damage-silhouette zones, indicator pips and glyph quads —
//! `docs/findings/2026-10-20-m01-hud-art-instruments.md` measured them.
//!
//! What this module does:
//!
//! * [`read_instrument_table`] reads each instrument's HUD-space translation
//!   from `ZBD/zrdr.zbd`'s `instruments.zrd` through the production
//!   `decode_zrd` — the measured placements, never a guess.
//! * [`build_hud_art`] walks the `player > player_pfighter > cockpit1 >
//!   gauges` subtree of the *same* [`PlaytestContainer`] the player visual
//!   reads, uploads every instrument node's stored mesh through
//!   [`crate::world::retail::stored_render_mesh`] +
//!   [`WorldMeshes::insert_render_mesh`] and binds materials through
//!   [`TextureBinder::parts`] — the production pipeline, not a parallel one.
//!   Each part's transform is `instruments.zrd` translation × HUD scale × the
//!   node's transform **relative to its instrument node**, so children keep
//!   their authored offsets.
//! * [`spawn_hud_art`] puts the parts on a dedicated [`RenderLayers`] layer
//!   under a second [`Camera3d`] (order 1, no clear) whose viewport is the
//!   4:3 safe area; [`fit_hud_viewport`] recomputes that viewport per frame.
//! * [`update_hud_art`] drives the live elements every frame from the same
//!   authorities [`super::cockpit_hud`] reads: speed/altitude needles, the
//!   compass tape, missile/gun pips, the ammo/type glyph quads and the
//!   per-zone damage material swap. The cockpit panel's dials
//!   (`super::cockpit_dials`, #1812) carry the same markers, so the one
//!   drive serves both clusters; the `pfhorizon` attitude ball it also
//!   drives only ever exists on the panel.
//! * [`fit_hud_viewport`] also switches the pass off in the pilot-eye views:
//!   there the dials are the panel meshes and the corner cluster must not
//!   draw over them (PARITY3-COCKPIT-VIEW.04).
//! * [`read_reticle_image`] decodes `impact_point` from `ZBD/rimage.zbd` through
//!   `cs_formats::texture::read_zbd_textures` + the production RGB565
//!   expansion; [`spawn_reticle`] draws it as a centred `ImageNode`.
//!
//! **Measured vs designed.** Placements, needle rates, units and the 4:3 fit
//! come from the finding's measured rows. The `instruments.zrd` units→HUD
//! scale (`DIAL_RADIUS_UNITS` of 0.9 per measured face radius), the altimeter
//! needle rates, the compass tape period, the pip fill rule and the damage
//! state→colour mapping are **designed** and recorded as such in the finding.
//! The `horiz` attitude
//! indicator is not listed in `instruments.zrd` and stays out of the HUD
//! pass (the cockpit dials draw it from its stored transform);
//! `lowalt_on`/`stallwarning_on`/`mgarrow`/`ggarrow` and the nitro
//! needles have no measured driver yet, so they keep their authored state
//! (the flags hidden, as authored) and each is reported in
//! [`HudArt::unknowns`].
//!
//! 16:9 rule: the HUD plane keeps its 4:3 proportions inside the largest
//! centred 4:3 rectangle of the window — the same `safe_area` rule the text
//! HUD uses; the rest of the window shows the world behind it.

use std::collections::BTreeMap;
use std::path::Path;

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{Camera, ClearColorConfig, PerspectiveProjection, Projection, Viewport};
use bevy::prelude::{
    App, Assets, Camera3d, ChildOf, Children, Component, Entity, Handle, Image, Mat4, Mesh, Mesh3d,
    MeshMaterial3d, Plugin, Quat, Query, Res, ResMut, Resource, StandardMaterial, Transform, UVec2,
    Update, Val, Vec3, Visibility, Window, With, Without, World, default,
};
use bevy::ui::widget::ImageNode;
use bevy::ui::{Node, PositionType, UiRect};
use bevy::window::PrimaryWindow;
use cs_content::scene::{SceneGraph, SceneNode};
use cs_content::stunts::{ZrdValue, decode_zrd};
use cs_formats::script_raw::discovery::discover_container;
use cs_formats::texture::read_zbd_textures;
use cs_sim::damage::{DamageNodeKey, PartState};
use cs_types::content::Resolved;
use cs_types::net::{ActorId, SessionId};

use super::cockpit_hud::safe_area;
use super::compose::MissionCompositionError;
use super::launch_cinematic::CinematicOwnedVisibility;
use super::player_visual::PlayerAirframeSource;
use super::weapons::{MissionOrdnanceSession, MissionWeapons};
use crate::airframe_damage::AirframeDamageRegion;
use crate::playtest::PlaytestState;
use crate::playtest_retail::{
    AircraftPartAsset, PlaytestContainer, aircraft_graph, playtest_adapter,
};
use crate::playtest_textures::{NeutralColor, PlaytestPart, TextureBinder};
use crate::scene::NodeVisualTransform;
use crate::ui::hud::{airframe_zones, ordnance_rows, weapon_gauge};
use crate::weapons::{SessionDamageResolver, WeaponSession};
use crate::world::meshes::WorldMeshes;
use crate::world::retail::{stored_presentation_unknowns, stored_render_mesh};

/// The `RenderLayers` layer the HUD pass draws on; the world camera draws the
/// default layer only, so the cluster never appears in the world and the
/// world never appears in the HUD pass.
pub const HUD_CAMERA_LAYER: usize = 1;

/// Half-height of the measured HUD instrument space at `|z| = 17.5` (4:3).
pub const HUD_HALF_HEIGHT: f32 = 7.5;
/// Half-width of the measured HUD instrument space.
pub const HUD_HALF_WIDTH: f32 = 10.0;
/// The depth almost every `instruments.zrd` placement shares (nitro sits at
/// −23.5, mostly below the safe area, as the original shows).
pub const HUD_PLANE_DEPTH: f32 = 17.5;
/// The 4:3 camera's vertical field of view that fits the measured space:
/// `2·atan(7.5/17.5)` radians ≈ 46.4° (the finding's fit; the fit itself is
/// designed).
pub const HUD_FOV: f32 = 0.810_731_4;
/// The cluster's aspect: the original's 4:3 instrument space.
pub const HUD_ASPECT: f32 = 4.0 / 3.0;

/// Dial-face radius in HUD units, fitted to the owner's 1024×768 captures:
/// the dials measure ~92 px across — 92/768·15 ≈ 1.8 units of diameter —
/// and the stack's centres land exactly on the `instruments.zrd` rows
/// (missilegauge −2.1, altimeter −4.05, damage −6.0). The stored face's own
/// radius measures ≈1.0, so the drawn scale comes out ≈0.9; the value itself
/// is **designed** (the original's radius is a capture fit, not a stored row).
const DIAL_RADIUS_UNITS: f32 = 0.9;

/// `hud_v2.zrd`'s measured `LIGHT_HUD_NORM` (14,196,8) — the neutral colour a
/// HUD mesh falls back to when its stored material names no texture.
const LIGHT_HUD_NORM: [f32; 4] = [14.0 / 255.0, 196.0 / 255.0, 8.0 / 255.0, 1.0];

/// Engine-measured speed needle rate, rad per m/s (VA `0x6076e8`,
/// `data/owner-static/engine-facts.toml`): one revolution per 500 mph,
/// negative = the stored winding.
const SPEED_RAD_PER_MPS: f32 = -0.028_110_17;
/// Engine-measured altitude unit (engine-facts.toml).
const M_TO_FT: f64 = 3.280_839_9;
/// Altimeter needle rates — same `needle.tif` family as the speed needle;
/// `−2π` per 1 000 ft / per 10 000 ft is **designed** (the finding records the
/// per-needle scale as unmeasured).
const ALT_HUNDREDS_RAD_PER_FT: f32 = -std::f32::consts::TAU / 1_000.0;
/// See [`ALT_HUNDREDS_RAD_PER_FT`].
const ALT_THOUSANDS_RAD_PER_FT: f32 = -std::f32::consts::TAU / 10_000.0;

/// The instruments `instruments.zrd` places, in record order.
const INSTRUMENTS: [&str; 7] = [
    "speedometer",
    "altimeter",
    "comp",
    "damageindicator",
    "missilegauge",
    "gungauge",
    "nitrogauge",
];

// Measured node slots of the player tree's live elements (the finding's
// probe dump; the player's `cockpit1` subtree is slots 49–114).
const SLOT_SPEED: u32 = 75;
const SLOT_STALL: u32 = 76;
const SLOT_ALT_HUNDREDS: u32 = 69;
const SLOT_ALT_THOUSANDS: u32 = 70;
const SLOT_LOWALT: u32 = 71;
const SLOT_COMPASS: u32 = 65;
const SLOT_NITRO_BOOST: u32 = 110;
const SLOT_NITRO_CHARGE: u32 = 112;
const SLOT_MGARROW: u32 = 92;
const SLOT_GGARROW: u32 = 106;
const SLOT_SPEED_FACE: u32 = 78;
const SLOT_MG_AMMO: u32 = 87;
const SLOT_MG_TYPE: u32 = 93;
const SLOT_GG_AMMO: u32 = 101;
const SLOT_GG_TYPE: u32 = 107;
/// `pfhorizon` (slot 61): the attitude ball inside `horiz` — cockpit-view
/// only, so [`build_hud_art`] never reaches it.
pub(crate) const SLOT_PFHORIZON: u32 = 61;

/// One instrument's measured `instruments.zrd` translation in HUD units.
#[derive(Clone, Debug, PartialEq)]
pub struct InstrumentPlacement {
    /// The `gauges` child the placement belongs to.
    pub name: String,
    /// `[x, y, z]` in the HUD instrument space (camera at origin, −Z forward).
    pub position: [f32; 3],
}

/// The `instruments.zrd` read: the measured placements, or why none can be
/// claimed — the same refusal-vs-measured-absence rule [`super::parts`]
/// carries.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InstrumentTable {
    /// Every placement the member decoded, in record order.
    pub placements: Vec<InstrumentPlacement>,
    /// The logical key of the container the member decoded from.
    pub carrier: Option<String>,
    /// Why no table could be read, verbatim.
    pub refusal: Option<String>,
}

/// Reads `ZBD/zrdr.zbd`'s `instruments.zrd` member the way
/// [`super::parts::read_scope_turrets`] reads `ai.zrd`: the manifest names
/// the host path, `discover_container` finds the member and `decode_zrd`
/// parses it. A member that will not decode is a refusal, never an empty
/// table; a carrier that holds no such member is a measured absence.
#[must_use]
pub fn read_instrument_table(install_root: &Path) -> InstrumentTable {
    let refused = |reason: String| InstrumentTable {
        refusal: Some(reason),
        ..InstrumentTable::default()
    };
    let found = match cs_assets::install::discover(install_root) {
        Ok(found) => found,
        Err(error) => {
            return refused(format!("the installation cannot be discovered: {error}"));
        }
    };
    let archive = "zbd/zrdr.zbd";
    let Some(record) = found
        .manifest
        .files
        .iter()
        .find(|record| record.relative_spelling.logical_key() == archive)
    else {
        return refused(format!("{archive} is not in the manifest"));
    };
    let host = install_root.join(record.relative_spelling.as_str());
    let bytes = match std::fs::read(&host) {
        Ok(bytes) => bytes,
        Err(error) => return refused(format!("cannot read {}: {error}", host.display())),
    };
    let discovery = discover_container(archive, &record.relative_spelling, &bytes);
    let Some(program) = discovery.programs().iter().find(|program| {
        program
            .locator()
            .member()
            .is_some_and(|name| name.eq_ignore_ascii_case("instruments.zrd"))
    }) else {
        return InstrumentTable {
            carrier: Some(archive.to_owned()),
            ..InstrumentTable::default()
        };
    };
    let decoded = match decode_zrd(program.bytes()) {
        Ok(decoded) => decoded,
        Err(error) => {
            return refused(format!("{archive}'s instruments.zrd refuses: {error}"));
        }
    };
    let mut placements = Vec::new();
    collect_placements(&decoded, &mut placements);
    InstrumentTable {
        placements,
        carrier: Some(archive.to_owned()),
        refusal: None,
    }
}

/// Walks the decoded member and pulls every `name → (x, y, z)` record for a
/// named instrument. `comp` stores a second triple `21, 0, 0` the finding
/// records as unmeasured — only the first triple is taken.
fn collect_placements(node: &ZrdValue, out: &mut Vec<InstrumentPlacement>) {
    let Some(children) = node.as_list() else {
        return;
    };
    let number = |value: &ZrdValue| -> Option<f32> {
        match value {
            ZrdValue::Int(v) => Some(*v as f32),
            ZrdValue::Float(v) => Some(*v),
            _ => None,
        }
    };
    for (index, child) in children.iter().enumerate() {
        if let Some(name) = child.as_text()
            && INSTRUMENTS
                .iter()
                .any(|known| known.eq_ignore_ascii_case(name))
            && !out.iter().any(|p: &InstrumentPlacement| p.name == name)
        {
            let triple: Vec<f32> = children[index + 1..]
                .iter()
                .filter_map(number)
                .take(3)
                .collect();
            if let [x, y, z] = triple.as_slice() {
                out.push(InstrumentPlacement {
                    name: name.to_owned(),
                    position: [*x, *y, *z],
                });
            }
        }
        collect_placements(child, out);
    }
}

/// What [`build_hud_art`] produced — one [`AircraftPartAsset`] per mesh node
/// of the placed instruments, plus the measured placement table it used and
/// the decoded glyph/zone-state images the live drive swaps between.
#[derive(Clone, Debug)]
pub struct HudArt {
    /// The drawn parts: every mesh-bearing node under each placed instrument,
    /// transformed into HUD space.
    pub parts: Vec<AircraftPartAsset>,
    /// The measured placements the parts were laid out with.
    pub placements: Vec<InstrumentPlacement>,
    /// The authored→HUD scale the parts were re-rooted at (`DIAL_RADIUS_UNITS`
    /// per measured face radius).
    pub scale: f32,
    /// Texture name → engine image, preloaded for the glyph and zone-state
    /// swaps (`zero`–`nine`, `A`–`Z`, `space`, and the yellow/orange/red
    /// `*hilite` and `*_hatchptrn` gauge frames).
    pub images: BTreeMap<String, Handle<Image>>,
    /// Nodes the table does not place and roles without a measured driver —
    /// reported, never silent.
    pub unknowns: Vec<String>,
}

/// The texture names the glyph quads can show: `zero`…`nine`, `A`…`Z`,
/// `space` — the archive's own stored names (`one.tif` &c.).
fn glyph_names() -> Vec<String> {
    let mut names: Vec<String> = (b'A'..=b'Z')
        .map(|c| String::from_utf8(vec![c]).expect("ascii"))
        .collect();
    names.extend(
        [
            "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "space",
        ]
        .iter()
        .map(|name| (*name).to_owned()),
    );
    names
}

/// The texture names the damage zones recolour to (finding §Damage zones).
const ZONE_TEXTURES: [&str; 6] = [
    "yellowhilite",
    "orangehilite",
    "redhilite",
    "yel_hatchptrn",
    "orng_hatchptrn",
    "red_hatchptrn",
];

/// The measured gauge ladder: a part's `<part>_damage_green|yellow|red`
/// `injure_anims` thresholds, identical for the nose, tail and both wings
/// (`docs/findings/2026-10-23-parity-m01-player-damage-gauge-mapping.md`).
/// Each level's animation holds one `OBJECT_CYCLE_TEXTURE ... RESET (n)`,
/// `n` = 1, 2, 3. The handler (engine image `0x4eabd0` → `0x564410`, static)
/// selects frame `n` of the node's 0-based texture cycle
/// `[green, yellow, orange, red]`, so the frame for a remaining-health
/// fraction is the number of thresholds it has fallen to or below. The unit
/// (remaining vs damage fraction) and the boundary comparison are inferred
/// from the descending order and stay recorded as unknown.
pub const GAUGE_THRESHOLDS: [f64; 3] = [0.72, 0.46, 0.2];

/// The gauge cycle frame (`0` green … `3` red) of a zone: the measured
/// ladder over `remaining_fraction` (remaining integrity / declared pool),
/// with `Destroyed` pinned to the last frame; an unresolved pool draws the
/// intact frame (the ladder says nothing for it).
#[must_use]
pub fn gauge_frame(remaining_fraction: Option<f64>, state: PartState) -> u8 {
    match state {
        PartState::Intact | PartState::Unknown => 0,
        PartState::Destroyed => 3,
        // Damaged but above the first level (or with no resolved pool) is
        // still the intact frame: the ladder has no entry before 0.72.
        PartState::Damaged => remaining_fraction.map_or(0, |fraction| {
            GAUGE_THRESHOLDS.iter().filter(|t| fraction <= **t).count() as u8
        }),
    }
}

/// The texture name of one cycle frame for a zone piece role.
#[must_use]
pub fn zone_texture_name(role: ZonePieceRole, frame: u8) -> &'static str {
    match (role, frame) {
        (ZonePieceRole::Hilite, 0) => "greenhilite",
        (ZonePieceRole::Hilite, 1) => "yellowhilite",
        (ZonePieceRole::Hilite, 2) => "orangehilite",
        (ZonePieceRole::Hilite, _) => "redhilite",
        (ZonePieceRole::Hatch, 0) => "grn_hatchptrn",
        (ZonePieceRole::Hatch, 1) => "yel_hatchptrn",
        (ZonePieceRole::Hatch, 2) => "orng_hatchptrn",
        (ZonePieceRole::Hatch, _) => "red_hatchptrn",
    }
}

/// Builds the HUD instrument parts from the same aircraft container the
/// player visual draws.
///
/// Only `player > player_pfighter > cockpit1 > gauges` children named in
/// `table` are placed — `horiz` and the furniture nodes keep out of the HUD
/// pass by name, not by accident.
///
/// # Errors
///
/// [`MissionCompositionError::AirframeVisual`] when the scene graph, an
/// instrument node or the mesh pipeline refuses.
pub fn build_hud_art(
    app: &mut App,
    source: &PlayerAirframeSource,
    table: &InstrumentTable,
) -> Result<HudArt, MissionCompositionError> {
    let refuse = |reason: String| MissionCompositionError::AirframeVisual(reason);
    let aircraft = source.sources().aircraft();
    let adapter = playtest_adapter()
        .map_err(|error| refuse(format!("the scene adapter refuses: {error}")))?;
    let planes = aircraft_graph(aircraft, &adapter)
        .map_err(|error| refuse(format!("planes.zbd's graph refuses: {error}")))?;
    let gauges = find_path(
        &planes,
        &["player", "player_pfighter", "cockpit1", "gauges"],
    )
    .ok_or_else(|| refuse("the player tree has no cockpit1 > gauges path".to_owned()))?;

    let mut unknowns: Vec<String> = Vec::new();
    for child in gauges.children() {
        let Some(node) = planes.node(child) else {
            continue;
        };
        let name = node.name().trim_end_matches('\0');
        if !INSTRUMENTS.contains(&name) {
            unknowns.push(format!(
                "gauges child {name} is not in instruments.zrd: kept out of the HUD pass"
            ));
        }
    }
    unknowns.push(
        "lowalt_on/stallwarning_on/mgarrow/ggarrow/nitro needles keep their authored \
         state: no measured driver (the finding records thresholds and roles as \
         unmeasured)"
            .to_owned(),
    );

    let mut meshes = WorldMeshes::new();
    let mut binder = TextureBinder::new(source.sources().textures(), true, true);
    let mut parts = Vec::new();
    let mut placements = Vec::new();

    // The authored→HUD scale: measured once from the speedometer face's own
    // extent so the dials land at the finding's radius-≈0.9 fit whatever the
    // authored units are. When the face cannot be measured the parts draw at
    // authored scale and the record says so.
    let scale = face_radius(&planes, aircraft, gauges)
        .map(|radius| DIAL_RADIUS_UNITS / radius)
        .unwrap_or_else(|| {
            unknowns.push(
                "the speedometer face's authored radius could not be measured; \
                 the instruments draw at authored scale"
                    .to_owned(),
            );
            1.0
        });

    for placement in &table.placements {
        let Some(instrument) = find_child(&planes, gauges, &placement.name) else {
            unknowns.push(format!(
                "instruments.zrd places '{}' but the gauges subtree has no such node",
                placement.name
            ));
            continue;
        };
        placements.push(placement.clone());
        let w_instr = world_transform(instrument)?;
        let place = Mat4::from_translation(Vec3::from(placement.position))
            * Mat4::from_scale(Vec3::splat(scale));
        for node in planes.subtree(instrument.id()) {
            let Some((handle, pieces)) =
                upload_gauge_part(app, aircraft, &mut binder, &mut meshes, node, &mut unknowns)?
            else {
                continue;
            };
            let world = world_transform(node)?;
            let placed = place * w_instr.inverse() * world;
            let (p_scale, rotation, translation) = placed.to_scale_rotation_translation();
            parts.push(AircraftPartAsset {
                node_slot: node.index(),
                node_name: node.name().trim_end_matches('\0').to_owned(),
                mesh: handle,
                pieces,
                local: Transform {
                    translation,
                    rotation,
                    scale: p_scale,
                },
            });
        }
    }
    if parts.is_empty() {
        return Err(refuse(
            "no instrument mesh built: the HUD art has nothing to draw".to_owned(),
        ));
    }

    let (images, texture_unknowns) = preload_drive_images(app, source.sources().textures());
    unknowns.extend(texture_unknowns);
    let _ = binder.finish();
    Ok(HudArt {
        parts,
        placements,
        scale,
        images,
        unknowns,
    })
}

/// What [`upload_gauge_part`] yields for one drawable node: the whole-mesh
/// handle and its bound material pieces.
type GaugePartDraw = (Handle<Mesh>, Vec<PlaytestPart>);

/// Uploads one mesh-binding node's stored mesh and binds its materials —
/// the shared production block [`build_hud_art`] and
/// [`super::cockpit_dials::build_cockpit_dials`] both run. Returns `None`
/// (with a recorded reason in `unknowns`) when the node binds no mesh or the
/// binding cannot be drawn.
///
/// # Errors
///
/// [`MissionCompositionError::AirframeVisual`] when an upload that reported
/// success left no registered mesh — the same hard refusal the HUD build
/// makes.
pub(crate) fn upload_gauge_part(
    app: &mut App,
    aircraft: &PlaytestContainer,
    binder: &mut TextureBinder,
    meshes: &mut WorldMeshes,
    node: &SceneNode,
    unknowns: &mut Vec<String>,
) -> Result<Option<GaugePartDraw>, MissionCompositionError> {
    let Some(binding) = node.mesh() else {
        return Ok(None);
    };
    let id = match &binding.mesh {
        Resolved::Known(known) => known.value.clone(),
        Resolved::Unknown { reason, .. } => {
            unknowns.push(format!(
                "instrument node {} mesh {}: binding unknown: {reason}",
                node.index(),
                binding.index
            ));
            return Ok(None);
        }
    };
    let Some(slot) = aircraft.meshes().get(binding.index) else {
        unknowns.push(format!(
            "instrument node {}: mesh {} is not in the container",
            node.index(),
            binding.index
        ));
        return Ok(None);
    };
    let render = match stored_render_mesh(slot) {
        Ok(render) => render,
        Err(error) => {
            unknowns.push(format!(
                "instrument node {}: stored mesh {} refused: {error}",
                node.index(),
                binding.index
            ));
            return Ok(None);
        }
    };
    let unknown_list = stored_presentation_unknowns(&render);
    if let Err(reason) = meshes.insert_render_mesh(id.clone(), &render, &unknown_list) {
        unknowns.push(format!(
            "instrument node {}: the mesh would not upload: {reason}",
            node.index()
        ));
        return Ok(None);
    }
    let Some(uploaded) = meshes.get(&id) else {
        return Err(MissionCompositionError::AirframeVisual(format!(
            "the instrument mesh {} is not registered after upload",
            id.key()
        )));
    };
    let handle: Handle<Mesh> = app
        .world_mut()
        .resource_mut::<Assets<Mesh>>()
        .add(uploaded.mesh().clone());
    let pieces = binder.parts(
        app,
        aircraft,
        uploaded,
        NeutralColor(LIGHT_HUD_NORM),
        binding.index,
    );
    Ok(Some((handle, pieces)))
}

/// Preloads the glyph alphabet and the zone recolours the live drive swaps
/// between through `archive` (the same texture archive the parts bound):
/// names that refuse are reported and the quads keep their authored
/// material. Shared by [`build_hud_art`] and
/// [`super::cockpit_dials::build_cockpit_dials`].
pub(crate) fn preload_drive_images(
    app: &mut App,
    archive: &crate::playtest_textures::PlaytestTextureArchive,
) -> (BTreeMap<String, Handle<Image>>, Vec<String>) {
    let mut images = BTreeMap::new();
    let mut unknowns = Vec::new();
    for name in glyph_names()
        .into_iter()
        .chain(ZONE_TEXTURES.iter().map(|name| (*name).to_owned()))
    {
        match archive.fetch(&format!("{name}.tif")) {
            Ok((_, decoded)) => {
                let (image, _, _) = crate::playtest_textures::to_bevy_image(&decoded);
                let handle = app.world_mut().resource_mut::<Assets<Image>>().add(image);
                images.insert(name, handle);
            }
            Err((_, reason)) => unknowns.push(format!(
                "texture {name}.tif refused ({reason}); affected quads keep their authored material"
            )),
        }
    }
    (images, unknowns)
}

/// One node of the path under the graph's `player` root, by name.
pub(crate) fn find_child<'g>(
    graph: &'g SceneGraph,
    parent: &SceneNode,
    name: &str,
) -> Option<&'g SceneNode> {
    parent
        .children()
        .iter()
        .filter_map(|child| graph.node(child))
        .find(|node| {
            node.name()
                .trim_end_matches('\0')
                .eq_ignore_ascii_case(name)
        })
}

/// Resolves `root("player") > …path` to the terminal node.
pub(crate) fn find_path<'g>(graph: &'g SceneGraph, path: &[&str]) -> Option<&'g SceneNode> {
    let mut node = graph.root(path.first()?).ok()?;
    for name in &path[1..] {
        node = find_child(graph, node, name)?;
    }
    Some(node)
}

/// The node's composed world transform as a `Mat4` (canonical → engine).
pub(crate) fn world_transform(node: &SceneNode) -> Result<Mat4, MissionCompositionError> {
    Ok(NodeVisualTransform::from_canonical(node.visual_transform())
        .map_err(|error| {
            MissionCompositionError::AirframeVisual(format!(
                "instrument node {} would not convert: {error}",
                node.index()
            ))
        })?
        .global()
        .to_matrix())
}

/// Half-width of the speedometer's `g784` face (slot 78, mesh #9) in
/// speedometer-space — the authored dial radius the HUD scale maps onto
/// [`DIAL_RADIUS_UNITS`].
fn face_radius(
    graph: &SceneGraph,
    aircraft: &PlaytestContainer,
    gauges: &SceneNode,
) -> Option<f32> {
    let speedometer = find_child(graph, gauges, "speedometer")?;
    let w_instr = world_transform(speedometer).ok()?;
    let node = graph
        .subtree(speedometer.id())
        .into_iter()
        .find(|node| node.index() == SLOT_SPEED_FACE)?;
    let binding = node.mesh()?;
    let rel = w_instr.inverse() * world_transform(node).ok()?;
    let render = stored_render_mesh(aircraft.meshes().get(binding.index)?).ok()?;
    let mut max = 0.0f32;
    for vertex in render.vertices() {
        let p = rel.transform_point3(Vec3::from(vertex.position));
        max = max.max(p.x.abs()).max(p.y.abs());
    }
    (max > 0.0).then_some(max)
}

// ---------------------------------------------------------------------------
// Spawned-entity markers: what one entity of the drawn cluster animates as.
// ---------------------------------------------------------------------------

/// The root of the drawn cluster — also the entity the cinematic hides.
#[derive(Component, Clone, Copy, Debug)]
pub struct HudArtRoot;

/// The dedicated HUD camera.
#[derive(Component, Clone, Copy, Debug)]
pub struct HudArtCamera;

/// A needle entity: rotation driven on top of the authored base.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct HudNeedle {
    /// The needle's measured role.
    pub role: NeedleRole,
    /// The transform's authored rotation, kept so the drive composes on it.
    pub base: Quat,
}

/// A needle's role.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NeedleRole {
    /// `speed` (slot 75): engine-measured `−0.02811017 rad/(m/s)`.
    Speed,
    /// `hundreds` (slot 69): designed `−2π` per 1 000 ft.
    AltHundreds,
    /// `thousands` (slot 70): designed `−2π` per 10 000 ft.
    AltThousands,
}

/// The `compass` tape (slot 65): heading slides it sideways.
#[derive(Component, Clone, Copy, Debug)]
pub struct HudCompassTape {
    /// The authored x of the tape in HUD space.
    pub base_x: f32,
    /// Units of tape per turn of heading (designed: one measured tape width
    /// per 360°).
    pub period: f32,
}

/// A `_on` flag quad (`lowalt_on`, `stallwarning_on`): drawn exactly while its
/// (unmeasured) flag is on — no authority publishes one yet, so the authored
/// state is hidden.
#[derive(Component, Clone, Copy, Debug)]
pub struct HudFlag;

/// The `pfhorizon` attitude ball (slot 61, under `horiz`): the cockpit dials
/// tag it; [`update_hud_art`] counter-rotates it by the telemetry's pitch and
/// roll (the designed attitude-indicator rule — the original's ball mapping,
/// axis and range are unmeasured).
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct HudAttitudeBall {
    /// The transform's authored rotation, kept so the drive composes on it.
    pub base: Quat,
}

/// One damage-zone overlay (`rightwingdamage` … `nosedamage`, slots 79–82).
#[derive(Component, Clone, Copy, Debug)]
pub struct HudZone {
    /// The measured airframe region the zone quad answers to.
    pub region: AirframeDamageRegion,
}

/// Which material role one piece of a zone quad plays (the zone meshes bind a
/// `*hilite` tint and a `*_hatchptrn` pattern together).
#[derive(Component, Clone, Debug)]
pub struct HudZonePiece {
    /// The zone it belongs to.
    pub region: AirframeDamageRegion,
    /// `*hilite` tint or `*_hatchptrn` pattern.
    pub role: ZonePieceRole,
    /// The material the piece spawned with — the green `*hilite`/`*_hatchptrn`
    /// pair the intact state draws and a damaged zone returns to.
    pub authored: Handle<StandardMaterial>,
}

/// The two material roles a zone mesh binds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZonePieceRole {
    /// `greenhilite`/`yellowhilite`/`redhilite` — the translucent tint.
    Hilite,
    /// `grn_hatchptrn`/`yel_hatchptrn`/`red_hatchptrn` — the opaque pattern.
    Hatch,
}

/// One indicator pip (`mgindicator0..7`, `ggindicator0..3`).
#[derive(Component, Clone, Copy, Debug)]
pub struct HudPip {
    /// Missile or gun gauge.
    pub gauge: PipGauge,
    /// The pip's index inside its gauge (the node's own suffix).
    pub index: usize,
}

/// Which gauge a pip or glyph row answers to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PipGauge {
    /// `missilegauge` — 8 rocket pips + `4char_ammo` + `6char_type`.
    Missile,
    /// `gungauge` — 4 gun pips + `4char_ammo` + `6char_type`.
    Gun,
}

/// One glyph quad of a `4char_ammo` or `6char_type` row.
#[derive(Component, Clone, Copy, Debug)]
pub struct HudGlyphCell {
    /// The gauge the row belongs to.
    pub gauge: PipGauge,
    /// `4char_ammo` = the count, `6char_type` = the type name.
    pub kind: GlyphRow,
    /// The cell's position in the row, left to right.
    pub cell: usize,
}

/// Which row a glyph cell belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlyphRow {
    /// `4char_ammo` — four digit quads.
    Ammo,
    /// `6char_type` — six letter quads.
    Type,
}

/// The `mgarrow`/`ggarrow` ordnance-type pointers: a measured mesh but an
/// unmeasured driver — kept at the authored pose and reported.
#[derive(Component, Clone, Copy, Debug)]
pub struct HudArrow;

/// The nitro needle quads (`nitro_boost`/`nitro_charge`): a measured mesh but
/// an unmeasured driver — kept at the authored pose and reported.
#[derive(Component, Clone, Copy, Debug)]
pub struct HudNitroNeedle;

/// Resource: the build record plus the variant-material caches the live drive
/// fills (a cell or zone piece only ever swaps to an original texture on the
/// authored material's own parameters).
#[derive(Resource, Default)]
pub struct MissionHudArt {
    /// The measured placements the cluster was laid out with.
    pub placements: Vec<InstrumentPlacement>,
    /// The measured authored→HUD scale.
    pub scale: f32,
    /// The build's recorded unknowns.
    pub unknowns: Vec<String>,
    /// Texture name → decoded engine image (the glyph alphabet and the zone
    /// recolours), preloaded at build.
    pub images: BTreeMap<String, Handle<Image>>,
    /// `char → material` cache built from the authored cell material.
    pub glyphs: BTreeMap<char, Handle<StandardMaterial>>,
    /// `(role, colour) → material` for the zone recolour.
    pub zone_materials: BTreeMap<(u8, u8), Handle<StandardMaterial>>,
    /// The reticle texture's name once decoded, so evidence can cite it.
    pub reticle: Option<String>,
    /// The decoded `impact_point` image the reticle `ImageNode` draws.
    pub reticle_image: Option<Handle<Image>>,
}

/// Spawns the built cluster under a fresh HUD pass: a root entity carrying
/// [`CinematicOwnedVisibility`], a `Camera3d` on [`HUD_CAMERA_LAYER`] with the
/// measured 4:3 fit, and every part re-rooted at its `instruments.zrd`
/// placement. Returns the root entity.
pub fn spawn_hud_art(world: &mut World, art: &HudArt) -> Entity {
    let layer = RenderLayers::layer(HUD_CAMERA_LAYER);
    let root = world
        .spawn((
            Transform::IDENTITY,
            Visibility::Inherited,
            HudArtRoot,
            CinematicOwnedVisibility,
        ))
        .id();
    world.spawn((
        Camera3d::default(),
        Camera {
            order: 1,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        Projection::Perspective(PerspectiveProjection {
            fov: HUD_FOV,
            aspect_ratio: HUD_ASPECT,
            near: 1.0,
            far: 100.0,
            ..PerspectiveProjection::default()
        }),
        layer.clone(),
        HudArtCamera,
        // Not the scene's eye: the world-follow systems
        // (`render::horizon`, `render::weather`, `render::precipitation`,
        // `world_draw`'s horizon anchor, `actor_census`) keep reading the one
        // camera that carries [`crate::playtest::PlaytestCameraMarker`].
        crate::render::OverlayCamera,
        ChildOf(root),
    ));
    for part in &art.parts {
        let entity = part.spawn(world, root, part.local);
        insert_layer(world, entity, &layer);
        tag_part(world, entity, part, &part.local);
    }
    root
}

/// Puts `layer` on `entity` and every descendant that draws a mesh — the HUD
/// pass sees exactly the cluster and the world pass never does.
fn insert_layer(world: &mut World, entity: Entity, layer: &RenderLayers) {
    if world.get::<Mesh3d>(entity).is_some() {
        world.entity_mut(entity).insert(layer.clone());
    }
    let children: Vec<Entity> = world
        .get::<Children>(entity)
        .map(|children| children.iter().copied().collect())
        .unwrap_or_default();
    for child in children {
        insert_layer(world, child, layer);
    }
}

/// Attaches the live-element markers [`update_hud_art`] reads, keyed by the
/// measured node slot the part draws. `transform` is the transform the part
/// entity actually carries — the HUD placement for [`spawn_hud_art`], the
/// oriented stored transform for [`super::cockpit_dials::spawn_cockpit_dials`]
/// — so the bases the drive composes on match what is spawned.
pub(crate) fn tag_part(
    world: &mut World,
    entity: Entity,
    part: &AircraftPartAsset,
    transform: &Transform,
) {
    match part.node_slot {
        SLOT_SPEED => {
            world.entity_mut(entity).insert(HudNeedle {
                role: NeedleRole::Speed,
                base: transform.rotation,
            });
        }
        SLOT_ALT_HUNDREDS => {
            world.entity_mut(entity).insert(HudNeedle {
                role: NeedleRole::AltHundreds,
                base: transform.rotation,
            });
        }
        SLOT_ALT_THOUSANDS => {
            world.entity_mut(entity).insert(HudNeedle {
                role: NeedleRole::AltThousands,
                base: transform.rotation,
            });
        }
        SLOT_PFHORIZON => {
            world.entity_mut(entity).insert(HudAttitudeBall {
                base: transform.rotation,
            });
        }
        SLOT_COMPASS => {
            let period = compass_period(world, part, transform);
            world.entity_mut(entity).insert(HudCompassTape {
                base_x: transform.translation.x,
                period,
            });
        }
        SLOT_LOWALT | SLOT_STALL => {
            world
                .entity_mut(entity)
                .insert((HudFlag, Visibility::Hidden));
        }
        SLOT_NITRO_BOOST | SLOT_NITRO_CHARGE => {
            world.entity_mut(entity).insert(HudNitroNeedle);
        }
        SLOT_MGARROW | SLOT_GGARROW => {
            world.entity_mut(entity).insert(HudArrow);
        }
        _ => {
            if let Some(region) = damage_region_of(part) {
                world.entity_mut(entity).insert(HudZone { region });
                tag_zone_pieces(world, entity, part, region);
            } else if let Some(pip) = pip_of(part) {
                world.entity_mut(entity).insert(pip);
            } else if let Some((gauge, kind)) = glyph_row_of(part) {
                tag_glyph_cells(world, entity, part, gauge, kind);
            }
        }
    }
}

/// The damage region a zone part's node name denotes.
fn damage_region_of(part: &AircraftPartAsset) -> Option<AirframeDamageRegion> {
    AirframeDamageRegion::from_node_name(&part.node_name)
}

/// The pip marker a part is, by its `mgindicatorN`/`ggindicatorN` node name.
fn pip_of(part: &AircraftPartAsset) -> Option<HudPip> {
    let name = part.node_name.as_str();
    for (prefix, gauge) in [
        ("mgindicator", PipGauge::Missile),
        ("ggindicator", PipGauge::Gun),
    ] {
        if let Some(rest) = name.strip_prefix(prefix)
            && let Ok(index) = rest.parse::<usize>()
        {
            return Some(HudPip { gauge, index });
        }
    }
    None
}

/// `(gauge, row)` of a `4char_ammo`/`6char_type` part — distinguished by the
/// measured slot (`missilegauge` 87/93, `gungauge` 101/107).
fn glyph_row_of(part: &AircraftPartAsset) -> Option<(PipGauge, GlyphRow)> {
    Some(match part.node_slot {
        SLOT_MG_AMMO => (PipGauge::Missile, GlyphRow::Ammo),
        SLOT_MG_TYPE => (PipGauge::Missile, GlyphRow::Type),
        SLOT_GG_AMMO => (PipGauge::Gun, GlyphRow::Ammo),
        SLOT_GG_TYPE => (PipGauge::Gun, GlyphRow::Type),
        _ => return None,
    })
}

/// Tags each piece of a zone quad with the material role its bound texture
/// names (`*_hatchptrn` → Hatch, `*hilite` → Hilite).
fn tag_zone_pieces(
    world: &mut World,
    entity: Entity,
    part: &AircraftPartAsset,
    region: AirframeDamageRegion,
) {
    let children: Vec<Entity> = world
        .get::<Children>(entity)
        .map(|children| children.iter().copied().collect())
        .unwrap_or_default();
    for (index, child) in children.iter().enumerate() {
        let Some(piece) = part.pieces.get(index) else {
            continue;
        };
        let role = if piece
            .image
            .as_deref()
            .is_some_and(|name| name.contains("hatchptrn"))
        {
            ZonePieceRole::Hatch
        } else {
            ZonePieceRole::Hilite
        };
        let mut child = world.entity_mut(*child);
        child.insert(HudZonePiece {
            region,
            role,
            authored: piece.material.clone(),
        });
        // The zone quads are authored flat at z = 0, coplanar with the
        // silhouette face (`g843`), so the face hid the wing-shaped fan and
        // only the rim sliver showed (#2031). Lifting each piece clear of the
        // face is designed, as for the glyph cells; the original's draw
        // order is unmeasured.
        if let Some(mut transform) = child.get_mut::<Transform>() {
            transform.translation.z += GLYPH_LIFT;
        }
    }
}

/// How far each glyph or damage-zone quad stands in front of its dial face, in the gauge's
/// own units: the face mesh reaches z = 0.024 (measured from the stored mesh).
const GLYPH_LIFT: f32 = 0.05;

/// Tags each piece of a glyph row with its left-to-right cell index.
fn tag_glyph_cells(
    world: &mut World,
    entity: Entity,
    part: &AircraftPartAsset,
    gauge: PipGauge,
    kind: GlyphRow,
) {
    let children: Vec<Entity> = world
        .get::<Children>(entity)
        .map(|children| children.iter().copied().collect())
        .unwrap_or_default();
    for (index, child) in children.iter().enumerate() {
        if index >= part.pieces.len() {
            break;
        }
        let mut child = world.entity_mut(*child);
        child.insert(HudGlyphCell {
            gauge,
            kind,
            cell: index,
        });
        // The glyph quads are authored flat at z = 0 while the dial face's
        // mesh bulges to +0.024 toward the camera, so the face hid every
        // digit and letter (#1797). Lifting each cell clear of the face
        // is designed; the original's draw order is unmeasured.
        if let Some(mut transform) = child.get_mut::<Transform>() {
            transform.translation.z += GLYPH_LIFT;
        }
    }
}

/// The tape's designed units-per-turn: its own measured width is one 360°
/// cycle (the strip carries the compass lettering around it). `transform` is
/// the transform the part entity actually carries — its scale multiplies the
/// mesh's own width.
pub(crate) fn compass_period(
    world: &mut World,
    part: &AircraftPartAsset,
    transform: &Transform,
) -> f32 {
    let Some(piece) = part.pieces.first() else {
        return 1.0;
    };
    let Some(mesh) = world.resource::<Assets<Mesh>>().get(&piece.mesh) else {
        return 1.0;
    };
    let positions = match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
        Some(bevy::mesh::VertexAttributeValues::Float32x3(values)) => values,
        _ => return 1.0,
    };
    let width = positions
        .iter()
        .map(|p| p[0])
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), x| {
            (lo.min(x), hi.max(x))
        });
    let width = (width.1 - width.0) * transform.scale.x.abs();
    if width > 0.0 { width } else { 1.0 }
}

// ---------------------------------------------------------------------------
// The reticle: `ZBD/rimage.zbd`'s `impact_point` as a centred `ImageNode`.
// ---------------------------------------------------------------------------

/// The `ZBD/rimage.zbd` texture the centre reticle draws: the 32×32 ring with
/// cross ticks the original owner frames show at screen centre (`rcircle`, the
/// hand-drawn circle, is not the gunsight). Identity by visual match; the
/// original consumer was not traced in the engine image.
pub const RETICLE_TEXTURE: &str = "impact_point";

/// Decodes `impact_point` (32×32, alpha) from `ZBD/rimage.zbd` through the
/// production `read_zbd_textures` + RGB565 expansion path.
///
/// # Errors
///
/// A `String` reason when the installation, the container member or the
/// stored texture refuses — the caller records the reticle absent rather
/// than drawing a substitute.
pub fn read_reticle_image(
    install_root: &Path,
    images: &mut Assets<Image>,
) -> Result<Handle<Image>, String> {
    let found = cs_assets::install::discover(install_root)
        .map_err(|error| format!("the installation cannot be discovered: {error}"))?;
    let record = found
        .manifest
        .files
        .iter()
        .find(|record| record.relative_spelling.logical_key() == "zbd/rimage.zbd")
        .ok_or_else(|| "zbd/rimage.zbd is not in the manifest".to_owned())?;
    let host = install_root.join(record.relative_spelling.as_str());
    let bytes =
        std::fs::read(&host).map_err(|error| format!("cannot read {}: {error}", host.display()))?;
    let mut budget = cs_formats::AllocationBudget::with_defaults("hud-art-reticle");
    let package = read_zbd_textures("zbd/rimage.zbd", &bytes, &mut budget)
        .map_err(|error| format!("zbd/rimage.zbd refuses: {error}"))?;
    let texture = package
        .named(RETICLE_TEXTURE)
        .next()
        .ok_or_else(|| format!("zbd/rimage.zbd holds no texture named {RETICLE_TEXTURE}"))?;
    let decoded = texture
        .decode(&mut budget)
        .map_err(|error| format!("{RETICLE_TEXTURE} refuses to decode: {error}"))?;
    let decoded = crate::hd_textures::pack()
        .and_then(|pack| pack.ui_decoded("rimage/impact_point", &decoded))
        .unwrap_or(decoded);
    let (image, _, _) = crate::playtest_textures::to_bevy_image(&decoded);
    Ok(images.add(image))
}

/// The centred reticle node — the `impact_point` texture at its stored 32×32
/// extent.
#[derive(Component, Clone, Copy, Debug)]
pub struct HudReticle;

/// Spawns the reticle `ImageNode` under `parent` (the safe-area root).
pub fn spawn_reticle(parent: &mut bevy::prelude::ChildSpawnerCommands, image: Handle<Image>) {
    parent.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Percent(50.0),
            top: Val::Percent(50.0),
            width: Val::Px(32.0),
            height: Val::Px(32.0),
            margin: UiRect {
                left: Val::Px(-16.0),
                top: Val::Px(-16.0),
                ..default()
            },
            ..default()
        },
        ImageNode::new(image),
        HudReticle,
        CinematicOwnedVisibility,
    ));
}

// ---------------------------------------------------------------------------
// The live drive.
// ---------------------------------------------------------------------------

/// Recomputes the HUD camera's viewport so it matches the 4:3 safe area —
/// the 16:9 rule: keep the original proportions inside the centred 4:3 rect.
/// The launch cinematic holds the cluster back until it hands over — the
/// same hold `fit_safe_area` makes for the text layer (#1663), here by
/// switching the pass itself off so nothing lands in the letterbox bars.
/// The Flight Check holds it too: its black cover is a UI node, and a UI
/// node cannot cover what a second `Camera3d` draws over the window — a run
/// with no cinematic installed (the no-teleport smoke, a mission without a
/// launch record) would otherwise draw the cluster over the check screen.
/// The pilot-eye views hold it as well (PARITY3-COCKPIT-VIEW.04): there the
/// dials are the cockpit panel's own meshes, so the chase-view corner
/// cluster stands down with the text layer.
pub fn fit_hud_viewport(
    windows: Query<&Window, With<PrimaryWindow>>,
    phase: Option<Res<super::launch_cinematic::LaunchCinematic>>,
    check: Option<Res<super::flight_check::FlightCheckPhase>>,
    view: Option<Res<super::view_cycle::MissionViewMode>>,
    crash: Option<Res<crate::playtest::fatal_contact::CrashSequence>>,
    debrief: Option<Res<super::debrief_screen::DebriefScreen>>,
    mut cameras: Query<&mut Camera, With<HudArtCamera>>,
) {
    let held = phase.is_some_and(|phase| phase.is_playing())
        || check.is_some_and(|phase| phase.is_pending())
        || view.is_some_and(|view| view.at_pilot_eye())
        || crash.is_some()
        || debrief.is_some();
    let rect = windows.single().ok().and_then(|window| {
        let (w, h) = safe_area(window.width(), window.height());
        if w <= 0.0 || h <= 0.0 {
            return None;
        }
        let scale = window.resolution.scale_factor();
        let (w, h) = (w * scale, h * scale);
        Some((
            ((window.physical_width() as f32 - w) / 2.0).max(0.0) as u32,
            ((window.physical_height() as f32 - h) / 2.0).max(0.0) as u32,
            w as u32,
            h as u32,
        ))
    });
    for mut camera in &mut cameras {
        camera.is_active = !held;
        if let Some((x, y, w, h)) = rect {
            camera.viewport = Some(Viewport {
                physical_position: UVec2::new(x, y),
                physical_size: UVec2::new(w, h),
                ..default()
            });
        }
    }
}

/// The attitude-ball side of [`update_hud_art`]'s parameter set, factored
/// out for readability; disjoint from the needle and tape queries so the
/// `&mut Transform` accesses do not conflict.
type AttitudeBalls<'w, 's> = Query<
    'w,
    's,
    (&'static mut Transform, &'static HudAttitudeBall),
    (Without<HudNeedle>, Without<HudCompassTape>),
>;

/// Per-frame drive of the drawn cluster: needles from telemetry, pips and
/// glyph counts from the weapon/ordnance authorities, zone materials from the
/// damage resolver — the same session reads [`super::cockpit_hud`] makes.
#[allow(clippy::too_many_arguments)]
pub fn update_hud_art(
    state: Res<PlaytestState>,
    mission: Option<Res<MissionWeapons>>,
    weapons: Option<Res<WeaponSession>>,
    ordnance: Option<Res<MissionOrdnanceSession>>,
    damage: Option<Res<SessionDamageResolver>>,
    mut hud: ResMut<MissionHudArt>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut needles: Query<(&mut Transform, &HudNeedle), Without<HudCompassTape>>,
    mut tapes: Query<(&mut Transform, &HudCompassTape), Without<HudNeedle>>,
    mut balls: AttitudeBalls,
    mut pips: Query<(&mut Visibility, &HudPip)>,
    mut zone_pieces: Query<
        (&mut MeshMaterial3d<StandardMaterial>, &HudZonePiece),
        Without<HudGlyphCell>,
    >,
    mut glyph_cells: Query<
        (&mut MeshMaterial3d<StandardMaterial>, &HudGlyphCell),
        Without<HudZonePiece>,
    >,
) {
    let t = state.telemetry;
    let speed_rad = t.speed_m_s * SPEED_RAD_PER_MPS;
    let altitude_ft = (t.altitude_m as f64 * M_TO_FT) as f32;
    for (mut transform, needle) in &mut needles {
        let angle = match needle.role {
            NeedleRole::Speed => speed_rad,
            NeedleRole::AltHundreds => altitude_ft * ALT_HUNDREDS_RAD_PER_FT,
            NeedleRole::AltThousands => altitude_ft * ALT_THOUSANDS_RAD_PER_FT,
        };
        transform.rotation = needle.base * Quat::from_rotation_z(angle);
    }
    for (mut transform, tape) in &mut tapes {
        // Designed direction: turning right scrolls the tape left.
        transform.translation.x = tape.base_x - (t.heading_deg / 360.0) * tape.period;
    }
    for (mut transform, ball) in &mut balls {
        // Designed attitude rule: the ball is gyro-fixed to the world, so it
        // counter-rotates the airframe's pitch and roll inside the panel
        // frame — `Rz(-roll) * Rx(-pitch)` is the inverse of the X-then-Z
        // attitude composition. The original's mapping and range are
        // unmeasured (the finding records it).
        transform.rotation = ball.base
            * Quat::from_rotation_z(-t.roll_deg.to_radians())
            * Quat::from_rotation_x(-t.pitch_deg.to_radians());
    }

    let Some(mission) = mission else {
        return;
    };
    let session: SessionId = mission.session();
    let actor: ActorId = mission.player();

    let gauge = weapon_gauge(weapons.as_deref(), session, actor)
        .ok()
        .flatten();
    let rows = ordnance_rows(ordnance.as_deref().map(|o| &o.0), session, actor)
        .ok()
        .flatten();
    let rockets: u64 = rows
        .as_deref()
        .map_or(0, |rows| rows.iter().map(|row| row.remaining_units).sum());
    let rocket_capacity: u64 = rows
        .as_deref()
        .map_or(0, |rows| rows.iter().map(|row| row.capacity_units).sum());
    let rounds = gauge
        .as_ref()
        .map_or(0, super::cockpit_hud::current_gun_rounds);
    let gun_capacity: u64 = gauge.as_ref().map_or(0, |gauge| {
        gauge
            .guns
            .iter()
            .find(|gun| gun.selected)
            .map_or(0, |gun| gun.rounds.max(1))
    });
    let ordnance_name = rows
        .as_deref()
        .and_then(|rows| rows.first())
        .map(|_| super::cockpit_hud::ROCKETS_LABEL.to_owned())
        .unwrap_or_default();
    let ammo_name = gauge
        .as_ref()
        .and_then(|gauge| gauge.ammunition.first())
        .map(|_| super::cockpit_hud::GUNS_LABEL.to_owned())
        .unwrap_or_default();

    for (mut visibility, pip) in &mut pips {
        let (remaining, capacity, count) = match pip.gauge {
            PipGauge::Missile => (rockets, rocket_capacity, 8u64),
            PipGauge::Gun => (rounds, gun_capacity, 4),
        };
        // Designed rule: pips fill in index order over the capacity — pip i
        // is lit while remaining/capacity > i/count, compared without the
        // division a capacity under `count` would make zero.
        let lit = capacity > 0
            && remaining.saturating_mul(count) > (pip.index as u64).saturating_mul(capacity);
        let wanted = if lit {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != wanted {
            *visibility = wanted;
        }
    }

    let ammo_text = format!("{rockets:0>4}");
    let rounds_text = format!("{rounds:0>4}");
    for (mut material, cell) in &mut glyph_cells {
        let text = match (cell.gauge, cell.kind) {
            (PipGauge::Missile, GlyphRow::Ammo) => ammo_text.as_str(),
            (PipGauge::Missile, GlyphRow::Type) => ordnance_name.as_str(),
            (PipGauge::Gun, GlyphRow::Ammo) => rounds_text.as_str(),
            (PipGauge::Gun, GlyphRow::Type) => ammo_name.as_str(),
        };
        let glyph = text.chars().nth(cell.cell).unwrap_or(' ');
        if let Some(handle) = glyph_material(&mut hud, &mut materials, material.0.clone(), glyph)
            && material.0 != handle
        {
            material.0 = handle;
        }
    }

    // Damage zones: the four measured regions' own states drive a material
    // swap per zone piece — never a tint of the silhouette.
    let zones = airframe_zones(
        damage.as_deref().map(SessionDamageResolver::resolver),
        session,
        actor,
    )
    .ok()
    .flatten()
    .unwrap_or_default();
    let resolver = damage.as_deref().map(SessionDamageResolver::resolver);
    for (mut material, piece) in &mut zone_pieces {
        let key = DamageNodeKey::new(piece.region.node_name()).ok();
        let state = zones
            .iter()
            .find(|zone| key.as_ref() == Some(&zone.node))
            .map_or(PartState::Unknown, |zone| zone.state);
        let fraction = resolver
            .zip(key.as_ref())
            .and_then(|(resolver, key)| remaining_fraction(resolver, actor, key));
        let frame = gauge_frame(fraction, state);
        if let Some(handle) =
            zone_material(&mut hud, &mut materials, &piece.authored, piece.role, frame)
            && material.0 != handle
        {
            material.0 = handle;
        }
    }
}

/// `char` → the digit/letter material on the authored cell's parameters.
/// `chars` outside the glyph set draw the `space` glyph; a texture the
/// archive never decoded leaves the quad on its authored material.
fn glyph_material(
    hud: &mut MissionHudArt,
    materials: &mut Assets<StandardMaterial>,
    base: Handle<StandardMaterial>,
    glyph: char,
) -> Option<Handle<StandardMaterial>> {
    let key = match glyph {
        '0'..='9' | 'A'..='Z' => glyph,
        'a'..='z' => glyph.to_ascii_uppercase(),
        _ => ' ',
    };
    if let Some(handle) = hud.glyphs.get(&key) {
        return Some(handle.clone());
    }
    let name = match key {
        '0'..='9' => [
            "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine",
        ][key as usize - '0' as usize]
            .to_owned(),
        ' ' => "space".to_owned(),
        letter => letter.to_string(),
    };
    let image = hud.images.get(&name)?.clone();
    let mut variant = materials.get(&base)?.clone();
    variant.base_color_texture = Some(image);
    let handle = materials.add(variant);
    hud.glyphs.insert(key, handle.clone());
    Some(handle)
}

/// A node's remaining integrity over its declared pool; `None` when either
/// is unresolved or the pool is empty.
fn remaining_fraction(
    resolver: &cs_sim::damage::DamageResolver,
    actor: ActorId,
    key: &DamageNodeKey,
) -> Option<f64> {
    let Resolved::Known(pool) = resolver.graph(&actor)?.node(key)?.integrity() else {
        return None;
    };
    let pool = pool.value;
    let remaining = resolver.remaining_integrity(&actor, key)?;
    (pool > 0.0).then(|| (remaining / pool).clamp(0.0, 1.0))
}

/// `(role, gauge frame)` → the zone material: the cycle frame's
/// `*hilite`/`*_hatchptrn` image on the piece's authored material. Frame 0
/// is the authored green, so a zone returning to it restores `base` rather
/// than keeping the last variant.
fn zone_material(
    hud: &mut MissionHudArt,
    materials: &mut Assets<StandardMaterial>,
    base: &Handle<StandardMaterial>,
    role: ZonePieceRole,
    frame: u8,
) -> Option<Handle<StandardMaterial>> {
    if frame == 0 {
        return Some(base.clone());
    }
    let key = (
        match role {
            ZonePieceRole::Hilite => 0u8,
            ZonePieceRole::Hatch => 1,
        },
        frame,
    );
    if let Some(handle) = hud.zone_materials.get(&key) {
        return Some(handle.clone());
    }
    let image = hud.images.get(zone_texture_name(role, frame))?.clone();
    let mut variant = materials.get(base)?.clone();
    variant.base_color_texture = Some(image);
    let handle = materials.add(variant);
    hud.zone_materials.insert(key, handle.clone());
    Some(handle)
}

// -- the scheduled pieces ---------------------------------------------------

/// Installs the HUD-art drive systems (the cluster itself is spawned by the
/// composition, which owns the install root).
#[derive(Clone, Copy, Debug, Default)]
pub struct MissionHudArtPlugin;

impl Plugin for MissionHudArtPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MissionHudArt>()
            .add_systems(Update, (fit_hud_viewport, update_hud_art));
    }
}
