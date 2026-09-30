//! The deterministic frame capture: the acceptance scenario's "screenshot
//! same camera/tick twice under fixed comparison settings"
//! (`specs/F17-rendering-material-fidelity-and-scalable-presentation.md`,
//! stage `### F17-B`, AC02).
//!
//! A capture is what the renderer was handed for one tick: the comparison
//! settings, the camera, the tick, every surface's geometry, image and render
//! state in draw order, and everything that was *not* drawn with the reason
//! why. It is a value, not a picture, and it is built without a render world,
//! so the property AC02 asks for — two captures of the same camera and tick
//! are identical — is a property of the *adapter* rather than of a GPU driver.
//! That is the honest way to test it headless: what a screenshot would show
//! is decided entirely by the digests recorded here, and this stage has no
//! claim at all about pixel values.
//!
//! Three things make the capture worth comparing, and all three are refusals
//! rather than defaults:
//!
//! * **Fixed comparison settings.** Spec F17 non-negotiable 3: "Camera
//!   exposure, tonemapping and gamma are fixed in comparison mode." A
//!   capture is only produced from the fixed set ([`ComparisonSettings`]);
//!   anything else is [`CaptureError::SettingsNotFixed`], because a capture
//!   taken under unpinned settings compares nothing.
//! * **A complete scene.** Every index the plan names must have exactly one
//!   upload or one refusal, and every upload must be in the plan. A surface
//!   the plan draws but the scene has no upload for, an upload the plan never
//!   reaches, or two outcomes for one index are
//!   [`CaptureError::SceneIncomplete`] — the stale-list failures that would
//!   otherwise show up as "the same scene" that is not.
//! * **Reported refusals.** A surface the adapters refused is not drawn and
//!   not silently dropped: it appears in [`FrameCapture::refusals`] with its
//!   reason code, it is part of the capture's digest, and its phase stays
//!   present but empty. A frame with a hole in it is therefore visibly
//!   different from a frame that never had one. Spec F17 non-negotiable 2 asks
//!   for limitations to be reported instead of geometry hidden.
//!
//! The camera is a [`Projection`] next to the [`SceneView`] pose, because a
//! screenshot comparison that does not pin the field of view is not a
//! comparison. Both are `Designed` engine values, and [`FrameCapture:
//! :fingerprint`] pins them exactly.

use std::fmt;

use bevy::pbr::StandardMaterial;
use cs_assets::install::sha256;
use cs_content::mesh::{MeshPresentationUnknown, RenderMesh};
use cs_formats::texture::DecodedImage;
use cs_types::Tick;
use cs_types::evidence::ContentHash;

use crate::render::bevy_image::{ImageUpload, upload_image};
use crate::render::bevy_mesh::{GroupUpload, upload_group};
use crate::render::bevy_state::{MaterialGap, RenderState, render_state};
use crate::render::material::{Coverage, RenderPhase};
use crate::render::plan::{DrawItem, DrawItemKey, DrawPlan, SceneView, SortingLimitation};

/// The image tonemapping a comparison frame used.
///
/// New-engine vocabulary mirroring Bevy's `TonemappingMethod` subset that
/// matters for a fixed comparison. `None` is the fixed comparison value: any
/// tone curve is a presentation decision, and the sheet's fidelity mode
/// "preserves authored appearance", so the comparison baseline applies none.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tonemap {
    /// No tone curve: the linear framebuffer is written as it is.
    None,
    /// A filmic curve.
    Filmic,
}

impl Tonemap {
    /// Stable lowercase identifier.
    pub const fn code(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Filmic => "filmic",
        }
    }
}

/// The camera exposure, tone curve and gamma a comparison frame used, and the
/// only combination [`capture`] accepts.
///
/// The values are designed, not measured: F17 fixes *that* they are pinned
/// during a comparison, not what the original renderer used, and nothing here
/// asserts the latter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ComparisonSettings {
    exposure: f32,
    tonemap: Tonemap,
    gamma: f32,
    msaa_samples: u32,
}

