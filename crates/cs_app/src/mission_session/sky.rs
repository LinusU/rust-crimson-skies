//! A **designed** horizon cue for the mission composition
//! (M01-PLAY-WORLD-VISIBLE, Rally #1433).
//!
//! While the original sky is unlanded (F19 owns it), a mission window that
//! shows only terrain over a flat clear colour leaves the horizon ambiguous
//! where the terrain thins. This module draws a large unlit dome around the
//! chase camera with a vertical **gradient** — a light band at the horizon,
//! deepening toward the zenith and darkening below it — so up, down and the
//! horizon read at a glance.
//!
//! Everything here is **designed**, and labelled so:
//! [`DESIGNED_SKY_CLAIM`] is the claim id the colours are filed under, the
//! values are this stage's own, and [`DesignedSky`] carries the claim into a
//! run's report. Nothing about it is a reading of the original's sky texture,
//! its atmosphere or its horizon; it is a temporary orientation cue that the
//! real sky replaces.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, Mesh, PrimitiveTopology};
use bevy::prelude::{
    Added, App, Assets, ChildOf, ClearColor, Color, Commands, Component, Mesh3d, MeshMaterial3d,
    Plugin, Query, Res, ResMut, Resource, StandardMaterial, Transform, Update, With,
};
use cs_types::evidence::ClaimId;

use crate::playtest::PlaytestCameraMarker;

/// The claim every colour of the cue is filed under.
pub const DESIGNED_SKY_CLAIM: &str = "m01-play.designed-sky-horizon-cue";

/// The sky's colour at the zenith: the deep end of the gradient, and the
/// window's clear colour where the dome is not drawn.
pub const SKY_ZENITH_COLOR: Color = Color::srgb(0.24, 0.44, 0.78);

/// The sky's colour at the horizon: the light band the gradient reaches at
/// the camera's own height.
pub const SKY_HORIZON_COLOR: Color = Color::srgb(0.80, 0.86, 0.93);

/// The colour below the horizon: the dark end of the gradient, so "down"
/// reads without any terrain.
pub const SKY_NEUTRAL_COLOR: Color = Color::srgb(0.30, 0.33, 0.35);

/// The dome's radius, metres. It is parented to the chase camera, and the
/// playtest camera's far plane is 40 km, so the dome always surrounds the
/// eye without being clipped.
pub const SKY_RADIUS_M: f32 = 20_000.0;

/// Longitudes and latitudes of the dome mesh: a coarse UV sphere is enough
/// for a smooth vertical gradient.
const SKY_SECTORS: u32 = 48;
const SKY_STACKS: u32 = 24;

/// The drawn horizon dome's marker: what the evidence probe hides to measure
/// the world's own pixels, and what [`teardown`](super::teardown) could name.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct DesignedSkyDome;

/// The designed cue as a value: the claim it is filed under and the colours
/// it draws. Held as a resource so a run can report what it is (and label it
/// designed), never as a claim about the original.
#[derive(Resource, Clone, Debug)]
pub struct DesignedSky {
    claim: ClaimId,
    zenith: Color,
    horizon: Color,
    below: Color,
    radius_m: f32,
}

impl DesignedSky {
    /// The cue this build declares, or the refusal of its claim id.
    ///
    /// # Errors
    ///
    /// The claim-id grammar's own reason when [`DESIGNED_SKY_CLAIM`] is not a
    /// valid claim id (it is a constant, so this is a compile-time-shaped
    /// constant failing its own grammar, refused rather than unwrapped).
    pub fn declared() -> Result<Self, String> {
        Ok(Self {
            claim: ClaimId::new(DESIGNED_SKY_CLAIM).map_err(|error| error.to_string())?,
            zenith: SKY_ZENITH_COLOR,
            horizon: SKY_HORIZON_COLOR,
            below: SKY_NEUTRAL_COLOR,
            radius_m: SKY_RADIUS_M,
        })
    }

    /// The claim the cue is filed under.
    #[must_use]
    pub const fn claim(&self) -> &ClaimId {
        &self.claim
    }

    /// The zenith colour.
    #[must_use]
    pub const fn zenith(&self) -> Color {
        self.zenith
    }

