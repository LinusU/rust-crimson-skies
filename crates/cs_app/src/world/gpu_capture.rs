//! A real GPU capture of one stored world mesh, offscreen
//! (`specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`,
//! stage `### F18-D`). Shared contract: `docs/contracts/CLI-EVIDENCE.md`.
//!
//! # What this is for
//!
//! F18-D's acceptance scenario is *"visit every discovered world group and
//! compare representative geometry **and** traversal routes"*, and the sheet
//! declares `gpu` as a required capability of the stage. This module is the
//! `gpu` half: it takes one world's **real** stored render mesh — the value
//! `cs_content::mesh` produced and the production upload adapter turns into a
//! Bevy `Mesh` — draws it on the real renderer and writes a PNG.
//!
//! [`crate::render::capture`] deliberately records a frame as a *value* and
//! claims nothing about pixels, which is the right call for a headless
//! comparison. This is the other thing: a picture, from a real adapter, with the
//! measured facts about that picture attached so a reader can tell a drawn frame
//! from a blank one without opening the file.
//!
//! # What is **not** claimed
//!
//! * **Not a placement.** A world group's meshes have no position, orientation
//!   or scale in world space (no production path decodes a GameZ node array), so
//!   this renders one mesh at its own stored coordinates and nothing else. It is
//!   evidence that the group's stored geometry is drawable as stored; it is not
//!   evidence about a level.
//! * **Not a metric.** The stored vertex unit is unmeasured, so the camera's
//!   distances below are in *stored units* and the frame is not a measurement of
//!   anything's size.
//! * **Not the original's appearance.** The material is a flat colour chosen
//!   here, the lighting is a declared key light, and F17's presentation unknowns
//!   (`FrontFaceWinding`, `UvOrigin`, `VertexColor`) are open. The capture is
//!   evidence about *geometry*, drawn through the real pipeline.
//!
//! # The refusals
//!
//! Every way this can produce a file that is not evidence of a drawn frame is a
//! named error, never a written PNG:
//!
//! * [`GpuCaptureError::NoAdapter`] — the renderer got no adapter, so nothing
//!   was drawn.
//! * [`GpuCaptureError::EmptyMesh`] — the mesh has no drawable triangle, so a
//!   frame would be empty however well it was framed.
//! * [`GpuCaptureError::DegenerateBounds`] — every stored corner is the same
//!   point, so there is no frame to put it in.
//! * [`GpuCaptureError::NoScreenshotCaptured`] — the renderer ran but produced
//!   no image, which is a driver-side absence, not a blank frame.
//! * [`GpuCaptureError::UniformFrame`] — the frame came back and every pixel is
//!   the clear colour: the mesh was not drawn. This is the check that makes the
//!   PNG evidence rather than a decoration.
//! * [`GpuCaptureError::Io`] — the PNG could not be written or read back.
//!
//! [`GpuCapture::covered_pixels`] and [`GpuCapture::distinct_luminance`] are the
//! measurements behind that last refusal, so a reader sees how much of the frame
//! the geometry actually reached.

use std::fmt;
use std::path::Path;
use std::sync::Mutex;

use bevy::asset::RenderAssetUsages;
use bevy::camera::RenderTarget;
use bevy::image::{Image, ImageSampler};
use bevy::app::PluginGroup;
use bevy::mesh::Mesh;
use bevy::prelude::{
    App, Assets, Camera3d, Color, DefaultPlugins, DirectionalLight, Handle, Mesh3d,
    MeshMaterial3d, On, Res, Resource, StandardMaterial, Transform, WindowPlugin, default,
};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::render::view::screenshot::{save_to_disk, Screenshot, ScreenshotCaptured};
use cs_assets::install::sha256;
use cs_content::mesh::{MeshPresentationUnknown, RenderMesh};
use cs_types::evidence::ContentHash;

use crate::render::bevy_mesh::{GroupUpload, upload_group};

