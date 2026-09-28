//! Texture archives on a content session, the image catalog they populate,
//! and the handoff to the GPU upload boundary
//! (`specs/F08-texture-archives-and-conventional-image-decoding.md`, stage
//! `### F08-C`; contract `docs/contracts/IDENTITY-CONTENT.md`).
//!
//! Stage F08-B made the readers; this stage connects them:
//!
//! * **the producer — the VFS.** [`TextureArchive::open`] resolves a texture
//!   archive key in a [`ContentSession`] (so `world/default/texture.zbd`
//!   names the *selected world's* archive), reads it through
//!   [`ZbdContainer::open`] (whose F06-A dispatch must route it to the
//!   texture family) and reads it with the production
//!   [`read_zbd_textures`]. The archive owns its bytes.
//! * **identity.** A [`TextureId`] is the installation-relative archive
//!   path, the mount that served it, the key's variant, the entry index and
//!   the stored name (spec F08 non-negotiable #4). Two chapter archives
//!   that both store `sky` give two ids that never compare equal.
//! * **the lookup contract.** [`TextureCatalog::resolve`] takes a
//!   [`TextureRef`] — archive key plus stored name — and returns one exact
//!   origin with its ordered attempts: the VFS trace that chose the archive,
//!   then the entries that hold the name. There is no search across other
//!   archives: a name the archive does not hold is
//!   [`TextureResolveError::NotFound`], and a name it holds twice is
//!   [`TextureResolveError::Duplicate`] with both entry indices.
//! * **the catalog.** [`TextureCatalog::records`] gives every texture a row
//!   with the contract's fields, and a failed archive stays a row with its
//!   diagnostic ("collections cannot exclude failed entries").
//! * **the consumer — the GPU upload boundary.**
//!   [`TextureCatalog::prepare_upload`] turns a resolved texture into a
//!   [`TextureUpload`]: tightly packed rows, top row first, channel values
//!   exactly as decoded, plus the presentation decisions the evidence has
//!   not settled ([`PresentationUnknown`]). No color-space conversion, no
//!   565 channel expansion, no alpha baking and no mip generation happen
//!   here: they happen once, in the renderer adapter (F17-B), which must
//!   read [`TextureUpload::unknowns`] before it chooses anything
//!   (non-negotiable #3).
//!
//! # Teardown, retry and stale state
//!
//! * A catalog owns every archive's bytes, so closing its session does not
//!   invalidate what was already resolved or uploaded.
//! * It is stamped with the [`SessionGeneration`] that read it:
//!   [`TextureCatalog::resolve`] and [`TextureCatalog::prepare_upload`]
//!   refuse any other session, so a texture resolved for world `c1` is never
//!   served after a switch to world `c2`; a [`ResolvedTexture`] from another
//!   catalog of the same session is refused too.
//! * [`TextureCatalog::retry_failed`] reopens only the failed archives of
//!   the same session and keeps the ones that loaded.
//!
//! Only the ZBD texture package is catalogued here; the conventional BMP and
//! TGA readers have no archive-member role yet. Design decisions and
//! unknowns: `docs/findings/2026-09-28-f08-c-texture-catalog-and-upload-boundary.md`.

use std::fmt;
use std::ops::Range;

use cs_assets::install::sha256;
use cs_assets::vfs::{ContentSession, ResolutionTrace, SessionGeneration};
use cs_assets::zbd::{ZbdContainer, ZbdError};
use cs_formats::io::AllocationBudget;
use cs_formats::texture::{
    AlphaSource, AlphaTest, ColorSpace, DecodedFormat, DecodedImage, Extent, ImageDescriptor,
    TextureError, ZbdStretch, ZbdTextureError, decode_base_level, read_zbd_textures,
};
use cs_formats::zbd::ZbdFamily;
use cs_types::asset_id::{AssetKey, AssetVariant, MountId, SourceSpan};
use cs_types::evidence::ContentHash;
use cs_types::install::{ParseState, RelativePath};

/// Stable identity of one stored texture.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TextureId {
    /// Installation-relative path of the archive that stores it.
    pub archive: RelativePath,
    /// The mount that served the archive.
    pub mount: MountId,
    /// The variant of the key the archive was resolved with.
    pub variant: AssetVariant,
    /// Position in the archive's texture table. Names are not unique inside
    /// an archive; `(entry_index, name)` is.
    pub entry_index: usize,
    /// The stored name, as the archive spells it.
    pub name: String,
}

impl fmt::Display for TextureId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}#{}:{} ({}, variant {})",
            self.archive, self.entry_index, self.name, self.mount, self.variant
        )
    }
}

/// What a caller asks for: the texture stored as `name` in the archive
/// `archive` resolves to under the session's context.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TextureRef {
    /// The archive key, e.g. `world/default/texture.zbd`.
    pub archive: AssetKey,
    /// The stored name, compared exactly (no case folding, no aliases).
    pub name: String,
}

impl TextureRef {
    /// A reference to `name` inside `archive`.
    pub fn new(archive: AssetKey, name: &str) -> Self {
        Self {
            archive,
            name: name.to_owned(),
        }
    }
}

impl fmt::Display for TextureRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.archive, self.name)
    }
}

/// Why a texture archive could not be opened.
#[derive(Debug)]
pub enum TextureArchiveError {
    /// Resolving, reading or dispatching the container failed.
    Container(ZbdError),
    /// The container was routed to a family other than textures.
    WrongFamily {
        /// The key that was opened.
        key: AssetKey,
        /// The family the dispatch named.
        family: ZbdFamily,
    },
    /// The texture package reader refused the container.
    Package(ZbdTextureError),
}

impl TextureArchiveError {
    /// Stable lowercase identifier for logs and catalog rows.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Container(error) => error.code(),
            Self::WrongFamily { .. } => "wrong_family",
            Self::Package(error) => error.code(),
        }
    }
}

impl fmt::Display for TextureArchiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Container(error) => write!(f, "{error}"),
            Self::WrongFamily { key, family } => write!(
                f,
                "{key} is routed to the {} family, not to texture archives",
                family.as_str()
            ),
            Self::Package(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for TextureArchiveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Container(error) => Some(error),
            Self::WrongFamily { .. } => None,
            Self::Package(error) => Some(error),
        }
    }
}

impl From<ZbdError> for TextureArchiveError {
    fn from(error: ZbdError) -> Self {
        Self::Container(error)
    }
}

impl From<ZbdTextureError> for TextureArchiveError {
    fn from(error: ZbdTextureError) -> Self {
        Self::Package(error)
    }
}

/// One texture of an archive: the reader's facts, and where its stored
/// level lies in the archive's owned bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
struct TextureEntry {
    id: TextureId,
    label: String,
    stretch: ZbdStretch,
    descriptor: ImageDescriptor,
    stored: Range<usize>,
}

/// One texture archive, read through a content session.
#[derive(Clone, Debug)]
pub struct TextureArchive {
    container: ZbdContainer,
    trace: ResolutionTrace,
    entries: Vec<TextureEntry>,
}

