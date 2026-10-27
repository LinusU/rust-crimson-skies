//! The drawn precipitation of a windowed scene: the mission environment's
//! authored rain or snow (task #1459 `M01-PLAY-PRECIPITATION`).
//!
//! Tasks #1434 and #1440 drew the horizon, the cloud deck and the haze.
//! `weather.zrd` also authors precipitation — M01's is `TYPE RAIN` — and
//! [`crate::environment::cosmetic`] resolves it into a
//! [`PrecipitationEffect`](crate::environment::cosmetic::PrecipitationEffect) with a
//! [`CosmeticField`](crate::environment::cosmetic::CosmeticField) drawn from the run's
//! *cosmetic* stream. This module is the consumer: both windowed compositions
//! install [`PrecipitationPlugin`] right after the weather.
//!
//! # What is original and what is designed
//!
//! Every field of a drawn field carries a
//! [`WeatherOrigin`](crate::render::weather::WeatherOrigin):
//!
//! * **bound** — only the *kind* (rain or snow), with the provenance of the
//!   record's `TYPE` claim (task #636, `ClaimStatus::ObservedTool`);
//! * **designed** — everything else, labelled by
//!   [`DESIGNED_LABEL`](crate::render::horizon::DESIGNED_LABEL): the
//!   particle size, the fall speed, the colour (the record's `COLOR
//!   (128,128,128)` has an unmeasured scale and is unbound), the count (the
//!   record's `PARTICLES 100` is unbound; the count is the cosmetic field's
//!   own), the placement (the cosmetic stream's offsets, tiled), and the
//!   impact rule (none is drawn: nothing measured one).
//!
//! # Gameplay is untouched
//!
//! Particle offsets come from the cosmetic stream only
//! ([`CosmeticField::particles`](crate::environment::cosmetic::CosmeticField::particles));
//! this module draws no random number of its own, owns no game state and is
//! read by no gameplay query. The motion is a
//! function of the render clock and the camera position alone. The
//! authoritative wind is **not** applied to the drawn motion in this task;
//! that is recorded in `docs/findings/2026-10-16-m01-play-precipitation.md`.
//!
//! A clear record draws nothing, and so does an unknown kind: unknown is never
//! turned into a clear sky or into invented rain.
//!
//! Nothing here is `verified_original`.

use bevy::asset::{Assets, RenderAssetUsages};
use bevy::mesh::{Indices, Mesh, Mesh3d, PrimitiveTopology};
use bevy::prelude::{
    AlphaMode, App, Camera3d, Color, Commands, Component, GlobalTransform, MeshMaterial3d, Plugin,
    Quat, Query, Res, ResMut, Resource, StandardMaterial, Startup, Time, Transform, Update, Vec3,
    Visibility, With, Without,
};
use cs_content::environment::PrecipitationKind;
use cs_types::content::{Provenance, Resolved};

use crate::environment::cosmetic::{COSMETIC_FIELD_HALF_EXTENT_M, PrecipitationEffect};
use crate::environment::session::EnvironmentSession;
use crate::render::weather::WeatherOrigin;

/// The label every designed precipitation value carries.
pub const DESIGNED_LABEL: &str = "M01-PLAY-PRECIPITATION designed precipitation, 2026-10-16";

/// The designed rain streak length in metres.
pub const DESIGNED_RAIN_LENGTH_M: f32 = 1.4;
/// The designed rain streak width in metres.
pub const DESIGNED_RAIN_WIDTH_M: f32 = 0.025;
/// The designed rain fall speed in metres per second.
pub const DESIGNED_RAIN_FALL_M_S: f64 = 9.0;
/// The designed snow flake edge in metres.
pub const DESIGNED_SNOW_SIZE_M: f32 = 0.12;
/// The designed snow fall speed in metres per second.
pub const DESIGNED_SNOW_FALL_M_S: f64 = 1.5;
/// The designed precipitation colour (sRGB). The record's own `COLOR` has an
/// unmeasured scale and is not used.
pub const DESIGNED_COLOUR: [f32; 3] = [0.78, 0.82, 0.88];
/// The designed opacity of a drawn particle.
pub const DESIGNED_ALPHA: f32 = 0.55;
/// The tiles of the particle lattice on each axis: `-1..=1`, so the camera
/// always sits inside a block of 3 x 3 x 3 repeats of the cosmetic cube.
pub const LATTICE_RADIUS: i32 = 1;
/// The edge of one repeat of the cosmetic cube, in metres.
pub const LATTICE_PERIOD_M: f64 = COSMETIC_FIELD_HALF_EXTENT_M * 2.0;

