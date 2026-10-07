//! A real GPU capture of one stored mesh **with its own textures**,
//! offscreen (`specs/F17-rendering-material-fidelity-and-scalable-presentation.md`,
//! stage `### F17-E`; shared contract `docs/contracts/CLI-EVIDENCE.md`).
//!
//! # What this is for
//!
//! [`super::gpu_capture::capture_world_mesh`] — the F17-D comparison
//! matrix's capture — draws a stored mesh with one declared flat material.
//! That is the right geometry witness, but it cannot show the original's
//! own textures, which is the second half of F17-E. This module draws the
//! same subject, on the same real renderer, with the material every stored
//! material group actually names: the image is resolved out of the world
//! group's **own** texture archive through the production
//! [`PlaytestTextureArchive`]/[`TextureBinder`] path (task #666), and the
//! decoded image is expanded through the F17-B `rgb565` policy
//! ([`crate::render::rgb565`]) exactly as the playtest draws it.
//!
//! The strict F17-B adapter [`crate::render::bevy_image::upload_image`] is
//! **not** the binding used here, and the difference is measured, not
//! chosen: every decoder leaves `DecodedImage::color_space` at
//! [`cs_formats::texture::ColorSpace::Unknown`], so `upload_image` refuses
//! every retail image with `color_space_unknown`. The #666 path is the one
//! production route the corpus's textures actually reach; its presentation
//! is the declared provisional one (`sRGB`, repeat addressing, keyed
//! coverage as a 0.5 mask — `PLAYTEST_TEXTURE_PRESENTATION_IS_PROVISIONAL`)
//! and the report carries that claim id.
//!
//! # What is **not** claimed
//!
//! * **Not the original's appearance.** The texture→material binding is the
//!   #666 designed reading ([`NAME_READING`](crate::playtest_textures::NAME_READING)),
//!   not the original renderer's rule, and the presentation is provisional.
//!   The capture is evidence that the group's own textures resolve and draw
//!   on the subject's real geometry — nothing more.
//! * **Not a placement**, same as the flat capture: the mesh is drawn at
//!   its own stored coordinates with no node transform.
//! * **Not a loosened comparison.** The camera framing, clear colour, key
//!   light, warmup and readback bound are [`super::gpu_capture`]'s own
//!   values — shared constants, not copies — and the settings record on the
//!   result is the fixed [`ComparisonSettings::comparison`] set of spec F17
//!   non-negotiable 3.
//!
//! # The refusals
//!
//! Every way a PNG could come back that is not evidence of a textured draw
//! is a named error, never a written PNG:
//!
//! * [`TexturedCaptureError::EmptyMesh`] /
//!   [`TexturedCaptureError::DegenerateBounds`] — as the flat capture.
//! * [`TexturedCaptureError::MeshUpload`] — the production
//!   [`WorldMeshes::insert_render_mesh`] refused a material group or the
//!   merge.
//! * [`TexturedCaptureError::NoParts`] / [`TexturedCaptureError::Unsliceable`]
//!   — the merged mesh produced no parts, or could not be cut back into one
//!   part per stored material group, so no material could be bound at all.
//! * [`TexturedCaptureError::MissingTexture`] — a stored material group
//!   names a texture that the group's archive does not resolve, or the
//!   group stores no UV to sample it with. **This is the capture's core
//!   contract: a missing texture stays a refusal, never a fallback** — the
//!   part is not drawn neutral and no PNG is produced.
//! * [`TexturedCaptureError::NoScreenshotCaptured`] /
//!   [`TexturedCaptureError::UniformFrame`] / [`TexturedCaptureError::Io`] —
//!   as the flat capture.

use std::fmt;
use std::path::Path;

use bevy::app::PluginGroup;
use bevy::camera::{ClearColorConfig, PerspectiveProjection, Projection, RenderTarget};
use bevy::image::Image;
use bevy::prelude::{
    App, Assets, Camera, Camera3d, Color, DefaultPlugins, DirectionalLight, Mesh3d, MeshMaterial3d,
    Transform, WindowPlugin, default,
};
use cs_assets::install::sha256;
use cs_content::mesh::{MeshPresentationUnknown, RenderMesh};
use cs_formats::gamez::GameZMaterials;
use cs_types::evidence::ContentHash;