/// The exposure a comparison frame uses: no exposure change.
pub const COMPARISON_EXPOSURE: f32 = 1.0;
/// The display gamma a comparison frame encodes with.
pub const COMPARISON_GAMMA: f32 = 2.2;
/// The sample count a comparison frame renders with: no antialiasing.
///
/// Antialiasing is a modern improvement, not evidence of original parity
/// (spec F17 non-negotiable 5), so the fidelity baseline renders at one
/// sample per pixel and the enhanced profiles (F17-C) opt into more.
pub const COMPARISON_MSAA_SAMPLES: u32 = 1;

impl ComparisonSettings {
    /// The fixed comparison set.
    pub const fn comparison() -> Self {
        Self {
            exposure: COMPARISON_EXPOSURE,
            tonemap: Tonemap::None,
            gamma: COMPARISON_GAMMA,
            msaa_samples: COMPARISON_MSAA_SAMPLES,
        }
    }

    /// A set with the given exposure, for the failure case that must be
    /// refused.
    pub const fn with_exposure(exposure: f32) -> Self {
        Self {
            exposure,
            ..Self::comparison()
        }
    }

    /// A set with the given sample count, for the failure case that must be
    /// refused.
    pub const fn with_msaa_samples(msaa_samples: u32) -> Self {
        Self {
            msaa_samples,
            ..Self::comparison()
        }
    }

    /// The camera exposure.
    pub const fn exposure(&self) -> f32 {
        self.exposure
    }

    /// The tone curve.
    pub const fn tonemap(&self) -> Tonemap {
        self.tonemap
    }

    /// The display gamma.
    pub const fn gamma(&self) -> f32 {
        self.gamma
    }

    /// Samples per pixel.
    pub const fn msaa_samples(&self) -> u32 {
        self.msaa_samples
    }

    /// Whether this is exactly the fixed comparison set.
    pub fn is_fixed(&self) -> bool {
        *self == Self::comparison()
    }
}

/// Why a camera projection was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectionError {
    /// A field is not finite.
    NonFinite,
    /// The vertical field of view is not in `(0, π)`.
    FieldOfView,
    /// The aspect ratio is not positive.
    AspectRatio,
    /// The near plane is not positive, or is not closer than the far plane.
    Clipping,
}

impl fmt::Display for ProjectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFinite => write!(f, "a projection field is not finite"),
            Self::FieldOfView => write!(f, "the vertical field of view is outside (0, pi)"),
            Self::AspectRatio => write!(f, "the aspect ratio is not positive"),
            Self::Clipping => write!(f, "the near plane is not inside (0, far)"),
        }
    }
}

impl std::error::Error for ProjectionError {}

/// The camera projection a capture pins: vertical field of view, aspect ratio
/// and the two clipping planes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Projection {
    fov_y_radians: f32,
    aspect_ratio: f32,
    near_m: f32,
    far_m: f32,
}

impl Projection {
    /// The comparison projection: 60° vertical, 4:3, a 0.1 m to 10 km range.
    ///
    /// Designed values, chosen to be ordinary rather than measured: F17
    /// pins the projection for a comparison but asserts nothing about the
    /// original camera.
    pub const fn comparison() -> Self {
        Self {
            fov_y_radians: std::f32::consts::FRAC_PI_3,
            aspect_ratio: 4.0 / 3.0,
            near_m: 0.1,
            far_m: 10_000.0,
        }
    }

    /// Validates a projection.
    ///
    /// # Errors
    ///
    /// [`ProjectionError`] for a non-finite field, a field of view outside
    /// `(0, π)`, a non-positive aspect ratio or a near plane that is not
    /// inside `(0, far)`.
    pub fn new(
        fov_y_radians: f32,
        aspect_ratio: f32,
        near_m: f32,
        far_m: f32,
    ) -> Result<Self, ProjectionError> {
        if ![fov_y_radians, aspect_ratio, near_m, far_m]
            .iter()
            .all(|value| value.is_finite())
        {
            return Err(ProjectionError::NonFinite);
        }
        if !(fov_y_radians > 0.0 && fov_y_radians < std::f32::consts::PI) {
            return Err(ProjectionError::FieldOfView);
        }
        if aspect_ratio <= 0.0 {
            return Err(ProjectionError::AspectRatio);
        }
        if !(near_m > 0.0 && near_m < far_m) {
            return Err(ProjectionError::Clipping);
        }
        Ok(Self {
            fov_y_radians,
            aspect_ratio,
            near_m,
            far_m,
        })
    }