/// One field of a drawn precipitation spec, named so its origin can be asked
/// for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrecipitationField {
    /// Rain or snow.
    Kind,
    /// The particle's colour.
    Colour,
    /// The particle's size.
    Size,
    /// The fall speed.
    FallSpeed,
    /// How many particles are drawn.
    Count,
    /// Where the particles are (the cosmetic stream's offsets).
    Placement,
    /// What a particle does on reaching the ground (nothing is drawn).
    Impact,
}

/// Every field a [`PrecipitationSpec`] answers for.
pub const PRECIPITATION_FIELDS: &[PrecipitationField] = &[
    PrecipitationField::Kind,
    PrecipitationField::Colour,
    PrecipitationField::Size,
    PrecipitationField::FallSpeed,
    PrecipitationField::Count,
    PrecipitationField::Placement,
    PrecipitationField::Impact,
];

/// Why a spec draws nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Undrawn {
    /// No precipitation is authored (or no record exists at all).
    Clear,
    /// The authored kind is an explicit unknown.
    UnknownKind {
        /// The claim id of the unknown kind.
        claim_id: String,
    },
}

/// The drawn particle field of a spec.
#[derive(Clone, Debug, PartialEq)]
struct DrawnField {
    kind: PrecipitationKind,
    offsets_m: Vec<[f32; 3]>,
    origins: Vec<(PrecipitationField, WeatherOrigin)>,
}

/// What a windowed scene draws for precipitation, with the origin of every
/// value.
#[derive(Clone, Debug, PartialEq, Resource)]
pub struct PrecipitationSpec {
    drawn: Result<DrawnField, Undrawn>,
}

impl PrecipitationSpec {
    /// A scene with no precipitation record (the original-assets playtest)
    /// draws nothing.
    #[must_use]
    pub fn none() -> Self {
        Self {
            drawn: Err(Undrawn::Clear),
        }
    }

    /// Resolves what `effect` asks for. `kind_provenance` is the provenance
    /// of the record's kind claim, when the record knows one.
    #[must_use]
    pub fn from_effect(effect: &PrecipitationEffect, kind_provenance: Option<&Provenance>) -> Self {
        let drawn = match effect {
            PrecipitationEffect::Clear => Err(Undrawn::Clear),
            PrecipitationEffect::Unknown { claim_id, .. } => Err(Undrawn::UnknownKind {
                claim_id: claim_id.as_str().to_owned(),
            }),
            PrecipitationEffect::Field(field) => {
                let designed = || WeatherOrigin::Designed(DESIGNED_LABEL);
                let kind_origin = kind_provenance.map_or_else(designed, |provenance| {
                    WeatherOrigin::Bound(provenance.clone())
                });
                #[allow(clippy::cast_possible_truncation)]
                let offsets_m = field
                    .particles()
                    .iter()
                    .map(|particle| particle.offset_m().map(|value| value as f32))
                    .collect();
                Ok(DrawnField {
                    kind: field.kind(),
                    offsets_m,
                    origins: PRECIPITATION_FIELDS
                        .iter()
                        .copied()
                        .zip([
                            kind_origin,
                            designed(),
                            designed(),
                            designed(),
                            designed(),
                            designed(),
                            designed(),
                        ])
                        .collect(),
                })
            }
        };
        Self { drawn }
    }

