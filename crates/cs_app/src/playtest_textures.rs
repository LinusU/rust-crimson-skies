//! Original textures for the retail playtest's area and aircraft (task #666,
//! `PLAYTEST-TEXTURES`).
//!
//! #648 drew every surface with one neutral development material. The pieces
//! needed to do better already exist: the GameZ material records and their
//! texture-name table (F10-C.02), the ZBD texture catalog (F08-B/C) and the
//! RGB565 expansion policy (F17-B). This module joins them for one development
//! scene and **declares** the three choices the evidence has not settled, each
//! under its own claim id:
//!
//! | decision | value | claim |
//! | --- | --- | --- |
//! | archive | the highest-numbered `rtexture<N>.zbd` tier of the world group, else `texture.zbd` | [`PLAYTEST_TEXTURE_ARCHIVE_IS_DESIGNED`] |
//! | name reading | the stored name up to its first `.`, ASCII lower case ([`NAME_READING`]) | [`PLAYTEST_TEXTURE_NAME_READING_IS_DESIGNED`] |
//! | presentation | a plain lit `StandardMaterial`, sRGB, repeat addressing, keyed coverage as a 0.5 mask, no vertex colour | [`PLAYTEST_TEXTURE_PRESENTATION_IS_PROVISIONAL`] |
//!
//! The exact-name rule of the lookup contract resolves 10 of 4 478 retail
//! material rows (`docs/findings/2026-09-29-f10-c-04-gamez-texture-archive-binding.md`),
//! so a scene that insisted on it would draw nothing textured. The reading is a
//! measured instrument there and a **designed development value** here; it is
//! scoped to this playtest and admitted nowhere else.
//!
//! # What is never done
//!
//! * No archive of another world group is consulted: every resolved texture is
//!   checked against the group's own directory ([`check_group`]).
//! * A material whose texture cannot be resolved keeps the neutral material and
//!   is reported once, with its source id and the number of meshes using it. It
//!   never aborts the scene.
//! * The airframe's archive is "a runtime fact about which world is loaded"
//!   (F10-C.04), so the aircraft is textured from the **flown world's** archive,
//!   by the same declared choice.

use std::collections::BTreeMap;
use std::fmt;

use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, Mesh, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::{App, Assets, Color, Handle, StandardMaterial};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use cs_assets::install::{self, Discovery};
use cs_assets::vfs::{ContentSession, SessionBuilder, WORLD_NAMESPACE};
use cs_content::mesh::TextureNameRule;
use cs_content::textures::{TextureCatalog, TextureId, TextureRef};
use cs_formats::gamez::GameZMaterials;
use cs_formats::texture::DecodedImage;
use cs_types::asset_id::{AssetKey, ResolveContext, WorldGroup};

use crate::playtest_retail::PlaytestContainer;
use crate::render::rgb565::{CoverageSource, ExpansionPolicy, coverage_byte, expand_texel};
use crate::world::WorldMesh;

/// The archive choice is a designed development value.
pub const PLAYTEST_TEXTURE_ARCHIVE_IS_DESIGNED: &str =
    "playtest-textures.archive-selection-is-designed";

/// The stored-name to archive-name reading is a designed development value.
pub const PLAYTEST_TEXTURE_NAME_READING_IS_DESIGNED: &str =
    "playtest-textures.name-reading-is-designed";

/// Lighting, blending, colour space, addressing and vertex colour are provisional.
pub const PLAYTEST_TEXTURE_PRESENTATION_IS_PROVISIONAL: &str =
    "playtest-textures.presentation-is-provisional";

/// The reading the playtest looks stored texture names up under.
///
/// One of the five instruments `cs_content::mesh::measure_bindings` reports; the
/// one that reached the most names (219 of the airframe's 221, 549 of `C1`'s
/// 551). Not the original engine's rule, which is unmeasured.
pub const NAME_READING: TextureNameRule = TextureNameRule::FirstDotCaseFolded;