    /// The vertical field of view in radians.
    pub const fn fov_y_radians(&self) -> f32 {
        self.fov_y_radians
    }

    /// Width divided by height.
    pub const fn aspect_ratio(&self) -> f32 {
        self.aspect_ratio
    }

    /// The near clipping distance in meters.
    pub const fn near_m(&self) -> f32 {
        self.near_m
    }

    /// The far clipping distance in meters.
    pub const fn far_m(&self) -> f32 {
        self.far_m
    }
}

/// One surface that reached the renderer: its geometry, its image, its render
/// state and the drawable material when one exists.
#[derive(Debug)]
pub struct SurfaceUpload {
    key: DrawItemKey,
    geometry: GroupUpload,
    state: RenderState,
    standard_material: Option<StandardMaterial>,
    material_gap: Option<MaterialGap>,
    image: Option<ImageUpload>,
}

impl SurfaceUpload {
    /// What one draw item needs at the renderer: the material group of its
    /// mesh, the render state its classified material reads out, and the
    /// canonical image it samples when it has one.
    pub fn new(
        key: DrawItemKey,
        geometry: GroupUpload,
        state: RenderState,
        image: Option<ImageUpload>,
    ) -> Self {
        let (standard_material, material_gap) = match state.to_standard_material() {
            Ok(material) => (Some(material), None),
            Err(gap) => (None, Some(gap)),
        };
        Self {
            key,
            geometry,
            state,
            standard_material,
            material_gap,
            image,
        }
    }

    /// The draw item this surface draws.
    pub const fn key(&self) -> &DrawItemKey {
        &self.key
    }

    /// The uploaded geometry.
    pub const fn geometry(&self) -> &GroupUpload {
        &self.geometry
    }

    /// The render state.
    pub const fn state(&self) -> &RenderState {
        &self.state
    }

    /// The drawable `StandardMaterial`, when this class has one.
    pub const fn standard_material(&self) -> Option<&StandardMaterial> {
        self.standard_material.as_ref()
    }

    /// Why this surface has no `StandardMaterial`, when it has none. The
    /// surface is still captured: the state above is complete.
    pub const fn material_gap(&self) -> Option<MaterialGap> {
        self.material_gap
    }

    /// The canonical image this surface samples, if it has one.
    pub const fn image(&self) -> Option<&ImageUpload> {
        self.image.as_ref()
    }
}

/// The reason codes [`upload_surface`] adds beyond the three adapters' own.
pub mod surface_codes {
    /// The material declares coverage that lives in a texture and the surface
    /// has no image. A missing texture may never become a default material
    /// (`docs/findings/2026-09-29-f10-c-02-gamez-material-records.md`).
    pub const MISSING_IMAGE: &str = "missing_image";
    /// The material's declared coverage source is not the image's.
    pub const COVERAGE_SOURCE_MISMATCH: &str = "coverage_source_mismatch";
    /// The material declares a constant opacity *and* carries an image whose
    /// own alpha channel would fight it. Overwriting the image's alpha with
    /// the constant is a coverage decision the image adapter does not make.
    pub const UNIFORM_COVERAGE_WITH_IMAGE: &str = "uniform_coverage_with_image";
}

/// What one submitted draw item needs at the renderer.
pub struct SceneSurface<'a> {
    /// The submitted item: its key, its classified material and its place in
    /// the scene.
    pub item: &'a DrawItem,
    /// The canonical mesh IR the item's group is uploaded from.
    pub mesh: &'a RenderMesh,
    /// Which material group of `mesh` this item draws.
    pub group: usize,
    /// The canonical image the item samples, when it has one.
    pub image: Option<&'a DecodedImage>,
    /// The presentation unknowns the content pipeline established are still
    /// open, carried into the upload unchanged.
    pub unknowns: &'a [MeshPresentationUnknown],
}

