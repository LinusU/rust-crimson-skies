//! A real GPU capture of a mesh posed by an evaluated animation sample,
//! offscreen (`specs/F20-object-animation-and-authored-destruction-states.md`,
//! stage `### F20-D`; shared contract `docs/contracts/CLI-EVIDENCE.md`).
//!
//! # What this is for
//!
//! F20-D declares `gpu` as a required capability, so the runtime's animation
//! output has to reach a real rendered frame, not just an asserted component
//! value. [`capture_animated_pose`] takes a production
//! [`RenderMesh`] and a [`PoseSample`] — the value the playback publishes as
//! [`crate::animation::NodeAnimatedPose`] — and draws that mesh at that pose
//! on the real renderer, offscreen, writing a PNG and reporting the measured
//! facts of the frame that came back.
//!
//! It is the animation-path sibling of
//! [`crate::world::gpu_capture::capture_world_mesh`]: same declared framing,
//! same measured refusals, and the same production upload adapter
//! ([`crate::render::bevy_mesh::upload_group_parts`]) — the difference is the
//! pose. The mesh entity carries the sample's rotation, translation and scale
//! as its own [`Transform`]; Bevy's transform propagation derives the
//! `GlobalTransform` the renderer draws from it, so what the frame shows is
//! exactly the pose the evaluator produced.
//!
//! Two captures of one mesh at two evaluated poses whose digests differ are
//! the evidence that the transform track's output actually reaches the
//! render pipeline; one capture alone proves only that the mesh drew.
//!
//! # What is **not** claimed
//!
//! * **Not original animation data.** The original animation containers are
//!   still undecoded (F13), so the pose comes from the *designed* evaluator
//!   on a declared clip — the capture measures the render path, never the
//!   original's animation semantics.
//! * **Not a placement or a metric.** The mesh is drawn at the pose alone,
//!   in stored units; nothing about a level's composition follows.
//! * **Not the original's appearance.** The flat colour and key light are
//!   declared here, as in the world capture; F17's presentation unknowns are
//!   open and the artifact is about geometry reaching the frame.
//!
//! # The refusals
//!
//! Every way this can produce a file that is not evidence of a drawn frame is
//! a named error, never a written PNG — the same contract as the world
//! capture, plus the one the pose adds:
//!
//! * [`PoseCaptureError::UnrepresentablePose`] — a `PoseSample` component did
//!   not survive the f64 → f32 cast, so no frame could carry it.

use std::fmt;
use std::path::Path;
use std::sync::Mutex;

use bevy::app::PluginGroup;
use bevy::asset::RenderAssetUsages;
use bevy::camera::{ClearColorConfig, PerspectiveProjection, Projection, RenderTarget};
use bevy::image::{Image, ImageSampler};
use bevy::math::{Mat4, Quat, Vec3};
use bevy::mesh::Mesh;
use bevy::prelude::{
    App, Assets, Camera, Camera3d, Color, DefaultPlugins, DirectionalLight, Handle, Mesh3d,
    MeshMaterial3d, On, Res, Resource, StandardMaterial, Transform, WindowPlugin, default,
};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use cs_assets::install::sha256;
use cs_content::mesh::{MeshPresentationUnknown, RenderMesh};
use cs_sim::animated_object::PoseSample;
use cs_types::evidence::ContentHash;

use crate::render::bevy_mesh::{GroupUpload, upload_group_parts};

/// The capture frame's width in pixels, matching
/// [`crate::world::CAPTURE_WIDTH`] so animation and world captures are the
/// same artifact size.
pub const POSE_CAPTURE_WIDTH: u32 = crate::world::CAPTURE_WIDTH;

/// The capture frame's height in pixels. As [`POSE_CAPTURE_WIDTH`].
pub const POSE_CAPTURE_HEIGHT: u32 = crate::world::CAPTURE_HEIGHT;

/// The clear colour and the flat mesh colour, identical to the world
/// capture's declared values so a frame of either kind is measured against
/// the same background.
const CLEAR_COLOR: [f32; 4] = [0.043, 0.055, 0.075, 1.0];
const MESH_COLOR: [f32; 4] = [0.78, 0.74, 0.66, 1.0];
const KEY_LIGHT_ILLUMINANCE: f32 = 12_000.0;

