//! The drawn horizon of a windowed scene: the sky dome, the sea and the fog
//! (task #1434 `M01-PLAY-SKY`).
//!
//! `cs --cs-path <install> --mission M01` and `cs --playtest --cs-path
//! <install> --world c1c` both drew a flat clear colour: no sky, no horizon,
//! no sea. This module is the one production path that draws all three, and
//! both compositions install it ([`HorizonPlugin`]).
//!
//! # What is original and what is designed
//!
//! Every value the dome, the sea and the fog are built from carries a
//! [`HorizonOrigin`]:
//!
//! * **bound** — the value came out of the mission's own
//!   [`EnvironmentDefinition`](cs_content::environment::EnvironmentDefinition),
//!   which for a retail mission is the production bind of `weather.zrd`
//!   (task #636), carrying that record's own provenance.
//! * **designed** — the value is this build's own, labelled by
//!   [`DESIGNED_LABEL`]. M01's `weather.zrd` binds no fog colour, no ambient
//!   colour, no sky texture, no sea level and no viewing distance (those
//!   conventions are unmeasured — `docs/findings/
//!   2026-10-05-m01-lc-weather-bind.md`), so every drawn value of M01's
//!   horizon is designed today. `docs/findings/
//!   2026-10-14-m01-play-sky-horizon.md` lists both halves field by field.
//!
//! The original data does hold sky and sea material — `ZBD/C1C/gamez.zbd`'s
//! own texture table carries `Sky1`, `cloud1`, `cloud2`, `water1`,
//! `cloudlayer` and `moon1`, and its node array carries a `horizon` node —
//! but *how* the original renderer consumes them is unmeasured, so this stage
//! draws a designed gradient instead of guessing a mapping, as the task
//! allows. Binding the authored art is follow-up work (`M01-PLAY-WEATHER`,
//! #1440).
//!
//! # The three things drawn
//!
//! * **Sky**: an unlit unit sphere scaled to [`HorizonSpec::dome_radius_m`],
//!   whose every vertex colour comes from [`HorizonSpec::sky_colour_at`]:
//!   above the horizon it blends from [`HorizonSpec::sky_horizon`] at eye
//!   level to [`HorizonSpec::sky_zenith`] at the zenith, and below the
//!   horizon it *is* the fog colour — so the sea's far edge fades into
//!   exactly what the dome shows under the horizon, and no seam appears
//!   where the dome occludes the sea. The dome is not fogged
//!   (`fog_enabled: false`): its distance from the camera is constant, so
//!   fog would wash the whole gradient flat.
//! * **Sea**: a flat unlit plane of half extent
//!   [`HorizonSpec::sea_extent_m`] at [`HorizonSpec::sea_level_m`], kept
//!   horizontally under the camera so the sea always reaches past the fog's
//!   far distance and its edge is never seen as an edge.
//! * **Fog**: a [`DistanceFog`] with a linear falloff on the camera itself,
//!   from [`HorizonSpec::fog_start_m`] to [`HorizonSpec::fog_end_m`] in
//!   [`HorizonSpec::fog_colour`], plus the scene's [`GlobalAmbientLight`] in
//!   [`HorizonSpec::ambient_colour`].
//!
//! # F19 behaviour 3
//!
//! The dome is centred on the camera's **world** translation and keeps an
//! identity rotation: the camera may roll and pitch as it likes, the horizon
//! stays where the world put it (`specs/F19-sky-atmosphere-weather-and-
//! visibility.md`, non-negotiable 3). These scenes carry no
//! [`crate::origin::WorldOrigin`], so the record is not a
//! [`crate::environment::frame::SkyFrame`]; the rule it obeys is the same
//! one that frame records for sessions which do have an origin to rebase.
//!
//! Nothing here is `verified_original`: this is a labelled development
//! presentation over original content, and no original executable ran.

use bevy::asset::Assets;
use bevy::math::primitives::{Plane3d, Sphere};
use bevy::mesh::{Mesh, Mesh3d, VertexAttributeValues};
use bevy::prelude::{
    App, Camera3d, ClearColor, Color, Commands, Component, DistanceFog, Entity, FogFalloff,
    GlobalAmbientLight, GlobalTransform, MeshMaterial3d, Plugin, Quat, Query, Res, ResMut,
    Resource, StandardMaterial, Startup, Transform, Update, Vec2, Vec3, Visibility, With, Without,
};
use cs_content::environment::EnvironmentDefinition;
use cs_types::content::{Provenance, Resolved};
use cs_types::evidence::ClaimStatus;