/// Runs the three adapters for one submitted draw item.
///
/// The result is an outcome, never an error: a refusal *is* the answer when a
/// fact is not established, and it carries every reason that applies, in
/// adapter order — geometry, then render state, then image, then the
/// coverage/image agreement. Nothing is defaulted to make a surface drawable.
pub fn upload_surface(surface: &SceneSurface<'_>) -> SceneOutcome {
    let key = surface.item.key().clone();
    let mut reasons: Vec<&'static str> = Vec::new();

    let geometry = match upload_group(surface.mesh, surface.group, surface.unknowns) {
        Ok(geometry) => Some(geometry),
        Err(error) => {
            reasons.push(error.code());
            None
        }
    };
    let state = match render_state(surface.item.material()) {
        Ok(state) => Some(state),
        Err(error) => {
            reasons.push(error.code());
            None
        }
    };
    // The sampler needs the material's declared addressing. When the state
    // was refused there is none, and the image adapter reports that itself
    // rather than inventing a mode.
    let addressing = state.as_ref().map(RenderState::address);
    let mut image = None;
    match surface.image {
        None => {}
        Some(source) => match upload_image(source, addressing) {
            Ok(uploaded) => image = Some(uploaded),
            Err(error) => reasons.push(error.code()),
        },
    }

    // The material's coverage and the image have to agree, or the surface has
    // no established coverage at all.
    match surface.item.material().coverage() {
        Coverage::Texture(source) => match image.as_ref() {
            None => reasons.push(surface_codes::MISSING_IMAGE),
            Some(uploaded) if uploaded.image_ref().alpha_source() != source => {
                reasons.push(surface_codes::COVERAGE_SOURCE_MISMATCH);
            }
            Some(_) => {}
        },
        Coverage::Uniform(_) if image.is_some() => {
            reasons.push(surface_codes::UNIFORM_COVERAGE_WITH_IMAGE);
        }
        Coverage::Opaque | Coverage::Uniform(_) | Coverage::Unknown => {}
    }

    match (geometry, state) {
        (Some(geometry), Some(state)) if reasons.is_empty() => {
            SceneOutcome::Uploaded(Box::new(SurfaceUpload::new(key, geometry, state, image)))
        }
        _ => SceneOutcome::Refused(SurfaceRefusal::new(key, reasons)),
    }
}

/// One surface the adapters refused, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SurfaceRefusal {
    key: DrawItemKey,
    reasons: Vec<&'static str>,
}

impl SurfaceRefusal {
    /// Records a refusal for `key` with one reason code per failed adapter,
    /// in adapter order.
    pub fn new(key: DrawItemKey, reasons: Vec<&'static str>) -> Self {
        Self { key, reasons }
    }

    /// The draw item that was not drawn.
    pub const fn key(&self) -> &DrawItemKey {
        &self.key
    }

    /// The reason codes, in adapter order: geometry, then state, then image,
    /// then the coverage/image agreement.
    pub fn reasons(&self) -> &[&'static str] {
        &self.reasons
    }
}

/// What the scene holds for one submitted draw item: an upload or a refusal.
///
/// The two are the same type because a refusal is a *result*, not an
/// exception: the geometry adapter, the state reader and the image adapter
/// each refuse with a reason code, and a surface that any of them refused is
/// reported next to the surfaces that drew.
#[derive(Debug)]
pub enum SceneOutcome {
    /// The surface reached the renderer. Boxed because a Bevy `Mesh` is two
    /// orders of magnitude larger than a refusal, and one per surface is
    /// cheaper than one large enum.
    Uploaded(Box<SurfaceUpload>),
    /// It did not, and here is every reason.
    Refused(SurfaceRefusal),
}

impl SceneOutcome {
    /// The draw item this outcome belongs to.
    pub fn key(&self) -> &DrawItemKey {
        match self {
            Self::Uploaded(upload) => upload.key(),
            Self::Refused(refusal) => refusal.key(),
        }
    }
}