    /// The horizon colour.
    #[must_use]
    pub const fn horizon(&self) -> Color {
        self.horizon
    }

    /// The below-horizon colour.
    #[must_use]
    pub const fn below(&self) -> Color {
        self.below
    }

    /// The dome's radius, metres.
    #[must_use]
    pub const fn radius_m(&self) -> f32 {
        self.radius_m
    }
}

/// Installs the designed cue: the zenith clear colour and the dome, attached
/// to the chase camera the frame it appears. Added only to the windowed
/// composition ([`super::compose::add_composition`] with visuals), the same
/// way the playtest's own visual plugin is.
#[derive(Clone, Copy, Debug, Default)]
pub struct DesignedSkyPlugin;

impl Plugin for DesignedSkyPlugin {
    fn build(&self, app: &mut App) {
        let Ok(sky) = DesignedSky::declared() else {
            // The claim constant failing its own grammar is a build fault, not
            // a run state: refuse the cue by installing nothing rather than
            // drawing an unlabelled sky.
            return;
        };
        app.insert_resource(ClearColor(sky.zenith()));
        app.insert_resource(sky);
        app.add_systems(Update, attach_designed_sky);
    }
}

/// Spawns the dome under the chase camera once. The camera entity is the one
/// `playtest::setup_scene` spawns; the dome is its child, so it follows the
/// eye exactly and the horizon sits at the eye's own height.
fn attach_designed_sky(
    mut commands: Commands,
    sky: Option<Res<DesignedSky>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    cameras: Query<bevy::prelude::Entity, Added<PlaytestCameraMarker>>,
    domes: Query<(), With<DesignedSkyDome>>,
) {
    let Some(sky) = sky else {
        return;
    };
    for camera in &cameras {
        if !domes.is_empty() {
            continue;
        }
        let mesh = meshes.add(designed_sky_dome_mesh(sky.radius_m()));
        let material = materials.add(StandardMaterial {
            base_color: Color::WHITE,
            unlit: true,
            // The eye is inside the dome, so its inward faces are the ones
            // drawn: the material must not cull them.
            cull_mode: None,
            ..bevy::prelude::default()
        });
        commands.spawn((
            DesignedSkyDome,
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::IDENTITY,
            ChildOf(camera),
        ));
    }
}

/// The dome mesh: a `+Y`-up UV sphere of `radius` with a per-vertex colour
/// gradient — [`SKY_ZENITH_COLOR`] at the top pole, [`SKY_HORIZON_COLOR`] at
/// the equator, [`SKY_NEUTRAL_COLOR`] at the bottom.
///
/// Written out rather than taken from `Sphere`'s own mesh because Bevy's UV
/// sphere is `+Z`-poled and carries no vertex colours; the gradient is the
/// whole point of the cue.
#[must_use]
pub fn designed_sky_dome_mesh(radius: f32) -> Mesh {
    let row = SKY_SECTORS + 1;
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity((row * (SKY_STACKS + 1)) as usize);
    let mut normals: Vec<[f32; 3]> = Vec::with_capacity((row * (SKY_STACKS + 1)) as usize);
    let mut colours: Vec<[f32; 4]> = Vec::with_capacity((row * (SKY_STACKS + 1)) as usize);
    let mut indices: Vec<u32> = Vec::new();
    for stack in 0..=SKY_STACKS {
        let v = stack as f32 / SKY_STACKS as f32;
        let theta = std::f32::consts::PI * v;
        let (sin_theta, cos_theta) = theta.sin_cos();
        let up = cos_theta;
        let ring = sin_theta;
        for sector in 0..=SKY_SECTORS {
            let u = sector as f32 / SKY_SECTORS as f32;
            let phi = std::f32::consts::TAU * u;
            let (sin_phi, cos_phi) = phi.sin_cos();
            let direction = [ring * cos_phi, up, ring * sin_phi];
            positions.push([
                direction[0] * radius,
                direction[1] * radius,
                direction[2] * radius,
            ]);
            normals.push(direction);
            colours.push(sky_gradient(up));
        }
    }
    for stack in 0..SKY_STACKS {
        for sector in 0..SKY_SECTORS {
            let top_left = stack * row + sector;
            let bottom_left = top_left + row;
            indices.extend_from_slice(&[
                top_left,
                bottom_left,
                top_left + 1,
                top_left + 1,
                bottom_left,
                bottom_left + 1,
            ]);
        }
    }
    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colours);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// The gradient at unit height `up` (`+1` zenith, `0` horizon, `-1` nadir),