/// The label every designed horizon value carries.
///
/// It travels in each [`HorizonOrigin::Designed`] payload, so a screenshot, a
/// test failure or a review note always says which build painted these
/// colours.
pub const DESIGNED_LABEL: &str = "M01-PLAY-SKY designed horizon, 2026-10-14";

/// One field of the drawn horizon, named so its origin can be asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HorizonField {
    /// The sky's colour at the zenith.
    SkyZenith,
    /// The sky's colour at eye level.
    SkyHorizon,
    /// The sea's colour.
    SeaColour,
    /// The sea plane's altitude in metres.
    SeaLevel,
    /// The sea plane's half extent in metres.
    SeaExtent,
    /// The fog's colour, which is also the dome's colour below the horizon.
    FogColour,
    /// Where the fog starts, in metres from the camera.
    FogStart,
    /// Where the fog saturates, in metres from the camera.
    FogEnd,
    /// The sky dome's radius in metres.
    DomeRadius,
    /// The scene's ambient light colour.
    AmbientColour,
    /// The scene's ambient light brightness.
    AmbientBrightness,
}

/// Every field a [`HorizonSpec`] answers for.
pub const HORIZON_FIELDS: &[HorizonField] = &[
    HorizonField::SkyZenith,
    HorizonField::SkyHorizon,
    HorizonField::SeaColour,
    HorizonField::SeaLevel,
    HorizonField::SeaExtent,
    HorizonField::FogColour,
    HorizonField::FogStart,
    HorizonField::FogEnd,
    HorizonField::DomeRadius,
    HorizonField::AmbientColour,
    HorizonField::AmbientBrightness,
];

/// Where one drawn horizon value came from.
#[derive(Clone, Debug, PartialEq)]
pub enum HorizonOrigin {
    /// The value is the mission environment record's own, carrying that
    /// record's provenance unchanged.
    Bound(Provenance),
    /// The value is this build's own, labelled.
    Designed(&'static str),
}

/// Why a horizon field's origin could not be answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnknownHorizonField {
    /// The field that was asked about.
    pub field: HorizonField,
}

impl std::fmt::Display for UnknownHorizonField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "no origin was recorded for {:?}", self.field)
    }
}

impl std::error::Error for UnknownHorizonField {}

/// What the drawn horizon paints, with the origin of every value.
///
/// Colours are **sRGB display values** and distances **metres**, exactly as
/// the scene's other colours are written.
/// [`HorizonSpec::from_environment`] is the constructor a composed mission
/// uses: it reads the mission's own definition and marks each field bound or
/// designed, so a value nobody measured is labelled rather than silently
/// coloured.
#[derive(Clone, Debug, PartialEq, Resource)]
pub struct HorizonSpec {
    sky_zenith: [f32; 3],
    sky_horizon: [f32; 3],
    sea_colour: [f32; 3],
    sea_level_m: f32,
    sea_extent_m: f32,
    fog_colour: [f32; 3],
    fog_start_m: f32,
    fog_end_m: f32,
    dome_radius_m: f32,
    ambient_colour: [f32; 3],
    ambient_brightness: f32,
    origins: Vec<(HorizonField, HorizonOrigin)>,
}

impl HorizonSpec {
    /// The designed palette this build draws when the environment record
    /// carries no measured value for a field.
    ///
    /// These are development colours chosen to read as sky, sea and haze on
    /// an ordinary monitor; none of them is a claim about the original.
    #[must_use]
    pub fn designed() -> Self {
        Self::from_record(None, None, None)
    }

    /// The spec a mission's own environment definition asks for.
    ///
    /// A fog colour or an ambient colour the record carries as
    /// `Resolved::Known` is taken verbatim with its provenance; every value
    /// the record leaves `Resolved::Unknown` falls back to the designed
    /// palette and is recorded as [`HorizonOrigin::Designed`]. M01 leaves all
    /// of them unknown, so its horizon is entirely designed — which is what
    /// `docs/findings/2026-10-14-m01-play-sky-horizon.md` records.
    #[must_use]
    pub fn from_environment(environment: &EnvironmentDefinition) -> Self {
        Self::from_record(
            known_pair(environment.fog().color_linear()),
            known_pair(environment.lighting().ambient_linear()),
            known_provenance(environment.sky().texture()),
        )
    }