/// Why a capture could not be taken.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CaptureError {
    /// The settings are not the fixed comparison set, so the capture would
    /// not be comparable to another.
    SettingsNotFixed {
        /// The exposure that was asked for.
        exposure_bits: u32,
        /// The tone curve that was asked for.
        tonemap: &'static str,
        /// The gamma that was asked for.
        gamma_bits: u32,
        /// The sample count that was asked for.
        msaa_samples: u32,
    },
    /// The scene and the plan disagree about which surfaces exist.
    SceneIncomplete {
        /// A stable code for the kind of disagreement.
        code: &'static str,
        /// The draw item it is about.
        key: DrawItemKey,
    },
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SettingsNotFixed {
                exposure_bits,
                tonemap,
                gamma_bits,
                msaa_samples,
            } => write!(
                f,
                "comparison settings must be the fixed set: asked for exposure {exposure_bits}, \
                 tonemap {tonemap}, gamma {gamma_bits}, {msaa_samples} samples per pixel"
            ),
            Self::SceneIncomplete { code, key } => {
                write!(f, "the scene does not match the draw plan at {key}: {code}")
            }
        }
    }
}

impl std::error::Error for CaptureError {}

/// The ways a scene and a plan can disagree.
pub mod scene_codes {
    /// The plan draws an index the scene has no outcome for.
    pub const MISSING_OUTCOME: &str = "plan_entry_without_scene_outcome";
    /// The outcome at an index names a different draw item than the plan's.
    pub const KEY_MISMATCH: &str = "scene_outcome_key_mismatch";
    /// The scene has an outcome no plan entry reaches.
    pub const UNUSED_OUTCOME: &str = "scene_outcome_without_plan_entry";
}

/// One surface as it appears in the capture: identity, digests, phase and
/// the view depth the sort used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapturedSurface {
    key: DrawItemKey,
    phase: RenderPhase,
    depth_m_bits: u32,
    geometry: ContentHash,
    state: ContentHash,
    image: Option<ContentHash>,
    material_gap: Option<&'static str>,
}

/// One pass of the capture: a render phase and the surfaces that draw in it,
/// in draw order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapturedPass {
    phase: RenderPhase,
    surfaces: Vec<CapturedSurface>,
}

impl CapturedPass {
    /// The phase this pass draws.
    pub const fn phase(&self) -> RenderPhase {
        self.phase
    }

    /// The surfaces, in draw order.
    pub fn surfaces(&self) -> &[CapturedSurface] {
        &self.surfaces
    }
}

/// One tick's frame, captured.
#[derive(Clone, Debug, PartialEq)]
pub struct FrameCapture {
    settings: ComparisonSettings,
    projection: Projection,
    position_m: [f32; 3],
    forward: [f32; 3],
    tick: Tick,
    passes: Vec<CapturedPass>,
    refusals: Vec<SurfaceRefusal>,
    limitations: Vec<SortingLimitation>,
    fingerprint: ContentHash,
}