use crate::playtest_retail::PlaytestContainer;
use crate::playtest_textures::{
    BoundImage, NeutralColor, PlaytestPart, PlaytestTextureArchive, TextureBinder,
    UnresolvedMaterial,
};
use crate::render::capture::ComparisonSettings;
use crate::world::meshes::WorldMeshes;
use crate::world::retail::container_mesh_key;

use super::gpu_capture::{
    CAPTURE_VIEW_DIRECTION, CLEAR_COLOR, CaptureTarget, CapturedFrame, FAR_PLANE_FACTOR,
    FRAMING_DISTANCE_FACTOR, KEY_LIGHT_ILLUMINANCE, MAX_CAPTURE_UPDATES, MESH_COLOR,
    NEAR_PLANE_FRACTION, adapter_name, capture_image, covered_permille, discard_capture,
    drive_capture, stored_bounds,
};

/// One stored material group whose named texture could not be bound.
///
/// The record a [`TexturedCaptureError::MissingTexture`] carries per refused
/// part: the material slot, the stored texture name it pointed at, and why
/// the production binder could not bind it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissingTexture {
    /// The stored material index the part draws.
    pub stored_material: u32,
    /// The stored texture name the material pointed at, when it stored one.
    pub texture: Option<String>,
    /// Why the texture was not bound: the binder's own code
    /// (`texture_not_resolved`, `texture_not_decoded`,
    /// `texture_index_out_of_range`, `material_index_out_of_range`,
    /// `foreign_group`), or this capture's `texture_not_bound` when the
    /// group stores no UV to sample the named texture with.
    pub reason: String,
}

/// What one drawn part's material came out as.
///
/// The classifier is pure — it needs the parts, the material records and
/// the binder's refusal list, none of which needs a renderer — so the
/// decision [`capture_subject_textured`] turns into a refusal can be
/// exercised without one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PartOutcome {
    /// The part draws a resolved original texture: the bound image's key.
    Textured {
        /// [`crate::playtest_textures::image_key`] of the bound image.
        image: String,
    },
    /// The part's stored material names no texture — a flat record, drawn
    /// with the capture's declared neutral colour.
    Flat,
    /// The part's stored material names a texture that could not be bound.
    MissingTexture(MissingTexture),
    /// The merged mesh could not be cut back into per-group parts: the
    /// binder drew one neutral part that attributes no material at all
    /// (the `u32::MAX` sentinel), so nothing about textures can be said.
    Unsliceable,
}

/// The binder's refusal of one part's material, if it recorded one.
fn unresolved_for(
    unresolved: &[UnresolvedMaterial],
    stored_material: u32,
) -> Option<&UnresolvedMaterial> {
    let suffix = format!("#material-{stored_material}");
    unresolved
        .iter()
        .find(|entry| entry.source_id.ends_with(&suffix))
}