/// Coverage threshold of a keyed texel, a provisional blend choice.
const MASK_CUTOFF: f32 = 0.5;

/// Why the textures could not be set up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaytestTextureError {
    /// The world group holds no texture archive at all.
    NoArchive {
        /// The group searched.
        group: String,
    },
    /// The session or the catalog refused the archive.
    Archive {
        /// The archive's installation-relative path.
        archive: String,
        /// The refusal.
        reason: String,
    },
    /// A texture of another world group's archive was offered.
    ForeignGroup {
        /// The group the playtest flies in.
        group: String,
        /// The texture that was offered.
        texture: String,
    },
}

impl fmt::Display for PlaytestTextureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoArchive { group } => {
                write!(f, "world group {group} holds no texture archive")
            }
            Self::Archive { archive, reason } => {
                write!(f, "texture archive {archive} was refused: {reason}")
            }
            Self::ForeignGroup { group, texture } => write!(
                f,
                "texture {texture} is not from world group {group}'s own archive; textures are \
                 never aliased across world groups"
            ),
        }
    }
}

impl std::error::Error for PlaytestTextureError {}

/// The texture archive one playtest world draws from, opened through the
/// production content session and texture catalog.
#[derive(Debug)]
pub struct PlaytestTextureArchive {
    session: ContentSession,
    catalog: TextureCatalog,
    key: AssetKey,
    group: String,
    path: String,
    sha256: String,
    selection: String,
}

impl PlaytestTextureArchive {
    /// Chooses and opens the archive of `group` out of an inventoried
    /// installation.
    ///
    /// # Errors
    ///
    /// [`PlaytestTextureError::NoArchive`] when the group holds neither an
    /// `rtexture<N>.zbd` nor a `texture.zbd`, and
    /// [`PlaytestTextureError::Archive`] when the session cannot mount it or the
    /// catalog cannot read it.
    pub fn open(
        install_root: &std::path::Path,
        found: &Discovery,
        group: &str,
    ) -> Result<Self, PlaytestTextureError> {
        let lower = group.to_ascii_lowercase();
        let prefix = format!("zbd/{lower}/");
        let mut tiers: Vec<(u32, String, String, String)> = Vec::new();
        let mut primary: Option<(String, String, String)> = None;
        for record in &found.manifest.files {
            let logical = record.relative_spelling.logical_key();
            let Some(name) = logical.strip_prefix(&prefix) else {
                continue;
            };
            let spelling = record.relative_spelling.as_str().to_owned();
            let file = spelling.rsplit('/').next().unwrap_or(&spelling).to_owned();
            if name == "texture.zbd" {
                primary = Some((file, spelling, record.sha256.to_hex()));
            } else if let Some(number) = name
                .strip_prefix("rtexture")
                .and_then(|rest| rest.strip_suffix(".zbd"))
                .and_then(|digits| digits.parse::<u32>().ok())
            {
                tiers.push((number, file, spelling, record.sha256.to_hex()));
            }
        }
        tiers.sort_by_key(|tier| tier.0);
        let (file, path, sha256, selection) = if let Some((number, file, path, sha)) = tiers.pop() {
            (
                file,
                path,
                sha,
                format!("highest-numbered resolution tier (rtexture{number}) of the group"),
            )
        } else if let Some((file, path, sha)) = primary {
            (
                file,
                path,
                sha,
                "the group's texture.zbd (no resolution tier stored)".to_owned(),
            )
        } else {
            return Err(PlaytestTextureError::NoArchive {
                group: group.to_owned(),
            });
        };
        let fail = |reason: String| PlaytestTextureError::Archive {
            archive: path.clone(),
            reason,
        };
        let group_path = format!("zbd/{lower}");
        let world_group = found
            .diagnosis
            .world_groups
            .iter()
            .find(|candidate| candidate.as_str().eq_ignore_ascii_case(&group_path))
            .ok_or_else(|| fail("discovery names no such world group".to_owned()))?
            .clone();
        let context = ResolveContext::new(install::fingerprint(&found.manifest))
            .with_world_group(WorldGroup::from_relative(world_group));
        let mut builder = SessionBuilder::new(context);
        builder
            .mount_installation(install_root, &found.diagnosis)
            .map_err(|error| fail(error.to_string()))?;
        let session = builder.open();
        let key = AssetKey::from_spelling(WORLD_NAMESPACE, &file, "default")
            .map_err(|error| fail(error.to_string()))?;
        let catalog = TextureCatalog::open(&session, std::slice::from_ref(&key));
        if let Some((_, error)) = catalog.failures().next() {
            return Err(fail(error.to_string()));
        }
        let archive = Self {
            session,
            catalog,
            key,
            group: lower,
            path,
            sha256,
            selection,
        };
        Ok(archive)
    }