    /// Builds the spec from the fields an environment record can carry (fog
    /// colour, ambient colour, sky texture identity), falling back to the
    /// designed palette for whichever of them is absent.
    fn from_record(
        fog: Option<([f64; 3], Provenance)>,
        ambient: Option<([f64; 3], Provenance)>,
        sky: Option<Provenance>,
    ) -> Self {
        let mut spec = Self {
            sky_zenith: DESIGNED_SKY_ZENITH,
            sky_horizon: DESIGNED_SKY_HORIZON,
            sea_colour: DESIGNED_SEA_COLOUR,
            sea_level_m: DESIGNED_SEA_LEVEL_M,
            sea_extent_m: DESIGNED_SEA_EXTENT_M,
            fog_colour: DESIGNED_SKY_HORIZON,
            fog_start_m: DESIGNED_FOG_START_M,
            fog_end_m: DESIGNED_FOG_END_M,
            dome_radius_m: DESIGNED_DOME_RADIUS_M,
            ambient_colour: DESIGNED_AMBIENT_COLOUR,
            ambient_brightness: DESIGNED_AMBIENT_BRIGHTNESS,
            origins: HORIZON_FIELDS
                .iter()
                .map(|field| (*field, HorizonOrigin::Designed(DESIGNED_LABEL)))
                .collect(),
        };
        if let Some((color, provenance)) = fog {
            spec.fog_colour = linear_to_srgb(color);
            spec.set_origin(HorizonField::FogColour, HorizonOrigin::Bound(provenance));
            // The dome's horizon band follows the fog colour, so one value
            // decides both and the sea cannot fade into a colour the dome
            // does not show under it.
            spec.sky_horizon = spec.fog_colour;
            spec.set_origin(
                HorizonField::SkyHorizon,
                HorizonOrigin::Designed("the dome's horizon band follows the fog colour"),
            );
        }
        if let Some((color, provenance)) = ambient {
            spec.ambient_colour = linear_to_srgb(color);
            spec.set_origin(
                HorizonField::AmbientColour,
                HorizonOrigin::Bound(provenance),
            );
        }
        if let Some(provenance) = sky {
            spec.set_origin(HorizonField::SkyZenith, HorizonOrigin::Bound(provenance));
        }
        spec
    }

    fn set_origin(&mut self, field: HorizonField, origin: HorizonOrigin) {
        if let Some(slot) = self.origins.iter_mut().find(|(known, _)| *known == field) {
            slot.1 = origin;
        } else {
            self.origins.push((field, origin));
        }
    }

    /// The origin recorded for `field`.
    ///
    /// # Errors
    ///
    /// [`UnknownHorizonField`] when no origin was recorded for it, which the
    /// constructors make impossible for a field in [`HORIZON_FIELDS`].
    pub fn origin(&self, field: HorizonField) -> Result<&HorizonOrigin, UnknownHorizonField> {
        self.origins
            .iter()
            .find(|(known, _)| *known == field)
            .map(|(_, origin)| origin)
            .ok_or(UnknownHorizonField { field })
    }

    /// Whether every field of this spec is a designed value: no bound value
    /// reaches the screen.
    ///
    /// A field bound to a provenance that is *itself* `Designed` counts as
    /// designed — the record carries a value somebody authored, not one
    /// measured from the original.
    #[must_use]
    pub fn is_entirely_designed(&self) -> bool {
        self.origins.iter().all(|(_, origin)| match origin {
            HorizonOrigin::Designed(_) => true,
            HorizonOrigin::Bound(provenance) => provenance.class == ClaimStatus::Designed,
        })
    }

    /// The sky's colour at the zenith, sRGB.
    #[must_use]
    pub const fn sky_zenith(&self) -> [f32; 3] {
        self.sky_zenith
    }