impl TextureArchive {
    /// Resolves `key` in `session`, reads the container it names and reads
    /// it as a ZBD texture package.
    ///
    /// # Errors
    ///
    /// [`TextureArchiveError::Container`] when the key does not resolve to
    /// exactly one origin, cannot be read or is not routed at all;
    /// [`TextureArchiveError::WrongFamily`] when it is routed elsewhere;
    /// [`TextureArchiveError::Package`] when the texture reader refuses it.
    pub fn open(session: &ContentSession, key: &AssetKey) -> Result<Self, TextureArchiveError> {
        let trace = session.resolve(key).map_err(ZbdError::from)?;
        let trace = trace.resolved().trace.clone();
        let container = ZbdContainer::open(session, key)?;
        if container.family() != ZbdFamily::Texture {
            return Err(TextureArchiveError::WrongFamily {
                key: key.clone(),
                family: container.family(),
            });
        }
        let bytes = container.bytes();
        let mut budget = AllocationBudget::with_defaults(container.label());
        let package = read_zbd_textures(container.label(), bytes, &mut budget)?;
        let base = bytes.as_ptr() as usize;
        let entries = package
            .textures()
            .iter()
            .map(|texture| {
                // The reader borrows the stored level from `bytes`, so its
                // position is the distance between the two slices.
                let start = texture.stored().as_ptr() as usize - base;
                TextureEntry {
                    id: TextureId {
                        archive: container.path().clone(),
                        mount: container.mount().clone(),
                        variant: key.variant().clone(),
                        entry_index: texture.entry_index(),
                        name: texture.name().to_owned(),
                    },
                    label: texture.label().to_owned(),
                    stretch: texture.stretch(),
                    descriptor: texture.descriptor().clone(),
                    stored: start..start + texture.stored().len(),
                }
            })
            .collect();
        Ok(Self {
            container,
            trace,
            entries,
        })
    }

    /// The key the archive was resolved with.
    pub fn key(&self) -> &AssetKey {
        self.container.key()
    }

    /// The installation-relative path of the archive.
    pub fn path(&self) -> &RelativePath {
        self.container.path()
    }

    /// The immutable origin of the archive's bytes.
    pub fn span(&self) -> &SourceSpan {
        self.container.span()
    }

    /// The session generation that read the archive.
    pub fn generation(&self) -> SessionGeneration {
        self.container.generation()
    }

    /// The VFS attempts that chose this archive.
    pub fn trace(&self) -> &ResolutionTrace {
        &self.trace
    }

    /// Every texture id, in table order.
    pub fn ids(&self) -> impl Iterator<Item = &TextureId> {
        self.entries.iter().map(|entry| &entry.id)
    }

    fn entry(&self, entry_index: usize) -> Option<&TextureEntry> {
        self.entries.get(entry_index)
    }

    fn decode(&self, entry: &TextureEntry) -> Result<DecodedImage, TextureError> {
        let mut budget = AllocationBudget::with_defaults(entry.label.as_str());
        decode_base_level(
            &entry.label,
            &entry.descriptor,
            &self.container.bytes()[entry.stored.clone()],
            &mut budget,
        )
    }
}

/// One step of a texture lookup, in the order it was taken.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextureAttempt {
    /// The VFS resolved the archive key; the trace names every mount it
    /// consulted.
    Archive {
        /// The installation-relative archive the key resolved to.
        archive: RelativePath,
        /// The mount that served it.
        mount: MountId,
        /// The VFS attempts.
        trace: ResolutionTrace,
    },
    /// The archive's table was searched for the exact stored name.
    Name {
        /// The name searched for.
        name: String,
        /// Every entry that stores it, in table order.
        entries: Vec<usize>,
    },
    /// The archive's table was indexed by position, not by name.
    Entry {
        /// The position asked for.
        entry_index: usize,
        /// How many entries the table holds.
        entries: usize,
    },
}

impl fmt::Display for TextureAttempt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Archive {
                archive,
                mount,
                trace,
            } => write!(f, "archive {archive} from {mount} [{trace}]"),
            Self::Name { name, entries } => write!(f, "name {name:?} at entries {entries:?}"),
            Self::Entry {
                entry_index,
                entries,
            } => write!(f, "entry {entry_index} of {entries}"),
        }
    }
}

/// One resolved texture: its exact origin and how it was chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedTexture {
    generation: SessionGeneration,
    id: TextureId,
    archive_span: SourceSpan,
    attempts: Vec<TextureAttempt>,
}

impl ResolvedTexture {
    /// The texture's identity.
    pub fn id(&self) -> &TextureId {
        &self.id
    }

    /// The origin of the archive that stores it.
    pub fn archive_span(&self) -> &SourceSpan {
        &self.archive_span
    }

    /// The ordered attempts: the archive resolution, then the name (or
    /// entry) lookup.
    pub fn attempts(&self) -> &[TextureAttempt] {
        &self.attempts
    }

    /// The session generation that resolved it.
    pub fn generation(&self) -> SessionGeneration {
        self.generation
    }
}

/// Why a texture reference did not resolve to exactly one texture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextureResolveError {
    /// The catalog was read by another session.
    ForeignSession {
        /// The session asking.
        session: SessionGeneration,
        /// The session that read the catalog.
        catalog: SessionGeneration,
    },
    /// The catalog was not opened with this archive key.
    ArchiveNotCatalogued {
        /// The archive key asked for.
        archive: Box<AssetKey>,
    },
    /// The archive is in the catalog but failed to open.
    ArchiveFailed {
        /// The archive key asked for.
        archive: Box<AssetKey>,
        /// The failure's stable code.
        code: &'static str,
        /// The failure's text.
        diagnostic: String,
    },
    /// The archive holds no texture stored under the name.
    NotFound {
        /// What was asked for.
        reference: Box<TextureRef>,
        /// The attempts taken.
        attempts: Vec<TextureAttempt>,
    },
    /// The archive holds the name more than once; equal candidates are
    /// never chosen between silently.
    Duplicate {
        /// What was asked for.
        reference: Box<TextureRef>,
        /// The attempts taken; the last names every entry.
        attempts: Vec<TextureAttempt>,
    },
    /// The archive's table has no entry at the position asked for.
    EntryNotFound {
        /// The archive key asked for.
        archive: Box<AssetKey>,
        /// The attempts taken; the last names the table length.
        attempts: Vec<TextureAttempt>,
    },
    /// A resolved texture handed back to this catalog did not come from it.
    NotFromThisCatalog {
        /// The texture handed in.
        id: Box<TextureId>,
    },
}

impl TextureResolveError {
    /// Stable lowercase identifier.
    pub fn code(&self) -> &'static str {
        match self {
            Self::ForeignSession { .. } => "foreign_session",
            Self::ArchiveNotCatalogued { .. } => "archive_not_catalogued",
            Self::ArchiveFailed { .. } => "archive_failed",
            Self::NotFound { .. } => "texture_not_found",
            Self::Duplicate { .. } => "duplicate_texture_name",
            Self::EntryNotFound { .. } => "texture_entry_not_found",
            Self::NotFromThisCatalog { .. } => "not_from_this_catalog",
        }
    }
}