/// Framing, clip-plane and view-direction constants, identical to the world
/// capture's declared values
/// ([`crate::world::FRAMING_DISTANCE_FACTOR`] and the private constants
/// beside it): the pose changes where the geometry lands, never how the
/// capture frames it.
const FRAMING_DISTANCE_FACTOR: f64 = crate::world::FRAMING_DISTANCE_FACTOR;
const NEAR_PLANE_FRACTION: f64 = 0.01;
const FAR_PLANE_FACTOR: f64 = 4.0;
const CAPTURE_VIEW_DIRECTION: [f64; 3] = [0.5, 0.6, 0.62];

/// How many frames the capture waits before asking for the screenshot, and
/// the bound on updates one capture may drive — the world capture's measured
/// values.
const WARMUP_FRAMES: u32 = 4;
const MAX_CAPTURE_UPDATES: u32 = 24;

/// What one posed capture produced, all of it measured from the frame that
/// came back.
#[derive(Clone, Debug)]
pub struct PoseCapture {
    /// The provenance label the artifact is filed under.
    pub label: String,
    /// The pose the mesh was drawn at — the evaluated sample, recorded so
    /// the artifact itself says what was drawn.
    pub pose: PoseSample,
    /// The adapter the renderer actually selected, as the driver reported it.
    pub adapter: String,
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// How many distinct luminance levels the frame holds. One means the
    /// whole frame is the background.
    pub distinct_luminance: usize,
    /// Pixels that differ from the clear colour: the geometry's own coverage.
    pub covered_pixels: usize,
    /// [`Self::covered_pixels`] over the frame's pixel count.
    pub covered_permille: u32,
    /// Triangles submitted for this frame.
    pub triangles: usize,
    /// Render vertices uploaded for this frame.
    pub vertices: usize,
    /// Material groups the mesh was drawn as, one draw each.
    pub groups: usize,
    /// SHA-256 of the written PNG's bytes.
    pub png_sha256: ContentHash,
    /// How many bytes the PNG has.
    pub png_bytes: u64,
    /// Where the PNG was written.
    pub png: String,
}

impl PoseCapture {
    /// Whether the frame drew anything at all.
    ///
    /// A capture that came back with a single luminance level is a uniform
    /// frame and is refused by [`capture_animated_pose`], so a `PoseCapture`
    /// that exists has already passed this; the predicate is a method so the
    /// refusal and the check cannot drift apart.
    #[must_use]
    pub const fn drew_geometry(&self) -> bool {
        self.distinct_luminance > 1 && self.covered_pixels > 0
    }
}

/// Why a posed capture could not be produced.
#[derive(Debug)]
pub enum PoseCaptureError {
    /// The renderer got no adapter, so no frame was drawn at all.
    NoAdapter,
    /// The mesh holds no drawable triangle.
    EmptyMesh {
        /// The request's label.
        label: String,
        /// The material groups the mesh does have.
        groups: usize,
    },
    /// Every stored corner of the mesh lands on one point — either stored, or
    /// after the pose's scale collapsed it — so there is no frame to put it
    /// in.
    DegenerateBounds {
        /// The request's label.
        label: String,
    },
    /// A `PoseSample` component did not survive the f64 → f32 cast: the
    /// pose cannot be carried into the render affine, so no frame of it
    /// exists.
    UnrepresentablePose {
        /// The request's label.
        label: String,
    },
    /// The renderer ran but produced no image.
    NoScreenshotCaptured {
        /// How many updates the capture drove before giving up.
        updates: u32,
    },
    /// The frame came back and every pixel is the background: the mesh was
    /// not drawn.
    UniformFrame {
        /// The request's label.
        label: String,
        /// How many distinct luminance levels the frame held. Always one.
        distinct_luminance: usize,
        /// Pixels that differ from the background, and how many pixels there
        /// are — so a near miss shows its size rather than reading as zero.
        covered_pixels: usize,
        /// The frame's pixel count.
        total_pixels: usize,
    },
    /// A material group refused to upload, with the adapter's own reason.
    GroupRefused {
        /// The request's label.
        label: String,
        /// The material group that would not upload.
        material_group: usize,
        /// The adapter's message, verbatim.
        reason: String,
    },
    /// The PNG could not be written or read back.
    Io {
        /// The path involved.
        path: String,
        /// The operating system's message, verbatim.
        reason: String,
    },
}