    /// The sky's colour at eye level, sRGB. Equal to [`Self::fog_colour`].
    #[must_use]
    pub const fn sky_horizon(&self) -> [f32; 3] {
        self.sky_horizon
    }

    /// The sea's colour, sRGB.
    #[must_use]
    pub const fn sea_colour(&self) -> [f32; 3] {
        self.sea_colour
    }

    /// The sea plane's altitude in metres (unmeasured: designed).
    #[must_use]
    pub const fn sea_level_m(&self) -> f32 {
        self.sea_level_m
    }

    /// The sea plane's half extent in metres.
    #[must_use]
    pub const fn sea_extent_m(&self) -> f32 {
        self.sea_extent_m
    }

    /// The fog's colour, sRGB.
    #[must_use]
    pub const fn fog_colour(&self) -> [f32; 3] {
        self.fog_colour
    }

    /// Where the fog starts, in metres from the camera.
    #[must_use]
    pub const fn fog_start_m(&self) -> f32 {
        self.fog_start_m
    }

    /// Where the fog saturates, in metres from the camera.
    #[must_use]
    pub const fn fog_end_m(&self) -> f32 {
        self.fog_end_m
    }

    /// The sky dome's radius in metres.
    #[must_use]
    pub const fn dome_radius_m(&self) -> f32 {
        self.dome_radius_m
    }

    /// The scene's ambient light colour, sRGB.
    #[must_use]
    pub const fn ambient_colour(&self) -> [f32; 3] {
        self.ambient_colour
    }

    /// The scene's ambient light brightness.
    #[must_use]
    pub const fn ambient_brightness(&self) -> f32 {
        self.ambient_brightness
    }

    /// The dome's colour at elevation `unit_y` (−1 below, 0 at the horizon,
    /// +1 at the zenith), as a linear-space `[r, g, b, a]` vertex colour.
    ///
    /// This is the one function the dome's mesh is built from, which is what
    /// makes the seam rule testable without a GPU: at `unit_y <= 0` the
    /// returned colour is the fog colour exactly, so the dome shows, under
    /// the horizon, precisely what the fogged sea fades into.
    #[must_use]
    pub fn sky_colour_at(&self, unit_y: f32) -> [f32; 4] {
        let srgb = if unit_y <= 0.0 {
            self.fog_colour
        } else {
            let t = unit_y.min(1.0);
            [
                lerp(self.sky_horizon[0], self.sky_zenith[0], t),
                lerp(self.sky_horizon[1], self.sky_zenith[1], t),
                lerp(self.sky_horizon[2], self.sky_zenith[2], t),
            ]
        };
        let linear = Color::srgb(srgb[0], srgb[1], srgb[2]).to_linear();
        [linear.red, linear.green, linear.blue, 1.0]
    }

    /// The sky dome's mesh: a unit sphere whose every vertex carries
    /// [`Self::sky_colour_at`] of its own elevation.
    ///
    /// The sphere is a unit one so the elevation the gradient reads is the
    /// vertex's own normalized position; the entity's transform scales it to
    /// [`Self::dome_radius_m`]. The winding is left alone because the camera
    /// sits inside it and the material is created without face culling.
    #[must_use]
    pub fn dome_mesh(&self) -> Mesh {
        let mut mesh = Mesh::from(Sphere::new(1.0));
        let positions = match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
            Some(VertexAttributeValues::Float32x3(positions)) => positions.clone(),
            _ => Vec::new(),
        };
        let colours: Vec<[f32; 4]> = positions
            .iter()
            .map(|position| {
                let length = (position[0] * position[0]
                    + position[1] * position[1]
                    + position[2] * position[2])
                    .sqrt();
                let unit_y = if length > 0.0 {
                    position[1] / length
                } else {
                    0.0
                };
                self.sky_colour_at(unit_y)
            })
            .collect();
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colours);
        mesh
    }

    /// The sea's mesh: one flat quad in the XZ plane, twice
    /// [`Self::sea_extent_m`] across.
    #[must_use]
    pub fn sea_mesh(&self) -> Mesh {
        Mesh::from(Plane3d::new(Vec3::Y, Vec2::splat(self.sea_extent_m)))
    }
}