/// Captures one tick's frame.
///
/// `outcomes` is the scene in **submission order**: outcome `i` belongs to
/// the draw item the plan's entry `i` names, and the capture refuses to
/// proceed when the two disagree (see [`CaptureError::SceneIncomplete`]).
/// The capture keeps the plan's order — it never re-sorts — so the ordering
/// the plan established is the ordering that is captured.
///
/// # Errors
///
/// [`CaptureError::SettingsNotFixed`] when `settings` is not the fixed
/// comparison set, and [`CaptureError::SceneIncomplete`] when the scene and
/// the plan do not describe the same surfaces.
pub fn capture(
    outcomes: &[SceneOutcome],
    plan: &DrawPlan,
    view: &SceneView,
    projection: &Projection,
    tick: Tick,
    settings: &ComparisonSettings,
) -> Result<FrameCapture, CaptureError> {
    if !settings.is_fixed() {
        return Err(CaptureError::SettingsNotFixed {
            exposure_bits: settings.exposure().to_bits(),
            tonemap: settings.tonemap().code(),
            gamma_bits: settings.gamma().to_bits(),
            msaa_samples: settings.msaa_samples(),
        });
    }

    // Every plan entry needs an outcome at its own index, and every outcome
    // needs a plan entry that reaches it. Both directions are checked: an
    // outcome the plan never reaches is a leftover from an earlier scene, and
    // an index with no outcome is a surface that silently would not draw.
    let mut drawn: Vec<Option<CapturedSurface>> = vec![None; outcomes.len()];
    let mut planned: Vec<bool> = vec![false; outcomes.len()];
    let mut refusals: Vec<SurfaceRefusal> = Vec::new();
    for entry in plan.entries() {
        let outcome = outcomes
            .get(entry.item)
            .ok_or(CaptureError::SceneIncomplete {
                code: scene_codes::MISSING_OUTCOME,
                key: entry.key.clone(),
            })?;
        if outcome.key() != &entry.key {
            return Err(CaptureError::SceneIncomplete {
                code: scene_codes::KEY_MISMATCH,
                key: entry.key.clone(),
            });
        }
        planned[entry.item] = true;
        match outcome {
            SceneOutcome::Uploaded(upload) => {
                drawn[entry.item] = Some(CapturedSurface {
                    key: entry.key.clone(),
                    phase: entry.phase,
                    depth_m_bits: entry.depth_m.to_bits(),
                    geometry: upload.geometry().fingerprint(),
                    state: upload.state().fingerprint(),
                    image: upload.image().map(ImageUpload::fingerprint),
                    material_gap: upload.material_gap().map(MaterialGap::code),
                });
            }
            SceneOutcome::Refused(refusal) => refusals.push(refusal.clone()),
        }
    }
    if let Some(index) = planned.iter().position(|reached| !reached) {
        return Err(CaptureError::SceneIncomplete {
            code: scene_codes::UNUSED_OUTCOME,
            key: outcomes[index].key().clone(),
        });
    }

    // Passes in the fixed phase order, each in the plan's draw order.
    let mut passes = Vec::with_capacity(RenderPhase::ALL.len());
    for phase in RenderPhase::ALL {
        let surfaces = plan
            .entries()
            .iter()
            .filter(|entry| entry.phase == phase)
            .filter_map(|entry| drawn[entry.item].clone())
            .collect();
        passes.push(CapturedPass { phase, surfaces });
    }
    // Refusals ordered by draw item key, so the report of what did not draw is
    // the same list whichever order the plan happened to draw the rest in.
    refusals.sort_by(|a, b| a.key.cmp(&b.key));

    // The digest is taken over the finished capture, so it cannot miss a
    // field or cover one twice.
    let mut capture = FrameCapture {
        settings: *settings,
        projection: *projection,
        position_m: view.position_m(),
        forward: view.forward(),
        tick,
        passes,
        refusals,
        limitations: plan.limitations().to_vec(),
        fingerprint: ContentHash::from_bytes([0; 32]),
    };
    capture.fingerprint = capture_fingerprint(&capture);
    Ok(capture)
}