    /// The precipitation of a running mission environment, at the state its
    /// clock has reached, decorated from the run's cosmetic seed.
    #[must_use]
    pub fn from_session(session: &EnvironmentSession) -> Self {
        let state = session.clock().state();
        let provenance = match state.precipitation().kind() {
            Resolved::Known(known) => Some(&known.provenance),
            Resolved::Unknown { .. } => None,
        };
        Self::from_effect(session.effects().precipitation(), provenance)
    }

    /// The drawn kind, or `None` when nothing is drawn.
    #[must_use]
    pub fn kind(&self) -> Option<PrecipitationKind> {
        self.drawn.as_ref().ok().map(|field| field.kind)
    }

    /// Why nothing is drawn, or `None` when something is.
    #[must_use]
    pub fn undrawn(&self) -> Option<&Undrawn> {
        self.drawn.as_ref().err()
    }

    /// Whether this spec draws any particle.
    #[must_use]
    pub fn is_drawn(&self) -> bool {
        self.drawn.is_ok()
    }

    /// The cosmetic offsets of one lattice cell, in metres.
    #[must_use]
    pub fn cell_offsets_m(&self) -> &[[f32; 3]] {
        self.drawn
            .as_ref()
            .map_or(&[], |field| field.offsets_m.as_slice())
    }

    /// How many particles the mesh holds (one cell's offsets, tiled).
    #[must_use]
    pub fn particle_count(&self) -> usize {
        let tiles = (2 * LATTICE_RADIUS as usize + 1).pow(3);
        self.cell_offsets_m().len() * tiles
    }

    /// The origin recorded for `field`, or `None` when nothing is drawn.
    #[must_use]
    pub fn origin(&self, field: PrecipitationField) -> Option<&WeatherOrigin> {
        self.drawn.as_ref().ok().and_then(|drawn| {
            drawn
                .origins
                .iter()
                .find(|(known, _)| *known == field)
                .map(|(_, origin)| origin)
        })
    }

    /// The fall speed in metres per second, or `None` when nothing is drawn.
    #[must_use]
    pub fn fall_speed_m_s(&self) -> Option<f64> {
        self.kind().map(|kind| match kind {
            PrecipitationKind::Snow => DESIGNED_SNOW_FALL_M_S,
            _ => DESIGNED_RAIN_FALL_M_S,
        })
    }

    /// The mesh of the particle lattice around the origin, or `None` when
    /// nothing is drawn. Every particle is two crossed vertical quads, so it
    /// reads from any side without following the camera.
    #[must_use]
    pub fn mesh(&self) -> Option<Mesh> {
        let field = self.drawn.as_ref().ok()?;
        let (half_width, half_height) = match field.kind {
            PrecipitationKind::Snow => (DESIGNED_SNOW_SIZE_M / 2.0, DESIGNED_SNOW_SIZE_M / 2.0),
            _ => (DESIGNED_RAIN_WIDTH_M / 2.0, DESIGNED_RAIN_LENGTH_M / 2.0),
        };
        let linear =
            Color::srgb(DESIGNED_COLOUR[0], DESIGNED_COLOUR[1], DESIGNED_COLOUR[2]).to_linear();
        let colour = [linear.red, linear.green, linear.blue, DESIGNED_ALPHA];
        let mut positions = Vec::new();
        let mut indices = Vec::new();
        #[allow(clippy::cast_possible_truncation)]
        let period = LATTICE_PERIOD_M as f32;
        for tx in -LATTICE_RADIUS..=LATTICE_RADIUS {
            for ty in -LATTICE_RADIUS..=LATTICE_RADIUS {
                for tz in -LATTICE_RADIUS..=LATTICE_RADIUS {
                    let shift = [tx as f32 * period, ty as f32 * period, tz as f32 * period];
                    for offset in &field.offsets_m {
                        let c = [
                            offset[0] + shift[0],
                            offset[1] + shift[1],
                            offset[2] + shift[2],
                        ];
                        for (dx, dz) in [(half_width, 0.0), (0.0, half_width)] {
                            let base = positions.len() as u32;
                            positions.extend_from_slice(&[
                                [c[0] - dx, c[1] - half_height, c[2] - dz],
                                [c[0] + dx, c[1] - half_height, c[2] + dz],
                                [c[0] + dx, c[1] + half_height, c[2] + dz],
                                [c[0] - dx, c[1] + half_height, c[2] - dz],
                            ]);
                            indices.extend_from_slice(&[
                                base,
                                base + 1,
                                base + 2,
                                base,
                                base + 2,
                                base + 3,
                            ]);
                        }
                    }
                }
            }
        }
        let vertex_count = positions.len();
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; vertex_count]);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![colour; vertex_count]);
        mesh.insert_indices(Indices::U32(indices));
        Some(mesh)
    }
}