/// Classifies every part against the container's material records and the
/// binder's refusals.
///
/// A part the binder drew textured is [`PartOutcome::Textured`]; a part whose
/// stored material names no texture is [`PartOutcome::Flat`]; a part whose
/// material **does** name a texture but carries no bound image is
/// [`PartOutcome::MissingTexture`], with the binder's own reason when it
/// recorded one and `texture_not_bound` when the group's missing UV is why.
/// The one-part `u32::MAX` sentinel is [`PartOutcome::Unsliceable`].
#[must_use]
pub fn part_outcomes(
    parts: &[PlaytestPart],
    materials: &GameZMaterials,
    unresolved: &[UnresolvedMaterial],
) -> Vec<PartOutcome> {
    parts
        .iter()
        .map(|part| {
            if let Some(image) = &part.image {
                return PartOutcome::Textured {
                    image: image.clone(),
                };
            }
            if part.stored_material == u32::MAX {
                return PartOutcome::Unsliceable;
            }
            match materials.material(part.stored_material) {
                Some(material) if materials.names_a_texture(material) => {
                    let recorded = unresolved_for(unresolved, part.stored_material);
                    PartOutcome::MissingTexture(MissingTexture {
                        stored_material: part.stored_material,
                        texture: recorded
                            .and_then(|entry| entry.texture.clone())
                            .or_else(|| {
                                materials.texture_of(material).map(|name| name.name.clone())
                            }),
                        reason: recorded.map_or_else(
                            || "texture_not_bound".to_owned(),
                            |e| e.reason.to_owned(),
                        ),
                    })
                }
                Some(_) => PartOutcome::Flat,
                None => {
                    let recorded = unresolved_for(unresolved, part.stored_material);
                    PartOutcome::MissingTexture(MissingTexture {
                        stored_material: part.stored_material,
                        texture: recorded.and_then(|entry| entry.texture.clone()),
                        reason: recorded.map_or_else(
                            || "material_index_out_of_range".to_owned(),
                            |e| e.reason.to_owned(),
                        ),
                    })
                }
            }
        })
        .collect()
}

/// Every [`PartOutcome::MissingTexture`] of the list, as refusal records.
#[must_use]
pub fn missing_textures(outcomes: &[PartOutcome]) -> Vec<MissingTexture> {
    outcomes
        .iter()
        .filter_map(|outcome| match outcome {
            PartOutcome::MissingTexture(missing) => Some(missing.clone()),
            _ => None,
        })
        .collect()
}

/// What one textured capture produced, all of it measured from the frame
/// that came back plus the production binding's own report.
#[derive(Clone, Debug, PartialEq)]
pub struct TexturedCapture {
    /// The world group the subject was resolved in.
    pub group: String,
    /// The subject's code (`cockpit`, `skyline`, ...).
    pub subject: String,
    /// The container's logical key the mesh came from.
    pub container: String,
    /// The mesh's array index in the container.
    pub mesh_index: u32,
    /// The texture archive the images were bound from, installation-relative.
    pub archive: String,
    /// SHA-256 of that archive, as discovery measured it.
    pub archive_sha256: String,
    /// Why that archive was chosen (the #666 declared selection).
    pub archive_selection: String,
    /// The reading stored names were looked up under.
    pub name_reading: &'static str,
    /// Parts the mesh drew.
    pub parts: usize,
    /// Of those, parts bound to a resolved texture.
    pub textured_parts: usize,
    /// Parts whose stored material names no texture (flat records).
    pub flat_parts: usize,
    /// Stored texture names that resolved, as the archive spells them.
    pub resolved_names: Vec<String>,
    /// Every image the mesh's materials bound, once.
    pub images: Vec<BoundImage>,
    /// Per-corner attributes the merged mesh could not carry, by code.
    pub dropped_attributes: Vec<&'static str>,
    /// The adapter the renderer actually selected, as the driver reported it.
    pub adapter: String,
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// How many distinct luminance levels the frame holds.
    pub distinct_luminance: usize,
    /// Pixels that differ from the clear colour.
    pub covered_pixels: usize,
    /// [`Self::covered_pixels`] over the frame's pixel count.
    pub covered_permille: u32,
    /// Triangles submitted for this frame.
    pub triangles: usize,
    /// Material groups the mesh was drawn as.
    pub groups: usize,
    /// The merged upload's fingerprint.
    pub fingerprint: ContentHash,
    /// The exposure the fixed comparison set declares — the
    /// [`crate::render::capture::COMPARISON_EXPOSURE`] value; recorded so a
    /// frame that forgot the pin cannot read as a comparison frame.
    pub exposure: f32,
    /// The tone curve the fixed comparison set declares — the code of
    /// [`crate::render::capture::Tonemap::None`].
    pub tonemap: &'static str,
    /// The display gamma the fixed comparison set declares —
    /// [`crate::render::capture::COMPARISON_GAMMA`].
    pub gamma: f32,
    /// The sample count the fixed comparison set declares —
    /// [`crate::render::capture::COMPARISON_MSAA_SAMPLES`].
    pub msaa_samples: u32,
    /// SHA-256 of the written PNG's bytes.
    pub png_sha256: ContentHash,
    /// How many bytes the PNG has.
    pub png_bytes: u64,
    /// Where the PNG was written.
    pub png: String,
}