impl fmt::Display for TextureResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let attempts = |attempts: &[TextureAttempt]| {
            attempts
                .iter()
                .map(TextureAttempt::to_string)
                .collect::<Vec<_>>()
                .join("; ")
        };
        match self {
            Self::ForeignSession { session, catalog } => write!(
                f,
                "the texture catalog was read by {catalog} and cannot serve {session}"
            ),
            Self::ArchiveNotCatalogued { archive } => {
                write!(f, "texture archive {archive} is not in this catalog")
            }
            Self::ArchiveFailed {
                archive,
                code,
                diagnostic,
            } => write!(f, "texture archive {archive} failed ({code}): {diagnostic}"),
            Self::NotFound {
                reference,
                attempts: tried,
            } => write!(f, "no texture {reference}: {}", attempts(tried)),
            Self::Duplicate {
                reference,
                attempts: tried,
            } => write!(
                f,
                "texture {reference} is stored more than once: {}",
                attempts(tried)
            ),
            Self::EntryNotFound {
                archive,
                attempts: tried,
            } => write!(f, "no texture entry in {archive}: {}", attempts(tried)),
            Self::NotFromThisCatalog { id } => {
                write!(f, "texture {id} was not resolved by this catalog")
            }
        }
    }
}

impl std::error::Error for TextureResolveError {}

/// Why a resolved texture could not be handed to the upload boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UploadError {
    /// The texture could not be resolved against this catalog and session.
    Resolve(TextureResolveError),
    /// The stored level did not decode.
    Decode(TextureError),
}

impl UploadError {
    /// Stable lowercase identifier.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Resolve(error) => error.code(),
            Self::Decode(error) => error.code(),
        }
    }
}

impl fmt::Display for UploadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Resolve(error) => write!(f, "{error}"),
            Self::Decode(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for UploadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Resolve(error) => Some(error),
            Self::Decode(error) => Some(error),
        }
    }
}

impl From<TextureResolveError> for UploadError {
    fn from(error: TextureResolveError) -> Self {
        Self::Resolve(error)
    }
}

impl From<TextureError> for UploadError {
    fn from(error: TextureError) -> Self {
        Self::Decode(error)
    }
}

/// A presentation decision the evidence has not settled for one texture.
///
/// The renderer adapter must not guess these away: each one is a reason
/// the texture is not ready for release presentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PresentationUnknown {
    /// The color space of the stored values is not established.
    ColorSpace,
    /// Whether and at which threshold an alpha test applies.
    AlphaTest,
    /// Which texels are transparent.
    AlphaSource,
    /// How a stored 565 word becomes 8-bit channels on the original renderer.
    Rgb565Expansion,
    /// What the archive's stretch word does.
    Stretch,
}

impl PresentationUnknown {
    /// Stable lowercase identifier, used as a catalog unsupported reason.
    pub const fn code(self) -> &'static str {
        match self {
            Self::ColorSpace => "color_space_unknown",
            Self::AlphaTest => "alpha_test_unknown",
            Self::AlphaSource => "alpha_source_unknown",
            Self::Rgb565Expansion => "rgb565_expansion_unknown",
            Self::Stretch => "stretch_unknown",
        }
    }
}

fn presentation_unknowns(entry: &TextureEntry) -> Vec<PresentationUnknown> {
    let descriptor = &entry.descriptor;
    let mut unknowns = Vec::new();
    if descriptor.color_space() == ColorSpace::Unknown {
        unknowns.push(PresentationUnknown::ColorSpace);
    }
    if descriptor.alpha_test() == AlphaTest::Unknown {
        unknowns.push(PresentationUnknown::AlphaTest);
    }
    if descriptor.alpha_source() == AlphaSource::Unknown {
        unknowns.push(PresentationUnknown::AlphaSource);
    }
    let packed_565 = matches!(
        descriptor.palette(),
        Some(cs_formats::texture::Palette::Rgb565(_))
    ) || descriptor.format() == cs_formats::texture::PixelFormat::Rgb565;
    if packed_565 {
        unknowns.push(PresentationUnknown::Rgb565Expansion);
    }
    // No stretch value, `None` included, has a measured renderer meaning.
    unknowns.push(PresentationUnknown::Stretch);
    unknowns
}

/// What the renderer adapter receives for one texture: the decoded values,
/// unchanged, and everything it still has to decide.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextureUpload {
    id: TextureId,
    generation: SessionGeneration,
    image: DecodedImage,
    stretch: ZbdStretch,
    unknowns: Vec<PresentationUnknown>,
}

impl TextureUpload {
    /// The texture this upload is for.
    pub fn id(&self) -> &TextureId {
        &self.id
    }

    /// The session generation that produced it.
    pub fn generation(&self) -> SessionGeneration {
        self.generation
    }

    /// Width and height of the single stored level (no mips are stored and
    /// none are generated here).
    pub fn extent(&self) -> Extent {
        self.image.extent()
    }

    /// Layout of [`Self::rows`].
    pub fn format(&self) -> DecodedFormat {
        self.image.format()
    }

    /// Bytes per row: width times [`DecodedFormat::channels`], no padding.
    pub fn row_bytes(&self) -> usize {
        self.image.extent().width as usize * self.image.format().channels()
    }

    /// Every row, top row first, tightly packed, values as decoded.
    pub fn rows(&self) -> &[u8] {
        self.image.texels()
    }

    /// The separate coverage plane, same order, values as stored.
    pub fn alpha_plane(&self) -> Option<&[u8]> {
        self.image.alpha()
    }

    /// Palette indices for an indexed source, so a palette key can be
    /// applied at presentation instead of being baked in.
    pub fn indices(&self) -> Option<&[u8]> {
        self.image.indices()
    }

    /// The whole decoded image, with its alpha and color-space metadata.
    pub fn image(&self) -> &DecodedImage {
        &self.image
    }

    /// The stored stretch word.
    pub fn stretch(&self) -> ZbdStretch {
        self.stretch
    }

    /// The presentation decisions still open, in a fixed order.
    pub fn unknowns(&self) -> &[PresentationUnknown] {
        &self.unknowns
    }

    /// Whether nothing is left open for release presentation.
    pub fn is_release_ready(&self) -> bool {
        self.unknowns.is_empty()
    }
}

/// Whether a catalogued image can be presented as the original did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImageReadiness {
    /// Decoded, and every presentation decision is established.
    Ready,
    /// Decoded, but presentation decisions are open; the reasons list them.
    DecodedWithUnknowns,
    /// Not decodable; the reasons say why.
    Failed,
}

/// One row of the image catalog (IDENTITY-CONTENT "required catalog
/// collections": render images).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageRecord {
    /// The texture, or `None` for an archive that failed to open.
    pub id: Option<TextureId>,
    /// Always `"image"`.
    pub kind: &'static str,
    /// The archive key the row came from.
    pub archive_key: AssetKey,
    /// The archive's origin, when it resolved.
    pub origin: Option<SourceSpan>,
    /// The archive the row depends on.
    pub dependencies: Vec<RelativePath>,
    /// Whether the archive and entry were read.
    pub parse_state: ParseState,
    /// Whether the stored level decoded.
    pub normalize_state: ParseState,
    /// The consumers this row feeds.
    pub runtime_consumers: Vec<&'static str>,
    /// Presentation readiness.
    pub readiness: ImageReadiness,
    /// Stable codes of everything that keeps the row from being ready.
    pub unsupported_reasons: Vec<String>,
    /// SHA-256 of the stored level (texels, alpha plane), or of nothing for
    /// a failed archive.
    pub fingerprint: Option<ContentHash>,
}

