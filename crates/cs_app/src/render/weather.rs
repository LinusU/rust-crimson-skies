//! The drawn weather of a windowed scene: the designed cloud deck of a record
//! that authors no cloud (task #1440 `M01-PLAY-WEATHER`, narrowed by task #2001
//! `BUG-M01-WHITE-WEATHER-SHEET`).
//!
//! Task #1434 (`M01-PLAY-SKY`) drew the *horizon* of a windowed scene — the
//! sky dome, the sea and the fog. This module used to add two flat translucent
//! quads over it, a cloud deck and a "low haze" veil. Both windowed
//! compositions install it ([`WeatherPlugin`]) right after the horizon.
//!
//! # What is drawn, and what no longer is
//!
//! * **No haze plane.** The haze was a designed, horizontal, translucent quad
//!   at 300 m. Seen from above it blended over the whole ground and washed the
//!   island out white, and it cut straight through hills. Nothing in the
//!   original puts a flat layer there: the original's distance fog is the
//!   zone's `FOG_COLOR`/`FOG_RANGES`, which `render::horizon` binds as real
//!   fog (`docs/findings/2026-10-23-m01-fog-update-static.md`). The haze quad
//!   is deleted, not tuned.
//! * **No deck for an authored `CLOUD_COVER`.** The actual campaign M01
//!   (`ZBD/C3/M01`) authors `BOTTOM 10000`, `TOP 11000`: in the original that
//!   band is a *volume* — a screen-colour and vertex-intensity effect applied
//!   to geometry whose world Y lies between the two, zero at and outside
//!   them (`docs/findings/2026-10-18-parity-m01-atmo-a-weather-units-and-
//!   sky-consumers.md`, §3). Drawing it as one coplanar quad would present a
//!   volume as a sheet, so an authored layer draws **nothing** here; how the
//!   volume looks from inside is the open question of `PARITY-M01-ATMO`
//!   (#1643, #1745).
//! * **The designed deck** remains for an environment that authors no cloud
//!   layer at all (the original-assets playtest, which carries no record):
//!   one horizontal, unlit, translucent quad whose per-vertex alpha is a
//!   designed, deterministic pattern whose *area* is the deck's coverage, so
//!   it is never a uniform sheet. It is labelled [`DESIGNED_LABEL`] on every
//!   field.
//!
//! The deck is kept horizontally under the camera on every frame and never
//! rotated: it sits at its own world altitude whatever the camera's attitude
//! (`specs/F19-sky-atmosphere-weather-and-visibility.md`, non-negotiable 3).
//!
//! Nothing here is `verified_original`: this is a labelled development
//! presentation over original content, and no original executable ran.

use std::f32::consts::TAU;

use bevy::asset::{Assets, RenderAssetUsages};
use bevy::mesh::{Indices, Mesh, Mesh3d, PrimitiveTopology};
use bevy::prelude::{
    AlphaMode, App, Camera3d, Color, Commands, Component, GlobalTransform, MeshMaterial3d, Plugin,
    Quat, Query, Res, ResMut, Resource, StandardMaterial, Startup, Transform, Update, Vec3,
    Visibility, With, Without,
};
use cs_content::environment::EnvironmentDefinition;
use cs_types::content::Provenance;
use cs_types::evidence::ClaimStatus;

/// The label every designed weather value carries.
///
/// It travels in each [`WeatherOrigin::Designed`] payload, so a screenshot, a
/// test failure or a review note always says which build painted these
/// values.
pub const DESIGNED_LABEL: &str = "M01-PLAY-WEATHER designed weather, 2026-10-15";

/// The designed cloud deck's altitude in metres above the ground plane: above
/// the world a mission spawns in and below the sky dome, so the deck reads as
/// a ceiling rather than as the horizon.
pub const DESIGNED_CLOUD_ALTITUDE_M: f32 = 2_400.0;

/// The designed cloud deck's coverage fraction, used when no record authors
/// one. It is the fraction of the deck's area the pattern paints, so it is
/// also what the frame shows.
pub const DESIGNED_CLOUD_COVERAGE: f32 = 0.55;

/// The designed cloud colour (sRGB): a bright, slightly cool white.
pub const DESIGNED_CLOUD_COLOUR: [f32; 3] = [0.86, 0.88, 0.92];