impl fmt::Display for PoseCaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoAdapter => {
                f.write_str("the renderer selected no GPU adapter, so no frame was drawn")
            }
            Self::EmptyMesh { label, groups } => write!(
                f,
                "{label} holds no drawable triangle ({groups} material groups), so a frame of it \
                 would be empty however well it was framed"
            ),
            Self::DegenerateBounds { label } => write!(
                f,
                "{label} has every corner on one point after the pose, so it has no bounds to \
                 frame"
            ),
            Self::UnrepresentablePose { label } => write!(
                f,
                "{label}'s pose does not fit the f32 render affine, so no frame can carry it"
            ),
            Self::NoScreenshotCaptured { updates } => write!(
                f,
                "the renderer ran {updates} updates and produced no captured image, so no frame \
                 came back"
            ),
            Self::UniformFrame {
                label,
                distinct_luminance,
                covered_pixels,
                total_pixels,
            } => write!(
                f,
                "{label} rendered a frame with {distinct_luminance} distinct luminance level and \
                 {covered_pixels} of {total_pixels} pixels off the background, i.e. the posed \
                 geometry was not drawn"
            ),
            Self::GroupRefused {
                label,
                material_group,
                reason,
            } => write!(
                f,
                "{label} material group {material_group} would not upload: {reason}"
            ),
            Self::Io { path, reason } => {
                write!(f, "the capture image {path} could not be used: {reason}")
            }
        }
    }
}

impl std::error::Error for PoseCaptureError {}

/// One posed mesh asked for: what to call it, the render mesh itself, the
/// pose to draw it at and where its PNG goes.
pub struct PoseCaptureRequest<'a> {
    /// The provenance label, used in refusal messages and the artifact's own
    /// record — e.g. the clip and tick the pose was evaluated from.
    pub label: &'a str,
    /// The production render mesh, straight from the content layer.
    pub render: &'a RenderMesh,
    /// The presentation unknowns the content pipeline reported for it,
    /// handed to the upload adapter unchanged.
    pub unknowns: &'a [MeshPresentationUnknown],
    /// The evaluated pose to draw the mesh at.
    pub pose: PoseSample,
    /// Where the PNG is written. The parent directory must exist.
    pub png: &'a Path,
}

/// The frame the capture app renders into, handed to the driver loop.
#[derive(Resource, Clone)]
struct CaptureTarget {
    image: Handle<Image>,
}

/// What the observer recorded about the frame that came back.
#[derive(Resource, Default)]
struct CapturedFrame(Mutex<Option<FrameFacts>>);

/// The measured facts about one frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FrameFacts {
    width: u32,
    height: u32,
    distinct_luminance: usize,
    covered_pixels: usize,
}

/// The pose as a Bevy [`Transform`]: the sample's own translation, rotation
/// and scale.
///
/// `None` when a component does not survive the f64 → f32 cast — the pose is
/// then unrepresentable rather than approximated.
fn pose_transform(pose: &PoseSample) -> Option<Transform> {
    let [rx, ry, rz, rw] = pose.rotation().components();
    let t = pose.translation_m();
    let s = pose.scale();
    let transform = Transform {
        translation: Vec3::new(t[0] as f32, t[1] as f32, t[2] as f32),
        rotation: Quat::from_xyzw(rx as f32, ry as f32, rz as f32, rw as f32),
        scale: Vec3::new(s[0] as f32, s[1] as f32, s[2] as f32),
    };
    (transform.translation.is_finite()
        && transform.rotation.is_finite()
        && transform.scale.is_finite())
    .then_some(transform)
}

/// The mesh's stored AABB pushed through the pose, in posed space.
///
/// The eight corners of the stored bounds are transformed rather than the
/// centre/extents pair, so a rotation or a non-uniform scale inflates the
/// frame correctly instead of clipping the drawn geometry.
fn posed_bounds(render: &RenderMesh, matrix: Mat4) -> Option<([f32; 3], [f32; 3])> {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    let mut any = false;
    for vertex in render.vertices() {
        for axis in 0..3 {
            let value = vertex.position[axis];
            if value.is_nan() {
                continue;
            }
            min[axis] = min[axis].min(value);
            max[axis] = max[axis].max(value);
        }
    }
    if min.iter().any(|value| !value.is_finite()) || max.iter().any(|v| !v.is_finite()) {
        return None;
    }
    let mut posed_min = [f32::INFINITY; 3];
    let mut posed_max = [f32::NEG_INFINITY; 3];
    for index in 0..8 {
        let corner = Vec3::new(
            if index & 1 == 0 { min[0] } else { max[0] },
            if index & 2 == 0 { min[1] } else { max[1] },
            if index & 4 == 0 { min[2] } else { max[2] },
        );
        let posed = matrix.transform_point3(corner);
        if !posed.is_finite() {
            continue;
        }
        any = true;
        for axis in 0..3 {
            posed_min[axis] = posed_min[axis].min(posed[axis]);
            posed_max[axis] = posed_max[axis].max(posed[axis]);
        }
    }
    any.then_some((posed_min, posed_max))
}