/// The consumer every catalogued texture row feeds.
pub const GPU_UPLOAD_CONSUMER: &str = "gpu_upload";

#[derive(Debug)]
struct ArchiveSlot {
    key: AssetKey,
    state: Result<TextureArchive, TextureArchiveError>,
}

/// The texture archives one session makes available, and the images they
/// hold.
#[derive(Debug)]
pub struct TextureCatalog {
    generation: SessionGeneration,
    slots: Vec<ArchiveSlot>,
}

impl TextureCatalog {
    /// Opens every archive key in `session`. A key that fails stays in the
    /// catalog as a failed row; it is never dropped. A repeated key is
    /// opened once.
    pub fn open(session: &ContentSession, archives: &[AssetKey]) -> Self {
        let mut slots: Vec<ArchiveSlot> = Vec::new();
        for key in archives {
            if slots.iter().any(|slot| &slot.key == key) {
                continue;
            }
            slots.push(ArchiveSlot {
                key: key.clone(),
                state: TextureArchive::open(session, key),
            });
        }
        Self {
            generation: session.generation(),
            slots,
        }
    }

    /// The session generation that read the catalog.
    pub fn generation(&self) -> SessionGeneration {
        self.generation
    }

    /// Every archive that opened.
    pub fn archives(&self) -> impl Iterator<Item = &TextureArchive> {
        self.slots
            .iter()
            .filter_map(|slot| slot.state.as_ref().ok())
    }

    /// Every archive that failed, with its error.
    pub fn failures(&self) -> impl Iterator<Item = (&AssetKey, &TextureArchiveError)> {
        self.slots
            .iter()
            .filter_map(|slot| slot.state.as_ref().err().map(|error| (&slot.key, error)))
    }

    /// Reopens every failed archive in `session`, keeping those that
    /// loaded. Returns how many are still failing.
    ///
    /// # Errors
    ///
    /// [`TextureResolveError::ForeignSession`] when `session` is not the one
    /// that read the catalog; a new session needs a new catalog.
    pub fn retry_failed(&mut self, session: &ContentSession) -> Result<usize, TextureResolveError> {
        self.require_session(session)?;
        for slot in &mut self.slots {
            if slot.state.is_err() {
                slot.state = TextureArchive::open(session, &slot.key);
            }
        }
        Ok(self.failures().count())
    }

    /// Refuses a session that did not read this catalog.
    ///
    /// # Errors
    ///
    /// [`TextureResolveError::ForeignSession`] when the generations differ.
    pub fn require_session(&self, session: &ContentSession) -> Result<(), TextureResolveError> {
        if session.generation() == self.generation {
            Ok(())
        } else {
            Err(TextureResolveError::ForeignSession {
                session: session.generation(),
                catalog: self.generation,
            })
        }
    }

    fn archive(&self, key: &AssetKey) -> Result<&TextureArchive, TextureResolveError> {
        let slot = self
            .slots
            .iter()
            .find(|slot| &slot.key == key)
            .ok_or_else(|| TextureResolveError::ArchiveNotCatalogued {
                archive: Box::new(key.clone()),
            })?;
        slot.state
            .as_ref()
            .map_err(|error| TextureResolveError::ArchiveFailed {
                archive: Box::new(key.clone()),
                code: error.code(),
                diagnostic: error.to_string(),
            })
    }

    /// Resolves `reference` to exactly one stored texture.
    ///
    /// # Errors
    ///
    /// [`TextureResolveError::ForeignSession`] for another session,
    /// [`TextureResolveError::ArchiveNotCatalogued`] or
    /// [`TextureResolveError::ArchiveFailed`] when the archive is not
    /// available, [`TextureResolveError::NotFound`] when it does not store
    /// the name and [`TextureResolveError::Duplicate`] when it stores it more
    /// than once.
    pub fn resolve(
        &self,
        session: &ContentSession,
        reference: &TextureRef,
    ) -> Result<ResolvedTexture, TextureResolveError> {
        self.require_session(session)?;
        let archive = self.archive(&reference.archive)?;
        let entries: Vec<usize> = archive
            .entries
            .iter()
            .filter(|entry| entry.id.name == reference.name)
            .map(|entry| entry.id.entry_index)
            .collect();
        let attempts = vec![
            TextureAttempt::Archive {
                archive: archive.path().clone(),
                mount: archive.container.mount().clone(),
                trace: archive.trace.clone(),
            },
            TextureAttempt::Name {
                name: reference.name.clone(),
                entries: entries.clone(),
            },
        ];
        match entries.as_slice() {
            [entry_index] => Ok(ResolvedTexture {
                generation: self.generation,
                id: archive.entries[*entry_index].id.clone(),
                archive_span: archive.span().clone(),
                attempts,
            }),
            [] => Err(TextureResolveError::NotFound {
                reference: Box::new(reference.clone()),
                attempts,
            }),
            _ => Err(TextureResolveError::Duplicate {
                reference: Box::new(reference.clone()),
                attempts,
            }),
        }
    }

    /// Resolves the texture at table position `entry_index` of `archive`,
    /// whatever its name. This is how a whole-archive consumer (the F08-D
    /// decode audit) reaches every entry, including names the archive stores
    /// more than once, which [`Self::resolve`] refuses by design.
    ///
    /// # Errors
    ///
    /// As [`Self::resolve`] for the session and the archive;
    /// [`TextureResolveError::EntryNotFound`] when the table is shorter.
    pub fn resolve_entry(
        &self,
        session: &ContentSession,
        archive: &AssetKey,
        entry_index: usize,
    ) -> Result<ResolvedTexture, TextureResolveError> {
        self.require_session(session)?;
        let texture_archive = self.archive(archive)?;
        let attempts = vec![
            TextureAttempt::Archive {
                archive: texture_archive.path().clone(),
                mount: texture_archive.container.mount().clone(),
                trace: texture_archive.trace.clone(),
            },
            TextureAttempt::Entry {
                entry_index,
                entries: texture_archive.entries.len(),
            },
        ];
        match texture_archive.entry(entry_index) {
            Some(entry) => Ok(ResolvedTexture {
                generation: self.generation,
                id: entry.id.clone(),
                archive_span: texture_archive.span().clone(),
                attempts,
            }),
            None => Err(TextureResolveError::EntryNotFound {
                archive: Box::new(archive.clone()),
                attempts,
            }),
        }
    }

    fn entry_of(
        &self,
        resolved: &ResolvedTexture,
    ) -> Result<(&TextureArchive, &TextureEntry), TextureResolveError> {
        let not_ours = || TextureResolveError::NotFromThisCatalog {
            id: Box::new(resolved.id.clone()),
        };
        if resolved.generation != self.generation {
            return Err(not_ours());
        }
        self.archives()
            .filter(|archive| archive.span() == &resolved.archive_span)
            .find_map(|archive| {
                archive
                    .entry(resolved.id.entry_index)
                    .filter(|entry| entry.id == resolved.id)
                    .map(|entry| (archive, entry))
            })
            .ok_or_else(not_ours)
    }