/// The designed half extent of every drawn layer, in metres: past the
/// horizon's fog far distance, so a layer's edge is always already haze and
/// is never visible as an edge.
pub const DESIGNED_LAYER_EXTENT_M: f32 = 20_000.0;

/// How many cells each drawn layer's grid has per axis: one cell per 625 m at
/// [`DESIGNED_LAYER_EXTENT_M`], which is fine enough for the designed cloud
/// blobs and coarse enough to stay one cheap mesh.
pub const LAYER_MESH_CELLS: usize = 64;

/// The width of the designed cloud pattern's soft edge, as a fraction of the
/// pattern's own range: the band over which a cell fades from clear to
/// covered instead of switching at a hard line.
pub const CLOUD_PATTERN_EDGE: f32 = 0.06;

/// One field of a drawn layer, named so its origin can be asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeatherField {
    /// The layer's altitude above the ground plane, in metres.
    Altitude,
    /// The layer's coverage fraction: the share of the sky it paints.
    Coverage,
    /// The layer's colour.
    Colour,
    /// The layer's half extent in metres.
    Extent,
}

/// Every field a [`WeatherLayerSpec`] answers for.
pub const WEATHER_FIELDS: &[WeatherField] = &[
    WeatherField::Altitude,
    WeatherField::Coverage,
    WeatherField::Colour,
    WeatherField::Extent,
];

/// Where one drawn weather value came from.
#[derive(Clone, Debug, PartialEq)]
pub enum WeatherOrigin {
    /// The value is the mission environment record's own, carrying that
    /// record's provenance unchanged.
    Bound(Provenance),
    /// The value is this build's own, labelled.
    Designed(&'static str),
}

/// Why a drawn layer's origin could not be answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnknownWeatherField {
    /// The index of the layer in [`WeatherSpec::layers`].
    pub layer: usize,
    /// The field that was asked about.
    pub field: WeatherField,
}

impl std::fmt::Display for UnknownWeatherField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "no origin was recorded for {:?} of layer {}",
            self.field, self.layer
        )
    }
}

impl std::error::Error for UnknownWeatherField {}

/// One drawn layer: a horizontal quad the plugin keeps under the camera.
///
/// Colours are **sRGB display values**, distances **metres** and coverage a
/// fraction in `[0, 1]`, exactly as the scene's other colours are written.
#[derive(Clone, Debug, PartialEq)]
pub struct WeatherLayerSpec {
    altitude_m: f32,
    coverage: f32,
    colour: [f32; 3],
    half_extent_m: f32,
    origins: Vec<(WeatherField, WeatherOrigin)>,
}

impl WeatherLayerSpec {
    /// The designed cloud deck: what a record that authors no cloud layer
    /// gets, and the only layer this module draws.
    #[must_use]
    pub fn designed_cloud() -> Self {
        Self::recorded(
            DESIGNED_CLOUD_ALTITUDE_M,
            DESIGNED_CLOUD_COVERAGE,
            DESIGNED_CLOUD_COLOUR,
            DESIGNED_LAYER_EXTENT_M,
            [
                WeatherOrigin::Designed(DESIGNED_LABEL),
                WeatherOrigin::Designed(DESIGNED_LABEL),
                WeatherOrigin::Designed(DESIGNED_LABEL),
                WeatherOrigin::Designed(DESIGNED_LABEL),
            ],
        )
    }

    fn recorded(
        altitude_m: f32,
        coverage: f32,
        colour: [f32; 3],
        half_extent_m: f32,
        origins: [WeatherOrigin; 4],
    ) -> Self {
        Self {
            altitude_m,
            coverage,
            colour,
            half_extent_m,
            origins: WEATHER_FIELDS.iter().copied().zip(origins).collect(),
        }
    }

    /// The layer's altitude above the ground plane, in metres.
    #[must_use]
    pub const fn altitude_m(&self) -> f32 {
        self.altitude_m
    }

    /// The layer's coverage fraction.
    ///
    /// It is the share of the deck's own area the pattern paints, never the
    /// uniform opacity of the plane.
    #[must_use]
    pub const fn coverage(&self) -> f32 {
        self.coverage
    }

    /// The layer's colour, sRGB.
    #[must_use]
    pub const fn colour(&self) -> [f32; 3] {
        self.colour
    }

