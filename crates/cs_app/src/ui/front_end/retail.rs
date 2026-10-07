//! The original front-end screen inventory and its artwork (`retail`)
//! (`specs/F45-main-menu-pandora-cabin-briefing-and-flight-check.md`, stage
//! `### F45-D`). Shared contract: `docs/contracts/CLI-EVIDENCE.md`.
//!
//! # What this is for
//!
//! F45-D's minimum scenario is *"capture and review all original front-end
//! screens and navigation paths"*, and the sheet gives this stage the `retail`
//! capability. This module is where the original installation is actually
//! read:
//!
//! * [`FrontEndScreens::inventory`] walks **both** places the original
//!   front-end artwork lives — the shared UI/HUD texture archive
//!   `ZBD/rimage.zbd` (254 stored textures, measured) and the graphics members
//!   of `GOSDATA/ASSETS/crimson.rof` (the `ASSETS/GRAPHICS/**` images, the
//!   `*_BACKGROUND` screens among them) — and records every image with its
//!   measured extent, format and digest. Nothing is skipped: an image that
//!   will not decode is recorded with the reason instead of dropped.
//! * [`FrontEndScreens::artwork`] decodes one recorded image into the
//!   [`Artwork`] the GPU capture draws, through the production ZBD decoder or
//!   the standard `image` decoder the workspace already ships with Bevy.
//!
//! # What is **not** claimed
//!
//! * **Not the original's screen list.** Which original image belongs to which
//!   front-end screen is unread; nothing here maps an image onto a
//!   [`super::Screen`](super::Screen). That binding is resolving task **#742**
//!   (`docs/findings/2026-10-07-f45-b-original-asset-screen-decks.md`).
//! * **Not a hotspot layout.** No original button rectangle is read anywhere,
//!   so a retail capture draws the artwork alone and never a button region.
//! * **Not a run of the original executable.** `retail` here means read access
//!   to the owner's files. What the original *renders* when it runs needs an
//!   original run (REF-OWNER-FIRST-CAPTURE), which no agent can produce.
//!
//! # The selection rule
//!
//! [`MINIMUM_SCREEN_EXTENT`] is the stage's one designed selection rule: an
//! image is **screen-capable** when it can cover the smallest front-end screen
//! this installation stores — `mainmenu` and `escapemenu` in `rimage.zbd`,
//! both measured 640x480. The rule is designed and declared here, never
//! presented as the original's own classification; every image is in the inventory either
//! way, with its measured extent, so a reader can reselect.

use std::fmt;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use cs_assets::install::{content_fingerprint, discover, fingerprint, sha256};
use cs_assets::rof::{RofSource, mount_rof_into};
use cs_assets::vfs::{ContentSession, INSTALL_NAMESPACE, MountBuilder, SessionBuilder};
use cs_content::textures::{TEXTURE_ARCHIVE_VARIANT, TextureCatalog, TextureId, TextureRef};
use cs_formats::texture::DecodedImage;
use cs_types::asset_id::{AssetKey, MountId, MountNamespace, PrecedenceClass, ResolveContext};

use super::capture::Artwork;
use crate::render::rgb565::{
    CoverageSource, ExpansionPolicy, coverage_byte, expand_texel, stores_texel_words,
};

/// The shared UI/HUD texture archive, installation-relative as discovery
/// spells it.
pub const UI_IMAGE_CONTAINER: &str = "ZBD/rimage.zbd";

/// The archive key that reaches [`UI_IMAGE_CONTAINER`] through the texture
/// catalog's install namespace.
pub const UI_IMAGE_KEY: &str = "zbd/rimage.zbd";

/// The container the original front-end screen artwork is mounted from.
pub const SCREEN_CONTAINER: &str = "GOSDATA/ASSETS/crimson.rof";

/// The member prefix every front-end graphics image lives under inside
/// [`SCREEN_CONTAINER`].
pub const GRAPHICS_PREFIX: &str = "ASSETS/GRAPHICS/";