    /// The installation-relative path of the chosen archive.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The group the archive belongs to, lower case.
    #[must_use]
    pub fn group(&self) -> &str {
        &self.group
    }

    /// SHA-256 of the archive file, as discovery measured it.
    #[must_use]
    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    /// Why this archive was chosen, in words.
    #[must_use]
    pub fn selection(&self) -> &str {
        &self.selection
    }

    /// The first texture id of the archive, for a test that needs a real one.
    #[must_use]
    pub fn sample_id(&self) -> Option<TextureId> {
        self.catalog
            .archives()
            .next()
            .and_then(|archive| archive.ids().next().cloned())
    }

    /// How many textures the archive stores.
    #[must_use]
    pub fn texture_count(&self) -> usize {
        self.catalog
            .archives()
            .next()
            .map_or(0, |archive| archive.ids().count())
    }

    /// Resolves and decodes the texture a stored name reaches under
    /// [`NAME_READING`].
    fn fetch(&self, stored: &str) -> Result<(TextureId, DecodedImage), (String, &'static str)> {
        let projected = NAME_READING.project(stored);
        let reference = TextureRef::new(self.key.clone(), &projected);
        let resolved = self
            .catalog
            .resolve(&self.session, &reference)
            .map_err(|error| (error.to_string(), "texture_not_resolved"))?;
        check_group(&self.group, resolved.id())
            .map_err(|error| (error.to_string(), "foreign_group"))?;
        let image = self
            .catalog
            .decode(&self.session, &resolved)
            .map_err(|error| (error.to_string(), "texture_not_decoded"))?;
        Ok((resolved.id().clone(), image))
    }
}

/// Refuses a texture that does not live in `group`'s own directory.
///
/// # Errors
///
/// [`PlaytestTextureError::ForeignGroup`] when the archive path names another
/// group (or no group).
pub fn check_group(group: &str, id: &TextureId) -> Result<(), PlaytestTextureError> {
    let prefix = format!("zbd/{}/", group.to_ascii_lowercase());
    if id
        .archive
        .as_str()
        .to_ascii_lowercase()
        .starts_with(&prefix)
    {
        Ok(())
    } else {
        Err(PlaytestTextureError::ForeignGroup {
            group: group.to_owned(),
            texture: id.to_string(),
        })
    }
}

/// The image key a bound image is reported under: the archive it came from and
/// the member inside it.
#[must_use]
pub fn image_key(id: &TextureId) -> String {
    format!("{}#{}:{}", id.archive, id.entry_index, id.name)
}

/// One image bound as a base-colour texture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoundImage {
    /// [`image_key`]: the source archive and member.
    pub key: String,
    /// Width in texels.
    pub width: u32,
    /// Height in texels.
    pub height: u32,
    /// Texels with less than full coverage.
    pub translucent_texels: u32,
    /// Why coverage was treated as opaque, when the stored plane is unknown.
    pub coverage_note: Option<&'static str>,
}