/// Where the particle lattice sits for a camera at `camera` after `elapsed_s`
/// of render time falling at `fall_m_s`.
///
/// The lattice repeats every [`LATTICE_PERIOD_M`], so the fall is taken
/// modulo one period (no precision is lost to a long session) and the origin
/// snaps to the repeat nearest the camera; the particles themselves never
/// move relative to each other.
#[must_use]
pub fn lattice_origin(camera: Vec3, elapsed_s: f64, fall_m_s: f64) -> Vec3 {
    let fall = (fall_m_s * elapsed_s).rem_euclid(LATTICE_PERIOD_M);
    #[allow(clippy::cast_possible_truncation)]
    let (period, drift) = (LATTICE_PERIOD_M as f32, -(fall as f32));
    let snap = |value: f32, drift: f32| ((value - drift) / period).round() * period + drift;
    Vec3::new(
        snap(camera.x, 0.0),
        snap(camera.y, drift),
        snap(camera.z, 0.0),
    )
}

/// The drawn precipitation entity.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrecipitationLayer;

/// Draws the authored precipitation for one windowed scene. Owns no game
/// state: a spec once, one entity at `Startup`, a re-placement every `Update`.
#[derive(Clone, Debug)]
pub struct PrecipitationPlugin {
    spec: PrecipitationSpec,
}

impl PrecipitationPlugin {
    /// Draws this spec.
    #[must_use]
    pub fn new(spec: PrecipitationSpec) -> Self {
        Self { spec }
    }

    /// Draws what the mission's environment session authors.
    #[must_use]
    pub fn from_session(session: &EnvironmentSession) -> Self {
        Self::new(PrecipitationSpec::from_session(session))
    }

    /// Draws nothing: the scene carries no precipitation record.
    #[must_use]
    pub fn none() -> Self {
        Self::new(PrecipitationSpec::none())
    }

    /// The spec this plugin draws.
    #[must_use]
    pub const fn spec(&self) -> &PrecipitationSpec {
        &self.spec
    }
}

impl Plugin for PrecipitationPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.spec.clone())
            .add_systems(Startup, spawn_precipitation)
            .add_systems(Update, place_precipitation_around_camera);
    }
}

fn spawn_precipitation(
    mut commands: Commands,
    spec: Res<PrecipitationSpec>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(mesh) = spec.mesh() else {
        return;
    };
    let material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        ..StandardMaterial::default()
    });
    commands.spawn((
        PrecipitationLayer,
        Mesh3d(meshes.add(mesh)),
        MeshMaterial3d(material),
        Transform::default(),
        Visibility::default(),
    ));
}

fn place_precipitation_around_camera(
    spec: Res<PrecipitationSpec>,
    time: Res<Time>,
    cameras: Query<&GlobalTransform, (With<Camera3d>, Without<crate::render::OverlayCamera>)>,
    mut layers: Query<&mut Transform, (With<PrecipitationLayer>, Without<Camera3d>)>,
) {
    let (Ok(camera), Some(fall)) = (cameras.single(), spec.fall_speed_m_s()) else {
        return;
    };
    for mut transform in &mut layers {
        transform.translation = lattice_origin(camera.translation(), time.elapsed_secs_f64(), fall);
        transform.rotation = Quat::IDENTITY;
    }
}