/// as the **linear** RGBA the vertex-colour attribute carries.
fn sky_gradient(up: f32) -> [f32; 4] {
    let (from, to, t) = if up >= 0.0 {
        (SKY_HORIZON_COLOR, SKY_ZENITH_COLOR, up)
    } else {
        (SKY_HORIZON_COLOR, SKY_NEUTRAL_COLOR, -up)
    };
    let (from, to) = (from.to_linear(), to.to_linear());
    [
        from.red + (to.red - from.red) * t,
        from.green + (to.green - from.green) * t,
        from.blue + (to.blue - from.blue) * t,
        from.alpha + (to.alpha - from.alpha) * t,
    ]
}

#[cfg(test)]
mod tests {
    use bevy::mesh::VertexAttributeValues;

    use super::*;

    /// **The cue is a gradient, not a flat colour.** The dome's vertex colours
    /// must carry the declared zenith/horizon/below values at the three poles,
    /// in the linear space the vertex-colour attribute is read in — a dome
    /// that drew a single colour would leave the horizon unreadable, which is
    /// the defect this cue exists to repair.
    #[test]
    fn accept_m01_play_world_the_designed_sky_dome_carries_a_vertical_gradient() {
        let mesh = designed_sky_dome_mesh(SKY_RADIUS_M);
        let colours = match mesh.attribute(Mesh::ATTRIBUTE_COLOR) {
            Some(VertexAttributeValues::Float32x4(values)) => values.clone(),
            other => panic!("the dome must carry f32x4 vertex colours, got {other:?}"),
        };
        assert_eq!(
            colours.len(),
            ((SKY_SECTORS + 1) * (SKY_STACKS + 1)) as usize,
            "every dome vertex carries one colour"
        );
        let zenith = SKY_ZENITH_COLOR.to_linear();
        let horizon = SKY_HORIZON_COLOR.to_linear();
        let below = SKY_NEUTRAL_COLOR.to_linear();
        let close = |left: [f32; 4], right: [f32; 4]| {
            left.iter()
                .zip(right.iter())
                .all(|(left, right)| (left - right).abs() < 1.0e-6)
        };
        let top = colours.first().copied().expect("the top pole exists");
        let bottom = colours.last().copied().expect("the bottom pole exists");
        assert!(
            close(top, [zenith.red, zenith.green, zenith.blue, zenith.alpha]),
            "the top pole draws the zenith colour, got {top:?}"
        );
        assert!(
            close(bottom, [below.red, below.green, below.blue, below.alpha]),
            "the bottom pole draws the below-horizon colour, got {bottom:?}"
        );
        assert!(
            colours.iter().any(|colour| close(
                *colour,
                [horizon.red, horizon.green, horizon.blue, horizon.alpha]
            )),
            "the equator draws the horizon colour"
        );
        // The gradient is monotone from horizon to zenith on the upper half.
        assert!(
            top[2] < horizon.blue,
            "the zenith is deeper than the horizon: {} vs {}",
            top[2],
            horizon.blue
        );
        assert_eq!(mesh.count_vertices(), colours.len());
        assert!(mesh.indices().is_some_and(|indices| indices.len() % 3 == 0));
    }

    /// **The declared cue is filed under its own claim, and the claim is
    /// designed.** `DesignedSky::declared` must name [`DESIGNED_SKY_CLAIM`]
    /// and the three distinct colours, so a run can label what it draws.
    #[test]
    fn accept_m01_play_world_the_designed_sky_declares_its_claim_and_colours() {
        let sky = DesignedSky::declared().expect("the declared cue is well formed");
        assert_eq!(sky.claim().as_str(), DESIGNED_SKY_CLAIM);
        assert_ne!(sky.zenith(), sky.horizon());
        assert_ne!(sky.horizon(), sky.below());
        assert!(sky.radius_m() > 0.0);
    }
}