/// The smallest front-end screen this installation stores: `mainmenu` and
/// `escapemenu` in [`UI_IMAGE_CONTAINER`], both measured 640x480.
///
/// This is the declared selection rule of [`OriginalImage::screen_capable`]:
/// an image that cannot cover this extent cannot be a full front-end screen.
/// It is a designed rule stated in this module, never attributed to the
/// original — and every image of both sources is in the inventory with its
/// measured extent regardless, so a reader can reselect.
pub const MINIMUM_SCREEN_EXTENT: (u32, u32) = (640, 480);

/// Where an original image is stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageSource {
    /// The shared UI/HUD texture archive, [`UI_IMAGE_CONTAINER`].
    UiArchive,
    /// The screen container, [`SCREEN_CONTAINER`].
    ScreenContainer,
}

impl ImageSource {
    /// The source's name as the artifact spells it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UiArchive => "zbd:rimage.zbd",
            Self::ScreenContainer => "rof:crimson.rof",
        }
    }
}

/// One original image, as the inventory recorded it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OriginalImage {
    /// Where it is stored.
    pub source: ImageSource,
    /// Its stored name (texture name) or member spelling.
    pub name: String,
    /// The format, by magic bytes (`png`, `jpeg`, `tga`, `bmp`, …) or
    /// `zbd-texture` for a stored game texture.
    pub format: String,
    /// Measured width, or `0` when no header could be read.
    pub width: u32,
    /// Measured height, or `0` when no header could be read.
    pub height: u32,
    /// Bytes: the decoded texel plane for a game texture, the decoded member
    /// for a container image.
    pub bytes: u64,
    /// SHA-256: the decoded texel plane for a game texture, the stored member
    /// extent (as the mount recorded it) for a container image.
    pub sha256: String,
    /// Whether production code decoded the whole image, not just its header.
    pub decodable: bool,
    /// Why it did not decode; empty when it did.
    pub refusal: String,
}

impl OriginalImage {
    /// The stage's declared selection rule: large enough to be a full
    /// front-end screen. See [`MINIMUM_SCREEN_EXTENT`].
    #[must_use]
    pub fn screen_capable(&self) -> bool {
        self.width >= MINIMUM_SCREEN_EXTENT.0 && self.height >= MINIMUM_SCREEN_EXTENT.1
    }

    /// A file-name-safe stem for a capture PNG: every character outside
    /// `[A-Za-z0-9._-]` folded to `_`.
    #[must_use]
    pub fn file_stem(&self) -> String {
        self.name
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
                    character
                } else {
                    '_'
                }
            })
            .collect::<String>()
    }
}

/// The whole measured inventory of original front-end screen artwork.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrontEndInventory {
    /// SHA-256 of the installation, as discovery measured it.
    pub install_sha256: String,
    /// SHA-256 of the canonical content, as discovery measured it.
    pub content_sha256: String,
    /// Every image of both sources, in source order.
    pub images: Vec<OriginalImage>,
}

impl FrontEndInventory {
    /// Every screen-capable image ([`OriginalImage::screen_capable`]).
    #[must_use]
    pub fn screen_capable(&self) -> Vec<&OriginalImage> {
        self.images
            .iter()
            .filter(|image| image.screen_capable())
            .collect()
    }

    /// How many images did not decode.
    #[must_use]
    pub fn undecodable(&self) -> usize {
        self.images.iter().filter(|image| !image.decodable).count()
    }

    /// The inventory as the artifact this task writes.
    #[must_use]
    pub fn json(&self) -> String {
        let images: Vec<String> = self
            .images
            .iter()
            .map(|image| {
                format!(
                    "{{\"source\":\"{}\",\"name\":{},\"format\":{},\"width\":{},\"height\":{},\
                     \"bytes\":{},\"sha256\":{},\"decodable\":{},\"screen_capable\":{},\
                     \"refusal\":{}}}",
                    image.source.as_str(),
                    json(&image.name),
                    json(&image.format),
                    image.width,
                    image.height,
                    image.bytes,
                    json(&image.sha256),
                    image.decodable,
                    image.screen_capable(),
                    json(&image.refusal)
                )
            })
            .collect();
        format!(
            "{{\"schema_version\":1,\"task\":\"F45-D\",\"install_sha256\":{},\"content_sha256\":{},\
             \"minimum_screen_extent\":[{},{}],\"images\":[{}],\"image_count\":{},\
             \"screen_capable_count\":{},\"undecodable_count\":{}}}",
            json(&self.install_sha256),
            json(&self.content_sha256),
            MINIMUM_SCREEN_EXTENT.0,
            MINIMUM_SCREEN_EXTENT.1,
            images.join(","),
            self.images.len(),
            self.screen_capable().len(),
            self.undecodable()
        )
    }
}