impl TexturedCapture {
    /// Whether the frame drew anything at all — the same measured predicate
    /// as the flat capture's `drew_geometry`.
    #[must_use]
    pub const fn drew_geometry(&self) -> bool {
        self.distinct_luminance > 1 && self.covered_pixels > 0
    }
}

/// Why a textured capture could not be produced.
#[derive(Debug)]
pub enum TexturedCaptureError {
    /// The mesh holds no drawable triangle.
    EmptyMesh {
        /// The group the mesh came from.
        group: String,
        /// The mesh's array index.
        mesh_index: u32,
        /// The material groups the mesh does have.
        groups: usize,
    },
    /// Every stored corner of the mesh is the same point.
    DegenerateBounds {
        /// The group the mesh came from.
        group: String,
        /// The mesh's array index.
        mesh_index: u32,
    },
    /// The production mesh upload refused the mesh, with its own reason.
    MeshUpload {
        /// The group the mesh came from.
        group: String,
        /// The mesh's array index.
        mesh_index: u32,
        /// The upload's message, verbatim.
        reason: String,
    },
    /// The binder produced no parts for the mesh at all.
    NoParts {
        /// The group the mesh came from.
        group: String,
        /// The mesh's array index.
        mesh_index: u32,
    },
    /// The merged mesh could not be cut back into one part per stored
    /// material group, so no material could be bound.
    Unsliceable {
        /// The group the mesh came from.
        group: String,
        /// The mesh's array index.
        mesh_index: u32,
    },
    /// At least one stored material group names a texture that the group's
    /// archive does not resolve, or that the group cannot sample (no UV).
    /// The refusal is the record; no neutral stand-in is drawn and no PNG
    /// is produced.
    MissingTexture {
        /// The group the mesh came from.
        group: String,
        /// The subject being captured.
        subject: String,
        /// The mesh's array index.
        mesh_index: u32,
        /// Every refused material group, one record each.
        missing: Vec<MissingTexture>,
    },
    /// The renderer ran but produced no image.
    NoScreenshotCaptured {
        /// How many updates the capture drove before giving up.
        updates: u32,
    },
    /// The frame came back and every pixel is the background.
    UniformFrame {
        /// The group the mesh came from.
        group: String,
        /// The mesh's array index.
        mesh_index: u32,
        /// How many distinct luminance levels the frame held. Always one.
        distinct_luminance: usize,
        /// Pixels that differ from the background.
        covered_pixels: usize,
        /// The frame's pixel count.
        total_pixels: usize,
    },
    /// The PNG could not be written or read back.
    Io {
        /// The path involved.
        path: String,
        /// The operating system's message, verbatim.
        reason: String,
    },
}

impl TexturedCaptureError {
    /// Stable lowercase identifier, for rows and reports.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::EmptyMesh { .. } => "empty_mesh",
            Self::DegenerateBounds { .. } => "degenerate_bounds",
            Self::MeshUpload { .. } => "mesh_upload",
            Self::NoParts { .. } => "no_parts",
            Self::Unsliceable { .. } => "unsliceable",
            Self::MissingTexture { .. } => "missing_texture",
            Self::NoScreenshotCaptured { .. } => "no_screenshot",
            Self::UniformFrame { .. } => "uniform_frame",
            Self::Io { .. } => "io",
        }
    }
}