fn capture_fingerprint(capture: &FrameCapture) -> ContentHash {
    let FrameCapture {
        settings,
        projection,
        position_m,
        forward,
        tick,
        passes,
        refusals,
        limitations,
        ..
    } = capture;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"cs/render/frame_capture/v1\0");
    for value in [
        settings.exposure().to_bits(),
        settings.gamma().to_bits(),
        settings.msaa_samples(),
    ] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(settings.tonemap().code().as_bytes());
    bytes.push(0);
    for value in [
        projection.fov_y_radians().to_bits(),
        projection.aspect_ratio().to_bits(),
        projection.near_m().to_bits(),
        projection.far_m().to_bits(),
    ] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for value in position_m.iter().chain(forward.iter()) {
        bytes.extend_from_slice(&value.to_bits().to_le_bytes());
    }
    bytes.extend_from_slice(&tick.0.to_le_bytes());
    for pass in passes {
        bytes.extend_from_slice(pass.phase.code().as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(&(pass.surfaces.len() as u32).to_le_bytes());
        for surface in &pass.surfaces {
            bytes.extend_from_slice(surface.key.as_str().as_bytes());
            bytes.push(0);
            bytes.extend_from_slice(&surface.depth_m_bits.to_le_bytes());
            bytes.extend_from_slice(surface.geometry.as_bytes());
            bytes.extend_from_slice(surface.state.as_bytes());
            match surface.image {
                None => bytes.push(0),
                Some(hash) => {
                    bytes.push(1);
                    bytes.extend_from_slice(hash.as_bytes());
                }
            }
            match surface.material_gap {
                None => bytes.push(0),
                Some(code) => {
                    bytes.push(1);
                    bytes.extend_from_slice(code.as_bytes());
                }
            }
            bytes.push(0);
        }
    }
    bytes.extend_from_slice(&(refusals.len() as u32).to_le_bytes());
    for refusal in refusals {
        bytes.extend_from_slice(refusal.key.as_str().as_bytes());
        bytes.push(0);
        for reason in &refusal.reasons {
            bytes.extend_from_slice(reason.as_bytes());
            bytes.push(0);
        }
    }
    bytes.extend_from_slice(&(limitations.len() as u32).to_le_bytes());
    for limitation in limitations {
        bytes.extend_from_slice(limitation.code().as_bytes());
        bytes.push(0);
    }
    sha256(&bytes)
}

impl FrameCapture {
    /// The settings this frame was captured under.
    pub const fn settings(&self) -> &ComparisonSettings {
        &self.settings
    }

    /// The projection this frame was captured under.
    pub const fn projection(&self) -> &Projection {
        &self.projection
    }

    /// The view position in meters.
    pub const fn position_m(&self) -> [f32; 3] {
        self.position_m
    }

    /// The normalized view forward.
    pub const fn forward(&self) -> [f32; 3] {
        self.forward
    }

    /// The tick this frame is.
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    /// The passes, in the fixed phase order.
    pub fn passes(&self) -> &[CapturedPass] {
        &self.passes
    }

    /// The surfaces of one phase, in draw order.
    pub fn pass(&self, phase: RenderPhase) -> Option<&[CapturedSurface]> {
        self.passes
            .iter()
            .find(|pass| pass.phase == phase)
            .map(CapturedPass::surfaces)
    }

    /// Every surface in draw order across all passes.
    pub fn surfaces(&self) -> impl Iterator<Item = &CapturedSurface> {
        self.passes.iter().flat_map(|pass| pass.surfaces.iter())
    }

    /// The surfaces that did not draw, ordered by draw item key.
    pub fn refusals(&self) -> &[SurfaceRefusal] {
        &self.refusals
    }

    /// The plan's sorting limitations, carried through so a comparison
    /// covers the ordering's known limits and not only its order.
    pub fn limitations(&self) -> &[SortingLimitation] {
        &self.limitations
    }

    /// A digest of everything above: the settings, the camera, the tick, every
    /// surface's identity, geometry, state and image in draw order, the
    /// refusals and the sorting limitations.
    ///
    /// Two captures of the same camera and tick under the fixed settings are
    /// equal exactly when this digest matches. It is an `Artifact`
    /// fingerprint: it pins the adapter's output, never original data.
    pub const fn fingerprint(&self) -> ContentHash {
        self.fingerprint
    }
}

impl CapturedSurface {
    /// The draw item.
    pub const fn key(&self) -> &DrawItemKey {
        &self.key
    }

    /// The phase it draws in.
    pub const fn phase(&self) -> RenderPhase {
        self.phase
    }

    /// The view depth the sort used.
    pub fn depth_m(&self) -> f32 {
        f32::from_bits(self.depth_m_bits)
    }

    /// The digest of the uploaded geometry.
    pub const fn geometry(&self) -> ContentHash {
        self.geometry
    }

    /// The digest of the render state.
    pub const fn state(&self) -> ContentHash {
        self.state
    }

    /// The digest of the canonical image, when the surface has one.
    pub const fn image(&self) -> Option<ContentHash> {
        self.image
    }

    /// Why this surface has no drawable `StandardMaterial`, when it has none.
    pub const fn material_gap(&self) -> Option<&'static str> {
        self.material_gap
    }
}