    /// Decodes a resolved texture's stored level.
    ///
    /// # Errors
    ///
    /// [`UploadError::Resolve`] when the texture is not from this catalog or
    /// `session` is foreign, [`UploadError::Decode`] when decoding fails.
    pub fn decode(
        &self,
        session: &ContentSession,
        resolved: &ResolvedTexture,
    ) -> Result<DecodedImage, UploadError> {
        self.require_session(session)?;
        let (archive, entry) = self.entry_of(resolved)?;
        Ok(archive.decode(entry)?)
    }

    /// Hands a resolved texture to the GPU upload boundary.
    ///
    /// # Errors
    ///
    /// As [`Self::decode`].
    pub fn prepare_upload(
        &self,
        session: &ContentSession,
        resolved: &ResolvedTexture,
    ) -> Result<TextureUpload, UploadError> {
        self.require_session(session)?;
        let (archive, entry) = self.entry_of(resolved)?;
        let image = archive.decode(entry)?;
        Ok(TextureUpload {
            id: entry.id.clone(),
            generation: self.generation,
            image,
            stretch: entry.stretch,
            unknowns: presentation_unknowns(entry),
        })
    }

    /// One row per texture of every archive that opened, and one row per
    /// archive that failed, in catalog order.
    pub fn records(&self) -> Vec<ImageRecord> {
        let mut records = Vec::new();
        for slot in &self.slots {
            let archive = match &slot.state {
                Ok(archive) => archive,
                Err(error) => {
                    records.push(ImageRecord {
                        id: None,
                        kind: "image",
                        archive_key: slot.key.clone(),
                        origin: None,
                        dependencies: Vec::new(),
                        parse_state: ParseState::Failed {
                            diagnostic: error.to_string(),
                        },
                        normalize_state: ParseState::Unparsed,
                        runtime_consumers: vec![GPU_UPLOAD_CONSUMER],
                        readiness: ImageReadiness::Failed,
                        unsupported_reasons: vec![error.code().to_owned()],
                        fingerprint: None,
                    });
                    continue;
                }
            };
            for entry in &archive.entries {
                let stored = &archive.container.bytes()[entry.stored.clone()];
                let (normalize_state, readiness, unsupported_reasons) = match archive.decode(entry)
                {
                    Ok(_) => {
                        let reasons: Vec<String> = presentation_unknowns(entry)
                            .into_iter()
                            .map(|unknown| unknown.code().to_owned())
                            .collect();
                        let readiness = if reasons.is_empty() {
                            ImageReadiness::Ready
                        } else {
                            ImageReadiness::DecodedWithUnknowns
                        };
                        (ParseState::Parsed, readiness, reasons)
                    }
                    Err(error) => (
                        ParseState::Failed {
                            diagnostic: error.to_string(),
                        },
                        ImageReadiness::Failed,
                        vec![error.code().to_owned()],
                    ),
                };
                records.push(ImageRecord {
                    id: Some(entry.id.clone()),
                    kind: "image",
                    archive_key: slot.key.clone(),
                    origin: Some(archive.span().clone()),
                    dependencies: vec![archive.path().clone()],
                    parse_state: ParseState::Parsed,
                    normalize_state,
                    runtime_consumers: vec![GPU_UPLOAD_CONSUMER],
                    readiness,
                    unsupported_reasons,
                    fingerprint: Some(sha256(stored)),
                });
            }
        }
        records
    }
}