/// Renders `render` at `pose` on the real GPU and writes its PNG.
///
/// `png`'s parent directory must exist; the file is written by the renderer's
/// own screenshot path and then read back for its digest, so the digest in
/// the capture is of the file on disk rather than of a buffer this module
/// imagined.
///
/// # Errors
///
/// Every [`PoseCaptureError`], each of which leaves no PNG behind: an empty
/// mesh, degenerate bounds, an unrepresentable pose, a refused material
/// group, no adapter, no image, a uniform frame, or a read/write failure.
pub fn capture_animated_pose(
    request: &PoseCaptureRequest<'_>,
) -> Result<PoseCapture, PoseCaptureError> {
    let uploads = upload_all(request)?;
    let transform = pose_transform(&request.pose).ok_or(PoseCaptureError::UnrepresentablePose {
        label: request.label.to_owned(),
    })?;
    let matrix = Mat4::from_scale_rotation_translation(
        transform.scale,
        transform.rotation,
        transform.translation,
    );
    let Some((min, max)) = posed_bounds(request.render, matrix) else {
        return Err(PoseCaptureError::DegenerateBounds {
            label: request.label.to_owned(),
        });
    };
    let extent = (0..3).map(|axis| max[axis] - min[axis]);
    if !extent.clone().any(|side| side > 0.0) {
        return Err(PoseCaptureError::DegenerateBounds {
            label: request.label.to_owned(),
        });
    }

    let mut app = App::new();
    app.init_resource::<CapturedFrame>();
    // No window: the frame is rendered into an image asset and read back, so
    // the capture needs an adapter and not a display.
    app.add_plugins(
        DefaultPlugins
            .build()
            .disable::<bevy::winit::WinitPlugin>()
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: bevy::window::ExitCondition::DontExit,
                close_when_requested: false,
                ..WindowPlugin::default()
            }),
    );
    app.finish();
    app.cleanup();

    let target = spawn_scene(&mut app, &uploads, transform, (min, max));
    drive_capture(&mut app, target, request)?;

    // Every refusal from here on **removes the PNG the renderer already
    // wrote**: the screenshot observer saves the frame the moment it
    // arrives, before this function has looked at it, so without the
    // removal a uniform frame would leave a file on disk that reads exactly
    // like a good capture.
    let facts = {
        let recorded = app.world().resource::<CapturedFrame>();
        let guard = recorded.0.lock().map_err(|_| {
            discard_capture(request.png);
            PoseCaptureError::NoScreenshotCaptured {
                updates: MAX_CAPTURE_UPDATES,
            }
        })?;
        match *guard {
            Some(facts) => facts,
            None => {
                discard_capture(request.png);
                return Err(PoseCaptureError::NoScreenshotCaptured {
                    updates: MAX_CAPTURE_UPDATES,
                });
            }
        }
    };
    if facts.distinct_luminance <= 1 {
        discard_capture(request.png);
        return Err(PoseCaptureError::UniformFrame {
            label: request.label.to_owned(),
            distinct_luminance: facts.distinct_luminance,
            covered_pixels: facts.covered_pixels,
            total_pixels: facts.width as usize * facts.height as usize,
        });
    }

    let bytes = std::fs::read(request.png).map_err(|error| PoseCaptureError::Io {
        path: request.png.display().to_string(),
        reason: error.to_string(),
    })?;
    let (vertices, triangles) = upload_counts(&uploads);
    Ok(PoseCapture {
        label: request.label.to_owned(),
        pose: request.pose,
        adapter: adapter_name(&app),
        width: facts.width,
        height: facts.height,
        distinct_luminance: facts.distinct_luminance,
        covered_pixels: facts.covered_pixels,
        covered_permille: covered_permille(facts.covered_pixels, facts.width, facts.height),
        triangles,
        vertices,
        groups: uploads.len(),
        png_sha256: sha256(&bytes),
        png_bytes: bytes.len() as u64,
        png: request.png.display().to_string(),
    })
}