impl fmt::Display for TexturedCaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyMesh {
                group,
                mesh_index,
                groups,
            } => write!(
                f,
                "{group} mesh {mesh_index} holds no drawable triangle ({groups} material groups), \
                 so a textured frame of it would be empty however well it was framed"
            ),
            Self::DegenerateBounds { group, mesh_index } => write!(
                f,
                "{group} mesh {mesh_index} has every stored corner on one point, so it has no \
                 bounds to frame"
            ),
            Self::MeshUpload {
                group,
                mesh_index,
                reason,
            } => write!(
                f,
                "{group} mesh {mesh_index} would not upload through the production adapter: \
                 {reason}"
            ),
            Self::NoParts { group, mesh_index } => write!(
                f,
                "{group} mesh {mesh_index} produced no drawable part, so there is nothing to bind"
            ),
            Self::Unsliceable { group, mesh_index } => write!(
                f,
                "{group} mesh {mesh_index} could not be cut back into one part per stored \
                 material group, so no texture can be attributed to it"
            ),
            Self::MissingTexture {
                group,
                subject,
                mesh_index,
                missing,
            } => {
                write!(
                    f,
                    "{group} {subject} (mesh {mesh_index}) cannot be captured textured: {} \
                     material group(s) name a texture that is not bound: ",
                    missing.len()
                )?;
                for (index, entry) in missing.iter().enumerate() {
                    if index > 0 {
                        f.write_str(", ")?;
                    }
                    match &entry.texture {
                        Some(name) => write!(
                            f,
                            "material {} names {name:?} ({})",
                            entry.stored_material, entry.reason
                        )?,
                        None => write!(f, "material {} ({})", entry.stored_material, entry.reason)?,
                    }
                }
                Ok(())
            }
            Self::NoScreenshotCaptured { updates } => write!(
                f,
                "the renderer ran {updates} updates and produced no captured image, so no frame \
                 came back"
            ),
            Self::UniformFrame {
                group,
                mesh_index,
                distinct_luminance,
                covered_pixels,
                total_pixels,
            } => write!(
                f,
                "{group} mesh {mesh_index} rendered a frame with {distinct_luminance} distinct \
                 luminance level and {covered_pixels} of {total_pixels} pixels off the \
                 background, i.e. the geometry was not drawn"
            ),
            Self::Io { path, reason } => {
                write!(f, "the capture image {path} could not be used: {reason}")
            }
        }
    }
}

impl std::error::Error for TexturedCaptureError {}

/// One mesh asked for: the subject label, the container that owns its
/// materials, the render mesh, the group's own texture archive and where the
/// PNG goes.
pub struct TexturedCaptureRequest<'a> {
    /// The world group key the mesh came from, used in the refusal messages
    /// and the artifact name.
    pub group: &'a str,
    /// The subject's code, e.g. `skyline` — carried into the record.
    pub subject: &'a str,
    /// The container whose material table the mesh's groups name.
    pub container: &'a PlaytestContainer,
    /// The mesh's array index in `container`.
    pub mesh_index: u32,
    /// The production render mesh, straight from the content layer.
    pub render: &'a RenderMesh,
    /// The presentation unknowns the content pipeline reported for it,
    /// handed to the upload adapter unchanged.
    pub unknowns: &'a [MeshPresentationUnknown],
    /// The group's own texture archive. A texture that does not resolve
    /// refuses the capture; no other group's archive is consulted.
    pub archive: &'a PlaytestTextureArchive,
    /// Where the PNG is written. The parent directory must exist.
    pub png: &'a Path,
}