    /// The layer's half extent in metres.
    #[must_use]
    pub const fn half_extent_m(&self) -> f32 {
        self.half_extent_m
    }

    /// The origin recorded for `field`, or `None` when none was.
    #[must_use]
    pub fn origin(&self, field: WeatherField) -> Option<&WeatherOrigin> {
        self.origins
            .iter()
            .find(|(known, _)| *known == field)
            .map(|(_, origin)| origin)
    }

    /// Whether every field of this layer is a designed value.
    ///
    /// A field bound to a provenance that is *itself* `Designed` counts as
    /// designed — the record carries a value somebody authored, not one
    /// measured from the original.
    #[must_use]
    pub fn is_entirely_designed(&self) -> bool {
        WEATHER_FIELDS
            .iter()
            .all(|field| match self.origin(*field) {
                Some(WeatherOrigin::Designed(_)) => true,
                Some(WeatherOrigin::Bound(provenance)) => provenance.class == ClaimStatus::Designed,
                None => false,
            })
    }

    /// The layer's mesh: one flat grid in the XZ plane, twice
    /// [`Self::half_extent_m`] across, whose every vertex carries the layer's
    /// colour in linear space and the layer's own alpha at that vertex's
    /// position ([`Self::alpha_with`]).
    ///
    /// The entity's transform carries the altitude; the mesh is built at the
    /// origin so the layer's own coordinates are the ones the pattern reads.
    #[must_use]
    pub fn mesh(&self) -> Mesh {
        let cells = LAYER_MESH_CELLS;
        let threshold = self.pattern_threshold();
        let linear = Color::srgb(self.colour[0], self.colour[1], self.colour[2]).to_linear();
        let vertices = cells + 1;
        let mut positions = Vec::with_capacity(vertices * vertices);
        let mut normals = Vec::with_capacity(vertices * vertices);
        let mut colours = Vec::with_capacity(vertices * vertices);
        for row in 0..=cells {
            for column in 0..=cells {
                // A whole grid, so a layer's edge is the extent's edge and not
                // a line the pattern happens to draw.
                let u = normalized_coordinate(column, cells);
                let v = normalized_coordinate(row, cells);
                positions.push([u * self.half_extent_m, 0.0, v * self.half_extent_m]);
                normals.push([0.0, 1.0, 0.0]);
                colours.push([
                    linear.red,
                    linear.green,
                    linear.blue,
                    self.alpha_with(threshold, u, v),
                ]);
            }
        }
        let mut indices = Vec::with_capacity(cells * cells * 6);
        for row in 0..cells {
            for column in 0..cells {
                let base = (row * vertices + column) as u32;
                let right = base + 1;
                let below = base + vertices as u32;
                let below_right = below + 1;
                indices.extend_from_slice(&[base, below, right, right, below, below_right]);
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

    /// The layer's alpha at layer-normalized coordinates `u` and `v`, each in
    /// `[-1, 1]` from the layer's centre to its edge, for the pattern
    /// threshold [`Self::pattern_threshold`].
    ///
    /// This is the one function the layer's mesh is built from, which is what
    /// makes the drawn pattern testable without a GPU:
    ///
    /// The deck reads the designed pattern of [`designed_cloud_field`] and
    /// paints, at full opacity and with [`CLOUD_PATTERN_EDGE`] of soft edge,
    /// every cell the pattern puts above the threshold — which
    /// `pattern_threshold` places so that the painted share of the deck's own
    /// grid is the layer's coverage.
    ///
    /// `u` and `v` outside `[-1, 1]` are clamped, so an extended mesh can
    /// never paint a value the pattern does not have.
    #[must_use]
    fn alpha_with(&self, threshold: f32, u: f32, v: f32) -> f32 {
        let u = u.clamp(-1.0, 1.0);
        let v = v.clamp(-1.0, 1.0);
        smoothstep(
            threshold - CLOUD_PATTERN_EDGE,
            threshold + CLOUD_PATTERN_EDGE,
            designed_cloud_field(u, v),
        )
    }

    /// The [`designed_cloud_field`] value the deck paints from: the quantile
    /// of the layer's own grid at `1 - coverage`, so the share of the grid the
    /// pattern paints *is* the layer's coverage rather than a threshold that
    /// happens to look about right for the pattern's distribution.
    #[must_use]
    fn pattern_threshold(&self) -> f32 {
        let cells = LAYER_MESH_CELLS;
        let mut samples: Vec<f32> = (0..=cells)
            .flat_map(|row| (0..=cells).map(move |column| (row, column)))
            .map(|(row, column)| {
                designed_cloud_field(
                    normalized_coordinate(column, cells),
                    normalized_coordinate(row, cells),
                )
            })
            .collect();
        samples.sort_by(f32::total_cmp);
        // The number of samples the coverage asks for, as a rank from the top.
        let painted =
            ((f64::from(self.coverage) * samples.len() as f64).round() as usize).min(samples.len());
        samples[samples.len() - painted]
    }
}

/// The `[-1, 1]` coordinate of one grid index: the layer's centre is `0` and
/// its edge is `±1`.
fn normalized_coordinate(index: usize, cells: usize) -> f32 {
    #[allow(clippy::cast_precision_loss)]
    {
        (index as f32 / cells as f32) * 2.0 - 1.0
    }
}

/// The designed cloud pattern: a deterministic field in `[0, 1]` over the
/// layer's own coordinates.
///
/// Three sine lobes at different frequencies and directions, so the deck
/// breaks into cloud masses with clear gaps between them. A designed pattern,
/// and labelled as one: the original's cloud texture and how the 2000 engine
/// mapped it onto `cloudlayer` are unmeasured (`docs/findings/
/// 2026-10-14-m01-play-sky-horizon.md`).
#[must_use]
pub fn designed_cloud_field(u: f32, v: f32) -> f32 {
    let raw = (TAU * (3.0 * u + 2.0 * v)).sin()
        + 0.7 * (TAU * (-2.0 * u + 5.0 * v)).sin()
        + 0.4 * (TAU * (7.0 * u - 3.0 * v)).sin();
    // The three lobes sum to [-2.1, 2.1]; normalizing keeps the field in [0, 1]
    // whatever a layer's extent is.
    (raw / 2.1) * 0.5 + 0.5
}

/// Hermite's smoothstep: `0` at or below `edge0`, `1` at or above `edge1`, and
/// a cubic ease between them.
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    if edge1 <= edge0 {
        return if x < edge0 { 0.0 } else { 1.0 };
    }
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Every layer a windowed scene draws, with the origin of each value.
///
/// [`WeatherSpec::from_environment`] is the constructor a composed mission
/// uses. It may hold no layer at all: a record that authors a cloud band
/// describes a volume, which this module does not draw as a sheet.
#[derive(Clone, Debug, PartialEq, Resource)]
pub struct WeatherSpec {
    layers: Vec<WeatherLayerSpec>,
}

impl WeatherSpec {
    /// The designed weather: one labelled cloud deck.
    ///
    /// This is what the original-assets playtest draws, which carries no
    /// environment record at all.
    #[must_use]
    pub fn designed() -> Self {
        Self {
            layers: vec![WeatherLayerSpec::designed_cloud()],
        }
    }

    /// The weather a mission's own environment definition asks for.
    ///
    /// A record that authors cloud layers draws **no** layer: its
    /// `CLOUD_COVER` band is a volume (`BOTTOM`/`TOP` world-Y metres, M01:
    /// 10 000–11 000), and a flat quad would show it as a sheet. A record that
    /// authors none gets one designed deck, and the label travels with it.
    /// Nothing is ever drawn as a haze plane: the distance fog is the
    /// horizon's.
    #[must_use]
    pub fn from_environment(environment: &EnvironmentDefinition) -> Self {
        let layers = if environment.cloud_layers().is_empty() {
            vec![WeatherLayerSpec::designed_cloud()]
        } else {
            Vec::new()
        };
        Self { layers }
    }

    /// Every drawn layer.
    #[must_use]
    pub fn layers(&self) -> &[WeatherLayerSpec] {
        &self.layers
    }

    /// The origin recorded for `field` of the layer at `layer`.
    ///
    /// # Errors
    ///
    /// [`UnknownWeatherField`] when no such layer exists or it recorded no
    /// origin for the field, which the constructors make impossible for every
    /// field in [`WEATHER_FIELDS`].
    pub fn origin(
        &self,
        layer: usize,
        field: WeatherField,
    ) -> Result<&WeatherOrigin, UnknownWeatherField> {
        self.layers
            .get(layer)
            .and_then(|drawn| drawn.origin(field))
            .ok_or(UnknownWeatherField { layer, field })
    }

    /// Whether every drawn value of this spec is designed: no measured value
    /// reaches the screen.
    #[must_use]
    pub fn is_entirely_designed(&self) -> bool {
        self.layers
            .iter()
            .all(WeatherLayerSpec::is_entirely_designed)
    }
}

/// One drawn layer entity: the deck the plugin keeps under the camera.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct WeatherLayer {
    /// The index of the layer in [`WeatherSpec::layers`].
    pub layer: usize,
}

/// Draws the designed cloud deck, when a scene has one.
///
/// The plugin owns no game state: it takes a [`WeatherSpec`] once, spawns one
/// entity per layer at `Startup` and re-places them around the camera every
/// `Update`. The spec stays readable as the [`WeatherSpec`] resource for a
/// test or a capture to report.
#[derive(Clone, Debug)]
pub struct WeatherPlugin {
    spec: WeatherSpec,
}

impl WeatherPlugin {
    /// Draws this spec's weather.
    #[must_use]
    pub fn new(spec: WeatherSpec) -> Self {
        Self { spec }
    }

    /// Draws the weather the mission's own environment definition asks for
    /// (see [`WeatherSpec::from_environment`]).
    #[must_use]
    pub fn from_environment(environment: &EnvironmentDefinition) -> Self {
        Self::new(WeatherSpec::from_environment(environment))
    }

    /// Draws the designed weather for a scene that carries no environment
    /// record (the original-assets playtest).
    #[must_use]
    pub fn designed() -> Self {
        Self::new(WeatherSpec::designed())
    }

    /// The spec this plugin draws.
    #[must_use]
    pub const fn spec(&self) -> &WeatherSpec {
        &self.spec
    }
}

impl Plugin for WeatherPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.spec.clone())
            .add_systems(Startup, spawn_weather)
            .add_systems(Update, place_weather_around_camera);
    }
}