fn json(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if (control as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// Why the original screen artwork could not be read.
#[derive(Debug)]
pub enum RetailError {
    /// Production discovery refused the installation.
    Discovery {
        /// The path asked about.
        path: String,
        /// The operating system's message.
        reason: String,
    },
    /// The install/ROF session or mount could not be built.
    Session {
        /// Which one, and why.
        reason: String,
    },
    /// The UI texture archive would not open.
    Archive {
        /// The refusal.
        reason: String,
    },
    /// A texture would not resolve or decode.
    Texture {
        /// The stored name.
        name: String,
        /// The refusal.
        reason: String,
    },
    /// A container member would not read.
    Member {
        /// The member spelling.
        name: String,
        /// The refusal.
        reason: String,
    },
    /// The image decoder refused the bytes.
    Image {
        /// The member or texture name.
        name: String,
        /// The refusal, verbatim.
        reason: String,
    },
}

impl fmt::Display for RetailError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Discovery { path, reason } => {
                write!(f, "discovery refused {path}: {reason}")
            }
            Self::Session { reason } => {
                write!(f, "the content session could not be built: {reason}")
            }
            Self::Archive { reason } => write!(f, "the UI texture archive did not open: {reason}"),
            Self::Texture { name, reason } => {
                write!(f, "the stored texture {name} was refused: {reason}")
            }
            Self::Member { name, reason } => {
                write!(f, "the member {name} could not be read: {reason}")
            }
            Self::Image { name, reason } => {
                write!(f, "the image {name} could not be decoded: {reason}")
            }
        }
    }
}

impl std::error::Error for RetailError {}

/// The original front-end screen artwork, opened once over the installation.
pub struct FrontEndScreens {
    install_root: PathBuf,
    session: ContentSession,
    catalog: TextureCatalog,
    ui_key: AssetKey,
    rof: RofSource,
    install_sha256: String,
    content_sha256: String,
}

impl FrontEndScreens {
    /// Discovers `install_root`, mounts the installation and the screen
    /// container, and opens the UI texture archive over them.
    ///
    /// The installation is only ever **read**: discovery, the session's install
    /// mount and the ROF mount all take it read-only.
    ///
    /// # Errors
    ///
    /// [`RetailError::Discovery`], [`RetailError::Session`],
    /// [`RetailError::Archive`] or [`RetailError::Member`].
    pub fn open(install_root: &Path) -> Result<Self, RetailError> {
        let found = discover(install_root).map_err(|error| RetailError::Discovery {
            path: install_root.display().to_string(),
            reason: error.to_string(),
        })?;
        let install_sha256 = fingerprint(&found.manifest).to_hex();
        let content_sha256 = content_fingerprint(&found.manifest).to_hex();
        let context = ResolveContext::new(fingerprint(&found.manifest));

        let mut builder = SessionBuilder::new(context.clone());
        builder
            .mount_installation(install_root, &found.diagnosis)
            .map_err(|error| RetailError::Session {
                reason: error.to_string(),
            })?;
        let session = builder.open();

        let ui_key =
            AssetKey::from_spelling(INSTALL_NAMESPACE, UI_IMAGE_KEY, TEXTURE_ARCHIVE_VARIANT)
                .map_err(|error| RetailError::Archive {
                    reason: error.to_string(),
                })?;
        let catalog = TextureCatalog::open(&session, std::slice::from_ref(&ui_key));
        if let Some((_, error)) = catalog.failures().next() {
            return Err(RetailError::Archive {
                reason: error.to_string(),
            });
        }

        let record = found
            .manifest
            .files
            .iter()
            .find(|record| {
                record
                    .relative_spelling
                    .as_str()
                    .eq_ignore_ascii_case(SCREEN_CONTAINER)
            })
            .ok_or_else(|| RetailError::Member {
                name: SCREEN_CONTAINER.to_owned(),
                reason: "the installation inventories no such container".to_owned(),
            })?;
        let path = install_root.join(record.relative_spelling.as_str());
        let mount = MountBuilder::new(
            MountId::new("rof-front-end-screens").map_err(|error| RetailError::Session {
                reason: error.to_string(),
            })?,
            MountNamespace::new(INSTALL_NAMESPACE).map_err(|error| RetailError::Session {
                reason: error.to_string(),
            })?,
            PrecedenceClass::Shared,
            SCREEN_CONTAINER,
        )
        .retail();
        let mut rof_builder = SessionBuilder::new(context);
        let rof = mount_rof_into(&mut rof_builder, mount, &path).map_err(|error| {
            RetailError::Member {
                name: SCREEN_CONTAINER.to_owned(),
                reason: error.to_string(),
            }
        })?;

        Ok(Self {
            install_root: install_root.to_path_buf(),
            session,
            catalog,
            ui_key,
            rof,
            install_sha256,
            content_sha256,
        })
    }