/// The capture frame's width in pixels.
///
/// A small fixed frame on purpose: the artifact is a geometry witness, not a
/// screenshot, and a smaller frame is a smaller file and a faster read for every
/// reviewer who opens one.
pub const CAPTURE_WIDTH: u32 = 320;

/// The capture frame's height in pixels. As [`CAPTURE_WIDTH`].
pub const CAPTURE_HEIGHT: u32 = 240;

/// How far the camera sits from the mesh's bounding-sphere centre, as a multiple
/// of that sphere's radius.
///
/// A declared framing constant, not a tuned one. The camera's field of view is
/// Bevy's 3D default (vertical 45°), whose half-angle tangent is 0.4142, so a
/// sphere of radius `r` fits at any distance `d ≥ r / 0.4142 = 2.414 r`;
/// `2.75 r` leaves 14 % margin and still fills most of the frame. Written down
/// so a reader can check the mesh was in shot rather than trusting a distance.
pub const FRAMING_DISTANCE_FACTOR: f64 = 2.75;

/// The clear colour the capture renders onto: the frame's background.
///
/// Chosen dark and **not** equal to the material's colour, so a frame that drew
/// nothing and a frame that drew something are distinguishable by
/// [`GpuCapture::covered_pixels`].
const CLEAR_COLOR: [f32; 4] = [0.043, 0.055, 0.075, 1.0];

/// The flat colour the captured mesh is presented with.
///
/// Declared here, and it is deliberately *not* a stored material: the capture
/// measures whether the stored geometry is drawable, and a stored texture or
/// material would make the artifact depend on a second unmeasured subsystem
/// (F17's presentation unknowns).
const MESH_COLOR: [f32; 4] = [0.78, 0.74, 0.66, 1.0];

/// The key light's illuminance, in lux, and its direction relative to the camera.
///
/// Declared: the capture is a geometry witness, so the light exists to make a
/// surface visible and its value is not a claim about the original's lighting.
const KEY_LIGHT_ILLUMINANCE: f32 = 12_000.0;

/// How many frames the capture waits before asking for the screenshot.
///
/// The mesh asset has to be uploaded to the GPU before a frame can draw it, and
/// that takes at least one `RenderApp` pass. Four frames is a measured
/// sufficiency on the pinned pair, not a guess: the spike this was built from
/// drew a mesh correctly from the fifth update onwards and produced a uniform
/// frame before that.
const WARMUP_FRAMES: u32 = 4;

/// What one capture produced, all of it measured from the frame that came back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GpuCapture {
    /// The world group key the mesh came from, for the artifact's own name.
    pub group: String,
    /// The mesh's array index in its container.
    pub mesh_index: u32,
    /// The adapter the renderer actually selected, as the driver reported it.
    pub adapter: String,
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// How many distinct luminance levels the frame holds. One means the whole
    /// frame is the background.
    pub distinct_luminance: usize,
    /// Pixels that differ from [`CLEAR_COLOR`]: the geometry's own coverage.
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

impl GpuCapture {
    /// Whether the frame drew anything at all.
    ///
    /// A capture that came back with a single luminance level is a **uniform
    /// frame** and never a capture: it is rejected by
    /// [`capture_world_mesh`], so a `GpuCapture` that exists has already passed
    /// this. It is a method rather than a field so the refusal and the
    /// predicate cannot drift apart.
    #[must_use]
    pub const fn drew_geometry(&self) -> bool {
        self.distinct_luminance > 1 && self.covered_pixels > 0
    }
}