/// A material that kept the neutral development material.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnresolvedMaterial {
    /// `<container key>#material-<stored index>`.
    pub source_id: String,
    /// The stored texture name, when the record names one.
    pub texture: Option<String>,
    /// Stable lowercase reason code.
    pub reason: &'static str,
    /// Distinct meshes that use the material.
    pub meshes: usize,
}

/// What one container's materials resolved to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SubjectTextures {
    /// The container key.
    pub container_key: String,
    /// Distinct materials bound to a decoded base-colour image.
    pub textured_materials: usize,
    /// Distinct materials drawn neutral (flat-colour records and unresolved).
    pub neutral_materials: usize,
    /// Of those, flat-colour records that name no texture at all.
    pub flat_materials: usize,
    /// Drawn parts (mesh material groups) with a texture.
    pub textured_parts: usize,
    /// Drawn parts with the neutral material.
    pub neutral_parts: usize,
    /// Stored names that resolved, projected as the archive spells them.
    pub resolved_names: Vec<String>,
    /// Stored names that did not.
    pub unresolved_names: Vec<String>,
    /// Every image bound, once.
    pub images: Vec<BoundImage>,
    /// Every material kept neutral because its texture did not resolve.
    pub unresolved: Vec<UnresolvedMaterial>,
}

/// The textures a scene drew, for the startup line and `report.json`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaytestTextureReport {
    /// The three claims the choices are filed under.
    pub claims: [&'static str; 3],
    /// The archive chosen, installation-relative.
    pub archive: String,
    /// Its SHA-256.
    pub archive_sha256: String,
    /// The world group it belongs to.
    pub group: String,
    /// Why it was chosen.
    pub selection: String,
    /// The reading names were looked up under.
    pub name_reading: &'static str,
    /// Per container: the area's and the aircraft's.
    pub subjects: Vec<SubjectTextures>,
}

impl PlaytestTextureReport {
    /// Textured materials over every subject.
    #[must_use]
    pub fn textured_materials(&self) -> usize {
        self.subjects.iter().map(|s| s.textured_materials).sum()
    }

    /// Neutral materials over every subject.
    #[must_use]
    pub fn neutral_materials(&self) -> usize {
        self.subjects.iter().map(|s| s.neutral_materials).sum()
    }

    /// The subject of one container.
    #[must_use]
    pub fn subject(&self, container_key: &str) -> Option<&SubjectTextures> {
        self.subjects
            .iter()
            .find(|subject| subject.container_key == container_key)
    }