    /// The installation root this inventory was built over.
    #[must_use]
    pub fn install_root(&self) -> &Path {
        &self.install_root
    }

    /// SHA-256 of the installation.
    #[must_use]
    pub fn install_sha256(&self) -> &str {
        &self.install_sha256
    }

    /// SHA-256 of the canonical content.
    #[must_use]
    pub fn content_sha256(&self) -> &str {
        &self.content_sha256
    }

    /// The mounted screen container, for a caller that needs its members.
    #[must_use]
    pub fn screens_container(&self) -> &RofSource {
        &self.rof
    }

    /// Measures **every** image of both sources: extents, formats, digests and
    /// whether production code could decode it.
    ///
    /// Both halves are walked to the end, so the inventory is the complete set
    /// rather than a selection; [`FrontEndInventory::screen_capable`] applies
    /// the stage's declared rule afterwards.
    ///
    /// # Errors
    ///
    /// [`RetailError::Texture`] when a stored texture refuses to resolve or
    /// decode through the production reader, [`RetailError::Member`] when a
    /// container member will not read. A single image the *image decoder*
    /// refuses is not an error: it is recorded with [`OriginalImage::refusal`].
    pub fn inventory(&self) -> Result<FrontEndInventory, RetailError> {
        let mut images = Vec::new();

        // 1. The shared UI/HUD texture archive: every entry, decoded through
        //    the production ZBD reader (F08-B), never by a header guess.
        for archive in self.catalog.archives() {
            let count = archive.ids().count();
            for entry_index in 0..count {
                let resolved = self
                    .catalog
                    .resolve_entry(&self.session, &self.ui_key, entry_index)
                    .map_err(|error| RetailError::Texture {
                        name: format!("entry {entry_index}"),
                        reason: error.to_string(),
                    })?;
                let decoded = self
                    .catalog
                    .decode(&self.session, &resolved)
                    .map_err(|error| RetailError::Texture {
                        name: resolved.id().name.clone(),
                        reason: error.to_string(),
                    })?;
                let extent = decoded.extent();
                images.push(OriginalImage {
                    source: ImageSource::UiArchive,
                    name: resolved.id().name.clone(),
                    format: "zbd-texture".to_owned(),
                    width: extent.width,
                    height: extent.height,
                    bytes: decoded.texels().len() as u64,
                    sha256: sha256(decoded.texels()).to_hex(),
                    decodable: true,
                    refusal: String::new(),
                });
            }
        }

        // 2. The screen container's graphics members: every image file under
        //    ASSETS/GRAPHICS/, decoded by the workspace's image decoder.
        let graphics: Vec<_> = self
            .rof
            .members()
            .filter(|member| {
                let spelling = member.spelling.to_ascii_uppercase();
                spelling.starts_with(GRAPHICS_PREFIX) && image_extension(&spelling).is_some()
            })
            .cloned()
            .collect();
        for member in graphics {
            let key = AssetKey::from_spelling(
                self.rof.namespace().as_str(),
                &member.spelling,
                TEXTURE_ARCHIVE_VARIANT,
            )
            .map_err(|error| RetailError::Member {
                name: member.spelling.clone(),
                reason: error.to_string(),
            })?;
            let read = self.rof.read(&key).map_err(|error| RetailError::Member {
                name: member.spelling.clone(),
                reason: error.to_string(),
            })?;
            let declared = image_extension(&member.spelling.to_ascii_uppercase())
                .unwrap_or("unknown")
                .to_owned();
            let (format, width, height, decodable, refusal) =
                match decode_image(&member.spelling, &read.data) {
                    Ok((format, width, height, _)) => (format, width, height, true, String::new()),
                    Err(reason) => (declared, 0, 0, false, reason),
                };
            images.push(OriginalImage {
                source: ImageSource::ScreenContainer,
                name: member.spelling.clone(),
                format,
                width,
                height,
                bytes: read.decoded_len,
                sha256: member.sha256.to_hex(),
                decodable,
                refusal,
            });
        }

        Ok(FrontEndInventory {
            install_sha256: self.install_sha256.clone(),
            content_sha256: self.content_sha256.clone(),
            images,
        })
    }