/// Why a capture could not be produced.
#[derive(Debug)]
pub enum GpuCaptureError {
    /// The renderer got no adapter, so no frame was drawn at all.
    NoAdapter,
    /// The mesh holds no drawable triangle.
    EmptyMesh {
        /// The group the mesh came from.
        group: String,
        /// The mesh's array index.
        mesh_index: u32,
        /// The material groups the mesh does have.
        groups: usize,
    },
    /// Every stored corner of the mesh is the same point, so there is no frame
    /// to put it in.
    DegenerateBounds {
        /// The group the mesh came from.
        group: String,
        /// The mesh's array index.
        mesh_index: u32,
    },
    /// The renderer ran but produced no image.
    NoScreenshotCaptured {
        /// How many updates the capture drove before giving up.
        updates: u32,
    },
    /// The frame came back and every pixel is the background: the mesh was not
    /// drawn.
    UniformFrame {
        /// The group the mesh came from.
        group: String,
        /// The mesh's array index.
        mesh_index: u32,
        /// How many distinct luminance levels the frame held. Always one.
        distinct_luminance: usize,
    },
    /// A material group refused to upload, with the adapter's own reason.
    GroupRefused {
        /// The group the mesh came from.
        group: String,
        /// The mesh's array index.
        mesh_index: u32,
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

impl fmt::Display for GpuCaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoAdapter => {
                f.write_str("the renderer selected no GPU adapter, so no frame was drawn")
            }
            Self::EmptyMesh {
                group,
                mesh_index,
                groups,
            } => write!(
                f,
                "{group} mesh {mesh_index} holds no drawable triangle ({groups} material groups), \
                 so a frame of it would be empty however well it was framed"
            ),
            Self::DegenerateBounds { group, mesh_index } => write!(
                f,
                "{group} mesh {mesh_index} has every stored corner on one point, so it has no \
                 bounds to frame"
            ),
            Self::NoScreenshotCaptured { updates } => write!(
                f,
                "the renderer ran {updates} updates and produced no captured image, so no frame \
                 came back"
            ),
            Self::UniformFrame {
                group,
                mesh_index,
                distinct_luminance,
            } => write!(
                f,
                "{group} mesh {mesh_index} rendered a frame with {distinct_luminance} distinct \
                 luminance level, i.e. the whole frame is the background: the geometry was not \
                 drawn"
            ),
            Self::GroupRefused {
                group,
                mesh_index,
                material_group,
                reason,
            } => write!(
                f,
                "{group} mesh {mesh_index} material group {material_group} would not upload: \
                 {reason}"
            ),
            Self::Io { path, reason } => {
                write!(f, "the capture image {path} could not be used: {reason}")
            }
        }
    }
}

impl std::error::Error for GpuCaptureError {}

/// One mesh asked for: which group's, which index, the render mesh itself, and
/// where its PNG goes.
pub struct CaptureRequest<'a> {
    /// The world group key the mesh came from, used in the refusal messages and
    /// the artifact name.
    pub group: &'a str,
    /// The mesh's array index in its container.
    pub mesh_index: u32,
    /// The production render mesh, straight from the content layer.
    pub render: &'a RenderMesh,
    /// The presentation unknowns the content pipeline reported for it, handed to
    /// the upload adapter unchanged.
    pub unknowns: &'a [MeshPresentationUnknown],
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

