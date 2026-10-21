//! The lens flare of the sun in a composed mission (PARITY4-LENS-FLARE-B,
//! #1859).
//!
//! The placement and fade rule is [`crate::render::lens_flare`]; this module
//! finds the lights, loads the four `lflare` textures and draws the quads.
//!
//! * **Sources.** [`LensFlareSources::read`] walks the world container's mesh
//!   lights and keeps those with the flare word (`unk60 != 0`), the stored
//!   record the engine's record filler gates on (`docs/findings/2026-10-22-
//!   parity4-lens-flare-a-sun-source-and-blend.md` §1). A flagged light whose
//!   node hangs under the `horizon` subtree (M01's `sun`, mesh 491) is drawn
//!   at its bulb position `extra[0]` carried by the node's placed transform.
//!   The two `lens_flash` nodes carry the flag too, but nothing recovered says
//!   where they sit or what raises them, so they are *held back* and reported
//!   in [`LensFlareSources::held_back`], never drawn at a guess.
//! * **Art.** `lflare1..4` come from the world group's own texture archive.
//! * **Draw.** Four UI image nodes per source, under every other UI layer,
//!   placed by [`flare_quads`] from the light's projection through the scene
//!   camera. The blend is **designed**: 50 percent alpha ([`FLARE_ALPHA`], the
//!   sprite routine's opacity argument). Whether the textures are additive
//!   (their descriptor flag) is not traced, so it stays unknown.
//!
//! Not drawn, and unknown: the terrain ray cast the engine makes per flare
//! (the sun is above any terrain in M01) and the depth sorting among
//! translucent draws (the quads are a screen overlay, so the cockpit and the
//! world never cover them).

use bevy::asset::{Assets, Handle};
use bevy::camera::Camera;
use bevy::image::Image;
use bevy::prelude::IntoScheduleConfigs;
use bevy::prelude::{
    App, Camera3d, Color, Commands, Component, GlobalTransform, GlobalZIndex, Node, Plugin,
    PositionType, PostUpdate, Query, Res, Resource, Startup, Val, Vec3, Visibility, With, Without,
};
use bevy::transform::TransformSystems;
use bevy::ui::widget::ImageNode;
use cs_formats::gamez::{GameZMeshes, GameZNodes};

use super::world_draw::{HorizonSubtree, node_slot_of};
use crate::playtest_retail::PlaytestContainer;
use crate::playtest_textures::{PlaytestTextureArchive, to_bevy_image};
use crate::render::OverlayCamera;
use crate::render::lens_flare::{flare_fade, flare_quads};
use crate::world::contacts::WorldObjectBinding;

/// The four texture names, in slot order (`LensFlareTexture 0..3`).
pub const FLARE_TEXTURES: [&str; 4] = ["lflare1", "lflare2", "lflare3", "lflare4"];

/// The opacity of a flare quad: the `0.5` the engine passes to its sprite
/// routine. That it is an alpha blend rather than an additive one is the
/// unproven part (finding §4).
pub const FLARE_ALPHA: f32 = 0.5;

/// One drawn flare source: a stored mesh light on a node.
#[derive(Clone, Debug, PartialEq)]
pub struct FlareSource {
    /// The stored node slot that owns the mesh.
    pub slot: u32,
    /// The node's name.
    pub name: String,
    /// The bulb position `extra[0]` in the node's own space.
    pub bulb: Vec3,
    /// The flare fade minimum (`unk68`).
    pub fade_min: f32,
    /// The flare fade maximum (`unk72`).
    pub fade_max: f32,
}

/// The flare-flagged lights of a world container.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LensFlareSources {
    /// The lights drawn.
    pub drawn: Vec<FlareSource>,
    /// Flagged lights not drawn, with the node name and the reason.
    pub held_back: Vec<(String, &'static str)>,
}

impl LensFlareSources {
    /// Reads the flagged mesh lights of `nodes` / `meshes`; a light is drawn
    /// when its node is in the `horizon` subtree.
    #[must_use]
    pub fn read(
        nodes: &GameZNodes,
        meshes: &GameZMeshes,
        horizon: Option<&HorizonSubtree>,
    ) -> Self {
        let mut out = Self::default();
        for node in &nodes.nodes {
            let Ok(index) = u32::try_from(node.mesh_index()) else {
                continue;
            };
            let Some(mesh) = meshes.present().find(|mesh| mesh.index == index) else {
                continue;
            };
            for light in mesh.lights.iter().filter(|l| l.header.unk60 != 0) {
                let in_horizon = horizon.is_some_and(|h| h.contains(node.index));
                match (in_horizon, light.extra.first()) {
                    (true, Some(&[x, y, z])) => out.drawn.push(FlareSource {
                        slot: node.index,
                        name: node.name.clone(),
                        bulb: Vec3::new(x, y, z),
                        fade_min: light.header.unk68,
                        fade_max: light.header.unk72,
                    }),
                    (true, None) => out
                        .held_back
                        .push((node.name.clone(), "the light stores no bulb position")),
                    (false, _) => out.held_back.push((
                        node.name.clone(),
                        "placement and trigger of a flare outside the horizon are not recovered",
                    )),
                }
            }
        }
        out
    }