    /// Decodes one recorded image into the [`Artwork`] the GPU capture draws.
    ///
    /// # Errors
    ///
    /// [`RetailError::Image`] when the image is not in the inventory's source
    /// or will not decode, [`RetailError::Texture`]/[`RetailError::Member`]
    /// for the read itself.
    pub fn artwork(&self, image: &OriginalImage) -> Result<Artwork, RetailError> {
        match image.source {
            ImageSource::UiArchive => {
                let id = self
                    .catalog
                    .archives()
                    .flat_map(|archive| archive.ids())
                    .find(|id| id.name == image.name)
                    .cloned()
                    .ok_or_else(|| RetailError::Texture {
                        name: image.name.clone(),
                        reason: "the UI archive no longer lists this texture".to_owned(),
                    })?;
                let resolved = self
                    .catalog
                    .resolve_entry(&self.session, &self.ui_key, id.entry_index)
                    .map_err(|error| RetailError::Texture {
                        name: image.name.clone(),
                        reason: error.to_string(),
                    })?;
                let decoded = self
                    .catalog
                    .decode(&self.session, &resolved)
                    .map_err(|error| RetailError::Texture {
                        name: image.name.clone(),
                        reason: error.to_string(),
                    })?;
                let extent = decoded.extent();
                let rgba = rgba_from_decoded(&decoded).map_err(|reason| RetailError::Image {
                    name: image.name.clone(),
                    reason,
                })?;
                Artwork::new(extent.width, extent.height, rgba).map_err(|error| {
                    RetailError::Image {
                        name: image.name.clone(),
                        reason: error.to_string(),
                    }
                })
            }
            ImageSource::ScreenContainer => {
                let key = AssetKey::from_spelling(
                    self.rof.namespace().as_str(),
                    &image.name,
                    TEXTURE_ARCHIVE_VARIANT,
                )
                .map_err(|error| RetailError::Member {
                    name: image.name.clone(),
                    reason: error.to_string(),
                })?;
                let read = self.rof.read(&key).map_err(|error| RetailError::Member {
                    name: image.name.clone(),
                    reason: error.to_string(),
                })?;
                let (_, width, height, rgba) =
                    decode_image(&image.name, &read.data).map_err(|reason| RetailError::Image {
                        name: image.name.clone(),
                        reason,
                    })?;
                Artwork::new(width, height, rgba).map_err(|error| RetailError::Image {
                    name: image.name.clone(),
                    reason: error.to_string(),
                })
            }
        }
    }