/// The designed sky zenith (sRGB).
const DESIGNED_SKY_ZENITH: [f32; 3] = [0.21, 0.42, 0.72];
/// The designed sky colour at eye level, also the designed haze (sRGB).
const DESIGNED_SKY_HORIZON: [f32; 3] = [0.66, 0.75, 0.84];
/// The designed sea colour (sRGB).
const DESIGNED_SEA_COLOUR: [f32; 3] = [0.07, 0.20, 0.36];
/// The designed sea altitude in metres: unmeasured, the original's water
/// level for `c1c` is not recorded anywhere in this workspace.
const DESIGNED_SEA_LEVEL_M: f32 = -1_000.0;
/// The designed sea half extent in metres: past the fog's far distance, so
/// its edge is never seen as an edge.
const DESIGNED_SEA_EXTENT_M: f32 = 20_000.0;
/// The designed distance where fog begins, in metres.
const DESIGNED_FOG_START_M: f32 = 6_000.0;
/// The designed distance where fog saturates, in metres: below
/// [`DESIGNED_DOME_RADIUS_M`], so the dome only ever occludes world that the
/// fog has already taken to the haze colour.
const DESIGNED_FOG_END_M: f32 = 20_000.0;
/// The designed sky dome radius in metres: inside the play camera's 40 km
/// far plane (`crate::playtest::visuals::FAR_PLANE_M`) and outside every
/// distance the fog still differentiates.
const DESIGNED_DOME_RADIUS_M: f32 = 24_000.0;
/// The designed ambient colour (sRGB): the haze, so lit surfaces sit in the
/// same air the fog paints.
const DESIGNED_AMBIENT_COLOUR: [f32; 3] = [0.72, 0.79, 0.87];
/// The designed ambient brightness: the playtest's own 400, kept, so this
/// stage changes only the ambient *colour*.
const DESIGNED_AMBIENT_BRIGHTNESS: f32 = 400.0;

fn lerp(from: f32, to: f32, t: f32) -> f32 {
    from + (to - from) * t
}

/// Converts a record's linear colour triple to sRGB for display.
fn linear_to_srgb(linear: [f64; 3]) -> [f32; 3] {
    let color = Color::linear_rgb(linear[0] as f32, linear[1] as f32, linear[2] as f32);
    let srgb = color.to_srgba();
    [srgb.red, srgb.green, srgb.blue]
}

fn known_pair(resolved: &Resolved<[f64; 3]>) -> Option<([f64; 3], Provenance)> {
    match resolved {
        Resolved::Known(known) => Some((known.value, known.provenance.clone())),
        Resolved::Unknown { .. } => None,
    }
}

fn known_provenance(resolved: &Resolved<cs_types::content::ContentId>) -> Option<Provenance> {
    match resolved {
        Resolved::Known(known) => Some(known.provenance.clone()),
        Resolved::Unknown { .. } => None,
    }
}

/// The dome entity: the gradient sphere that follows the camera.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct HorizonDome;

/// The sea entity: the flat plane under the camera.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct HorizonSea;

/// Draws the sky, the sea and the fog for one windowed scene.
///
/// The plugin owns no game state: it takes a [`HorizonSpec`] once, spawns the
/// two meshes at `Startup` and re-places them around the camera every
/// `Update`. The spec stays readable as the [`HorizonSpec`] resource for a
/// test or a capture to report.
#[derive(Clone, Debug)]
pub struct HorizonPlugin {
    spec: HorizonSpec,
}

impl HorizonPlugin {
    /// Draws this spec's horizon.
    #[must_use]
    pub fn new(spec: HorizonSpec) -> Self {
        Self { spec }
    }

    /// Draws the horizon the mission's own environment definition asks for
    /// (see [`HorizonSpec::from_environment`]).
    #[must_use]
    pub fn from_environment(environment: &EnvironmentDefinition) -> Self {
        Self::new(HorizonSpec::from_environment(environment))
    }

    /// Draws the designed horizon for a scene that carries no environment
    /// record (the original-assets playtest).
    #[must_use]
    pub fn designed() -> Self {
        Self::new(HorizonSpec::designed())
    }

    /// The spec this plugin draws.
    #[must_use]
    pub const fn spec(&self) -> &HorizonSpec {
        &self.spec
    }
}