/// Renders one stored world mesh on the real GPU and writes its PNG.
///
/// `png`'s parent directory must exist; the file is written by the renderer's
/// own screenshot path and then read back for its digest, so the digest in the
/// capture is of the file on disk rather than of a buffer this module imagined.
///
/// # Errors
///
/// Every [`GpuCaptureError`], each of which leaves no PNG behind: an empty mesh,
/// a mesh with no bounds, a refused material group, no adapter, no image, a
/// uniform frame, or a read/write failure.
pub fn capture_world_mesh(request: &CaptureRequest<'_>) -> Result<GpuCapture, GpuCaptureError> {
    let uploads = upload_all(request)?;

    let mut app = App::new();
    app.init_resource::<CapturedFrame>();
    // No window: the frame is rendered into an image asset and read back, so the
    // capture needs an adapter and not a display. `WinitPlugin` is the one
    // default that would require a main-thread event loop, which a test
    // harness does not have.
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

    let target = spawn_scene(&mut app, &uploads, request);
    drive_capture(&mut app, target, request)?;

    let facts = {
        let recorded = app.world().resource::<CapturedFrame>();
        let guard = recorded.0.lock().map_err(|_| GpuCaptureError::NoScreenshotCaptured {
            updates: 0,
        })?;
        guard.ok_or(GpuCaptureError::NoScreenshotCaptured {
            updates: WARMUP_FRAMES + 2,
        })?
    };
    if facts.distinct_luminance <= 1 {
        return Err(GpuCaptureError::UniformFrame {
            group: request.group.to_owned(),
            mesh_index: request.mesh_index,
            distinct_luminance: facts.distinct_luminance,
        });
    }

    let bytes = std::fs::read(request.png).map_err(|error| GpuCaptureError::Io {
        path: request.png.display().to_string(),
        reason: error.to_string(),
    })?;
    let (vertices, triangles) = upload_counts(&uploads);
    Ok(GpuCapture {
        group: request.group.to_owned(),
        mesh_index: request.mesh_index,
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
fn upload_all(request: &CaptureRequest<'_>) -> Result<Vec<Mesh>, GpuCaptureError> {
    if request.render.triangles().is_empty() {
        return Err(GpuCaptureError::EmptyMesh {
            group: request.group.to_owned(),
            mesh_index: request.mesh_index,
            groups: request.render.groups().len(),
        });
    }
    (0..request.render.groups().len())
        .map(|group| {
            upload_group(request.render, group, request.unknowns)
                .map(GroupUpload::into_mesh)
                .map_err(|error| GpuCaptureError::GroupRefused {
                    group: request.group.to_owned(),
                    mesh_index: request.mesh_index,
                    material_group: group,
                    reason: error.to_string(),
                })
        })
        .collect()
}

/// The total vertex and triangle count the uploads will draw.
fn upload_counts(uploads: &[Mesh]) -> (usize, usize) {
    let mut vertices = 0;
    let mut triangles = 0;
    for mesh in uploads {
        vertices += mesh.count_vertices() as usize;
        triangles += mesh.indices().map_or(0, |indices| indices.len() / 3);
    }
    (vertices, triangles)
}

/// Spawns the camera, the key light, one entity per material group and the
/// render target the frame is drawn into.
fn spawn_scene(
    app: &mut App,
    uploads: &[Mesh],
    request: &CaptureRequest<'_>,
) -> CaptureTarget {
    let (min, max) = stored_bounds(request.render);
    let centre = [
        (min[0] + max[0]) * 0.5,
        (min[1] + max[1]) * 0.5,
        (min[2] + max[2]) * 0.5,
    ];
    let radius = (0..3)
        .map(|axis| max[axis] - min[axis])
        .fold(0.0_f64, f64::max);
    let distance = radius * FRAMING_DISTANCE_FACTOR;

    let image = capture_image();
    let handle = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(image);

    let centre_f = bevy::math::Vec3::new(centre[0] as f32, centre[1] as f32, centre[2] as f32);
    let radius_f = radius as f32;
    let distance_f = distance as f32;
    app.world_mut().spawn((
        Camera3d::default(),
        RenderTarget::Image(handle.clone().into()),
        Transform::from_xyz(centre[0] as f32, centre[1] as f32, centre[2] as f32 + distance_f)
            .looking_at(centre_f, bevy::math::Vec3::Y),
    ));
    app.world_mut().spawn((
        DirectionalLight {
            illuminance: KEY_LIGHT_ILLUMINANCE,
            ..default()
        },
        Transform::from_xyz(
            centre[0] as f32,
            centre[1] as f32 + radius_f,
            centre[2] as f32 + distance_f,
        )
        .looking_at(centre_f, bevy::math::Vec3::Y),
    ));

    let surface: Handle<StandardMaterial> = app
        .world_mut()
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial {
            base_color: Color::srgb(MESH_COLOR[0], MESH_COLOR[1], MESH_COLOR[2]),
            metallic: 0.0,
            ..default()
        });
    for mesh in uploads {
        // The upload owns exactly the `f32`/`u32` bit patterns the content
        // pipeline produced; the handle is the only conversion, and it is the
        // same one the collider-on-body path uses.
        let handle = app.world_mut().resource_mut::<Assets<Mesh>>().add(mesh.clone());
        app.world_mut().spawn((
            Mesh3d(handle),
            MeshMaterial3d(surface.clone()),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
    }

    CaptureTarget { image: handle }
}

/// The render target the frame is drawn into and read back from.
fn capture_image() -> Image {
    let size = Extent3d {
        width: CAPTURE_WIDTH,
        height: CAPTURE_HEIGHT,
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
    // The screenshot path reads the target back through a buffer, so the
    // texture needs `COPY_SRC`; it is also sampled, so it needs
    // `TEXTURE_BINDING`.
    image.texture_descriptor.usage = TextureUsages::COPY_DST
        | TextureUsages::COPY_SRC
        | TextureUsages::TEXTURE_BINDING
        | TextureUsages::RENDER_ATTACHMENT;
    image.sampler = ImageSampler::linear();
    image
}

/// The stored-unit bounds of one render mesh.
///
/// A non-finite corner is clamped to the axis's other end rather than propagated,
/// because a `NaN` transform would put the camera somewhere undefined and the
/// capture would then measure a frame that says nothing about the mesh. The
/// clamp is why [`GpuCaptureError::DegenerateBounds`] is checked *before* the
/// scene is spawned, from the raw corners.
fn stored_bounds(render: &RenderMesh) -> ([f64; 3], [f64; 3]) {
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for vertex in render.vertices() {
        for axis in 0..3 {
            let value = f64::from(vertex.position[axis]);
            if value.is_nan() {
                continue;
            }
            min[axis] = min[axis].min(value);
            max[axis] = max[axis].max(value);
        }
    }
    for axis in 0..3 {
        if !min[axis].is_finite() {
            min[axis] = 0.0;
        }
        if !max[axis].is_finite() {
            max[axis] = 0.0;
        }
    }
    (min, max)
}

/// Drives the app until the screenshot comes back or the updates run out.
fn drive_capture(
    app: &mut App,
    target: CaptureTarget,
    request: &CaptureRequest<'_>,
) -> Result<(), GpuCaptureError> {
    let png = request.png.to_path_buf();
    let observer = move |captured: On<ScreenshotCaptured>, frame: Res<CapturedFrame>| {
        if captured.image.data.is_some() {
            let image = captured.image.clone();
            *frame.0.lock().expect("the capture frame mutex") = Some(measure(&image));
        }
        save_to_disk(png.clone())(captured);
    };
    app.add_observer(observer);
    for update in 0..(WARMUP_FRAMES + 2) {
        if update == WARMUP_FRAMES {
            app.world_mut()
                .spawn(Screenshot::image(target.image.clone()));
        }
        app.update();
    }
    Ok(())
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
    for pixel in data.chunks_exact(4) {
        // Rec. 601 luminance in eight bits: the frame's own shading order, not
        // a colour-fidelity claim.
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

/// The adapter the driver reported, as `<name> (<backend>)`.
///
/// Read from the renderer's own [`RenderAdapterInfo`] resource, so the name in a
/// capture is the one the driver printed rather than a value this module wrote.
/// `"no adapter reported"` when the resource is absent, which cannot happen for
/// a capture that exists (a frame only comes back from an adapter) and says so
/// rather than inventing a device.
fn adapter_name(app: &App) -> String {
    app.world()
        .get_resource::<bevy::render::renderer::RenderAdapterInfo>()
        .map(|info| {
            let info = &*info.0;
            format!("{} ({:?})", info.name, info.backend)
        })
        .unwrap_or_else(|| "no adapter reported".to_owned())
}