/// Spawns one entity per drawn layer, once, from the spec resource.
fn spawn_weather(
    mut commands: Commands,
    spec: Res<WeatherSpec>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (index, layer) in spec.layers().iter().enumerate() {
        let material = materials.add(StandardMaterial {
            // The layer's colour and its alpha both travel in the mesh's own
            // vertex colours, so one material serves every layer.
            base_color: Color::WHITE,
            // A layer is the air itself, not a lit surface.
            unlit: true,
            // The vertex alpha is the layer's opacity; without this the alpha
            // would be ignored and every layer would draw solid.
            alpha_mode: AlphaMode::Blend,
            // A layer is seen from below when the aircraft climbs under it and
            // from above when it is flown through.
            cull_mode: None,
            ..StandardMaterial::default()
        });
        commands.spawn((
            WeatherLayer { layer: index },
            Mesh3d(meshes.add(layer.mesh())),
            MeshMaterial3d(material),
            Transform::from_xyz(0.0, layer.altitude_m(), 0.0),
            Visibility::default(),
        ));
    }
}

/// Keeps every layer horizontally under the camera at its own world altitude.
///
/// This is F19 non-negotiable 3 applied to a scene with no `WorldOrigin`:
/// translation follows the camera, orientation never does, and the altitude
/// is the layer's own — so rolling or climbing cannot tilt or drag it. The
/// rotation is pinned to the identity every frame, exactly as
/// [`crate::render::horizon`] pins the dome's, so a camera that rolls cannot
/// rotate a layer through this system.
fn place_weather_around_camera(
    spec: Res<WeatherSpec>,
    cameras: Query<&GlobalTransform, (With<Camera3d>, Without<crate::render::OverlayCamera>)>,
    mut layers: Query<(&WeatherLayer, &mut Transform), Without<Camera3d>>,
) {
    let Ok(camera) = cameras.single() else {
        return;
    };
    let translation = camera.translation();
    for (drawn, mut transform) in &mut layers {
        let Some(layer) = spec.layers().get(drawn.layer) else {
            continue;
        };
        transform.translation = Vec3::new(translation.x, layer.altitude_m(), translation.z);
        transform.rotation = Quat::IDENTITY;
    }
}