    /// The report as one JSON object (the startup `playtest sources` line and
    /// the smoke `report.json` embed it).
    #[must_use]
    pub fn json(&self) -> String {
        let subjects = self
            .subjects
            .iter()
            .map(|s| {
                let unresolved = s
                    .unresolved
                    .iter()
                    .map(|u| {
                        format!(
                            "{{\"source\":\"{}\",\"texture\":{},\"reason\":\"{}\",\"meshes\":{}}}",
                            esc(&u.source_id),
                            u.texture
                                .as_ref()
                                .map_or_else(|| "null".to_owned(), |t| format!("\"{}\"", esc(t))),
                            u.reason,
                            u.meshes
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                format!(
                    "{{\"container\":\"{}\",\"textured_materials\":{},\"neutral_materials\":{},\
\"flat_materials\":{},\"textured_parts\":{},\"neutral_parts\":{},\"images\":{},\
\"unresolved\":[{unresolved}]}}",
                    esc(&s.container_key),
                    s.textured_materials,
                    s.neutral_materials,
                    s.flat_materials,
                    s.textured_parts,
                    s.neutral_parts,
                    s.images.len(),
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"archive\":\"{}\",\"archive_sha256\":\"{}\",\"group\":\"{}\",\"selection\":\"{}\",\
\"name_reading\":\"{}\",\"claims\":[\"{}\",\"{}\",\"{}\"],\"subjects\":[{subjects}]}}",
            esc(&self.archive),
            self.archive_sha256,
            esc(&self.group),
            esc(&self.selection),
            self.name_reading,
            self.claims[0],
            self.claims[1],
            self.claims[2],
        )
    }
}

fn esc(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

/// One drawn part of a mesh: the triangles of one stored material group, with
/// the material it is drawn with.
#[derive(Clone, Debug)]
pub struct PlaytestPart {
    /// The engine mesh of this group.
    pub mesh: Handle<Mesh>,
    /// The material it is drawn with.
    pub material: Handle<StandardMaterial>,
    /// The stored raw material index.
    pub stored_material: u32,
    /// Triangles in this part.
    pub triangles: usize,
    /// The bound image's key, when textured.
    pub image: Option<String>,
}

/// Neutral colours, one per subject (the #648 development values).
#[derive(Clone, Copy, Debug)]
pub struct NeutralColor(pub [f32; 4]);

#[derive(Clone)]
enum Binding {
    Textured {
        material: Handle<StandardMaterial>,
        key: String,
    },
    Neutral,
}

/// Binds stored materials to engine materials, one per `(container, material)`.
pub struct TextureBinder<'a> {
    archive: &'a PlaytestTextureArchive,
    enabled: bool,
    images: BTreeMap<usize, (Handle<Image>, BoundImage)>,
    bindings: BTreeMap<(String, u32), Binding>,
    neutral: BTreeMap<String, Handle<StandardMaterial>>,
    subjects: BTreeMap<String, SubjectTextures>,
    uses: BTreeMap<(String, u32), usize>,
}

impl<'a> TextureBinder<'a> {
    /// A binder over `archive`. A disabled binder binds nothing: every part is
    /// drawn with the neutral material (the untextured baseline).
    #[must_use]
    pub fn new(archive: &'a PlaytestTextureArchive, enabled: bool) -> Self {
        Self {
            archive,
            enabled,
            images: BTreeMap::new(),
            bindings: BTreeMap::new(),
            neutral: BTreeMap::new(),
            subjects: BTreeMap::new(),
            uses: BTreeMap::new(),
        }
    }

    fn subject(&mut self, container: &PlaytestContainer) -> &mut SubjectTextures {
        self.subjects
            .entry(container.container_key().to_owned())
            .or_insert_with(|| SubjectTextures {
                container_key: container.container_key().to_owned(),
                ..SubjectTextures::default()
            })
    }

    fn neutral(
        &mut self,
        app: &mut App,
        container: &PlaytestContainer,
        color: NeutralColor,
    ) -> Handle<StandardMaterial> {
        self.neutral
            .entry(container.container_key().to_owned())
            .or_insert_with(|| {
                app.world_mut()
                    .resource_mut::<Assets<StandardMaterial>>()
                    .add(neutral_material(color.0))
            })
            .clone()
    }

    /// Resolves one stored material of `container`, once.
    fn bind(
        &mut self,
        app: &mut App,
        container: &PlaytestContainer,
        materials: &GameZMaterials,
        index: u32,
    ) -> Binding {
        let cache_key = (container.container_key().to_owned(), index);
        if let Some(found) = self.bindings.get(&cache_key) {
            return found.clone();
        }
        let source_id = format!("{}#material-{index}", container.container_key());
        let unresolved = |binder: &mut Self, texture: Option<String>, reason: &'static str| {
            let subject = binder.subject(container);
            subject.neutral_materials += 1;
            if let Some(name) = &texture {
                subject.unresolved_names.push(name.clone());
            }
            subject.unresolved.push(UnresolvedMaterial {
                source_id: source_id.clone(),
                texture,
                reason,
                meshes: 0,
            });
            Binding::Neutral
        };
        let binding = match materials.material(index) {
            None => unresolved(self, None, "material_index_out_of_range"),
            Some(material) if !materials.names_a_texture(material) => {
                let subject = self.subject(container);
                subject.neutral_materials += 1;
                subject.flat_materials += 1;
                Binding::Neutral
            }
            Some(material) => match materials.texture_of(material) {
                None => unresolved(self, None, "texture_index_out_of_range"),
                Some(name) => match self.archive.fetch(&name.name) {
                    Err((_, reason)) => unresolved(self, Some(name.name.clone()), reason),
                    Ok((id, decoded)) => {
                        let (handle, key) = self.image_for(app, &id, &decoded);
                        let masked = keyed(&decoded);
                        let material = app
                            .world_mut()
                            .resource_mut::<Assets<StandardMaterial>>()
                            .add(textured_material(handle, masked));
                        let subject = self.subject(container);
                        subject.textured_materials += 1;
                        subject
                            .resolved_names
                            .push(NAME_READING.project(&name.name));
                        Binding::Textured { material, key }
                    }
                },
            },
        };
        self.bindings.insert(cache_key, binding.clone());
        binding
    }

    fn image_for(
        &mut self,
        app: &mut App,
        id: &TextureId,
        decoded: &DecodedImage,
    ) -> (Handle<Image>, String) {
        if let Some((handle, bound)) = self.images.get(&id.entry_index) {
            return (handle.clone(), bound.key.clone());
        }
        let (image, translucent, note) = to_bevy_image(decoded);
        let handle = app.world_mut().resource_mut::<Assets<Image>>().add(image);
        let bound = BoundImage {
            key: image_key(id),
            width: decoded.extent().width,
            height: decoded.extent().height,
            translucent_texels: translucent,
            coverage_note: note,
        };
        let key = bound.key.clone();
        self.images.insert(id.entry_index, (handle.clone(), bound));
        (handle, key)
    }

    /// Splits `world_mesh` into one part per stored material group and gives each
    /// the material its stored index resolves to.
    ///
    /// A mesh whose merged buffers cannot be sliced per group (the counts the
    /// merge recorded do not add up, or it carries no UV) is drawn as **one**
    /// neutral part: never dropped, never guessed.
    pub fn parts(
        &mut self,
        app: &mut App,
        container: &PlaytestContainer,
        world_mesh: &WorldMesh,
        neutral: NeutralColor,
    ) -> Vec<PlaytestPart> {
        let materials = container.materials();
        let slices = slice_groups(world_mesh);
        let mut seen: Vec<u32> = Vec::new();
        let mut parts = Vec::new();
        match slices {
            Some(slices) => {
                for (stored_material, mesh) in slices {
                    if !seen.contains(&stored_material) {
                        seen.push(stored_material);
                        *self
                            .uses
                            .entry((container.container_key().to_owned(), stored_material))
                            .or_default() += 1;
                    }
                    let triangles = mesh.indices().map_or(0, |i| i.len() / 3);
                    let has_uv = mesh.attribute(Mesh::ATTRIBUTE_UV_0).is_some();
                    let binding = if has_uv && self.enabled {
                        self.bind(app, container, materials, stored_material)
                    } else {
                        Binding::Neutral
                    };
                    let mesh = app.world_mut().resource_mut::<Assets<Mesh>>().add(mesh);
                    parts.push(self.part(
                        app,
                        container,
                        mesh,
                        stored_material,
                        triangles,
                        &binding,
                        neutral,
                    ));
                }
            }
            None => {
                let triangles = world_mesh.triangles();
                let mesh = app
                    .world_mut()
                    .resource_mut::<Assets<Mesh>>()
                    .add(world_mesh.mesh().clone());
                parts.push(self.part(
                    app,
                    container,
                    mesh,
                    u32::MAX,
                    triangles,
                    &Binding::Neutral,
                    neutral,
                ));
            }
        }
        parts
    }

    #[allow(clippy::too_many_arguments)]
    fn part(
        &mut self,
        app: &mut App,
        container: &PlaytestContainer,
        mesh: Handle<Mesh>,
        stored_material: u32,
        triangles: usize,
        binding: &Binding,
        neutral: NeutralColor,
    ) -> PlaytestPart {
        let (material, image) = match binding {
            Binding::Textured { material, key } => {
                self.subject(container).textured_parts += 1;
                (material.clone(), Some(key.clone()))
            }
            Binding::Neutral => {
                self.subject(container).neutral_parts += 1;
                (self.neutral(app, container, neutral), None)
            }
        };
        PlaytestPart {
            mesh,
            material,
            stored_material,
            triangles,
            image,
        }
    }

    /// Finishes the binder into the report.
    #[must_use]
    pub fn finish(mut self) -> PlaytestTextureReport {
        for subject in self.subjects.values_mut() {
            for entry in &mut subject.unresolved {
                if let Some(index) = entry
                    .source_id
                    .rsplit('-')
                    .next()
                    .and_then(|digits| digits.parse::<u32>().ok())
                {
                    entry.meshes = self
                        .uses
                        .get(&(subject.container_key.clone(), index))
                        .copied()
                        .unwrap_or(0);
                }
            }
            subject.resolved_names.sort();
            subject.resolved_names.dedup();
            subject.unresolved_names.sort();
            subject.unresolved_names.dedup();
        }
        let image_entries: Vec<(usize, BoundImage)> = self
            .images
            .iter()
            .map(|(entry, (_, bound))| (*entry, bound.clone()))
            .collect();
        // Images are shared by the archive, so every subject that bound one lists it.
        for subject in self.subjects.values_mut() {
            subject.images = image_entries.iter().map(|(_, b)| b.clone()).collect();
        }
        PlaytestTextureReport {
            claims: [
                PLAYTEST_TEXTURE_ARCHIVE_IS_DESIGNED,
                PLAYTEST_TEXTURE_NAME_READING_IS_DESIGNED,
                PLAYTEST_TEXTURE_PRESENTATION_IS_PROVISIONAL,
            ],
            archive: self.archive.path().to_owned(),
            archive_sha256: self.archive.sha256().to_owned(),
            group: self.archive.group().to_owned(),
            selection: self.archive.selection().to_owned(),
            name_reading: NAME_READING.code(),
            subjects: self.subjects.into_values().collect(),
        }
    }
}

/// Whether the image carries keyed coverage a mask must honour.
fn keyed(image: &DecodedImage) -> bool {
    CoverageSource::from_source(image.alpha_source()).is_ok_and(CoverageSource::carries_coverage)
}

/// The neutral development material of #648.
#[must_use]
pub fn neutral_material(color: [f32; 4]) -> StandardMaterial {
    StandardMaterial {
        base_color: Color::srgba(color[0], color[1], color[2], color[3]),
        metallic: 0.0,
        // Both sides are drawn: front-face winding is F17's open question, so
        // culling would make the frame depend on an unmeasured rule.
        cull_mode: None,
        ..Default::default()
    }
}

fn textured_material(image: Handle<Image>, masked: bool) -> StandardMaterial {
    StandardMaterial {
        base_color: Color::WHITE,
        base_color_texture: Some(image),
        metallic: 0.0,
        perceptual_roughness: 1.0,
        cull_mode: None,
        alpha_mode: if masked {
            bevy::prelude::AlphaMode::Mask(MASK_CUTOFF)
        } else {
            bevy::prelude::AlphaMode::Opaque
        },
        ..Default::default()
    }
}

/// Converts one decoded image to an sRGB RGBA8 engine image through the F17-B
/// expansion and coverage policies, under the provisional presentation.
fn to_bevy_image(decoded: &DecodedImage) -> (Image, u32, Option<&'static str>) {
    let (coverage, note) = match CoverageSource::from_source(decoded.alpha_source()) {
        Ok(source) => (source, None),
        // The stored coverage plane is unknown: treated as opaque and said so.
        Err(_) => (
            CoverageSource::Opaque,
            Some("alpha_source_unknown_treated_opaque"),
        ),
    };
    let extent = decoded.extent();
    let mut data = Vec::with_capacity(extent.width as usize * extent.height as usize * 4);
    let mut translucent = 0u32;
    for y in 0..extent.height {
        for x in 0..extent.width {
            // A texel the policy refuses (it cannot, for a decoded image) is drawn
            // magenta, so a fault is visible rather than a plausible colour.
            let rgba = expand_texel_any(decoded, coverage, x, y).unwrap_or([255, 0, 255, 255]);
            if rgba[3] != u8::MAX {
                translucent += 1;
            }
            data.extend_from_slice(&rgba);
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: extent.width,
            height: extent.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Nearest,
        ..ImageSamplerDescriptor::default()
    });
    (image, translucent, note)
}

fn expand_texel_any(
    decoded: &DecodedImage,
    coverage: CoverageSource,
    x: u32,
    y: u32,
) -> Option<[u8; 4]> {
    if crate::render::rgb565::stores_texel_words(decoded.format()) {
        expand_texel(decoded, coverage, &ExpansionPolicy::DECIDED, x, y).ok()
    } else {
        let texel = decoded.texel(x, y)?;
        let alpha = coverage_byte(decoded, coverage, x, y).ok()?;
        Some([*texel.first()?, *texel.get(1)?, *texel.get(2)?, alpha])
    }
}

/// Cuts a merged world mesh back into one mesh per uploaded group, using the
/// per-group vertex and triangle counts the merge recorded. `None` when the
/// buffers do not add up to those counts.
fn slice_groups(world_mesh: &WorldMesh) -> Option<Vec<(u32, Mesh)>> {
    let mesh = world_mesh.mesh();
    let positions = match mesh.attribute(Mesh::ATTRIBUTE_POSITION)? {
        VertexAttributeValues::Float32x3(values) => values,
        _ => return None,
    };
    let normals = match mesh.attribute(Mesh::ATTRIBUTE_NORMAL) {
        Some(VertexAttributeValues::Float32x3(values)) => Some(values),
        _ => None,
    };
    let uvs = match mesh.attribute(Mesh::ATTRIBUTE_UV_0) {
        Some(VertexAttributeValues::Float32x2(values)) => Some(values),
        _ => None,
    };
    let indices: Vec<u32> = match mesh.indices()? {
        Indices::U32(values) => values.clone(),
        Indices::U16(values) => values.iter().map(|i| u32::from(*i)).collect(),
    };
    let mut vertex_base = 0usize;
    let mut index_base = 0usize;
    let mut out = Vec::new();
    for group in world_mesh.groups() {
        let vertex_end = vertex_base.checked_add(group.vertices())?;
        let index_end = index_base.checked_add(group.triangles().checked_mul(3)?)?;
        if vertex_end > positions.len() || index_end > indices.len() {
            return None;
        }
        let base = u32::try_from(vertex_base).ok()?;
        let end = u32::try_from(vertex_end).ok()?;
        let mut local = Vec::with_capacity(index_end - index_base);
        for index in &indices[index_base..index_end] {
            if *index < base || *index >= end {
                return None;
            }
            local.push(index - base);
        }
        let mut part = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        part.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            positions[vertex_base..vertex_end].to_vec(),
        );
        if let Some(normals) = normals {
            part.insert_attribute(
                Mesh::ATTRIBUTE_NORMAL,
                normals[vertex_base..vertex_end].to_vec(),
            );
        }
        if let Some(uvs) = uvs {
            part.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs[vertex_base..vertex_end].to_vec());
        }
        part.insert_indices(Indices::U32(local));
        out.push((group.material(), part));
        vertex_base = vertex_end;
        index_base = index_end;
    }
    if vertex_base != positions.len() || index_base != indices.len() {
        return None;
    }
    Some(out)
}