/// Acceptance stage F08-C. Every archive is newly authored synthetic bytes
/// built here from the layout recorded in
/// `docs/findings/2026-09-28-f08-b-02-zbd-texture-package.md`, written under
/// the system temporary directory; the one retail test reads `$CS_GAME_DIR`
/// only and commits nothing from it.
#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use cs_assets::install;
    use cs_assets::vfs::{
        AttemptOutcome, ContentSession, ReadError, SessionBuilder, WORLD_NAMESPACE,
    };
    use cs_formats::texture::zbd::{
        FLAG_BYTES_PER_PIXEL2, FLAG_FULL_ALPHA, FLAG_HAS_ALPHA, FLAG_NO_ALPHA,
        ZBD_TEXTURE_HEADER_BYTES,
    };
    use cs_formats::texture::{AlphaSource, DecodedFormat, Extent, read_zbd_textures};
    use cs_types::asset_id::{AssetKey, ResolveContext, WorldGroup};

    use super::*;

    const OPAQUE: u32 = FLAG_BYTES_PER_PIXEL2 | FLAG_NO_ALPHA;
    const SIMPLE: u32 = FLAG_BYTES_PER_PIXEL2 | FLAG_HAS_ALPHA;
    const FULL: u32 = FLAG_BYTES_PER_PIXEL2 | FLAG_HAS_ALPHA | FLAG_FULL_ALPHA;

    // RGB565 words whose two bytes differ, so a byte swap is visible.
    const RED: u16 = 0xF800;
    const GREEN: u16 = 0x07E0;
    const BLUE: u16 = 0x001F;
    const YELLOW: u16 = 0xFFE0;
    const CYAN: u16 = 0x07FF;
    const MAGENTA: u16 = 0xF81F;

    /// One authored texture of a synthetic package.
    struct Tex {
        name: &'static str,
        flags: u32,
        width: u16,
        height: u16,
        words: Vec<u16>,
        indices: Vec<u8>,
        alpha: Vec<u8>,
        palette: Vec<u16>,
    }

    impl Tex {
        fn direct(name: &'static str, flags: u32, width: u16, height: u16, words: &[u16]) -> Self {
            Self {
                name,
                flags,
                width,
                height,
                words: words.to_vec(),
                indices: Vec::new(),
                alpha: Vec::new(),
                palette: Vec::new(),
            }
        }

        fn body(&self) -> Vec<u8> {
            let mut out = Vec::new();
            out.extend_from_slice(&self.flags.to_le_bytes());
            out.extend_from_slice(&self.width.to_le_bytes());
            out.extend_from_slice(&self.height.to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&(self.palette.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            for word in &self.words {
                out.extend_from_slice(&word.to_le_bytes());
            }
            out.extend_from_slice(&self.indices);
            out.extend_from_slice(&self.alpha);
            for word in &self.palette {
                out.extend_from_slice(&word.to_le_bytes());
            }
            out
        }
    }

    fn package(textures: &[Tex]) -> Vec<u8> {
        let mut out = Vec::new();
        for word in [0u32, 1, 0, textures.len() as u32, 0, 0] {
            out.extend_from_slice(&word.to_le_bytes());
        }
        let mut offset = ZBD_TEXTURE_HEADER_BYTES + textures.len() * 40;
        let bodies: Vec<Vec<u8>> = textures.iter().map(Tex::body).collect();
        for (texture, body) in textures.iter().zip(&bodies) {
            let mut name = [0u8; 32];
            name[..texture.name.len()].copy_from_slice(texture.name.as_bytes());
            out.extend_from_slice(&name);
            out.extend_from_slice(&(offset as u32).to_le_bytes());
            out.extend_from_slice(&(-1i32).to_le_bytes());
            offset += body.len();
        }
        for body in bodies {
            out.extend_from_slice(&body);
        }
        out
    }

    /// World one's `sky`: the asymmetric 3x2 image.
    const SKY_C1: [u16; 6] = [RED, GREEN, BLUE, YELLOW, CYAN, MAGENTA];
    /// World two's `sky`: the same six words in another arrangement.
    const SKY_C2: [u16; 6] = [MAGENTA, CYAN, YELLOW, BLUE, GREEN, RED];

    fn world_one_archive() -> Vec<u8> {
        package(&[
            Tex::direct("ground", OPAQUE, 1, 1, &[GREEN]),
            Tex::direct("sky", OPAQUE, 3, 2, &SKY_C1),
        ])
    }

    fn world_two_archive() -> Vec<u8> {
        package(&[
            Tex::direct("sky", OPAQUE, 3, 2, &SKY_C2),
            Tex {
                alpha: vec![0, 255],
                ..Tex::direct("smoke", FULL, 2, 1, &[BLUE, RED])
            },
            Tex {
                indices: vec![1, 0],
                palette: vec![YELLOW, CYAN],
                ..Tex::direct("sign", SIMPLE, 1, 2, &[])
            },
            Tex::direct("twin", OPAQUE, 1, 1, &[RED]),
            Tex::direct("twin", OPAQUE, 1, 1, &[BLUE]),
        ])
    }

    static NEXT_TREE: AtomicU64 = AtomicU64::new(0);

    /// A disposable fixture installation under the temporary directory.
    struct Tree(PathBuf);

    impl Tree {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "cs-f08-c-{}-{}",
                std::process::id(),
                NEXT_TREE.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).expect("fixture root is created");
            Self(root)
        }

        fn write(&self, spelling: &str, bytes: &[u8]) {
            let path = self.0.join(spelling);
            fs::create_dir_all(path.parent().expect("a parent")).expect("fixture dirs");
            fs::write(path, bytes).expect("fixture bytes are written");
        }

        fn two_worlds() -> Self {
            let tree = Self::new();
            tree.write("ZBD/c1/texture.zbd", &world_one_archive());
            tree.write("ZBD/c2/texture.zbd", &world_two_archive());
            tree
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// A session of the installation at `root` with the production mount
    /// layout, selecting the world group spelled `world`.
    fn world_session(root: &Path, world: &str) -> ContentSession {
        let found = install::discover(root).expect("installation is discovered");
        session_of(root, &found, world)
    }

    /// [`world_session`] for an installation already discovered.
    fn session_of(root: &Path, found: &install::Discovery, world: &str) -> ContentSession {
        let group = found
            .diagnosis
            .world_groups
            .iter()
            .find(|group| group.as_str().eq_ignore_ascii_case(world))
            .unwrap_or_else(|| panic!("world group {world} is discovered"))
            .clone();
        let context = ResolveContext::new(install::fingerprint(&found.manifest))
            .with_world_group(WorldGroup::from_relative(group));
        let mut builder = SessionBuilder::new(context);
        builder
            .mount_installation(root, &found.diagnosis)
            .expect("installation mounts");
        builder.open()
    }

    fn world_key(path: &str) -> AssetKey {
        AssetKey::from_spelling(WORLD_NAMESPACE, path, "default").expect("valid key")
    }

    fn texture_ref(name: &str) -> TextureRef {
        TextureRef::new(world_key("texture.zbd"), name)
    }

    fn words(upload: &TextureUpload) -> Vec<u16> {
        upload
            .rows()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect()
    }

    /// AC03: the same key and name resolve to each world's own archive and
    /// decode to that archive's texels.
    #[test]
    fn accept_f08_c_same_name_texture_in_two_chapter_archives_resolves_per_world() {
        let tree = Tree::two_worlds();
        let mut ids = Vec::new();
        for (world, expected_archive, expected_entry, expected) in [
            ("ZBD/c1", "ZBD/c1/texture.zbd", 1, SKY_C1),
            ("ZBD/c2", "ZBD/c2/texture.zbd", 0, SKY_C2),
        ] {
            let session = world_session(&tree.0, world);
            let catalog = TextureCatalog::open(&session, &[world_key("texture.zbd")]);
            assert_eq!(catalog.failures().count(), 0, "{world}: archive opens");
            let resolved = catalog
                .resolve(&session, &texture_ref("sky"))
                .expect("sky resolves");
            assert_eq!(resolved.id().archive.as_str(), expected_archive);
            assert_eq!(resolved.id().entry_index, expected_entry);
            assert_eq!(resolved.id().name, "sky");
            assert_eq!(resolved.archive_span().member_key(), Some("texture.zbd"));
            assert_eq!(resolved.generation(), session.generation());

            // Ordered attempts: the archive chosen by the VFS for this world
            // (the other world's mount skipped), then the name lookup.
            match resolved.attempts() {
                [
                    TextureAttempt::Archive { archive, trace, .. },
                    TextureAttempt::Name { name, entries },
                ] => {
                    assert_eq!(archive.as_str(), expected_archive);
                    assert!(
                        trace
                            .attempts
                            .iter()
                            .any(|attempt| matches!(attempt.outcome, AttemptOutcome::Selected))
                    );
                    assert!(
                        trace
                            .attempts
                            .iter()
                            .any(|attempt| matches!(attempt.outcome, AttemptOutcome::Skipped(_))),
                        "the other world's archive is skipped, not searched: {trace}"
                    );
                    assert_eq!(name, "sky");
                    assert_eq!(entries, &vec![expected_entry]);
                }
                other => panic!("unexpected attempts {other:?}"),
            }

            let upload = catalog
                .prepare_upload(&session, &resolved)
                .expect("sky uploads");
            assert_eq!(upload.id(), resolved.id());
            assert_eq!(
                upload.extent(),
                Extent {
                    width: 3,
                    height: 2
                }
            );
            assert_eq!(upload.format(), DecodedFormat::Rgb565);
            assert_eq!(upload.row_bytes(), 6);
            // Top row first, stored 565 words unchanged (no expansion).
            assert_eq!(words(&upload), expected);
            ids.push(resolved.id().clone());
        }
        assert_ne!(ids[0], ids[1], "same-name textures keep distinct ids");
    }

    /// No fallback search: a name only another world stores is not found,
    /// and the failure names the one archive that was searched. A name the
    /// archive stores twice is refused with both entries.
    #[test]
    fn accept_f08_c_missing_or_duplicate_name_fails_visibly_without_fallback() {
        let tree = Tree::two_worlds();
        let session = world_session(&tree.0, "ZBD/c1");
        let catalog = TextureCatalog::open(&session, &[world_key("texture.zbd")]);
        let error = catalog
            .resolve(&session, &texture_ref("smoke"))
            .expect_err("world one stores no smoke");
        assert_eq!(error.code(), "texture_not_found");
        match &error {
            TextureResolveError::NotFound { attempts, .. } => {
                assert!(matches!(
                    &attempts[0],
                    TextureAttempt::Archive { archive, .. } if archive.as_str() == "ZBD/c1/texture.zbd"
                ));
                assert!(matches!(
                    &attempts[1],
                    TextureAttempt::Name { entries, .. } if entries.is_empty()
                ));
            }
            other => panic!("unexpected {other:?}"),
        }
        // Stored names are compared exactly.
        assert_eq!(
            catalog
                .resolve(&session, &texture_ref("SKY"))
                .expect_err("no case folding")
                .code(),
            "texture_not_found"
        );

        let session = world_session(&tree.0, "ZBD/c2");
        let catalog = TextureCatalog::open(&session, &[world_key("texture.zbd")]);
        match catalog.resolve(&session, &texture_ref("twin")) {
            Err(TextureResolveError::Duplicate { attempts, .. }) => assert!(matches!(
                &attempts[1],
                TextureAttempt::Name { entries, .. } if entries == &vec![3, 4]
            )),
            other => panic!("expected a duplicate, got {other:?}"),
        }
        assert_eq!(
            catalog
                .resolve(
                    &session,
                    &TextureRef::new(world_key("rtexture2.zbd"), "sky")
                )
                .expect_err("not catalogued")
                .code(),
            "archive_not_catalogued"
        );
    }

    /// The upload boundary carries values unchanged and lists every
    /// presentation decision still open.
    #[test]
    fn accept_f08_c_upload_keeps_alpha_and_indices_and_lists_presentation_unknowns() {
        let tree = Tree::two_worlds();
        let session = world_session(&tree.0, "ZBD/c2");
        let catalog = TextureCatalog::open(&session, &[world_key("texture.zbd")]);

        let smoke = catalog
            .resolve(&session, &texture_ref("smoke"))
            .expect("smoke resolves");
        let upload = catalog.prepare_upload(&session, &smoke).expect("uploads");
        assert_eq!(words(&upload), vec![BLUE, RED]);
        assert_eq!(upload.alpha_plane(), Some(&[0u8, 255][..]));
        assert_eq!(upload.image().alpha_source(), AlphaSource::Plane);
        assert_eq!(
            upload.unknowns(),
            &[
                PresentationUnknown::ColorSpace,
                PresentationUnknown::AlphaTest,
                PresentationUnknown::Rgb565Expansion,
                PresentationUnknown::Stretch,
            ]
        );
        assert!(!upload.is_release_ready());

        let sign = catalog
            .resolve(&session, &texture_ref("sign"))
            .expect("sign resolves");
        let upload = catalog.prepare_upload(&session, &sign).expect("uploads");
        // Palette lookup of indices [1, 0]; the indices stay available so a
        // palette key is applied at presentation, not baked in.
        assert_eq!(words(&upload), vec![CYAN, YELLOW]);
        assert_eq!(upload.indices(), Some(&[1u8, 0][..]));
        assert_eq!(upload.row_bytes(), 2);
        assert!(
            upload
                .unknowns()
                .contains(&PresentationUnknown::AlphaSource)
        );
        assert!(
            upload
                .unknowns()
                .contains(&PresentationUnknown::Rgb565Expansion)
        );
    }

    /// Stale state: a catalog serves only the session that read it, a
    /// resolution only the catalog that made it, and uploads outlive the
    /// session because they own their values.
    #[test]
    fn accept_f08_c_world_switch_refuses_stale_catalog_and_resolution() {
        let tree = Tree::two_worlds();
        let first = world_session(&tree.0, "ZBD/c1");
        let first_catalog = TextureCatalog::open(&first, &[world_key("texture.zbd")]);
        let sky = first_catalog
            .resolve(&first, &texture_ref("sky"))
            .expect("sky resolves");

        let second = world_session(&tree.0, "ZBD/c2");
        let second_catalog = TextureCatalog::open(&second, &[world_key("texture.zbd")]);
        assert_eq!(
            first_catalog
                .resolve(&second, &texture_ref("sky"))
                .expect_err("foreign session")
                .code(),
            "foreign_session"
        );
        assert_eq!(
            first_catalog
                .prepare_upload(&second, &sky)
                .expect_err("foreign session")
                .code(),
            "foreign_session"
        );
        // World one's resolution handed to world two's catalog.
        assert_eq!(
            second_catalog
                .prepare_upload(&second, &sky)
                .expect_err("not from this catalog")
                .code(),
            "not_from_this_catalog"
        );

        let upload = first_catalog
            .prepare_upload(&first, &sky)
            .expect("same session uploads");
        let _teardown = first.close();
        assert_eq!(words(&upload), SKY_C1, "an upload owns its values");
    }

    /// Failed archives stay catalog rows with their code; every texture of
    /// a good archive is a row with origin, dependency, consumer and
    /// fingerprint; retry is refused for another session and a remount
    /// after the file is repaired loads it.
    #[test]
    fn accept_f08_c_failed_archive_is_a_catalog_row_and_recovers_after_remount() {
        let tree = Tree::two_worlds();
        let mut broken = world_one_archive();
        broken.push(0); // one trailing byte
        tree.write("ZBD/c1/texture.zbd", &broken);
        tree.write("ZBD/c1/rtexture2.zbd", b"not a texture package");

        let session = world_session(&tree.0, "ZBD/c1");
        let keys = [
            world_key("texture.zbd"),
            world_key("missing.zbd"),
            world_key("rtexture2.zbd"),
            world_key("texture.zbd"),
        ];
        let mut catalog = TextureCatalog::open(&session, &keys);
        let failures: Vec<(String, &str)> = catalog
            .failures()
            .map(|(key, error)| (key.to_string(), error.code()))
            .collect();
        assert_eq!(failures.len(), 3, "{failures:?}");
        assert_eq!(failures[0].1, "trailing_bytes");
        assert_eq!(failures[1].1, "resolve");
        let records = catalog.records();
        assert_eq!(records.len(), 3, "one row per failed archive");
        for record in &records {
            assert_eq!(record.id, None);
            assert_eq!(record.readiness, ImageReadiness::Failed);
            assert!(matches!(record.parse_state, ParseState::Failed { .. }));
            assert_eq!(record.unsupported_reasons.len(), 1);
        }
        assert_eq!(
            catalog
                .resolve(&session, &texture_ref("sky"))
                .expect_err("failed archive")
                .code(),
            "archive_failed"
        );
        assert_eq!(catalog.retry_failed(&session), Ok(3), "still failing");

        let other = world_session(&tree.0, "ZBD/c2");
        assert_eq!(
            catalog.retry_failed(&other).expect_err("foreign").code(),
            "foreign_session"
        );

        // Repairing the file does not change what this session mounted: the
        // retry fails on the mount-time digest instead of reading new bytes.
        tree.write("ZBD/c1/texture.zbd", &world_one_archive());
        assert_eq!(catalog.retry_failed(&session), Ok(3));
        assert!(matches!(
            catalog.failures().next(),
            Some((
                _,
                TextureArchiveError::Container(ZbdError::Read(
                    ReadError::ChangedOnDisk { .. } | ReadError::DigestMismatch { .. }
                ))
            ))
        ));

        let session = world_session(&tree.0, "ZBD/c1");
        let catalog = TextureCatalog::open(&session, &[world_key("texture.zbd")]);
        assert_eq!(catalog.failures().count(), 0);
        let records = catalog.records();
        assert_eq!(records.len(), 2, "one row per texture");
        let sky = &records[1];
        let id = sky.id.as_ref().expect("a texture row");
        assert_eq!((id.entry_index, id.name.as_str()), (1, "sky"));
        assert_eq!(sky.kind, "image");
        assert_eq!(sky.parse_state, ParseState::Parsed);
        assert_eq!(sky.normalize_state, ParseState::Parsed);
        assert_eq!(sky.readiness, ImageReadiness::DecodedWithUnknowns);
        assert_eq!(sky.runtime_consumers, vec![GPU_UPLOAD_CONSUMER]);
        assert_eq!(sky.dependencies[0].as_str(), "ZBD/c1/texture.zbd");
        assert!(
            sky.unsupported_reasons
                .contains(&"color_space_unknown".to_owned())
        );
        let stored: Vec<u8> = SKY_C1.iter().flat_map(|word| word.to_le_bytes()).collect();
        assert_eq!(sky.fingerprint, Some(sha256(&stored)));
    }

    /// A container the dispatch routes to another family is not read as
    /// textures, even when its bytes would parse as a texture package.
    #[test]
    fn accept_f08_c_container_of_another_family_is_refused() {
        let tree = Tree::two_worlds();
        tree.write("ZBD/c1/zrdr.zbd", &world_one_archive());
        let session = world_session(&tree.0, "ZBD/c1");
        let catalog = TextureCatalog::open(&session, &[world_key("zrdr.zbd")]);
        let (_, error) = catalog.failures().next().expect("refused");
        assert!(
            matches!(
                error,
                TextureArchiveError::WrongFamily {
                    family: ZbdFamily::Reader,
                    ..
                }
            ),
            "{error}"
        );
        assert_eq!(error.code(), "wrong_family");
    }

    /// Retail AC03: textures stored under the same name in the `C1` and
    /// `C2` world archives resolve to their own world's archive and decode
    /// to exactly what the reader finds at that entry of that file.
    #[test]
    #[ignore = "requires CS_GAME_DIR"]
    fn accept_f08_c_retail_same_name_textures_resolve_per_world() {
        let dir = std::env::var_os("CS_GAME_DIR")
            .expect("CS_GAME_DIR must point at the original installation for this test");
        let root = PathBuf::from(dir);
        assert!(
            root.is_dir(),
            "CS_GAME_DIR {} is not a directory",
            root.display()
        );

        let worlds = ["ZBD/C1", "ZBD/C2"];
        let found = install::discover(&root).expect("installation is discovered");
        let sessions: Vec<ContentSession> = worlds
            .iter()
            .map(|world| session_of(&root, &found, world))
            .collect();
        let catalogs: Vec<TextureCatalog> = sessions
            .iter()
            .map(|session| TextureCatalog::open(session, &[world_key("texture.zbd")]))
            .collect();
        for catalog in &catalogs {
            assert_eq!(catalog.failures().count(), 0);
        }
        let unique_names = |catalog: &TextureCatalog| {
            let archive = catalog.archives().next().expect("one archive");
            let mut counts = std::collections::BTreeMap::<String, usize>::new();
            for id in archive.ids() {
                *counts.entry(id.name.clone()).or_default() += 1;
            }
            counts
                .into_iter()
                .filter(|(_, count)| *count == 1)
                .map(|(name, _)| name)
                .collect::<std::collections::BTreeSet<_>>()
        };
        let shared: Vec<String> = unique_names(&catalogs[0])
            .intersection(&unique_names(&catalogs[1]))
            .cloned()
            .collect();
        assert!(!shared.is_empty(), "C1 and C2 share texture names");

        let mut differing = 0usize;
        for name in &shared {
            let mut stored = Vec::new();
            for ((world, session), catalog) in worlds.iter().zip(&sessions).zip(&catalogs) {
                let resolved = catalog
                    .resolve(session, &texture_ref(name))
                    .expect("resolves");
                let archive = format!("{world}/texture.zbd");
                assert!(
                    resolved
                        .id()
                        .archive
                        .as_str()
                        .eq_ignore_ascii_case(&archive),
                    "{name} in {world} came from {}",
                    resolved.id()
                );
                let upload = catalog.prepare_upload(session, &resolved).expect("uploads");

                // Independent path: the file read directly from the host.
                let bytes = fs::read(root.join(&archive)).expect("archive readable");
                let mut budget = AllocationBudget::with_defaults(archive.as_str());
                let package = read_zbd_textures(&archive, &bytes, &mut budget).expect("reads");
                let direct = &package.textures()[resolved.id().entry_index];
                assert_eq!(direct.name(), name);
                let decoded = direct
                    .decode(&mut AllocationBudget::with_defaults(archive.as_str()))
                    .expect("decodes");
                assert_eq!(upload.image(), &decoded, "{name} in {world}");
                stored.push(direct.stored().to_vec());
            }
            if stored[0] != stored[1] {
                differing += 1;
            }
        }
        assert!(
            differing > 0,
            "at least one shared name stores different bytes per world"
        );
        eprintln!(
            "F08-C retail: {} names stored once in both C1 and C2 texture.zbd, {differing} with different bytes",
            shared.len()
        );
    }

    /// F08-D: a whole-archive consumer reaches every entry by position,
    /// including a name stored twice, and each entry keeps its own texels;
    /// a position past the table and a foreign session are refused.
    #[test]
    fn accept_f08_d_resolve_entry_reaches_duplicate_names_by_position() {
        let tree = Tree::two_worlds();
        let session = world_session(&tree.0, "ZBD/c2");
        let key = world_key("texture.zbd");
        let catalog = TextureCatalog::open(&session, std::slice::from_ref(&key));

        let mut twins = Vec::new();
        for entry_index in [3, 4] {
            let resolved = catalog
                .resolve_entry(&session, &key, entry_index)
                .expect("the entry exists");
            assert_eq!(resolved.id().entry_index, entry_index);
            assert_eq!(resolved.id().name, "twin");
            assert!(matches!(
                &resolved.attempts()[1],
                TextureAttempt::Entry { entry_index: asked, entries: 5 } if *asked == entry_index
            ));
            let upload = catalog
                .prepare_upload(&session, &resolved)
                .expect("the entry decodes");
            twins.push((resolved.id().clone(), words(&upload)));
        }
        assert_ne!(twins[0].0, twins[1].0, "two entries, two ids");
        assert_eq!(twins[0].1, vec![RED]);
        assert_eq!(twins[1].1, vec![BLUE]);

        let by_position = catalog
            .resolve_entry(&session, &key, 0)
            .expect("entry 0 exists");
        let by_name = catalog
            .resolve(&session, &texture_ref("sky"))
            .expect("sky is stored once");
        assert_eq!(by_position.id(), by_name.id());

        match catalog.resolve_entry(&session, &key, 5) {
            Err(error @ TextureResolveError::EntryNotFound { .. }) => {
                assert_eq!(error.code(), "texture_entry_not_found");
                assert!(error.to_string().contains("entry 5 of 5"), "{error}");
            }
            other => panic!("expected a missing entry, got {other:?}"),
        }
        let other = world_session(&tree.0, "ZBD/c2");
        assert_eq!(
            catalog
                .resolve_entry(&other, &key, 0)
                .expect_err("foreign session")
                .code(),
            "foreign_session"
        );
    }
}