/// Uploads every material group of the mesh, or names the first that refused.
fn upload_all(request: &PoseCaptureRequest<'_>) -> Result<Vec<Mesh>, PoseCaptureError> {
    if request.render.triangles().is_empty() {
        return Err(PoseCaptureError::EmptyMesh {
            label: request.label.to_owned(),
            groups: request.render.groups().len(),
        });
    }
    let mut meshes = Vec::new();
    for group in 0..request.render.groups().len() {
        let parts =
            upload_group_parts(request.render, group, request.unknowns).map_err(|error| {
                PoseCaptureError::GroupRefused {
                    label: request.label.to_owned(),
                    material_group: group,
                    reason: error.to_string(),
                }
            })?;
        meshes.extend(parts.into_iter().map(GroupUpload::into_mesh));
    }
    Ok(meshes)
}

/// The total vertex and triangle count the uploads will draw.
fn upload_counts(uploads: &[Mesh]) -> (usize, usize) {
    let mut vertices = 0;
    let mut triangles = 0;
    for mesh in uploads {
        vertices += mesh.count_vertices();
        triangles += mesh.indices().map_or(0, |indices| indices.len() / 3);
    }
    (vertices, triangles)
}

/// Spawns the camera, the key light, one entity per material group at the
/// pose, and the render target the frame is drawn into.
fn spawn_scene(
    app: &mut App,
    uploads: &[Mesh],
    transform: Transform,
    bounds: ([f32; 3], [f32; 3]),
) -> CaptureTarget {
    let (min, max) = bounds;
    let centre = Vec3::new(
        (min[0] + max[0]) * 0.5,
        (min[1] + max[1]) * 0.5,
        (min[2] + max[2]) * 0.5,
    );
    // The bounding-sphere radius of the posed AABB, so `radius` means what
    // `FRAMING_DISTANCE_FACTOR` says it means whichever axis is longest.
    let radius = 0.5
        * (0..3)
            .map(|axis| f64::from(max[axis] - min[axis]).powi(2))
            .sum::<f64>()
            .sqrt();
    let distance = radius * FRAMING_DISTANCE_FACTOR;
    let unit = {
        let length = CAPTURE_VIEW_DIRECTION
            .iter()
            .map(|component| component * component)
            .sum::<f64>()
            .sqrt();
        [
            CAPTURE_VIEW_DIRECTION[0] / length,
            CAPTURE_VIEW_DIRECTION[1] / length,
            CAPTURE_VIEW_DIRECTION[2] / length,
        ]
    };
    let eye = centre + Vec3::new(unit[0] as f32, unit[1] as f32, unit[2] as f32) * distance as f32;

    let image = capture_image();
    let handle = app.world_mut().resource_mut::<Assets<Image>>().add(image);

    app.world_mut().spawn((
        Camera3d::default(),
        Camera {
            clear_color: ClearColorConfig::Custom(Color::srgba(
                CLEAR_COLOR[0],
                CLEAR_COLOR[1],
                CLEAR_COLOR[2],
                CLEAR_COLOR[3],
            )),
            ..default()
        },
        RenderTarget::Image(handle.clone().into()),
        Projection::Perspective(PerspectiveProjection {
            near: (distance * NEAR_PLANE_FRACTION) as f32,
            far: (distance * FAR_PLANE_FACTOR) as f32,
            ..PerspectiveProjection::default()
        }),
        Transform::from_xyz(eye.x, eye.y, eye.z).looking_at(centre, Vec3::Y),
    ));
    app.world_mut().spawn((
        DirectionalLight {
            illuminance: KEY_LIGHT_ILLUMINANCE,
            ..default()
        },
        Transform::from_xyz(eye.x, eye.y + radius as f32, eye.z).looking_at(centre, Vec3::Y),
    ));

    let surface: Handle<StandardMaterial> = app
        .world_mut()
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial {
            base_color: Color::srgb(MESH_COLOR[0], MESH_COLOR[1], MESH_COLOR[2]),
            metallic: 0.0,
            // No back-face culling, as in the world capture: the artifact is
            // a geometry witness, and which winding the original treated as
            // front-facing is F17's open question.
            cull_mode: None,
            ..default()
        });
    for mesh in uploads {
        let handle = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .add(mesh.clone());
        // The pose is carried as the entity's own `Transform`, which the
        // render pipeline draws after Bevy's propagation derives its
        // `GlobalTransform`. Spawning a bare `GlobalTransform` here instead
        // would leave the `Transform` that `Mesh3d` requires at its identity
        // default, and propagation would copy that identity over the pose —
        // the frame would show the mesh at the origin, not at the pose.
        app.world_mut()
            .spawn((Mesh3d(handle), MeshMaterial3d(surface.clone()), transform));
    }

    CaptureTarget { image: handle }
}