/// Renders one stored mesh with its stored materials on the real GPU and
/// writes its PNG.
///
/// The geometry reaches the renderer through the same production upload the
/// playtest uses — [`WorldMeshes::insert_render_mesh`] into one merged mesh,
/// then [`TextureBinder::parts`] binding every material group — and the
/// image each textured part draws was resolved out of the group's own
/// archive. The framing, clear colour, key light and readback bound are
/// [`super::gpu_capture`]'s shared values, so a textured capture is
/// comparable with its flat sibling.
///
/// # Errors
///
/// Every [`TexturedCaptureError`], each of which leaves no PNG behind. In
/// particular [`TexturedCaptureError::MissingTexture`] — a material that
/// names a texture and does not get it bound is the named refusal
/// `missing_texture`, never a neutral fallback.
pub fn capture_subject_textured(
    request: &TexturedCaptureRequest<'_>,
) -> Result<TexturedCapture, TexturedCaptureError> {
    let group = request.group.to_owned();
    let mesh_index = request.mesh_index;
    if request.render.triangles().is_empty() {
        return Err(TexturedCaptureError::EmptyMesh {
            group,
            mesh_index,
            groups: request.render.groups().len(),
        });
    }
    let (min, max) = stored_bounds(request.render);
    let extent = (0..3).map(|axis| max[axis] - min[axis]);
    if !extent.clone().any(|side| side > 0.0) {
        return Err(TexturedCaptureError::DegenerateBounds { group, mesh_index });
    }

    let mut app = App::new();
    app.init_resource::<CapturedFrame>();
    // No window, as the flat capture: the frame renders into an image asset
    // and is read back, so the capture needs an adapter and not a display.
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

    // The production upload: every stored material group of the mesh merged
    // into one engine mesh, keyed the way the world import keys it.
    let id =
        container_mesh_key(request.container.group(), mesh_index as usize).map_err(|error| {
            TexturedCaptureError::MeshUpload {
                group: group.clone(),
                mesh_index,
                reason: error.to_string(),
            }
        })?;
    let mut meshes = WorldMeshes::new();
    meshes
        .insert_render_mesh(id.clone(), request.render, request.unknowns)
        .map_err(|error| TexturedCaptureError::MeshUpload {
            group: group.clone(),
            mesh_index,
            reason: error.to_string(),
        })?;
    let world_mesh = meshes
        .get(&id)
        .expect("the mesh was inserted under this id");

    // The production binding: the group's own archive, strict. A part whose
    // stored material names a texture but carries no bound image refuses
    // the whole capture — before any frame is drawn, so no PNG can result.
    let mut binder = TextureBinder::new(request.archive, true);
    let parts = binder.parts(
        &mut app,
        request.container,
        world_mesh,
        NeutralColor(MESH_COLOR),
    );
    let report = binder.finish();
    if parts.is_empty() {
        return Err(TexturedCaptureError::NoParts { group, mesh_index });
    }
    let subject_report = report.subject(request.container.container_key());
    let unresolved: &[UnresolvedMaterial] = subject_report
        .map(|subject| subject.unresolved.as_slice())
        .unwrap_or(&[]);
    let outcomes = part_outcomes(&parts, request.container.materials(), unresolved);
    if outcomes
        .iter()
        .any(|outcome| matches!(outcome, PartOutcome::Unsliceable))
    {
        return Err(TexturedCaptureError::Unsliceable { group, mesh_index });
    }
    let missing = missing_textures(&outcomes);
    if !missing.is_empty() {
        return Err(TexturedCaptureError::MissingTexture {
            group,
            subject: request.subject.to_owned(),
            mesh_index,
            missing,
        });
    }

    let target = spawn_textured_scene(&mut app, &parts, request);
    drive_capture(&mut app, target, request.png).map_err(|error| match error {
        super::gpu_capture::GpuCaptureError::NoScreenshotCaptured { updates } => {
            TexturedCaptureError::NoScreenshotCaptured { updates }
        }
        other => TexturedCaptureError::Io {
            path: request.png.display().to_string(),
            reason: other.to_string(),
        },
    })?;

    // Every refusal from here on removes the PNG the renderer already wrote,
    // exactly as the flat capture's contract requires.
    let facts = {
        let recorded = app.world().resource::<CapturedFrame>();
        let guard = recorded.0.lock().map_err(|_| {
            discard_capture(request.png);
            TexturedCaptureError::NoScreenshotCaptured {
                updates: MAX_CAPTURE_UPDATES,
            }
        })?;
        match *guard {
            Some(facts) => facts,
            None => {
                discard_capture(request.png);
                return Err(TexturedCaptureError::NoScreenshotCaptured {
                    updates: MAX_CAPTURE_UPDATES,
                });
            }
        }
    };
    if facts.distinct_luminance <= 1 {
        discard_capture(request.png);
        return Err(TexturedCaptureError::UniformFrame {
            group,
            mesh_index,
            distinct_luminance: facts.distinct_luminance,
            covered_pixels: facts.covered_pixels,
            total_pixels: facts.width as usize * facts.height as usize,
        });
    }

    let bytes = std::fs::read(request.png).map_err(|error| TexturedCaptureError::Io {
        path: request.png.display().to_string(),
        reason: error.to_string(),
    })?;
    let settings = ComparisonSettings::comparison();
    debug_assert!(
        settings.is_fixed(),
        "the comparison capture only ever records the fixed comparison set"
    );
    Ok(TexturedCapture {
        group,
        subject: request.subject.to_owned(),
        container: request.container.container_key().to_owned(),
        mesh_index,
        archive: report.archive.clone(),
        archive_sha256: report.archive_sha256.clone(),
        archive_selection: report.selection.clone(),
        name_reading: report.name_reading,
        parts: parts.len(),
        textured_parts: subject_report.map_or(0, |subject| subject.textured_parts),
        flat_parts: subject_report.map_or(0, |subject| subject.flat_materials),
        resolved_names: subject_report
            .map(|subject| subject.resolved_names.clone())
            .unwrap_or_default(),
        images: subject_report
            .map(|subject| subject.images.clone())
            .unwrap_or_default(),
        dropped_attributes: world_mesh
            .dropped_attributes()
            .iter()
            .map(|kind| kind.code())
            .collect(),
        adapter: adapter_name(&app),
        width: facts.width,
        height: facts.height,
        distinct_luminance: facts.distinct_luminance,
        covered_pixels: facts.covered_pixels,
        covered_permille: covered_permille(facts.covered_pixels, facts.width, facts.height),
        triangles: parts.iter().map(|part| part.triangles).sum(),
        groups: world_mesh.group_count(),
        fingerprint: world_mesh.fingerprint(),
        exposure: settings.exposure(),
        tonemap: settings.tonemap().code(),
        gamma: settings.gamma(),
        msaa_samples: settings.msaa_samples(),
        png_sha256: sha256(&bytes),
        png_bytes: bytes.len() as u64,
        png: request.png.display().to_string(),
    })
}