impl Plugin for HorizonPlugin {
    fn build(&self, app: &mut App) {
        // The wiring sites add this plugin *after* `PlaytestVisualsPlugin`,
        // so these resources replace the flat clear colour and the untinted
        // ambient that plugin installs.
        app.insert_resource(self.spec.clone())
            .insert_resource(ClearColor(Color::srgb(
                self.spec.sky_horizon()[0],
                self.spec.sky_horizon()[1],
                self.spec.sky_horizon()[2],
            )))
            .insert_resource(GlobalAmbientLight {
                color: Color::srgb(
                    self.spec.ambient_colour()[0],
                    self.spec.ambient_colour()[1],
                    self.spec.ambient_colour()[2],
                ),
                brightness: self.spec.ambient_brightness(),
                ..GlobalAmbientLight::default()
            })
            .add_systems(Startup, spawn_horizon)
            .add_systems(Update, (place_fog, place_around_camera));
    }
}

/// Spawns the dome and the sea once, from the spec resource.
fn spawn_horizon(
    mut commands: Commands,
    spec: Res<HorizonSpec>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let dome_material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        unlit: true,
        // The dome's distance from the camera never changes, so fog would
        // wash the whole gradient towards the fog colour at once.
        fog_enabled: false,
        // The camera is inside the sphere and sees its back faces.
        cull_mode: None,
        ..StandardMaterial::default()
    });
    let sea_material = materials.add(StandardMaterial {
        base_color: Color::srgb(
            spec.sea_colour()[0],
            spec.sea_colour()[1],
            spec.sea_colour()[2],
        ),
        unlit: true,
        perceptual_roughness: 1.0,
        ..StandardMaterial::default()
    });
    commands.spawn((
        HorizonDome,
        Mesh3d(meshes.add(spec.dome_mesh())),
        MeshMaterial3d(dome_material),
        Transform::from_scale(Vec3::splat(spec.dome_radius_m())),
        Visibility::default(),
    ));
    commands.spawn((
        HorizonSea,
        Mesh3d(meshes.add(spec.sea_mesh())),
        MeshMaterial3d(sea_material),
        Transform::from_xyz(0.0, spec.sea_level_m(), 0.0),
        Visibility::default(),
    ));
}

/// Puts the scene's fog on every camera that does not carry one yet.
fn place_fog(
    spec: Res<HorizonSpec>,
    cameras: Query<Entity, (With<Camera3d>, Without<DistanceFog>)>,
    mut commands: Commands,
) {
    for entity in &cameras {
        commands.entity(entity).insert(DistanceFog {
            color: Color::srgb(
                spec.fog_colour()[0],
                spec.fog_colour()[1],
                spec.fog_colour()[2],
            ),
            // The fog is atmospheric, not a light effect: no sun glow.
            directional_light_color: Color::NONE,
            directional_light_exponent: 0.0,
            falloff: FogFalloff::Linear {
                start: spec.fog_start_m(),
                end: spec.fog_end_m(),
            },
        });
    }
}

/// Keeps the dome centred on the camera's world translation with an identity
/// rotation, and the sea under the camera's horizontal position.
///
/// This is F19 non-negotiable 3 applied to a scene with no `WorldOrigin`:
/// translation follows, orientation never does, so rolling the aircraft
/// cannot tilt the horizon.
type DomeTransforms<'w, 's> =
    Query<'w, 's, &'static mut Transform, (With<HorizonDome>, Without<HorizonSea>)>;
type SeaTransforms<'w, 's> =
    Query<'w, 's, &'static mut Transform, (With<HorizonSea>, Without<HorizonDome>)>;

fn place_around_camera(
    spec: Res<HorizonSpec>,
    cameras: Query<&GlobalTransform, With<Camera3d>>,
    mut domes: DomeTransforms<'_, '_>,
    mut seas: SeaTransforms<'_, '_>,
) {
    let Ok(camera) = cameras.single() else {
        return;
    };
    let translation = camera.translation();
    for mut transform in &mut domes {
        transform.translation = translation;
        transform.rotation = Quat::IDENTITY;
        transform.scale = Vec3::splat(spec.dome_radius_m());
    }
    for mut transform in &mut seas {
        transform.translation = Vec3::new(translation.x, spec.sea_level_m(), translation.z);
    }
}