/// The render target the frame is drawn into and read back from.
fn capture_image() -> Image {
    let size = Extent3d {
        width: POSE_CAPTURE_WIDTH,
        height: POSE_CAPTURE_HEIGHT,
        depth_or_array_layers: 1,
    };
    let mut image = Image::new_fill(
        size,
        TextureDimension::D2,
        &[
            (CLEAR_COLOR[0] * 255.0) as u8,
            (CLEAR_COLOR[1] * 255.0) as u8,
            (CLEAR_COLOR[2] * 255.0) as u8,
            255,
        ],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.usage = TextureUsages::COPY_DST
        | TextureUsages::COPY_SRC
        | TextureUsages::TEXTURE_BINDING
        | TextureUsages::RENDER_ATTACHMENT;
    image.sampler = ImageSampler::linear();
    image
}

/// Drives the app until the screenshot comes back or the updates run out.
fn drive_capture(
    app: &mut App,
    target: CaptureTarget,
    request: &PoseCaptureRequest<'_>,
) -> Result<(), PoseCaptureError> {
    let png = request.png.to_path_buf();
    let observer = move |captured: On<ScreenshotCaptured>, frame: Res<CapturedFrame>| {
        if captured.image.data.is_some() {
            let image = captured.image.clone();
            *frame.0.lock().expect("the capture frame mutex") = Some(measure(&image));
        }
        save_to_disk(png.clone())(captured);
    };
    app.add_observer(observer);
    for update in 0..MAX_CAPTURE_UPDATES {
        if update == WARMUP_FRAMES {
            app.world_mut()
                .spawn(Screenshot::image(target.image.clone()));
        }
        app.update();
        if app
            .world()
            .resource::<CapturedFrame>()
            .0
            .lock()
            .expect("the capture frame mutex")
            .is_some()
        {
            return Ok(());
        }
    }
    Err(PoseCaptureError::NoScreenshotCaptured {
        updates: MAX_CAPTURE_UPDATES,
    })
}

/// Removes a capture the renderer already wrote, so no refusal leaves a file.
fn discard_capture(png: &Path) {
    let _ = std::fs::remove_file(png);
}

/// The measured facts of one frame: its size, how many distinct luminance
/// levels it holds and how many pixels differ from the background.
fn measure(image: &Image) -> FrameFacts {
    let data = image.data.as_ref().expect("a captured frame has data");
    let clear = [
        (CLEAR_COLOR[0] * 255.0).round() as u8,
        (CLEAR_COLOR[1] * 255.0).round() as u8,
        (CLEAR_COLOR[2] * 255.0).round() as u8,
    ];
    let mut levels: std::collections::BTreeSet<u8> = std::collections::BTreeSet::new();
    let mut covered = 0_usize;
    for pixel in data.as_chunks::<4>().0 {
        let luminance = ((29 * u32::from(pixel[0])
            + 150 * u32::from(pixel[1])
            + 77 * u32::from(pixel[2]))
            >> 8) as u8;
        levels.insert(luminance);
        if pixel[0] != clear[0] || pixel[1] != clear[1] || pixel[2] != clear[2] {
            covered += 1;
        }
    }
    FrameFacts {
        width: image.width(),
        height: image.height(),
        distinct_luminance: levels.len(),
        covered_pixels: covered,
    }
}

/// Coverage as a permille of the frame, rounded down.
fn covered_permille(covered: usize, width: u32, height: u32) -> u32 {
    let total = u64::from(width) * u64::from(height);
    if total == 0 {
        return 0;
    }
    ((covered as u128 * 1000) / total as u128) as u32
}

/// The adapter the driver reported, as `<name> (<backend)>`.
fn adapter_name(app: &App) -> String {
    app.world()
        .get_resource::<bevy::render::renderer::RenderAdapterInfo>()
        .map(|info| {
            let info = &*info.0;
            format!("{} ({:?})", info.name, info.backend)
        })
        .unwrap_or_else(|| "no adapter reported".to_owned())
}