/// Spawns the camera, the key light and one entity per bound part, on the
/// flat capture's own framing constants.
fn spawn_textured_scene(
    app: &mut App,
    parts: &[PlaytestPart],
    request: &TexturedCaptureRequest<'_>,
) -> CaptureTarget {
    let (min, max) = stored_bounds(request.render);
    let centre = [
        (min[0] + max[0]) * 0.5,
        (min[1] + max[1]) * 0.5,
        (min[2] + max[2]) * 0.5,
    ];
    let radius = 0.5
        * (0..3)
            .map(|axis| (max[axis] - min[axis]).powi(2))
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
    let eye = [
        centre[0] + unit[0] * distance,
        centre[1] + unit[1] * distance,
        centre[2] + unit[2] * distance,
    ];

    let image = capture_image();
    let handle = app.world_mut().resource_mut::<Assets<Image>>().add(image);

    let centre_f = bevy::math::Vec3::new(centre[0] as f32, centre[1] as f32, centre[2] as f32);
    let up = bevy::math::Vec3::Y;
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
        Transform::from_xyz(eye[0] as f32, eye[1] as f32, eye[2] as f32).looking_at(centre_f, up),
    ));
    app.world_mut().spawn((
        DirectionalLight {
            illuminance: KEY_LIGHT_ILLUMINANCE,
            ..default()
        },
        Transform::from_xyz(eye[0] as f32, eye[1] as f32 + radius as f32, eye[2] as f32)
            .looking_at(centre_f, up),
    ));

    for part in parts {
        app.world_mut().spawn((
            Mesh3d(part.mesh.clone()),
            MeshMaterial3d(part.material.clone()),
            Transform::from_xyz(0.0, 0.0, 0.0),
        ));
    }

    CaptureTarget { image: handle }
}