    /// The recorded texture id of a UI-archive image, for a test that needs
    /// the stored identity rather than the name.
    #[must_use]
    pub fn ui_texture_id(&self, name: &str) -> Option<TextureId> {
        self.catalog
            .archives()
            .flat_map(|archive| archive.ids())
            .find(|id| id.name == name)
            .cloned()
    }

    /// Resolves a stored texture name the production way, for a caller that
    /// wants the attempts the lookup made.
    ///
    /// # Errors
    ///
    /// [`RetailError::Texture`] for a name the archive refuses.
    pub fn resolve(
        &self,
        name: &str,
    ) -> Result<cs_content::textures::ResolvedTexture, RetailError> {
        let reference = TextureRef::new(self.ui_key.clone(), name);
        self.catalog
            .resolve(&self.session, &reference)
            .map_err(|error| RetailError::Texture {
                name: name.to_owned(),
                reason: error.to_string(),
            })
    }
}

/// The extensions this inventory treats as an image.
fn image_extension(upper: &str) -> Option<&'static str> {
    match upper.rsplit('.').next()? {
        "PNG" => Some("png"),
        "JPG" | "JPEG" => Some("jpeg"),
        "TGA" => Some("tga"),
        "BMP" => Some("bmp"),
        _ => None,
    }
}

/// Decodes one container image: format, extent and RGBA8 pixels, row-major
/// from the top.
///
/// The header is read first, so the extent of an image whose pixels later
/// refuse to decode is still measured (and reported as such). `name` is the
/// member spelling: **TGA carries no magic bytes at all**, so when the sniffed
/// format is unknown the extension is the only identification this corpus
/// offers, and the decoder is told the format explicitly instead of the image
/// being reported as "format could not be determined".
fn decode_image(name: &str, bytes: &[u8]) -> Result<(String, u32, u32, Vec<u8>), String> {
    let mut reader = image::ImageReader::new(Cursor::new(bytes.to_vec()))
        .with_guessed_format()
        .map_err(|error| error.to_string())?;
    if reader.format().is_none()
        && let Some(extension) = name.rsplit('.').next()
        && let Some(format) = image::ImageFormat::from_extension(extension)
    {
        reader.set_format(format);
    }
    let format = reader
        .format()
        .map(|format| {
            format
                .extensions_str()
                .first()
                .map(|extension| (*extension).to_owned())
                .unwrap_or_else(|| format!("{format:?}").to_ascii_lowercase())
        })
        .unwrap_or_else(|| "unknown".to_owned());
    let decoded = reader.decode().map_err(|error| error.to_string())?;
    let rgba = decoded.into_rgba8();
    let (width, height) = rgba.dimensions();
    Ok((format, width, height, rgba.into_raw()))
}

/// A stored game texture expanded to RGBA8 through the production RGB565
/// policy ([`ExpansionPolicy::DECIDED`], F17-B) and the same coverage rule
/// the playtest draws with.
fn rgba_from_decoded(decoded: &DecodedImage) -> Result<Vec<u8>, String> {
    let source =
        CoverageSource::from_source(decoded.alpha_source()).unwrap_or(CoverageSource::Opaque);
    let extent = decoded.extent();
    let mut rgba = Vec::with_capacity(extent.width as usize * extent.height as usize * 4);
    for y in 0..extent.height {
        for x in 0..extent.width {
            let pixel = if stores_texel_words(decoded.format()) {
                expand_texel(decoded, source, &ExpansionPolicy::DECIDED, x, y)
                    .map_err(|error| error.to_string())?
            } else {
                let texel = decoded
                    .texel(x, y)
                    .ok_or_else(|| format!("texel ({x}, {y}) is out of the extent"))?;
                let alpha =
                    coverage_byte(decoded, source, x, y).map_err(|error| error.to_string())?;
                [
                    *texel.first().ok_or("a texel with no red channel")?,
                    *texel.get(1).ok_or("a texel with no green channel")?,
                    *texel.get(2).ok_or("a texel with no blue channel")?,
                    alpha,
                ]
            };
            rgba.extend_from_slice(&pixel);
        }
    }
    Ok(rgba)
}