    /// [`Self::read`] over a playtest container.
    #[must_use]
    pub fn from_container(container: &PlaytestContainer, horizon: Option<&HorizonSubtree>) -> Self {
        Self::read(container.nodes(), container.meshes(), horizon)
    }
}

/// Decodes `lflare1..4` out of the world group's texture archive.
///
/// # Errors
///
/// A reason when any of the four does not resolve: the flare is then not drawn
/// (the engine enables flares only when all four slots are loaded).
pub fn read_flare_images(
    archive: &PlaytestTextureArchive,
    images: &mut Assets<Image>,
) -> Result<[Handle<Image>; 4], String> {
    let mut handles = Vec::with_capacity(4);
    for name in FLARE_TEXTURES {
        let (_, decoded) = archive
            .fetch(name)
            .map_err(|(reason, code)| format!("{name}: {code}: {reason}"))?;
        handles.push(images.add(to_bevy_image(&decoded).0));
    }
    handles
        .try_into()
        .map_err(|_| "four textures expected".to_owned())
}

/// The mission's flare: sources and art.
#[derive(Resource, Clone, Debug)]
pub struct MissionLensFlare {
    sources: LensFlareSources,
    images: [Handle<Image>; 4],
}

impl MissionLensFlare {
    /// A flare of `sources` drawn with `images`.
    #[must_use]
    pub const fn new(sources: LensFlareSources, images: [Handle<Image>; 4]) -> Self {
        Self { sources, images }
    }

    /// The sources.
    #[must_use]
    pub const fn sources(&self) -> &LensFlareSources {
        &self.sources
    }
}

/// One quad's UI node: which source and which of the four textures.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlareQuadNode {
    /// Index into [`LensFlareSources::drawn`].
    pub source: usize,
    /// 0 = `lflare1` .. 3 = `lflare4`.
    pub texture: usize,
}

/// Installs the flare consumer; does nothing without a [`MissionLensFlare`].
#[derive(Clone, Copy, Debug, Default)]
pub struct MissionLensFlarePlugin;

impl Plugin for MissionLensFlarePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_flare_nodes)
            .add_systems(PostUpdate, place_flares.after(TransformSystems::Propagate));
    }
}

fn spawn_flare_nodes(mut commands: Commands, flare: Option<Res<MissionLensFlare>>) {
    let Some(flare) = flare else {
        return;
    };
    for source in 0..flare.sources.drawn.len() {
        for (texture, image) in flare.images.iter().enumerate() {
            commands.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    ..Default::default()
                },
                ImageNode::new(image.clone()).with_color(Color::srgba(1.0, 1.0, 1.0, FLARE_ALPHA)),
                GlobalZIndex(-100),
                Visibility::Hidden,
                FlareQuadNode { source, texture },
            ));
        }
    }
}

/// The scene camera, not the HUD pass.
type SceneCamera = (With<Camera3d>, Without<OverlayCamera>);

fn place_flares(
    flare: Option<Res<MissionLensFlare>>,
    cameras: Query<(&Camera, &GlobalTransform), SceneCamera>,
    objects: Query<(&WorldObjectBinding, &GlobalTransform)>,
    mut quads: Query<(&FlareQuadNode, &mut Node, &mut Visibility)>,
) {
    let Some(flare) = flare else {
        return;
    };
    let camera = cameras.iter().find(|(camera, _)| camera.is_active);
    for (_, _, mut visibility) in &mut quads {
        *visibility = Visibility::Hidden;
    }
    let Some((camera, camera_transform)) = camera else {
        return;
    };
    let Some(viewport) = camera.logical_viewport_size() else {
        return;
    };
    let to_view = camera_transform.to_matrix().inverse();
    for (index, source) in flare.sources.drawn.iter().enumerate() {
        let Some((_, placed)) = objects
            .iter()
            .find(|(binding, _)| node_slot_of(binding.object().as_str()) == Some(source.slot))
        else {
            continue;
        };
        let world = placed.transform_point(source.bulb);
        let depth = -to_view.transform_point3(world).z;
        let Some(fade) = flare_fade(depth, source.fade_min, source.fade_max) else {
            continue;
        };
        let Ok(at) = camera.world_to_viewport(camera_transform, world) else {
            continue;
        };
        // The engine drops a light outside the viewport.
        if at.x < 0.0 || at.y < 0.0 || at.x > viewport.x || at.y > viewport.y {
            continue;
        }
        for placed in flare_quads([viewport.x, viewport.y], [at.x, at.y], fade) {
            for (quad, mut node, mut visibility) in &mut quads {
                if quad.source == index && quad.texture == placed.texture {
                    let half = placed.half_extent;
                    node.left = Val::Px(placed.centre[0] - half);
                    node.top = Val::Px(placed.centre[1] - half);
                    node.width = Val::Px(half * 2.0);
                    node.height = Val::Px(half * 2.0);
                    *visibility = Visibility::Inherited;
                }
            }
        }
    }
}
